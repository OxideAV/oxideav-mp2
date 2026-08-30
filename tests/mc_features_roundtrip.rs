//! ISO/IEC 13818-3 §2.5 multichannel encode — round trips for the
//! r453 encoder remainders, all closed through this crate's own §2.5
//! decoder (and, where the reference binary is installed, the
//! black-box base-layer acceptance check):
//!
//! 1. **Phase-mixed surround encode** (`dematrix_procedure` `'10'`,
//!    §C.2.1.5 `Lo = Lw + Cw − jSw`): every 3/2 and 3/1
//!    `tc_allocation` arm decodes with correct channel separation
//!    (arms 5 collapse the surround to its transmitted mono component
//!    by construction — those pin the jS recovery instead).
//! 2. **Signal-adaptive per-subband-group `tc_allocation`**
//!    (§C.2.1.6, `tc_sbgr_select = '0'`): band-disjoint material
//!    makes the election vary across groups; the stream must still
//!    round-trip with full separation.
//! 3. **Phantom-centre coding** (`centre = '11'`, §C.2.1.9): the
//!    centre's low band survives in C, its high band reappears in
//!    L/R at −3 dB, and the wire signals `Centre::Phantom`.
//! 4. **Second stereo programme** (`surround = '11'`): 3/0+2/0 and
//!    2/0+2/0, `L2` / `R2` transmitted unmatrixed and separated.
//! 5. **Multilingual channels** (§2.5.2.18): full-rate and half-rate
//!    `ml_audio_data()`, per-channel reconstruction quality.
//! 6. **Extension bit stream** (§2.5.1.5): an over-capacity budget
//!    spills into `ext_frame()`s that the decoder consumes; a
//!    fits-in-base configuration pairs every frame with a header-only
//!    extension frame.
//! 7. **Dynamic crosstalk election** (§C.2.1.7): strongly correlated
//!    surround content trips `dyn_cross_on` and still round-trips;
//!    band-disjoint content must NOT trip it.
//!
//! Clean-room basis: ISO/IEC 13818-3 (1997) §2.5 / Annex C.2 via this
//! crate's own `mc` / `mc_encode` modules; the reference decoder is
//! invoked as an opaque binary only (workspace black-box policy).

// Channel loops deliberately index parallel per-channel buffers (the
// spec's own `for (ch…)` notation, as in the sibling suites), and the
// syncword compare keeps its full-byte mask for legibility.
#![allow(clippy::needless_range_loop, clippy::identity_op)]

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::mc::{decode_mc_stream, has_mc_extension};
use oxideav_mp2::mc_encode::{
    encode_mc_all_frames, encode_mc_all_frames_ext, encode_mc_frame_ext_with, McEncodeConfig,
    McEncodeError, McEncodeState,
};
use oxideav_mp2::{Centre, FrameHeader, Surround, PCM_SAMPLES_PER_CHANNEL};

/// Combined analysis + synthesis filterbank group delay (samples).
/// The exact cross-correlation peak is 481 (the ISO-suite harness
/// compensates exactly that); tone material at higher frequencies is
/// sensitive to the last sample, so the residual metric below searches
/// this candidate set instead of assuming one value.
const FILTERBANK_DELAYS: [usize; 2] = [480, 481];
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

/// Distinct per-channel tone frequencies (Hz), same spacing rationale
/// as `tests/mc_encode_roundtrip.rs`, two extra slots for `L2` / `R2`
/// and multilingual feeds.
const CHANNEL_TONES_HZ: [f64; 7] = [430.0, 700.0, 1_150.0, 1_800.0, 2_600.0, 3_400.0, 4_200.0];

fn tone(freq_hz: f64, sample_rate: u32, total: usize, amp: f64) -> Vec<f64> {
    let omega = 2.0 * std::f64::consts::PI * freq_hz / f64::from(sample_rate);
    (0..total).map(|i| amp * (omega * i as f64).sin()).collect()
}

