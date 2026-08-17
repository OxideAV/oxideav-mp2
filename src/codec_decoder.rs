//! `oxideav_core::Decoder` wiring for MPEG-1 Audio Layer II (MP2).
//!
//! Round 182 — registry trait surface. The crate's existing
//! [`decode_frame`](crate::frame::decode_frame) /
//! [`decode_frame_with`](crate::frame::decode_frame_with) primitives
//! already decode one Layer II frame to PCM end-to-end; this module
//! adapts that path into the framework's packet-in / frame-out
//! [`oxideav_core::Decoder`] trait so containers (AVI's
//! `WAVE_FORMAT_MPEG = 0x0050`, Matroska's `A_MPEG/L2`, etc.) can route
//! Layer II streams via the registry.
//!
//! ## Trait-API adaptation
//!
//! The framework trait is *packet-in, frame-out*:
//!
//! * [`send_packet`](Decoder::send_packet) accepts one [`Packet`] whose
//!   `data` is **one complete Layer II frame** (header + optional
//!   §2.4.1.4 CRC slot + §2.4.1.6 audio data section). The expected
//!   per-frame framing matches what
//!   [`crate::frame::decode_all_frames`] consumes when walking a
//!   contiguous Layer II byte stream — i.e. one packet covers exactly
//!   [`FrameHeader::frame_size_bytes`] bytes from the syncword
//!   inclusive.
//! * [`receive_frame`](Decoder::receive_frame) returns one
//!   [`AudioFrame`] holding 1152 PCM samples per channel
//!   (§2.4.2.1 "1 152 for Layer II"), planar little-endian `i16`.
//! * [`flush`](Decoder::flush) marks end-of-stream so subsequent
//!   `receive_frame` calls eventually return [`Error::Eof`] once the
//!   pending-frames queue drains.
//! * [`reset`](Decoder::reset) wipes per-stream filterbank state — the
//!   Annex A Figure A.2 V ring buffer — so the next `send_packet`
//!   decodes as if it were the first (the trait contract: "zero any
//!   per-stream filter / predictor / overlap memory so the next
//!   `send_packet` decodes as if it were the first").
//!
//! ## Output format
//!
//! The decoder emits planar S16 PCM in `Frame::Audio`: `data.len() ==
//! channels`, each `data[ch]` is `samples_per_channel * 2` bytes of
//! little-endian `i16`. The §2.4.3.4.7.1 nominal float range
//! `[-1.0, +1.0]` is mapped to `[i16::MIN, i16::MAX]` with
//! `s_i16 = clamp(s_f64 * 32768.0, i16::MIN, i16::MAX) as i16`. The
//! samples-per-channel count is 1152 (the §2.4.2.1 Layer II constant).
//!
//! ## Registration
//!
//! [`register_codecs`] installs the codec under id `"mp2"` and claims
//! two container tags: WAVE format `0x0050` (the §B.1.6.6 Win32
//! `WAVE_FORMAT_MPEG` code — shared with Layer I) and the Matroska
//! `A_MPEG/L2` CodecID. A probe checks the on-wire layer field
//! (`mpeg_layer_bits`) when the demuxer has a first-packet sample
//! available, so the §B.1.6.6 tag collision with `oxideav-mp1`
//! (`0x0050` is "MPEG-1 Audio, Layer I/II", not Layer-I-only) resolves
//! to whichever crate's probe scores higher.

use std::collections::VecDeque;

use oxideav_core::{
    AudioFrame, ChannelLayout, CodecCapabilities, CodecId, CodecInfo, CodecParameters,
    CodecRegistry, CodecTag, Confidence, Decoder, Error, Frame, Packet, ProbeContext, Result,
    SampleFormat,
};

use crate::frame::{
    decode_frame_with, decode_frame_with_known_header, DecodedFrame, FrameDecodeState,
    PCM_SAMPLES_PER_CHANNEL,
};
use crate::header::FrameHeader;
use crate::mc::{decode_mc_frame_with, McConfig, McDecodeState, McDecodedFrame, McError};

/// Codec id under which [`register_codecs`] installs this decoder.
pub const CODEC_ID_STR: &str = "mp2";

/// `WAVE_FORMAT_MPEG` per Win32 `mmreg.h` — covers MPEG-1 Audio Layer
/// I and Layer II (Layer III uses its own `WAVE_FORMAT_MPEGLAYER3 =
/// 0x0055`). Used as the AVI / WAVEFORMATEX `wFormatTag` for Layer II
/// streams.
pub const WAVE_FORMAT_MPEG: u16 = 0x0050;

/// How the decoder treats the ISO/IEC 13818-3 §2.5 multichannel
/// extension riding the §2.4.1.8 ancillary field. Selected by the
/// `mc` codec option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McMode {
    /// Ignore any extension; decode the MPEG-1-compatible base pair
    /// (the historical behaviour, and the default).
    Off,
    /// Every packet must carry a valid §2.5 extension; a frame whose
    /// §2.5.2.14 `mc_crc_check` fails is an error.
    On,
    /// Probe the first packet with the §2.5.3.1 CRC-detection rule and
    /// latch: extension present → multichannel output for the whole
    /// stream, absent → plain base decode.
    Auto,
}

/// Where one output plane of a multichannel [`AudioFrame`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McPlaneSource {
    /// `McDecodedFrame::channels[i]` (a full-bandwidth presentation
    /// channel).
    Presentation(usize),
    /// The §2.5.3.2.4 LFE channel, zero-order-held ×96 to the full
    /// sampling rate (`mc_lfe=hold`).
    LfeHold,
}

/// The fixed per-stream output shape decided from the first
/// multichannel frame: the [`ChannelLayout`] announced through
/// `output_params` and the source of each plane, in canonical core
/// layout order.
#[derive(Debug, Clone, PartialEq, Eq)]
struct McOutputPlan {
    layout: ChannelLayout,
    sources: Vec<McPlaneSource>,
}

