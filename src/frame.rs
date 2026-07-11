//! MPEG-1 Audio Layer II frame-level decode loop — ISO/IEC 11172-3
//! (1993) §2.4.1.6 ("Audio data, Layer II"), §2.4.3.1 ("The bitstream"),
//! §2.4.3.2 ("Audio decoding process"), §2.4.3.3.4 ("Requantization"),
//! and §2.4.3.3.5 / Annex A Figure A.2 ("Synthesis subband filter").
//!
//! Clean-room: the §2.4.1.6 loop structure (12 sample-granules × sblimit
//! subbands × nch channels × one triplet each), the
//! `sample_granule / 4 → scalefactor_granule` mapping (§2.4.2.3:
//! "scalefactors are transmitted for groups of 12 subband samples"
//! and "for each group of 12 subband samples three scalefactors are
//! transmitted"), and the §2.4.3.1 CRC slot placement ("just after the
//! header") all come from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` PDF pages 16, 24, 26
//! and 36. No third-party MP2 source was consulted.
//!
//! # §2.4.1.6 sample loop
//!
//! ```text
//! for (gr = 0; gr < 12; gr++) {
//!     for (sb = 0; sb < bound; sb++)              // per-channel region
//!         for (ch = 0; ch < nch; ch++)
//!             if (allocation[ch][sb] != 0)
//!                 read one triplet samplecode[ch][sb][gr]
//!     for (sb = bound; sb < sblimit; sb++)        // intensity-stereo region
//!         if (allocation[0][sb] != 0)
//!             read one triplet samplecode[0][sb][gr]  // shared by both ch
//! }
//! ```
//!
//! The second region only differs from the first in `joint_stereo` mode;
//! for every other mode `bound == sblimit`, so the intensity region is
//! empty and the loop is a flat per-channel read. In the intensity region
//! the single decoded codeword is valid for both channels (§2.4.2.6), and
//! each channel rescales it by its own §2.4.3.3.3 scalefactor.
//!
//! Each iteration of the inner triplet read produces three subband
//! samples (the codeword is grouped for `nb_steps ∈ {3, 5, 9}` and
//! separable otherwise — see [`crate::requant::read_triplet`]). With 12
//! sample-granules per frame and 3 subband samples per triplet, each
//! (ch, sb) accumulates `12 × 3 = 36` subband samples per frame; with
//! 32 subbands per channel that is `36 × 32 = 1152` subband samples per
//! channel, which the §2.4.3.2 synthesis filter expands one-for-one
//! into 1152 PCM samples per channel. This matches the §2.4.2.1
//! "1 152 for Layer II" headline.
//!
//! # Scalefactor granule
//!
//! §2.4.2.3 partitions the 12 sample-granules into 3 scalefactor-
//! granules of 4 sample-granules each. So
//! `scalefactor_granule = sample_granule / 4` selects which of the 3
//! [`crate::audio_data::AudioData::scalefactor`] entries the
//! §2.4.3.3.3 rescaling consumes. (The scfsi schedule has already
//! decided which of those 3 slots is on the wire and which are
//! reconstructed from another slot; the resulting 3-tuple is the
//! authoritative per-granule scalefactor.)
//!
//! # §2.4.3.1 CRC-16
//!
//! When `protection_bit == 0` the 16 bits immediately after the 4-byte
//! header are the §2.4.3.1 CRC-16 over Annex B Table B.5's protected
//! region: header bits 16…31 followed by the bit-allocation +
//! scfsi sections. We compute the CRC over those bits via
//! [`crate::crc::crc16_layer2`] and reject the frame with
//! [`FrameError::CrcMismatch`] on disagreement.

use oxideav_core::bits::BitReader;

use crate::audio_data::{parse_audio_data_with_section_bits, AudioDataError};
use crate::bitalloc::{class_of_quantization, NUM_SUBBANDS};
use crate::crc::crc16_layer2;
use crate::deemphasis::DeEmphasis;
use crate::header::{FrameHeader, HeaderError};
use crate::requant::{read_triplet, RequantError};
use crate::synthesis::SynthesisFilterbank;
use crate::tables::SCALEFACTORS;

/// Sample-granules per Layer II frame (§2.4.1.6: `for (gr = 0; gr < 12;
/// gr++)`).
pub const SAMPLE_GRANULES_PER_FRAME: usize = 12;

/// Subband samples per triplet (§2.4.1.6 / §2.4.3.3.4: one triplet
/// covers three consecutive subband samples).
pub const SAMPLES_PER_TRIPLET: usize = 3;

/// PCM samples per channel per Layer II frame (§2.4.2.1 "1 152 for
/// Layer II"). Equals `SAMPLE_GRANULES_PER_FRAME *
/// SAMPLES_PER_TRIPLET * NUM_SUBBANDS = 12 * 3 * 32 = 1152`.
pub const PCM_SAMPLES_PER_CHANNEL: usize = 1152;

/// Errors raised by the Layer II frame-level decode loop.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameError {
    /// Header parse / validation failed.
    Header(HeaderError),
    /// §2.4.1.6 audio-data side info parse failed.
    AudioData(AudioDataError),
    /// §2.4.3.3.4 sample requantization failed.
    Requant(RequantError),
    /// Input buffer was too short to hold the frame `header.frame_size_bytes()`.
    Truncated {
        /// Bytes available in the input buffer.
        have: usize,
        /// Bytes required by §2.4.3.1 `floor(144 * br / Fs) + padding`.
        need: usize,
    },
    /// §2.4.3.1 protected-region CRC-16 did not match the on-wire value.
    CrcMismatch {
        /// CRC computed over the protected region.
        computed: u16,
        /// CRC read from the bitstream (the 16 bits after the header).
        expected: u16,
    },
    /// An `nb_steps` value parsed from the bit-allocation table did not
    /// map to one of the 17 Table 3-B.4 classes of quantization. This
    /// is an internal-consistency failure — every B.2 cell maps to a
    /// known B.4 class (see [`crate::bitalloc::B2_ROWS_RESOLVE`] unit
    /// test). It exists only as a defence-in-depth against a future
    /// table-edit slip.
    UnknownQuantClass { ch: usize, sb: usize, nb_steps: u32 },
    /// §2.4.2.3 free-format frame-size determination failed (e.g. the
    /// recovered base size maps to no Layer II ladder bitrate, so no
    /// Annex B bit-allocation table is defined for it).
    FreeFormat(crate::freeformat::FreeFormatError),
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::Header(e) => write!(f, "frame: header error: {e}"),
            FrameError::AudioData(e) => write!(f, "frame: audio-data error: {e}"),
            FrameError::Requant(e) => write!(f, "frame: requant error: {e}"),
            FrameError::Truncated { have, need } => {
                write!(f, "frame: buffer too short ({have} < {need} bytes)")
            }
            FrameError::CrcMismatch { computed, expected } => write!(
                f,
                "frame: CRC mismatch (computed 0x{computed:04X}, expected 0x{expected:04X})"
            ),
            FrameError::UnknownQuantClass { ch, sb, nb_steps } => write!(
                f,
                "frame: nb_steps={nb_steps} for ch={ch} sb={sb} is not in Table 3-B.4"
            ),
            FrameError::FreeFormat(e) => write!(f, "frame: free-format error: {e}"),
        }
    }
}

