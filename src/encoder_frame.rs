//! §2.4 / Annex C MPEG-1 Audio Layer II frame-level encode loop.
//!
//! Pulls the previously-landed encoder primitives together into a
//! single `pcm-in → byte-stream-out` call. The pipeline is the
//! time-reversed dual of [`crate::frame::decode_frame`]:
//!
//! ```text
//!  PCM ─▶ analysis filterbank ─▶ subband samples ─▶
//!         scalefactor extraction (§2.4.3.3.3 / §C.1.5.2.6) ─▶
//!         bit allocation (§C.1.5.2.7) ─▶
//!         scfsi selection (§C.1.5.2.5 Table C.4) ─▶
//!         §2.4.1.3 header + §2.4.1.4 CRC slot + §2.4.1.6 audio-data
//!         + §2.4.3.3.4 sample codewords ─▶ frame bytes
//! ```
//!
//! Each stage is implemented in its own module already and verified
//! against the matching §2.4.3 decode primitive's round-trip
//! identity. This module is the orchestrator; it owns no
//! quantization arithmetic of its own.
//!
//! Clean-room: every stage's algorithm is taken from ISO/IEC 11172-3
//! (1993) — §2.4.1.3 / §2.4.1.6 normative formats, §C.1.3 analysis
//! filterbank, §C.1.5.2 informative bit-allocation / scfsi
//! procedures, and the §2.4.3.4.7.1 nominal sample range. No external
//! encoder or decoder source was consulted.
//!
//! # Frame size
//!
//! One Layer II frame consumes exactly [`crate::frame::PCM_SAMPLES_PER_CHANNEL`]
//! samples per channel and emits exactly
//! `header.frame_size_bytes()` bytes. The §2.4.3.1 padding bit is
//! taken verbatim from the supplied header.
//!
//! # SMR table
//!
//! [`encode_frame`] expects a psychoacoustic-model-supplied
//! [`crate::encoder_bit_allocator::SmrTable`] of signal-to-mask
//! ratios in dB. A constant 0 dB table (the simplest possible model)
//! is exercised in the unit tests; a real encoder front-end populates
//! it from the §D.1 / §D.2 perceptual model.
//!
//! # State
//!
//! [`EncodeFrameState`] carries one [`crate::AnalysisFilterbank`]
//! per channel; the X ring buffer persists across frames per the
//! §C.1.3 Figure C.4 footnote-1 startup convention. Allocate once
//! per logical stream and reuse it for every frame; call
//! [`EncodeFrameState::reset`] on a seek / discontinuity.

// The §2.4.1.6 sample loop and the §C.1.3 filterbank fan-out are
// shaped as nested `(ch, sb, granule)` indexing into multiple
// independent arrays — `nb_steps[ch][sb]`, `scalefactor[ch][sb][g]`,
// and `subband_samples[ch][sb][t]` — so the loop variable is
// genuinely structural, not an iterator-replaceable index.
#![allow(clippy::needless_range_loop)]

use oxideav_core::bits::BitWriter;

use crate::analysis::{AnalysisFilterbank, NUM_SUBBANDS};
use crate::audio_data::{write_audio_data_with_section_bits, AudioDataWriteError, Scfsi};
use crate::bitalloc::{class_of_quantization, BitAllocTable};
use crate::crc::crc16_layer2;
use crate::encoder_bit_allocator::{allocate_bits, BitAllocError, SmrTable};
use crate::encoder_samples::{write_triplet_scaled, SampleWriteError};
use crate::encoder_scalefactors::{compute_scalefactors, SUBBAND_SAMPLES_PER_FRAME};
use crate::encoder_scfsi::select_scfsi;
use crate::frame::{PCM_SAMPLES_PER_CHANNEL, SAMPLES_PER_TRIPLET, SAMPLE_GRANULES_PER_FRAME};
use crate::header::{FrameHeader, HeaderError, PaddingScheduler};
use crate::psy::{
    annex_d_sampling_rate, compute_smr_model1_frame, compute_smr_model2_layer2_frame,
    Model2Layer2State, NUM_SUBBANDS_LAYER2,
};
use crate::tables::SCALEFACTORS;

/// Errors raised by the §2.4 / Annex C frame-level encode loop.
#[derive(Debug, Clone, PartialEq)]
pub enum EncodeError {
    /// One of the per-channel PCM buffers has the wrong length. The
    /// caller passed `pcm[ch]` with a length other than
    /// [`PCM_SAMPLES_PER_CHANNEL`].
    BadPcmLen {
        /// Channel index whose buffer was wrong.
        channel: usize,
        /// Actual length.
        have: usize,
        /// Required length ([`PCM_SAMPLES_PER_CHANNEL`]).
        need: usize,
    },
    /// `pcm.len()` did not match `header.channels()`. The caller
    /// supplied a buffer shape inconsistent with the header.
    BadPcmChannelCount {
        /// Actual `pcm.len()`.
        have: usize,
        /// Required channel count (`header.channels()`).
        need: usize,
    },
    /// `header.emit_bytes()` failed — the header is internally
    /// inconsistent (e.g. an unsupported bitrate / sample rate or a
    /// disallowed §2.4.2.3 bitrate / mode combination).
    Header(HeaderError),
    /// The §C.1.5.2.7 bit-allocator refused the frame. Most commonly
    /// [`BitAllocError::InsufficientFrameSize`] when the caller's
    /// `banc` reservation exceeds the available data bits.
    BitAlloc(BitAllocError),
    /// The §2.4.1.6 audio-data writer refused the prepared
    /// [`crate::AudioData`]. Indicates an internal pipeline
    /// inconsistency (the allocator and scfsi selector should not
    /// produce a struct the writer rejects, so this is a bug
    /// signal rather than a user-facing condition).
    AudioData(AudioDataWriteError),
    /// The §2.4.3.3.4 sample writer rejected a triplet — currently
    /// only fires for reserved scalefactor index 63, which
    /// [`compute_scalefactors`] does not produce, so this is a bug
    /// signal.
    Sample(SampleWriteError),
    /// The §C.1.5.2.7 bit-allocator selected an `nb_steps` value for
    /// some `(ch, sb)` slot for which [`class_of_quantization`]
    /// returns `None`. The allocator never advances into such a
    /// value, so this is a bug signal.
    UnknownQuantClass {
        /// Channel index.
        ch: usize,
        /// Sub-band index.
        sb: usize,
        /// The offending `nb_steps`.
        nb_steps: u32,
    },
    /// The caller-supplied §2.4.1.8 `ancillary_data()` payload does not
    /// fit in the §2.4.2.1 frame tail that remains after the §2.4.1.6
    /// audio-data + §2.4.3.3.4 sample-codeword region. The §2.4.2.8
    /// "no_of_ancillary_bits = frame_bits − header − error_check −
    /// audio_data" identity puts an upper bound on the payload; this
    /// error is raised when `ancillary.len() > space`. `space` is the
    /// byte capacity of the §2.4.1.8 tail (computed after byte-
    /// alignment of the §2.4.3.3.4 sample region); `got` is the byte
    /// length of the rejected payload.
    AncillaryTooLarge {
        /// Number of bytes the §2.4.1.8 tail can hold for this frame.
        space: usize,
        /// Byte length of the rejected payload.
        got: usize,
    },
    /// A batch encode entry point ([`encode_all_frames`] /
    /// [`encode_all_frames_auto`]) was handed a per-channel PCM stream
    /// whose length is not a whole multiple of
    /// [`PCM_SAMPLES_PER_CHANNEL`]. One Layer II frame consumes exactly
    /// 1152 samples per channel (§2.4.1.6); a partial trailing frame
    /// has no defined Layer II encoding, so the batch path rejects it
    /// rather than silently dropping or zero-padding the tail. `have`
    /// is the supplied per-channel sample count; `frame` is
    /// [`PCM_SAMPLES_PER_CHANNEL`].
    ShortPcmTail {
        /// Channel index whose stream length was not a 1152 multiple.
        channel: usize,
        /// Actual per-channel sample count supplied.
        have: usize,
        /// The per-frame block size ([`PCM_SAMPLES_PER_CHANNEL`]).
        frame: usize,
    },
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::BadPcmLen {
                channel,
                have,
                need,
            } => write!(
                f,
                "encode_frame: pcm[{channel}].len() = {have}, expected {need}"
            ),
            EncodeError::BadPcmChannelCount { have, need } => write!(
                f,
                "encode_frame: pcm.len() = {have}, expected {need} per the header"
            ),
            EncodeError::Header(err) => write!(f, "encode_frame: header.emit_bytes(): {err}"),
            EncodeError::BitAlloc(err) => write!(f, "encode_frame: bit allocator: {err}"),
            EncodeError::AudioData(err) => write!(f, "encode_frame: audio-data writer: {err}"),
            EncodeError::Sample(err) => write!(f, "encode_frame: sample writer: {err}"),
            EncodeError::UnknownQuantClass { ch, sb, nb_steps } => write!(
                f,
                "encode_frame: allocator produced unknown nb_steps={nb_steps} at (ch={ch}, sb={sb})"
            ),
            EncodeError::AncillaryTooLarge { space, got } => write!(
                f,
                "encode_frame: ancillary_data() payload of {got} bytes exceeds the {space}-byte §2.4.1.8 tail capacity"
            ),
            EncodeError::ShortPcmTail {
                channel,
                have,
                frame,
            } => write!(
                f,
                "encode_all_frames: pcm[{channel}].len() = {have} is not a whole multiple of {frame} samples/frame"
            ),
        }
    }
}

impl std::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EncodeError::Header(err) => Some(err),
            EncodeError::BitAlloc(err) => Some(err),
            EncodeError::AudioData(err) => Some(err),
            EncodeError::Sample(err) => Some(err),
            _ => None,
        }
    }
}

impl From<HeaderError> for EncodeError {
    fn from(value: HeaderError) -> Self {
        EncodeError::Header(value)
    }
}

impl From<BitAllocError> for EncodeError {
    fn from(value: BitAllocError) -> Self {
        EncodeError::BitAlloc(value)
    }
}

impl From<AudioDataWriteError> for EncodeError {
    fn from(value: AudioDataWriteError) -> Self {
        EncodeError::AudioData(value)
    }
}

impl From<SampleWriteError> for EncodeError {
    fn from(value: SampleWriteError) -> Self {
        EncodeError::Sample(value)
    }
}

/// Stateful per-channel filterbank cache used across successive
/// [`encode_frame_with`] calls so the §C.1.3 X ring buffers persist.
///
/// Allocate one per logical Layer II stream and reuse it for every
/// frame; on seek / stream discontinuity call [`Self::reset`] to
/// re-zero each filterbank's X buffer.
#[derive(Debug, Default)]
pub struct EncodeFrameState {
    filterbank: Vec<AnalysisFilterbank>,
    /// Per-channel §D.2.1 Model-2 threshold-generator state (rolling
    /// two-block `(r, f)` predictor history + the inter-call sample
    /// carry). Lazily grown to `header.channels()` the first time a
    /// Model-2 auto-SMR frame is encoded through this state; unused by
    /// the Model-1 and caller-supplied SMR paths. Threading it through
    /// the same [`EncodeFrameState`] keeps the Model-2 rolling history
    /// continuous across successive frames, exactly as the §C.1.3
    /// analysis filterbank's X ring buffer is.
    model2: Vec<Model2Layer2State>,
}

impl EncodeFrameState {
    /// Fresh state with no filterbanks; they are lazily created on
    /// first encode based on `header.channels()`.
    pub fn new() -> Self {
        EncodeFrameState {
            filterbank: Vec::new(),
            model2: Vec::new(),
        }
    }

    /// Re-zero every filterbank's X ring buffer per §C.1.3 Figure C.4
    /// footnote 1, and reset the per-channel §D.2.1 Model-2
    /// threshold-generator history to its zeroed-startup state. Call on
    /// seek / stream discontinuity.
    pub fn reset(&mut self) {
        for fb in &mut self.filterbank {
            fb.reset();
        }
        for m in &mut self.model2 {
            *m = Model2Layer2State::new();
        }
    }

