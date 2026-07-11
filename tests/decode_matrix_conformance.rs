//! Full Layer II decode-conformance matrix: header → bit-allocation →
//! §2.4.3.3.4 requantization → §2.4.3.3.3 scalefactor rescaling →
//! §2.4.3.2 / Annex A Figure A.2 synthesis filterbank, validated against
//! an independent black-box reference decoder across the whole
//! channel-mode × sampling-rate × bit-allocation-sub-table matrix.
//!
//! The existing `tests/layer2_pcm_conformance.rs` pins one staged
//! 44.1 kHz stereo stream. This test broadens that single point of
//! ground truth into the whole rate ladder — MPEG-1 single-channel and
//! stereo at 32 / 44.1 / 48 kHz, and MPEG-2 LSF at 16 / 22.05 / 24 kHz —
//! and (since r411) into every Table 3-B.2 allocation sub-table and the
//! bitrate-ladder extremes, so a regression localised to one rate's
//! bit-allocation table ([`oxideav_mp2::bitalloc::select_table`]) or LSF
//! sizing cannot hide behind the others.
//!
//! # Conformance bounds
//!
//! Two reference formats coexist under `tests/fixtures/`:
//!
//! * **s16 references** (the original six cells): the reference
//!   decoder's own integer PCM. ISO/IEC 11172-3 §2.4.3.2 defines the
//!   synthesis filterbank in floating point with no prescribed
//!   accumulation order, so conformance (ISO/IEC 11172-4 / 13818-4) is a
//!   *bounded* difference signal: max abs ≤ 1 LSB, rms well under 1 LSB,
//!   high exact-match ratio.
//!
//! * **f32 references** (the r411 corpus): the reference decoder's
//!   *floating-point* PCM, captured before any integer conversion. Here
//!   the bound is far tighter — our f64 chain matches the reference's
//!   f32 output to ≤ 0.05 LSB (measured ≤ 0.025 LSB across the corpus,
//!   i.e. the reference's own 24-bit-mantissa precision floor), and the
//!   residual ±1 LSB flips in the s16 projection occur only where a
//!   sample lands within that wobble of a rounding boundary. That pins
//!   the divergence root-cause: it is integer-rounding latitude on a
//!   float-defined filterbank, not a decode-chain difference. See
//!   `fixtures/GENERATION.md` for the full latitude study (two
//!   independent reference decoders disagree with *each other* at the
//!   same magnitude).

use oxideav_mp2::frame::{decode_frame_with, FrameDecodeState};
use oxideav_mp2::header::FrameHeader;
use oxideav_mp2::{decode_all_frames, PCM_SAMPLES_PER_CHANNEL};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");

/// Float-domain conformance bound for f32-reference cells, in units of
/// one s16 LSB (`2^-15` full scale). Measured worst case across the
/// r411 corpus is 0.025 LSB — the reference decoder computes in f32
/// (24-bit mantissa), so ~1e-7 relative wobble over a 512-tap window
/// accumulation is its own precision floor, not a decode difference.
const F32_REF_BOUND_LSB: f64 = 0.05;

/// (fixture stem, channel count, human label).
///
/// The r411 corpus expansion covers every Table 3-B.2 allocation
/// sub-table in both channel modes (per-channel bitrate in kbit/s is the
/// total for `single_channel`, total/2 for the two-channel modes — see
/// [`oxideav_mp2::bitalloc::select_table`]), the ladder extremes
/// (32 kbit/s mono … 384 kbit/s stereo, LSF 8 … 160 kbit/s), the
/// LSF-only 144 kbit/s bitrate index, and heavy §2.4.2.3 padding at the
/// fractional rates (44.1 / 22.05 kHz). See `fixtures/GENERATION.md`.
const MATRIX: &[(&str, usize, &str)] = &[
    // ── original cells (s16 references) ────────────────────────────────
    ("mono_44k_128", 1, "MPEG-1 single-channel 44.1 kHz B.2b"),
    ("mono_32k_96", 1, "MPEG-1 single-channel 32 kHz B.2b"),
    ("stereo_48k_192", 2, "MPEG-1 stereo 48 kHz B.2a"),
    ("mono_22k_64", 1, "MPEG-2 LSF single-channel 22.05 kHz"),
    ("stereo_24k_64", 2, "MPEG-2 LSF stereo 24 kHz"),
    ("stereo_16k_64", 2, "MPEG-2 LSF stereo 16 kHz"),
    // ── r411: Table 3-B.2 sub-table coverage, MPEG-1 (f32 references) ──
    (
        "mono_44k_32",
        1,
        "MPEG-1 mono 44.1 kHz 32k B.2c (ladder floor)",
    ),
    (
        "stereo_44k_64",
        2,
        "MPEG-1 stereo 44.1 kHz 64k (32k/ch) B.2c",
    ),
    (
        "stereo_44k_128",
        2,
        "MPEG-1 stereo 44.1 kHz 128k (64k/ch) B.2a",
    ),
    (
        "stereo_44k_256",
        2,
        "MPEG-1 stereo 44.1 kHz 256k (128k/ch) B.2b",
    ),
    ("mono_48k_48", 1, "MPEG-1 mono 48 kHz 48k B.2c"),
    ("mono_48k_56", 1, "MPEG-1 mono 48 kHz 56k B.2a"),
    ("stereo_48k_96", 2, "MPEG-1 stereo 48 kHz 96k (48k/ch) B.2c"),
    (
        "stereo_48k_384",
        2,
        "MPEG-1 stereo 48 kHz 384k (192k/ch) B.2a (ladder top)",
    ),
    ("mono_32k_48", 1, "MPEG-1 mono 32 kHz 48k B.2d"),
    ("mono_32k_56", 1, "MPEG-1 mono 32 kHz 56k B.2a"),
    ("stereo_32k_64", 2, "MPEG-1 stereo 32 kHz 64k (32k/ch) B.2d"),
    (
        "stereo_32k_224",
        2,
        "MPEG-1 stereo 32 kHz 224k (112k/ch) B.2b",
    ),
    // ── r411: MPEG-2 LSF ladder extremes + LSF-only 144 index ──────────
    (
        "mono_16k_8",
        1,
        "MPEG-2 LSF mono 16 kHz 8k (LSF ladder floor)",
    ),
    (
        "stereo_22k_96",
        2,
        "MPEG-2 LSF stereo 22.05 kHz 96k (padding-heavy)",
    ),
    (
        "mono_24k_144",
        1,
        "MPEG-2 LSF mono 24 kHz 144k (LSF-only index)",
    ),
    (
        "stereo_24k_160",
        2,
        "MPEG-2 LSF stereo 24 kHz 160k (LSF ladder top)",
    ),
];