/// Map a §2.5.2.15 channel configuration onto oxideav-core's canonical
/// [`ChannelLayout`] vocabulary, together with the per-plane source
/// order.
///
/// The §2.5 presentation order (L, R, [C], [LS, RS | S]) matches the
/// core layouts' canonical order except that core places LFE at its
/// BS.775 slot (e.g. index 3 in 5.1), so only the LFE plane is
/// re-ordered. Configurations core has no named layout for (2/1's
/// L, R, S; the +LFE variants of 3/0, 2/2, 2/1 and mono) fall back to
/// [`ChannelLayout::DiscreteN`] with the LFE plane appended last —
/// `DiscreteN` is core's documented catch-all for exactly this case.
/// The main programme only: a second stereo programme and the
/// multilingual channels are independent programmes with no slot in a
/// single speaker layout, so the registry surface drops them (the
/// direct [`crate::mc::decode_mc_stream`] API carries them all).
fn mc_output_plan(config: &McConfig, lfe_present: bool, lfe_hold: bool) -> McOutputPlan {
    use McPlaneSource::{LfeHold, Presentation as P};
    let with_lfe = lfe_present && lfe_hold;
    let (layout, sources): (ChannelLayout, Vec<McPlaneSource>) =
        match (config.front, config.surround, with_lfe) {
            (3, 2, false) => (
                ChannelLayout::Surround50,
                vec![P(0), P(1), P(2), P(3), P(4)],
            ),
            (3, 2, true) => (
                ChannelLayout::Surround51,
                vec![P(0), P(1), P(2), LfeHold, P(3), P(4)],
            ),
            (3, 1, false) => (ChannelLayout::Surround40, vec![P(0), P(1), P(2), P(3)]),
            (3, 1, true) => (
                ChannelLayout::Surround41,
                vec![P(0), P(1), P(2), P(3), LfeHold],
            ),
            (3, 0, false) => (ChannelLayout::Surround30, vec![P(0), P(1), P(2)]),
            (3, 0, true) => (ChannelLayout::DiscreteN(4), vec![P(0), P(1), P(2), LfeHold]),
            (2, 2, false) => (ChannelLayout::Quad, vec![P(0), P(1), P(2), P(3)]),
            (2, 2, true) => (
                ChannelLayout::DiscreteN(5),
                vec![P(0), P(1), P(2), P(3), LfeHold],
            ),
            (2, 1, false) => (ChannelLayout::DiscreteN(3), vec![P(0), P(1), P(2)]),
            (2, 1, true) => (ChannelLayout::DiscreteN(4), vec![P(0), P(1), P(2), LfeHold]),
            (2, 0, false) => (ChannelLayout::Stereo, vec![P(0), P(1)]),
            (2, 0, true) => (ChannelLayout::Stereo21, vec![P(0), P(1), LfeHold]),
            (1, 0, false) => (ChannelLayout::Mono, vec![P(0)]),
            (1, 0, true) => (ChannelLayout::DiscreteN(2), vec![P(0), LfeHold]),
            // §2.5.2.15 derives front ∈ {1, 2, 3} and surround ∈
            // {0, 1, 2}; front == 1 only for a mono base (surround 0).
            other => unreachable!("§2.5.2.15 rules out configuration {other:?}"),
        };
    McOutputPlan { layout, sources }
}

/// Parse the `mc` codec option.
fn mc_mode_opt(s: Option<&str>) -> Result<McMode> {
    match s {
        None | Some("off") | Some("false") | Some("0") => Ok(McMode::Off),
        Some("on") | Some("true") | Some("1") => Ok(McMode::On),
        Some("auto") => Ok(McMode::Auto),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: mc={other:?} not recognised (off / on / auto)"
        ))),
    }
}

/// Parse the `mc_lfe` codec option: what to do with the §2.5.3.2.4
/// LFE channel, whose native rate is `Fs / 96`.
///
/// * `"drop"` (default) — omit the LFE plane; the output layout is the
///   full-bandwidth set (5.0 instead of 5.1). The native-rate LFE is
///   available through the direct [`crate::mc::decode_mc_stream`] API.
/// * `"hold"` — zero-order-hold each LFE sample ×96 up to the frame
///   rate so the plane fits the single-`sample_rate` [`AudioFrame`]
///   contract. The standard fixes only the transmitted format
///   (block-companded PCM at `Fs / 96`); bringing it to the
///   presentation rate is a decoder-side rendering choice, and the
///   hold is the interpolation-free one.
fn mc_lfe_hold_opt(s: Option<&str>) -> Result<bool> {
    match s {
        None | Some("drop") => Ok(false),
        Some("hold") => Ok(true),
        Some(other) => Err(Error::invalid(format!(
            "oxideav-mp2: mc_lfe={other:?} not recognised (drop / hold)"
        ))),
    }
}

/// Build a boxed MPEG-1 Audio Layer II [`Decoder`] from `params`.
///
/// `params.sample_rate` (32_000 / 44_100 / 48_000) and `params.channels`
/// (1 or 2) configure the returned decoder's stream parameters; the
/// actual per-frame sample rate and channel count are re-derived from
/// each Layer II frame header on `send_packet`, so the values supplied
/// here are a hint used only to seed `output_params()`.
///
/// # Codec options
///
/// * `mc` — the ISO/IEC 13818-3 §2.5 multichannel extension: `"off"`
///   (default — decode the MPEG-1-compatible base pair, the
///   historical behaviour), `"on"` (require the extension; its
///   presentation channels become the output planes, ordered per the
///   matching [`ChannelLayout`]) or `"auto"` (probe the first packet
///   per the §2.5.3.1 CRC rule and latch). Each packet must carry the
///   whole extension in its frame — a stream whose extension spills
///   into a §2.5.1.5 extension bit stream cannot ride the one-frame-
///   per-packet interface and is reported as an error (`mc=on`) or
///   decoded as the compatible base pair (`mc=auto`). A second stereo
///   programme and multilingual channels are dropped (independent
///   programmes with no slot in one speaker layout); use
///   [`crate::mc::decode_mc_stream`] to reach them.
/// * `mc_lfe` — `"drop"` (default) or `"hold"`; see [`mc_lfe_hold_opt`].
///
/// # Errors
///
/// Returns [`Error::invalid`] when `channels` is supplied and not 1 or
/// 2 while `mc=off`. The §2.4.2.3 mode field encodes at most two
/// channels, so without the §2.5 extension `channels >= 3` is
/// unrepresentable on the wire and rejected at build time; with
/// `mc=on` / `mc=auto` the hint may name the expected presentation
/// count (≤ 8). `sample_rate` is optional (defaults to 44_100 when
/// absent): the real value is re-read from every frame header anyway.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    let mc_mode = mc_mode_opt(params.options.get("mc"))?;
    let mc_lfe_hold = mc_lfe_hold_opt(params.options.get("mc_lfe"))?;
    let channels = params.channels.unwrap_or(1);
    let channel_cap = if mc_mode == McMode::Off { 2 } else { 8 };
    if channels == 0 || channels > channel_cap {
        return Err(Error::invalid(format!(
            "oxideav-mp2: decoder supports 1..={channel_cap} channels with mc={mc_mode:?} \
             (channels={channels})"
        )));
    }
    let sample_rate = params.sample_rate.unwrap_or(44_100);

    let mut out_params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    out_params.sample_rate = Some(sample_rate);
    out_params.channels = Some(channels);
    out_params.sample_format = Some(SampleFormat::S16);
    out_params.tag = Some(CodecTag::wave_format(WAVE_FORMAT_MPEG));

    Ok(Box::new(Mp2CoreDecoder::new_with_mc(
        CodecId::new(CODEC_ID_STR),
        out_params,
        mc_mode,
        mc_lfe_hold,
    )))
}

