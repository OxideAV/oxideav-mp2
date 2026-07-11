//! `oxideav_core::Encoder` wiring for MPEG-1 Audio Layer II (MP2).
//!
//! Round 371 — registry encoder trait surface. The crate's
//! [`encode_frame_auto_with`](crate::encode_frame_auto_with) primitive
//! already encodes one Layer II frame from per-channel PCM end-to-end
//! (§C.1.3 analysis filterbank → §D.1 Model-1 auto-SMR → §C.1.5.2.7 bit
//! allocation → §2.4.1.6 audio-data + §2.4.3.3.4 sample codewords →
//! §2.4.3.1 CRC). This module adapts that path into the framework's
//! frame-in / packet-out [`oxideav_core::Encoder`] trait so muxers can
//! emit Layer II streams via the registry, the encode-side dual of
//! [`crate::codec_decoder::Mp2CoreDecoder`].
//!
//! ## Trait-API adaptation
//!
//! The framework trait is *frame-in, packet-out*:
//!
//! * [`send_frame`](Encoder::send_frame) accepts one [`Frame::Audio`]
//!   carrying planar **S16** PCM (`data.len() == channels`, each plane
//!   `samples * 2` little-endian bytes). Samples are accumulated into a
//!   per-channel ring; every time [`PCM_SAMPLES_PER_CHANNEL`] (= 1152,
//!   the §2.4.2.1 Layer II constant) samples per channel are available,
//!   one Layer II frame is encoded and queued.
//! * [`receive_packet`](Encoder::receive_packet) pops one queued Layer
//!   II frame as a [`Packet`] whose `data` is exactly
//!   [`FrameHeader::frame_size_bytes`] bytes (header + optional CRC +
//!   audio data), the same framing
//!   [`crate::frame::decode_all_frames`] / [`Mp2CoreDecoder`] consume.
//! * [`flush`](Encoder::flush) zero-pads any partial trailing frame up
//!   to the 1152-sample boundary and encodes it, then drains. (A
//!   partial Layer II frame has no defined encoding; zero-padding is the
//!   only way to terminate a stream whose length is not a multiple of
//!   1152.)
//!
//! ## Input format
//!
//! Planar S16: the inverse of [`Mp2CoreDecoder`]'s output mapping. Each
//! little-endian `i16` is divided by `2^15 = 32768` to land in the
//! §2.4.3.4.7.1 nominal `[-1.0, +1.0]` float range the analysis
//! filterbank consumes.
//!
//! ## Stream configuration
//!
//! The encoder's [`FrameHeader`] is fixed at construction from
//! [`CodecParameters`]: `sample_rate` (one of the six Layer II rates),
//! `channels` (1 → `SingleChannel`, 2 → `Stereo`), and `bit_rate`. When
//! `bit_rate` is absent a per-rate default is chosen
//! ([`default_bitrate_bps`]); when present it is validated against the
//! §2.4.2.3 bitrate / mode matrix and the per-rate allocation-table
//! coverage (an unrepresentable combination is rejected at build time).

use std::collections::VecDeque;

use oxideav_core::{
    AudioFrame, CodecId, CodecParameters, CodecTag, Encoder, Error, Frame, Packet, Result,
    SampleFormat, TimeBase,
};

use crate::bitalloc::select_table;
use crate::codec_decoder::{CODEC_ID_STR, WAVE_FORMAT_MPEG};
use crate::encoder_frame::{
    encode_frame_auto_js_model2, encode_frame_auto_js_with, encode_frame_auto_model2,
    encode_frame_auto_with, EncodeFrameState,
};
use crate::frame::PCM_SAMPLES_PER_CHANNEL;
use crate::header::{
    is_layer2_bitrate_mode_allowed, Emphasis, FrameHeader, Mode, ModeExtension, PaddingScheduler,
};

/// Which Annex D psychoacoustic model drives the auto-SMR allocation.
///
/// Selected by the `psymodel` codec option (`"model1"` / `"model2"`);
/// defaults to Model 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PsyModel {
    /// §D.1 Model 1 (tonal / non-tonal masker labelling).
    Model1,
    /// §D.2 Model 2 (unpredictability-driven, twice-per-frame Layer II).
    Model2,
}

/// Full-scale divisor mapping S16 → the §2.4.3.4.7.1 `[-1, +1]` float
/// range. `2^15` so `i16::MIN ↦ -1.0` exactly (the MSB-is-−1 convention
/// the requantizer round-trips), matching the decoder's `* 32768.0`.
const FULL_SCALE: f64 = 32768.0;

/// Pick a per-rate default total bitrate (bit/s) for `mode` when the
/// caller did not supply one.
///
/// The picks are middle-of-the-ladder rates that pass both the
/// §2.4.2.3 bitrate / mode matrix and [`select_table`]'s per-rate
/// allocation-table coverage:
///
/// * MPEG-1 (32 / 44,1 / 48 kHz): 192 kbit/s stereo (96 kbit/s/ch),
///   128 kbit/s mono.
/// * MPEG-2 LSF (16 / 22,05 / 24 kHz): 64 kbit/s either way (the LSF
///   ladder centres lower; 64 kbit/s is valid at every LSF rate for
///   both modes and uses the §2.4.3.1 Table B.1 allocation table).
#[must_use]
pub fn default_bitrate_bps(sample_rate: u32, mode: Mode) -> u32 {
    let lsf = matches!(sample_rate, 16_000 | 22_050 | 24_000);
    if lsf {
        64_000
    } else {
        match mode {
            Mode::SingleChannel => 128_000,
            _ => 192_000,
        }
    }
}

/// Map a channel count + optional `mode` override to the Layer II
/// [`Mode`] the encoder emits.
///
/// 1 channel → always `SingleChannel`. 2 channels → `Stereo` by default,
/// or `JointStereo` / `DualChannel` when the `mode` codec option selects
/// it. A `mode` override that contradicts the channel count (e.g.
/// `single_channel` for a 2-channel frame) is rejected.
fn mode_for_channels(channels: u16, mode_opt: Option<&str>) -> Result<Mode> {
    match channels {
        1 => match mode_opt {
            None | Some("single_channel") => Ok(Mode::SingleChannel),
            Some(other) => Err(Error::invalid(format!(
                "oxideav-mp2: mode={other:?} is incompatible with a 1-channel stream \
                 (only single_channel)"
            ))),
        },
        2 => match mode_opt {
            None | Some("stereo") => Ok(Mode::Stereo),
            Some("joint_stereo") => Ok(Mode::JointStereo),
            Some("dual_channel") => Ok(Mode::DualChannel),
            Some(other) => Err(Error::invalid(format!(
                "oxideav-mp2: mode={other:?} not recognised for a 2-channel stream \
                 (stereo / joint_stereo / dual_channel)"
            ))),
        },
        _ => Err(Error::invalid(format!(
            "oxideav-mp2: encoder supports 1 or 2 channels (channels={channels})"
        ))),
    }
}