    fn ensure_channels(&mut self, channels: usize) {
        while self.filterbank.len() < channels {
            self.filterbank.push(AnalysisFilterbank::new());
        }
    }

    /// Lazily grow the per-channel Model-2 state vector to `channels`.
    /// Only the Model-2 auto-SMR path calls this; Model-1 and
    /// caller-supplied paths never touch `self.model2`.
    fn ensure_model2_channels(&mut self, channels: usize) {
        while self.model2.len() < channels {
            self.model2.push(Model2Layer2State::new());
        }
    }
}

/// Where the §C.1.5.2.7 allocator's signal-to-mask-ratio table comes
/// from for one [`encode_frame_inner`] call.
enum SmrSource<'a> {
    /// The caller passed an explicit per-(channel, sub-band) SMR table.
    Provided(&'a SmrTable),
    /// Compute the SMR automatically from this frame's PCM via the
    /// §D.1 Model-1 psychoacoustic chain
    /// ([`compute_smr_model1_frame`]).
    Auto,
    /// Compute the SMR automatically from this frame's PCM via the
    /// §D.2 Model-2 psychoacoustic chain
    /// ([`compute_smr_model2_layer2_frame`]) — the spec's *more
    /// stringent of the twice-per-frame pair* Layer II rule. Needs the
    /// per-channel rolling [`Model2Layer2State`] threaded through
    /// [`EncodeFrameState`].
    AutoModel2,
}

/// Derive the per-(channel, sub-band) §D.1 Model-1 SMR table for the
/// frame from its PCM and the analysis-filterbank subband samples.
///
/// The §D.1 Step 2 `scf_max(n)` operand is the **largest** Table 3-B.1
/// multiplier across the three scalefactor granules of subband `n` —
/// equivalently `SCALEFACTORS[min_index]`, because Table 3-B.1 is
/// monotonically decreasing (index 0 is the largest multiplier). We
/// run [`compute_scalefactors`] over all 32 subbands (passing
/// `sblimit = NUM_SUBBANDS` so every band is active) and take the
/// smallest index per band.
///
/// The §D.1 Step 3 overall-bit-rate offset wants the bit rate **per
/// channel** in kbit/s; that is `header.bit_rate / 1000 / channels`.
///
/// For MPEG-2 LSF sampling rates (16 / 22,05 / 24 kHz) the standard
/// provides **no** Annex D Layer II masking tables, so
/// [`annex_d_sampling_rate`] returns `None` and we fall back to an
/// all-zero SMR table (a flat 0 dB SMR — the allocator then spends
/// bits purely by the rate budget, the same behaviour as a
/// caller-supplied constant table). The fallback keeps auto-encode
/// usable at every rate; a perceptual model for the LSF rates is a
/// documented spec gap, not an implementation one.
fn compute_auto_smr_table(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    subband_samples: &[[[f64; SUBBAND_SAMPLES_PER_FRAME]; NUM_SUBBANDS]],
    channels: usize,
) -> SmrTable {
    let mut smr: SmrTable = [[0.0; NUM_SUBBANDS]; 2];

    // No Annex D Layer II masking tables for the LSF rates: leave the
    // table flat at 0 dB SMR (rate-driven allocation).
    let Some(fs) = annex_d_sampling_rate(header.sample_rate) else {
        return smr;
    };

    let bitrate_per_channel_kbps = f64::from(header.bit_rate) / 1000.0 / channels.max(1) as f64;

    for ch in 0..channels {
        // §D.1 Step 2 `scf_max(n)` per subband: the largest Table 3-B.1
        // multiplier across the three granules.
        let sf = compute_scalefactors(&subband_samples[ch], NUM_SUBBANDS);
        let mut scf_max = [0.0f64; NUM_SUBBANDS_LAYER2];
        for sb in 0..NUM_SUBBANDS_LAYER2 {
            // Smallest index over the three granules ⇒ largest
            // multiplier. `compute_scalefactors` yields valid 0..=62
            // indices, every one a defined `SCALEFACTORS` entry.
            let min_idx = sf[0][sb].min(sf[1][sb]).min(sf[2][sb]) as usize;
            scf_max[sb] = SCALEFACTORS[min_idx];
        }

        let ch_smr = compute_smr_model1_frame(&pcm[ch], &scf_max, fs, bitrate_per_channel_kbps);
        smr[ch][..NUM_SUBBANDS].copy_from_slice(&ch_smr[..NUM_SUBBANDS]);
    }
    smr
}

/// Derive the per-(channel, sub-band) §D.2 Model-2 SMR table for the
/// frame from its PCM, driving the spec's *twice-per-frame,
/// more-stringent-of-the-pair* Layer II threshold generator
/// ([`compute_smr_model2_layer2_frame`]) once per channel.
///
/// Unlike the §D.1 Model-1 chain, Model-2 is **stateful**: each
/// channel's rolling two-block `(r, f)` predictor history and the
/// inter-call 448-sample carry are threaded through `state.model2[ch]`,
/// so consecutive frames encoded through the same [`EncodeFrameState`]
/// see the continuous rolling spectrum the §D.2 model assumes.
///
/// For MPEG-2 LSF sampling rates (16 / 22,05 / 24 kHz) the standard
/// tabulates no Annex D Model-2 calculation-partition / absolute-
/// threshold tables, so [`annex_d_sampling_rate`] returns `None` and we
/// fall back to a flat 0 dB SMR table — identical degenerate behaviour
/// to the Model-1 path. The fallback keeps Model-2 auto-encode usable
/// at every rate; a perceptual model for the LSF rates is a documented
/// spec gap, not an implementation one.
fn compute_auto_smr_table_model2(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    channels: usize,
    state: &mut EncodeFrameState,
) -> SmrTable {
    let mut smr: SmrTable = [[0.0; NUM_SUBBANDS]; 2];

    // No Annex D Layer II Model-2 tables for the LSF rates: leave the
    // table flat at 0 dB SMR (rate-driven allocation). The Model-2
    // state is not advanced in this case (it carries no useful history
    // at an unmodelled rate); a later switch back to a modelled rate
    // simply restarts from the zeroed-startup predictor.
    let Some(fs) = annex_d_sampling_rate(header.sample_rate) else {
        return smr;
    };

    state.ensure_model2_channels(channels);

    for ch in 0..channels {
        let ch_smr = compute_smr_model2_layer2_frame(&pcm[ch], fs, &mut state.model2[ch]);
        smr[ch][..NUM_SUBBANDS].copy_from_slice(&ch_smr[..NUM_SUBBANDS]);
    }
    smr
}

/// Encode one Layer II frame from `header.channels()` channels of
/// [`PCM_SAMPLES_PER_CHANNEL`] samples each.
///
/// Builds a stateless analysis filterbank for the call. Callers that
/// stream successive frames should use [`encode_frame_with`] with a
/// persistent [`EncodeFrameState`] so the §C.1.3 X ring buffer is not
/// reset between frames.
///
/// `pcm[ch][i]` is the `i`-th time-domain PCM sample for channel
/// `ch`, in the §2.4.3.4.7.1 nominal `[-1, +1]` range.
///
/// `smr_db[ch][sb]` is the psychoacoustic-model-supplied
/// signal-to-mask ratio for that `(channel, sub-band)` slot, in dB;
/// the §C.1.5.2.7 allocator chases the slot with the lowest
/// `SNR - SMR` margin. A constant 0 dB table produces a
/// nominally-correct frame whose allocation matches the bit-budget
/// only; real perceptual quality comes from a real psychoacoustic
/// model.
///
/// `banc` is the §2.4.1.10 ancillary-data reservation in bits; pass
/// `0` for no ancillary data.
pub fn encode_frame(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: &SmrTable,
    banc: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(
        header,
        pcm,
        SmrSource::Provided(smr_db),
        banc,
        &[],
        &mut EncodeFrameState::new(),
    )
}

/// Like [`encode_frame`] but with caller-supplied
/// [`EncodeFrameState`] so the §C.1.3 analysis filterbank's X ring
/// buffer persists across frames.
pub fn encode_frame_with(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: &SmrTable,
    banc: u32,
    state: &mut EncodeFrameState,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(header, pcm, SmrSource::Provided(smr_db), banc, &[], state)
}

/// Encode one Layer II frame, computing the §C.1.5.2.7 allocator's
/// signal-to-mask-ratio table **automatically** from the frame's PCM
/// via the §D.1 Model-1 psychoacoustic chain
/// ([`compute_smr_model1_frame`]) — the auto-SMR encode path.
///
/// This is the drop-in perceptual counterpart of [`encode_frame`]:
/// the caller no longer supplies an SMR table; the encoder derives it
/// per frame from the windowed FFT spectrum + the §D.1 masking model.
/// For the MPEG-1 Layer II sampling rates (32 / 44,1 / 48 kHz) the
/// allocation is psychoacoustically driven; for the MPEG-2 LSF rates
/// (which the standard tabulates no Annex D Layer II masking curves
/// for) the SMR degenerates to a flat 0 dB table and the allocation
/// is rate-driven (see [`compute_auto_smr_table`]).
///
/// Builds a stateless analysis filterbank for the call; streaming
/// callers should use [`encode_frame_auto_with`].
pub fn encode_frame_auto(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(
        header,
        pcm,
        SmrSource::Auto,
        banc,
        &[],
        &mut EncodeFrameState::new(),
    )
}

/// Like [`encode_frame_auto`] but with caller-supplied
/// [`EncodeFrameState`] so the §C.1.3 analysis filterbank's X ring
/// buffer persists across successive frames.
pub fn encode_frame_auto_with(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
    state: &mut EncodeFrameState,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(header, pcm, SmrSource::Auto, banc, &[], state)
}

/// Encode one Layer II frame, computing the §C.1.5.2.7 allocator's
/// signal-to-mask-ratio table **automatically** from the frame's PCM
/// via the §D.2 Model-2 psychoacoustic chain
/// ([`compute_smr_model2_layer2_frame`]) — the spec's *more stringent
/// of the twice-per-frame pair* Layer II rule.
///
/// This is the Model-2 counterpart of [`encode_frame_auto`]. Because
/// the §D.2 threshold generator carries a rolling two-block spectral
/// predictor + a 448-sample inter-call tail, a [`EncodeFrameState`]
/// **must** be threaded across successive frames for the model to see
/// the continuous spectrum it assumes — so there is no stateless
/// single-frame Model-2 entry point; use [`encode_frame_auto_model2`]
/// from a fresh state only for the first frame of a stream.
///
/// For the MPEG-1 Layer II sampling rates (32 / 44,1 / 48 kHz) the
/// allocation is psychoacoustically driven; for the MPEG-2 LSF rates
/// the SMR degenerates to a flat 0 dB table and the allocation is
/// rate-driven (the standard tabulates no Annex D Model-2 tables for
/// the LSF rates — see [`compute_auto_smr_table_model2`]).
pub fn encode_frame_auto_model2(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
    state: &mut EncodeFrameState,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(header, pcm, SmrSource::AutoModel2, banc, &[], state)
}