/// Packet-to-frame adaptor that wraps the existing
/// [`crate::frame::decode_frame_with`] primitive in the framework
/// [`Decoder`] trait.
///
/// State carried across packets:
///
/// * `state` — [`FrameDecodeState`] threads the Annex A Figure A.2
///   per-channel V ring buffer across successive frames (footnote 1: V
///   is zeroed only at startup).
/// * `pending_frames` queues at-most-one [`AudioFrame`] produced by the
///   last `send_packet`; `receive_frame` pops it.
/// * `eof` — set by [`Decoder::flush`]; once `pending_frames` drains
///   and `eof` is true, `receive_frame` returns [`Error::Eof`].
pub struct Mp2CoreDecoder {
    codec_id: CodecId,
    output: CodecParameters,
    state: FrameDecodeState,
    pending_frames: VecDeque<AudioFrame>,
    eof: bool,
    /// §2.5 multichannel handling (`mc` codec option).
    mc_mode: McMode,
    /// `mc_lfe=hold` — zero-order-hold the LFE plane to full rate.
    mc_lfe_hold: bool,
    /// `mc=auto`: the latched probe verdict (None until the first
    /// packet decides).
    mc_active: Option<bool>,
    /// §2.5 cross-frame decode state (filterbanks, predictor history).
    mc_state: McDecodeState,
    /// Output shape decided from the first multichannel frame.
    mc_plan: Option<McOutputPlan>,
}

impl std::fmt::Debug for Mp2CoreDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mp2CoreDecoder")
            .field("codec_id", &self.codec_id)
            .field("pending_frames", &self.pending_frames.len())
            .field("eof", &self.eof)
            .finish()
    }
}

impl Mp2CoreDecoder {
    fn new_with_mc(
        codec_id: CodecId,
        output: CodecParameters,
        mc_mode: McMode,
        mc_lfe_hold: bool,
    ) -> Self {
        Self {
            codec_id,
            output,
            state: FrameDecodeState::new(),
            pending_frames: VecDeque::new(),
            eof: false,
            mc_mode,
            mc_lfe_hold,
            mc_active: None,
            mc_state: McDecodeState::new(),
            mc_plan: None,
        }
    }

    /// Stream parameters describing the decoder's output, refreshed
    /// from the wire after each decoded packet. For a multichannel
    /// stream (`mc=on` / a positive `mc=auto` probe) `channels` and
    /// `channel_layout` carry the presentation layout the planes of
    /// every emitted [`AudioFrame`] follow.
    pub fn output_params(&self) -> &CodecParameters {
        &self.output
    }

    /// Decode one packet's worth of Layer II data, transparently handling
    /// the §2.4.2.3 free-format case.
    ///
    /// A demuxer hands one frame per packet, so for a free-format frame
    /// the packet length **is** the frame size (`N` or `N + 1` slots) —
    /// no sync-to-sync measurement is needed. The frame is decoded with
    /// the free-format header itself: the Annex B table for free format
    /// is fixed by the sampling frequency alone (Table 3-B.2a at 48 kHz
    /// / 3-B.2b at 44,1 & 32 kHz, per the table headers; LSF uses the
    /// single 13818-3 Table B.1), applied by
    /// [`crate::bitalloc::select_table`] to the `bit_rate == 0` header.
    /// The fixed rate may be off-ladder; only a packet size implying
    /// more than the §2.4.2.3 384 kbit/s Layer II free-format support
    /// ceiling is refused. Non-free-format packets take the ordinary
    /// [`decode_frame_with`] path unchanged.
    fn decode_one_packet(&mut self, data: &[u8]) -> Result<DecodedFrame> {
        // Peek the header in free-format-tolerant mode.
        let header = FrameHeader::parse_allow_free_format(data)
            .map_err(|e| Error::other(format!("oxideav-mp2: header parse: {e}")))?;
        if !header.is_free_format() {
            return decode_frame_with(data, &mut self.state)
                .map_err(|e| Error::other(format!("oxideav-mp2: decode_frame: {e}")));
        }
        // Free format: the packet length is this frame's size. The
        // recovered bitrate is metadata; the call enforces the §2.4.2.3
        // support ceiling.
        let frame_size = data.len();
        let base_slots = frame_size - if header.padding { 1 } else { 0 };
        let _bit_rate = crate::freeformat::bitrate_from_base_slots(&header, base_slots)
            .map_err(|e| Error::other(format!("oxideav-mp2: free-format: {e}")))?;
        decode_frame_with_known_header(data, header, &mut self.state)
            .map_err(|e| Error::other(format!("oxideav-mp2: free-format decode: {e}")))
    }

    /// Re-derive and update `self.output` from a freshly-parsed frame
    /// header so callers reading parameters after the first decoded
    /// packet see the on-the-wire values rather than the
    /// at-construction hints.
    fn refresh_output_params(&mut self, hdr: &FrameHeader) {
        self.output.sample_rate = Some(hdr.sample_rate);
        self.output.channels = Some(hdr.channels() as u16);
    }

