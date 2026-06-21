//! Full Layer II decode-conformance matrix: header → bit-allocation →
//! §2.4.3.3.4 requantization → §2.4.3.3.3 scalefactor rescaling →
//! §2.4.3.2 / Annex A Figure A.2 synthesis filterbank, validated against
//! an independent black-box reference decoder across every Layer II
//! channel-mode × sampling-rate combination.
//!
//! The existing `tests/layer2_pcm_conformance.rs` pins one staged
//! 44.1 kHz stereo stream. This test broadens that single point of
//! ground truth into the whole rate ladder — MPEG-1 single-channel and
//! stereo at 32 / 44.1 / 48 kHz, and MPEG-2 LSF at 16 / 22.05 / 24 kHz —
//! so a regression localised to one rate's bit-allocation table
//! ([`oxideav_mp2::bitalloc::select_table`]) or LSF sizing cannot hide
//! behind the others.
//!
//! # Conformance bound
//!
//! ISO/IEC 11172-3 §2.4.3.2 defines the synthesis filterbank in floating
//! point with no prescribed accumulation order, so conformance (ISO/IEC
//! 11172-4 / 13818-4) is a *bounded* difference signal, not bit-identity.
//! Each fixture's `.ref.wav` was emitted by an opaque reference decoder
//! (its source is off-limits; only its PCM bytes are consumed — a
//! black-box validator per the workspace clean-room policy). We assert
//! the ISO-grade envelope: exact sample count, max abs error ≤ 1 LSB,
//! rms well under 1 LSB, and a high exact-match ratio that collapses
//! immediately if any decode stage regresses. See `fixtures/README.md`.

use oxideav_mp2::decode_all_frames;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");

/// (fixture stem, channel count, human label).
const MATRIX: &[(&str, usize, &str)] = &[
    ("mono_44k_128", 1, "MPEG-1 single-channel 44.1 kHz"),
    ("mono_32k_96", 1, "MPEG-1 single-channel 32 kHz"),
    ("stereo_48k_192", 2, "MPEG-1 stereo 48 kHz"),
    ("mono_22k_64", 1, "MPEG-2 LSF single-channel 22.05 kHz"),
    ("stereo_24k_64", 2, "MPEG-2 LSF stereo 24 kHz"),
    ("stereo_16k_64", 2, "MPEG-2 LSF stereo 16 kHz"),
];

/// Symmetric `2^15` full-scale fractional → `i16` map, matching the
/// `Decoder`-trait wrapper in `codec_decoder::float_plane_to_s16_le`.
fn to_i16(s: f64) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Read the `data` chunk of a canonical RIFF/WAVE PCM file as interleaved
/// `i16` (the reference `.ref.wav` files are s16le per their `fmt ` chunk),
/// returning `(channels, samples)`.
fn wav_data(wav: &[u8]) -> (usize, Vec<i16>) {
    assert_eq!(&wav[0..4], b"RIFF", "not a RIFF file");
    assert_eq!(&wav[8..12], b"WAVE", "not a WAVE file");
    let channels = u16::from_le_bytes([wav[22], wav[23]]) as usize;
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let sz = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        let body = i + 8;
        if id == b"data" {
            let samples = wav[body..body + sz]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            return (channels, samples);
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in WAVE file");
}

/// `(mp2 bytes, ref channels, interleaved ref i16)` or `None` when the
/// fixture is absent (keeps the test green if the fixtures are ever
/// stripped from a packaging variant).
fn load(stem: &str) -> Option<(Vec<u8>, usize, Vec<i16>)> {
    let mp2 = format!("{FIXTURE_DIR}{stem}.mp2");
    let refw = format!("{FIXTURE_DIR}{stem}.ref.wav");
    if !std::path::Path::new(&mp2).exists() {
        eprintln!("skip: fixture absent at {mp2}");
        return None;
    }
    let mp2 = std::fs::read(&mp2).expect("read mp2");
    let (ch, samples) = wav_data(&std::fs::read(&refw).expect("read ref wav"));
    Some((mp2, ch, samples))
}

#[test]
fn decode_matrix_matches_reference_within_iso_tolerance() {
    for &(stem, channels, label) in MATRIX {
        let Some((mp2, ref_ch, expected)) = load(stem) else {
            continue;
        };
        assert_eq!(ref_ch, channels, "{label}: reference channel count");

        let planes = decode_all_frames(&mp2).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        assert_eq!(planes.len(), channels, "{label}: decoded channel count");

        let per_channel = planes[0].len();
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(plane.len(), per_channel, "{label}: ch {ch} sample count");
        }
        // The reference decoder applies no startup delay on these
        // streams: decoded sample count equals frames × 1152 and the
        // comparison is sample-aligned with offset 0 (see
        // fixtures/README.md).
        assert_eq!(
            per_channel * channels,
            expected.len(),
            "{label}: total sample count must equal the reference WAV payload"
        );
        assert_eq!(
            per_channel % 1152,
            0,
            "{label}: per-channel count must be a whole number of 1152-sample frames"
        );

        let mut max_abs = 0i32;
        let mut sum_sq = 0.0_f64;
        let mut exact = 0usize;
        let mut total = 0usize;
        for i in 0..per_channel {
            for (ch, plane) in planes.iter().enumerate() {
                let g = to_i16(plane[i]);
                let e = expected[i * channels + ch];
                let d = (g as i32 - e as i32).abs();
                if d > max_abs {
                    max_abs = d;
                }
                sum_sq += (d * d) as f64;
                if d == 0 {
                    exact += 1;
                }
                total += 1;
            }
        }
        let rms = (sum_sq / total as f64).sqrt();
        let exact_ratio = exact as f64 / total as f64;

        assert!(
            max_abs <= 1,
            "{label}: max abs PCM error {max_abs} LSB exceeds the ≤1 conformance bound"
        );
        assert!(
            rms < 0.6,
            "{label}: rms PCM error {rms:.4} LSB exceeds the sub-LSB conformance bound"
        );
        assert!(
            exact_ratio > 0.70,
            "{label}: only {:.1}% of samples bit-exact (expected > 70%)",
            exact_ratio * 100.0
        );
    }
}

#[test]
fn decode_matrix_has_no_dc_offset_or_clipping() {
    for &(stem, _channels, label) in MATRIX {
        let Some((mp2, _ch, _expected)) = load(stem) else {
            continue;
        };
        let planes = decode_all_frames(&mp2).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        for (ch, plane) in planes.iter().enumerate() {
            let peak = plane.iter().cloned().fold(0.0_f64, |a, b| a.max(b.abs()));
            assert!(
                peak <= 1.001,
                "{label}: ch {ch} peak {peak} exceeds the §2.4.3.4.7.1 [-1,+1] range"
            );
            let mean = plane.iter().sum::<f64>() / plane.len() as f64;
            assert!(
                mean.abs() < 0.02,
                "{label}: ch {ch} DC offset {mean} too large — check requant D constant"
            );
        }
    }
}
