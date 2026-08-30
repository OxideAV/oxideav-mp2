//! Equal-rate encoder quality A/B against the installed **black-box
//! reference encoder** (opaque binary invocation only — the workspace
//! clean-room policy bars reading any implementation source, and this
//! harness never does; it compares *outputs*).
//!
//! Both encoders consume the same stereo programme material (a
//! multi-tone with slow amplitude modulation, the fixture-generation
//! convention) at the same signalled bitrate; both bitstreams are
//! decoded by THIS crate's §2.4.3 decoder (the conformance-validated
//! common denominator), and the delay-searched SNR of each chain is
//! measured, along with a per-tone level table (the "per-band"
//! comparison at the material's spectral lines).
//!
//! Assertions are deliberately robust (they gate regressions, not
//! bragging rights):
//!
//! * our chain clears an absolute floor at every rate;
//! * our chain stays within 6 dB of the reference chain (in practice
//!   it measures at least comparable — the printed table is the
//!   deliverable).
//!
//! Skips silently when no reference binary is installed.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{decode_all_frames, encode_all_frames, FrameHeader, PCM_SAMPLES_PER_CHANNEL};
use std::process::Command;

const SAMPLE_RATE: u32 = 48_000;
const N_FRAMES: usize = 25; // 0,6 s per case

/// Multi-tone stems (Hz) — spread across the Layer II subband range.
const TONES_HZ: [f64; 6] = [430.0, 1_150.0, 2_600.0, 4_900.0, 7_900.0, 11_300.0];

fn programme(channel: usize, total: usize) -> Vec<f64> {
    let fs = f64::from(SAMPLE_RATE);
    (0..total)
        .map(|i| {
            let t = i as f64 / fs;
            let am = 0.65 + 0.35 * (2.0 * std::f64::consts::PI * (3.0 + channel as f64) * t).sin();
            let mut s = 0.0;
            for (k, &f) in TONES_HZ.iter().enumerate() {
                let a = 0.16 / (1.0 + 0.35 * k as f64);
                let phase = channel as f64 * 0.7 + k as f64;
                s += a * (2.0 * std::f64::consts::PI * f * t + phase).sin();
            }
            am * s
        })
        .collect()
}

fn goertzel_power(signal: &[f64], freq_hz: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq_hz / f64::from(SAMPLE_RATE);
    let coeff = 2.0 * w.cos();
    let (mut s_prev, mut s_prev2) = (0.0f64, 0.0f64);
    for &x in signal {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    (s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2) / signal.len() as f64
}

/// Best-lag SNR (dB) of `output` against `input`, searching the lag —
/// each chain carries its own (encoder + decoder) delay.
fn best_lag_snr_db(input: &[f64], output: &[f64]) -> (usize, f64) {
    let probe = 8 * PCM_SAMPLES_PER_CHANNEL;
    let start = 2 * PCM_SAMPLES_PER_CHANNEL;
    let mut best = (0usize, f64::NEG_INFINITY);
    for lag in 0..2_000usize {
        if start + lag + probe > output.len() || start + probe > input.len() {
            break;
        }
        let (mut dot, mut e_in, mut e_out) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..probe {
            let a = input[start + i];
            let b = output[start + lag + i];
            dot += a * b;
            e_in += a * a;
            e_out += b * b;
        }
        let rho = dot / (e_in * e_out).sqrt().max(f64::MIN_POSITIVE);
        if rho > best.1 {
            best = (lag, rho);
        }
    }
    let lag = best.0;
    let lo = 2 * PCM_SAMPLES_PER_CHANNEL;
    let hi = (input.len() - PCM_SAMPLES_PER_CHANNEL).min(output.len() - lag);
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in lo..hi {
        let e = output[i + lag] - input[i];
        sig += input[i] * input[i];
        err += e * e;
    }
    (lag, 10.0 * (sig / err.max(f64::MIN_POSITIVE)).log10())
}

fn interleave_f64le(pcm: &[Vec<f64>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pcm[0].len() * pcm.len() * 8);
    for i in 0..pcm[0].len() {
        for ch in pcm {
            out.extend_from_slice(&ch[i].to_le_bytes());
        }
    }
    out
}

fn reference_encode(pcm: &[Vec<f64>], bit_rate: u32) -> Option<Vec<u8>> {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return None;
    }
    let dir = std::env::temp_dir();
    let stem = format!("oxideav-mp2-r453-ab-{}-{bit_rate}", std::process::id());
    let raw = dir.join(format!("{stem}.f64"));
    let mp2 = dir.join(format!("{stem}.mp2"));
    std::fs::write(&raw, interleave_f64le(pcm)).ok()?;
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "f64le", "-ar"])
        .arg(SAMPLE_RATE.to_string())
        .args(["-ac", "2", "-i"])
        .arg(&raw)
        .args(["-c:a", "mp2", "-b:a"])
        .arg(format!("{}", bit_rate))
        .arg(&mp2)
        .output()
        .ok()?;
    let bytes = out.status.success().then(|| std::fs::read(&mp2).ok())??;
    let _ = std::fs::remove_file(&raw);
    let _ = std::fs::remove_file(&mp2);
    Some(bytes)
}