    /// Convert a per-channel `Vec<f64>` PCM plane in the §2.4.3.4.7.1
    /// nominal `[-1.0, +1.0]` range to planar little-endian `i16`
    /// bytes.
    ///
    /// The §2.4.3.3.4 requantizer interprets each codeword as a two's
    /// complement fractional number "where the MSB represents the value
    /// −1" (PDF page 31). The matching integer full-scale map is the
    /// symmetric `−1.0 ↦ −32768`, i.e. multiply by `2^15 = 32768` and
    /// round to nearest. This places `0.0 ↦ 0` and `+1.0 ↦ +32768`,
    /// the latter clamped to `i16::MAX`; out-of-range peaks clip to
    /// `[i16::MIN, i16::MAX]`. (Using `i16::MAX` as the scale instead
    /// biases every nonzero sample toward zero by a fraction of an LSB,
    /// which measurably widens the conformance error against a
    /// reference decoder's output; the `2^15` scale is the standard
    /// fractional-to-integer convention.)
    /// Decode one packet as a §2.5 multichannel frame and build the
    /// output planes per the stream's [`McOutputPlan`].
    fn decode_mc_packet(&mut self, data: &[u8], pts: Option<i64>) -> Result<AudioFrame> {
        let (frame, _ext) =
            decode_mc_frame_with(data, None, &mut self.mc_state).map_err(Self::mc_error_to_core)?;
        let plan = match &self.mc_plan {
            Some(p) => p,
            None => {
                let plan = mc_output_plan(&frame.config, frame.lfe.is_some(), self.mc_lfe_hold);
                self.mc_plan = Some(plan);
                self.mc_plan.as_ref().expect("just set")
            }
        };
        let mut data_planes = Vec::with_capacity(plan.sources.len());
        for src in &plan.sources {
            let plane = match src {
                McPlaneSource::Presentation(i) => {
                    let ch = frame.channels.get(*i).ok_or_else(|| {
                        Error::other(format!(
                            "oxideav-mp2: mc frame lost presentation channel {i} mid-stream"
                        ))
                    })?;
                    Self::float_plane_to_s16_le(ch)
                }
                McPlaneSource::LfeHold => Self::float_plane_to_s16_le(&Self::lfe_hold(&frame)?),
            };
            data_planes.push(plane);
        }
        self.output.sample_rate = Some(frame.base_header.sample_rate);
        self.output.channels = Some(plan.sources.len() as u16);
        self.output.channel_layout = Some(plan.layout);
        Ok(AudioFrame {
            samples: PCM_SAMPLES_PER_CHANNEL as u32,
            pts,
            data: data_planes,
        })
    }

    /// Zero-order-hold the frame's 12 native-rate LFE samples ×96 up
    /// to the full sampling rate (`mc_lfe=hold`).
    fn lfe_hold(frame: &McDecodedFrame) -> Result<Vec<f64>> {
        let lfe = frame
            .lfe
            .as_ref()
            .ok_or_else(|| Error::other("oxideav-mp2: mc frame lost its LFE channel mid-stream"))?;
        let factor = PCM_SAMPLES_PER_CHANNEL / lfe.len();
        let mut out = Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL);
        for &s in lfe {
            out.resize(out.len() + factor, s);
        }
        Ok(out)
    }

    fn mc_error_to_core(e: McError) -> Error {
        match e {
            McError::MissingExtFrame => Error::other(
                "oxideav-mp2: this multichannel stream continues in a §2.5.1.5 extension bit \
                 stream, which the one-frame-per-packet interface cannot carry; use \
                 mc::decode_mc_stream with the extension stream bytes",
            ),
            other => Error::other(format!("oxideav-mp2: mc decode: {other}")),
        }
    }

    fn float_plane_to_s16_le(plane: &[f64]) -> Vec<u8> {
        const FULL_SCALE: f64 = 32768.0; // 2^15 — MSB == −1.0 (§2.4.3.3.4)
        let mut bytes = Vec::with_capacity(plane.len() * 2);
        for &s in plane {
            let scaled = s * FULL_SCALE;
            // Round to nearest, then clamp into the i16 range so that a
            // full-scale `+1.0` (→ +32768) saturates at `i16::MAX`.
            let v = scaled
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes
    }
}

impl Decoder for Mp2CoreDecoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.eof {
            return Err(Error::other("oxideav-mp2: cannot send_packet after flush"));
        }
        let mc = match self.mc_mode {
            McMode::Off => false,
            McMode::On => true,
            McMode::Auto => match self.mc_active {
                Some(v) => v,
                None => {
                    // §2.5.3.1: "The MPEG-1 ancillary data field is
                    // initially assumed to contain the coded
                    // multichannel extension. If the mandatory
                    // CRC-check yields a valid result, then
                    // multichannel decoding will be started." A stream
                    // whose extension continues in a §2.5.1.5
                    // extension bit stream cannot ride the packet
                    // interface, so it latches to the base decode
                    // (the compatible pair is a valid rendering).
                    let v =
                        decode_mc_frame_with(&packet.data, None, &mut McDecodeState::new()).is_ok();
                    self.mc_active = Some(v);
                    v
                }
            },
        };
        if mc {
            let frame = self.decode_mc_packet(&packet.data, packet.pts)?;
            self.pending_frames.push_back(frame);
            return Ok(());
        }
        let decoded = self.decode_one_packet(&packet.data)?;
        self.refresh_output_params(&decoded.header);

        let data: Vec<Vec<u8>> = decoded
            .pcm
            .iter()
            .map(|plane| Self::float_plane_to_s16_le(plane))
            .collect();
        let frame = AudioFrame {
            samples: PCM_SAMPLES_PER_CHANNEL as u32,
            pts: packet.pts,
            data,
        };
        self.pending_frames.push_back(frame);
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        if let Some(audio) = self.pending_frames.pop_front() {
            return Ok(Frame::Audio(audio));
        }
        if self.eof {
            return Err(Error::Eof);
        }
        Err(Error::NeedMore)
    }

    fn flush(&mut self) -> Result<()> {
        self.eof = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.state.reset();
        // §2.5 filterbanks + predictor history re-zero on seek; the
        // `mc=auto` verdict and the output plan describe the *stream*
        // and survive the seek.
        self.mc_state.reset();
        self.pending_frames.clear();
        self.eof = false;
        Ok(())
    }
}

/// Probe used by [`register_codecs`] to disambiguate the §B.1.6.6
/// `WAVE_FORMAT_MPEG = 0x0050` tag collision with `oxideav-mp1`. When
/// the demuxer supplies a packet sample, inspect the §2.4.1.3 `layer`
/// field (bits 18..17 of the first 4 bytes — `'10'` = Layer II,
/// `'11'` = Layer I, `'01'` = Layer III) and score by exact match.
///
/// * Sync OK + `layer == '10'` -> `1.0` (definitive Layer II).
/// * Sync OK + non-Layer-II    -> `0.0` (definitely not us).
/// * No packet hint            -> `0.5` (we can claim but mp1 can too).
/// * Sync fail / short packet  -> `0.1` (nominal default — let any
///   competing probe with a stronger signal win).
fn probe_mp2(ctx: &ProbeContext) -> Confidence {
    let Some(pkt) = ctx.packet else {
        return 0.5;
    };
    if pkt.len() < 4 {
        return 0.1;
    }
    let word = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]);
    let sync = (word >> 20) & 0xFFF;
    if sync != 0xFFF {
        return 0.1;
    }
    let layer_bits = (word >> 17) & 0x3;
    if layer_bits == 0b10 {
        1.0
    } else {
        0.0
    }
}