fn tone_streams(channels: usize, sample_rate: u32, n_frames: usize, amp: f64) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    (0..channels)
        .map(|ch| tone(CHANNEL_TONES_HZ[ch], sample_rate, total, amp))
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

/// Residual-vs-delayed-original energy ratio over the steady middle,
/// with a caller-chosen guard (one frame's samples at the signal's
/// own rate), minimised over the filterbank-delay candidates.
fn residual_ratio_guarded(input: &[f64], output: &[f64], guard: usize) -> f64 {
    let mut best = f64::INFINITY;
    for delay in FILTERBANK_DELAYS {
        let total = output.len();
        let lo = delay + guard;
        let hi = total - guard;
        assert!(hi > lo);
        let (mut sig, mut err) = (0.0f64, 0.0f64);
        for i in lo..hi {
            let want = input[i - delay];
            let e = output[i] - want;
            sig += want * want;
            err += e * e;
        }
        assert!(sig > 0.0);
        best = best.min(err / sig);
    }
    best
}

fn residual_ratio(input: &[f64], output: &[f64]) -> f64 {
    residual_ratio_guarded(input, output, PCM_SAMPLES_PER_CHANNEL)
}

fn steady(out: &[f64]) -> &[f64] {
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = out.len() - PCM_SAMPLES_PER_CHANNEL;
    &out[lo..hi]
}

/// Assert each decoded channel carries its own tone and none of the
/// listed other tones.
fn assert_separation(
    label: &str,
    channels: &[Vec<f64>],
    tones: &[f64],
    sample_rate: u32,
    check: &[usize],
) {
    for &ch in check {
        let s = steady(&channels[ch]);
        let own = goertzel_power(s, tones[ch], sample_rate);
        for (other, &f) in tones.iter().enumerate() {
            if other == ch || !check.contains(&other) {
                continue;
            }
            let leak = goertzel_power(s, f, sample_rate);
            assert!(
                own > 20.0 * leak.max(f64::MIN_POSITIVE),
                "{label} ch {ch}: tone {own:.3e} vs leak from ch {other} {leak:.3e}"
            );
        }
    }
}

// -------------------------------------------------------------------
// 1. Phase-mixed surround ('10')
// -------------------------------------------------------------------

#[test]
fn phase_mixed_surround_round_trips_through_every_tc_arm() {
    let sample_rate = 48_000;
    let n_frames = 4;
    let header = base_header(sample_rate, 384_000);
    for (front, surround, tc_values) in [(3u8, 2u8, 0..=7u8), (3, 1, 0..=5u8)] {
        for tc in tc_values {
            let cfg = McEncodeConfig {
                front,
                surround,
                dematrix_procedure: 2,
                tc_allocation: tc,
                ..McEncodeConfig::default()
            };
            let channels = cfg.presentation_channels();
            let pcm = tone_streams(channels, sample_rate, n_frames, 0.28);
            let stream = encode_mc_all_frames(&header, &cfg, &pcm, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} '10' tc {tc}: encode: {e}"));
            let decoded = decode_mc_stream(&stream, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} '10' tc {tc}: decode: {e}"));
            assert_eq!(decoded.mc_header.dematrix_procedure, 2);
            assert_eq!(decoded.channels.len(), channels);

            // Arm 5 transmits all three front channels; only the mono
            // surround jS survives (both surround outputs carry it) —
            // §2.5.3.2.1.1's '10' tables have no LS/RS recovery there.
            let mono_surround_arm = tc == 5;
            let front_n = usize::from(front);
            let check: Vec<usize> = if mono_surround_arm {
                (0..front_n).collect()
            } else {
                (0..channels).collect()
            };
            assert_separation(
                &format!("{front}/{surround} '10' tc {tc}"),
                &decoded.channels,
                &CHANNEL_TONES_HZ,
                sample_rate,
                &check,
            );
            for ch in 0..channels {
                let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
                if mono_surround_arm && ch >= front_n {
                    continue;
                }
                assert!(
                    ratio < 0.5,
                    "{front}/{surround} '10' tc {tc} ch {ch}: residual {ratio:.4}"
                );
            }
            if mono_surround_arm {
                // The surround outputs must both equal the recovered
                // mono component: their difference is (near) zero.
                let a = steady(&decoded.channels[front_n]);
                let b = steady(&decoded.channels[channels - 1]);
                let diff: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>();
                let energy: f64 = a.iter().map(|x| x * x).sum::<f64>();
                if surround == 2 {
                    assert!(
                        diff < 1e-3 * energy.max(1e-12),
                        "tc 5: surround outputs differ ({diff:.3e} vs {energy:.3e})"
                    );
                }
                // …and jS = (LS + RS)/2 (or S) is actually present.
                let js_want: Vec<f64> = if surround == 2 {
                    pcm[3]
                        .iter()
                        .zip(&pcm[4])
                        .map(|(x, y)| 0.5 * (x + y))
                        .collect()
                } else {
                    pcm[3].clone()
                };
                let ratio = residual_ratio(&js_want, &decoded.channels[front_n]);
                assert!(ratio < 0.5, "tc 5: jS residual {ratio:.4}");
            }
        }
    }
}

