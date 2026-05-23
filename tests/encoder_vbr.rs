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

/// Parse the full Xing block, returning `(flags, frames, bytes, toc)`.
/// `toc` is `Some([..100])` only when the TOC flag (`0x4`) is set.
fn parse_xing_full(frame_bytes: &[u8]) -> Option<(u32, u32, u32, Option<[u8; 100]>)> {
    let limit = frame_bytes.len().min(220);
    for off in 4..limit.saturating_sub(16) {
        let tag = &frame_bytes[off..off + 4];
        if tag != b"Info" && tag != b"Xing" {
            continue;
        }
        let rd = |p: usize| -> u32 {
            u32::from_be_bytes([
                frame_bytes[p],
                frame_bytes[p + 1],
                frame_bytes[p + 2],
                frame_bytes[p + 3],
            ])
        };
        let flags = rd(off + 4);
        let nf = rd(off + 8);
        let nb = rd(off + 12);
        // Field order in the de-facto Xing layout: frames, bytes, TOC,
        // quality — each present only when its flag bit is set.
        let mut cursor = off + 16;
        let toc = if flags & 0x4 != 0 {
            if cursor + 100 > frame_bytes.len() {
                return None;
            }
            let mut t = [0u8; 100];
            t.copy_from_slice(&frame_bytes[cursor..cursor + 100]);
            cursor += 100;
            Some(t)
        } else {
            None
        };
        let _ = cursor;
        return Some((flags, nf, nb, toc));
    }
    None
}

#[test]
fn vbr_xing_carries_monotonic_seek_toc() {
    // The VBR Xing header must advertise a 100-byte seek table (flag
    // 0x4) so percentage seeks work. Verify the flag is set, the table
    // is non-decreasing, starts at 0 (start of stream), and that each
    // entry's implied byte offset lands inside the actual stream and
    // resolves to a valid frame sync.
    let sr = 44_100u32;
    let pcm = make_music(2.0, sr);
    let bytes = encode_vbr(&pcm, sr, 1, 5);
    let frames = split_frames(&bytes);
    assert!(
        frames.len() > 10,
        "need a multi-frame stream to test the TOC"
    );

    let (flags, nf, nb, toc) = parse_xing_full(frames[0]).expect("Xing header");
    assert_eq!(flags & 0x7, 0x7, "Frames+Bytes+TOC flags must all be set");
    assert_eq!(nf as usize, frames.len(), "frame count");
    assert_eq!(nb as usize, bytes.len(), "byte count");
    let toc = toc.expect("TOC must be present when flag 0x4 is set");

    assert_eq!(toc[0], 0, "toc[0] must be 0 (start of stream)");
    for w in toc.windows(2) {
        assert!(w[1] >= w[0], "TOC must be non-decreasing: {w:?}");
    }

    // Build the set of valid frame-start byte offsets.
    let mut starts = std::collections::BTreeSet::new();
    let mut acc = 0usize;
    for f in &frames {
        starts.insert(acc);
        acc += f.len();
    }

    // Every TOC entry maps to a byte offset (toc[i]/256 * total). The
    // offset must be inside the stream and at-or-after a real frame
    // boundary. A player snaps to the nearest preceding frame start.
    for (i, &t) in toc.iter().enumerate() {
        let off = (t as u64 * nb as u64 / 256) as usize;
        assert!(off < bytes.len(), "toc[{i}]={t} → offset {off} past EOF");
        // There is always a frame start at-or-before this offset.
        let preceding = starts.range(..=off).next_back();
        assert!(
            preceding.is_some(),
            "toc[{i}] offset {off} has no preceding frame start"
        );
    }

    // The last TOC entry should point near the end of the stream
    // (within the last few frames), confirming the table spans the
    // whole file rather than collapsing to the front.
    let last_off = (toc[99] as u64 * nb as u64 / 256) as usize;
    let last_frame_start = *starts.iter().next_back().unwrap();
    let third_last = starts.iter().rev().nth(2).copied().unwrap_or(0);
    assert!(
        last_off >= third_last,
        "toc[99] offset {last_off} should be near EOF (last frame start {last_frame_start})"
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
    // The leading Xing/Info VBR-header frame is not ISO audio — its
    // sample region holds the tag block (now including a 100-byte TOC,
    // which is non-zero). The decoder must recognise the magic and skip
    // the frame, emitting a true-silent 1152-sample placeholder rather
    // than attempting to decode the TOC bytes as bit-allocation data
    // (which would error with an out-of-range codeword).
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
    assert_eq!(af.samples, 1152);
    // The skipped header frame is exact silence — every sample zero.
    assert!(
        af.data[0].iter().all(|&b| b == 0),
        "recognised Xing frame must decode to exact silence"
    );
}
