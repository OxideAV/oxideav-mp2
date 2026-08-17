//! §2.5 multichannel **encoder** validated on official ISO/IEC 13818-4
//! programme material as oracle *input*.
//!
//! The suite's multichannel reference PCM files are real multichannel
//! programme material (the §2.5.4.1 measurement inputs). This harness
//! feeds them **into** the `mc_encode` §2.5 encoder — never comparing
//! against any third-party encoder's bitstream — and decodes the
//! result with this crate's own §2.5 decoder, pinning:
//!
//! * the emitted stream is detected as multichannel by the §2.5.3.1
//!   CRC rule and decodes to the full presentation-channel set;
//! * per-channel delay-compensated SNR floors on real material (the
//!   §2.5.3.3 matrixing + §C.1.5.2.7 MC allocation survive content
//!   far denser than the in-tree tone matrix);
//! * the MPEG-1-compatible base decode stays a bounded-error rendering
//!   of the §2.5.3.3 compatible downmix of the same material.
//!
//! Gating and licence handling match the decode sweeps: the vectors
//! are ISO *use*-licensed and never committed; set
//! `OXIDEAV_MP2_ISO13818_4_DIR` to the extracted archives (recipe +
//! SHA-256 manifest: `docs/audio/mp3/iso-13818-4-audio-conformance.md`)
//! or this test skips silently.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::mc::{decode_mc_stream, has_mc_extension};
use oxideav_mp2::mc_encode::{encode_mc_all_frames, McEncodeConfig};
use oxideav_mp2::{decode_all_frames, FrameHeader, PCM_SAMPLES_PER_CHANNEL};
use std::path::{Path, PathBuf};

const ENV_DIR: &str = "OXIDEAV_MP2_ISO13818_4_DIR";

/// Combined analysis + synthesis filterbank group delay (samples).
/// Established empirically here by a cross-correlation lag sweep on
/// the suite's broadband programme material — the exact peak is 481
/// samples (the tone-based in-tree tests are insensitive to the last
/// sample and use the 480 envelope constant; broadband material is
/// not, so this harness compensates the exact lag).
const FILTERBANK_DELAY: usize = 481;

const SQRT2: f64 = std::f64::consts::SQRT_2;

fn suite_dir() -> Option<PathBuf> {
    let dir = std::env::var_os(ENV_DIR)?;
    let dir = PathBuf::from(dir);
    if dir.is_dir() {
        Some(dir)
    } else {
        eprintln!("skip: {ENV_DIR}={} is not a directory", dir.display());
        None
    }
}

/// Tolerate both the flat and nested archive layouts.
fn stream_file(dir: &Path, name: &str, file: &str) -> Option<PathBuf> {
    let flat = dir.join(name).join(file);
    if flat.is_file() {
        return Some(flat);
    }
    let nested = dir.join(name).join(name).join(file);
    nested.is_file().then_some(nested)
}

/// Headerless 16-bit big-endian reference PCM → f64 `[-1, +1)`.
fn read_ref_f64(path: &Path) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    bytes
        .chunks_exact(2)
        .map(|c| f64::from(i16::from_be_bytes([c[0], c[1]])) / 32768.0)
        .collect()
}

fn base_header(sample_rate: u32, bit_rate: u32) -> FrameHeader {
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

/// Delay-compensated per-channel SNR (dB) over the steady middle.
fn snr_db(input: &[f64], output: &[f64]) -> f64 {
    let total = output.len().min(input.len() + FILTERBANK_DELAY);
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = total - PCM_SAMPLES_PER_CHANNEL;
    assert!(hi > lo, "stream long enough");
    let (mut sig, mut err) = (0.0f64, 0.0f64);
    for i in lo..hi {
        let want = input[i - FILTERBANK_DELAY];
        let e = output[i] - want;
        sig += want * want;
        err += e * e;
    }
    assert!(sig > 0.0, "non-trivial reference material");
    10.0 * (sig / err.max(f64::MIN_POSITIVE)).log10()
}

/// One oracle case: reference channel suffixes in
/// [`oxideav_mp2::McConfig::layout`] order, the matching encode
/// configuration, and the asserted per-channel SNR floor (dB) —
/// measured values sit ≥ 3 dB above every floor.
struct OracleCase {
    name: &'static str,
    sample_rate: u32,
    bit_rate: u32,
    suffixes: &'static [&'static str],
    front: u8,
    surround: u8,
    snr_floor_db: f64,
}