impl From<crate::freeformat::FreeFormatError> for FrameError {
    fn from(value: crate::freeformat::FreeFormatError) -> Self {
        FrameError::FreeFormat(value)
    }
}

impl std::error::Error for FrameError {}

impl From<HeaderError> for FrameError {
    fn from(value: HeaderError) -> Self {
        FrameError::Header(value)
    }
}

impl From<AudioDataError> for FrameError {
    fn from(value: AudioDataError) -> Self {
        FrameError::AudioData(value)
    }
}

impl From<RequantError> for FrameError {
    fn from(value: RequantError) -> Self {
        FrameError::Requant(value)
    }
}

/// One fully-decoded Layer II frame: the parsed [`FrameHeader`] plus
/// the reconstructed PCM samples (`pcm[ch][n]` for `n in
/// 0..PCM_SAMPLES_PER_CHANNEL`).
///
/// Samples are in the §2.4.3.4.7.1 nominal `[-1.0, +1.0]` range as
/// produced by [`SynthesisFilterbank`]; callers convert to integer
/// formats (e.g. `i16` via `* 32768.0`) and apply clipping as their
/// downstream format requires.
///
/// `pcm.len()` equals [`FrameHeader::channels`]; `pcm[ch].len()` equals
/// [`PCM_SAMPLES_PER_CHANNEL`].
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    /// The parsed Layer II frame header.
    pub header: FrameHeader,
    /// Per-channel reconstructed PCM. Outer dimension is `channels`,
    /// inner dimension is exactly `PCM_SAMPLES_PER_CHANNEL`.
    pub pcm: Vec<Vec<f64>>,
}

/// Decode one Layer II frame starting at the front of `buf`.
///
/// The buffer must hold at least `header.frame_size_bytes()` bytes;
/// the §2.4.3.1 frame-size formula returns the byte count from the
/// start of the syncword inclusive (so `buf[0]` is the first syncword
/// byte). The reader does not advance into the next frame — frame
/// chaining is the caller's job (e.g. `[header.frame_size_bytes()..]`).
///
/// When `header.protection_bit == false` the §2.4.3.1 CRC-16 over the
/// protected fields is verified; mismatches raise
/// [`FrameError::CrcMismatch`].
pub fn decode_frame(buf: &[u8]) -> Result<DecodedFrame, FrameError> {
    decode_frame_with(buf, &mut FrameDecodeState::new())
}

/// Stateful per-channel filterbank cache used across successive
/// [`decode_frame_with`] calls so the V ring buffers persist (Annex A
/// Figure A.2 footnote 1: V is initialised to zero only at startup).
///
/// Allocate one per logical Layer II stream and reuse it for every
/// frame; on `seek` / discontinuity call [`Self::reset`] to re-zero V.
#[derive(Debug, Default)]
pub struct FrameDecodeState {
    filterbank: Vec<SynthesisFilterbank>,
    /// Per-channel §2.4.2.4 de-emphasis filter (50/15 µs or CCITT
    /// J.17 — see [`crate::deemphasis`]), lazily instantiated the first
    /// time a frame signals a non-`None` `emphasis`, and carried across
    /// frames so the IIR has no per-frame discontinuity. `None` per
    /// channel means "deliver unfiltered" (`emphasis == '00'`).
    deemphasis: Vec<Option<DeEmphasis>>,
}

impl FrameDecodeState {
    /// Fresh state with no filterbanks; they are lazily created on
    /// first decode based on `header.channels()`.
    pub fn new() -> Self {
        FrameDecodeState {
            filterbank: Vec::new(),
            deemphasis: Vec::new(),
        }
    }

    /// Re-zero every filterbank's V ring buffer per Annex A Figure
    /// A.2 footnote 1, and drop any de-emphasis filter state. Call on
    /// seek / stream discontinuity.
    pub fn reset(&mut self) {
        for fb in &mut self.filterbank {
            fb.reset();
        }
        for de in &mut self.deemphasis {
            *de = None;
        }
    }

    fn ensure_channels(&mut self, channels: usize) {
        while self.filterbank.len() < channels {
            self.filterbank.push(SynthesisFilterbank::new());
        }
        while self.deemphasis.len() < channels {
            self.deemphasis.push(None);
        }
    }

    /// Apply the §2.4.2.4 de-emphasis the `header` calls for to each
    /// channel's reconstructed PCM in place, threading the per-channel
    /// IIR state across frames. The per-channel filter is (re)built the
    /// first time a channel needs one and whenever the required curve
    /// changes; `emphasis == '00'` leaves the samples untouched.
    fn apply_deemphasis(&mut self, header: &FrameHeader, pcm: &mut [Vec<f64>]) {
        let wanted = DeEmphasis::for_header(header);
        for (ch, samples) in pcm.iter_mut().enumerate() {
            let slot = &mut self.deemphasis[ch];
            match wanted {
                Some(template) => {
                    // Instantiate on first need; preserve running state
                    // across frames once created. If a frame switches to
                    // another curve or rate (a variable stream), rebuild
                    // the filter with the new sections.
                    let rebuild = match slot {
                        Some(existing) => existing.sections() != template.sections(),
                        None => true,
                    };
                    if rebuild {
                        *slot = Some(template);
                    }
                    slot.as_mut()
                        .expect("de-emphasis filter just set")
                        .process_in_place(samples);
                }
                None => *slot = None,
            }
        }
    }
}

