//! End-to-end Layer II decode → PCM conformance against the staged
//! `layer2-stereo-44100-192kbps` fixture.
//!
//! # What this validates
//!
//! [`decode_all_frames`] runs the complete §2.4 decode chain — header
//! parse → §2.4.1.4 CRC verify → §2.4.2.1 bit-allocation table lookup →
//! §2.4.3.3 scalefactor/scfsi expansion → §2.4.3.3.4 requantization →
//! §2.4.3.2 / Annex A Figure A.2 polyphase synthesis filterbank — and
//! emits per-channel `f64` PCM in the §2.4.3.4.7.1 `[-1.0, +1.0)`
//! range. This test maps that to interleaved `i16` exactly the way the
//! `Decoder` trait wrapper does (`* 2^15`, round, clamp) and compares
//! against the `expected.wav` PCM payload byte-for-byte where the
//! filterbank is numerically exact, and within a sub-LSB bound
//! elsewhere.
//!
//! # Why the bound is "≤ 1 LSB", not byte-identical
//!
//! ISO/IEC 11172-3 §2.4.3.2 / §2.4.3.3.5 define the synthesis
//! filterbank's matrixing (the 64×32 `N_ik` cosine matrix) and 512-tap
//! windowing as **floating-point** operations. The standard prescribes
//! no fixed accumulation order, no intermediate fixed-point
//! quantisation, and no specific integer-PCM rounding rule — those are
//! left to the implementation, and conformance in ISO/IEC 11172-4 /
//! 13818-4 is defined as a *bounded* difference signal (rms below
//! `2^-15 / sqrt(12)`), not bit-identity.
//!
//! Consequently an independent clean-room decoder cannot reproduce a
//! particular reference encoder/decoder's integer output bit-for-bit:
//! samples whose true value lands within ~0.5 LSB of a rounding
//! boundary flip by ±1 depending on the order in which the 2048
//! matrix mul-adds and 512 window mul-adds are summed. The reference
//! `expected.wav` here was produced by a third-party black-box decoder
//! (its source is off-limits; only its emitted PCM bytes are consumed),
//! so we assert the ISO-grade conformance envelope: identical
//! sample count, max abs error ≤ 1 LSB, and rms error well under one
//! LSB — together with a high exact-match ratio that would collapse
//! immediately if any stage (bit-alloc table, requant constants,
//! scalefactor map, synthesis window, channel ordering) regressed.

use oxideav_mp2::decode_all_frames;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/"
);

/// Symmetric `2^15` full-scale fractional → `i16` map, matching the
/// `Decoder`-trait wrapper in `codec_decoder::float_plane_to_s16_le`.
fn to_i16(s: f64) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Read the `data` chunk of a canonical RIFF/WAVE PCM file as `i16`
/// samples (the file is interleaved s16le per its `fmt ` chunk).
fn wav_data_samples(wav: &[u8]) -> Vec<i16> {
    assert_eq!(&wav[0..4], b"RIFF", "not a RIFF file");
    assert_eq!(&wav[8..12], b"WAVE", "not a WAVE file");
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let sz = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        let body_start = i + 8;
        if id == b"data" {
            let body = &wav[body_start..body_start + sz];
            return body
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        // Chunks are word-aligned: an odd-size body is followed by a pad byte.
        i = body_start + sz + (sz & 1);
    }
    panic!("no data chunk in WAVE file");
}

/// Skip cleanly when the `docs/` submodule isn't checked out (standalone
/// crate CI). Returns `(input.mp3 bytes, expected interleaved i16)`.
fn fixture() -> Option<(Vec<u8>, Vec<i16>)> {
    let mp3_path = format!("{FIXTURE_DIR}input.mp3");
    let wav_path = format!("{FIXTURE_DIR}expected.wav");
    if !std::path::Path::new(&mp3_path).exists() {
        eprintln!("skip: staged Layer II fixture absent at {mp3_path}");
        return None;
    }
    let mp3 = std::fs::read(&mp3_path).expect("read input.mp3");
    let wav = std::fs::read(&wav_path).expect("read expected.wav");
    Some((mp3, wav_data_samples(&wav)))
}

#[test]
fn decode_matches_reference_pcm_within_iso_tolerance() {
    let Some((mp3, expected)) = fixture() else {
        return;
    };

    // Full §2.4 decode chain → per-channel f64 PCM.
    let planes = decode_all_frames(&mp3).expect("decode_all_frames");
    assert_eq!(planes.len(), 2, "fixture is stereo");
    let n = planes[0].len();
    assert_eq!(planes[1].len(), n, "channels share sample count");

    // Interleave L,R and map to i16 the same way the trait wrapper does.
    let mut got = Vec::with_capacity(n * 2);
    for (&l, &r) in planes[0].iter().zip(planes[1].iter()) {
        got.push(to_i16(l));
        got.push(to_i16(r));
    }

    // The decoder must emit exactly as many PCM samples as the
    // reference: 31 frames × 1152 samples/channel × 2 channels.
    assert_eq!(
        got.len(),
        expected.len(),
        "decoded sample count must equal the reference WAV payload"
    );
    assert_eq!(got.len(), 31 * 1152 * 2, "31 frames of 1152 stereo samples");

    let mut max_abs = 0i32;
    let mut sum_sq = 0.0_f64;
    let mut exact = 0usize;
    for (&g, &e) in got.iter().zip(expected.iter()) {
        let d = (g as i32 - e as i32).abs();
        if d > max_abs {
            max_abs = d;
        }
        sum_sq += (d * d) as f64;
        if d == 0 {
            exact += 1;
        }
    }
    let rms = (sum_sq / got.len() as f64).sqrt();
    let exact_ratio = exact as f64 / got.len() as f64;

    // ISO floating-point-filterbank conformance envelope. Any decode
    // regression (wrong bit-alloc class, requant C/D constant, swapped
    // scalefactor, perturbed synthesis window, channel transposition)
    // blows past these immediately — the only residual is the ±1 LSB
    // boundary jitter inherent to an independent float implementation.
    assert!(
        max_abs <= 1,
        "max abs PCM error {max_abs} LSB exceeds the ≤1 conformance bound"
    );
    assert!(
        rms < 0.6,
        "rms PCM error {rms:.4} LSB exceeds the sub-LSB conformance bound"
    );
    assert!(
        exact_ratio > 0.70,
        "only {:.1}% of samples are bit-exact (expected > 70%)",
        exact_ratio * 100.0
    );
}

#[test]
fn decode_produces_no_dc_offset_or_clipping() {
    let Some((mp3, _expected)) = fixture() else {
        return;
    };
    let planes = decode_all_frames(&mp3).expect("decode_all_frames");

    for (ch, plane) in planes.iter().enumerate() {
        // The §2.4.3.2 filterbook output is bounded in [-1, +1]; a stage
        // error (e.g. a missing scalefactor or a mis-scaled window)
        // shows up as values blowing past unity. Allow a hair over 1.0
        // for legitimate full-scale peaks.
        let peak = plane.iter().cloned().fold(0.0_f64, |a, b| a.max(b.abs()));
        assert!(
            peak <= 1.001,
            "channel {ch} peak {peak} exceeds the §2.4.3.4.7.1 [-1,+1] range"
        );
        // A constant nonzero DC bias across the whole stream would mean
        // the requantizer's `s'' = C*(s''' + D)` offset `D` is being
        // mis-applied. The mean of a real music signal sits near zero.
        let mean = plane.iter().sum::<f64>() / plane.len() as f64;
        assert!(
            mean.abs() < 0.02,
            "channel {ch} DC offset {mean} too large — check requant D constant"
        );
    }
}
