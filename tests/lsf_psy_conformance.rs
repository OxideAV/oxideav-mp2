//! Round 419 — the MPEG-2 LSF psychoacoustic axis, end-to-end.
//!
//! ISO/IEC 13818-3 carries its own Annex D ("Psychoacoustic model 1/2
//! for Lower Sampling Frequencies"); this crate now transcribes those
//! tables (`tables_lsf`, `tables_model2`'s LSF section) and drives
//! both auto-SMR encode paths with them at 16 / 22,05 / 24 kHz. This
//! suite is the LSF counterpart of `psy_model_shapes_allocation.rs`
//! plus a measured round-trip SNR conformance floor:
//!
//! 1. **Allocation shaping** — at every LSF rate, the Model-1 and
//!    Model-2 encodes differ from the flat-0 dB (rate-driven) encode
//!    both byte-wise and in the first-frame per-subband allocation,
//!    and the two models differ from each other; every stream still
//!    decodes with the dominant masker tone intact.
//! 2. **Round-trip SNR floors** — the full public pipeline
//!    (`encode_all_frames*` → `decode_all_frames`) is measured as a
//!    delay-aligned time-domain SNR over the steady middle of the
//!    stream, per rate × {flat, Model 1, Model 2}. The asserted
//!    floors are set ~3 dB under the measured values (recorded in the
//!    per-case comments) so a regression that materially degrades the
//!    LSF chain trips the test while floating-point jitter does not.
//! 3. **The psychoacoustic encodes beat the rate-driven baseline** —
//!    at every LSF rate both models' measured SNR exceeds the flat
//!    encode's by ≥ 2,7 dB (asserted with a 1 dB-margin floor): the
//!    13818-3 masking tables steer bits toward the audible structure
//!    even by the crude waveform-SNR metric.
//!
//! Conformance basis: ISO/IEC 11172-4 bounded-difference conformance
//! (floating-point filterbanks ⇒ envelope assertions, not byte
//! equalities); the allocation-shape assertions are exact (§2.4.1.6
//! integer fields). Clean-room basis: the staged
//! `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf` Annex D and
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` §C.1.5.2.7 / Annex D.
//! No third-party MP2 implementation source was consulted.

use oxideav_core::bits::BitReader;
use oxideav_mp2::audio_data::parse_audio_data_with_section_bits;
use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames, encode_all_frames_model2, encode_all_frames_with_smr,
    FrameHeader, SmrTable, NUM_SUBBANDS, PCM_SAMPLES_PER_CHANNEL,
};

/// Combined analysis+synthesis filterbank group delay (samples).
const FILTERBANK_DELAY: usize = 480;

fn lsf_stereo_header(sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf: true,
        protection_bit: true,
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

/// A perceptually-structured two-tone-plus-noise stereo signal (same
/// construction as the MPEG-1 shaping suite): a loud 600 Hz masker, a
/// quiet 3 kHz tone (below every LSF Nyquist) and a whisper of
/// deterministic noise.
fn structured_stream(sample_rate: u32, n_frames: usize) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let w1 = 2.0 * std::f64::consts::PI * 600.0 / f64::from(sample_rate);
    let w2 = 2.0 * std::f64::consts::PI * 3_000.0 / f64::from(sample_rate);
    let mut rng: u64 = 0x1234_5678_9abc_def0;
    let mut next = || {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((rng >> 33) as f64 / f64::from(u32::MAX)) * 2.0 - 1.0
    };
    let plane: Vec<f64> = (0..total)
        .map(|i| {
            let t = i as f64;
            0.55 * (w1 * t).sin() + 0.12 * (w2 * t).sin() + 0.02 * next()
        })
        .collect();
    vec![plane.clone(), plane]
}

/// Delay-aligned time-domain SNR (dB) of channel 0 over the steady
/// middle of the stream.
fn round_trip_snr_db(stream: &[Vec<f64>], decoded: &[Vec<f64>], n_frames: usize) -> f64 {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = total - PCM_SAMPLES_PER_CHANNEL;
    assert!(hi > lo);
    let mut sig = 0.0_f64;
    let mut err = 0.0_f64;
    for i in lo..hi {
        let want = stream[0][i - FILTERBANK_DELAY];
        let got = decoded[0][i];
        sig += want * want;
        let e = got - want;
        err += e * e;
    }
    assert!(sig > 0.0);
    10.0 * (sig / err.max(f64::MIN_POSITIVE)).log10()
}

fn first_frame_allocation(header: &FrameHeader, bytes: &[u8]) -> [[u32; NUM_SUBBANDS]; 2] {
    let mut reader = BitReader::with_position(bytes, 4);
    let (audio, _, _) =
        parse_audio_data_with_section_bits(header, &mut reader).expect("parse audio-data");
    let mut out = [[0u32; NUM_SUBBANDS]; 2];
    let sblimit = audio.sblimit.min(NUM_SUBBANDS);
    for (ch, row) in out.iter_mut().enumerate() {
        row[..sblimit].copy_from_slice(&audio.nb_steps[ch][..sblimit]);
    }
    out
}

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

#[test]
fn lsf_models_shape_allocation_differently_from_flat_smr() {
    // 64 kbit/s stereo = 32 kbit/s/ch at the LSF rates — constrained
    // enough that the SMR ordering genuinely matters.
    let n_frames = 8;
    for sample_rate in [16_000u32, 22_050, 24_000] {
        let header = lsf_stereo_header(sample_rate, 64_000);
        let stream = structured_stream(sample_rate, n_frames);

        let flat_table: SmrTable = [[0.0; NUM_SUBBANDS]; 2];
        let flat =
            encode_all_frames_with_smr(&header, &stream, &flat_table, 0).expect("flat encode");
        let m1 = encode_all_frames(&header, &stream, 0).expect("model1 encode");
        let m2 = encode_all_frames_model2(&header, &stream, 0).expect("model2 encode");

        assert_eq!(flat.len(), m1.len(), "{sample_rate} Hz: frame sizes match");
        assert_eq!(flat.len(), m2.len(), "{sample_rate} Hz: frame sizes match");

        // The perceptual encodes differ from the rate-driven baseline
        // and from each other.
        assert_ne!(flat, m1, "{sample_rate} Hz: Model 1 inert vs flat");
        assert_ne!(flat, m2, "{sample_rate} Hz: Model 2 inert vs flat");
        assert_ne!(m1, m2, "{sample_rate} Hz: the two models must differ");

        // The first-frame per-subband allocation moved.
        let flat_alloc = first_frame_allocation(&header, &flat);
        assert_ne!(
            flat_alloc,
            first_frame_allocation(&header, &m1),
            "{sample_rate} Hz: Model 1 must redistribute the allocation"
        );
        assert_ne!(
            flat_alloc,
            first_frame_allocation(&header, &m2),
            "{sample_rate} Hz: Model 2 must redistribute the allocation"
        );

        // Every stream still decodes with the dominant masker intact.
        for (label, bytes) in [("model1", &m1), ("model2", &m2)] {
            let planes =
                decode_all_frames(bytes).unwrap_or_else(|e| panic!("{label} decode: {e:?}"));
            assert_eq!(planes.len(), 2);
            let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
            let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
            let hi = total - PCM_SAMPLES_PER_CHANNEL;
            let steady = &planes[0][lo..hi];
            let tone = goertzel(steady, 600.0, sample_rate);
            let probe = goertzel(steady, 5_600.0, sample_rate);
            assert!(
                tone > 50.0 * probe.max(f64::MIN_POSITIVE),
                "{label} at {sample_rate} Hz: tone {tone:.3e} vs probe {probe:.3e}"
            );
        }
    }
}

#[test]
fn lsf_round_trip_snr_clears_conformance_floors() {
    let n_frames = 8;
    // Both models must beat the flat baseline by at least this margin
    // (measured gains: +3.36 / +2.96 / +2.73 dB at 16 / 22,05 /
    // 24 kHz for Model 1; Model 2 within 0,1 dB of Model 1).
    const MIN_GAIN_OVER_FLAT_DB: f64 = 1.0;

    // Per-rate floors ≈ 3 dB under the measured values (2026-07-20,
    // this crate's f64 chain):
    //   16 kHz:    flat 6.07 dB, model1  9.43 dB, model2  9.42 dB
    //   22,05 kHz: flat 8.92 dB, model1 11.88 dB, model2 11.95 dB
    //   24 kHz:    flat 10.17 dB, model1 12.90 dB, model2 12.97 dB
    for (sample_rate, flat_floor_db, model_floor_db) in [
        (16_000u32, 3.0, 6.4),
        (22_050, 5.9, 8.9),
        (24_000, 7.1, 9.9),
    ] {
        let header = lsf_stereo_header(sample_rate, 64_000);
        let stream = structured_stream(sample_rate, n_frames);
        let flat_table: SmrTable = [[0.0; NUM_SUBBANDS]; 2];

        let snr = |bytes: &[u8]| {
            let planes = decode_all_frames(bytes).expect("decode");
            round_trip_snr_db(&stream, &planes, n_frames)
        };

        let flat_snr = snr(&encode_all_frames_with_smr(&header, &stream, &flat_table, 0).unwrap());
        let m1_snr = snr(&encode_all_frames(&header, &stream, 0).unwrap());
        let m2_snr = snr(&encode_all_frames_model2(&header, &stream, 0).unwrap());
        println!(
            "LSF {sample_rate} Hz 64 kbit/s stereo SNR: flat {flat_snr:.2} dB, \
             model1 {m1_snr:.2} dB, model2 {m2_snr:.2} dB"
        );

        assert!(
            flat_snr > flat_floor_db,
            "flat at {sample_rate} Hz: SNR {flat_snr:.2} dB under the {flat_floor_db} dB floor"
        );
        for (label, v) in [("model1", m1_snr), ("model2", m2_snr)] {
            assert!(
                v > model_floor_db,
                "{label} at {sample_rate} Hz: SNR {v:.2} dB under the {model_floor_db} dB floor"
            );
            assert!(
                v > flat_snr + MIN_GAIN_OVER_FLAT_DB,
                "{label} at {sample_rate} Hz: SNR {v:.2} dB does not beat the flat \
                 baseline {flat_snr:.2} dB by {MIN_GAIN_OVER_FLAT_DB} dB — the 13818-3 \
                 masking tables are not steering the allocation"
            );
        }
    }
}