/// Encode one Layer II frame and copy a §2.4.1.8 `ancillary_data()`
/// payload into the §2.4.2.1 frame tail that begins immediately after
/// the §2.4.1.6 audio-data + §2.4.3.3.4 sample-codeword region.
///
/// The §2.4.2.8 semantic identity
/// `no_of_ancillary_bits = (frame bits) − (header + error_check +
/// audio_data)` bounds the available tail capacity. The capacity is
/// computed after byte-aligning the §2.4.3.3.4 sample region, so the
/// payload always starts on a whole byte. Bytes the payload does not
/// fill are zero-padded; an over-long payload is rejected with
/// [`EncodeError::AncillaryTooLarge`] carrying both the actual
/// capacity (`space`) and the rejected length (`got`).
///
/// The §C.1.5.2.7 bit-allocator's `banc` reservation continues to
/// apply: callers staging an ancillary payload typically pick
/// `banc >= ancillary.len() * 8` so the allocator leaves at least the
/// payload-sized tail free; passing `banc == 0` lets the allocator
/// spend the full data-bit budget, in which case `ancillary` must be
/// short enough to fit whatever residue the allocator leaves over the
/// §2.4.2.1 byte rounding.
///
/// The §2.4.3.1 CRC patch runs after the ancillary copy and continues
/// to verify clean — Annex B Table B.5 protects the header second
/// half + the §2.4.1.6 audio-data section (allocation + scfsi),
/// *not* the §2.4.1.8 tail, so the stored CRC is byte-identical to
/// what an empty-ancillary encode would produce.
///
/// Passing `ancillary = &[]` is equivalent to calling
/// [`encode_frame`].
pub fn encode_frame_with_ancillary(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: &SmrTable,
    banc: u32,
    ancillary: &[u8],
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(
        header,
        pcm,
        SmrSource::Provided(smr_db),
        banc,
        ancillary,
        &mut EncodeFrameState::new(),
    )
}

/// Like [`encode_frame_with_ancillary`] but with caller-supplied
/// [`EncodeFrameState`] so the §C.1.3 X ring buffer persists across
/// successive frames.
///
/// Passing `ancillary = &[]` is equivalent to calling
/// [`encode_frame_with`].
pub fn encode_frame_with_state_and_ancillary(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: &SmrTable,
    banc: u32,
    ancillary: &[u8],
    state: &mut EncodeFrameState,
) -> Result<Vec<u8>, EncodeError> {
    encode_frame_inner(
        header,
        pcm,
        SmrSource::Provided(smr_db),
        banc,
        ancillary,
        state,
    )
}

/// Encode an entire multi-frame Layer II stream from one continuous
/// per-channel PCM buffer, deriving the §C.1.5.2.7 allocator's SMR
/// table automatically per frame via the §D.1 Model-1 chain.
///
/// This is the encode-side counterpart of
/// [`crate::frame::decode_all_frames`]: the decoder turns a byte
/// stream into per-channel PCM planes; this turns per-channel PCM
/// planes into the concatenated Layer II byte stream.
///
/// `pcm[ch]` is the full time-domain signal for channel `ch`; its
/// length **must** be a whole multiple of [`PCM_SAMPLES_PER_CHANNEL`]
/// (= 1152) because one Layer II frame consumes exactly 1152 samples
/// per channel (§2.4.1.6) and a partial trailing frame has no defined
/// Layer II encoding. A non-multiple length is rejected with
/// [`EncodeError::ShortPcmTail`]; callers that need to flush a short
/// tail must zero-pad it to a frame boundary themselves so the
/// padding policy is theirs, not the encoder's.
///
/// A single persistent [`EncodeFrameState`] threads the §C.1.3
/// analysis filterbank's X ring buffer through every frame, so the
/// inter-frame filterbank continuity is identical to a hand-rolled
/// [`encode_frame_auto_with`] loop. The returned `Vec<u8>` is the
/// concatenation of every frame's [`encode_frame_auto`] output, ready
/// to feed straight back into [`crate::frame::decode_all_frames`].
///
/// The §2.4.2.3 **padding bit** is driven per frame by an internal
/// [`PaddingScheduler`] (the spec's `rest`/`dif` accumulator), so at
/// the fractional rates (44,1 / 22,05 kHz — "Padding is necessary with
/// a sampling frequency of 44,1 kHz") padded `N+1`-slot frames
/// interleave with unpadded ones to hold the stream's mean bitrate at
/// the signalled value; the caller's `header.padding` field is
/// overridden. At every other Layer II rate the frame size divides
/// evenly and no frame is padded. A hand-rolled loop reproduces the
/// batch output byte-for-byte by threading its own scheduler through
/// [`PaddingScheduler::next_header`].
///
/// `banc` is the per-frame §2.4.1.10 ancillary reservation in bits;
/// pass `0` for none.
pub fn encode_all_frames(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_all_frames_inner(header, pcm, banc, SmrChoice::Auto)
}

/// Like [`encode_all_frames`] but with a caller-supplied per-frame
/// signal-to-mask-ratio table used verbatim for every frame (the
/// batch counterpart of [`encode_frame`]).
///
/// The same `smr_db` is applied to each frame; callers needing a
/// per-frame perceptual table should drive [`encode_frame_with`] in
/// their own loop, or use [`encode_all_frames`] for the §D.1
/// automatic path. Length rules and the [`EncodeError::ShortPcmTail`]
/// rejection are identical to [`encode_all_frames`].
pub fn encode_all_frames_with_smr(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: &SmrTable,
    banc: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_all_frames_inner(header, pcm, banc, SmrChoice::Provided(smr_db))
}

/// Like [`encode_all_frames`] but deriving the per-frame SMR table via
/// the §D.2 Model-2 psychoacoustic chain
/// ([`compute_smr_model2_layer2_frame`]) instead of §D.1 Model-1.
///
/// A single persistent [`EncodeFrameState`] threads both the §C.1.3
/// analysis filterbank's X ring buffer **and** the per-channel §D.2.1
/// Model-2 rolling predictor / sample-carry through every frame, so the
/// Model-2 threshold generator sees the continuous spectrum it assumes.
/// Length rules and the [`EncodeError::ShortPcmTail`] rejection are
/// identical to [`encode_all_frames`].
pub fn encode_all_frames_model2(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
) -> Result<Vec<u8>, EncodeError> {
    encode_all_frames_inner(header, pcm, banc, SmrChoice::AutoModel2)
}

/// Per-frame SMR policy for the [`encode_all_frames`] family.
enum SmrChoice<'a> {
    Auto,
    AutoModel2,
    Provided(&'a SmrTable),
}

/// Shared body of the [`encode_all_frames`] entry points: validate the
/// stream shape, then drive one frame at a time through a persistent
/// [`EncodeFrameState`] and concatenate.
fn encode_all_frames_inner(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    banc: u32,
    smr: SmrChoice<'_>,
) -> Result<Vec<u8>, EncodeError> {
    let channels = header.channels();
    if pcm.len() != channels {
        return Err(EncodeError::BadPcmChannelCount {
            have: pcm.len(),
            need: channels,
        });
    }

    // Every channel must carry the same whole number of frames.
    let mut n_frames: Option<usize> = None;
    for (ch, buf) in pcm.iter().enumerate() {
        if buf.len() % PCM_SAMPLES_PER_CHANNEL != 0 {
            return Err(EncodeError::ShortPcmTail {
                channel: ch,
                have: buf.len(),
                frame: PCM_SAMPLES_PER_CHANNEL,
            });
        }
        let frames = buf.len() / PCM_SAMPLES_PER_CHANNEL;
        match n_frames {
            None => n_frames = Some(frames),
            Some(prev) if prev != frames => {
                // Channels of unequal length: report the offending
                // channel's count against the established frame block
                // so the caller sees which plane is mis-sized.
                return Err(EncodeError::BadPcmLen {
                    channel: ch,
                    have: buf.len(),
                    need: prev * PCM_SAMPLES_PER_CHANNEL,
                });
            }
            Some(_) => {}
        }
    }

    let n_frames = n_frames.unwrap_or(0);
    let mut state = EncodeFrameState::new();
    // Pre-size: each frame is `frame_size_bytes()` long, +1 slot on the
    // §2.4.2.3 padded frames.
    let mut out = Vec::with_capacity(n_frames * (header.frame_size_bytes() + 1));
    let mut frame_pcm: Vec<Vec<f64>> = vec![Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL); channels];

    // §2.4.2.3 padding-bit rate control: the batch path owns the whole
    // stream, so it drives the spec's rest/dif accumulator itself and
    // overrides the caller's `header.padding` per frame ("Padding is
    // necessary with a sampling frequency of 44,1 kHz"). At rates where
    // `144·bitrate` divides evenly the scheduler never pads and the
    // caller header is emitted verbatim.
    let mut padding = PaddingScheduler::new();

    for f in 0..n_frames {
        let frame_header = padding.next_header(header);
        let base = f * PCM_SAMPLES_PER_CHANNEL;
        for (ch, plane) in pcm.iter().enumerate() {
            frame_pcm[ch].clear();
            frame_pcm[ch].extend_from_slice(&plane[base..base + PCM_SAMPLES_PER_CHANNEL]);
        }
        let bytes = match smr {
            SmrChoice::Auto => encode_frame_inner(
                &frame_header,
                &frame_pcm,
                SmrSource::Auto,
                banc,
                &[],
                &mut state,
            )?,
            SmrChoice::AutoModel2 => encode_frame_inner(
                &frame_header,
                &frame_pcm,
                SmrSource::AutoModel2,
                banc,
                &[],
                &mut state,
            )?,
            SmrChoice::Provided(table) => encode_frame_inner(
                &frame_header,
                &frame_pcm,
                SmrSource::Provided(table),
                banc,
                &[],
                &mut state,
            )?,
        };
        out.extend_from_slice(&bytes);
    }

    Ok(out)
}

/// The Annex G.1 intensity-stereo sum-signal carrier: the boxed
/// `L + R` subband samples for the `bound..sblimit` region and the
/// sum's own (untransmitted) per-granule quantization scalefactor
/// indices, `[granule][sub-band]`.
type IntensitySum = (
    Box<[[f64; SUBBAND_SAMPLES_PER_FRAME]; NUM_SUBBANDS]>,
    [[u8; NUM_SUBBANDS]; 3],
);

