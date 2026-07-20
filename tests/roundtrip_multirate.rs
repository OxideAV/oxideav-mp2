//! Round 350 — multi-rate encode → decode PCM round-trip property test
//! across the full Layer II sampling-rate matrix.
//!
//! This is the integration-level counterpart to the per-frame
//! round-trip unit tests in `src/encoder_frame.rs`: it drives the
//! complete public encode → decode pipeline
//! ([`encode_all_frames`] → [`decode_all_frames`]) over a continuous
//! multi-frame tone at **every** MPEG-1 Layer II sampling rate
//! (32 / 44,1 / 48 kHz; ISO/IEC 11172-3 §2.4.2.3 Table) **and** every
//! MPEG-2 LSF sampling rate (16 / 22,05 / 24 kHz; ISO/IEC 13818-3
//! §2.4.2.3 Table), and asserts spectral / energy properties of the
//! reconstruction.
//!
//! The §2.4.3.2 synthesis and §C.1.3 analysis filterbanks are defined
//! in floating point with no fixed accumulation order (ISO/IEC 11172-4
//! defines Layer II conformance as a *bounded* difference signal, not
//! a bit-exact one), so the assertions here are envelope properties,
//! not byte equalities:
//!
//! 1. **Shape:** the decoded stream has exactly `n_frames × 1152`
//!    samples per channel — the round-trip neither drops nor
//!    fabricates samples.
//! 2. **Reconstruction energy:** after the combined analysis+synthesis
//!    filterbank delay, the residual against the original tone holds a
//!    fraction of the signal energy — a broken allocation (all-zero /
//!    inverted SMR) would blow past this because the band carrying the
//!    tone would get too few quantiser steps and the output would be
//!    near-silent or noise-dominated.
//! 3. **Spectral localisation:** the reconstruction's energy at the
//!    input tone frequency (measured by a Goertzel single-bin
//!    estimator) dominates its energy at an unrelated probe frequency —
//!    the decoder reproduces the *right* tone, not broadband noise.
//! 4. **Silence:** an all-zero input round-trips to exact-zero PCM at
//!    every rate (no scalefactors / sample bits transmitted ⇒ the
//!    requantiser never runs).
//!
//! Clean-room basis: the rate ladders are read from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (§2.4.2.3) and
//! `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf` (§2.4.2.3 LSF
//! Table). No third-party MP2 implementation source was consulted.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames, FrameHeader, PaddingScheduler, PCM_SAMPLES_PER_CHANNEL,
};

/// Total byte length of an `n_frames` stream under the §2.4.2.3 padding
/// schedule the batch encoder drives (per-frame `N` / `N+1` slots at
/// the fractional 44,1 / 22,05 kHz rates; constant `N` elsewhere).
fn scheduled_stream_len(header: &FrameHeader, n_frames: usize) -> usize {
    let mut s = PaddingScheduler::new();
    (0..n_frames)
        .map(|_| s.next_header(header).frame_size_bytes())
        .sum()
}

/// (is_lsf, sample_rate_hz, total_bitrate_bps) — one entry per Layer II
/// sampling rate. The bitrates are picked comfortably above the
/// per-rate floor so the §C.1.5.2.7 allocator has room to localise a
/// loud tone (the per-cell allowed-bitrate matrix is enforced by
/// `FrameHeader`).
const RATE_MATRIX: &[(bool, u32, u32)] = &[
    // MPEG-1 (ISO/IEC 11172-3 §2.4.2.3).
    (false, 32_000, 128_000),
    (false, 44_100, 192_000),
    (false, 48_000, 192_000),
    // MPEG-2 LSF (ISO/IEC 13818-3 §2.4.2.3).
    (true, 16_000, 64_000),
    (true, 22_050, 64_000),
    (true, 24_000, 64_000),
];

/// Combined §C.1.3 analysis + §2.4.3.2 synthesis filterbank group
/// delay for Layer II, in samples. Established empirically by the
/// streaming round-trip unit test in `src/encoder_frame.rs`.
const FILTERBANK_DELAY: usize = 480;