/// The joint-stereo intensity-bound policy selected by the `bound`
/// codec option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundChoice {
    /// A fixed §2.4.2.3 `mode_extension` bound for every frame.
    Fixed(ModeExtension),
    /// Annex G.1 demand-driven per-frame selection: full `Stereo` when
    /// the frame's required bits fit the budget, else the widest
    /// `JointStereo` bound that fits (16 / 12 / 8 / 4).
    Auto,
}

/// Parse the `bound` (joint-stereo intensity bound) codec option. Only
/// meaningful when `mode == JointStereo`; `"4" / "8" / "12" / "16"`
/// map to the four §2.4.2.3 bounds (default `Bound4`), and `"auto"`
/// selects the Annex G.1 demand-driven per-frame policy.
fn mode_extension_opt(s: Option<&str>) -> Result<BoundChoice> {
    match s {
        None | Some("4") => Ok(BoundChoice::Fixed(ModeExtension::Bound4)),
        Some("8") => Ok(BoundChoice::Fixed(ModeExtension::Bound8)),
        Some("12") => Ok(BoundChoice::Fixed(ModeExtension::Bound12)),
        Some("16") => Ok(BoundChoice::Fixed(ModeExtension::Bound16)),
        Some("auto") => Ok(BoundChoice::Auto),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: bound={other:?} not a valid intensity bound (4 / 8 / 12 / 16 / auto)"
        ))),
    }
}

/// Parse the `psymodel` codec option (`"model1"` / `"model2"`), default
/// Model 1.
fn psymodel_opt(s: Option<&str>) -> Result<PsyModel> {
    match s {
        None | Some("model1") => Ok(PsyModel::Model1),
        Some("model2") => Ok(PsyModel::Model2),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: psymodel={other:?} not recognised (model1 / model2)"
        ))),
    }
}

/// Parse the `freeformat` option: when `"true"` / `"1"`, the encoder emits
/// §2.4.2.3 free-format frames (`bitrate_index == '0000'`) at the
/// configured constant bitrate. Absent / `"false"` / `"0"` keeps the
/// standard signalled-bitrate framing.
fn freeformat_opt(s: Option<&str>) -> Result<bool> {
    match s {
        None | Some("false") | Some("0") => Ok(false),
        Some("true") | Some("1") => Ok(true),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: freeformat={other:?} not recognised (true / false)"
        ))),
    }
}

/// Parse the `crc` option: when `"true"` / `"1"`, every emitted frame
/// carries the §2.4.1.4 / §2.4.3.1 16-bit CRC word (header
/// `protection_bit = '0'` — the §2.4.2.3 inverted convention) computed
/// over the Annex B Table B.5 protected fields, so a decoder can detect
/// transmission errors in the bit-allocation / scfsi section. Absent /
/// `"false"` / `"0"` emits unprotected frames (`protection_bit = '1'`).
fn crc_opt(s: Option<&str>) -> Result<bool> {
    match s {
        None | Some("false") | Some("0") => Ok(false),
        Some("true") | Some("1") => Ok(true),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: crc={other:?} not recognised (true / false)"
        ))),
    }
}

/// Parse a boolean `params.options` value (`"true"` / `"1"` → true,
/// `"false"` / `"0"` / absent → `default`), rejecting anything else.
/// Used for the §2.4.2.3 header metadata flags whose value has no
/// signal-processing effect (`copyright` / `original` / `private`).
fn bool_opt(s: Option<&str>, name: &str, default: bool) -> Result<bool> {
    match s {
        None => Ok(default),
        Some("true") | Some("1") => Ok(true),
        Some("false") | Some("0") => Ok(false),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: {name}={other:?} not recognised (true / false)"
        ))),
    }
}

/// Parse the `emphasis` option: `"none"` (default) delivers PCM
/// unaltered; `"50/15"` (also accepted as `"5015"` / `"50_15"`) selects
/// the §2.4.2.4 50/15 µs pre-emphasis and `"j17"` (also `"j.17"` /
/// `"ccitt_j17"`) the CCITT J.17 pre-emphasis (applied at encode,
/// undone by the decoder — see [`crate::deemphasis`] and
/// [`crate::j17`]). The reserved `'10'` code is not offered.
fn emphasis_opt(s: Option<&str>) -> Result<Emphasis> {
    match s {
        None | Some("none") | Some("0") => Ok(Emphasis::None),
        Some("50/15") | Some("5015") | Some("50_15") => Ok(Emphasis::FiftyFifteen),
        Some("j17") | Some("j.17") | Some("ccitt_j17") => Ok(Emphasis::CcittJ17),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: emphasis={other:?} not recognised (none / 50/15 / j17)"
        ))),
    }
}

/// Build the fixed per-stream [`FrameHeader`] from encoder parameters,
/// validating the rate / channel / bitrate combination up front.
/// `crc = true` sets `protection_bit = '0'` (the §2.4.2.3 inverted
/// convention), making the frame writer emit + patch the §2.4.1.4
/// 16-bit CRC word after the header.
fn build_header(
    sample_rate: u32,
    mode: Mode,
    mode_extension: ModeExtension,
    bit_rate: u32,
    crc: bool,
    emphasis: Emphasis,
) -> Result<FrameHeader> {
    let lsf = match sample_rate {
        32_000 | 44_100 | 48_000 => false,
        16_000 | 22_050 | 24_000 => true,
        other => {
            return Err(Error::invalid(format!(
                "oxideav-mp2: encoder sample_rate {other} is not a Layer II rate \
                 (MPEG-1: 32000/44100/48000, LSF: 16000/22050/24000)"
            )))
        }
    };

    // §2.4.2.3 bitrate / mode matrix (MPEG-1 only; LSF imposes no
    // mode restriction).
    if !lsf && !is_layer2_bitrate_mode_allowed(bit_rate, mode) {
        return Err(Error::invalid(format!(
            "oxideav-mp2: bit_rate {} kbit/s is not allowed for mode {mode:?} \
             at {sample_rate} Hz (§2.4.2.3 matrix)",
            bit_rate / 1000
        )));
    }

    let header = FrameHeader {
        lsf,
        // §2.4.2.3 inverted convention: '1' = no CRC, '0' = the
        // §2.4.1.4 16-bit CRC word follows the header.
        protection_bit: !crc,
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode,
        mode_extension,
        copyright: false,
        original: true,
        emphasis,
    };

    // The (rate, per-channel bitrate) pair must select a defined
    // §2.4.3.1 / Annex B allocation table, else encode would fail per
    // frame.
    if select_table(&header).is_none() {
        return Err(Error::invalid(format!(
            "oxideav-mp2: no Layer II allocation table for {} kbit/s at {sample_rate} Hz \
             (mode {mode:?}); pick a bitrate the §2.4.3.1 ladder covers",
            bit_rate / 1000
        )));
    }

    Ok(header)
}

