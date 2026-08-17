//! ISO/IEC 13818-3 §2.5 multichannel **encode → decode** round trip.
//!
//! The `mc_encode` module emits a Layer II base frame whose §2.4.1.8
//! ancillary field carries the `mc_extension()`; this suite closes the
//! loop through the crate's own §2.5 decoder (`mc::decode_mc_stream`)
//! and pins:
//!
//! 1. **Every emittable configuration round-trips** — 3/2, 3/1, 3/0,
//!    2/2, 2/1 × dematrix procedures `'00'` / `'01'` / `'11'`: correct
//!    layout, exact sample counts, per-channel reconstruction energy
//!    and per-channel spectral localisation (each presentation channel
//!    carries a *distinct* tone, so a dematrixing error that leaks one
//!    channel into another fails the localisation pin, not just the
//!    energy envelope).
//! 2. **MPEG-1 backward compatibility** (§2.5.1.3): a §2.5-unaware
//!    decode of the same bytes yields the compatible downmix
//!    `Lo = α(L + βC + γLS)` — checked against the matrix equations.
//! 3. **LFE** (§2.5.3.2.4): a 12-samples-per-frame LFE channel
//!    round-trips within its block-companding quantization step.
//! 4. **Wire premises**: `has_mc_extension` fires, the decoded
//!    `mc_header` matches the configuration, no dynamic crosstalk /
//!    prediction frames are signalled, and tampering a
//!    §2.5.2.14-protected bit is detected as `McCrcMismatch`.
//! 5. **Silence** round-trips to exact-zero presentation channels.
//! 6. The §2.4.2.3 padding schedule at 44,1 kHz interoperates with the
//!    extension splice (frame sizes vary, the extension still parses).
//!
//! Clean-room basis: ISO/IEC 13818-3 (1997) §2.5 via this crate's own
//! `mc` / `mc_encode` modules; assertions follow the bounded-difference
//! conformance convention of `tests/roundtrip_multirate.rs`.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::mc::{decode_mc_frame_with, decode_mc_stream, has_mc_extension, McDecodeState};
use oxideav_mp2::mc_encode::{
    encode_mc_all_frames, encode_mc_frame_with, McEncodeConfig, McEncodeError, McEncodeState,
};
use oxideav_mp2::{decode_all_frames, FrameHeader, McError, PCM_SAMPLES_PER_CHANNEL};

/// Combined analysis + synthesis filterbank group delay (samples) —
/// same constant `tests/roundtrip_multirate.rs` uses.
const FILTERBANK_DELAY: usize = 480;

const SQRT2: f64 = std::f64::consts::SQRT_2;

fn base_header(sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf: false,
        protection_bit: true, // '1' == no CRC (§2.4.2.3 inverted convention)
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    }
}

/// Distinct per-channel tone frequencies (Hz) — all well inside the
/// Table B.2a/B.2b `msblimit` bandwidth at every MPEG-1 rate, and far
/// enough apart that a channel leak shows up in the Goertzel probes.
const CHANNEL_TONES_HZ: [f64; 5] = [430.0, 700.0, 1_150.0, 1_800.0, 2_600.0];

fn tone_streams(channels: usize, sample_rate: u32, n_frames: usize, amp: f64) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    (0..channels)
        .map(|ch| {
            let omega = 2.0 * std::f64::consts::PI * CHANNEL_TONES_HZ[ch] / f64::from(sample_rate);
            (0..total).map(|i| amp * (omega * i as f64).sin()).collect()
        })
        .collect()
}

fn goertzel_power(signal: &[f64], freq_hz: f64, sample_rate: u32) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq_hz / f64::from(sample_rate);
    let coeff = 2.0 * w.cos();
    let (mut s_prev, mut s_prev2) = (0.0f64, 0.0f64);
    for &x in signal {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2
}