fn stereo_header(lsf: bool, sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf,
        protection_bit: true, // true == "no CRC" per the §2.4.2.3 inverted convention
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

/// `n_frames` of a continuous per-channel sine at `freq_hz`, sampled at
/// `sample_rate`. Amplitude stays inside the §2.4.3.4.7.1 `[-1, +1]`
/// range.
fn tone_stream(
    channels: usize,
    freq_hz: f64,
    amp: f64,
    sample_rate: u32,
    n_frames: usize,
) -> Vec<Vec<f64>> {
    let omega = 2.0 * std::f64::consts::PI * freq_hz / sample_rate as f64;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    (0..channels)
        .map(|_| (0..total).map(|i| amp * (omega * i as f64).sin()).collect())
        .collect()
}

/// Goertzel single-bin power estimate of `signal` at `freq_hz`
/// (sampled at `sample_rate`). Returns the squared magnitude of the
/// DFT bin — a cheap energy-at-frequency probe with no FFT.
fn goertzel_power(signal: &[f64], freq_hz: f64, sample_rate: u32) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq_hz / sample_rate as f64;
    let coeff = 2.0 * w.cos();
    let mut s_prev = 0.0;
    let mut s_prev2 = 0.0;
    for &x in signal {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    // |X(k)|^2 = s_prev^2 + s_prev2^2 - coeff * s_prev * s_prev2.
    s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2
}

#[test]
fn encode_decode_round_trips_a_tone_at_every_layer2_rate() {
    let n_frames = 8;
    let amp = 0.5;
    let tone_hz = 1_000.0;
    // An unrelated probe frequency well away from the tone and from its
    // low harmonics, used to show the reconstruction is not broadband.
    let probe_hz = 7_000.0;

    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(2, tone_hz, amp, sample_rate, n_frames);

        let bytes = encode_all_frames(&header, &stream, 0)
            .unwrap_or_else(|e| panic!("encode at {sample_rate} Hz (lsf={lsf}): {e:?}"));
        // Each frame is exactly frame_size_bytes() long, with the
        // §2.4.2.3 padded frames one slot larger.
        assert_eq!(
            bytes.len(),
            scheduled_stream_len(&header, n_frames),
            "byte length at {sample_rate} Hz"
        );

        let planes = decode_all_frames(&bytes)
            .unwrap_or_else(|e| panic!("decode at {sample_rate} Hz (lsf={lsf}): {e:?}"));

        // Property 1 — shape: two channels, exact sample count.
        assert_eq!(planes.len(), 2, "stereo decode at {sample_rate} Hz");
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(
                plane.len(),
                total,
                "channel {ch} sample count at {sample_rate} Hz"
            );
        }

        let out = &planes[0];

        // Property 2 — reconstruction energy: residual against the
        // delayed original holds a fraction of the signal energy over
        // the steady middle of the stream.
        let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL; // skip ramp-in
        let hi = total - PCM_SAMPLES_PER_CHANNEL; // skip trailing partial
        assert!(hi > lo, "stream long enough to have a steady middle");
        let mut sig_energy = 0.0_f64;
        let mut err_energy = 0.0_f64;
        for i in lo..hi {
            let want = stream[0][i - FILTERBANK_DELAY];
            let got = out[i];
            sig_energy += want * want;
            let e = got - want;
            err_energy += e * e;
        }
        assert!(sig_energy > 0.0, "non-trivial signal at {sample_rate} Hz");
        // All six rates are perceptually shaped (13818-3's own Annex D
        // drives the LSF rates as of r419); the envelope keeps the
        // pre-r419 headroom since it exists to catch a *broken*
        // allocation, which blows far past 1.0. The tighter psy-driven
        // LSF floors are pinned by tests/lsf_psy_conformance.rs.
        let ratio = err_energy / sig_energy;
        assert!(
            ratio < 0.5,
            "reconstruction error/signal energy {ratio:.4} too high at {sample_rate} Hz (lsf={lsf})"
        );

        // Property 3 — spectral localisation: the steady middle's
        // energy at the tone frequency dominates its energy at the
        // unrelated probe frequency.
        let steady = &out[lo..hi];
        let tone_power = goertzel_power(steady, tone_hz, sample_rate);
        let probe_power = goertzel_power(steady, probe_hz, sample_rate);
        assert!(
            tone_power > 100.0 * probe_power.max(f64::MIN_POSITIVE),
            "tone power {tone_power:.3e} does not dominate probe power {probe_power:.3e} at {sample_rate} Hz (lsf={lsf})"
        );
    }
}