/// Build a boxed MPEG-1 / MPEG-2 LSF Audio Layer II [`Encoder`] from
/// `params`.
///
/// Reads `sample_rate` (required — one of the six Layer II rates),
/// `channels` (1 or 2; default 2), and `bit_rate` (optional — a per-rate
/// default is chosen when absent).
///
/// # Codec options
///
/// The following `params.options` keys tune the encode (all optional):
///
/// * `mode` — for a 2-channel stream: `"stereo"` (default),
///   `"joint_stereo"` (intensity stereo), or `"dual_channel"` (two
///   independent mono programmes). Ignored / must be `"single_channel"`
///   for 1 channel.
/// * `bound` — for `joint_stereo`: the §2.4.2.3 intensity bound,
///   `"4"` (default) / `"8"` / `"12"` / `"16"`, or `"auto"` for the
///   Annex G.1 demand-driven per-frame policy (each frame is emitted
///   as full `Stereo` when its required bits fit the budget, else as
///   `JointStereo` with the widest intensity bound that fits).
/// * `psymodel` — which Annex D model drives the auto-SMR allocation:
///   `"model1"` (§D.1, default) or `"model2"` (§D.2).
/// * `freeformat` — `"true"` / `"1"` emits §2.4.2.3 free-format frames
///   (`bitrate_index == '0000'`) at the configured constant bitrate;
///   default `"false"` keeps the standard signalled-bitrate framing. The
///   output decodes via `decode_free_format_stream` or the registry
///   decoder's free-format packet path.
/// * `crc` — `"true"` / `"1"` emits the §2.4.1.4 / §2.4.3.1 16-bit CRC
///   word in every frame (header `protection_bit = '0'`), protecting
///   the Annex B Table B.5 fields (header second half + bit-allocation
///   + scfsi); default `"false"` emits unprotected frames.
/// * `emphasis` — `"50/15"` applies the §2.4.2.4 50/15 µs pre-emphasis
///   and `"j17"` the CCITT J.17 pre-emphasis (see [`crate::j17`])
///   before quantization and signals the header field, so a decoder
///   undoes it via de-emphasis; default `"none"` encodes the PCM
///   unaltered. (The reserved `'10'` code is not offered.)
/// * `copyright` / `original` / `private` — the §2.4.2.3 header
///   metadata flags (`"true"` / `"false"`). They carry no
///   signal-processing effect and are round-tripped verbatim on decode;
///   defaults are `copyright=false`, `original=true`, `private=false`.
///
/// # Errors
///
/// * `channels` not 1 or 2 — the §2.4.2.3 mode field encodes at most
///   two channels.
/// * `sample_rate` absent or not a Layer II rate.
/// * `bit_rate` (if supplied) violates the §2.4.2.3 matrix or has no
///   §2.4.3.1 allocation table for the rate.
/// * an unrecognised `mode` / `bound` / `psymodel` option.
pub fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    let channels = params.channels.unwrap_or(2);
    let mode = mode_for_channels(channels, params.options.get("mode"))?;
    let bound_choice = mode_extension_opt(params.options.get("bound"))?;
    let psymodel = psymodel_opt(params.options.get("psymodel"))?;
    let freeformat = freeformat_opt(params.options.get("freeformat"))?;
    let crc = crc_opt(params.options.get("crc"))?;
    let emphasis = emphasis_opt(params.options.get("emphasis"))?;
    // §2.4.2.3 header metadata flags (no signal effect; round-tripped).
    let copyright = bool_opt(params.options.get("copyright"), "copyright", false)?;
    let original = bool_opt(params.options.get("original"), "original", true)?;
    let private_bit = bool_opt(params.options.get("private"), "private", false)?;

    // The Annex G.1 demand-driven policy needs a two-channel joint
    // stream to select over (Stereo vs the four intensity bounds).
    let (mode_extension, auto_bound) = match bound_choice {
        BoundChoice::Fixed(ext) => (ext, false),
        BoundChoice::Auto => {
            if mode != Mode::JointStereo {
                return Err(Error::invalid(
                    "oxideav-mp2: bound=auto requires mode=joint_stereo",
                ));
            }
            (ModeExtension::Bound4, true)
        }
    };

    let Some(sample_rate) = params.sample_rate else {
        return Err(Error::invalid(
            "oxideav-mp2: encoder requires params.sample_rate (a Layer II rate)",
        ));
    };

    let bit_rate = match params.bit_rate {
        Some(b) => u32::try_from(b).map_err(|_| {
            Error::invalid(format!(
                "oxideav-mp2: bit_rate {b} too large for a Layer II header"
            ))
        })?,
        None => default_bitrate_bps(sample_rate, mode),
    };

    let mut header = build_header(sample_rate, mode, mode_extension, bit_rate, crc, emphasis)?;
    // The §2.4.2.3 metadata flags do not affect allocation-table
    // selection, so they are applied after the validating build.
    header.copyright = copyright;
    header.original = original;
    header.private_bit = private_bit;

    // §2.4.2.3 free-format conformance: the emitted frames are laid out
    // with the signalled rate's Annex B table, but a conforming decoder
    // reads a free-format frame with the table fixed by the sampling
    // frequency alone (Table 3-B.2a at 48 kHz / 3-B.2b at 44,1 & 32 kHz,
    // per the table headers; LSF's single Table B.1 always coincides).
    // Reject a configuration whose two tables differ — the rewrite would
    // be well-formed but decode to garbage on every conforming decoder.
    if freeformat {
        let mut ff_probe = header;
        ff_probe.bit_rate = 0; // free-format sentinel
        if select_table(&header) != select_table(&ff_probe) {
            return Err(Error::invalid(format!(
                "oxideav-mp2: freeformat=true at {} kbit/s / {sample_rate} Hz would lay frames \
                 out with allocation table {:?}, but conforming decoders read free format with \
                 {:?}; use >= 56 kbit/s per channel at 48000 Hz or >= 96 kbit/s per channel at \
                 44100/32000 Hz (any LSF rate is fine)",
                bit_rate / 1000,
                select_table(&header),
                select_table(&ff_probe),
            )));
        }
    }

    let mut out_params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    out_params.sample_rate = Some(sample_rate);
    out_params.channels = Some(channels);
    out_params.sample_format = Some(SampleFormat::S16);
    out_params.bit_rate = Some(u64::from(bit_rate));
    out_params.tag = Some(CodecTag::wave_format(WAVE_FORMAT_MPEG));

    Ok(Box::new(Mp2CoreEncoder::new(
        CodecId::new(CODEC_ID_STR),
        out_params,
        header,
        psymodel,
        freeformat,
        auto_bound,
    )))
}