// -------------------------------------------------------------------
// 2. Signal-adaptive per-subband-group tc_allocation
// -------------------------------------------------------------------

#[test]
fn adaptive_tc_allocation_round_trips_on_band_disjoint_material() {
    // Give the centre a low band and the front pair a high band so the
    // §C.2.1.6 rule (quietest maximum scalefactor rides T2..T4) elects
    // different rows in different subband groups.
    let sample_rate = 48_000;
    let n_frames = 4;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        adaptive_tc: true,
        ..McEncodeConfig::default()
    };
    // sb width = 750 Hz at 48 kHz. L/R in sb 0-1, C in sb 6, LS in
    // sb 10, RS in sb 13 — each lands in a different subband group.
    let tones = [430.0, 1_150.0, 4_900.0, 7_900.0, 10_150.0];
    let mut pcm: Vec<Vec<f64>> = tones
        .iter()
        .map(|&f| tone(f, sample_rate, total, 0.3))
        .collect();
    // Vary relative levels so the election is not degenerate.
    for (ch, gain) in [(0usize, 1.0f64), (1, 0.8), (2, 0.25), (3, 0.6), (4, 0.15)] {
        for s in &mut pcm[ch] {
            *s *= gain;
        }
    }
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert_eq!(decoded.channels.len(), 5);
    assert_separation(
        "adaptive tc",
        &decoded.channels,
        &tones,
        sample_rate,
        &[0, 1, 2, 3, 4],
    );
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.5, "adaptive tc ch {ch}: residual {ratio:.4}");
    }
}

#[test]
fn adaptive_tc_allocation_works_under_every_matrixing_procedure() {
    let sample_rate = 44_100;
    let n_frames = 3;
    let header = base_header(sample_rate, 384_000);
    for (front, surround) in [(3u8, 2u8), (3, 1), (3, 0), (2, 2), (2, 1)] {
        for proc_ in [0u8, 1, 2, 3] {
            if proc_ == 2 && !(front == 3 && surround >= 1) {
                continue;
            }
            let cfg = McEncodeConfig {
                front,
                surround,
                dematrix_procedure: proc_,
                adaptive_tc: true,
                ..McEncodeConfig::default()
            };
            let channels = cfg.presentation_channels();
            let pcm = tone_streams(channels, sample_rate, n_frames, 0.25);
            let stream = encode_mc_all_frames(&header, &cfg, &pcm, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} proc {proc_}: encode: {e}"));
            let decoded = decode_mc_stream(&stream, None)
                .unwrap_or_else(|e| panic!("{front}/{surround} proc {proc_}: decode: {e}"));
            for ch in 0..channels {
                let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
                assert!(
                    ratio < 0.5,
                    "{front}/{surround} proc {proc_} adaptive ch {ch}: residual {ratio:.4}"
                );
            }
        }
    }
}

// -------------------------------------------------------------------
// 3. Phantom-centre coding
// -------------------------------------------------------------------