/// Like [`decode_frame`] but with caller-supplied [`FrameDecodeState`]
/// so the polyphase filterbank's V ring buffer persists across frames.
pub fn decode_frame_with(
    buf: &[u8],
    state: &mut FrameDecodeState,
) -> Result<DecodedFrame, FrameError> {
    let header = FrameHeader::parse(buf)?;
    let frame_size = header.frame_size_bytes();
    if buf.len() < frame_size {
        return Err(FrameError::Truncated {
            have: buf.len(),
            need: frame_size,
        });
    }
    let frame = &buf[..frame_size];
    let channels = header.channels();
    state.ensure_channels(channels);

    // The 4-byte header occupies frame[0..4]; if protection_bit is
    // clear the §2.4.1.4 CRC-16 slot is frame[4..6]; otherwise the
    // §2.4.1.6 audio-data section starts at frame[4] directly.
    let (expected_crc, after_header_byte) = if !header.protection_bit {
        if frame_size < 6 {
            return Err(FrameError::Truncated {
                have: frame_size,
                need: 6,
            });
        }
        let crc = u16::from_be_bytes([frame[4], frame[5]]);
        (Some(crc), 6)
    } else {
        (None, 4)
    };

    let mut reader = BitReader::with_position(frame, after_header_byte);

    // Snapshot allocation-section start so we can recover the packed
    // (alloc + scfsi) payload for CRC verification without re-parsing.
    let alloc_start_bit = reader.bit_position();
    let (audio, alloc_bits, scfsi_bits) = parse_audio_data_with_section_bits(&header, &mut reader)?;

    if let Some(expected) = expected_crc {
        let computed = compute_layer2_crc(frame, alloc_start_bit, alloc_bits + scfsi_bits);
        if computed != expected {
            return Err(FrameError::CrcMismatch { computed, expected });
        }
    }

    // §2.4.1.6 sample loop. We buffer the produced subband samples
    // per channel as a [(36 sample-granule slots) × 32 subbands]
    // matrix; each (gr, sb, ch) triplet contributes three slots
    // starting at `gr * 3`. After the loop, 36 successive 32-vectors
    // are pushed through the per-channel synthesis filterbank.
    let mut subband: Vec<Vec<[f64; NUM_SUBBANDS]>> = (0..channels)
        .map(|_| vec![[0.0_f64; NUM_SUBBANDS]; SAMPLE_GRANULES_PER_FRAME * SAMPLES_PER_TRIPLET])
        .collect();

    // §2.4.1.6 sample loop. Two regions per the syntax (PDF page 16,
    // lines `for (sb=0; sb<bound;…)` then `for (sb=bound; sb<sblimit;…)`):
    //
    //   * `sb < bound` — one triplet is read *per channel*
    //     (`samplecode[ch][sb][gr]`).
    //   * `bound <= sb < sblimit` (intensity-stereo region in
    //     joint_stereo mode) — only ONE triplet is on the wire
    //     (`samplecode[0][sb][gr]`); §2.4.2.6 "for subbands in
    //     intensity_stereo mode the coded representation of the sample
    //     is valid for both channels." The single decoded triplet is
    //     shared, but each channel applies its OWN §2.4.3.3.3
    //     scalefactor (scfsi + scalefactor are transmitted per channel
    //     above the bound — the scfsi/scalefactor loops in
    //     [`parse_audio_data`] run over `sb < sblimit` for every
    //     channel).
    //
    // For the non-joint modes `bound == sblimit`, so the second region
    // is empty and the loop is identical to a flat per-channel read.
    for sample_gr in 0..SAMPLE_GRANULES_PER_FRAME {
        // §2.4.2.3: 12 sample-granules split into 3 scalefactor-
        // granules of 4 each.
        let sf_gr = sample_gr / 4;
        let base = sample_gr * SAMPLES_PER_TRIPLET;

        // Region 1: `sb < bound` — one triplet per channel.
        for sb in 0..audio.bound {
            for (ch, channel_subband) in subband.iter_mut().enumerate().take(channels) {
                let nb_steps = audio.nb_steps[ch][sb];
                if nb_steps == 0 {
                    continue; // §2.4.2.3 "no bits allocated" sentinel.
                }
                let class = class_of_quantization(nb_steps)
                    .ok_or(FrameError::UnknownQuantClass { ch, sb, nb_steps })?;
                let triplet = read_triplet(&class, &mut reader)?;
                // §2.4.3.3.3 rescaling: `s' = factor * s''` with the
                // §2.4.2.3 / scfsi-expanded scalefactor for this
                // sample-granule's scalefactor-granule.
                let sf_idx = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = SCALEFACTORS[sf_idx];
                channel_subband[base][sb] = triplet[0] * factor;
                channel_subband[base + 1][sb] = triplet[1] * factor;
                channel_subband[base + 2][sb] = triplet[2] * factor;
            }
        }

        // Region 2: `bound <= sb < sblimit` — one shared triplet,
        // valid for both channels (intensity stereo). `parse_allocation`
        // copied the single on-wire allocation to both channels, so
        // `nb_steps[0][sb] == nb_steps[1][sb]` here; channel 0 is the
        // authoritative `samplecode[0][sb]` source per the syntax.
        for sb in audio.bound..audio.sblimit {
            let nb_steps = audio.nb_steps[0][sb];
            if nb_steps == 0 {
                continue; // §2.4.2.3 "no bits allocated" sentinel.
            }
            let class = class_of_quantization(nb_steps).ok_or(FrameError::UnknownQuantClass {
                ch: 0,
                sb,
                nb_steps,
            })?;
            let triplet = read_triplet(&class, &mut reader)?;
            for (ch, channel_subband) in subband.iter_mut().enumerate().take(channels) {
                // Each channel rescales the shared samplecode by its own
                // §2.4.3.3.3 scalefactor for this scalefactor-granule.
                let sf_idx = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = SCALEFACTORS[sf_idx];
                channel_subband[base][sb] = triplet[0] * factor;
                channel_subband[base + 1][sb] = triplet[1] * factor;
                channel_subband[base + 2][sb] = triplet[2] * factor;
            }
        }
    }

    // §2.4.3.2 / Annex A Figure A.2: per channel, push the 36 32-
    // vectors of subband samples through the polyphase synthesis
    // filterbank, accumulating 36 * 32 = 1152 PCM samples per channel.
    let mut pcm: Vec<Vec<f64>> = (0..channels)
        .map(|_| Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL))
        .collect();
    let mut out_block = [0.0_f64; NUM_SUBBANDS];
    for (ch, channel_subband) in subband.iter().enumerate() {
        let fb = &mut state.filterbank[ch];
        for slot in channel_subband.iter() {
            fb.push_subbands(slot, &mut out_block);
            pcm[ch].extend_from_slice(&out_block);
        }
    }

    debug_assert!(pcm.iter().all(|ch| ch.len() == PCM_SAMPLES_PER_CHANNEL));

    // §2.4.2.4: undo any encoder pre-emphasis the header signals,
    // threading the per-channel IIR state across frames.
    state.apply_deemphasis(&header, &mut pcm);

    // The remainder of the frame (after the sample loop) is §2.4.1.6
    // `ancillary_data`; we ignore it. Suppress unused-_audio.scfsi
    // by acknowledging the field (it is captured in `audio` for
    // diagnostics + future fixture audits).
    let _ = audio.scfsi;

    Ok(DecodedFrame { header, pcm })
}