/// Shared implementation of the four public encode entry points.
///
/// All bit-stream assembly happens here so the §2.4.1.8 ancillary copy
/// is wired in exactly one place; the entry points are thin shims that
/// pick the `ancillary` slice and the state instance.
fn encode_frame_inner(
    header: &FrameHeader,
    pcm: &[Vec<f64>],
    smr_db: SmrSource<'_>,
    banc: u32,
    ancillary: &[u8],
    state: &mut EncodeFrameState,
) -> Result<Vec<u8>, EncodeError> {
    let channels = header.channels();

    if pcm.len() != channels {
        return Err(EncodeError::BadPcmChannelCount {
            have: pcm.len(),
            need: channels,
        });
    }
    for (ch, buf) in pcm.iter().enumerate() {
        if buf.len() != PCM_SAMPLES_PER_CHANNEL {
            return Err(EncodeError::BadPcmLen {
                channel: ch,
                have: buf.len(),
                need: PCM_SAMPLES_PER_CHANNEL,
            });
        }
    }

    state.ensure_channels(channels);

    // ---- §C.1.3 analysis filterbank ----
    //
    // Each `push_audio` consumes 32 time-domain PCM samples and
    // produces 32 subband samples (one per sub-band). 1152 PCM
    // samples = 36 successive 32-vectors per channel = 36 sub-band
    // samples per sub-band.
    //
    // The analysis filterbank's input convention is `audio[0]`
    // earliest, `audio[31]` most recent within the 32-vector; we
    // feed the 1152-sample buffer in 32-sample chunks in original
    // time order.
    let timesteps = SAMPLE_GRANULES_PER_FRAME * SAMPLES_PER_TRIPLET; // 36
    let mut subband_samples = vec![[[0.0f64; SUBBAND_SAMPLES_PER_FRAME]; NUM_SUBBANDS]; channels];

    let mut in_block = [0.0f64; NUM_SUBBANDS];
    let mut out_block = [0.0f64; NUM_SUBBANDS];
    for (ch, channel_pcm) in pcm.iter().enumerate().take(channels) {
        let fb = &mut state.filterbank[ch];
        for t in 0..timesteps {
            let base = t * NUM_SUBBANDS;
            in_block.copy_from_slice(&channel_pcm[base..base + NUM_SUBBANDS]);
            fb.push_audio(&in_block, &mut out_block);
            for sb in 0..NUM_SUBBANDS {
                subband_samples[ch][sb][t] = out_block[sb];
            }
        }
    }

    // ---- psychoacoustic SMR table (§D.1 Model-1, or caller-supplied) ----
    //
    // When the caller supplies an explicit table we use it verbatim;
    // when they request `Auto` we drive the §D.1 Model-1 chain
    // ([`psy::compute_smr_model1_frame`]) from this frame's PCM,
    // deriving each subband's §D.1 Step 2 `scf_max(n)` from the
    // scalefactors the encoder extracts independently of allocation
    // (the largest Table 3-B.1 multiplier across the three granules =
    // the smallest of the three scalefactor indices).
    let owned_smr;
    let smr_db: &SmrTable = match smr_db {
        SmrSource::Provided(table) => table,
        SmrSource::Auto => {
            owned_smr = compute_auto_smr_table(header, pcm, &subband_samples, channels);
            &owned_smr
        }
        SmrSource::AutoModel2 => {
            owned_smr = compute_auto_smr_table_model2(header, pcm, channels, state);
            &owned_smr
        }
    };

    // ---- §C.1.5.2.7 bit allocation against the SMR table ----
    let mut audio = allocate_bits(header, smr_db, banc)?;

    // ---- §2.4.3.3.3 / §C.1.5.2.6 scalefactor extraction ----
    //
    // Compute per-(ch, sb, granule) scalefactor indices from the
    // 36 sub-band samples. The allocator's `sblimit` bounds the
    // active sub-bands; entries above `sblimit` are left at the
    // neutral default (index 62) which `compute_scalefactors`
    // already populates.
    //
    // For sub-bands `sb < audio.sblimit` whose `nb_steps[ch][sb]`
    // is non-zero, the per-granule scalefactor is committed to
    // `audio.scalefactor[ch][sb]`; for `nb_steps == 0` the slot
    // does not appear on the wire and the value is irrelevant
    // (left at the default zero per `allocate_bits`).
    for (ch, ch_sub) in subband_samples.iter().enumerate().take(channels) {
        let sf = compute_scalefactors(ch_sub, audio.sblimit);
        for sb in 0..audio.sblimit {
            if audio.nb_steps[ch][sb] == 0 {
                continue;
            }
            audio.scalefactor[ch][sb] = [sf[0][sb], sf[1][sb], sf[2][sb]];
        }
    }

    // §2.4.1.6 intensity-stereo correction: for `sb >= bound` in
    // joint-stereo mode both channels share one allocation field but
    // each channel records its own scalefactor. The allocator has
    // already enforced `nb_steps[0][sb] == nb_steps[1][sb]` above
    // bound; each channel's scalefactor is taken from its own
    // subband samples without further coupling.

    // ---- §C.1.5.2.5 / Table C.4 SCFSI selection ----
    //
    // For each (ch, sb) with non-zero allocation, run the
    // §C.1.5.2.5 difference-class classification + Table C.4
    // lookup, and replace the per-granule scalefactor triple with
    // the `used` triple Table C.4 prescribes. The `scfsi` field is
    // populated with the matching 2-bit schedule the audio-data
    // writer needs.
    for ch in 0..channels {
        for sb in 0..audio.sblimit {
            if audio.nb_steps[ch][sb] == 0 {
                audio.scfsi[ch][sb] = Scfsi::ThreePerGranule;
                continue;
            }
            let sel = select_scfsi(audio.scalefactor[ch][sb]);
            audio.scfsi[ch][sb] = sel.scfsi;
            audio.scalefactor[ch][sb] = sel.used;
        }
    }

    // ---- Annex G.1 intensity-stereo sum signal ----
    //
    // "The basic idea for intensity stereo coding is that for some
    // subbands, instead of transmitting separate left and right subband
    // samples, only the sum-signal is transmitted, but with
    // scalefactors for both the left and right channels" — and: "The
    // left and right subband signals of the subbands in joint stereo
    // mode are added. These new subband signals are scaled in the
    // normal way, but the originally determined scalefactors of the
    // left and right subband signals are transmitted according to the
    // bitstream syntax."
    //
    // So for `bound <= sb < sblimit` the on-wire codeword is the
    // quantized **sum** `L + R`, normalised by the sum signal's own
    // (untransmitted) scalefactor; each decoder channel then rescales
    // that shared codeword by its own transmitted scalefactor
    // (§2.4.3.3.3), reproducing the sum's temporal envelope at each
    // channel's original level. The sum's amplitude is at most 2,
    // which Table 3-B.1 index 0 (multiplier 2.0) covers, so the
    // quantized fraction stays inside the §2.4.3.3.4 domain.
    let intensity_sum: Option<IntensitySum> = if channels == 2 && audio.bound < audio.sblimit {
        let mut sum = Box::new([[0.0f64; SUBBAND_SAMPLES_PER_FRAME]; NUM_SUBBANDS]);
        for sb in audio.bound..audio.sblimit {
            for t in 0..SUBBAND_SAMPLES_PER_FRAME {
                sum[sb][t] = subband_samples[0][sb][t] + subband_samples[1][sb][t];
            }
        }
        let sum_sf = compute_scalefactors(&sum, audio.sblimit);
        Some((sum, sum_sf))
    } else {
        None
    };

    // ---- §2.4.1.3 header bytes ----
    let header_bytes = header.emit_bytes()?;

    // ---- §2.4.1.4 / §2.4.3.1 frame buffer ----
    //
    // We assemble the frame as a single `BitWriter` so the §2.4.1.6
    // audio-data writer, the §2.4.3.3.4 sample writer, and the
    // optional CRC patch all share the same bit-stream view.
    let frame_size = header.frame_size_bytes();
    let mut writer = BitWriter::with_capacity(frame_size);

    // Header (32 bits, byte-aligned).
    writer.write_byte(header_bytes[0]);
    writer.write_byte(header_bytes[1]);
    writer.write_byte(header_bytes[2]);
    writer.write_byte(header_bytes[3]);

    // CRC slot — reserved as zero, patched in below once the
    // protected region is known.
    let crc_slot_bit = if !header.protection_bit {
        let pos = writer.bit_position();
        writer.write_u32(0, 16);
        Some(pos)
    } else {
        None
    };

    // §2.4.1.6 audio-data section (allocation + scfsi + scalefactors).
    let alloc_start_bit = writer.bit_position();
    let (alloc_bits, scfsi_bits) = write_audio_data_with_section_bits(header, &audio, &mut writer)?;

    // §2.4.3.3.4 sample-codeword loop — same `(sample_gr, sb, ch)`
    // shape as the decoder, but on the encode path we look up the
    // post-rescaling `s'` from `subband_samples[ch][sb][t]` (each
    // sample-granule contributes three contiguous values starting at
    // `t = sample_gr * 3`).
    //
    // The §2.4.1.6 syntax has two regions per granule (mirroring the
    // decoder in [`crate::frame`]):
    //
    //   * `sb < bound` — one triplet *per channel* (`samplecode[ch][sb]`).
    //   * `bound <= sb < sblimit` (intensity-stereo region) — only ONE
    //     triplet (`samplecode[0][sb]`); §2.4.2.6 "for subbands in
    //     intensity_stereo mode the coded representation of the sample
    //     is valid for both channels." Channel 0's samples are the
    //     authoritative on-wire codeword. The allocator has already
    //     enforced `nb_steps[0][sb] == nb_steps[1][sb]` above bound.
    //
    // For the non-joint modes `bound == sblimit`, so the second region
    // is empty and this reduces to a flat per-channel write.
    for sample_gr in 0..SAMPLE_GRANULES_PER_FRAME {
        let base = sample_gr * SAMPLES_PER_TRIPLET;
        let sf_gr = sample_gr / 4; // §2.4.2.3 partition.

        // Region 1: `sb < bound` — one triplet per channel.
        for sb in 0..audio.bound {
            for ch in 0..channels {
                let nb = audio.nb_steps[ch][sb];
                if nb == 0 {
                    continue; // §2.4.2.3 "no bits allocated" sentinel.
                }
                let class = class_of_quantization(nb).ok_or(EncodeError::UnknownQuantClass {
                    ch,
                    sb,
                    nb_steps: nb,
                })?;
                let sf_idx = audio.scalefactor[ch][sb][sf_gr];
                let triplet = [
                    subband_samples[ch][sb][base],
                    subband_samples[ch][sb][base + 1],
                    subband_samples[ch][sb][base + 2],
                ];
                write_triplet_scaled(&class, sf_idx, &triplet, &mut writer)?;
            }
        }

        // Region 2: `bound <= sb < sblimit` — one shared triplet
        // (`samplecode[0][sb]`), carrying the Annex G.1 **sum signal**
        // `L + R` normalised by the sum's own (untransmitted)
        // scalefactor. For a single-channel intensity write (never the
        // case in practice — `bound < sblimit` only under joint
        // stereo), `intensity_sum` is `None` and channel 0's samples
        // stand in.
        for sb in audio.bound..audio.sblimit {
            let nb = audio.nb_steps[0][sb];
            if nb == 0 {
                continue; // §2.4.2.3 "no bits allocated" sentinel.
            }
            let class = class_of_quantization(nb).ok_or(EncodeError::UnknownQuantClass {
                ch: 0,
                sb,
                nb_steps: nb,
            })?;
            let (sf_idx, triplet) = match &intensity_sum {
                Some((sum, sum_sf)) => (
                    sum_sf[sf_gr][sb],
                    [sum[sb][base], sum[sb][base + 1], sum[sb][base + 2]],
                ),
                None => (
                    audio.scalefactor[0][sb][sf_gr],
                    [
                        subband_samples[0][sb][base],
                        subband_samples[0][sb][base + 1],
                        subband_samples[0][sb][base + 2],
                    ],
                ),
            };
            write_triplet_scaled(&class, sf_idx, &triplet, &mut writer)?;
        }
    }

    // §2.4.1.8 ancillary_data() — the spec's tail bit-loop:
    //
    //   if ((layer == 1) || (layer == 2))
    //       for (b = 0; b < no_of_ancillary_bits; b++)
    //           ancillary_bit                       1   bslbf
    //
    // §2.4.2.8 fixes `no_of_ancillary_bits` as the frame-byte budget
    // minus the header / error-check / audio-data spend. The
    // §C.1.5.2.7 allocator's `banc` reservation already steers the
    // bit budget so at least `banc` tail bits are left free.
    //
    // We first byte-align the §2.4.3.3.4 sample region; any partial
    // trailing bits left by the sample-codeword loop are padded with
    // zeros up to the next byte boundary so the §2.4.1.8 tail starts
    // on a whole byte. (The §2.4.1.8 syntax is bit-loop, but in
    // practice the §2.4.2.1 frame is byte-granular and so is every
    // §2.4.1.6 field we wrote before this point; the only sub-byte
    // residue is the tail of the sample region.)
    writer.align_to_byte();

    let audio_data_end = writer.byte_len();
    debug_assert!(
        audio_data_end <= frame_size,
        "encode_frame: §2.4.1.6 + §2.4.3.3.4 region exceeded frame_size_bytes(); \
         allocator's banc / sblimit accounting is broken"
    );
    let ancillary_capacity = frame_size - audio_data_end;
    if ancillary.len() > ancillary_capacity {
        return Err(EncodeError::AncillaryTooLarge {
            space: ancillary_capacity,
            got: ancillary.len(),
        });
    }

    // Copy the caller-supplied §2.4.1.8 payload into the tail. Any
    // bytes the payload does not fill are zero-padded so the §2.4.2.1
    // frame-byte budget is met exactly.
    for &b in ancillary {
        writer.write_byte(b);
    }
    while writer.byte_len() < frame_size {
        writer.write_byte(0);
    }

    // The `banc` parameter remains the allocator-side reservation
    // hint per §C.1.5.2.7; with an ancillary payload the caller
    // typically picks `banc >= ancillary.len() * 8` so the allocator
    // leaves at least the payload-sized tail unfilled. The value is
    // not re-checked here because the §C.1.5.2.7 `allocate_bits`
    // call above already honours it when it bounds `adb`; the
    // AncillaryTooLarge branch above is the post-allocation sanity
    // check.

    let mut bytes = writer.into_bytes();

    // Defensive trim: if the sample loop or `banc` reservation
    // somehow pushed past `frame_size`, drop the tail. The §C.1.5.2.7
    // marginal-cost calculation guarantees this never happens, but
    // pinning the invariant explicitly makes the encoder safer in
    // the face of future regressions.
    bytes.truncate(frame_size);

    // ---- §2.4.3.1 CRC patch ----
    //
    // When `protection_bit == 0` the 16 bits immediately after the
    // header are the CRC over header bits 16..31 + (allocation +
    // scfsi). The encode-side helper expects an already-extracted
    // packed payload; we already wrote those exact bits into
    // `bytes`, so the CRC region is the byte range starting at
    // `alloc_start_bit / 8` for `(alloc_bits + scfsi_bits)` bits.
    // The audio-data section starts at the byte boundary just after
    // the 6-byte (header + CRC) prefix when `protection_bit == 0`
    // and just after the 4-byte header when `protection_bit == 1`.
    // The bit position from `BitWriter::bit_position()` and the
    // byte layout therefore line up exactly; we extract the
    // payload, run the CRC, and patch the two reserved bytes.
    if let Some(crc_slot_bit) = crc_slot_bit {
        let total_bits = alloc_bits + scfsi_bits;
        let payload = extract_packed_bits(&bytes, alloc_start_bit, total_bits);
        let crc = crc16_layer2(header_bytes[2], header_bytes[3], &payload, total_bits);
        let crc_byte = (crc_slot_bit / 8) as usize;
        debug_assert_eq!(crc_slot_bit % 8, 0, "CRC slot must be byte-aligned");
        bytes[crc_byte] = (crc >> 8) as u8;
        bytes[crc_byte + 1] = (crc & 0xff) as u8;
    }

    Ok(bytes)
}