#[test]
fn phantom_centre_band_limits_c_and_folds_the_high_band_into_l_r() {
    let sample_rate = 48_000;
    let n_frames = 4;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        phantom_centre: true,
        ..McEncodeConfig::default()
    };
    // Centre carries a low tone (sb 3, transmitted) AND a high tone
    // (sb 20, phantom-folded); L/R/LS/RS carry their usual tones.
    let c_low = 2_600.0; // sb 3
    let c_high = 15_400.0; // sb 20 ≥ 12 ⇒ centre_limited
    let mut pcm = tone_streams(5, sample_rate, n_frames, 0.25);
    pcm[2] = tone(c_low, sample_rate, total, 0.25);
    let high = tone(c_high, sample_rate, total, 0.25);
    for (s, h) in pcm[2].iter_mut().zip(&high) {
        *s += h;
    }
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert_eq!(decoded.mc_header.centre, Centre::Phantom);
    assert!(decoded.config.phantom_centre);

    // The centre's low band survives in C…
    let c_out = steady(&decoded.channels[2]);
    let c_low_p = goertzel_power(c_out, c_low, sample_rate);
    let c_high_p = goertzel_power(c_out, c_high, sample_rate);
    assert!(
        c_low_p > 100.0 * c_high_p.max(f64::MIN_POSITIVE),
        "centre band limit: low {c_low_p:.3e} vs high {c_high_p:.3e}"
    );
    // …and the high band reappears in L and R as C/√2 (−3 dB): the
    // phantom source. Reference power = the −3 dB copy's own power.
    let want: Vec<f64> = high.iter().map(|h| h / SQRT2).collect();
    let want_p = goertzel_power(steady(&want), c_high, sample_rate);
    for ch in [0usize, 1] {
        let p = goertzel_power(steady(&decoded.channels[ch]), c_high, sample_rate);
        let db = 10.0 * (p / want_p).log10();
        assert!(
            db.abs() < 1.5,
            "phantom fold ch {ch}: {db:+.2} dB from the −3 dB copy"
        );
    }
    // The other channels still separate on their own tones.
    assert_separation(
        "phantom",
        &decoded.channels,
        &CHANNEL_TONES_HZ,
        sample_rate,
        &[0, 1, 3, 4],
    );
}

#[test]
fn phantom_centre_survives_the_adaptive_tc_election() {
    // The §2.5.2.15 Phantom restriction (tc ∈ {0, 3, 4, 5} in 3/2) must
    // hold under the per-sbgr election too — decode proves it (a wrong
    // row would put the centre off T2 and break the band-limit rule).
    let sample_rate = 48_000;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        phantom_centre: true,
        adaptive_tc: true,
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, sample_rate, 3, 0.25);
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert_eq!(decoded.mc_header.centre, Centre::Phantom);
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.5, "phantom adaptive ch {ch}: residual {ratio:.4}");
    }
}

// -------------------------------------------------------------------
// 4. Second stereo programme
// -------------------------------------------------------------------

#[test]
fn second_stereo_programme_round_trips_unmatrixed() {
    let sample_rate = 48_000;
    let n_frames = 4;
    let header = base_header(sample_rate, 384_000);
    for front in [3u8, 2] {
        let cfg = McEncodeConfig {
            front,
            surround: 0,
            second_stereo: true,
            ..McEncodeConfig::default()
        };
        let channels = cfg.presentation_channels();
        assert_eq!(channels, usize::from(front) + 2);
        let pcm = tone_streams(channels, sample_rate, n_frames, 0.25);
        let stream = encode_mc_all_frames(&header, &cfg, &pcm, None)
            .unwrap_or_else(|e| panic!("{front}/0+2/0: encode: {e}"));
        assert!(has_mc_extension(&stream));
        let decoded = decode_mc_stream(&stream, None).expect("decode");
        assert_eq!(decoded.mc_header.surround, Surround::SecondStereo);
        assert!(decoded.config.second_stereo);
        assert_eq!(decoded.channels.len(), channels);
        assert_separation(
            &format!("{front}/0+2/0"),
            &decoded.channels,
            &CHANNEL_TONES_HZ,
            sample_rate,
            &(0..channels).collect::<Vec<_>>(),
        );
        for ch in 0..channels {
            let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
            assert!(ratio < 0.5, "{front}/0+2/0 ch {ch}: residual {ratio:.4}");
        }
    }
}

