//! End-to-end encode → decode tests for the MP2 encoder's
//! **dual_channel** mode emission.
//!
//! Per ISO/IEC 11172-3 §2.4.2.3 Layer II defines four channel modes:
//!
//! - `00` stereo               (two channels of one signal)
//! - `01` joint_stereo         (intensity stereo above a bound)
//! - `10` dual_channel         (two unrelated channels — e.g. bilingual)
//! - `11` single_channel       (mono)
//!
//! The dual_channel bitstream layout is byte-identical to plain stereo
//! (every subband per-channel-independent, no shared coefficients);
//! only the header's 2-bit `mode` field differs. Acceptance:
//!
//! 1. With `dual_channel = true` on stereo input every emitted frame's
//!    header carries `mode = DualChannel` and the bitstream decodes
//!    cleanly.
//! 2. Round-tripped PCM matches the plain-stereo emission sample-for-
//!    sample (since the encoder takes the same bit-allocation paths
//!    either way).
//! 3. `joint_stereo = true` overrides `dual_channel = true` — the
//!    intensity-stereo path is preferred when both are requested.
//! 4. The option is silently ignored on mono input (the resulting
//!    stream stays in `mode = Mono`).

use oxideav_core::options::CodecOptions;
use oxideav_core::{AudioFrame, CodecId, CodecParameters, Frame, Packet, SampleFormat, TimeBase};
use oxideav_mp2::decoder::make_decoder;
use oxideav_mp2::encoder::make_encoder;
use oxideav_mp2::header::{parse_header, Mode};
use oxideav_mp2::CODEC_ID_STR;

/// Two-tone PCM where L and R carry *unrelated* signals — exactly the
/// kind of input dual_channel mode is meant to flag.
fn make_unrelated_stereo(duration_s: f32, sample_rate: u32) -> Vec<u8> {
    let n = (duration_s * sample_rate as f32) as usize;
    let mut out = Vec::with_capacity(n * 2 * 2);
    let two_pi = 2.0f32 * std::f32::consts::PI;
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        // L: 440 Hz + 660 Hz; R: 523 Hz + 784 Hz. Different tonal
        // content per channel, mimicking two-language broadcast.
        let l = ((two_pi * 440.0 * t).sin() + (two_pi * 660.0 * t).sin() * 0.5) * 0.3;
        let r = ((two_pi * 523.0 * t).sin() + (two_pi * 784.0 * t).sin() * 0.5) * 0.3;
        let li = (l.clamp(-1.0, 1.0) * 32767.0) as i16;
        let ri = (r.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&li.to_le_bytes());
        out.extend_from_slice(&ri.to_le_bytes());
    }
    out
}