#[test]
fn silence_round_trips_to_exact_zero_at_every_layer2_rate() {
    let n_frames = 4;
    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let stream: Vec<Vec<f64>> = vec![vec![0.0; total]; 2];

        let bytes = encode_all_frames(&header, &stream, 0)
            .unwrap_or_else(|e| panic!("encode silence at {sample_rate} Hz: {e:?}"));
        let planes = decode_all_frames(&bytes)
            .unwrap_or_else(|e| panic!("decode silence at {sample_rate} Hz: {e:?}"));

        assert_eq!(planes.len(), 2);
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(plane.len(), total, "ch {ch} len at {sample_rate} Hz");
            for (i, &s) in plane.iter().enumerate() {
                assert_eq!(
                    s, 0.0,
                    "silence sample[{i}] on ch {ch} at {sample_rate} Hz must be exact zero, got {s}"
                );
            }
        }
    }
}

#[test]
fn round_trip_is_deterministic_across_runs_at_every_rate() {
    // The whole encode pipeline (§C.1.3 filterbank + §D.1 auto-SMR +
    // §C.1.5.2.7 allocator) is stateless beyond the per-stream X ring
    // buffer, so two independent batch encodes of the same input must
    // be byte-identical at every rate.
    let n_frames = 3;
    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(2, 1_234.0, 0.45, sample_rate, n_frames);
        let a = encode_all_frames(&header, &stream, 0).expect("encode a");
        let b = encode_all_frames(&header, &stream, 0).expect("encode b");
        assert_eq!(
            a, b,
            "batch encode must be deterministic at {sample_rate} Hz"
        );
    }
}

#[test]
fn mixed_bitrate_stream_decodes_frame_by_frame() {
    // §2.4.2.3 notes a Layer II decoder "is not required to support a
    // continuously variable bitrate" — support is permitted, not
    // forbidden, and each §2.4.1.3 frame header carries its own
    // bitrate_index. Our decoder sizes and allocates every frame from
    // its own header, so a stream whose frames switch between ladder
    // bitrates (here 192 → 256 kbit/s, which also switch between the
    // B.2 allocation sub-tables) must decode with the exact sample
    // count and per-frame envelope intact.
    let sample_rate = 48_000; // dif == 0: constant frame size per rate
    let n_frames_each = 3;
    let stream = tone_stream(2, 1_000.0, 0.4, sample_rate, n_frames_each);

    let h192 = stereo_header(false, sample_rate, 192_000);
    let h256 = stereo_header(false, sample_rate, 256_000);
    let part1 = encode_all_frames(&h192, &stream, 0).expect("encode 192k");
    let part2 = encode_all_frames(&h256, &stream, 0).expect("encode 256k");

    let mut mixed = part1;
    mixed.extend_from_slice(&part2);

    let planes = decode_all_frames(&mixed).expect("decode mixed-bitrate stream");
    assert_eq!(planes.len(), 2);
    for (ch, plane) in planes.iter().enumerate() {
        assert_eq!(
            plane.len(),
            2 * n_frames_each * PCM_SAMPLES_PER_CHANNEL,
            "ch {ch}: every frame of both bitrate segments decoded"
        );
    }

    // Frame walk: the emitted stream genuinely switches bitrate.
    let mut off = 0;
    let mut seen = Vec::new();
    while off < mixed.len() {
        let h = FrameHeader::parse(&mixed[off..]).expect("frame header");
        seen.push(h.bit_rate);
        off += h.frame_size_bytes();
    }
    assert_eq!(seen.len(), 2 * n_frames_each);
    assert!(seen.contains(&192_000) && seen.contains(&256_000));
}
