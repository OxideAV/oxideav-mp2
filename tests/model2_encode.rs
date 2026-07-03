//! Round 371 — §D.2 Model-2 auto-SMR encode integration test.
//!
//! The §D.1 Model-1 auto-SMR encode path
//! ([`encode_all_frames`]) has long-standing round-trip coverage in
//! `tests/roundtrip_multirate.rs`. This file is the analogous
//! integration test for the newly-wired §D.2 **Model-2** auto-SMR
//! encode path ([`encode_all_frames_model2`] /
//! [`encode_frame_auto_model2`]).
//!
//! ISO/IEC 11172-3:1993 Annex D presents *two* informative example
//! encoder psychoacoustic models. Model 1 (clause D.1) is the simpler
//! tonal/non-tonal masker model; Model 2 (clause D.2) is the
//! unpredictability-driven model that, for Layer II, runs its
//! threshold generator **twice per 1152-sample coder frame** and keeps
//! the more stringent of each pair of signal-to-mask ratios (§D.2.1).
//! Both deliver a per-(channel, sub-band) SMR table to the §C.1.5.2.7
//! iterative bit-allocator; the encoder must be able to drive the
//! allocator from either.
//!
//! Because the §2.4.3.2 synthesis / §C.1.3 analysis filterbanks are
//! floating point with no fixed accumulation order (ISO/IEC 11172-4
//! defines Layer II conformance as a *bounded* difference signal, not a
//! bit-exact one), the assertions here are envelope properties, not
//! byte equalities, exactly as in `roundtrip_multirate.rs`:
//!
//! 1. **Shape** — `n_frames × 1152` samples per channel, no drop / no
//!    fabrication.
//! 2. **Reconstruction energy** — the residual against the delayed
//!    original holds a bounded fraction of the signal energy; a broken
//!    Model-2 SMR (e.g. an inverted or all-`-inf` table) would starve
//!    the tone-carrying band and blow past this.
//! 3. **Spectral localisation** — the reconstruction's energy at the
//!    input tone frequency dominates an unrelated probe frequency.
//! 4. **Silence** — all-zero input round-trips to exact-zero PCM (no
//!    scalefactors / sample bits transmitted), and the Model-2 state's
//!    rolling predictor never spuriously allocates against silence.
//! 5. **Statefulness** — driving the same input through one persistent
//!    [`EncodeFrameState`] is deterministic across runs, and the
//!    Model-2 batch path is byte-identical to a hand-rolled
//!    [`encode_frame_auto_model2`] loop sharing one state.
//!
//! Clean-room basis: the rate ladders are read from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (§2.4.2.3) and the
//! §D.2 Model-2 procedure from the same PDF Annex D; no third-party MP2
//! implementation source was consulted.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames_model2, encode_frame_auto_model2, EncodeFrameState,
    FrameHeader, PaddingScheduler, PCM_SAMPLES_PER_CHANNEL,
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

/// (is_lsf, sample_rate_hz, total_bitrate_bps). Model-2 perceptual
/// curves are tabulated only for the three MPEG-1 rates; the LSF rates
/// degenerate to a flat 0 dB table (same as Model-1) and are included
/// to prove the path stays usable at every rate.
const RATE_MATRIX: &[(bool, u32, u32)] = &[
    (false, 32_000, 128_000),
    (false, 44_100, 192_000),
    (false, 48_000, 192_000),
    (true, 16_000, 64_000),
    (true, 22_050, 64_000),
    (true, 24_000, 64_000),
];

/// Combined §C.1.3 analysis + §2.4.3.2 synthesis filterbank group
/// delay for Layer II, in samples (same constant the Model-1 round-trip
/// test established empirically).
const FILTERBANK_DELAY: usize = 480;

fn stereo_header(lsf: bool, sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf,
        protection_bit: true, // true == "no CRC" (inverted §2.4.2.3 convention)
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

/// Goertzel single-bin power estimate at `freq_hz`.
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
    s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2
}

#[test]
fn model2_encode_decode_round_trips_a_tone_at_every_layer2_rate() {
    let n_frames = 8;
    let amp = 0.5;
    let tone_hz = 1_000.0;
    let probe_hz = 7_000.0;

    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(2, tone_hz, amp, sample_rate, n_frames);

        let bytes = encode_all_frames_model2(&header, &stream, 0)
            .unwrap_or_else(|e| panic!("model2 encode at {sample_rate} Hz (lsf={lsf}): {e:?}"));
        assert_eq!(
            bytes.len(),
            scheduled_stream_len(&header, n_frames),
            "byte length at {sample_rate} Hz"
        );

        let planes = decode_all_frames(&bytes)
            .unwrap_or_else(|e| panic!("decode at {sample_rate} Hz (lsf={lsf}): {e:?}"));

        // Property 1 — shape.
        assert_eq!(planes.len(), 2, "stereo decode at {sample_rate} Hz");
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(plane.len(), total, "ch {ch} count at {sample_rate} Hz");
        }

        let out = &planes[0];

        // Property 2 — reconstruction energy.
        let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
        let hi = total - PCM_SAMPLES_PER_CHANNEL;
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
        let ratio = err_energy / sig_energy;
        assert!(
            ratio < 0.5,
            "model2 reconstruction error/signal {ratio:.4} too high at {sample_rate} Hz (lsf={lsf})"
        );

        // Property 3 — spectral localisation.
        let steady = &out[lo..hi];
        let tone_power = goertzel_power(steady, tone_hz, sample_rate);
        let probe_power = goertzel_power(steady, probe_hz, sample_rate);
        assert!(
            tone_power > 100.0 * probe_power.max(f64::MIN_POSITIVE),
            "model2 tone power {tone_power:.3e} does not dominate probe {probe_power:.3e} at {sample_rate} Hz (lsf={lsf})"
        );
    }
}