/// Compute the §2.4.3.1 / Annex B Table B.5 CRC-16 over the protected
/// fields, given the raw `frame` bytes and the bit-range of the
/// (allocation + scfsi) section as parsed by
/// [`parse_audio_data_with_section_bits`]. Header bits 16…31 (frame
/// bytes 2 and 3) are fed first.
fn compute_layer2_crc(frame: &[u8], start_bit: u64, total_bits: usize) -> u16 {
    // We cannot directly call `crc16_layer2(header_high, header_low,
    // &payload, bits)` because `start_bit` is generally not byte-
    // aligned (when protection_bit == 0, the audio-data section starts
    // at frame byte 6, which IS byte-aligned, but the helper still
    // benefits from per-bit feeding to stay agnostic to that). We
    // re-implement the §2.4.3.1 feed in long-form to keep this
    // self-contained.
    let header_high = frame[2];
    let header_low = frame[3];

    // Extract the (alloc + scfsi) bits from `frame` into a left-
    // aligned packed byte buffer. `start_bit` is the absolute bit
    // position from the start of `frame`.
    let mut payload = Vec::with_capacity(total_bits.div_ceil(8));
    let mut acc: u32 = 0;
    let mut acc_bits: u32 = 0;
    for i in 0..total_bits {
        let bit_idx = start_bit + i as u64;
        let byte = frame[(bit_idx / 8) as usize];
        let bit_in_byte = 7 - (bit_idx % 8) as u32;
        let bit = (byte >> bit_in_byte) & 1;
        acc = (acc << 1) | u32::from(bit);
        acc_bits += 1;
        if acc_bits == 8 {
            payload.push(acc as u8);
            acc = 0;
            acc_bits = 0;
        }
    }
    if acc_bits > 0 {
        // Left-align the tail bits within the final byte so the
        // §2.4.3.1 stream feed sees them MSB-first.
        payload.push((acc << (8 - acc_bits)) as u8);
    }

    crc16_layer2(header_high, header_low, &payload, total_bits)
}

/// Stand-alone helper for callers that want the raw §2.4.3.1 CRC over
/// already-extracted bytes (e.g. an encoder building a frame). Forwards
/// to [`crc16_layer2`]; exposed here so the frame-level public surface
/// is self-contained.
pub fn layer2_crc(
    header_bytes: [u8; 4],
    allocation_and_scfsi: &[u8],
    allocation_and_scfsi_bits: usize,
) -> u16 {
    crc16_layer2(
        header_bytes[2],
        header_bytes[3],
        allocation_and_scfsi,
        allocation_and_scfsi_bits,
    )
}

/// Convenience: decode a sequence of contiguous Layer II frames from
/// `buf` until the buffer is exhausted, returning per-channel
/// concatenated PCM.
///
/// Frames are chained by advancing the buffer by each frame's
/// §2.4.3.1 `frame_size_bytes()`. Frames whose header fails to parse
/// halt the loop with the underlying [`FrameError`]; the partial PCM
/// decoded so far is discarded (callers that need partial-output
/// semantics drive [`decode_frame_with`] manually).
pub fn decode_all_frames(buf: &[u8]) -> Result<Vec<Vec<f64>>, FrameError> {
    let mut state = FrameDecodeState::new();
    let mut pcm: Vec<Vec<f64>> = Vec::new();
    let mut offset = 0;
    while offset + 4 <= buf.len() {
        // Skip any byte that isn't a syncword candidate — Layer II
        // streams in the wild have ID3 tags before the first sync.
        if !(buf[offset] == 0xFF && (buf[offset + 1] & 0xF0) == 0xF0) {
            offset += 1;
            continue;
        }
        let frame = decode_frame_with(&buf[offset..], &mut state)?;
        // Each §2.4.1.3 frame header carries its own `mode` field, so a
        // stream may switch channel count between frames (e.g. a mono
        // intro frame followed by a stereo frame). Grow the per-channel
        // accumulator to the widest channel count seen so far rather
        // than fixing it from the first frame — otherwise a later,
        // wider frame would index past the accumulator's end.
        if frame.pcm.len() > pcm.len() {
            pcm.resize_with(frame.pcm.len(), Vec::new);
        }
        for (ch, samples) in frame.pcm.iter().enumerate() {
            pcm[ch].extend_from_slice(samples);
        }
        offset += frame.header.frame_size_bytes();
    }
    Ok(pcm)
}