const CASES: &[OracleCase] = &[
    // 3/2 at 44,1 kHz — five-channel programme material.
    OracleCase {
        name: "test20",
        sample_rate: 44_100,
        bit_rate: 384_000,
        suffixes: &["l", "r", "c", "ls", "rs"],
        front: 3,
        surround: 2,
        snr_floor_db: 15.0,
    },
    // Second five-channel programme at 44,1 kHz.
    OracleCase {
        name: "test21",
        sample_rate: 44_100,
        bit_rate: 384_000,
        suffixes: &["l", "r", "c", "ls", "rs"],
        front: 3,
        surround: 2,
        snr_floor_db: 10.0,
    },
    // 2/1 at 48 kHz — three-channel programme material.
    OracleCase {
        name: "test19",
        sample_rate: 48_000,
        bit_rate: 384_000,
        suffixes: &["l", "r", "s"],
        front: 2,
        surround: 1,
        snr_floor_db: 27.0,
    },
];

#[test]
fn mc_encoder_round_trips_official_programme_material() {
    let Some(dir) = suite_dir() else { return };
    for case in CASES {
        // Load the reference channels, trimmed to a common
        // whole-frame length.
        let mut pcm: Vec<Vec<f64>> = case
            .suffixes
            .iter()
            .map(|sfx| {
                let p = stream_file(&dir, case.name, &format!("{}_{sfx}.pcm", case.name))
                    .unwrap_or_else(|| panic!("{}: reference _{sfx}.pcm missing", case.name));
                read_ref_f64(&p)
            })
            .collect();
        let min_len = pcm.iter().map(Vec::len).min().unwrap();
        let frames = min_len / PCM_SAMPLES_PER_CHANNEL;
        assert!(frames >= 4, "{}: reference too short", case.name);
        let len = frames * PCM_SAMPLES_PER_CHANNEL;
        for ch in &mut pcm {
            ch.truncate(len);
        }

        // Both predictor elections must clear the same floors — the
        // §2.5.3.2.1.3 encode is exercised on real material too.
        for prediction in [false, true] {
            let cfg = McEncodeConfig {
                front: case.front,
                surround: case.surround,
                prediction,
                ..McEncodeConfig::default()
            };
            let header = base_header(case.sample_rate, case.bit_rate);
            let stream = encode_mc_all_frames(&header, &cfg, &pcm, None)
                .unwrap_or_else(|e| panic!("{}: encode: {e}", case.name));

            // §2.5.3.1 detection fires on the emitted stream.
            assert!(has_mc_extension(&stream), "{}", case.name);

            let decoded = decode_mc_stream(&stream, None)
                .unwrap_or_else(|e| panic!("{}: decode: {e}", case.name));
            assert_eq!(decoded.frames, frames, "{}", case.name);
            assert_eq!(decoded.channels.len(), pcm.len(), "{}", case.name);
            assert_eq!(decoded.dyn_cross_frames, 0);
            if !prediction {
                assert_eq!(decoded.prediction_frames, 0);
            }

            for (ch, out) in decoded.channels.iter().enumerate() {
                let snr = snr_db(&pcm[ch], out);
                eprintln!(
                    "{} (pred={prediction}) ch {ch} ({}): round-trip SNR {snr:.2} dB",
                    case.name, case.suffixes[ch]
                );
                assert!(
                    snr > case.snr_floor_db,
                    "{} ch {ch}: SNR {snr:.2} dB under the {:.1} dB floor",
                    case.name,
                    case.snr_floor_db
                );
            }

            // §2.5.1.3 backward compatibility on real material: the plain
            // Layer II decode approximates the §2.5.3.3 downmix.
            let plain = decode_all_frames(&stream).expect("plain base decode");
            let (alpha, beta, gamma) = (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2);
            let (c_of, ls_of, rs_of) = match (case.front, case.surround) {
                (3, 2) => (Some(2usize), Some(3usize), Some(4usize)),
                (2, 1) => (None, Some(2), Some(2)),
                other => unreachable!("case table only carries 3/2 and 2/1: {other:?}"),
            };
            let get = |idx: Option<usize>, i: usize| idx.map_or(0.0, |k| pcm[k][i]);
            let mut want_lo = vec![0.0f64; len];
            let mut want_ro = vec![0.0f64; len];
            for i in 0..len {
                want_lo[i] = alpha * (pcm[0][i] + beta * get(c_of, i) + gamma * get(ls_of, i));
                want_ro[i] = alpha * (pcm[1][i] + beta * get(c_of, i) + gamma * get(rs_of, i));
            }
            for (tag, want, got) in [("Lo", &want_lo, &plain[0]), ("Ro", &want_ro, &plain[1])] {
                let snr = snr_db(want, got);
                eprintln!("{} {tag}: compatible-downmix SNR {snr:.2} dB", case.name);
                assert!(
                    snr > case.snr_floor_db,
                    "{} {tag}: downmix SNR {snr:.2} dB under the floor",
                    case.name
                );
            }
        }
    }
}