/// Residual-vs-delayed-original energy ratio over the steady middle.
fn residual_ratio(input: &[f64], output: &[f64]) -> f64 {
    let total = output.len();
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = total - PCM_SAMPLES_PER_CHANNEL;
    assert!(hi > lo);
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in lo..hi {
        let want = input[i - FILTERBANK_DELAY];
        let e = output[i] - want;
        sig += want * want;
        err += e * e;
    }
    assert!(sig > 0.0);
    err / sig
}

#[test]
fn every_configuration_round_trips_with_channel_separation() {
    let n_frames = 6;
    let sample_rate = 48_000;
    let header = base_header(sample_rate, 384_000);
    for (front, surround) in [(3u8, 2u8), (3, 1), (3, 0), (2, 2), (2, 1)] {
        for proc_ in [0u8, 1, 3] {
            let cfg = McEncodeConfig {
                front,
                surround,
                dematrix_procedure: proc_,
                ..McEncodeConfig::default()
            };
            let channels = cfg.presentation_channels();
            let pcm = tone_streams(channels, sample_rate, n_frames, 0.30);
            let stream = encode_mc_all_frames(&header, &cfg, &pcm, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} proc {proc_}: encode: {e}"));
            let decoded = decode_mc_stream(&stream, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} proc {proc_}: decode: {e}"));

            assert_eq!(decoded.frames, n_frames, "{front}/{surround} proc {proc_}");
            assert_eq!(
                decoded.channels.len(),
                channels,
                "{front}/{surround} proc {proc_}"
            );
            assert_eq!(decoded.dyn_cross_frames, 0);
            assert_eq!(decoded.prediction_frames, 0);
            assert!(decoded.lfe.is_none());
            assert!(decoded.multilingual.is_empty());

            for (ch, out) in decoded.channels.iter().enumerate() {
                assert_eq!(out.len(), n_frames * PCM_SAMPLES_PER_CHANNEL);
                let ratio = residual_ratio(&pcm[ch], out);
                assert!(
                    ratio < 0.5,
                    "{front}/{surround} proc {proc_} ch {ch}: residual ratio {ratio:.4}"
                );
                // Channel separation: this channel's own tone must
                // dominate every *other* channel's tone frequency and
                // an unrelated probe — a dematrix leak fails here.
                let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
                let hi = out.len() - PCM_SAMPLES_PER_CHANNEL;
                let steady = &out[lo..hi];
                let own = goertzel_power(steady, CHANNEL_TONES_HZ[ch], sample_rate);
                for (other, &f) in CHANNEL_TONES_HZ[..channels].iter().enumerate() {
                    if other == ch {
                        continue;
                    }
                    let leak = goertzel_power(steady, f, sample_rate);
                    assert!(
                        own > 20.0 * leak.max(f64::MIN_POSITIVE),
                        "{front}/{surround} proc {proc_} ch {ch}: tone {own:.3e} vs \
                         leak from ch {other} {leak:.3e}"
                    );
                }
                let probe = goertzel_power(steady, 7_000.0, sample_rate);
                assert!(
                    own > 50.0 * probe.max(f64::MIN_POSITIVE),
                    "{front}/{surround} proc {proc_} ch {ch}: tone {own:.3e} vs probe {probe:.3e}"
                );
            }
        }
    }
}

#[test]
fn base_decode_of_an_mc_stream_is_the_compatible_downmix() {
    // §2.5.1.3: a §2.5-unaware Layer II decoder must play the
    // MPEG-1-compatible pair. Check the plain decode against the
    // §2.5.3.3 matrix equations Lo = α(L + βC + γLS).
    let n_frames = 6;
    let sample_rate = 48_000;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig::default(); // 3/2, proc '00'
    let pcm = tone_streams(5, sample_rate, n_frames, 0.30);
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");

    let plain = decode_all_frames(&stream).expect("plain base decode");
    assert_eq!(plain.len(), 2);

    let (alpha, beta, gamma) = (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2);
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let mut want_lo = vec![0.0f64; total];
    let mut want_ro = vec![0.0f64; total];
    for i in 0..total {
        want_lo[i] = alpha * (pcm[0][i] + beta * pcm[2][i] + gamma * pcm[3][i]);
        want_ro[i] = alpha * (pcm[1][i] + beta * pcm[2][i] + gamma * pcm[4][i]);
    }
    for (want, got) in [(&want_lo, &plain[0]), (&want_ro, &plain[1])] {
        let ratio = residual_ratio(want, got);
        assert!(ratio < 0.5, "compatible-pair residual ratio {ratio:.4}");
    }
}