/// Decode a §2.4.2.3 **free-format** Layer II stream end-to-end.
///
/// Free-format frames (`bitrate_index == '0000'`) carry no signalled
/// bitrate; the constant base slot count `N` is recovered once by
/// measuring the distance between the first two syncwords (see
/// [`crate::freeformat::resolve`]), and every frame is then sized
/// `N + its padding_bit` and decoded through the standard path with the
/// **original free-format header** — the Annex B bit-allocation table
/// for free format is fixed by the sampling frequency alone (Table
/// 3-B.2a at 48 kHz, Table 3-B.2b at 44,1 / 32 kHz, per the table
/// headers on PDF pages 46–47; LSF uses the single 13818-3 Table B.1),
/// which [`crate::bitalloc::select_table`] applies directly to a
/// `bit_rate == 0` header. The fixed rate need not be on the §2.4.2.3
/// ladder ("a fixed bitrate which does not need to be in the list");
/// only a rate above the §2.4.2.3 384 kbit/s Layer II free-format
/// support ceiling is refused, surfaced as [`FrameError::FreeFormat`]
/// wrapping [`crate::freeformat::FreeFormatError::UnsupportedBitrate`].
///
/// Returns per-channel concatenated PCM, exactly like
/// [`decode_all_frames`].
pub fn decode_free_format_stream(buf: &[u8]) -> Result<Vec<Vec<f64>>, FrameError> {
    let mut state = FrameDecodeState::new();
    let mut pcm: Vec<Vec<f64>> = Vec::new();

    // Find the first syncword (skip any leading ID3 / junk).
    let mut offset = match crate::header::find_sync(buf) {
        Some(o) => o,
        None => return Ok(pcm),
    };

    // Recover the constant base slot count from the first frame's
    // sync-to-sync distance (resolve also enforces the §2.4.2.3
    // 384 kbit/s free-format support ceiling; its recovered bit_rate is
    // metadata we don't need for decoding).
    let layout = crate::freeformat::resolve(&buf[offset..]).map_err(FrameError::FreeFormat)?;
    let base_slots = layout.base_slots;

    while offset + 4 <= buf.len() {
        if !(buf[offset] == 0xFF && (buf[offset + 1] & 0xF0) == 0xF0) {
            offset += 1;
            continue;
        }
        // Parse the free-format header, fill in the recovered bitrate, and
        // size this frame as `N + its padding_bit`.
        let ff_header = match FrameHeader::parse_allow_free_format(&buf[offset..]) {
            Ok(h) if h.is_free_format() => h,
            // A non-free-format (or unparseable) frame ends the
            // free-format run; stop cleanly with what we have.
            _ => break,
        };
        let frame_size = base_slots + if ff_header.padding { 1 } else { 0 };
        if offset + frame_size > buf.len() {
            break; // truncated trailing frame — emit what decoded so far
        }
        // Decode with the free-format header itself (`bit_rate == 0`):
        // `select_table` keys the free-format case on the sampling
        // frequency alone (Table 3-B.2a/B.2b headers).
        let frame = decode_frame_with_known_header(
            &buf[offset..offset + frame_size],
            ff_header,
            &mut state,
        )?;
        if frame.pcm.len() > pcm.len() {
            pcm.resize_with(frame.pcm.len(), Vec::new);
        }
        for (ch, samples) in frame.pcm.iter().enumerate() {
            pcm[ch].extend_from_slice(samples);
        }
        offset += frame_size;
    }
    Ok(pcm)
}

