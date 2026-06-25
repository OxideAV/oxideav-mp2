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
use crate::encoder_frame::{encode_frame_auto_with, EncodeFrameState};
use crate::frame::PCM_SAMPLES_PER_CHANNEL;
use crate::header::{is_layer2_bitrate_mode_allowed, Emphasis, FrameHeader, Mode, ModeExtension};

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

/// Map a channel count to the Layer II [`Mode`] the encoder emits.
///
/// 1 → `SingleChannel`, 2 → `Stereo`. (Dual-channel and joint-stereo
/// are valid Layer II modes but the registry encoder emits plain stereo
/// for the 2-channel case; callers needing intensity stereo drive
/// [`encode_frame_auto_with`] directly with a `JointStereo` header.)
fn mode_for_channels(channels: u16) -> Option<Mode> {
    match channels {
        1 => Some(Mode::SingleChannel),
        2 => Some(Mode::Stereo),
        _ => None,
    }
}

/// Build the fixed per-stream [`FrameHeader`] from encoder parameters,
/// validating the rate / channel / bitrate combination up front.
fn build_header(sample_rate: u32, mode: Mode, bit_rate: u32) -> Result<FrameHeader> {
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
        protection_bit: true, // no CRC (inverted §2.4.2.3 convention)
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
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
/// # Errors
///
/// * `channels` not 1 or 2 — the §2.4.2.3 mode field encodes at most
///   two channels.
/// * `sample_rate` absent or not a Layer II rate.
/// * `bit_rate` (if supplied) violates the §2.4.2.3 matrix or has no
///   §2.4.3.1 allocation table for the rate.
pub fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    let channels = params.channels.unwrap_or(2);
    let Some(mode) = mode_for_channels(channels) else {
        return Err(Error::invalid(format!(
            "oxideav-mp2: encoder supports 1 or 2 channels (channels={channels})"
        )));
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

    let header = build_header(sample_rate, mode, bit_rate)?;

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
    fn new(codec_id: CodecId, output: CodecParameters, header: FrameHeader) -> Self {
        let channels = header.channels();
        Self {
            codec_id,
            output,
            header,
            channels,
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
        let bytes = encode_frame_auto_with(&self.header, &frame_pcm, 0, &mut self.enc_state)
            .map_err(|e| Error::other(format!("oxideav-mp2: encode_frame: {e}")))?;
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
        let fsz = build_header(44_100, Mode::Stereo, 192_000)
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
        let fsz = build_header(44_100, Mode::SingleChannel, 128_000)
            .unwrap()
            .frame_size_bytes();
        let p0 = enc.receive_packet().expect("first frame");
        assert_eq!(p0.data.len(), fsz);
        assert!(matches!(enc.receive_packet(), Err(Error::NeedMore)));

        enc.flush().unwrap();
        let p1 = enc.receive_packet().expect("padded trailing frame");
        assert_eq!(p1.data.len(), fsz);
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