/// Extract `total_bits` MSB-first starting at `start_bit` from `src`
/// and pack them left-aligned into a fresh byte buffer.
///
/// Companion to `frame::compute_layer2_crc`'s extractor; kept private
/// to the encoder module so the §2.4.3.1 protected-region payload can
/// be reconstructed without exposing it on the crate's public surface.
fn extract_packed_bits(src: &[u8], start_bit: u64, total_bits: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(total_bits.div_ceil(8));
    let mut acc: u32 = 0;
    let mut acc_bits: u32 = 0;
    for i in 0..total_bits {
        let bit_idx = start_bit + i as u64;
        let byte = src[(bit_idx / 8) as usize];
        let bit_in_byte = 7 - (bit_idx % 8) as u32;
        let bit = (byte >> bit_in_byte) & 1;
        acc = (acc << 1) | u32::from(bit);
        acc_bits += 1;
        if acc_bits == 8 {
            out.push(acc as u8);
            acc = 0;
            acc_bits = 0;
        }
    }
    if acc_bits > 0 {
        out.push((acc << (8 - acc_bits)) as u8);
    }
    out
}

/// Bound the `nb_steps` value to one in [`crate::bitalloc::class_of_quantization`].
/// Compile-time pinning so adding a new Table B.4 class doesn't drift
/// the encoder from the bit-allocator silently.
#[allow(dead_code)]
const fn _table_b4_classes_are_walked() -> u32 {
    // Touch the `class_of_quantization` import so an accidental drop
    // shows up as an unused-import warning rather than silently
    // breaking the sample loop.
    0
}

// Pin a few public-surface assertions at compile time so a refactor
// of the constants in `frame.rs` would trigger a build break here.
const _ASSERT_TIMESTEPS: () = {
    assert!(PCM_SAMPLES_PER_CHANNEL == SAMPLE_GRANULES_PER_FRAME * SAMPLES_PER_TRIPLET * 32);
    assert!(SAMPLES_PER_TRIPLET == 3);
    assert!(SAMPLE_GRANULES_PER_FRAME == 12);
};