// -------------------------------------------------------------------
// 5. Multilingual channels
// -------------------------------------------------------------------

#[test]
fn multilingual_channels_round_trip_at_full_and_half_rate() {
    let sample_rate = 48_000;
    let n_frames = 6;
    let header = base_header(sample_rate, 384_000);
    for (fs_half, nml) in [(false, 2u8), (true, 3), (false, 1)] {
        let cfg = McEncodeConfig {
            front: 3,
            surround: 0,
            multilingual: nml,
            multilingual_fs_half: fs_half,
            ..McEncodeConfig::default()
        };
        let pcm = tone_streams(3, sample_rate, n_frames, 0.25);
        let ml_n = cfg.multilingual_samples_per_frame() * n_frames;
        let ml_rate = if fs_half {
            sample_rate / 2
        } else {
            sample_rate
        };
        let ml: Vec<Vec<f64>> = (0..usize::from(nml))
            .map(|i| tone(CHANNEL_TONES_HZ[4 + i], ml_rate, ml_n, 0.3))
            .collect();
        let stream = encode_mc_all_frames_ext(&header, &cfg, &pcm, None, &ml)
            .unwrap_or_else(|e| panic!("ml fs_half={fs_half} n={nml}: encode: {e}"));
        assert!(stream.ext.is_none());
        let decoded = decode_mc_stream(&stream.base, None).expect("decode");
        assert_eq!(decoded.mc_header.no_of_multi_lingual_ch, nml);
        assert_eq!(decoded.mc_header.multi_lingual_fs_half, fs_half);
        assert_eq!(decoded.multilingual.len(), usize::from(nml));
        let guard = cfg.multilingual_samples_per_frame();
        for (i, out) in decoded.multilingual.iter().enumerate() {
            assert_eq!(out.len(), ml_n, "ml {i} length");
            let ratio = residual_ratio_guarded(&ml[i], out, guard);
            assert!(
                ratio < 0.5,
                "ml fs_half={fs_half} ch {i}: residual {ratio:.4}"
            );
            // No leakage from the sibling commentary channel.
            let s = &out[FILTERBANK_DELAY + guard..out.len() - guard];
            let own = goertzel_power(s, CHANNEL_TONES_HZ[4 + i], ml_rate);
            for j in 0..usize::from(nml) {
                if j == i {
                    continue;
                }
                let leak = goertzel_power(s, CHANNEL_TONES_HZ[4 + j], ml_rate);
                assert!(
                    own > 20.0 * leak.max(f64::MIN_POSITIVE),
                    "ml {i}: own {own:.3e} vs leak {leak:.3e}"
                );
            }
        }
        // The main programme is unaffected.
        for ch in 0..3 {
            let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
            assert!(ratio < 0.5, "ml main ch {ch}: residual {ratio:.4}");
        }
    }
}

// -------------------------------------------------------------------
// 6. Extension bit stream
// -------------------------------------------------------------------

#[test]
fn extension_bit_stream_carries_the_spill_and_round_trips() {
    let sample_rate = 48_000;
    let n_frames = 4;
    // Low base bitrate + an explicit over-capacity budget: the frame is
    // 192 kbit/s (4608 bits) but the extension asks for 6000 bits.
    let header = base_header(sample_rate, 192_000);
    let cfg = McEncodeConfig {
        mc_bits: Some(6_000),
        ext_bit_stream: true,
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, sample_rate, n_frames, 0.25);
    let stream = encode_mc_all_frames_ext(&header, &cfg, &pcm, None, &[]).expect("encode");
    let ext = stream.ext.as_deref().expect("ext stream present");
    assert!(!ext.is_empty());
    // Every ext frame starts with the §2.5.2.10 syncword.
    assert_eq!(ext[0] & 0xFF, 0x7F);
    assert_eq!(ext[1] & 0xF0, 0xF0);

    // Without the extension frames the decode must fail…
    assert!(decode_mc_stream(&stream.base, None).is_err());
    // …and with them it round-trips.
    let decoded = decode_mc_stream(&stream.base, Some(ext)).expect("decode");
    assert!(decoded.mc_header.ext_bit_stream_present);
    assert_eq!(decoded.frames, n_frames);
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.6, "ext spill ch {ch}: residual {ratio:.4}");
    }
}

