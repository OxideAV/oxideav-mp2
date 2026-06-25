//! Round 371 — end-to-end proof that the §D.1 Model-1 / §D.2 Model-2
//! psychoacoustic chains actually *shape* the §C.1.5.2.7 bit allocation,
//! and that they differ from a flat (rate-driven) 0 dB SMR table.
//!
//! The encoder offers three SMR sources for the same input:
//!
//! * a caller-supplied table (here a flat 0 dB table — the rate-driven
//!   baseline, `encode_all_frames_with_smr`),
//! * the §D.1 Model-1 auto-SMR chain (`encode_all_frames`),
//! * the §D.2 Model-2 auto-SMR chain (`encode_all_frames_model2`).
//!
//! For a perceptually-structured signal at a constrained bitrate the
//! allocator's `MNR = SNR − SMR` ordering is driven by the per-subband
//! SMR, so a real psychoacoustic model **must** redistribute the
//! limited bit budget differently from a flat table. This test pins
//! three properties:
//!
//! 1. **The perceptual encodes differ from the flat encode.** A
//!    byte-identical result would mean the SMR table never reached the
//!    allocator's tie-breaks — i.e. the psychoacoustic chain is inert.
//! 2. **Model-1 and Model-2 are both non-trivial and decodable**, each
//!    reproducing the dominant tone (the allocator never starves the
//!    band that carries the signal).
//! 3. **The model output is non-flat**: at least one subband's
//!    allocation differs between the flat and perceptual encodes — the
//!    masking model raised or lowered the bit demand somewhere.
//!
//! Conformance basis: floating-point filterbanks ⇒ envelope assertions,
//! not byte equalities, for the reconstruction (ISO/IEC 11172-4). The
//! allocation-shape assertions ARE exact (the audio-data allocation
//! field is integer per §2.4.1.6).
//!
//! Clean-room basis: the SMR → allocation contract is §C.1.5.2.7 and the
//! two models are Annex D of the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`. No third-party MP2
//! implementation source was consulted.

use oxideav_mp2::audio_data::parse_audio_data_with_section_bits;
use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames, encode_all_frames_model2, encode_all_frames_with_smr,
    FrameHeader, SmrTable, NUM_SUBBANDS, PCM_SAMPLES_PER_CHANNEL,
};

use oxideav_core::bits::BitReader;

const FILTERBANK_DELAY: usize = 480;

fn stereo_header(sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf: false,
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

/// A perceptually-structured two-tone-plus-noise signal: a loud low
/// tone (a strong masker) plus a quieter mid tone plus a touch of
/// broadband noise, so the masking model has real structure to act on.
fn structured_stream(sample_rate: u32, n_frames: usize) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let w1 = 2.0 * std::f64::consts::PI * 600.0 / f64::from(sample_rate);
    let w2 = 2.0 * std::f64::consts::PI * 3_000.0 / f64::from(sample_rate);
    // Deterministic LCG noise so the test is reproducible.
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

/// Read the per-(ch, sb) allocation `nb_steps` of the first frame.
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

fn assert_tone_dominates(
    plane: &[f64],
    n_frames: usize,
    tone_hz: f64,
    sample_rate: u32,
    label: &str,
) {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = total - PCM_SAMPLES_PER_CHANNEL;
    let steady = &plane[lo..hi];
    let tone = goertzel(steady, tone_hz, sample_rate);
    let probe = goertzel(steady, tone_hz + 5_000.0, sample_rate);
    assert!(
        tone > 50.0 * probe.max(f64::MIN_POSITIVE),
        "{label}: dominant tone {tone:.3e} must dominate probe {probe:.3e}"
    );
}

#[test]
fn perceptual_models_shape_allocation_differently_from_flat_smr() {
    let n_frames = 8;
    // A constrained bitrate so the allocator must make real trade-offs:
    // 128 kbit/s stereo = 64 kbit/s/ch, comfortably above the floor but
    // far from transparent, so the SMR ordering genuinely matters.
    let sample_rate = 44_100;
    let bit_rate = 128_000;
    let header = stereo_header(sample_rate, bit_rate);
    let stream = structured_stream(sample_rate, n_frames);

    // Flat 0 dB SMR baseline.
    let flat: SmrTable = [[0.0; NUM_SUBBANDS]; 2];
    let flat_bytes = encode_all_frames_with_smr(&header, &stream, &flat, 0).expect("flat encode");
    let m1_bytes = encode_all_frames(&header, &stream, 0).expect("model1 encode");
    let m2_bytes = encode_all_frames_model2(&header, &stream, 0).expect("model2 encode");

    // All three are the same length (same header / frame size).
    assert_eq!(flat_bytes.len(), m1_bytes.len(), "frame sizes match");
    assert_eq!(flat_bytes.len(), m2_bytes.len(), "frame sizes match");

    // Property 1: the perceptual encodes differ from the flat baseline.
    // If the SMR table never influenced allocation, the encodes would be
    // byte-identical.
    assert_ne!(
        flat_bytes, m1_bytes,
        "Model-1 auto-SMR must produce a different stream than flat 0 dB SMR"
    );
    assert_ne!(
        flat_bytes, m2_bytes,
        "Model-2 auto-SMR must produce a different stream than flat 0 dB SMR"
    );

    // Property 3: the first-frame allocation differs in at least one
    // subband between flat and each model (the masking model raised or
    // lowered the bit demand somewhere).
    let flat_alloc = first_frame_allocation(&header, &flat_bytes);
    let m1_alloc = first_frame_allocation(&header, &m1_bytes);
    let m2_alloc = first_frame_allocation(&header, &m2_bytes);
    assert_ne!(
        flat_alloc, m1_alloc,
        "Model-1 must redistribute allocation vs flat in the first frame"
    );
    assert_ne!(
        flat_alloc, m2_alloc,
        "Model-2 must redistribute allocation vs flat in the first frame"
    );

    // Property 2: both perceptual encodes still decode and reproduce the
    // dominant 600 Hz masker tone (the allocator never starves the
    // band carrying the signal).
    for (label, bytes) in [("model1", &m1_bytes), ("model2", &m2_bytes)] {
        let planes = decode_all_frames(bytes).unwrap_or_else(|e| panic!("{label} decode: {e:?}"));
        assert_eq!(planes.len(), 2);
        assert_tone_dominates(&planes[0], n_frames, 600.0, sample_rate, label);
    }
}

#[test]
fn model1_and_model2_are_not_identical_for_structured_input() {
    // The two Annex D models use different analysis front-ends (Model 1:
    // tonal/non-tonal masker labelling; Model 2: unpredictability-driven
    // twice-per-frame thresholds), so for a structured signal at a
    // constrained rate they should generally reach different
    // allocations. This guards against an accidental wiring that routes
    // both auto paths through the same model.
    let n_frames = 6;
    let sample_rate = 48_000;
    let header = stereo_header(sample_rate, 128_000);
    let stream = structured_stream(sample_rate, n_frames);

    let m1 = encode_all_frames(&header, &stream, 0).expect("model1");
    let m2 = encode_all_frames_model2(&header, &stream, 0).expect("model2");

    assert_ne!(
        m1, m2,
        "Model-1 and Model-2 auto-SMR must not produce byte-identical \
         streams for a structured signal (would mean both paths use one model)"
    );
}
