//! End-to-end encode → re-parse tests for the MP2 encoder's header
//! **metadata flag** options: `copyright`, `original`, `emphasis`, and
//! `private_bit`.
//!
//! Per ISO/IEC 11172-3 §2.4.2.3 the trailing bits of the 32-bit Layer
//! II header carry:
//!
//! - `private_bit` (1 bit, bit 8) — reserved for private use.
//! - `copyright` (1 bit, bit 3) — `1` = copyright protected.
//! - `original/copy` (1 bit, bit 2) — `1` = original, `0` = copy.
//! - `emphasis` (2 bits, 1..0) — `00` none / `01` 50-15 µs / `10`
//!   reserved / `11` CCITT J.17.
//!
//! These fields are pure metadata: they do not alter the audio payload.
//! Acceptance is black-box — encode a stream with each option set, then
//! re-parse every emitted frame header and confirm the field round-trips
//! to the requested value, while the audio body is byte-identical to the
//! default (flags-clear) emission.

use oxideav_core::options::CodecOptions;
use oxideav_core::{AudioFrame, CodecId, CodecParameters, Frame, SampleFormat};
use oxideav_mp2::encoder::make_encoder;
use oxideav_mp2::header::{parse_header, Emphasis};
use oxideav_mp2::CODEC_ID_STR;

fn make_tone(duration_s: f32, sample_rate: u32, channels: u16) -> Vec<u8> {
    let n = (duration_s * sample_rate as f32) as usize;
    let mut out = Vec::with_capacity(n * 2 * channels as usize);
    let two_pi = 2.0f32 * std::f32::consts::PI;
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let s = ((two_pi * 440.0 * t).sin() * 0.3).clamp(-1.0, 1.0);
        let v = (s * 32767.0) as i16;
        for _ in 0..channels {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn encode(pcm: &[u8], sample_rate: u32, channels: u16, opts: CodecOptions) -> Vec<u8> {
    // 192 kbps is valid for MPEG-1 (32/44.1/48 kHz). MPEG-2 LSF
    // (16/22.05/24 kHz) caps the ladder at 160 kbps, so pick a valid
    // LSF slot for those rates.
    let bit_rate = match sample_rate {
        16_000 | 22_050 | 24_000 => 128_000,
        _ => 192_000,
    };
    let mut params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    params.channels = Some(channels);
    params.sample_rate = Some(sample_rate);
    params.sample_format = Some(SampleFormat::S16);
    params.bit_rate = Some(bit_rate);
    params.options = opts;
    let mut enc = make_encoder(&params).expect("encoder");
    let total_samples = (pcm.len() / (2 * channels as usize)) as u32;
    let frame = AudioFrame {
        samples: total_samples,
        pts: Some(0),
        data: vec![pcm.to_vec()],
    };
    enc.send_frame(&Frame::Audio(frame)).expect("send_frame");
    let mut bytes = Vec::new();
    while let Ok(p) = enc.receive_packet() {
        bytes.extend_from_slice(&p.data);
    }
    enc.flush().expect("flush");
    while let Ok(p) = enc.receive_packet() {
        bytes.extend_from_slice(&p.data);
    }
    bytes
}

fn split_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        if data[i] != 0xFF || (data[i + 1] & 0xF0) != 0xF0 {
            i += 1;
            continue;
        }
        let Ok(h) = parse_header(&data[i..]) else {
            i += 1;
            continue;
        };
        let len = h.frame_length();
        if i + len > data.len() {
            break;
        }
        frames.push(&data[i..i + len]);
        i += len;
    }
    frames
}

#[test]
fn copyright_bit_round_trips() {
    let sr = 44_100u32;
    let pcm = make_tone(0.4, sr, 2);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new().set("copyright", "true"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty(), "no frames produced");
    for (i, f) in frames.iter().enumerate() {
        let h = parse_header(f).expect("header parse");
        assert!(h.copyright, "frame {i} copyright bit not set");
    }
}

#[test]
fn copyright_default_is_clear() {
    let sr = 48_000u32;
    let pcm = make_tone(0.3, sr, 2);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new());
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).unwrap();
        assert!(!h.copyright, "default copyright bit should be clear");
        assert!(!h.original, "default original bit should be clear");
        assert_eq!(
            h.emphasis,
            Emphasis::None,
            "default emphasis should be None"
        );
        assert!(!h.private_bit, "default private_bit should be clear");
    }
}

#[test]
fn original_bit_round_trips() {
    let sr = 44_100u32;
    let pcm = make_tone(0.4, sr, 1);
    let bytes = encode(&pcm, sr, 1, CodecOptions::new().set("original", "true"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).unwrap();
        assert!(h.original, "original bit not set");
    }
}

#[test]
fn private_bit_round_trips() {
    let sr = 32_000u32;
    let pcm = make_tone(0.4, sr, 2);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new().set("private_bit", "true"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).unwrap();
        assert!(h.private_bit, "private_bit not set");
    }
}

#[test]
fn emphasis_5015_round_trips() {
    let sr = 48_000u32;
    let pcm = make_tone(0.3, sr, 2);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new().set("emphasis", "50/15"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).unwrap();
        assert_eq!(h.emphasis, Emphasis::FiftyFifteen);
    }
}

#[test]
fn emphasis_ccitt_round_trips() {
    let sr = 24_000u32; // MPEG-2 LSF
    let pcm = make_tone(0.3, sr, 2);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new().set("emphasis", "ccitt"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).unwrap();
        assert_eq!(h.emphasis, Emphasis::CcittJ17);
    }
}

#[test]
fn invalid_emphasis_string_rejected() {
    let mut params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    params.channels = Some(2);
    params.sample_rate = Some(44_100);
    params.sample_format = Some(SampleFormat::S16);
    params.bit_rate = Some(192_000);
    params.options = CodecOptions::new().set("emphasis", "bogus");
    assert!(
        make_encoder(&params).is_err(),
        "an unknown emphasis string must be rejected at construction"
    );
}

/// All four flags set at once must each round-trip, and the audio body
/// (everything past the 4-byte header) must be byte-identical to the
/// default emission — the flags are pure metadata.
#[test]
fn all_flags_compose_and_body_unchanged() {
    let sr = 44_100u32;
    let pcm = make_tone(0.5, sr, 2);

    let plain = encode(&pcm, sr, 2, CodecOptions::new());
    let flagged = encode(
        &pcm,
        sr,
        2,
        CodecOptions::new()
            .set("copyright", "true")
            .set("original", "true")
            .set("private_bit", "true")
            .set("emphasis", "ccitt"),
    );

    let plain_frames = split_frames(&plain);
    let flagged_frames = split_frames(&flagged);
    assert_eq!(
        plain_frames.len(),
        flagged_frames.len(),
        "frame counts diverged"
    );
    assert!(!plain_frames.is_empty());

    for (pf, ff) in plain_frames.iter().zip(flagged_frames.iter()) {
        let h = parse_header(ff).unwrap();
        assert!(h.copyright);
        assert!(h.original);
        assert!(h.private_bit);
        assert_eq!(h.emphasis, Emphasis::CcittJ17);
        // The audio body (post-header) is unaffected by the flags.
        assert_eq!(
            pf[4..],
            ff[4..],
            "metadata flags must not change the audio payload"
        );
    }
}