#[test]
fn extension_bit_stream_with_headroom_emits_header_only_ext_frames() {
    let sample_rate = 48_000;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        ext_bit_stream: true,
        ..McEncodeConfig::default()
    };
    let n_frames = 4;
    let pcm = tone_streams(5, sample_rate, n_frames, 0.25);
    let stream = encode_mc_all_frames_ext(&header, &cfg, &pcm, None, &[]).expect("encode");
    let ext = stream.ext.as_deref().expect("ext stream present");
    // Default budget fits the base frame → header-only (5-byte)
    // extension frames, one per base frame.
    assert_eq!(ext.len(), n_frames * 5);
    let decoded = decode_mc_stream(&stream.base, Some(ext)).expect("decode");
    assert_eq!(decoded.frames, n_frames);
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.5, "ext headroom ch {ch}: residual {ratio:.4}");
    }
}

#[test]
fn plain_entry_points_refuse_an_ext_bit_stream_config() {
    let header = base_header(48_000, 384_000);
    let cfg = McEncodeConfig {
        ext_bit_stream: true,
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, 48_000, 1, 0.2);
    assert!(matches!(
        encode_mc_all_frames(&header, &cfg, &pcm, None),
        Err(McEncodeError::BadConfig(_))
    ));
    let mut state = McEncodeState::new();
    assert!(matches!(
        oxideav_mp2::mc_encode::encode_mc_frame_with(&header, &cfg, &pcm, None, &mut state),
        Err(McEncodeError::BadConfig(_))
    ));
}

#[test]
fn over_capacity_budget_without_ext_stream_is_refused() {
    let header = base_header(48_000, 192_000);
    let cfg = McEncodeConfig {
        mc_bits: Some(6_000),
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, 48_000, 1, 0.2);
    assert!(matches!(
        encode_mc_all_frames(&header, &cfg, &pcm, None),
        Err(McEncodeError::BudgetTooSmall { .. })
    ));
}

// -------------------------------------------------------------------
// 7. Dynamic crosstalk election
// -------------------------------------------------------------------

#[test]
fn dynamic_crosstalk_fires_on_correlated_surround_and_round_trips() {
    let sample_rate = 48_000;
    let n_frames = 6;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let header = base_header(sample_rate, 256_000);
    let cfg = McEncodeConfig {
        dyn_cross: true,
        ..McEncodeConfig::default()
    };
    // LS and RS carry the *same* signal at slightly different levels —
    // the classic stereo-irrelevant case §C.2.1.7 describes. C is its
    // own tone.
    let mut pcm = tone_streams(5, sample_rate, n_frames, 0.25);
    let surround = tone(1_800.0, sample_rate, total, 0.25);
    pcm[3] = surround.clone();
    pcm[4] = surround.iter().map(|s| 0.8 * s).collect();
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert!(
        decoded.dyn_cross_frames > 0,
        "correlated surround did not trip dyn_cross_on"
    );
    // The programme still reconstructs (the substituted channels within
    // the 10 dB substitution bound, the rest as usual).
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.5, "dyn cross ch {ch}: residual {ratio:.4}");
    }
}