/// Symmetric `2^15` full-scale fractional → `i16` map, matching the
/// `Decoder`-trait wrapper in `codec_decoder::float_plane_to_s16_le`.
fn to_i16(s: f64) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Reference PCM: either the reference decoder's integer output (the
/// original cells) or its floating-point output (the r411 corpus),
/// interleaved.
enum RefPcm {
    S16(Vec<i16>),
    F32(Vec<f32>),
}

impl RefPcm {
    fn len(&self) -> usize {
        match self {
            RefPcm::S16(v) => v.len(),
            RefPcm::F32(v) => v.len(),
        }
    }
}

/// Read the `data` chunk of a RIFF/WAVE file, honouring the `fmt `
/// chunk's format code: 1 (integer PCM, 16-bit) or 3 (IEEE float,
/// 32-bit), returning `(channels, samples)`.
fn wav_data(wav: &[u8]) -> (usize, RefPcm) {
    assert_eq!(&wav[0..4], b"RIFF", "not a RIFF file");
    assert_eq!(&wav[8..12], b"WAVE", "not a WAVE file");
    let mut fmt_code = 0u16;
    let mut channels = 0usize;
    let mut i = 12;
    while i + 8 <= wav.len() {
        let id = &wav[i..i + 4];
        let sz = u32::from_le_bytes([wav[i + 4], wav[i + 5], wav[i + 6], wav[i + 7]]) as usize;
        let body = i + 8;
        if id == b"fmt " {
            fmt_code = u16::from_le_bytes([wav[body], wav[body + 1]]);
            channels = u16::from_le_bytes([wav[body + 2], wav[body + 3]]) as usize;
        }
        if id == b"data" {
            let payload = &wav[body..body + sz];
            let pcm = match fmt_code {
                1 => RefPcm::S16(
                    payload
                        .chunks_exact(2)
                        .map(|c| i16::from_le_bytes([c[0], c[1]]))
                        .collect(),
                ),
                3 => RefPcm::F32(
                    payload
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect(),
                ),
                other => panic!("unsupported WAVE format code {other}"),
            };
            return (channels, pcm);
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in WAVE file");
}

/// `(mp2 bytes, ref channels, interleaved reference PCM)` or `None` when
/// the fixture is absent (keeps the test green if the fixtures are ever
/// stripped from a packaging variant).
fn load(stem: &str) -> Option<(Vec<u8>, usize, RefPcm)> {
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

        match &expected {
            RefPcm::S16(expected) => {
                // Integer reference: the ISO-grade bounded-difference
                // envelope (see module docs).
                let mut max_abs = 0i32;
                let mut sum_sq = 0.0_f64;
                let mut exact = 0usize;
                let mut total = 0usize;
                for i in 0..per_channel {
                    for (ch, plane) in planes.iter().enumerate() {
                        let g = to_i16(plane[i]);
                        let e = expected[i * channels + ch];
                        let d = (g as i32 - e as i32).abs();
                        max_abs = max_abs.max(d);
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
            RefPcm::F32(expected) => {
                // Float reference: our f64 chain must sit within the
                // reference's own f32 precision floor of its output —
                // near-bit-exactness — and the s16 projection may then
                // differ only by boundary-straddle ±1 flips.
                let mut max_float_lsb = 0.0_f64;
                let mut s16_max = 0i32;
                let mut s16_exact = 0usize;
                let mut total = 0usize;
                for i in 0..per_channel {
                    for (ch, plane) in planes.iter().enumerate() {
                        let ours = plane[i];
                        let theirs = f64::from(expected[i * channels + ch]);
                        max_float_lsb = max_float_lsb.max((ours - theirs).abs() * 32768.0);
                        let d = (to_i16(ours) as i32 - to_i16(theirs) as i32).abs();
                        s16_max = s16_max.max(d);
                        if d == 0 {
                            s16_exact += 1;
                        }
                        total += 1;
                    }
                }
                let exact_ratio = s16_exact as f64 / total as f64;
                assert!(
                    max_float_lsb <= F32_REF_BOUND_LSB,
                    "{label}: float-domain divergence {max_float_lsb:.4} LSB exceeds \
                     the {F32_REF_BOUND_LSB} bound — a real decode-chain difference, \
                     not rounding latitude"
                );
                assert!(
                    s16_max <= 1,
                    "{label}: s16 projection differs by {s16_max} LSB (boundary flips \
                     can only ever be ±1)"
                );
                assert!(
                    exact_ratio > 0.99,
                    "{label}: only {:.2}% of s16 samples bit-exact (expected > 99%)",
                    exact_ratio * 100.0
                );
            }
        }
    }
}

#[test]
fn decode_matrix_is_within_bound_on_every_individual_frame() {
    // A whole-stream rms/exact-ratio can mask a regression confined to
    // one frame (e.g. a cold-start V-buffer slip on frame 0, or a
    // padding-byte frame mis-sized). Pin the envelope on *each*
    // 1152-sample frame independently — including frame 0, where the
    // §2.4.3.3.5 V ring buffer is cold (Annex A Figure A.2 footnote 1)
    // exactly as the reference decoder's is.
    for &(stem, channels, label) in MATRIX {
        let Some((mp2, _ref_ch, expected)) = load(stem) else {
            continue;
        };
        let planes = decode_all_frames(&mp2).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        let per_channel = planes[0].len();
        let n_frames = per_channel / PCM_SAMPLES_PER_CHANNEL;
        assert!(n_frames > 0, "{label}: at least one frame");

        for f in 0..n_frames {
            let mut frame_max_s16 = 0i32;
            let mut frame_max_float_lsb = 0.0_f64;
            for i in f * PCM_SAMPLES_PER_CHANNEL..(f + 1) * PCM_SAMPLES_PER_CHANNEL {
                for (ch, plane) in planes.iter().enumerate() {
                    match &expected {
                        RefPcm::S16(e) => {
                            let g = to_i16(plane[i]);
                            frame_max_s16 =
                                frame_max_s16.max((g as i32 - e[i * channels + ch] as i32).abs());
                        }
                        RefPcm::F32(e) => {
                            let theirs = f64::from(e[i * channels + ch]);
                            frame_max_float_lsb =
                                frame_max_float_lsb.max((plane[i] - theirs).abs() * 32768.0);
                        }
                    }
                }
            }
            assert!(
                frame_max_s16 <= 1,
                "{label}: frame {f} max abs PCM error {frame_max_s16} LSB exceeds the ≤1 bound"
            );
            assert!(
                frame_max_float_lsb <= F32_REF_BOUND_LSB,
                "{label}: frame {f} float divergence {frame_max_float_lsb:.4} LSB exceeds \
                 the {F32_REF_BOUND_LSB} bound"
            );
        }
    }
}

#[test]
fn streaming_decode_equals_batch_decode() {
    // Decoding frame-by-frame through `decode_frame_with` with a single
    // persisted `FrameDecodeState` must be byte-identical to the
    // batch `decode_all_frames` path: both thread the same §2.4.3.3.5 V
    // ring buffer across frames, so any divergence would mean the batch
    // loop resets or mis-chains filterbank state.
    for &(stem, channels, label) in MATRIX {
        let Some((mp2, _ref_ch, _expected)) = load(stem) else {
            continue;
        };
        let batch = decode_all_frames(&mp2).unwrap_or_else(|e| panic!("{label}: batch: {e:?}"));

        let mut state = FrameDecodeState::new();
        let mut streamed: Vec<Vec<f64>> = vec![Vec::new(); channels];
        let mut offset = 0usize;
        while offset + 4 <= mp2.len() {
            if !(mp2[offset] == 0xFF && (mp2[offset + 1] & 0xF0) == 0xF0) {
                offset += 1;
                continue;
            }
            let header = FrameHeader::parse(&mp2[offset..]).expect("header");
            let size = header.frame_size_bytes();
            let frame = decode_frame_with(&mp2[offset..], &mut state).expect("decode_frame_with");
            for (ch, samples) in frame.pcm.iter().enumerate() {
                streamed[ch].extend_from_slice(samples);
            }
            offset += size;
        }

        assert_eq!(streamed.len(), batch.len(), "{label}: channel count");
        for ch in 0..streamed.len() {
            assert_eq!(
                streamed[ch], batch[ch],
                "{label}: ch {ch} streaming decode diverged from batch decode (bit-identical f64 expected)"
            );
        }
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