#[test]
fn model2_silence_round_trips_to_exact_zero_at_every_layer2_rate() {
    let n_frames = 4;
    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let stream: Vec<Vec<f64>> = vec![vec![0.0; total]; 2];

        let bytes = encode_all_frames_model2(&header, &stream, 0)
            .unwrap_or_else(|e| panic!("model2 encode silence at {sample_rate} Hz: {e:?}"));
        let planes = decode_all_frames(&bytes)
            .unwrap_or_else(|e| panic!("decode silence at {sample_rate} Hz: {e:?}"));

        assert_eq!(planes.len(), 2);
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(plane.len(), total, "ch {ch} len at {sample_rate} Hz");
            for (i, &s) in plane.iter().enumerate() {
                assert_eq!(
                    s, 0.0,
                    "model2 silence sample[{i}] ch {ch} at {sample_rate} Hz must be exact zero, got {s}"
                );
            }
        }
    }
}

#[test]
fn model2_batch_matches_hand_rolled_stateful_loop() {
    // The §D.2 Model-2 threshold generator is stateful (rolling
    // two-block predictor + 448-sample inter-call carry), so the batch
    // entry point must thread one persistent EncodeFrameState through
    // every frame. A hand-rolled loop sharing one state must therefore
    // be byte-identical to the batch call.
    let n_frames = 5;
    for &(lsf, sample_rate, bit_rate) in RATE_MATRIX {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(2, 1_234.0, 0.45, sample_rate, n_frames);

        let batch = encode_all_frames_model2(&header, &stream, 0).expect("batch model2");

        // Hand-rolled: one shared state, frame by frame, with the same
        // §2.4.2.3 padding schedule the batch path drives.
        let mut state = EncodeFrameState::new();
        let mut padding = PaddingScheduler::new();
        let mut manual = Vec::new();
        for f in 0..n_frames {
            let frame_header = padding.next_header(&header);
            let base = f * PCM_SAMPLES_PER_CHANNEL;
            let frame_pcm: Vec<Vec<f64>> = stream
                .iter()
                .map(|plane| plane[base..base + PCM_SAMPLES_PER_CHANNEL].to_vec())
                .collect();
            let bytes = encode_frame_auto_model2(&frame_header, &frame_pcm, 0, &mut state)
                .expect("manual model2 frame");
            manual.extend_from_slice(&bytes);
        }

        assert_eq!(
            batch, manual,
            "model2 batch must equal hand-rolled stateful loop at {sample_rate} Hz (lsf={lsf})"
        );
    }
}

#[test]
fn model2_predictor_state_makes_later_frames_diverge_from_a_fresh_state() {
    // The §D.2.1 twice-per-frame predictor carries spectral history
    // across frames. Encoding frame N from a freshly-zeroed state must
    // (for a non-trivial signal at a modelled MPEG-1 rate) differ from
    // encoding it as the continuation of a stream — otherwise the
    // rolling predictor history is being dropped, which would mean the
    // state threading is a no-op.
    let header = stereo_header(false, 44_100, 192_000);
    let n_frames = 4;
    // A frequency-swept signal so consecutive frames have genuinely
    // different spectra (a pure tone has near-identical blocks, which
    // would make the predictor's contribution vanish).
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let sweep: Vec<f64> = (0..total)
        .map(|i| {
            let t = i as f64 / 44_100.0;
            // 500 Hz → 4000 Hz linear chirp.
            let f = 500.0 + 3_500.0 * (i as f64 / total as f64);
            0.5 * (2.0 * std::f64::consts::PI * f * t).sin()
        })
        .collect();
    let stream = vec![sweep.clone(), sweep];

    // Continuation: encode all frames through one state, keep frame 3.
    // Frame sizes vary under the §2.4.2.3 44,1 kHz padding schedule, so
    // locate the last frame by walking the schedule.
    let continued = encode_all_frames_model2(&header, &stream, 0).expect("continued");
    let last_start = scheduled_stream_len(&header, n_frames - 1);
    let last_end = scheduled_stream_len(&header, n_frames);
    let last_continued = &continued[last_start..last_end];

    // Fresh: encode ONLY the last frame's PCM from a zeroed state —
    // with the SAME per-frame padded header the schedule gave frame 3,
    // so the byte-divergence below can only come from the predictor
    // history, never from a frame-size mismatch.
    let mut sched = PaddingScheduler::new();
    let mut frame3_header = header;
    for _ in 0..n_frames {
        frame3_header = sched.next_header(&header);
    }
    assert_eq!(frame3_header.frame_size_bytes(), last_end - last_start);
    let base = (n_frames - 1) * PCM_SAMPLES_PER_CHANNEL;
    let last_pcm: Vec<Vec<f64>> = stream
        .iter()
        .map(|plane| plane[base..base + PCM_SAMPLES_PER_CHANNEL].to_vec())
        .collect();
    let mut fresh_state = EncodeFrameState::new();
    let last_fresh =
        encode_frame_auto_model2(&frame3_header, &last_pcm, 0, &mut fresh_state).expect("fresh");

    assert_ne!(
        last_continued,
        &last_fresh[..],
        "Model-2 rolling predictor history must influence later frames; \
         a fresh-state encode of the same frame should differ from the \
         streamed continuation"
    );
}