#[test]
fn dynamic_crosstalk_stays_off_for_band_disjoint_channels() {
    let sample_rate = 48_000;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        dyn_cross: true,
        ..McEncodeConfig::default()
    };
    // Independent full-band noise per channel: every subband group has
    // energy in every channel, and substituting any channel for
    // another fails the 10 dB bound — so no group may elect crosstalk.
    // (Band-limited material CAN legitimately fire it: a subband where
    // a channel is silent is substitutable for free.)
    let mut seed = 0x1234_5678_9abc_def0u64;
    let mut noise = |amp: f64, total: usize| -> Vec<f64> {
        (0..total)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                amp * ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5)
            })
            .collect()
    };
    let total = 4 * PCM_SAMPLES_PER_CHANNEL;
    let pcm: Vec<Vec<f64>> = (0..5).map(|_| noise(0.5, total)).collect();
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert_eq!(decoded.dyn_cross_frames, 0, "spurious dyn_cross_on");
}

#[test]
fn dynamic_crosstalk_covers_the_second_stereo_programme() {
    let sample_rate = 48_000;
    let n_frames = 4;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let header = base_header(sample_rate, 256_000);
    let cfg = McEncodeConfig {
        front: 2,
        surround: 0,
        second_stereo: true,
        dyn_cross: true,
        ..McEncodeConfig::default()
    };
    // L2 == R2 exactly: dyn_second_stereo substitutes R2 everywhere.
    let mut pcm = tone_streams(4, sample_rate, n_frames, 0.25);
    let programme2 = tone(2_600.0, sample_rate, total, 0.3);
    pcm[2] = programme2.clone();
    pcm[3] = programme2;
    let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
    let decoded = decode_mc_stream(&stream, None).expect("decode");
    assert!(decoded.dyn_cross_frames > 0, "identical L2/R2 did not trip");
    for ch in 0..4 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(
            ratio < 0.5,
            "second-stereo dyn ch {ch}: residual {ratio:.4}"
        );
    }
}

// -------------------------------------------------------------------
// Everything at once + per-frame ext API
// -------------------------------------------------------------------

#[test]
fn kitchen_sink_frame_api_round_trips() {
    // 3/2 + LFE + adaptive tc + dyn cross + prediction + multilingual
    // + extension bit stream, driven through the per-frame API.
    let sample_rate = 44_100;
    let n_frames = 3;
    let header = base_header(sample_rate, 384_000);
    let cfg = McEncodeConfig {
        lfe: true,
        adaptive_tc: true,
        dyn_cross: true,
        prediction: true,
        multilingual: 1,
        ext_bit_stream: true,
        mc_bits: Some(7_000),
        ..McEncodeConfig::default()
    };
    let pcm = tone_streams(5, sample_rate, n_frames, 0.22);
    let lfe: Vec<f64> = (0..n_frames * 12)
        .map(|i| 0.4 * (i as f64 * 0.7).sin())
        .collect();
    let ml = [tone(
        CHANNEL_TONES_HZ[5],
        sample_rate,
        n_frames * PCM_SAMPLES_PER_CHANNEL,
        0.3,
    )];
    let mut state = McEncodeState::new();
    let mut base = Vec::new();
    let mut ext = Vec::new();
    for f in 0..n_frames {
        let at = f * PCM_SAMPLES_PER_CHANNEL;
        let frame_pcm: Vec<Vec<f64>> = pcm
            .iter()
            .map(|ch| ch[at..at + PCM_SAMPLES_PER_CHANNEL].to_vec())
            .collect();
        let frame_ml: Vec<Vec<f64>> = ml
            .iter()
            .map(|ch| ch[at..at + PCM_SAMPLES_PER_CHANNEL].to_vec())
            .collect();
        let enc = encode_mc_frame_ext_with(
            &header,
            &cfg,
            &frame_pcm,
            Some(&lfe[f * 12..(f + 1) * 12]),
            &frame_ml,
            &mut state,
        )
        .expect("encode");
        base.extend_from_slice(&enc.base);
        ext.extend_from_slice(&enc.ext.expect("ext frame"));
    }
    let decoded = decode_mc_stream(&base, Some(&ext)).expect("decode");
    assert_eq!(decoded.frames, n_frames);
    assert!(decoded.lfe.is_some());
    assert_eq!(decoded.multilingual.len(), 1);
    for ch in 0..5 {
        let ratio = residual_ratio(&pcm[ch], &decoded.channels[ch]);
        assert!(ratio < 0.6, "kitchen sink ch {ch}: residual {ratio:.4}");
    }
    let ratio = residual_ratio(&ml[0], &decoded.multilingual[0]);
    assert!(ratio < 0.6, "kitchen sink ml: residual {ratio:.4}");
}