/// Frame-to-packet adaptor that wraps the
/// [`encode_frame_auto_with`](crate::encode_frame_auto_with) primitive
/// in the framework [`Encoder`] trait.
///
/// State carried across frames:
///
/// * `enc_state` — [`EncodeFrameState`] threads the §C.1.3 analysis
///   filterbank's per-channel X ring buffer across successive frames.
/// * `pending_pcm` — per-channel f64 accumulator; a Layer II frame is
///   emitted every time it reaches [`PCM_SAMPLES_PER_CHANNEL`].
/// * `pending_packets` — queue of encoded Layer II frames awaiting
///   [`receive_packet`].
/// * `samples_emitted` — running per-channel sample count, used to
///   stamp each packet's `pts` (in samples, the encoder's natural time
///   base).
pub struct Mp2CoreEncoder {
    codec_id: CodecId,
    output: CodecParameters,
    header: FrameHeader,
    channels: usize,
    psymodel: PsyModel,
    /// When true, emit §2.4.2.3 free-format frames (bitrate_index '0000')
    /// at the configured constant bitrate.
    freeformat: bool,
    /// §2.4.2.3 padding-bit rate control (the spec's `rest`/`dif`
    /// accumulator) — pads frames at the fractional rates
    /// (44,1 / 22,05 kHz) so the emitted stream's mean bitrate matches
    /// the signalled value.
    padding: PaddingScheduler,
    /// Annex G.1 demand-driven per-frame stereo-coding selection
    /// (`bound=auto`): each frame is emitted as full `Stereo` when its
    /// required bits fit the budget, else as `JointStereo` with the
    /// widest intensity bound that fits.
    auto_bound: bool,
    enc_state: EncodeFrameState,
    pending_pcm: Vec<Vec<f64>>,
    pending_packets: VecDeque<Packet>,
    samples_emitted: i64,
    flushed: bool,
}

impl std::fmt::Debug for Mp2CoreEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mp2CoreEncoder")
            .field("codec_id", &self.codec_id)
            .field("channels", &self.channels)
            .field(
                "pending_samples",
                &self.pending_pcm.first().map_or(0, Vec::len),
            )
            .field("pending_packets", &self.pending_packets.len())
            .field("flushed", &self.flushed)
            .finish()
    }
}

impl Mp2CoreEncoder {
    fn new(
        codec_id: CodecId,
        output: CodecParameters,
        header: FrameHeader,
        psymodel: PsyModel,
        freeformat: bool,
        auto_bound: bool,
    ) -> Self {
        let channels = header.channels();
        Self {
            codec_id,
            output,
            header,
            channels,
            psymodel,
            freeformat,
            auto_bound,
            padding: PaddingScheduler::new(),
            enc_state: EncodeFrameState::new(),
            pending_pcm: vec![Vec::new(); channels],
            pending_packets: VecDeque::new(),
            samples_emitted: 0,
            flushed: false,
        }
    }

    /// Decode one planar-S16 plane into f64 `[-1, +1]` samples.
    fn s16_le_to_f64(plane: &[u8]) -> Vec<f64> {
        plane
            .chunks_exact(2)
            .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])) / FULL_SCALE)
            .collect()
    }

    /// Drain every complete 1152-sample frame currently buffered into
    /// `pending_packets`.
    fn drain_complete_frames(&mut self) -> Result<()> {
        while self.pending_pcm[0].len() >= PCM_SAMPLES_PER_CHANNEL {
            let mut frame_pcm: Vec<Vec<f64>> = Vec::with_capacity(self.channels);
            for ch in 0..self.channels {
                let rest = self.pending_pcm[ch].split_off(PCM_SAMPLES_PER_CHANNEL);
                let frame = std::mem::replace(&mut self.pending_pcm[ch], rest);
                frame_pcm.push(frame);
            }
            self.encode_one(frame_pcm)?;
        }
        Ok(())
    }

    /// Encode one exactly-1152-sample-per-channel PCM frame and queue
    /// the resulting Layer II packet.
    fn encode_one(&mut self, frame_pcm: Vec<Vec<f64>>) -> Result<()> {
        // §2.4.2.3 padding-bit rate control: at the fractional rates
        // (44,1 / 22,05 kHz) the scheduler interleaves padded
        // `N+1`-slot frames so the mean bitrate matches the signalled
        // value; everywhere else it never fires.
        let frame_header = self.padding.next_header(&self.header);
        let mut bytes = match (self.psymodel, self.auto_bound) {
            (PsyModel::Model1, false) => {
                encode_frame_auto_with(&frame_header, &frame_pcm, 0, &mut self.enc_state)
            }
            (PsyModel::Model2, false) => {
                encode_frame_auto_model2(&frame_header, &frame_pcm, 0, &mut self.enc_state)
            }
            // Annex G.1 demand-driven per-frame stereo-coding choice.
            (PsyModel::Model1, true) => {
                encode_frame_auto_js_with(&frame_header, &frame_pcm, 0, &mut self.enc_state)
            }
            (PsyModel::Model2, true) => {
                encode_frame_auto_js_model2(&frame_header, &frame_pcm, 0, &mut self.enc_state)
            }
        }
        .map_err(|e| Error::other(format!("oxideav-mp2: encode_frame: {e}")))?;
        // §2.4.2.3 free-format: clear the frame's bitrate_index nibble to
        // '0000'. The §2.4.3.1 free-format frame size at this ladder
        // bitrate is byte-identical to the standard frame's size, so the
        // payload and frame boundary are untouched; the constant bitrate is
        // recoverable on decode from the frame size.
        if self.freeformat {
            let frame_size = bytes.len();
            crate::freeformat::rewrite_to_free_format(&mut bytes, frame_size);
        }
        let mut packet = Packet::new(
            0,
            TimeBase::new(1, i64::from(self.header.sample_rate)),
            bytes,
        );
        packet.pts = Some(self.samples_emitted);
        packet.dts = Some(self.samples_emitted);
        packet.duration = Some(PCM_SAMPLES_PER_CHANNEL as i64);
        packet.flags.keyframe = true; // every Layer II frame is independently decodable
        self.pending_packets.push_back(packet);
        self.samples_emitted += PCM_SAMPLES_PER_CHANNEL as i64;
        Ok(())
    }
}