#[test]
fn equal_rate_snr_tracks_the_reference_encoder() {
    let total = N_FRAMES * PCM_SAMPLES_PER_CHANNEL;
    let pcm: Vec<Vec<f64>> = (0..2).map(|ch| programme(ch, total)).collect();
    let mut any = false;
    for bit_rate in [128_000u32, 192_000, 256_000, 384_000] {
        let Some(ref_stream) = reference_encode(&pcm, bit_rate) else {
            eprintln!("skip: no reference encoder binary installed");
            return;
        };
        any = true;
        let header = FrameHeader {
            lsf: false,
            protection_bit: true,
            bit_rate,
            sample_rate: SAMPLE_RATE,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        };
        let our_stream = encode_all_frames(&header, &pcm, 0).expect("our encode");
        // Equal rate by construction: same signalled bitrate; sizes may
        // differ by at most one frame of stream-edge slack.
        assert!(
            our_stream.len().abs_diff(ref_stream.len()) <= 2 * (header.frame_size_bytes() + 1),
            "stream sizes diverge: ours {} vs reference {}",
            our_stream.len(),
            ref_stream.len()
        );
        let ours = decode_all_frames(&our_stream).expect("decode ours");
        let refd = decode_all_frames(&ref_stream).expect("decode reference");

        for ch in 0..2 {
            let (our_lag, our_snr) = best_lag_snr_db(&pcm[ch], &ours[ch]);
            let (ref_lag, ref_snr) = best_lag_snr_db(&pcm[ch], &refd[ch]);
            // Per-tone level table (the per-band view at the
            // material's spectral lines).
            let probe = |out: &[f64], lag: usize| -> Vec<f64> {
                let s = &out
                    [lag + PCM_SAMPLES_PER_CHANNEL..lag + (N_FRAMES - 1) * PCM_SAMPLES_PER_CHANNEL];
                let i = &pcm[ch][PCM_SAMPLES_PER_CHANNEL..(N_FRAMES - 1) * PCM_SAMPLES_PER_CHANNEL];
                TONES_HZ
                    .iter()
                    .map(|&f| 10.0 * (goertzel_power(s, f) / goertzel_power(i, f)).log10())
                    .collect()
            };
            let our_tones = probe(&ours[ch], our_lag);
            let ref_tones = probe(&refd[ch], ref_lag);
            println!(
                "{} kbit/s ch {ch}: ours {our_snr:.1} dB (lag {our_lag}) vs reference \
                 {ref_snr:.1} dB (lag {ref_lag})",
                bit_rate / 1000
            );
            for (k, f) in TONES_HZ.iter().enumerate() {
                println!(
                    "    {f:>7.0} Hz: level delta ours {:+.2} dB / reference {:+.2} dB",
                    our_tones[k], ref_tones[k]
                );
            }
            // Absolute floor: 128 kbit/s stereo multitone measures
            // ~18 dB on either chain (masking-driven allocation, not
            // waveform fidelity); the floor guards regressions only.
            assert!(
                our_snr >= 15.0,
                "{bit_rate} ch {ch}: our chain below the absolute floor ({our_snr:.1} dB)"
            );
            assert!(
                our_snr >= ref_snr - 6.0,
                "{bit_rate} ch {ch}: ours {our_snr:.1} dB more than 6 dB behind the \
                 reference {ref_snr:.1} dB"
            );
            // Per-tone level fidelity: no audible-band tone collapses
            // (each within 3 dB of its input level).
            for (k, f) in TONES_HZ.iter().enumerate() {
                assert!(
                    our_tones[k].abs() < 3.0,
                    "{bit_rate} ch {ch}: tone {f} Hz level delta {:+.2} dB",
                    our_tones[k]
                );
            }
        }
    }
    assert!(any);
}