fn make_mono(duration_s: f32, sample_rate: u32) -> Vec<u8> {
    let n = (duration_s * sample_rate as f32) as usize;
    let mut out = Vec::with_capacity(n * 2);
    let two_pi = 2.0f32 * std::f32::consts::PI;
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let s = ((two_pi * 440.0 * t).sin() * 0.3).clamp(-1.0, 1.0);
        let v = (s * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encode a complete stream. `opts` is consulted verbatim; the caller
/// is responsible for setting `dual_channel`, `joint_stereo`, etc.
fn encode(pcm: &[u8], sample_rate: u32, channels: u16, opts: CodecOptions) -> Vec<u8> {
    let mut params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    params.channels = Some(channels);
    params.sample_rate = Some(sample_rate);
    params.sample_format = Some(SampleFormat::S16);
    params.bit_rate = Some(192_000);
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

/// Every frame emitted with `dual_channel = true` on stereo input
/// must announce `Mode::DualChannel` in its header.
#[test]
fn dual_channel_option_sets_header_mode() {
    let sr = 44_100u32;
    let pcm = make_unrelated_stereo(0.5, sr);
    let opts = CodecOptions::new().set("dual_channel", "true");
    let bytes = encode(&pcm, sr, 2, opts);
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty(), "no frames produced");
    for (i, f) in frames.iter().enumerate() {
        let h = parse_header(f).expect("frame header parse");
        assert_eq!(
            h.mode,
            Mode::DualChannel,
            "frame {i} should be DualChannel, got {:?}",
            h.mode
        );
    }
}

/// The dual_channel bitstream is byte-identical to plain stereo
/// except for the 2-bit `mode` field in each header. Both decode to
/// the same PCM (the encoder runs the same bit-allocation path for
/// either mode).
#[test]
fn dual_channel_pcm_matches_plain_stereo() {
    let sr = 44_100u32;
    let pcm = make_unrelated_stereo(0.5, sr);

    let stereo_bytes = encode(&pcm, sr, 2, CodecOptions::new());
    let dual_bytes = encode(&pcm, sr, 2, CodecOptions::new().set("dual_channel", "true"));

    // The payload bodies (everything past byte 0 of each frame
    // header) should match. The 2-bit mode is at bits 54..56 of the
    // 64-bit frame prefix — i.e. inside byte 3. Verify that all other
    // header bytes and the entire payload past byte 3 are identical.
    assert_eq!(
        stereo_bytes.len(),
        dual_bytes.len(),
        "frame lengths diverged"
    );

    let stereo_frames = split_frames(&stereo_bytes);
    let dual_frames = split_frames(&dual_bytes);
    assert_eq!(stereo_frames.len(), dual_frames.len());
    for (sf, df) in stereo_frames.iter().zip(dual_frames.iter()) {
        // Byte 0,1,2 of header are the sync + version + layer +
        // protection + bitrate + sample_rate + padding + private —
        // identical between the two modes.
        assert_eq!(sf[0..2], df[0..2]);
        // Byte 2 (sr_idx/padding/private) is bits 16..23 — bit 16=sr,
        // bits 18,19=padding/private. All zero / identical.
        assert_eq!(sf[2], df[2]);
        // Byte 3 differs only in bits 7..6 (mode field): stereo=00,
        // dual_channel=10. Mask bit 7 to verify everything else
        // (mode_ext, copyright, original, emphasis) matches.
        assert_eq!(sf[3] & 0b0011_1111, df[3] & 0b0011_1111);
        // Bytes past the header are the bit allocation + scalefactors
        // + sample payload — the encoder takes the same code path
        // for stereo and dual_channel so they must match exactly.
        assert_eq!(sf[4..], df[4..]);
    }

    // Both decode to identical PCM.
    let dparams = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    let tb = TimeBase::new(1, sr as i64);
    let decode_all = |frames: &[&[u8]]| -> Vec<u8> {
        let mut dec = make_decoder(&dparams).expect("decoder");
        let mut out = Vec::new();
        for f in frames {
            let pkt = Packet::new(0, tb, f.to_vec());
            if dec.send_packet(&pkt).is_ok() {
                if let Ok(Frame::Audio(a)) = dec.receive_frame() {
                    out.extend_from_slice(&a.data[0]);
                }
            }
        }
        out
    };
    let stereo_pcm = decode_all(&stereo_frames);
    let dual_pcm = decode_all(&dual_frames);
    assert_eq!(
        stereo_pcm.len(),
        dual_pcm.len(),
        "decoded PCM lengths differ"
    );
    assert!(
        stereo_pcm == dual_pcm,
        "dual_channel decoded PCM must match plain stereo (both decoders treat the modes identically)"
    );
}

/// `joint_stereo = true` wins over `dual_channel = true` because the
/// `mode` header field can only carry one value at a time. Joint
/// stereo and dual channel are mutually exclusive in the spec — the
/// implementation gives precedence to joint stereo per the documented
/// behaviour.
#[test]
fn joint_stereo_overrides_dual_channel() {
    let sr = 44_100u32;
    // Make a highly-correlated stereo signal so joint stereo will
    // actually engage (otherwise the encoder falls back to plain
    // stereo, which would mask the joint-stereo precedence claim).
    let n = (0.5 * sr as f32) as usize;
    let mut pcm = Vec::with_capacity(n * 4);
    let two_pi = 2.0f32 * std::f32::consts::PI;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let s = (two_pi * 660.0 * t).sin() * 0.3
            + (two_pi * 1320.0 * t).sin() * 0.2
            + (two_pi * 2640.0 * t).sin() * 0.1;
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
        pcm.extend_from_slice(&v.to_le_bytes());
    }

    let opts = CodecOptions::new()
        .set("joint_stereo", "true")
        .set("dual_channel", "true");
    let bytes = encode(&pcm, sr, 2, opts);
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    // Expect at least one joint-stereo frame; *no* dual_channel
    // frames. (Some frames may legitimately fall back to plain
    // stereo if the correlation check fails for a given allocation,
    // but none should be dual_channel.)
    let mut n_js = 0usize;
    let mut n_dc = 0usize;
    for f in &frames {
        let h = parse_header(f).expect("header");
        match h.mode {
            Mode::JointStereo => n_js += 1,
            Mode::DualChannel => n_dc += 1,
            _ => {}
        }
    }
    assert_eq!(
        n_dc, 0,
        "dual_channel must not appear when joint_stereo overrides"
    );
    assert!(
        n_js > 0,
        "expected at least one JointStereo frame on correlated input"
    );
}

/// `dual_channel = true` is silently ignored for mono input — the
/// resulting bitstream stays in `mode = Mono` (`0b11`).
#[test]
fn dual_channel_ignored_for_mono() {
    let sr = 44_100u32;
    let pcm = make_mono(0.25, sr);
    let bytes = encode(&pcm, sr, 1, CodecOptions::new().set("dual_channel", "true"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());
    for f in &frames {
        let h = parse_header(f).expect("header");
        assert_eq!(h.mode, Mode::Mono, "mono input should stay Mono");
    }
}

/// Round-trip a dual_channel stream and confirm both channels keep
/// their respective input frequencies — i.e. the two channels are
/// treated as independent.
#[test]
fn dual_channel_roundtrip_preserves_per_channel_signal() {
    let sr = 44_100u32;
    let pcm = make_unrelated_stereo(0.5, sr);
    let bytes = encode(&pcm, sr, 2, CodecOptions::new().set("dual_channel", "true"));
    let frames = split_frames(&bytes);
    assert!(!frames.is_empty());

    let dparams = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    let mut dec = make_decoder(&dparams).expect("decoder");
    let tb = TimeBase::new(1, sr as i64);
    let mut samples_l = Vec::<f32>::new();
    let mut samples_r = Vec::<f32>::new();
    for f in &frames {
        let pkt = Packet::new(0, tb, f.to_vec());
        if dec.send_packet(&pkt).is_ok() {
            if let Ok(Frame::Audio(a)) = dec.receive_frame() {
                for ch in a.data[0].chunks_exact(4) {
                    let l = i16::from_le_bytes([ch[0], ch[1]]) as f32 / 32768.0;
                    let r = i16::from_le_bytes([ch[2], ch[3]]) as f32 / 32768.0;
                    samples_l.push(l);
                    samples_r.push(r);
                }
            }
        }
    }
    assert!(samples_l.len() > 1152, "too few samples decoded");
    let e_l: f32 = samples_l.iter().map(|s| s * s).sum::<f32>() / samples_l.len() as f32;
    let e_r: f32 = samples_r.iter().map(|s| s * s).sum::<f32>() / samples_r.len() as f32;
    assert!(e_l > 1e-4, "L channel silent: {e_l}");
    assert!(e_r > 1e-4, "R channel silent: {e_r}");

    // Cross-correlation between L and R should be LOW because they
    // carry unrelated tones (440/660 vs 523/784). If it were high we
    // would have accidentally collapsed the channels.
    let n = samples_l.len().min(samples_r.len());
    let mean_l: f32 = samples_l[..n].iter().sum::<f32>() / n as f32;
    let mean_r: f32 = samples_r[..n].iter().sum::<f32>() / n as f32;
    let mut num = 0.0f32;
    let mut den_l = 0.0f32;
    let mut den_r = 0.0f32;
    for i in 0..n {
        let dl = samples_l[i] - mean_l;
        let dr = samples_r[i] - mean_r;
        num += dl * dr;
        den_l += dl * dl;
        den_r += dr * dr;
    }
    let corr = num / (den_l.sqrt() * den_r.sqrt() + 1e-12);
    eprintln!("L/R Pearson correlation = {corr:.3}");
    assert!(
        corr.abs() < 0.5,
        "unrelated channels should have |corr| < 0.5, got {corr}"
    );
}
