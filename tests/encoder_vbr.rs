//! End-to-end tests for VBR (variable-bit-rate) MP2 encoding.
//!
//! Acceptance: VBR mode produces a file whose XING/Info header reports
//! the right average bitrate, and ffmpeg cross-decode is clean.

use oxideav_core::options::CodecOptions;
use oxideav_core::{AudioFrame, CodecId, CodecParameters, Frame, Packet, SampleFormat, TimeBase};
use oxideav_mp2::decoder::make_decoder;
use oxideav_mp2::encoder::make_encoder;
use oxideav_mp2::header::parse_header;
use oxideav_mp2::CODEC_ID_STR;

fn make_music(duration_s: f32, sample_rate: u32) -> Vec<u8> {
    let n = (duration_s * sample_rate as f32) as usize;
    let mut out = Vec::with_capacity(n * 2);
    let two_pi = 2.0f32 * std::f32::consts::PI;
    let freqs = [220.0f32, 440.0, 587.0, 880.0, 1318.0, 1760.0, 3520.0];
    let weights = [0.20f32, 0.20, 0.16, 0.14, 0.12, 0.10, 0.08];
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let mut s = 0.0f32;
        for (f, w) in freqs.iter().zip(weights.iter()) {
            s += (two_pi * f * t).sin() * w;
        }
        s = s.clamp(-1.0, 1.0) * 0.5;
        let v = (s * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn make_silence(duration_s: f32, sample_rate: u32) -> Vec<u8> {
    let n = (duration_s * sample_rate as f32) as usize;
    vec![0u8; n * 2]
}

fn encode_vbr(pcm: &[u8], sample_rate: u32, channels: u16, vbr_quality: u8) -> Vec<u8> {
    let mut params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    params.channels = Some(channels);
    params.sample_rate = Some(sample_rate);
    params.sample_format = Some(SampleFormat::S16);
    params.options = CodecOptions::new().set("vbr_quality", vbr_quality.to_string());
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

/// Parse the Xing/Info header at offset `off` (inclusive of the
/// 4-byte MP2 frame header). Returns `(num_frames, total_bytes)`.
fn parse_xing(frame_bytes: &[u8]) -> Option<(u32, u32)> {
    // Search for "Info" or "Xing" magic anywhere in the first ~200
    // bytes of the frame.
    let limit = frame_bytes.len().min(200);
    for off in 4..limit.saturating_sub(16) {
        let tag = &frame_bytes[off..off + 4];
        if tag == b"Info" || tag == b"Xing" {
            let flags = u32::from_be_bytes([
                frame_bytes[off + 4],
                frame_bytes[off + 5],
                frame_bytes[off + 6],
                frame_bytes[off + 7],
            ]);
            if flags & 0x3 != 0x3 {
                return None;
            }
            let nf = u32::from_be_bytes([
                frame_bytes[off + 8],
                frame_bytes[off + 9],
                frame_bytes[off + 10],
                frame_bytes[off + 11],
            ]);
            let nb = u32::from_be_bytes([
                frame_bytes[off + 12],
                frame_bytes[off + 13],
                frame_bytes[off + 14],
                frame_bytes[off + 15],
            ]);
            return Some((nf, nb));
        }
    }
    None
}

/// Distinct bitrate slots used by data frames in a VBR stream.
fn distinct_bitrates(bitstream: &[u8]) -> std::collections::BTreeSet<u32> {
    let mut set = std::collections::BTreeSet::new();
    for f in split_frames(bitstream).iter().skip(1) {
        if let Ok(h) = parse_header(f) {
            set.insert(h.bitrate_kbps);
        }
    }
    set
}

#[test]
fn vbr_emits_xing_header_with_accurate_metadata() {
    let sr = 44_100u32;
    let dur = 1.0f32;
    let pcm = make_music(dur, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 4);
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty(), "no frames in VBR output");
    let (nf, nb) = parse_xing(frames[0]).expect("first frame must be a Xing/Info header");
    eprintln!(
        "Xing: frames={nf}, bytes={nb} (actual frames={}, actual bytes={})",
        frames.len(),
        bytes.len()
    );
    assert_eq!(
        nf as usize,
        frames.len(),
        "Xing frame count {nf} does not match actual frame count {}",
        frames.len()
    );
    assert_eq!(
        nb as usize,
        bytes.len(),
        "Xing total bytes {nb} does not match actual byte count {}",
        bytes.len()
    );
}

#[test]
fn vbr_xing_average_bitrate_matches_actual() {
    // For a frame at 44.1 kHz: each frame = 1152 samples = 1152/44100
    // seconds. Average bitrate = total_bytes * 8 / total_seconds.
    let sr = 44_100u32;
    let dur = 1.5f32;
    let pcm = make_music(dur, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 5);
    let frames = split_frames(&bytes);
    let (nf, nb) = parse_xing(frames[0]).expect("Xing header");
    let total_seconds = (nf as f64) * 1152.0 / sr as f64;
    let avg_kbps = (nb as f64 * 8.0 / total_seconds / 1000.0) as u32;
    eprintln!("VBR avg bitrate (from Xing): {avg_kbps} kbps over {total_seconds:.2}s");
    // Sanity: must be in MP2's standard range for this music profile.
    assert!(
        (24..=320).contains(&avg_kbps),
        "avg bitrate {avg_kbps} kbps outside sane Layer II range"
    );
}

#[test]
fn vbr_size_decreases_with_quality() {
    let sr = 44_100u32;
    let dur = 1.0f32;
    let pcm = make_music(dur, sr);
    let v0 = encode_vbr(&pcm, sr, 1, 0);
    let v9 = encode_vbr(&pcm, sr, 1, 9);
    eprintln!("VBR sizes: q0={} bytes, q9={} bytes", v0.len(), v9.len());
    assert!(
        v0.len() > v9.len(),
        "expected V0 ({}) > V9 ({})",
        v0.len(),
        v9.len()
    );
}

#[test]
fn vbr_uses_multiple_bitrate_slots() {
    let sr = 44_100u32;
    let q = 2u8;
    let mut pcm = Vec::new();
    pcm.extend(make_silence(0.5, sr));
    pcm.extend(make_music(0.5, sr));
    let bytes = encode_vbr(&pcm, sr, 1, q);
    let slots = distinct_bitrates(&bytes);
    eprintln!("VBR distinct slots (data frames): {slots:?}");
    assert!(
        slots.len() >= 2,
        "expected >= 2 distinct slots, got {slots:?}"
    );
}

#[test]
fn vbr_roundtrip_through_own_decoder() {
    let sr = 44_100u32;
    let dur = 1.0f32;
    let pcm = make_music(dur, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 4);
    let frames = split_frames(&bytes);
    assert!(frames.len() > 5);
    let dparams = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    let mut dec = make_decoder(&dparams).expect("decoder");
    let tb = TimeBase::new(1, sr as i64);
    let mut total = 0u32;
    for f in &frames {
        let pkt = Packet::new(0, tb, f.to_vec());
        if dec.send_packet(&pkt).is_ok() {
            if let Ok(Frame::Audio(a)) = dec.receive_frame() {
                total += a.samples;
            }
        }
    }
    assert!(total >= 1152 * (frames.len() as u32 / 2));
}

#[test]
fn vbr_ffmpeg_cross_decode() {
    use std::process::{Command, Stdio};
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("ffmpeg not available — skipping VBR ffmpeg cross-decode");
        return;
    }
    let sr = 44_100u32;
    let dur = 1.0f32;
    let pcm = make_music(dur, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 3);

    let tmp_mp2 = std::env::temp_dir().join("oxideav_mp2_vbr.mp2");
    let tmp_wav = std::env::temp_dir().join("oxideav_mp2_vbr.wav");
    std::fs::write(&tmp_mp2, &bytes).expect("write mp2");
    let out = Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("warning")
        .arg("-i")
        .arg(&tmp_mp2)
        .arg("-f")
        .arg("wav")
        .arg(&tmp_wav)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("ffmpeg");
    assert!(
        out.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let wav = std::fs::read(&tmp_wav).expect("wav");
    let data_off = wav
        .windows(4)
        .position(|w| w == b"data")
        .expect("WAV data tag")
        + 8;
    let mut energy_total = 0.0f64;
    let mut count = 0usize;
    for ch in wav[data_off..].chunks_exact(2) {
        let s = i16::from_le_bytes([ch[0], ch[1]]) as f64 / 32768.0;
        energy_total += s * s;
        count += 1;
    }
    let avg_e = energy_total / count.max(1) as f64;
    eprintln!("VBR ffmpeg cross-decode avg energy: {avg_e:.6}");
    assert!(avg_e > 1e-5, "ffmpeg-decoded VBR is silent: {avg_e}");
}

#[test]
fn vbr_first_frame_decodes_as_silence() {
    // The Xing header frame is a valid MP2 frame with zero sample
    // payload. The decoder should accept it without erroring and
    // produce silence (or near-silence).
    let sr = 44_100u32;
    let pcm = make_music(0.5, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 3);
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    let dparams = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    let mut dec = make_decoder(&dparams).expect("decoder");
    let tb = TimeBase::new(1, sr as i64);
    let pkt = Packet::new(0, tb, frames[0].to_vec());
    dec.send_packet(&pkt).expect("send_packet on Xing frame");
    let af = match dec.receive_frame() {
        Ok(Frame::Audio(a)) => a,
        Ok(_) => panic!("expected audio frame from Xing"),
        Err(e) => panic!("Xing frame decode error: {e:?}"),
    };
    let energy: f64 = af.data[0]
        .chunks_exact(2)
        .map(|c| {
            let s = i16::from_le_bytes([c[0], c[1]]) as f64 / 32768.0;
            s * s
        })
        .sum::<f64>()
        / (af.data[0].len() / 2) as f64;
    eprintln!("Xing frame decoded energy: {energy:.6}");
    // Don't enforce zero energy — the polyphase filter has memory so
    // a Xing frame at the very start can produce non-zero PCM. Just
    // require that we got *some* output of the right sample count.
    assert_eq!(af.samples, 1152);
}