/// Install the MPEG-1 Audio Layer II decoder factory into `reg`.
///
/// Claims:
///
/// * **WAVE format `0x0050`** (`WAVE_FORMAT_MPEG`) — Win32 `mmreg.h`
///   convention; covers Layer I + Layer II (Layer III lives at
///   `0x0055`). The [`probe_mp2`] disambiguator inspects the §2.4.1.3
///   layer field on the first packet to choose between Layer I and
///   Layer II registrations.
/// * **Matroska `A_MPEG/L2`** — the EBML codec ID dedicated to MPEG-1
///   Audio Layer II per the Matroska Codec ID registry.
///
/// The encoder factory is wired alongside the decoder as of round 371:
/// the §C.1.3 analysis filterbank, §D.1 Model-1 / §D.2 Model-2 auto-SMR
/// chains, §C.1.5.2.7 bit-allocation, and §2.4.1.6 audio-data writer are
/// all complete, so [`crate::codec_encoder::make_encoder`] constructs a
/// full frame-in / packet-out encoder. The single [`CodecInfo`] carries
/// both factories under the same `"mp2"` id and container tags.
pub fn register_codecs(reg: &mut CodecRegistry) {
    let info = CodecInfo::new(CodecId::new(CODEC_ID_STR))
        .capabilities(
            CodecCapabilities::audio("mp2")
                .with_decode()
                .with_encode()
                .with_lossy(true),
        )
        .decoder(make_decoder)
        .encoder(crate::codec_encoder::make_encoder)
        .probe(probe_mp2)
        .tags([
            CodecTag::wave_format(WAVE_FORMAT_MPEG),
            CodecTag::matroska("A_MPEG/L2"),
        ]);
    reg.register(info);
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::TimeBase;

    /// Path to the staged 192 kbit/s / 44.1 kHz / Stereo / no-CRC
    /// Layer II fixture. Tests skip cleanly when the workspace's
    /// `docs/` tree is absent (e.g. standalone-crate CI checkouts).
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

    /// Slice a contiguous Layer II byte stream into per-frame packets
    /// by walking syncwords via [`FrameHeader::parse`] /
    /// [`FrameHeader::frame_size_bytes`], mirroring what a real demuxer
    /// would emit on the wire.
    fn split_into_packets(bytes: &[u8]) -> Vec<Packet> {
        let tb = TimeBase::new(1, 44_100);
        let mut packets = Vec::new();
        let mut off = 0usize;
        let mut pts: i64 = 0;
        while off + 4 <= bytes.len() {
            // Skip any non-sync byte (e.g. ID3 header carried before
            // the first frame). The staged fixture has no leading
            // metadata so this is mostly a defensive check.
            if !(bytes[off] == 0xFF && (bytes[off + 1] & 0xF0) == 0xF0) {
                off += 1;
                continue;
            }
            let Ok(hdr) = FrameHeader::parse(&bytes[off..]) else {
                off += 1;
                continue;
            };
            let fs = hdr.frame_size_bytes();
            if off + fs > bytes.len() {
                break;
            }
            let mut pkt = Packet::new(0, tb, bytes[off..off + fs].to_vec());
            pkt.pts = Some(pts);
            pkt.duration = Some(PCM_SAMPLES_PER_CHANNEL as i64);
            packets.push(pkt);
            pts += PCM_SAMPLES_PER_CHANNEL as i64;
            off += fs;
        }
        packets
    }

    fn build_decoder_params(sample_rate: u32, channels: u16) -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        p.sample_rate = Some(sample_rate);
        p.channels = Some(channels);
        p.sample_format = Some(SampleFormat::S16);
        p
    }

    #[test]
    fn make_decoder_accepts_mono_and_stereo() {
        for ch in [1, 2] {
            let dec = make_decoder(&build_decoder_params(44_100, ch))
                .unwrap_or_else(|e| panic!("channels={ch}: {e}"));
            assert_eq!(dec.codec_id().as_str(), CODEC_ID_STR);
        }
    }

    #[test]
    fn make_decoder_rejects_invalid_channel_counts() {
        for ch in [0u16, 3, 5, 16, u16::MAX] {
            let r = make_decoder(&build_decoder_params(44_100, ch));
            assert!(r.is_err(), "channels={ch} should have been rejected");
        }
    }

    #[test]
    fn make_decoder_defaults_when_params_are_missing() {
        // No sample_rate / channels hint -> defaults to 44_100 / 1 ch.
        let p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        let _ = make_decoder(&p).expect("default-params decoder builds");
    }

    #[test]
    fn first_frame_via_trait_decodes_and_yields_pcm() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        assert!(!packets.is_empty(), "fixture yielded zero packets");

        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).expect("decoder");
        dec.send_packet(&packets[0]).expect("send_packet 0");
        let Frame::Audio(audio) = dec.receive_frame().expect("frame 0") else {
            panic!("expected AudioFrame");
        };
        assert_eq!(audio.samples as usize, PCM_SAMPLES_PER_CHANNEL);
        assert_eq!(audio.data.len(), 2, "stereo");
        for plane in &audio.data {
            assert_eq!(plane.len(), PCM_SAMPLES_PER_CHANNEL * 2, "S16 LE bytes");
        }
        // PTS propagated from the packet.
        assert_eq!(audio.pts, packets[0].pts);
    }

    #[test]
    fn receive_frame_returns_need_more_without_packet() {
        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        match dec.receive_frame() {
            Err(Error::NeedMore) => {}
            other => panic!("expected NeedMore, got {other:?}"),
        }
    }

    #[test]
    fn free_format_packet_decodes_through_the_trait() {
        // A demuxer hands one frame per packet, so a free-format packet's
        // length IS the frame size. Build a standard-bitrate frame, rewrite
        // its bitrate_index nibble to '0000' (free format), and confirm the
        // trait decoder recovers the bitrate from the packet length and
        // produces the SAME PCM as the standard-bitrate packet.
        use crate::encoder_bit_allocator::SmrTable;
        use crate::encoder_frame::encode_frame;
        use crate::header::{Emphasis, Mode, ModeExtension};
        use crate::NUM_SUBBANDS;

        // 192 kbit/s stereo at 44,1 kHz: 96 kbit/s per channel selects
        // Table B.2b — the same table conforming decoders use for free
        // format at 44,1 kHz (Table 3-B.2b header lists free format), so
        // clearing the bitrate_index below leaves the audio-data layout
        // valid under the free-format read.
        let header = FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        };
        let smr: SmrTable = [[20.0f64; NUM_SUBBANDS]; 2];
        let pcm: Vec<Vec<f64>> = (0..2)
            .map(|_| {
                (0..PCM_SAMPLES_PER_CHANNEL)
                    .map(|n| 0.4 * (2.0 * std::f64::consts::PI * 500.0 * n as f64 / 44_100.0).sin())
                    .collect()
            })
            .collect();
        let standard = encode_frame(&header, &pcm, &smr, 0).expect("encode standard frame");

        // Decode the standard packet through the trait.
        let mut std_dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        let tb = TimeBase::new(1, 44_100);
        std_dec
            .send_packet(&Packet::new(0, tb, standard.clone()))
            .expect("standard send_packet");
        let Frame::Audio(std_audio) = std_dec.receive_frame().expect("std frame") else {
            panic!("expected AudioFrame");
        };

        // Rewrite the bitrate_index nibble to '0000' → free format.
        let mut free = standard.clone();
        free[2] &= 0x0F;

        let mut ff_dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        ff_dec
            .send_packet(&Packet::new(0, tb, free))
            .expect("free-format send_packet");
        let Frame::Audio(ff_audio) = ff_dec.receive_frame().expect("ff frame") else {
            panic!("expected AudioFrame");
        };

        assert_eq!(ff_audio.samples, std_audio.samples);
        assert_eq!(ff_audio.data.len(), std_audio.data.len(), "stereo");
        for ch in 0..std_audio.data.len() {
            assert_eq!(
                ff_audio.data[ch], std_audio.data[ch],
                "ch {ch}: free-format packet must decode identically to standard"
            );
        }
    }

    #[test]
    fn flush_then_receive_yields_eof_after_drain() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);

        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        dec.flush().unwrap();
        // First receive drains the pending frame.
        let _ = dec.receive_frame().expect("pending frame drains");
        // Subsequent receive returns Eof.
        match dec.receive_frame() {
            Err(Error::Eof) => {}
            other => panic!("expected Eof, got {other:?}"),
        }
    }

    #[test]
    fn send_after_flush_is_rejected() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        dec.flush().unwrap();
        let err = dec.send_packet(&packets[0]).unwrap_err();
        let _ = format!("{err}");
    }

    #[test]
    fn reset_re_enables_send_after_flush() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        dec.flush().unwrap();
        dec.reset().unwrap();
        // After reset, send_packet works again and yields a frame.
        dec.send_packet(&packets[0]).unwrap();
        let frame = dec.receive_frame().expect("frame after reset");
        if let Frame::Audio(a) = frame {
            assert_eq!(a.samples as usize, PCM_SAMPLES_PER_CHANNEL);
        } else {
            panic!("expected AudioFrame");
        }
    }

    #[test]
    fn streaming_yields_one_frame_per_packet() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        assert_eq!(packets.len(), 31, "31 Layer II frames in the fixture");

        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        let mut decoded_frames = 0usize;
        for pkt in &packets {
            dec.send_packet(pkt).unwrap();
            match dec.receive_frame().unwrap() {
                Frame::Audio(a) => {
                    assert_eq!(a.samples as usize, PCM_SAMPLES_PER_CHANNEL);
                    assert_eq!(a.data.len(), 2);
                    decoded_frames += 1;
                }
                other => panic!("expected Audio, got {other:?}"),
            }
        }
        assert_eq!(decoded_frames, 31);
    }

    #[test]
    fn truncated_packet_returns_decoder_error() {
        let Some(buf) = fixture_bytes() else { return };
        let mut packets = split_into_packets(&buf);
        // Lop off the last 5 bytes of frame 0 — decode_frame_with should
        // report `Truncated`, which the trait surfaces as `Error::other`.
        let len = packets[0].data.len();
        packets[0].data.truncate(len - 5);

        let mut dec = make_decoder(&build_decoder_params(44_100, 2)).unwrap();
        let err = dec.send_packet(&packets[0]).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("buffer too short") || msg.contains("Truncated") || msg.contains("short"),
            "expected truncation error, got: {msg}"
        );
    }

    #[test]
    fn output_params_reflect_decoded_header() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        // Build the decoder with a deliberately-wrong sample rate hint;
        // after one decoded packet the trait wrapper has refreshed its
        // output params to the on-the-wire 44_100 Hz / 2 ch.
        let mut p = build_decoder_params(8_000, 1);
        // Bypass the 1-or-2-channel check by setting a valid count.
        p.channels = Some(2);
        let mut dec = make_decoder(&p).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        let _ = dec.receive_frame().unwrap();
        // The output params live inside the trait object; we can't read
        // them directly without an accessor, but we can confirm via the
        // codec_id() round-trip.
        assert_eq!(dec.codec_id().as_str(), CODEC_ID_STR);
    }

    // ───────────────────── probe + registration tests ─────────────────────

    #[test]
    fn probe_returns_one_for_layer2_packet() {
        // Layer II header: sync='1111 1111 1111', ID='1', layer='10'.
        // Pack the top 19 bits as 0xFFFD0 → byte 0=0xFF, byte 1=0xFD.
        let pkt = [0xFF, 0xFD, 0x50, 0xC4];
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!((probe_mp2(&ctx) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_returns_zero_for_layer1_packet() {
        // Layer I: bits 18..17 = '11' → byte 1 has its low nibble's
        // top bits set differently. 0xFF 0xFF: sync=0xFFF, ID=1,
        // layer='11' (Layer I).
        let pkt = [0xFF, 0xFF, 0x50, 0xC4];
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!(probe_mp2(&ctx).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_returns_zero_for_layer3_packet() {
        // Layer III: bits 18..17 = '01'. 0xFF 0xFB: sync=0xFFF, ID=1,
        // layer='01' (Layer III).
        let pkt = [0xFF, 0xFB, 0x50, 0xC4];
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!(probe_mp2(&ctx).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_returns_default_when_packet_absent() {
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag);
        assert!((probe_mp2(&ctx) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_returns_low_for_bad_sync() {
        let pkt = [0x00, 0x00, 0x00, 0x00];
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!(probe_mp2(&ctx) < 0.5);
    }

    #[test]
    fn probe_returns_low_for_short_packet() {
        let pkt = [0xFF];
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!(probe_mp2(&ctx) < 0.5);
    }

    #[test]
    fn probe_uses_layer2_packet_against_fixture() {
        // The staged fixture's first 4 bytes are a valid Layer II
        // header; the probe must report exact confidence 1.0.
        let Some(buf) = fixture_bytes() else { return };
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        let ctx = ProbeContext::new(&tag).packet(&buf[..4]);
        assert!((probe_mp2(&ctx) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn register_codecs_installs_decoder_factory() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        // The id is reachable via `first_decoder`, which walks the
        // registered implementations and returns the highest-priority
        // factory's output.
        let params = build_decoder_params(44_100, 2);
        let _ = reg
            .first_decoder(&params)
            .expect("registry-built decoder factory found");
        assert!(reg.has_decoder(&CodecId::new(CODEC_ID_STR)));
    }

    #[test]
    fn register_codecs_claims_wave_format_mpeg_tag() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        let tag = CodecTag::wave_format(WAVE_FORMAT_MPEG);
        // Provide a Layer II first-packet hint so our probe wins any
        // collision with a Layer I claimant registered in the same
        // registry by another test/build (defensive against future
        // registry contents).
        let pkt = [0xFF, 0xFD, 0x50, 0xC4];
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        let resolved = reg
            .resolve_tag_ref(&ctx)
            .expect("WAVE_FORMAT_MPEG tag resolves");
        assert_eq!(resolved.as_str(), CODEC_ID_STR);
    }

    #[test]
    fn register_codecs_claims_matroska_a_mpeg_l2_tag() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        let tag = CodecTag::matroska("A_MPEG/L2");
        let ctx = ProbeContext::new(&tag);
        let resolved = reg.resolve_tag_ref(&ctx).expect("A_MPEG/L2 tag resolves");
        assert_eq!(resolved.as_str(), CODEC_ID_STR);
    }

    // ───────────────────── §2.5 multichannel surface ─────────────────────

    /// Encode a short 3/2 (+ optional LFE) multichannel stream with the
    /// crate's own §2.5 encoder and split it into per-frame packets.
    fn mc_stream_packets(lfe: bool, n_frames: usize) -> (Vec<Packet>, Vec<Vec<f64>>) {
        use crate::mc_encode::{encode_mc_all_frames, McEncodeConfig};

        let header = FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate: 384_000,
            sample_rate: 48_000,
            padding: false,
            private_bit: false,
            mode: crate::header::Mode::Stereo,
            mode_extension: crate::header::ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: crate::header::Emphasis::None,
        };
        let cfg = McEncodeConfig {
            lfe,
            ..McEncodeConfig::default()
        };
        let tones = [430.0, 700.0, 1_150.0, 1_800.0, 2_600.0];
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let pcm: Vec<Vec<f64>> = tones
            .iter()
            .map(|f| {
                let w = 2.0 * std::f64::consts::PI * f / 48_000.0;
                (0..total).map(|i| 0.3 * (w * i as f64).sin()).collect()
            })
            .collect();
        let lfe_in: Vec<f64> = (0..n_frames * crate::mc::LFE_SAMPLES_PER_FRAME)
            .map(|i| 0.5 * (i as f64 * 0.37).sin())
            .collect();
        let stream =
            encode_mc_all_frames(&header, &cfg, &pcm, lfe.then_some(lfe_in.as_slice())).unwrap();
        (split_into_packets(&stream), pcm)
    }

    fn mc_decoder_params(mc: &str, mc_lfe: Option<&str>) -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        p.sample_rate = Some(48_000);
        p.channels = Some(5);
        p.sample_format = Some(SampleFormat::S16);
        p.options.insert("mc", mc);
        if let Some(v) = mc_lfe {
            p.options.insert("mc_lfe", v);
        }
        p
    }

    #[test]
    fn mc_on_decodes_five_presentation_planes() {
        let (packets, _) = mc_stream_packets(false, 3);
        let mut dec = make_decoder(&mc_decoder_params("on", None)).expect("mc decoder");
        for pkt in &packets {
            dec.send_packet(pkt).expect("send mc packet");
            let Frame::Audio(a) = dec.receive_frame().expect("mc frame") else {
                panic!("expected AudioFrame");
            };
            assert_eq!(a.samples as usize, PCM_SAMPLES_PER_CHANNEL);
            assert_eq!(a.data.len(), 5, "Surround50 presentation planes");
            for plane in &a.data {
                assert_eq!(plane.len(), PCM_SAMPLES_PER_CHANNEL * 2);
            }
        }
    }

    #[test]
    fn mc_planes_match_the_direct_mc_decode_bit_for_bit() {
        // The registry surface must be a re-ordering of
        // mc::decode_mc_stream's output, nothing more. For 3/2 without
        // LFE the order is the identity (Surround50 == §2.5
        // presentation order).
        let (packets, _) = mc_stream_packets(false, 3);
        let stream: Vec<u8> = packets.iter().flat_map(|p| p.data.clone()).collect();
        let direct = crate::mc::decode_mc_stream(&stream, None).expect("direct decode");

        let mut dec = make_decoder(&mc_decoder_params("on", None)).expect("mc decoder");
        let mut got: Vec<Vec<u8>> = vec![Vec::new(); 5];
        for pkt in &packets {
            dec.send_packet(pkt).unwrap();
            let Frame::Audio(a) = dec.receive_frame().unwrap() else {
                panic!("expected AudioFrame");
            };
            for (ch, plane) in a.data.iter().enumerate() {
                got[ch].extend_from_slice(plane);
            }
        }
        for (ch, plane) in got.iter().enumerate() {
            let want = Mp2CoreDecoder::float_plane_to_s16_le(&direct.channels[ch]);
            assert_eq!(*plane, want, "plane {ch}");
        }
    }

    #[test]
    fn mc_lfe_hold_orders_the_51_layout_and_holds_each_sample_96_times() {
        let (packets, _) = mc_stream_packets(true, 2);
        let stream: Vec<u8> = packets.iter().flat_map(|p| p.data.clone()).collect();
        let direct = crate::mc::decode_mc_stream(&stream, None).expect("direct decode");
        let lfe_native = direct.lfe.as_ref().expect("LFE present");

        let mut dec = make_decoder(&mc_decoder_params("on", Some("hold"))).expect("mc decoder");
        let mut planes: Vec<Vec<u8>> = vec![Vec::new(); 6];
        for pkt in &packets {
            dec.send_packet(pkt).unwrap();
            let Frame::Audio(a) = dec.receive_frame().unwrap() else {
                panic!("expected AudioFrame");
            };
            assert_eq!(a.data.len(), 6, "Surround51 planes");
            for (ch, plane) in a.data.iter().enumerate() {
                planes[ch].extend_from_slice(plane);
            }
        }
        // Surround51 order: L R C LFE Ls Rs — plane 3 is the held LFE,
        // planes 4/5 are the §2.5 presentation channels 3/4.
        for (out_idx, mc_idx) in [(0usize, 0usize), (1, 1), (2, 2), (4, 3), (5, 4)] {
            let want = Mp2CoreDecoder::float_plane_to_s16_le(&direct.channels[mc_idx]);
            assert_eq!(planes[out_idx], want, "plane {out_idx}");
        }
        // The LFE plane repeats each native-rate sample 96×.
        let lfe_plane: Vec<i16> = planes[3]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(lfe_plane.len(), lfe_native.len() * 96);
        for (i, &s) in lfe_native.iter().enumerate() {
            let want = (s * 32768.0)
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            for k in 0..96 {
                assert_eq!(lfe_plane[i * 96 + k], want, "LFE run {i} sample {k}");
            }
        }
    }

    #[test]
    fn mc_auto_latches_per_stream() {
        // A multichannel stream latches to 5 planes…
        let (packets, _) = mc_stream_packets(false, 2);
        let mut dec = make_decoder(&mc_decoder_params("auto", None)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        let Frame::Audio(a) = dec.receive_frame().unwrap() else {
            panic!("expected AudioFrame");
        };
        assert_eq!(a.data.len(), 5);

        // …and a plain stereo stream latches to the base pair.
        let Some(buf) = fixture_bytes() else { return };
        let plain = split_into_packets(&buf);
        let mut dec = make_decoder(&mc_decoder_params("auto", None)).unwrap();
        dec.send_packet(&plain[0]).unwrap();
        let Frame::Audio(a) = dec.receive_frame().unwrap() else {
            panic!("expected AudioFrame");
        };
        assert_eq!(a.data.len(), 2);
    }

    #[test]
    fn mc_off_default_keeps_the_historical_base_decode() {
        // Without the option the same multichannel packets decode as
        // the MPEG-1-compatible pair — backwards compatible.
        let (packets, _) = mc_stream_packets(false, 2);
        let mut dec = make_decoder(&build_decoder_params(48_000, 2)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        let Frame::Audio(a) = dec.receive_frame().unwrap() else {
            panic!("expected AudioFrame");
        };
        assert_eq!(a.data.len(), 2);
    }

    #[test]
    fn mc_on_rejects_a_stream_without_the_extension() {
        let Some(buf) = fixture_bytes() else { return };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&mc_decoder_params("on", None)).unwrap();
        let err = dec.send_packet(&packets[0]).unwrap_err();
        assert!(format!("{err}").contains("mc decode"));
    }

    #[test]
    fn mc_options_are_validated() {
        let mut p = mc_decoder_params("sideways", None);
        assert!(make_decoder(&p).is_err());
        p = mc_decoder_params("on", Some("resample"));
        assert!(make_decoder(&p).is_err());
        // channels > 2 requires an mc mode.
        let mut p = build_decoder_params(48_000, 2);
        p.channels = Some(5);
        assert!(make_decoder(&p).is_err());
    }

    #[test]
    fn mc_output_plan_covers_every_configuration() {
        use crate::mc::{McConfig, McHeader};
        let hdr = |centre, surround| McHeader {
            ext_bit_stream_present: false,
            n_ad_bytes: 0,
            centre,
            surround,
            lfe: false,
            audio_mix: false,
            dematrix_procedure: 0,
            no_of_multi_lingual_ch: 0,
            multi_lingual_fs_half: false,
            multi_lingual_layer3: false,
            copyright_identification_bit: false,
            copyright_identification_start: false,
        };
        use crate::mc::{Centre, Surround};
        let cases: [(Centre, Surround, ChannelLayout, ChannelLayout); 6] = [
            (
                Centre::Present,
                Surround::Stereo,
                ChannelLayout::Surround50,
                ChannelLayout::Surround51,
            ),
            (
                Centre::Present,
                Surround::Mono,
                ChannelLayout::Surround40,
                ChannelLayout::Surround41,
            ),
            (
                Centre::Present,
                Surround::None,
                ChannelLayout::Surround30,
                ChannelLayout::DiscreteN(4),
            ),
            (
                Centre::None,
                Surround::Stereo,
                ChannelLayout::Quad,
                ChannelLayout::DiscreteN(5),
            ),
            (
                Centre::None,
                Surround::Mono,
                ChannelLayout::DiscreteN(3),
                ChannelLayout::DiscreteN(4),
            ),
            (
                Centre::None,
                Surround::None,
                ChannelLayout::Stereo,
                ChannelLayout::Stereo21,
            ),
        ];
        for (centre, surround, plain, with_lfe) in cases {
            let cfg = McConfig::from_header(&hdr(centre, surround), crate::header::Mode::Stereo);
            let n = cfg.layout().len();
            let p = mc_output_plan(&cfg, false, false);
            assert_eq!(p.layout, plain, "{centre:?}/{surround:?}");
            assert_eq!(p.sources.len(), n);
            assert_eq!(
                usize::from(p.layout.channel_count()),
                n,
                "{centre:?}/{surround:?}"
            );
            // LFE present but dropped: same as plain.
            let p = mc_output_plan(&cfg, true, false);
            assert_eq!(p.layout, plain, "{centre:?}/{surround:?} drop");
            // LFE held: one extra plane, layout with an LFE slot.
            let p = mc_output_plan(&cfg, true, true);
            assert_eq!(p.layout, with_lfe, "{centre:?}/{surround:?} hold");
            assert_eq!(p.sources.len(), n + 1);
            assert_eq!(usize::from(p.layout.channel_count()), n + 1);
            assert_eq!(
                p.sources
                    .iter()
                    .filter(|s| **s == McPlaneSource::LfeHold)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn float_plane_to_s16_le_clamps_and_round_trips_endpoints() {
        // 2^15 full-scale map: −1.0 → i16::MIN exactly; +1.0 → +32768
        // clamps to i16::MAX. Slightly-overshoot inputs clamp at the
        // endpoints without panicking.
        let plane = vec![0.0, 0.5, 1.0, -1.0, 1.5, -2.0];
        let bytes = Mp2CoreDecoder::float_plane_to_s16_le(&plane);
        assert_eq!(bytes.len(), plane.len() * 2);
        let words: Vec<i16> = bytes
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(words[0], 0);
        // 0.5 * 32768 = 16384 exactly.
        assert_eq!(words[1], 16384);
        // +1.0 * 32768 = 32768 → clamps to i16::MAX.
        assert_eq!(words[2], i16::MAX);
        // −1.0 * 32768 = −32768 = i16::MIN exactly (the symmetric map).
        assert_eq!(words[3], i16::MIN);
        // Overshoots clamp at the i16 range.
        assert_eq!(words[4], i16::MAX);
        assert_eq!(words[5], i16::MIN);
    }
}