// -------------------------------------------------------------------
// Black-box base-layer acceptance (opaque reference binary)
// -------------------------------------------------------------------

/// Every §2.5 stream is an ordinary MPEG-1 Layer II stream to a
/// §2.5-unaware decoder. Feed the emitted base frames (with all the new
/// machinery active) through the installed reference decoder binary as
/// an opaque process and require a successful, full-length stereo
/// decode. Skips silently when the binary is not installed.
#[test]
fn black_box_reference_decoder_accepts_the_emitted_base_frames() {
    use std::process::Command;
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("skip: no reference decoder binary installed");
        return;
    }
    let sample_rate = 48_000;
    let n_frames = 6;
    let header = base_header(sample_rate, 384_000);
    for cfg in [
        McEncodeConfig {
            adaptive_tc: true,
            dyn_cross: true,
            prediction: true,
            ..McEncodeConfig::default()
        },
        McEncodeConfig {
            dematrix_procedure: 2,
            ..McEncodeConfig::default()
        },
        McEncodeConfig {
            phantom_centre: true,
            ..McEncodeConfig::default()
        },
    ] {
        let pcm = tone_streams(cfg.presentation_channels(), sample_rate, n_frames, 0.25);
        let stream = encode_mc_all_frames(&header, &cfg, &pcm, None).expect("encode");
        let dir = std::env::temp_dir();
        let stem = format!(
            "oxideav-mp2-r453-bb-{}-{}",
            std::process::id(),
            cfg.dematrix_procedure as usize * 2 + usize::from(cfg.phantom_centre)
        );
        let mp2 = dir.join(format!("{stem}.mp2"));
        let wav = dir.join(format!("{stem}.f64"));
        std::fs::write(&mp2, &stream).expect("write stream");
        let out = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&mp2)
            .args(["-f", "f64le", "-acodec", "pcm_f64le"])
            .arg(&wav)
            .output()
            .expect("run reference decoder");
        assert!(
            out.status.success(),
            "reference decoder rejected the base stream: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bytes = std::fs::read(&wav).expect("read decode");
        let samples = bytes.len() / 8 / 2; // stereo f64
        assert!(
            samples >= (n_frames - 1) * PCM_SAMPLES_PER_CHANNEL,
            "reference decode too short: {samples} samples"
        );
        // The reference decode must be the compatible downmix: compare
        // against this crate's own base decode of the same bytes.
        let own = oxideav_mp2::decode_all_frames(&stream).expect("own base decode");
        let mut ref_ch: Vec<Vec<f64>> = vec![Vec::new(), Vec::new()];
        for pair in bytes.chunks_exact(16) {
            ref_ch[0].push(f64::from_le_bytes(pair[..8].try_into().unwrap()));
            ref_ch[1].push(f64::from_le_bytes(pair[8..].try_into().unwrap()));
        }
        for ch in 0..2 {
            let n = own[ch].len().min(ref_ch[ch].len());
            let lo = PCM_SAMPLES_PER_CHANNEL;
            let (mut sig, mut err) = (0.0f64, 0.0f64);
            for i in lo..n - lo.min(n / 2) {
                sig += own[ch][i] * own[ch][i];
                let e = own[ch][i] - ref_ch[ch][i];
                err += e * e;
            }
            assert!(
                err <= 1e-4 * sig.max(1e-9),
                "reference vs own base decode diverge on ch {ch}: {err:.3e} vs {sig:.3e}"
            );
        }
        let _ = std::fs::remove_file(&mp2);
        let _ = std::fs::remove_file(&wav);
    }
}