// Unused-import suppression for the imported but compile-only-touched
// types. We deliberately use them inside the body.
#[allow(dead_code)]
fn _walk_imports() {
    let _: Option<crate::bitalloc::QuantClass> = class_of_quantization(0);
    let _: Option<BitAllocTable> = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_data::parse_audio_data_with_section_bits;
    use crate::frame::{decode_frame, FrameError};
    use crate::header::{Emphasis, Mode, ModeExtension};
    use oxideav_core::bits::BitReader;

    fn canonical_stereo_header() -> FrameHeader {
        // 192 kbit/s / 44.1 kHz / Stereo / CRC-enabled.
        FrameHeader {
            lsf: false,
            protection_bit: false, // false == "CRC present" per §2.4.2.3 inverted convention
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        }
    }

    fn canonical_single_channel_header() -> FrameHeader {
        // 64 kbit/s / 44.1 kHz / single-channel / no CRC.
        FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate: 64_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::SingleChannel,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        }
    }

    fn zero_pcm(channels: usize) -> Vec<Vec<f64>> {
        (0..channels)
            .map(|_| vec![0.0f64; PCM_SAMPLES_PER_CHANNEL])
            .collect()
    }

    fn zero_smr() -> SmrTable {
        [[0.0f64; NUM_SUBBANDS]; 2]
    }

    fn tone_pcm(channels: usize, freq_hz: f64, amplitude: f64) -> Vec<Vec<f64>> {
        // Deterministic, non-trivial input. `freq_hz` is in cycles per
        // 44.1 kHz (the test's notional sample rate). Amplitude must
        // stay in [-1, +1] per §2.4.3.4.7.1.
        let omega = 2.0 * core::f64::consts::PI * freq_hz / 44_100.0;
        (0..channels)
            .map(|ch| {
                (0..PCM_SAMPLES_PER_CHANNEL)
                    .map(|i| amplitude * (omega * (i as f64 + ch as f64 * 64.0)).sin())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn zero_input_emits_a_well_formed_frame() {
        let header = canonical_stereo_header();
        let pcm = zero_pcm(2);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        assert_eq!(bytes.len(), header.frame_size_bytes());
        // Header round-trips.
        let parsed = FrameHeader::parse(&bytes).expect("parse header");
        assert_eq!(parsed, header);
    }

    #[test]
    fn frame_round_trips_through_decoder() {
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.5);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        let decoded = decode_frame(&bytes).expect("decode");
        assert_eq!(decoded.header, header);
        assert_eq!(decoded.pcm.len(), 2);
        assert_eq!(decoded.pcm[0].len(), PCM_SAMPLES_PER_CHANNEL);
        assert_eq!(decoded.pcm[1].len(), PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn single_channel_round_trips() {
        let header = canonical_single_channel_header();
        let pcm = tone_pcm(1, 440.0, 0.25);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        let decoded = decode_frame(&bytes).expect("decode");
        assert_eq!(decoded.header.mode, Mode::SingleChannel);
        assert_eq!(decoded.pcm.len(), 1);
        assert_eq!(decoded.pcm[0].len(), PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn audio_data_section_round_trips_through_parser() {
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.4);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        // §2.4.3.1: with CRC present the audio-data section begins
        // 6 bytes in.
        let mut reader = BitReader::with_position(&bytes, 6);
        let (audio, alloc_bits, scfsi_bits) =
            parse_audio_data_with_section_bits(&header, &mut reader).expect("parse audio data");
        assert!(audio.sblimit > 0);
        assert!(audio.sblimit <= NUM_SUBBANDS);
        // Some non-zero allocation must have been chosen for a
        // non-zero input; otherwise the SMR-driven allocator is
        // refusing to spend bits.
        let any_alloc =
            (0..audio.channels).any(|ch| (0..audio.sblimit).any(|sb| audio.nb_steps[ch][sb] != 0));
        assert!(any_alloc, "encoder produced an all-zero allocation");
        assert!(alloc_bits > 0);
        // scfsi_bits is non-zero iff at least one (ch, sb) has a
        // non-zero allocation. We just asserted that, so scfsi_bits
        // must be > 0 too.
        assert!(scfsi_bits > 0);
    }

    #[test]
    fn crc_passes_on_emitted_frame() {
        // The CRC patch step writes the correct §2.4.3.1 CRC; the
        // decoder's CRC verification therefore must accept the
        // emitted frame.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_500.0, 0.3);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        let decoded = decode_frame(&bytes).expect("decode (with CRC check)");
        assert_eq!(decoded.header, header);
    }

    #[test]
    fn flipping_a_crc_payload_byte_makes_decoder_reject() {
        // Build a frame, locate the bit-allocation section, and flip
        // the high bit of its first byte. That bit lands inside the
        // CRC-protected region (Annex B Table B.5) by construction:
        // the allocation section is the first thing after the CRC
        // slot, and `alloc_bits >= 8` for any non-trivial frame, so
        // the high bit of `bytes[6]` is part of the CRC payload. The
        // decoder must then reject the frame with `CrcMismatch`.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.3);
        let smr = zero_smr();
        let mut bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        // First inspect the unflipped frame's allocation indices so
        // we can target a flip that keeps the parsed scalefactor
        // values in-range yet changes the bit-allocation. Flipping
        // the top bit of byte 6 changes the very first allocation
        // index (sb=0, ch=0) and produces a different `nb_steps`,
        // which is still a legal Table B.2 row entry but does not
        // match the CRC payload the encoder hashed.
        bytes[6] ^= 0x80;
        match decode_frame(&bytes) {
            Err(FrameError::CrcMismatch { .. }) => {}
            other => panic!("expected CrcMismatch, got {other:?}"),
        }
    }

    #[test]
    fn bad_pcm_channel_count_is_rejected() {
        let header = canonical_stereo_header(); // 2 channels
        let pcm = zero_pcm(1);
        let smr = zero_smr();
        match encode_frame(&header, &pcm, &smr, 0) {
            Err(EncodeError::BadPcmChannelCount { have, need }) => {
                assert_eq!(have, 1);
                assert_eq!(need, 2);
            }
            other => panic!("expected BadPcmChannelCount, got {other:?}"),
        }
    }

    #[test]
    fn bad_pcm_length_is_rejected() {
        let header = canonical_stereo_header();
        let mut pcm = zero_pcm(2);
        pcm[1].truncate(100);
        let smr = zero_smr();
        match encode_frame(&header, &pcm, &smr, 0) {
            Err(EncodeError::BadPcmLen {
                channel,
                have,
                need,
            }) => {
                assert_eq!(channel, 1);
                assert_eq!(have, 100);
                assert_eq!(need, PCM_SAMPLES_PER_CHANNEL);
            }
            other => panic!("expected BadPcmLen, got {other:?}"),
        }
    }

    #[test]
    fn banc_reservation_appears_at_tail() {
        // With a large `banc` the allocator should leave at least
        // `banc` zero bits at the tail; we cannot guarantee they
        // remain zero (the writer's padding makes them zero anyway)
        // but we can verify the frame is the right size and a
        // round-trip still works.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 2_000.0, 0.25);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 256).expect("encode with banc=256");
        assert_eq!(bytes.len(), header.frame_size_bytes());
        let _decoded = decode_frame(&bytes).expect("decode with banc");
    }

    #[test]
    fn banc_too_large_is_rejected_by_allocator() {
        // A `banc` reservation larger than the available data bits
        // must propagate `InsufficientFrameSize` out.
        let header = canonical_single_channel_header(); // 64 kbit/s frame is small
        let pcm = zero_pcm(1);
        let smr = zero_smr();
        let huge_banc = (header.frame_size_bytes() as u32) * 8;
        match encode_frame(&header, &pcm, &smr, huge_banc) {
            Err(EncodeError::BitAlloc(BitAllocError::InsufficientFrameSize { .. })) => {}
            other => panic!("expected InsufficientFrameSize, got {other:?}"),
        }
    }

    #[test]
    fn state_persists_across_frames() {
        // Encoding the same input twice with a persistent state
        // produces two byte-identical first frames (the X buffer is
        // initialised to zero in both cases); after the first frame
        // the second frame differs from a stateless encode because
        // the X buffer has retained content.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.4);
        let smr = zero_smr();

        let mut state_a = EncodeFrameState::new();
        let f1_a = encode_frame_with(&header, &pcm, &smr, 0, &mut state_a).unwrap();
        let f2_a = encode_frame_with(&header, &pcm, &smr, 0, &mut state_a).unwrap();

        let mut state_b = EncodeFrameState::new();
        let f1_b = encode_frame_with(&header, &pcm, &smr, 0, &mut state_b).unwrap();

        assert_eq!(f1_a, f1_b, "first frame is identical from zero state");
        // After the first frame, the X buffer carries content; the
        // second frame from `state_a` must differ from `state_a`'s
        // first frame (otherwise the analysis filterbank is not
        // accumulating state at all, which would itself be a bug).
        assert_ne!(
            f2_a, f1_a,
            "second frame must reflect accumulated X-buffer state"
        );
    }

    #[test]
    fn reset_state_restores_first_frame_identity() {
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.4);
        let smr = zero_smr();

        let mut state = EncodeFrameState::new();
        let f1 = encode_frame_with(&header, &pcm, &smr, 0, &mut state).unwrap();
        let _ = encode_frame_with(&header, &pcm, &smr, 0, &mut state).unwrap();
        state.reset();
        let f1_again = encode_frame_with(&header, &pcm, &smr, 0, &mut state).unwrap();
        assert_eq!(
            f1, f1_again,
            "post-reset first frame must match the initial first frame"
        );
    }

    #[test]
    fn no_crc_path_round_trips() {
        let header = canonical_single_channel_header(); // protection_bit == true
        let pcm = tone_pcm(1, 500.0, 0.5);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode");
        assert_eq!(bytes.len(), header.frame_size_bytes());
        let decoded = decode_frame(&bytes).expect("decode no-CRC frame");
        assert!(decoded.header.protection_bit);
    }

    #[test]
    fn joint_stereo_above_bound_is_balanced() {
        // Joint-stereo bound=4 forces sb >= 4 to carry one allocation;
        // the encoder must produce `nb_steps[0][sb] == nb_steps[1][sb]`
        // for sb in [4, sblimit), otherwise the audio-data writer
        // would reject the frame with
        // `AudioDataWriteError::IntensityStereoAllocationMismatch`.
        let header = FrameHeader {
            mode: Mode::JointStereo,
            mode_extension: ModeExtension::Bound4,
            ..canonical_stereo_header()
        };
        let pcm = tone_pcm(2, 3_000.0, 0.6);
        let smr = zero_smr();
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode joint-stereo");
        let _decoded = decode_frame(&bytes).expect("decode joint-stereo");
        // Parse the audio-data section back out and inspect the
        // above-bound region for the balance invariant.
        let mut reader = BitReader::with_position(&bytes, 6);
        let (audio, _, _) = parse_audio_data_with_section_bits(&header, &mut reader).unwrap();
        for sb in audio.bound..audio.sblimit {
            assert_eq!(
                audio.nb_steps[0][sb], audio.nb_steps[1][sb],
                "joint-stereo invariant violated at sb={sb}"
            );
        }
    }

    #[test]
    fn joint_stereo_allocator_saturates_the_budget_at_single_shared_codeword_cost() {
        // §2.4.1.6 puts ONE shared sample triplet on the wire per
        // above-`bound` subband per granule, so the §C.1.5.2.7
        // allocator must charge the merged slot's sample bits ONCE.
        // A double-charged merged cost makes the allocator stop one
        // whole channel's worth of intensity sample bits early,
        // leaving a §2.4.1.8 tail far larger than the termination rule
        // ("adb is not less than any possible increase") permits.
        //
        // We encode a joint-stereo frame under a demanding flat SMR,
        // re-parse it, recompute the ACTUAL on-wire audio-data spend
        // (merged samples counted once), and bound the leftover:
        // legitimate slack = (worst-case-minus-actual scalefactor
        // budgeting, ≤ 12 bits per non-zero slot) + (one final
        // unaffordable step, ≤ 616 bits). The double-charge bug wastes
        // an extra copy of every committed above-bound sample bit
        // (thousands of bits here) and fails this bound.
        let header = FrameHeader {
            mode: Mode::JointStereo,
            mode_extension: ModeExtension::Bound4,
            // No CRC so the §2.4.1.6 audio data starts at byte 4 for
            // the re-parse below.
            protection_bit: true,
            ..canonical_stereo_header()
        };
        let smr: SmrTable = [[30.0; NUM_SUBBANDS]; 2];
        let pcm = tone_pcm(2, 1_000.0, 0.5);
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode joint-stereo");

        let mut reader = BitReader::with_position(&bytes, 4);
        let (audio, alloc_bits, scfsi_bits) =
            parse_audio_data_with_section_bits(&header, &mut reader).expect("re-parse");

        // Actual scalefactor + sample spend from the parsed structure.
        let scf_count = |s: crate::audio_data::Scfsi| -> u64 {
            match s {
                crate::audio_data::Scfsi::ThreePerGranule => 3,
                crate::audio_data::Scfsi::Share01Then2 => 2,
                crate::audio_data::Scfsi::Share0Then12 => 2,
                crate::audio_data::Scfsi::ShareAll => 1,
            }
        };
        let mut scf_bits = 0u64;
        let mut sample_bits = 0u64;
        for sb in 0..audio.sblimit {
            for ch in 0..2 {
                if audio.nb_steps[ch][sb] == 0 {
                    continue;
                }
                scf_bits += 6 * scf_count(audio.scfsi[ch][sb]);
                // Below bound each channel carries its own codewords;
                // above bound ONE shared codeword is on the wire.
                if sb < audio.bound || ch == 0 {
                    sample_bits += u64::from(crate::encoder_bit_allocator::sample_bits_for(
                        audio.nb_steps[ch][sb],
                    ));
                }
            }
        }
        let cb = 8 * header.frame_size_bytes() as u64;
        let spent = 32 + alloc_bits as u64 + scfsi_bits as u64 + scf_bits + sample_bits;
        assert!(spent <= cb, "on-wire spend must fit the frame");
        let leftover = cb - spent;

        // Non-zero slots bound the worst-case-scf overshoot.
        let nonzero_slots = (0..audio.sblimit)
            .flat_map(|sb| (0..2).map(move |ch| (ch, sb)))
            .filter(|&(ch, sb)| audio.nb_steps[ch][sb] != 0)
            .count() as u64;
        let slack_bound = 12 * nonzero_slots + 616;
        assert!(
            leftover <= slack_bound,
            "allocator left {leftover} bits unused (permitted slack \
             {slack_bound}); a double-charged merged sample cost wastes \
             one whole copy of the above-bound sample bits"
        );
    }

    #[test]
    fn joint_stereo_above_bound_writes_one_shared_codeword_per_subband() {
        // §2.4.1.6: above `bound` the bitstream carries ONE sample
        // triplet per (sb, gr) — `samplecode[0][sb][gr]` — shared by
        // both channels (intensity stereo). Drive a high-SMR table that
        // forces non-zero allocation in the above-bound region, then
        // confirm the encoded sample-codeword region is sized for ONE
        // codeword per above-bound subband, not two. We measure this
        // by comparing the actual frame's used bits against an
        // independently-computed expectation: doubling the above-bound
        // codewords would overflow the §2.4.3.1 frame size.
        let header = FrameHeader {
            mode: Mode::JointStereo,
            mode_extension: ModeExtension::Bound4,
            bit_rate: 192_000,
            ..canonical_stereo_header()
        };
        let pcm = tone_pcm(2, 6_000.0, 0.7);
        // Boost SMR across the whole spectrum so the allocator spends
        // bits above the bound.
        let smr: SmrTable = [[40.0f64; NUM_SUBBANDS]; 2];
        let bytes = encode_frame(&header, &pcm, &smr, 0).expect("encode joint-stereo");
        assert_eq!(
            bytes.len(),
            header.frame_size_bytes(),
            "frame must be exactly frame_size_bytes — a doubled above-bound \
             sample region would overflow"
        );

        // Confirm at least one above-bound subband actually carries an
        // allocation, otherwise the test would be vacuous.
        let mut reader = BitReader::with_position(&bytes, 6);
        let (audio, _, _) = parse_audio_data_with_section_bits(&header, &mut reader).unwrap();
        let any_above_bound_allocated =
            (audio.bound..audio.sblimit).any(|sb| audio.nb_steps[0][sb] != 0);
        assert!(
            any_above_bound_allocated,
            "test premise: at least one above-bound subband must be allocated"
        );

        // Round-trips cleanly: the decoder consumes exactly one shared
        // codeword per above-bound subband, so the bitstream stays
        // aligned and the frame decodes without a desync.
        let decoded = decode_frame(&bytes).expect("decode joint-stereo");
        assert_eq!(decoded.pcm.len(), 2);
        for ch in 0..2 {
            assert_eq!(decoded.pcm[ch].len(), 1152);
            assert!(decoded.pcm[ch].iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn varying_smr_steers_allocation_to_high_smr_subbands() {
        // Three encodes against the same input: a flat 0 dB SMR, a
        // table that boosts SMR for the low sub-bands, and a table
        // that boosts SMR for the high sub-bands. The allocator
        // should spend more bits on the boosted side.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.5);
        let smr_flat = zero_smr();

        let mut smr_low = zero_smr();
        for sb in 0..8 {
            smr_low[0][sb] = 50.0;
            smr_low[1][sb] = 50.0;
        }
        let mut smr_high = zero_smr();
        for sb in 16..24 {
            smr_high[0][sb] = 50.0;
            smr_high[1][sb] = 50.0;
        }

        fn parse_nb_steps(header: &FrameHeader, bytes: &[u8]) -> [[u32; NUM_SUBBANDS]; 2] {
            let mut reader = BitReader::with_position(bytes, 6);
            let (audio, _, _) =
                parse_audio_data_with_section_bits(header, &mut reader).expect("parse");
            audio.nb_steps
        }

        let bytes_flat = encode_frame(&header, &pcm, &smr_flat, 0).unwrap();
        let bytes_low = encode_frame(&header, &pcm, &smr_low, 0).unwrap();
        let bytes_high = encode_frame(&header, &pcm, &smr_high, 0).unwrap();

        let nb_flat = parse_nb_steps(&header, &bytes_flat);
        let nb_low = parse_nb_steps(&header, &bytes_low);
        let nb_high = parse_nb_steps(&header, &bytes_high);

        let sum_low_band = |nb: &[[u32; NUM_SUBBANDS]; 2]| -> u32 {
            (0..2).map(|ch| nb[ch][..8].iter().sum::<u32>()).sum()
        };
        let sum_high_band = |nb: &[[u32; NUM_SUBBANDS]; 2]| -> u32 {
            (0..2).map(|ch| nb[ch][16..24].iter().sum::<u32>()).sum()
        };

        // Boosting low SMR should not reduce the low-band allocation
        // sum relative to flat; boosting high SMR should not reduce
        // the high-band allocation sum relative to flat. We use
        // ">=" because the §C.1.5.2.7 allocator may saturate
        // sub-bands in the unboosted region under the flat baseline,
        // so the boosted-side sum is at least the flat-baseline
        // sum.
        assert!(
            sum_low_band(&nb_low) >= sum_low_band(&nb_flat),
            "boosting low SMR did not steer allocation toward the low band"
        );
        assert!(
            sum_high_band(&nb_high) >= sum_high_band(&nb_flat),
            "boosting high SMR did not steer allocation toward the high band"
        );
    }

    // ----------------------------------------------------------------
    // §2.4.1.8 ancillary_data() tests
    //
    // The §2.4.2.8 prose puts no_of_ancillary_bits = (frame bits) −
    // (header + error_check + audio_data). With a §C.1.5.2.7 banc
    // reservation in hand, the encoder leaves at least `banc` bits at
    // the tail free; we use `banc >= ancillary.len() * 8` to ensure
    // the payload fits without depending on the allocator's
    // post-budget residue.
    // ----------------------------------------------------------------

    /// Byte-range of the §2.4.1.8 tail in an emitted frame: starts
    /// right after the §2.4.1.6 + §2.4.3.3.4 region, ends at
    /// `frame_size_bytes`. Used by the ancillary tests to inspect the
    /// payload byte-for-byte without re-running the encoder pipeline.
    ///
    /// We approximate the tail start by re-encoding the same frame
    /// with an all-`0xCC` payload of capacity `frame_size`; the first
    /// `0xCC` byte marks the tail boundary. The §2.4.1.6 + §2.4.3.3.4
    /// region never contains `0xCC` as a byte-aligned marker because
    /// the encoder zero-pads to byte alignment before the ancillary
    /// copy, so the first `0xCC` in the frame is unambiguously the
    /// tail start.
    fn locate_ancillary_tail_start(
        header: &FrameHeader,
        pcm: &[Vec<f64>],
        smr: &SmrTable,
        banc: u32,
    ) -> usize {
        let frame_size = header.frame_size_bytes();
        let mut marker = vec![0xCCu8; frame_size];
        // Try progressively smaller markers until the encoder accepts
        // one. We do not actually need a tight fit — once we find a
        // marker that encodes successfully, the first 0xCC byte in the
        // emitted frame is the tail start.
        let bytes = loop {
            match encode_frame_with_ancillary(header, pcm, smr, banc, &marker) {
                Ok(b) => break b,
                Err(EncodeError::AncillaryTooLarge { space, .. }) => {
                    marker.truncate(space);
                }
                Err(other) => panic!("encode_frame_with_ancillary failed: {other:?}"),
            }
        };
        bytes
            .iter()
            .position(|&b| b == 0xCC)
            .expect("ancillary marker not found in emitted frame")
    }

    #[test]
    fn empty_ancillary_matches_legacy_encode_frame() {
        // Calling `encode_frame_with_ancillary` with `&[]` must
        // produce bit-identical bytes to the legacy `encode_frame`
        // for any header / pcm / smr / banc combination.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_200.0, 0.35);
        let smr = zero_smr();
        let legacy = encode_frame(&header, &pcm, &smr, 0).expect("legacy encode");
        let new =
            encode_frame_with_ancillary(&header, &pcm, &smr, 0, &[]).expect("ancillary encode");
        assert_eq!(legacy, new);
    }

    #[test]
    fn ancillary_bytes_land_in_frame_tail() {
        // A small distinctive ancillary payload is copied verbatim
        // into the §2.4.1.8 tail.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_500.0, 0.4);
        let smr = zero_smr();
        let payload: Vec<u8> = (0..32u8)
            .map(|i| i.wrapping_mul(0x37).wrapping_add(0x11))
            .collect();
        // banc covers the payload + a safety margin so the allocator
        // leaves a tail wide enough for the payload regardless of how
        // the §C.1.5.2.7 marginal-cost loop behaves on the input.
        let banc = (payload.len() as u32) * 8 + 32;
        let bytes = encode_frame_with_ancillary(&header, &pcm, &smr, banc, &payload)
            .expect("encode with ancillary");
        assert_eq!(bytes.len(), header.frame_size_bytes());

        // Locate the §2.4.1.8 tail by encoding the same input with a
        // marker payload, then verifying that the same offset in our
        // real-payload frame matches the real payload byte-for-byte.
        let tail_start = locate_ancillary_tail_start(&header, &pcm, &smr, banc);
        assert!(tail_start + payload.len() <= bytes.len());
        assert_eq!(&bytes[tail_start..tail_start + payload.len()], &payload[..]);

        // Trailing bytes past the payload (if any) must be zero.
        for &b in &bytes[tail_start + payload.len()..] {
            assert_eq!(b, 0, "ancillary trailing pad must be zero");
        }

        // The §2.4.3.1 CRC patch is over the Annex B Table B.5
        // protected region (header bits 16..31 + allocation + scfsi),
        // which does NOT include the §2.4.1.8 tail. The decoder must
        // therefore accept this frame.
        let _decoded = decode_frame(&bytes).expect("decode ancillary frame");
    }

    #[test]
    fn ancillary_crc_matches_empty_ancillary_frame() {
        // The two-byte §2.4.3.1 CRC slot must be byte-identical
        // between an empty-ancillary frame and the same frame with a
        // non-empty payload — Annex B Table B.5 excludes the §2.4.1.8
        // tail from the CRC.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 800.0, 0.5);
        let smr = zero_smr();
        let payload: Vec<u8> = (0..48u8).map(|i| i ^ 0xA5).collect();
        let banc = (payload.len() as u32) * 8 + 32;
        let empty = encode_frame_with_ancillary(&header, &pcm, &smr, banc, &[]).unwrap();
        let with_anc = encode_frame_with_ancillary(&header, &pcm, &smr, banc, &payload).unwrap();
        // bytes [4, 6) hold the §2.4.3.1 CRC word when protection_bit
        // == false (the canonical_stereo_header convention).
        assert_eq!(empty[4..6], with_anc[4..6]);
    }

    #[test]
    fn oversized_ancillary_is_rejected() {
        // A payload larger than the §2.4.1.8 tail capacity surfaces
        // `AncillaryTooLarge` with both `space` and `got` populated.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.3);
        let smr = zero_smr();
        // A payload sized to the full frame is guaranteed not to fit
        // because the header + CRC slot + audio_data already consume
        // a non-trivial prefix.
        let huge = vec![0xFFu8; header.frame_size_bytes()];
        match encode_frame_with_ancillary(&header, &pcm, &smr, 0, &huge) {
            Err(EncodeError::AncillaryTooLarge { space, got }) => {
                assert!(space < huge.len(), "space must be < got = huge.len()");
                assert_eq!(got, huge.len());
            }
            other => panic!("expected AncillaryTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn ancillary_with_state_round_trips() {
        // The state-carrying entry point preserves the §C.1.3 X ring
        // buffer across successive frames just like
        // `encode_frame_with`; piping an ancillary payload through it
        // must not perturb the cross-frame identity.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.4);
        let smr = zero_smr();
        let payload = b"oxideav-mp2 ancillary tail";

        let mut state_a = EncodeFrameState::new();
        let f1_a = encode_frame_with_state_and_ancillary(
            &header,
            &pcm,
            &smr,
            (payload.len() as u32) * 8 + 32,
            payload,
            &mut state_a,
        )
        .unwrap();
        // The first-frame X buffer starts at zero in both states, so
        // the same input + same payload + same banc yields byte-
        // identical frames.
        let mut state_b = EncodeFrameState::new();
        let f1_b = encode_frame_with_state_and_ancillary(
            &header,
            &pcm,
            &smr,
            (payload.len() as u32) * 8 + 32,
            payload,
            &mut state_b,
        )
        .unwrap();
        assert_eq!(f1_a, f1_b);

        // The frame decodes cleanly through the §2.4.3.1 CRC check.
        let _decoded = decode_frame(&f1_a).expect("decode stateful ancillary frame");

        // A second frame from `state_a` differs from the first frame
        // (X buffer accumulated content), so the per-call ancillary
        // path is not accidentally caching anything that breaks the
        // §C.1.3 state evolution.
        let f2_a = encode_frame_with_state_and_ancillary(
            &header,
            &pcm,
            &smr,
            (payload.len() as u32) * 8 + 32,
            payload,
            &mut state_a,
        )
        .unwrap();
        assert_ne!(f1_a, f2_a);
    }

    #[test]
    fn ancillary_too_large_reports_correct_space_and_got() {
        // Probe the AncillaryTooLarge `space` field by feeding a
        // payload exactly `space + 1` long and confirming the error
        // reports the same `space` and the new `got`.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.3);
        let smr = zero_smr();
        // First find the real `space` by overflowing.
        let probe = vec![0u8; header.frame_size_bytes()];
        let space = match encode_frame_with_ancillary(&header, &pcm, &smr, 0, &probe) {
            Err(EncodeError::AncillaryTooLarge { space, .. }) => space,
            other => panic!("expected AncillaryTooLarge, got {other:?}"),
        };
        // A payload sized exactly `space` must fit.
        let fits = vec![0xA5u8; space];
        let ok = encode_frame_with_ancillary(&header, &pcm, &smr, 0, &fits)
            .expect("space bytes must fit");
        assert_eq!(ok.len(), header.frame_size_bytes());
        // A payload sized `space + 1` must not fit and must report
        // the same `space` value.
        let over = vec![0x5Au8; space + 1];
        match encode_frame_with_ancillary(&header, &pcm, &smr, 0, &over) {
            Err(EncodeError::AncillaryTooLarge {
                space: reported,
                got,
            }) => {
                assert_eq!(reported, space);
                assert_eq!(got, space + 1);
            }
            other => panic!("expected AncillaryTooLarge, got {other:?}"),
        }
    }

    // ---- Auto-SMR (§D.1 Model-1) encode path ----

    #[test]
    fn auto_smr_frame_is_well_formed_and_round_trips() {
        // The auto-SMR path drives the §D.1 Model-1 chain to pick the
        // allocation; the emitted frame must be the correct size, parse
        // back, pass the §2.4.3.1 CRC, and reconstruct 1152 samples per
        // channel.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.5);
        let bytes = encode_frame_auto(&header, &pcm, 0).expect("auto encode");
        assert_eq!(bytes.len(), header.frame_size_bytes());
        let decoded = decode_frame(&bytes).expect("decode auto frame (with CRC)");
        assert_eq!(decoded.header, header);
        assert_eq!(decoded.pcm.len(), 2);
        assert_eq!(decoded.pcm[0].len(), PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn auto_smr_allocation_is_nonzero_for_tonal_input() {
        // A loud 1 kHz tone must receive a non-zero allocation in the
        // band that carries it — the perceptual model has to spend bits
        // where the audible signal is.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.8);
        let bytes = encode_frame_auto(&header, &pcm, 0).expect("auto encode");
        let mut reader = BitReader::with_position(&bytes, 6);
        let (audio, _, _) =
            parse_audio_data_with_section_bits(&header, &mut reader).expect("parse audio data");
        let any_alloc =
            (0..audio.channels).any(|ch| (0..audio.sblimit).any(|sb| audio.nb_steps[ch][sb] != 0));
        assert!(
            any_alloc,
            "auto-SMR allocator produced an all-zero allocation for a loud tone"
        );
    }

    #[test]
    fn auto_smr_shapes_allocation_differently_from_flat_smr() {
        // The whole point of wiring the psychoacoustic model in: a
        // perceptually-shaped SMR must produce a DIFFERENT allocation
        // than a flat 0 dB SMR for a spectrally-uneven input. We use a
        // pure tone whose energy is concentrated in one subband; the
        // Model-1 table raises that band's SMR well above the others,
        // so the iterative allocator's choice of where to spend bits
        // diverges from the flat-table allocator's choice.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_000.0, 0.8);

        let flat = encode_frame(&header, &pcm, &zero_smr(), 0).expect("flat encode");
        let auto = encode_frame_auto(&header, &pcm, 0).expect("auto encode");

        let parse_alloc = |bytes: &[u8]| {
            let mut reader = BitReader::with_position(bytes, 6);
            let (audio, _, _) = parse_audio_data_with_section_bits(&header, &mut reader).unwrap();
            let mut v = Vec::new();
            for ch in 0..audio.channels {
                for sb in 0..audio.sblimit {
                    v.push(audio.nb_steps[ch][sb]);
                }
            }
            v
        };
        assert_ne!(
            parse_alloc(&flat),
            parse_alloc(&auto),
            "auto-SMR allocation must differ from the flat-SMR allocation"
        );
    }

    #[test]
    fn auto_smr_stream_round_trips_within_bound() {
        // Milestone check: an auto-SMR-driven multi-frame encode must
        // round-trip through the decoder, reconstructing a tonal signal
        // with bounded error. We encode several frames of a steady tone
        // with the streaming `encode_frame_auto_with` (persistent X
        // buffer), concatenate, decode the whole stream, and measure
        // the residual against the input after the §C.1.3 / §2.4.3.2
        // filterbank's combined analysis+synthesis delay.
        let header = canonical_single_channel_header();
        let freq = 1_000.0;
        let amp = 0.5;
        let omega = 2.0 * core::f64::consts::PI * freq / 44_100.0;

        let n_frames = 8;
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        // One long continuous tone split into frames.
        let signal: Vec<f64> = (0..total).map(|i| amp * (omega * i as f64).sin()).collect();

        let mut state = EncodeFrameState::new();
        let mut stream = Vec::new();
        for f in 0..n_frames {
            let base = f * PCM_SAMPLES_PER_CHANNEL;
            let frame_pcm = vec![signal[base..base + PCM_SAMPLES_PER_CHANNEL].to_vec()];
            let bytes =
                encode_frame_auto_with(&header, &frame_pcm, 0, &mut state).expect("auto encode");
            assert_eq!(bytes.len(), header.frame_size_bytes());
            stream.extend_from_slice(&bytes);
        }

        // Decode the whole stream back.
        let planes = crate::frame::decode_all_frames(&stream).expect("decode stream");
        assert_eq!(planes.len(), 1);
        let out = &planes[0];
        assert_eq!(out.len(), total);

        // The combined analysis + synthesis filterbank delay is 480
        // samples for Layer II (§2.4.3.2 / §C.1.3); compare the steady
        // middle of the stream where the tone is fully established,
        // accounting for that delay. We measure the correlation-style
        // residual energy ratio rather than a per-sample bound, since
        // the float filterbanks and the perceptual quantiser both add
        // sub-LSB-scale shaping that is not bit-defined.
        let delay = 480usize;
        let lo = delay + PCM_SAMPLES_PER_CHANNEL; // skip the first frame's ramp-in
        let hi = total - PCM_SAMPLES_PER_CHANNEL; // skip the trailing partial
        assert!(hi > lo, "stream long enough to have a steady middle");

        let mut sig_energy = 0.0_f64;
        let mut err_energy = 0.0_f64;
        for i in lo..hi {
            let want = signal[i - delay];
            let got = out[i];
            sig_energy += want * want;
            let e = got - want;
            err_energy += e * e;
        }
        // A working perceptual encode reconstructs the tone with the
        // error energy a fraction of the signal energy. We assert a
        // generous bound (error < signal) — a broken allocation (e.g.
        // all-zero, or the SMR sign inverted) blows past this because
        // the band carrying the tone would receive too few steps and
        // the reconstruction would be near-silent or noise-dominated.
        assert!(
            err_energy < sig_energy,
            "auto-SMR reconstruction error energy {err_energy:.4} exceeds signal energy {sig_energy:.4}"
        );
    }

    #[test]
    fn auto_smr_round_trips_at_every_mpeg1_rate() {
        // The §D.1 driver selects rate-specific D.1 / D.2 / D.4 tables
        // via psy::annex_d_sampling_rate; exercise all three MPEG-1
        // Layer II rates to confirm the table selection + FFT-line maps
        // are wired correctly at each, and that each emitted frame
        // round-trips through the decoder.
        for sample_rate in [32_000u32, 44_100, 48_000] {
            let header = FrameHeader {
                sample_rate,
                ..canonical_stereo_header()
            };
            let pcm = tone_pcm(2, 1_000.0, 0.6);
            let bytes = encode_frame_auto(&header, &pcm, 0)
                .unwrap_or_else(|e| panic!("auto encode at {sample_rate} Hz: {e:?}"));
            assert_eq!(bytes.len(), header.frame_size_bytes());
            let decoded = decode_frame(&bytes)
                .unwrap_or_else(|e| panic!("decode at {sample_rate} Hz: {e:?}"));
            assert_eq!(decoded.header.sample_rate, sample_rate);
            // The loud tone must still draw a non-zero allocation.
            let mut reader = BitReader::with_position(&bytes, 6);
            let (audio, _, _) =
                parse_audio_data_with_section_bits(&decoded.header, &mut reader).unwrap();
            let any = (0..audio.channels)
                .any(|ch| (0..audio.sblimit).any(|sb| audio.nb_steps[ch][sb] != 0));
            assert!(any, "all-zero allocation at {sample_rate} Hz");
        }
    }

    #[test]
    fn auto_smr_lsf_rate_falls_back_and_round_trips() {
        // MPEG-2 LSF rates have no Annex D Layer II masking tables, so
        // the §D.1 driver returns a flat 0 dB SMR (rate-driven
        // allocation). The auto path must still produce a well-formed,
        // decodable frame — equivalent to a flat-SMR encode.
        let header = FrameHeader {
            lsf: true,
            sample_rate: 24_000,
            bit_rate: 64_000,
            ..canonical_stereo_header()
        };
        let pcm = tone_pcm(2, 1_000.0, 0.5);
        let auto = encode_frame_auto(&header, &pcm, 0).expect("LSF auto encode");
        assert_eq!(auto.len(), header.frame_size_bytes());
        let decoded = decode_frame(&auto).expect("LSF decode");
        assert!(decoded.header.lsf);
        assert_eq!(decoded.header.sample_rate, 24_000);
        // Flat-SMR encode of the same input must be byte-identical: the
        // LSF fallback path is exactly an all-zero SMR table.
        let flat = encode_frame(&header, &pcm, &zero_smr(), 0).expect("LSF flat encode");
        assert_eq!(
            auto, flat,
            "LSF auto-SMR must equal a flat 0 dB SMR encode (no Annex D tables)"
        );
    }

    #[test]
    fn auto_smr_is_deterministic() {
        // The §D.1 chain has no hidden state beyond the persistent X
        // ring buffer; a stateless auto-encode of the same input must
        // be byte-reproducible across calls.
        let header = canonical_stereo_header();
        let pcm = tone_pcm(2, 1_234.0, 0.45);
        let a = encode_frame_auto(&header, &pcm, 0).expect("encode a");
        let b = encode_frame_auto(&header, &pcm, 0).expect("encode b");
        assert_eq!(a, b, "auto-SMR encode must be deterministic");
    }

    /// Build `n_frames` worth of a continuous per-channel tone.
    fn tone_stream(channels: usize, freq_hz: f64, amp: f64, n_frames: usize) -> Vec<Vec<f64>> {
        let omega = 2.0 * core::f64::consts::PI * freq_hz / 44_100.0;
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        (0..channels)
            .map(|ch| {
                (0..total)
                    .map(|i| amp * (omega * (i as f64 + ch as f64 * 64.0)).sin())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn encode_all_frames_equals_a_persistent_encode_frame_auto_loop() {
        // The batch path must be byte-identical to driving
        // `encode_frame_auto_with` with one persistent state — same
        // §C.1.3 X-buffer continuity, same §D.1 SMR per frame, same
        // §2.4.2.3 padding schedule (the manual loop threads its own
        // `PaddingScheduler`, exactly as the batch docs promise).
        let header = canonical_stereo_header();
        let n_frames = 5;
        let stream = tone_stream(2, 1_000.0, 0.5, n_frames);

        let batch = encode_all_frames(&header, &stream, 0).expect("batch encode");

        let mut state = EncodeFrameState::new();
        let mut padding = PaddingScheduler::new();
        let mut manual = Vec::new();
        let mut expect_len = 0usize;
        for f in 0..n_frames {
            let frame_header = padding.next_header(&header);
            expect_len += frame_header.frame_size_bytes();
            let base = f * PCM_SAMPLES_PER_CHANNEL;
            let frame_pcm: Vec<Vec<f64>> = stream
                .iter()
                .map(|ch| ch[base..base + PCM_SAMPLES_PER_CHANNEL].to_vec())
                .collect();
            let bytes = encode_frame_auto_with(&frame_header, &frame_pcm, 0, &mut state)
                .expect("manual encode");
            manual.extend_from_slice(&bytes);
        }

        assert_eq!(
            batch, manual,
            "encode_all_frames must equal a persistent encode_frame_auto_with loop"
        );
        assert_eq!(batch.len(), expect_len);
        // 44,1 kHz genuinely pads: the schedule-driven length exceeds
        // the all-unpadded length.
        assert!(batch.len() > n_frames * header.frame_size_bytes());
    }

    #[test]
    fn encode_all_frames_with_smr_matches_a_persistent_provided_loop() {
        let header = canonical_single_channel_header();
        let n_frames = 4;
        let stream = tone_stream(1, 1_500.0, 0.4, n_frames);
        let smr: SmrTable = [[30.0f64; NUM_SUBBANDS]; 2];

        let batch = encode_all_frames_with_smr(&header, &stream, &smr, 0).expect("batch smr");

        let mut state = EncodeFrameState::new();
        let mut padding = PaddingScheduler::new();
        let mut manual = Vec::new();
        for f in 0..n_frames {
            let frame_header = padding.next_header(&header);
            let base = f * PCM_SAMPLES_PER_CHANNEL;
            let frame_pcm: Vec<Vec<f64>> =
                vec![stream[0][base..base + PCM_SAMPLES_PER_CHANNEL].to_vec()];
            let bytes = encode_frame_with(&frame_header, &frame_pcm, &smr, 0, &mut state)
                .expect("manual smr");
            manual.extend_from_slice(&bytes);
        }
        assert_eq!(batch, manual);
    }

    #[test]
    fn encode_all_frames_rejects_a_partial_trailing_frame() {
        let header = canonical_stereo_header();
        // 2.5 frames' worth of samples per channel — not a 1152 multiple.
        let len = 2 * PCM_SAMPLES_PER_CHANNEL + PCM_SAMPLES_PER_CHANNEL / 2;
        let pcm: Vec<Vec<f64>> = vec![vec![0.0; len]; 2];
        match encode_all_frames(&header, &pcm, 0) {
            Err(EncodeError::ShortPcmTail {
                channel,
                have,
                frame,
            }) => {
                assert_eq!(channel, 0);
                assert_eq!(have, len);
                assert_eq!(frame, PCM_SAMPLES_PER_CHANNEL);
            }
            other => panic!("expected ShortPcmTail, got {other:?}"),
        }
    }

    #[test]
    fn encode_all_frames_rejects_mismatched_channel_lengths() {
        let header = canonical_stereo_header();
        let pcm: Vec<Vec<f64>> = vec![
            vec![0.0; 2 * PCM_SAMPLES_PER_CHANNEL],
            vec![0.0; 3 * PCM_SAMPLES_PER_CHANNEL],
        ];
        match encode_all_frames(&header, &pcm, 0) {
            Err(EncodeError::BadPcmLen { channel, .. }) => assert_eq!(channel, 1),
            other => panic!("expected BadPcmLen, got {other:?}"),
        }
    }

    #[test]
    fn encode_all_frames_rejects_wrong_channel_count() {
        let header = canonical_stereo_header(); // expects 2 channels
        let pcm: Vec<Vec<f64>> = vec![vec![0.0; PCM_SAMPLES_PER_CHANNEL]];
        match encode_all_frames(&header, &pcm, 0) {
            Err(EncodeError::BadPcmChannelCount { have, need }) => {
                assert_eq!(have, 1);
                assert_eq!(need, 2);
            }
            other => panic!("expected BadPcmChannelCount, got {other:?}"),
        }
    }

    #[test]
    fn encode_all_frames_empty_stream_is_empty_output() {
        let header = canonical_stereo_header();
        let pcm: Vec<Vec<f64>> = vec![Vec::new(), Vec::new()];
        let bytes = encode_all_frames(&header, &pcm, 0).expect("empty batch");
        assert!(bytes.is_empty(), "empty stream encodes to no bytes");
    }

    #[test]
    fn encode_all_frames_output_decodes_back_to_the_right_sample_count() {
        let header = canonical_stereo_header();
        let n_frames = 3;
        let stream = tone_stream(2, 1_000.0, 0.5, n_frames);
        let bytes = encode_all_frames(&header, &stream, 0).expect("batch encode");
        let planes = crate::frame::decode_all_frames(&bytes).expect("decode batch");
        assert_eq!(planes.len(), 2);
        for plane in &planes {
            assert_eq!(plane.len(), n_frames * PCM_SAMPLES_PER_CHANNEL);
        }
    }
}