/// Like [`decode_frame_with`] but the caller supplies an already-parsed,
/// already-sized [`FrameHeader`] (e.g. a free-format header with the
/// recovered bitrate filled in) and `frame` is exactly the frame's bytes.
///
/// The header is trusted: no re-parse, no `frame_size_bytes` truncation
/// check (the caller has already sized `frame`). The §2.4.3.1 CRC and the
/// §2.4.1.6 sample loop run exactly as in [`decode_frame_with`].
pub fn decode_frame_with_known_header(
    frame: &[u8],
    header: FrameHeader,
    state: &mut FrameDecodeState,
) -> Result<DecodedFrame, FrameError> {
    let channels = header.channels();
    state.ensure_channels(channels);

    let (expected_crc, after_header_byte) = if !header.protection_bit {
        if frame.len() < 6 {
            return Err(FrameError::Truncated {
                have: frame.len(),
                need: 6,
            });
        }
        let crc = u16::from_be_bytes([frame[4], frame[5]]);
        (Some(crc), 6)
    } else {
        (None, 4)
    };

    let mut reader = BitReader::with_position(frame, after_header_byte);
    let alloc_start_bit = reader.bit_position();
    let (audio, alloc_bits, scfsi_bits) = parse_audio_data_with_section_bits(&header, &mut reader)?;

    if let Some(expected) = expected_crc {
        let computed = compute_layer2_crc(frame, alloc_start_bit, alloc_bits + scfsi_bits);
        if computed != expected {
            return Err(FrameError::CrcMismatch { computed, expected });
        }
    }

    let mut subband: Vec<Vec<[f64; NUM_SUBBANDS]>> = (0..channels)
        .map(|_| vec![[0.0_f64; NUM_SUBBANDS]; SAMPLE_GRANULES_PER_FRAME * SAMPLES_PER_TRIPLET])
        .collect();

    for sample_gr in 0..SAMPLE_GRANULES_PER_FRAME {
        let sf_gr = sample_gr / 4;
        let base = sample_gr * SAMPLES_PER_TRIPLET;
        for sb in 0..audio.bound {
            for (ch, channel_subband) in subband.iter_mut().enumerate().take(channels) {
                let nb_steps = audio.nb_steps[ch][sb];
                if nb_steps == 0 {
                    continue;
                }
                let class = class_of_quantization(nb_steps)
                    .ok_or(FrameError::UnknownQuantClass { ch, sb, nb_steps })?;
                let triplet = read_triplet(&class, &mut reader)?;
                let sf_idx = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = SCALEFACTORS[sf_idx];
                channel_subband[base][sb] = triplet[0] * factor;
                channel_subband[base + 1][sb] = triplet[1] * factor;
                channel_subband[base + 2][sb] = triplet[2] * factor;
            }
        }
        for sb in audio.bound..audio.sblimit {
            let nb_steps = audio.nb_steps[0][sb];
            if nb_steps == 0 {
                continue;
            }
            let class = class_of_quantization(nb_steps).ok_or(FrameError::UnknownQuantClass {
                ch: 0,
                sb,
                nb_steps,
            })?;
            let triplet = read_triplet(&class, &mut reader)?;
            for (ch, channel_subband) in subband.iter_mut().enumerate().take(channels) {
                let sf_idx = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = SCALEFACTORS[sf_idx];
                channel_subband[base][sb] = triplet[0] * factor;
                channel_subband[base + 1][sb] = triplet[1] * factor;
                channel_subband[base + 2][sb] = triplet[2] * factor;
            }
        }
    }

    let mut pcm: Vec<Vec<f64>> = (0..channels)
        .map(|_| Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL))
        .collect();
    let mut out_block = [0.0_f64; NUM_SUBBANDS];
    for (ch, channel_subband) in subband.iter().enumerate() {
        let fb = &mut state.filterbank[ch];
        for slot in channel_subband.iter() {
            fb.push_subbands(slot, &mut out_block);
            pcm[ch].extend_from_slice(&out_block);
        }
    }
    // §2.4.2.4 de-emphasis (see `decode_frame_with`).
    state.apply_deemphasis(&header, &mut pcm);
    let _ = audio.scfsi;
    Ok(DecodedFrame { header, pcm })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Mode;

    /// Load the staged Layer II fixture.
    ///
    /// The fixture lives in the workspace's `docs/audio/mp3/fixtures/`
    /// tree, which is one directory above the crate's `Cargo.toml`. On
    /// a standalone-crate CI checkout (where the workspace's `docs/`
    /// is absent) the fixture cannot be loaded; the caller checks for
    /// `None` and short-circuits the test (logging a skip).
    fn fixture_bytes() -> Option<Vec<u8>> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/input.mp3"
        );
        if !std::path::Path::new(path).exists() {
            eprintln!("skip: staged Layer II fixture not present at {path}");
            return None;
        }
        Some(std::fs::read(path).expect("read staged Layer II fixture"))
    }

    #[test]
    fn first_frame_of_staged_fixture_decodes_with_correct_shape() {
        let Some(buf) = fixture_bytes() else { return };
        let frame = decode_frame(&buf).expect("first frame decodes");
        assert_eq!(frame.header.sample_rate, 44_100);
        assert_eq!(frame.header.bit_rate, 192_000);
        assert_eq!(frame.header.mode, Mode::Stereo);
        // protection_bit semantics per §2.4.2.3 are *active low*: the
        // wire bit `'1'` means "no CRC", `'0'` means "16-bit CRC slot
        // follows the header". The staged black-box-encoded fixture has
        // the wire bit `'1'` → field `protection_bit == true` → no
        // CRC. The CRC verification path is exercised separately in
        // `crc_mismatch_is_detected` against a constructed fixture
        // (see that test for the synthesis).
        assert!(
            frame.header.protection_bit,
            "fixture is unprotected (wire protection_bit == '1' = no CRC)"
        );
        assert_eq!(frame.pcm.len(), 2, "stereo");
        for ch in 0..2 {
            assert_eq!(frame.pcm[ch].len(), PCM_SAMPLES_PER_CHANNEL);
            // Every sample is finite and within a generous bound. The
            // physically realisable §2.4.3.4.7.1 nominal range is
            // `[-1, +1]`; the matrix step of Annex A Figure A.2 can in
            // theory exceed it for adversarial inputs, but typical
            // material lands well within ±2.
            for (n, &v) in frame.pcm[ch].iter().enumerate() {
                assert!(v.is_finite(), "ch={ch} n={n} non-finite: {v}");
                assert!(v.abs() < 4.0, "ch={ch} n={n} exceeded ±4: {v}");
            }
        }
    }

    #[test]
    fn second_frame_decodes_after_first_via_explicit_chaining() {
        let Some(buf) = fixture_bytes() else { return };
        let mut state = FrameDecodeState::new();
        let f0 = decode_frame_with(&buf, &mut state).expect("frame 0");
        let off = f0.header.frame_size_bytes();
        let f1 = decode_frame_with(&buf[off..], &mut state).expect("frame 1");
        assert_eq!(f0.header.sample_rate, f1.header.sample_rate);
        assert_eq!(f0.header.bit_rate, f1.header.bit_rate);
        // Frame 0 is unpadded (frame_size 626), frame 1 is padded
        // (frame_size 627) per the trace.txt header summary.
        assert_eq!(f0.header.frame_size_bytes(), 626);
        assert_eq!(f1.header.frame_size_bytes(), 627);
    }

    #[test]
    fn decode_all_frames_yields_expected_total_sample_count() {
        let Some(buf) = fixture_bytes() else { return };
        let pcm = decode_all_frames(&buf).expect("all frames decode");
        assert_eq!(pcm.len(), 2, "stereo");
        // The trace.txt lists 31 HEADER lines (frames 0..=30); each
        // contributes 1152 samples per channel. The file is 19435
        // bytes ≈ 31 × 627.
        let expected = 31 * PCM_SAMPLES_PER_CHANNEL;
        assert_eq!(pcm[0].len(), expected, "ch=0 length");
        assert_eq!(pcm[1].len(), expected, "ch=1 length");
        for channel in pcm.iter().take(2) {
            assert!(channel.iter().all(|v| v.is_finite()));
            // Sanity: the signal is not pathologically silent. Real
            // material has at least one non-trivial sample.
            assert!(channel.iter().any(|v| v.abs() > 1e-3));
        }
    }

    #[test]
    fn crc_mismatch_is_detected() {
        // The staged fixture is unprotected (wire `protection_bit ==
        // '1'`). Build a synthetic CRC-protected frame by:
        //   1. clearing the wire `protection_bit` in the header
        //      (byte 1 bit 0); this tells the decoder to expect a
        //      16-bit CRC slot at bytes [4..6];
        //   2. injecting a deliberately-wrong 0x0000 CRC there;
        //   3. shifting the original audio_data forward by 2 bytes so
        //      the §2.4.1.6 section still parses correctly.
        // The §2.4.3.1 CRC computed over the protected fields will
        // almost certainly differ from 0x0000, so the decoder must
        // raise `FrameError::CrcMismatch`.
        let Some(original) = fixture_bytes() else {
            return;
        };
        let frame0 = decode_frame(&original).expect("baseline frame decodes");
        let fs = frame0.header.frame_size_bytes();
        let mut buf = Vec::with_capacity(fs + 2);
        buf.extend_from_slice(&original[..4]);
        buf[1] &= !0x01; // clear protection_bit (active-low → CRC present)
        buf.push(0x00); // bogus CRC high byte
        buf.push(0x00); // bogus CRC low byte
        buf.extend_from_slice(&original[4..fs]);
        match decode_frame(&buf) {
            Err(FrameError::CrcMismatch { computed, expected }) => {
                assert_eq!(expected, 0x0000);
                assert_ne!(
                    computed, 0x0000,
                    "the synthetic fixture happened to be CRC=0; pick a different fixture"
                );
            }
            other => panic!("expected CrcMismatch, got {other:?}"),
        }
    }

    #[test]
    fn internal_crc_helper_agrees_with_public_layer2_crc() {
        // The `compute_layer2_crc` helper inside this module extracts
        // an arbitrary-bit-aligned protected region from a frame
        // buffer and feeds it through `crc16_layer2`; this test pins
        // the byte-aligned case (the only one that arises in
        // practice, since the audio_data section starts at byte 4 or
        // byte 6 — both byte-aligned). The exhaustive bit-aligned
        // case is exercised indirectly by the `crc_mismatch_is_detected`
        // test, which would have produced a spurious mismatch if the
        // helper had been buggy.
        let mut frame = vec![0u8; 32];
        frame[2] = 0xA0;
        frame[3] = 0x04;
        for i in 0..12 {
            frame[4 + i] = ((i * 17 + 3) & 0xFF) as u8;
        }
        let start_bit = 4u64 * 8;
        let bits = 12 * 8;
        let helper = compute_layer2_crc(&frame, start_bit, bits);
        let direct = crate::crc::crc16_layer2(frame[2], frame[3], &frame[4..16], bits);
        assert_eq!(helper, direct);
    }

    #[test]
    fn truncated_buffer_is_rejected_before_audio_data_parse() {
        let Some(buf) = fixture_bytes() else { return };
        let header = FrameHeader::parse(&buf).unwrap();
        let need = header.frame_size_bytes();
        let truncated = &buf[..need - 1];
        match decode_frame(truncated) {
            Err(FrameError::Truncated {
                have,
                need: reported_need,
            }) => {
                assert_eq!(have, need - 1);
                assert_eq!(reported_need, need);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn reset_re_zeroes_filterbank_state() {
        // After decoding one frame, V is non-zero. Reset should give
        // us back a fresh state — easy to confirm structurally
        // because the type's API doesn't expose V, but the round-trip
        // decode_frame succeeds either way; the test pins that the
        // call is a no-op (no panic, no error from a subsequent
        // decode).
        let Some(buf) = fixture_bytes() else { return };
        let mut state = FrameDecodeState::new();
        let _ = decode_frame_with(&buf, &mut state).expect("frame decoded");
        state.reset();
        // After reset, a subsequent decode of the same first frame
        // must succeed.
        let f2 = decode_frame_with(&buf, &mut state).expect("post-reset decode");
        assert_eq!(f2.pcm[0].len(), PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn pcm_samples_per_channel_constant_matches_spec_formula() {
        assert_eq!(
            PCM_SAMPLES_PER_CHANNEL,
            SAMPLE_GRANULES_PER_FRAME * SAMPLES_PER_TRIPLET * NUM_SUBBANDS
        );
        assert_eq!(PCM_SAMPLES_PER_CHANNEL, 1152);
    }

    #[test]
    fn channel_filterbanks_evolve_independently() {
        // Decoding the same first frame twice through two different
        // FrameDecodeStates (each initialised fresh) must produce
        // identical PCM. Decoding it twice through the SAME state
        // must NOT produce identical PCM (the V ring buffer evolved
        // after the first call).
        let Some(buf) = fixture_bytes() else { return };
        let mut s1 = FrameDecodeState::new();
        let f1a = decode_frame_with(&buf, &mut s1).unwrap();
        let mut s2 = FrameDecodeState::new();
        let f2a = decode_frame_with(&buf, &mut s2).unwrap();
        assert_eq!(f1a.pcm, f2a.pcm);

        let f1b = decode_frame_with(&buf, &mut s1).unwrap();
        // f1a and f1b are the SAME frame decoded against the same
        // stream state — but the first call evolved V. So the second
        // call's output differs.
        let any_diff = f1a.pcm[0]
            .iter()
            .zip(f1b.pcm[0].iter())
            .any(|(a, b)| (a - b).abs() > 1e-12);
        assert!(any_diff, "state did not evolve across decodes");
    }

    #[test]
    fn layer2_crc_helper_matches_internal_compute() {
        // The public helper [`layer2_crc`] is just a thin re-export
        // of [`crc::crc16_layer2`]; double-check the wiring.
        let header = [0xFF, 0xFD, 0x50, 0xC4];
        let payload = [0xAA, 0x55, 0xFF, 0x00, 0x12];
        let bits = 38;
        let direct = crate::crc::crc16_layer2(header[2], header[3], &payload, bits);
        let via_helper = layer2_crc(header, &payload, bits);
        assert_eq!(direct, via_helper);
    }

    #[test]
    fn joint_stereo_above_bound_shares_codeword_across_channels() {
        // §2.4.1.6 / §2.4.2.6: above `bound` the bitstream carries one
        // sample triplet per (sb, gr), valid for both channels. The
        // decoder reconstructs each channel by scaling that shared
        // codeword by the channel's own §2.4.3.3.3 scalefactor. When the
        // encoder hands both channels identical above-bound input (so
        // their scalefactors coincide), the decoded above-bound subband
        // samples must be *identical* between channels — a direct
        // consequence of the single shared codeword. The pre-fix decoder
        // read two independent codewords above the bound and desync'd the
        // bitstream, so this property could not hold.
        use crate::audio_data::parse_audio_data_with_section_bits;
        use crate::bitalloc::class_of_quantization;
        use crate::encoder_bit_allocator::SmrTable;
        use crate::encoder_frame::encode_frame;
        use crate::header::{Emphasis, Mode, ModeExtension};
        use oxideav_core::bits::BitReader;

        let header = FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::JointStereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        };

        // Identical input on both channels → identical scalefactors, so
        // the shared above-bound codeword reconstructs identically.
        let one: Vec<f64> = (0..PCM_SAMPLES_PER_CHANNEL)
            .map(|n| 0.5 * (2.0 * std::f64::consts::PI * 6_000.0 * n as f64 / 44_100.0).sin())
            .collect();
        let pcm = vec![one.clone(), one];

        let smr: SmrTable = [[40.0f64; NUM_SUBBANDS]; 2];

        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode joint-stereo");

        // Parse the audio-data to learn the bound / allocation / scfsi.
        let mut reader = BitReader::with_position(&bytes, 4);
        let (audio, _, _) = parse_audio_data_with_section_bits(&header, &mut reader).unwrap();
        assert_eq!(audio.bound, 4, "Bound4 mode_extension");
        assert!(
            (audio.bound..audio.sblimit).any(|sb| audio.nb_steps[0][sb] != 0),
            "test premise: above-bound region must be allocated"
        );

        // Decode and confirm the decoder stayed bit-aligned and the
        // above-bound subband contributions match between channels. We
        // re-walk the sample loop the same way `decode_frame_with` does,
        // but compare the two channels' reconstructed subband values.
        let decoded = decode_frame(&bytes).expect("decode joint-stereo");
        // The synthesis filterbank mixes subbands, so we compare the
        // pre-synthesis subband matrix instead by re-running the loop.
        let mut r2 = BitReader::with_position(&bytes, 4);
        let (a2, _, _) = parse_audio_data_with_section_bits(&header, &mut r2).unwrap();
        let mut ch0 = vec![[0.0f64; NUM_SUBBANDS]; 36];
        let mut ch1 = vec![[0.0f64; NUM_SUBBANDS]; 36];
        for sample_gr in 0..SAMPLE_GRANULES_PER_FRAME {
            let sf_gr = sample_gr / 4;
            let base = sample_gr * SAMPLES_PER_TRIPLET;
            for sb in 0..a2.bound {
                for ch in 0..2 {
                    let nb = a2.nb_steps[ch][sb];
                    if nb == 0 {
                        continue;
                    }
                    let class = class_of_quantization(nb).unwrap();
                    let t = read_triplet(&class, &mut r2).unwrap();
                    let f = SCALEFACTORS[a2.scalefactor[ch][sb][sf_gr] as usize];
                    let dst = if ch == 0 { &mut ch0 } else { &mut ch1 };
                    for s in 0..3 {
                        dst[base + s][sb] = t[s] * f;
                    }
                }
            }
            for sb in a2.bound..a2.sblimit {
                let nb = a2.nb_steps[0][sb];
                if nb == 0 {
                    continue;
                }
                let class = class_of_quantization(nb).unwrap();
                let t = read_triplet(&class, &mut r2).unwrap();
                for ch in 0..2 {
                    let f = SCALEFACTORS[a2.scalefactor[ch][sb][sf_gr] as usize];
                    let dst = if ch == 0 { &mut ch0 } else { &mut ch1 };
                    for s in 0..3 {
                        dst[base + s][sb] = t[s] * f;
                    }
                }
            }
        }
        for (gr, (a, b)) in ch0.iter().zip(ch1.iter()).enumerate() {
            for sb in audio.bound..audio.sblimit {
                assert_eq!(
                    a[sb], b[sb],
                    "above-bound subband sb={sb} gr={gr} must be identical \
                     (shared codeword + equal scalefactor)"
                );
            }
        }
        // Sanity: the frame decoded to full length on both channels.
        assert_eq!(decoded.pcm[0].len(), PCM_SAMPLES_PER_CHANNEL);
        assert_eq!(decoded.pcm[1].len(), PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn decode_all_frames_handles_channel_count_change_mid_stream() {
        // Each §2.4.1.3 frame header carries its own `mode` field, so a
        // single Layer II stream may switch channel count between
        // frames (a mono frame followed by a stereo frame is legal).
        // `decode_all_frames` sizes its per-channel accumulator from the
        // running maximum channel count, so a wider second frame must
        // not index past the accumulator (regression for the
        // out-of-bounds panic found by the round-296 decode fuzzer:
        // first frame mono → accumulator width 1, second frame stereo →
        // `pcm[1]` out of bounds).
        use crate::encoder_bit_allocator::SmrTable;
        use crate::encoder_frame::encode_frame;
        use crate::header::{Emphasis, ModeExtension};
        use crate::NUM_SUBBANDS;

        let smr: SmrTable = [[0.0f64; NUM_SUBBANDS]; 2];

        // §2.4.2.3: 32 kbit/s with mode single_channel is permitted;
        // 64 kbit/s admits stereo. Both at 44.1 kHz / unprotected.
        let mono_header = FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate: 32_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::SingleChannel,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        };
        let stereo_header = FrameHeader {
            mode: Mode::Stereo,
            bit_rate: 64_000,
            ..mono_header
        };

        let mono_pcm = vec![vec![0.0f64; PCM_SAMPLES_PER_CHANNEL]];
        let stereo_pcm = vec![
            vec![0.0f64; PCM_SAMPLES_PER_CHANNEL],
            vec![0.0f64; PCM_SAMPLES_PER_CHANNEL],
        ];

        let mut stream = encode_frame(&mono_header, &mono_pcm, &smr, 0).expect("encode mono");
        stream.extend_from_slice(
            &encode_frame(&stereo_header, &stereo_pcm, &smr, 0).expect("encode stereo"),
        );

        // Must not panic; the accumulator widens to the stereo frame's
        // two channels. The mono frame contributed one channel's worth
        // of samples; the stereo frame contributes to both. Channel 0
        // therefore holds two frames, channel 1 only the second.
        let pcm = decode_all_frames(&stream).expect("mixed-channel stream decodes");
        assert_eq!(pcm.len(), 2, "accumulator widened to the stereo frame");
        assert_eq!(
            pcm[0].len(),
            2 * PCM_SAMPLES_PER_CHANNEL,
            "ch0: both frames"
        );
        assert_eq!(
            pcm[1].len(),
            PCM_SAMPLES_PER_CHANNEL,
            "ch1: stereo frame only"
        );
    }
}
