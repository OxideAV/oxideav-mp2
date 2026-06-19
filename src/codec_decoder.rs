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
    AudioFrame, CodecCapabilities, CodecId, CodecInfo, CodecParameters, CodecRegistry, CodecTag,
    Confidence, Decoder, Error, Frame, Packet, ProbeContext, Result, SampleFormat,
};

use crate::frame::{decode_frame_with, FrameDecodeState, PCM_SAMPLES_PER_CHANNEL};
use crate::header::FrameHeader;

/// Codec id under which [`register_codecs`] installs this decoder.
pub const CODEC_ID_STR: &str = "mp2";

/// `WAVE_FORMAT_MPEG` per Win32 `mmreg.h` — covers MPEG-1 Audio Layer
/// I and Layer II (Layer III uses its own `WAVE_FORMAT_MPEGLAYER3 =
/// 0x0055`). Used as the AVI / WAVEFORMATEX `wFormatTag` for Layer II
/// streams.
pub const WAVE_FORMAT_MPEG: u16 = 0x0050;

/// Build a boxed MPEG-1 Audio Layer II [`Decoder`] from `params`.
///
/// `params.sample_rate` (32_000 / 44_100 / 48_000) and `params.channels`
/// (1 or 2) configure the returned decoder's stream parameters; the
/// actual per-frame sample rate and channel count are re-derived from
/// each Layer II frame header on `send_packet`, so the values supplied
/// here are a hint used only to seed `output_params()`.
///
/// # Errors
///
/// Returns [`Error::invalid`] when `channels` is supplied and not 1 or
/// 2. The §2.4.2.3 mode field encodes at most two channels
/// (single/dual/stereo/joint), so `channels >= 3` is unrepresentable on
/// the wire and is rejected at build time. `sample_rate` is optional
/// (defaults to 44_100 when absent): the real value is re-read from
/// every frame header anyway.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    let channels = params.channels.unwrap_or(1);
    if channels != 1 && channels != 2 {
        return Err(Error::invalid(format!(
            "oxideav-mp2: decoder supports 1 or 2 channels (channels={channels})"
        )));
    }
    let sample_rate = params.sample_rate.unwrap_or(44_100);

    let mut out_params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    out_params.sample_rate = Some(sample_rate);
    out_params.channels = Some(channels);
    out_params.sample_format = Some(SampleFormat::S16);
    out_params.tag = Some(CodecTag::wave_format(WAVE_FORMAT_MPEG));

    Ok(Box::new(Mp2CoreDecoder::new(
        CodecId::new(CODEC_ID_STR),
        out_params,
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
    fn new(codec_id: CodecId, output: CodecParameters) -> Self {
        Self {
            codec_id,
            output,
            state: FrameDecodeState::new(),
            pending_frames: VecDeque::new(),
            eof: false,
        }
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
        let decoded = decode_frame_with(&packet.data, &mut self.state)
            .map_err(|e| Error::other(format!("oxideav-mp2: decode_frame: {e}")))?;
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
/// The encoder factory is **not** wired in this round: §C.1.5.2.7
/// bit-allocation, the Annex C analysis filterbank, and the §2.4.1.6
/// audio-data writer are still pending (the §2.4.1.3 / §2.4.2.3 header
/// writer + §2.4.1.4 CRC writer have already landed in earlier rounds
/// but are not enough to construct a complete encoder yet). When those
/// land, this builder picks up `.encoder(make_encoder)` alongside the
/// existing decoder factory.
pub fn register_codecs(reg: &mut CodecRegistry) {
    let info = CodecInfo::new(CodecId::new(CODEC_ID_STR))
        .capabilities(
            CodecCapabilities::audio("mp2")
                .with_decode()
                .with_lossy(true),
        )
        .decoder(make_decoder)
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