impl Encoder for Mp2CoreEncoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn output_params(&self) -> &CodecParameters {
        &self.output
    }

    fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        if self.flushed {
            return Err(Error::other("oxideav-mp2: cannot send_frame after flush"));
        }
        let Frame::Audio(AudioFrame { data, .. }) = frame else {
            return Err(Error::invalid(
                "oxideav-mp2: encoder only accepts Frame::Audio",
            ));
        };
        if data.len() != self.channels {
            return Err(Error::invalid(format!(
                "oxideav-mp2: frame has {} planes, encoder configured for {} channels",
                data.len(),
                self.channels
            )));
        }
        // Each plane must hold a whole number of S16 samples and all
        // planes must carry the same count (planar layout).
        let mut plane_samples: Option<usize> = None;
        for (ch, plane) in data.iter().enumerate() {
            if plane.len() % 2 != 0 {
                return Err(Error::invalid(format!(
                    "oxideav-mp2: S16 plane {ch} has odd byte length {}",
                    plane.len()
                )));
            }
            let n = plane.len() / 2;
            match plane_samples {
                None => plane_samples = Some(n),
                Some(prev) if prev != n => {
                    return Err(Error::invalid(format!(
                        "oxideav-mp2: plane {ch} has {n} samples, expected {prev} (planar)"
                    )))
                }
                Some(_) => {}
            }
        }
        for (ch, plane) in data.iter().enumerate() {
            self.pending_pcm[ch].extend(Self::s16_le_to_f64(plane));
        }
        self.drain_complete_frames()
    }

    fn receive_packet(&mut self) -> Result<Packet> {
        if let Some(pkt) = self.pending_packets.pop_front() {
            return Ok(pkt);
        }
        if self.flushed {
            return Err(Error::Eof);
        }
        Err(Error::NeedMore)
    }

    fn flush(&mut self) -> Result<()> {
        if self.flushed {
            return Ok(());
        }
        // Zero-pad any partial trailing frame up to 1152 samples per
        // channel and encode it — a partial Layer II frame has no
        // defined encoding, so termination requires padding.
        if !self.pending_pcm[0].is_empty() {
            let mut frame_pcm: Vec<Vec<f64>> = Vec::with_capacity(self.channels);
            for ch in 0..self.channels {
                let mut frame = std::mem::take(&mut self.pending_pcm[ch]);
                frame.resize(PCM_SAMPLES_PER_CHANNEL, 0.0);
                frame_pcm.push(frame);
            }
            self.encode_one(frame_pcm)?;
        }
        self.flushed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::decode_all_frames;
    use oxideav_core::{CodecId, CodecParameters};

    fn params(sample_rate: u32, channels: u16, bit_rate: Option<u64>) -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        p.sample_rate = Some(sample_rate);
        p.channels = Some(channels);
        p.bit_rate = bit_rate;
        p
    }

    /// One S16 plane of `n` samples of a sine at `freq_hz`.
    fn tone_plane(n: usize, freq_hz: f64, sample_rate: u32, amp: f64) -> Vec<u8> {
        let omega = 2.0 * std::f64::consts::PI * freq_hz / f64::from(sample_rate);
        let mut bytes = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = (amp * (omega * i as f64).sin() * FULL_SCALE)
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn make_encoder_rejects_bad_channel_count() {
        assert!(make_encoder(&params(44_100, 3, None)).is_err());
        assert!(make_encoder(&params(44_100, 0, None)).is_err());
    }

    #[test]
    fn make_encoder_requires_a_sample_rate() {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        p.channels = Some(2);
        assert!(make_encoder(&p).is_err());
    }

    #[test]
    fn make_encoder_rejects_non_layer2_sample_rate() {
        assert!(make_encoder(&params(11_025, 2, None)).is_err());
        assert!(make_encoder(&params(8_000, 1, None)).is_err());
    }

    #[test]
    fn make_encoder_rejects_bitrate_mode_matrix_violation() {
        // 320 kbit/s is forbidden for single_channel (§2.4.2.3 matrix).
        assert!(make_encoder(&params(44_100, 1, Some(320_000))).is_err());
        // 32 kbit/s total is single-channel-only.
        assert!(make_encoder(&params(44_100, 2, Some(32_000))).is_err());
    }

    #[test]
    fn make_encoder_accepts_defaults_for_every_layer2_rate() {
        for (sr, ch) in [
            (32_000, 1),
            (44_100, 2),
            (48_000, 2),
            (16_000, 1),
            (22_050, 2),
            (24_000, 2),
        ] {
            let enc = make_encoder(&params(sr, ch, None))
                .unwrap_or_else(|e| panic!("default encoder at {sr} Hz / {ch}ch: {e:?}"));
            assert_eq!(enc.output_params().sample_rate, Some(sr));
            assert_eq!(enc.output_params().channels, Some(ch));
            assert_eq!(enc.output_params().sample_format, Some(SampleFormat::S16));
        }
    }

    /// Run a 4-frame tone through an encoder built from `p`, returning
    /// the decoded planes. Drives the full send → flush → receive loop.
    fn encode_decode_through(p: &CodecParameters, sample_rate: u32) -> Vec<Vec<f64>> {
        let channels = p.channels.unwrap_or(2) as usize;
        let mut enc = make_encoder(p).expect("make_encoder");
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let data: Vec<Vec<u8>> = (0..channels)
            .map(|_| tone_plane(n, 1_000.0, sample_rate, 0.5))
            .collect();
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data,
        }))
        .unwrap();
        enc.flush().unwrap();
        let mut stream = Vec::new();
        loop {
            match enc.receive_packet() {
                Ok(pk) => stream.extend_from_slice(&pk.data),
                Err(Error::Eof) => break,
                Err(e) => panic!("receive_packet: {e:?}"),
            }
        }
        decode_all_frames(&stream).expect("decode")
    }

    #[test]
    fn mode_option_selects_joint_stereo_dual_channel_and_stereo() {
        for mode in ["stereo", "joint_stereo", "dual_channel"] {
            let mut p = params(44_100, 2, Some(192_000));
            p.options.insert("mode", mode);
            let planes = encode_decode_through(&p, 44_100);
            assert_eq!(planes.len(), 2, "mode={mode}: stereo decode");
            assert_eq!(
                planes[0].len(),
                4 * PCM_SAMPLES_PER_CHANNEL,
                "mode={mode}: sample count"
            );
        }
    }

    #[test]
    fn joint_stereo_bound_option_round_trips_at_every_bound() {
        for bound in ["4", "8", "12", "16"] {
            let mut p = params(44_100, 2, Some(192_000));
            p.options.insert("mode", "joint_stereo");
            p.options.insert("bound", bound);
            let planes = encode_decode_through(&p, 44_100);
            assert_eq!(
                planes[0].len(),
                4 * PCM_SAMPLES_PER_CHANNEL,
                "bound={bound}"
            );
        }
    }

    #[test]
    fn psymodel_option_selects_model2_and_differs_from_model1() {
        let mut p1 = params(44_100, 2, Some(128_000));
        p1.options.insert("psymodel", "model1");
        let mut p2 = params(44_100, 2, Some(128_000));
        p2.options.insert("psymodel", "model2");

        // Both decode to a valid stream of the right shape.
        let d1 = encode_decode_through(&p1, 44_100);
        let d2 = encode_decode_through(&p2, 44_100);
        assert_eq!(d1[0].len(), 4 * PCM_SAMPLES_PER_CHANNEL);
        assert_eq!(d2[0].len(), 4 * PCM_SAMPLES_PER_CHANNEL);

        // And the two psymodels produce DIFFERENT encoded bytes for a
        // structured signal — proving the option actually routes through
        // the selected model. Build raw streams to compare bytes.
        let make_stream = |p: &CodecParameters| {
            let mut enc = make_encoder(p).unwrap();
            let n = 4 * PCM_SAMPLES_PER_CHANNEL;
            // Two-tone structured signal so the models diverge.
            let plane: Vec<u8> = {
                let mut bytes = Vec::with_capacity(n * 2);
                let w1 = 2.0 * std::f64::consts::PI * 700.0 / 44_100.0;
                let w2 = 2.0 * std::f64::consts::PI * 3_300.0 / 44_100.0;
                for i in 0..n {
                    let s =
                        (0.5 * (w1 * i as f64).sin() + 0.15 * (w2 * i as f64).sin()) * FULL_SCALE;
                    let v = s.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                bytes
            };
            enc.send_frame(&Frame::Audio(AudioFrame {
                samples: n as u32,
                pts: Some(0),
                data: vec![plane.clone(), plane],
            }))
            .unwrap();
            enc.flush().unwrap();
            let mut out = Vec::new();
            while let Ok(pk) = enc.receive_packet() {
                out.extend_from_slice(&pk.data);
            }
            out
        };
        assert_ne!(
            make_stream(&p1),
            make_stream(&p2),
            "model1 and model2 must produce different encoded bytes"
        );
    }

    #[test]
    fn unrecognised_options_are_rejected() {
        let mut bad_mode = params(44_100, 2, None);
        bad_mode.options.insert("mode", "surround");
        assert!(make_encoder(&bad_mode).is_err());

        let mut bad_bound = params(44_100, 2, None);
        bad_bound.options.insert("mode", "joint_stereo");
        bad_bound.options.insert("bound", "7");
        assert!(make_encoder(&bad_bound).is_err());

        let mut bad_psy = params(44_100, 2, None);
        bad_psy.options.insert("psymodel", "model3");
        assert!(make_encoder(&bad_psy).is_err());

        // mode incompatible with channel count.
        let mut bad_mono = params(44_100, 1, None);
        bad_mono.options.insert("mode", "joint_stereo");
        assert!(make_encoder(&bad_mono).is_err());

        // unrecognised freeformat value.
        let mut bad_ff = params(44_100, 2, None);
        bad_ff.options.insert("freeformat", "maybe");
        assert!(make_encoder(&bad_ff).is_err());

        // unrecognised emphasis value (the reserved '10' code has no
        // accepted spelling).
        let mut bad_emph = params(44_100, 2, None);
        bad_emph.options.insert("emphasis", "reserved");
        assert!(make_encoder(&bad_emph).is_err());
    }

    #[test]
    fn emphasis_opt_parses_accepted_spellings() {
        assert_eq!(emphasis_opt(None).unwrap(), Emphasis::None);
        assert_eq!(emphasis_opt(Some("none")).unwrap(), Emphasis::None);
        for s in ["50/15", "5015", "50_15"] {
            assert_eq!(emphasis_opt(Some(s)).unwrap(), Emphasis::FiftyFifteen);
        }
        for s in ["j17", "j.17", "ccitt_j17"] {
            assert_eq!(emphasis_opt(Some(s)).unwrap(), Emphasis::CcittJ17);
        }
        assert!(emphasis_opt(Some("garbage")).is_err());
    }

    #[test]
    fn emphasis_5015_option_signals_the_header_and_round_trips() {
        let mut p = params(44_100, 2, Some(192_000));
        p.options.insert("emphasis", "50/15");

        // The emitted frame headers must signal the 50/15 µs curve.
        let mut enc = make_encoder(&p).unwrap();
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let plane = tone_plane(n, 1_000.0, 44_100, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data: vec![plane.clone(), plane],
        }))
        .unwrap();
        enc.flush().unwrap();
        let mut stream = Vec::new();
        while let Ok(pk) = enc.receive_packet() {
            stream.extend_from_slice(&pk.data);
        }
        let header = FrameHeader::parse(&stream).expect("parse first frame");
        assert_eq!(header.emphasis, Emphasis::FiftyFifteen);

        // And the full registry round-trip (pre-emphasis encode →
        // de-emphasis decode) reproduces the tone at the right shape.
        let planes = encode_decode_through(&p, 44_100);
        assert_eq!(planes[0].len(), 4 * PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn emphasis_j17_option_signals_the_header_and_round_trips() {
        let mut p = params(44_100, 2, Some(192_000));
        p.options.insert("emphasis", "j17");

        // The emitted frame headers must signal the CCITT J.17 curve.
        let mut enc = make_encoder(&p).unwrap();
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let plane = tone_plane(n, 1_000.0, 44_100, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data: vec![plane.clone(), plane],
        }))
        .unwrap();
        enc.flush().unwrap();
        let mut stream = Vec::new();
        while let Ok(pk) = enc.receive_packet() {
            stream.extend_from_slice(&pk.data);
        }
        let header = FrameHeader::parse(&stream).expect("parse first frame");
        assert_eq!(header.emphasis, Emphasis::CcittJ17);

        // And the full registry round-trip (J.17 pre-emphasis encode →
        // J.17 de-emphasis decode) reproduces the tone at the right
        // shape.
        let planes = encode_decode_through(&p, 44_100);
        assert_eq!(planes[0].len(), 4 * PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn metadata_flags_round_trip_through_the_header() {
        let mut p = params(44_100, 2, Some(192_000));
        p.options.insert("copyright", "true");
        p.options.insert("original", "false");
        p.options.insert("private", "true");

        let mut enc = make_encoder(&p).unwrap();
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let plane = tone_plane(n, 1_000.0, 44_100, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data: vec![plane.clone(), plane],
        }))
        .unwrap();
        enc.flush().unwrap();
        let mut stream = Vec::new();
        while let Ok(pk) = enc.receive_packet() {
            stream.extend_from_slice(&pk.data);
        }
        let header = FrameHeader::parse(&stream).expect("parse first frame");
        assert!(header.copyright);
        assert!(!header.original);
        assert!(header.private_bit);

        // Defaults when the options are absent.
        let mut enc2 = make_encoder(&params(44_100, 2, Some(192_000))).unwrap();
        let plane2 = tone_plane(n, 1_000.0, 44_100, 0.5);
        enc2.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data: vec![plane2.clone(), plane2],
        }))
        .unwrap();
        enc2.flush().unwrap();
        let mut s2 = Vec::new();
        while let Ok(pk) = enc2.receive_packet() {
            s2.extend_from_slice(&pk.data);
        }
        let h2 = FrameHeader::parse(&s2).unwrap();
        assert!(!h2.copyright);
        assert!(h2.original);
        assert!(!h2.private_bit);

        // Bad boolean value is rejected.
        let mut bad = params(44_100, 2, Some(192_000));
        bad.options.insert("copyright", "yes");
        assert!(make_encoder(&bad).is_err());
    }

    #[test]
    fn freeformat_option_emits_a_free_format_stream_that_round_trips() {
        // Encode the SAME structured input twice: once standard, once with
        // freeformat=true. The free-format stream must (a) parse as free
        // format, (b) decode through decode_free_format_stream, and (c)
        // produce byte-identical PCM to the standard encode (the payload is
        // untouched; only the bitrate_index nibble is cleared).
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let make_raw = |freeformat: bool| {
            let mut p = params(44_100, 2, Some(192_000));
            if freeformat {
                p.options.insert("freeformat", "true");
            }
            let mut enc = make_encoder(&p).unwrap();
            let data: Vec<Vec<u8>> = (0..2).map(|_| tone_plane(n, 900.0, 44_100, 0.5)).collect();
            enc.send_frame(&Frame::Audio(AudioFrame {
                samples: n as u32,
                pts: Some(0),
                data,
            }))
            .unwrap();
            enc.flush().unwrap();
            let mut out = Vec::new();
            while let Ok(pk) = enc.receive_packet() {
                out.extend_from_slice(&pk.data);
            }
            out
        };

        let standard = make_raw(false);
        let free = make_raw(true);

        // The free-format stream's first frame parses as free format.
        let h = crate::FrameHeader::parse_allow_free_format(&free).unwrap();
        assert!(h.is_free_format(), "freeformat=true → bitrate_index '0000'");
        // The standard stream's header is NOT free format.
        assert!(!crate::FrameHeader::parse(&standard)
            .unwrap()
            .is_free_format());

        // Both decode to identical PCM.
        let std_pcm = decode_all_frames(&standard).expect("standard decode");
        let ff_pcm = crate::frame::decode_free_format_stream(&free).expect("free-format decode");
        assert_eq!(ff_pcm.len(), std_pcm.len(), "channel count");
        for ch in 0..std_pcm.len() {
            assert_eq!(ff_pcm[ch], std_pcm[ch], "ch {ch} free-format round-trip");
        }
    }

    #[test]
    fn send_frame_buffers_until_1152_then_emits_one_packet() {
        let mut enc = make_encoder(&params(44_100, 2, Some(192_000))).unwrap();

        // Send 1000 samples/channel — below 1152, no packet yet.
        let half = PCM_SAMPLES_PER_CHANNEL - 152;
        let l = tone_plane(half, 1_000.0, 44_100, 0.5);
        let r = tone_plane(half, 1_000.0, 44_100, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: half as u32,
            pts: Some(0),
            data: vec![l, r],
        }))
        .unwrap();
        assert!(matches!(enc.receive_packet(), Err(Error::NeedMore)));

        // Send 152 more → crosses 1152 → exactly one packet.
        let l2 = tone_plane(152, 1_000.0, 44_100, 0.5);
        let r2 = tone_plane(152, 1_000.0, 44_100, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: 152,
            pts: Some(half as i64),
            data: vec![l2, r2],
        }))
        .unwrap();
        let pkt = enc.receive_packet().expect("one packet after 1152 samples");
        // Packet must be exactly one Layer II frame.
        let fsz = build_header(
            44_100,
            Mode::Stereo,
            ModeExtension::Bound4,
            192_000,
            false,
            Emphasis::None,
        )
        .unwrap()
        .frame_size_bytes();
        assert_eq!(pkt.data.len(), fsz);
        assert_eq!(pkt.pts, Some(0));
        assert!(pkt.flags.keyframe);
        assert!(matches!(enc.receive_packet(), Err(Error::NeedMore)));
    }

    #[test]
    fn registry_encode_decode_round_trips_a_tone() {
        // End-to-end: drive a multi-frame tone through the registry
        // Encoder, then decode the concatenated packets and check the
        // reconstruction is shaped (right sample count, tone-dominated).
        let n_frames = 6;
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let mut enc = make_encoder(&params(48_000, 2, Some(192_000))).unwrap();

        let l = tone_plane(total, 1_000.0, 48_000, 0.5);
        let r = tone_plane(total, 1_000.0, 48_000, 0.5);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: total as u32,
            pts: Some(0),
            data: vec![l, r],
        }))
        .unwrap();
        enc.flush().unwrap();

        let mut stream = Vec::new();
        let mut npkts = 0;
        loop {
            match enc.receive_packet() {
                Ok(p) => {
                    stream.extend_from_slice(&p.data);
                    npkts += 1;
                }
                Err(Error::Eof) => break,
                Err(e) => panic!("receive_packet: {e:?}"),
            }
        }
        assert_eq!(
            npkts, n_frames,
            "exactly n_frames packets for an exact multiple"
        );

        let planes = decode_all_frames(&stream).expect("decode encoder output");
        assert_eq!(planes.len(), 2);
        for plane in &planes {
            assert_eq!(plane.len(), total, "decoded sample count");
        }

        // Tone localisation: the steady middle's energy at 1 kHz must
        // dominate an unrelated probe frequency.
        let lo = PCM_SAMPLES_PER_CHANNEL;
        let hi = total - PCM_SAMPLES_PER_CHANNEL;
        let steady = &planes[0][lo..hi];
        let tone = goertzel(steady, 1_000.0, 48_000);
        let probe = goertzel(steady, 9_000.0, 48_000);
        assert!(
            tone > 100.0 * probe.max(f64::MIN_POSITIVE),
            "tone {tone:.3e} must dominate probe {probe:.3e}"
        );
    }

    #[test]
    fn flush_zero_pads_a_partial_trailing_frame() {
        // 1152 + 500 samples → 2 frames (the second zero-padded on flush).
        let mut enc = make_encoder(&params(44_100, 1, Some(128_000))).unwrap();
        let n = PCM_SAMPLES_PER_CHANNEL + 500;
        let plane = tone_plane(n, 800.0, 44_100, 0.4);
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data: vec![plane],
        }))
        .unwrap();
        // One full frame available before flush.
        let fsz = build_header(
            44_100,
            Mode::SingleChannel,
            ModeExtension::Bound4,
            128_000,
            false,
            Emphasis::None,
        )
        .unwrap()
        .frame_size_bytes();
        let p0 = enc.receive_packet().expect("first frame");
        assert_eq!(p0.data.len(), fsz);
        assert!(matches!(enc.receive_packet(), Err(Error::NeedMore)));

        enc.flush().unwrap();
        let p1 = enc.receive_packet().expect("padded trailing frame");
        // 44,1 kHz / 128 kbit/s: dif = (144·128000) mod 44100 = 42300,
        // so the §2.4.2.3 scheduler pads frame 1 (rest 0 − 42300 < 0) —
        // one slot larger than the unpadded frame 0.
        assert_eq!(p1.data.len(), fsz + 1);
        assert_eq!(p1.pts, Some(PCM_SAMPLES_PER_CHANNEL as i64));
        assert!(matches!(enc.receive_packet(), Err(Error::Eof)));

        // Both frames decode cleanly.
        let mut stream = p0.data.clone();
        stream.extend_from_slice(&p1.data);
        let planes = decode_all_frames(&stream).expect("decode padded stream");
        assert_eq!(planes[0].len(), 2 * PCM_SAMPLES_PER_CHANNEL);
    }

    #[test]
    fn send_frame_after_flush_is_rejected() {
        let mut enc = make_encoder(&params(44_100, 2, None)).unwrap();
        enc.flush().unwrap();
        let err = enc.send_frame(&Frame::Audio(AudioFrame {
            samples: 0,
            pts: None,
            data: vec![Vec::new(), Vec::new()],
        }));
        assert!(err.is_err());
    }

    #[test]
    fn send_frame_rejects_wrong_plane_count_and_odd_lengths() {
        let mut enc = make_encoder(&params(44_100, 2, None)).unwrap();
        // 1 plane for a 2-channel encoder.
        assert!(enc
            .send_frame(&Frame::Audio(AudioFrame {
                samples: 0,
                pts: None,
                data: vec![Vec::new()],
            }))
            .is_err());
        // Odd byte length (not whole S16 samples).
        assert!(enc
            .send_frame(&Frame::Audio(AudioFrame {
                samples: 0,
                pts: None,
                data: vec![vec![0u8; 3], vec![0u8; 3]],
            }))
            .is_err());
    }

    #[test]
    fn crc_option_emits_protected_frames_that_verify_and_detect_corruption() {
        // crc=true → every packet's header has protection_bit == '0'
        // (CRC present), the §2.4.1.4 word verifies on decode, and a
        // flipped bit-allocation byte is *detected* (CrcMismatch)
        // instead of silently mis-decoding.
        let n = 3 * PCM_SAMPLES_PER_CHANNEL;
        let mut p = params(48_000, 2, Some(192_000));
        p.options.insert("crc", "true");
        let mut enc = make_encoder(&p).unwrap();
        let data: Vec<Vec<u8>> = (0..2).map(|_| tone_plane(n, 700.0, 48_000, 0.4)).collect();
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data,
        }))
        .unwrap();
        enc.flush().unwrap();

        let mut packets = Vec::new();
        loop {
            match enc.receive_packet() {
                Ok(pk) => packets.push(pk.data),
                Err(Error::Eof) => break,
                Err(e) => panic!("receive_packet: {e:?}"),
            }
        }
        assert_eq!(packets.len(), 3);

        let mut stream = Vec::new();
        for pkt in &packets {
            let h = crate::FrameHeader::parse(pkt).expect("packet header");
            assert!(
                !h.protection_bit,
                "crc=true → protection_bit '0' (CRC present)"
            );
            stream.extend_from_slice(pkt);
        }
        // All frames verify.
        let planes = decode_all_frames(&stream).expect("CRC-protected stream decodes");
        assert_eq!(planes[0].len(), n);

        // Corrupt one bit-allocation byte (just after the 4-byte header
        // + 2-byte CRC word) — the decoder must flag the mismatch.
        let mut bad = packets[0].clone();
        bad[6] ^= 0x55;
        match crate::frame::decode_frame(&bad) {
            Err(crate::frame::FrameError::CrcMismatch { .. }) => {}
            other => panic!("expected CrcMismatch on corrupted frame, got {other:?}"),
        }
    }

    #[test]
    fn crc_option_default_is_unprotected_and_bad_value_is_rejected() {
        // Default (no `crc` key): protection_bit == '1' (no CRC).
        let mut enc = make_encoder(&params(48_000, 2, Some(192_000))).unwrap();
        let n = PCM_SAMPLES_PER_CHANNEL;
        let data: Vec<Vec<u8>> = (0..2).map(|_| tone_plane(n, 700.0, 48_000, 0.4)).collect();
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data,
        }))
        .unwrap();
        let pkt = enc.receive_packet().expect("one frame");
        let h = crate::FrameHeader::parse(&pkt.data).unwrap();
        assert!(h.protection_bit, "default is no CRC");

        // Unrecognised value rejected at build time.
        let mut bad = params(48_000, 2, Some(192_000));
        bad.options.insert("crc", "always");
        assert!(make_encoder(&bad).is_err());
    }

    #[test]
    fn bound_auto_selects_per_frame_and_round_trips() {
        // bound=auto (with mode=joint_stereo) drives the Annex G.1
        // demand-driven per-frame policy. At a generous 384 kbit/s a
        // modest tone fits full Stereo, so the emitted packets carry
        // mode Stereo; the stream still decodes cleanly.
        let n = 4 * PCM_SAMPLES_PER_CHANNEL;
        let mut p = params(44_100, 2, Some(384_000));
        p.options.insert("mode", "joint_stereo");
        p.options.insert("bound", "auto");
        let mut enc = make_encoder(&p).unwrap();
        let data: Vec<Vec<u8>> = (0..2).map(|_| tone_plane(n, 900.0, 44_100, 0.3)).collect();
        enc.send_frame(&Frame::Audio(AudioFrame {
            samples: n as u32,
            pts: Some(0),
            data,
        }))
        .unwrap();
        enc.flush().unwrap();

        let mut stream = Vec::new();
        let mut modes = Vec::new();
        loop {
            match enc.receive_packet() {
                Ok(pk) => {
                    modes.push(crate::FrameHeader::parse(&pk.data).unwrap().mode);
                    stream.extend_from_slice(&pk.data);
                }
                Err(Error::Eof) => break,
                Err(e) => panic!("receive_packet: {e:?}"),
            }
        }
        assert_eq!(modes.len(), 4);
        assert!(
            modes.iter().all(|&m| m == Mode::Stereo),
            "384 kbit/s tone fits full Stereo per frame (got {modes:?})"
        );
        let planes = decode_all_frames(&stream).expect("decode auto-bound stream");
        assert_eq!(planes[0].len(), n);
    }

    #[test]
    fn bound_auto_requires_joint_stereo_mode() {
        // Without mode=joint_stereo the auto policy has nothing to
        // select over — rejected at build time.
        let mut p = params(44_100, 2, Some(192_000));
        p.options.insert("bound", "auto");
        assert!(make_encoder(&p).is_err());

        let mut p2 = params(44_100, 2, Some(192_000));
        p2.options.insert("mode", "dual_channel");
        p2.options.insert("bound", "auto");
        assert!(make_encoder(&p2).is_err());
    }

    /// Goertzel single-bin power estimate (test-local copy).
    fn goertzel(signal: &[f64], freq_hz: f64, sample_rate: u32) -> f64 {
        let w = 2.0 * std::f64::consts::PI * freq_hz / f64::from(sample_rate);
        let coeff = 2.0 * w.cos();
        let mut s_prev = 0.0;
        let mut s_prev2 = 0.0;
        for &x in signal {
            let s = x + coeff * s_prev - s_prev2;
            s_prev2 = s_prev;
            s_prev = s;
        }
        s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2
    }
}