#[test]
fn lfe_round_trips_within_the_companding_step() {
    let n_frames = 4;
    let header = base_header(48_000, 384_000);
    for (front, surround) in [(2u8, 0u8), (3, 2)] {
        let cfg = McEncodeConfig {
            front,
            surround,
            lfe: true,
            ..McEncodeConfig::default()
        };
        let channels = cfg.presentation_channels();
        let pcm = tone_streams(channels, 48_000, n_frames, 0.25);
        // A slow ramp across the LFE's own ±(near full scale) range.
        let n_lfe = n_frames * oxideav_mp2::LFE_SAMPLES_PER_FRAME;
        let lfe_in: Vec<f64> = (0..n_lfe)
            .map(|i| -0.9 + 1.8 * i as f64 / (n_lfe - 1) as f64)
            .collect();
        let stream =
            encode_mc_all_frames(&header, &cfg, &pcm, Some(&lfe_in)).expect("encode with LFE");
        let decoded = decode_mc_stream(&stream, None).expect("decode with LFE");
        let lfe_out = decoded.lfe.expect("LFE present");
        assert_eq!(lfe_out.len(), n_lfe, "{front}/{surround}");
        // Default lfe_allocation = 7 → nb = 8 bits; the §2.5.3.2.4
        // requantisation step at scalefactor ≈ 1 is ≈ 2^-6 ≈ 0.016;
        // allow the step plus the Table B.1 scalefactor granularity.
        for (i, (&got, &want)) in lfe_out.iter().zip(&lfe_in).enumerate() {
            assert!(
                (got - want).abs() < 0.03,
                "{front}/{surround} LFE sample {i}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn wire_premises_hold_and_the_crc_protects_the_header() {
    let header = base_header(48_000, 384_000);
    let cfg = McEncodeConfig {
        front: 3,
        surround: 2,
        lfe: true,
        dematrix_procedure: 1,
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, 48_000, 1, 0.30);
    let lfe = vec![0.1f64; oxideav_mp2::LFE_SAMPLES_PER_FRAME];
    let mut state = McEncodeState::new();
    let frame = encode_mc_frame_with(&header, &cfg, &pcm, Some(&lfe), &mut state).expect("encode");

    // §2.5.3.1 CRC-detection probe fires.
    assert!(has_mc_extension(&frame));

    let mut dstate = McDecodeState::new();
    let (decoded, ext_used) = decode_mc_frame_with(&frame, None, &mut dstate).expect("decode");
    assert_eq!(ext_used, 0);
    assert!(!decoded.mc_header.ext_bit_stream_present);
    assert_eq!(decoded.mc_header.n_ad_bytes, 0);
    assert_eq!(decoded.mc_header.dematrix_procedure, 1);
    assert!(decoded.mc_header.lfe);
    assert!(!decoded.mc_header.multi_lingual_layer3);
    assert_eq!(decoded.mc_header.no_of_multi_lingual_ch, 0);
    assert_eq!(decoded.config.front, 3);
    assert_eq!(decoded.config.surround, 2);
    assert_eq!(decoded.config.nmch, 3);
    assert!(!decoded.dyn_cross_on);
    assert!(!decoded.mc_prediction_on);
    assert_eq!(decoded.channels.len(), 5);

    // Tampering a bit inside the §2.5.2.14-protected region (the
    // mc_header rides the first ancillary bits) must be detected.
    let plain = oxideav_mp2::frame::decode_frame(&frame).expect("base decode");
    let anc_start = frame.len() as u64 * 8 - plain.ancillary.bits as u64;
    let mut tampered = frame.clone();
    let flip = anc_start + 3; // inside mc_header
    tampered[(flip / 8) as usize] ^= 1 << (7 - (flip % 8) as u32);
    let mut dstate = McDecodeState::new();
    match decode_mc_frame_with(&tampered, None, &mut dstate) {
        Err(McError::McCrcMismatch { .. }) => {}
        other => panic!("expected McCrcMismatch, got {other:?}"),
    }
}

#[test]
fn silence_round_trips_to_exact_zero() {
    let header = base_header(48_000, 384_000);
    let cfg = McEncodeConfig::default();
    let pcm = vec![vec![0.0f64; 3 * PCM_SAMPLES_PER_CHANNEL]; 5];
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode silence");
    let decoded = decode_mc_stream(&stream, None).expect("decode silence");
    for (ch, out) in decoded.channels.iter().enumerate() {
        assert!(
            out.iter().all(|&s| s == 0.0),
            "channel {ch} not exactly zero"
        );
    }
}

#[test]
fn undersized_budget_is_refused_not_truncated() {
    let header = base_header(48_000, 384_000);
    let cfg = McEncodeConfig {
        mc_bits: Some(64),
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, 48_000, 1, 0.3);
    let mut state = McEncodeState::new();
    match encode_mc_frame_with(&header, &cfg, &pcm, None, &mut state) {
        Err(McEncodeError::BudgetTooSmall { fixed, budget }) => {
            assert!(fixed > budget);
        }
        other => panic!("expected BudgetTooSmall, got {other:?}"),
    }
}

#[test]
fn fractional_rate_padding_interoperates_with_the_extension() {
    // 44,1 kHz pads per §2.4.2.3; every frame — N and N+1 slots alike —
    // must carry a parseable extension at its own ancillary offset.
    let n_frames = 8;
    let header = base_header(44_100, 320_000);
    let cfg = McEncodeConfig::default();
    let pcm = tone_streams(5, 44_100, n_frames, 0.30);
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode 44.1");
    let decoded = decode_mc_stream(&stream, None).expect("decode 44.1");
    assert_eq!(decoded.frames, n_frames);
    assert_eq!(decoded.channels.len(), 5);
    for (ch, out) in decoded.channels.iter().enumerate() {
        let ratio = residual_ratio(&pcm[ch], out);
        assert!(ratio < 0.5, "44.1 kHz ch {ch}: residual ratio {ratio:.4}");
    }
}

#[test]
fn shape_errors_are_reported() {
    let header = base_header(48_000, 384_000);
    let cfg = McEncodeConfig::default();
    let mut state = McEncodeState::new();
    // Wrong channel count.
    let pcm4 = tone_streams(4, 48_000, 1, 0.3);
    assert!(matches!(
        encode_mc_frame_with(&header, &cfg, &pcm4, None, &mut state),
        Err(McEncodeError::BadPcmShape { have: 4, need: 5 })
    ));
    // LFE supplied without cfg.lfe.
    let pcm5 = tone_streams(5, 48_000, 1, 0.3);
    let lfe = vec![0.0f64; oxideav_mp2::LFE_SAMPLES_PER_FRAME];
    assert!(matches!(
        encode_mc_frame_with(&header, &cfg, &pcm5, Some(&lfe), &mut state),
        Err(McEncodeError::BadLfeShape { .. })
    ));
    // LSF base refused.
    let mut lsf_header = base_header(48_000, 160_000);
    lsf_header.lsf = true;
    lsf_header.sample_rate = 24_000;
    assert!(matches!(
        encode_mc_frame_with(&lsf_header, &cfg, &pcm5, None, &mut state),
        Err(McEncodeError::BadBaseHeader(_))
    ));
}
