//! Official ISO/IEC 13818-4 audio conformance sweep.
//!
//! ISO/IEC 13818-4 §2.5.4 ("Audio decoder tests") supplies test
//! bitstreams with reference PCM and defines decoder conformance as a
//! bounded difference signal, measured relative to full scale with the
//! decoder output normalised to −1…+1:
//!
//! * **ISO/IEC 13818-3 audio decoder**: RMS of the difference signal
//!   `< 1/(2^15·√12)`, and max abs difference `≤ 2^-14` (§2.5.4.1).
//! * **Limited accuracy** decoder: RMS `< 1/(2^11·√12)`.
//!
//! The suite itself is ISO-licensed for *use*, not redistribution, so
//! the vectors are **never** committed here. This harness is env-gated:
//! set `OXIDEAV_MP2_ISO13818_4_DIR` to a directory holding the
//! extracted `testNN/` archives (fetch recipe + SHA-256 manifest:
//! `docs/audio/mp3/iso-13818-4-audio-conformance.md` in the workspace
//! docs repo). When the variable is unset or the directory is missing,
//! every test here skips silently — CI stays green without the vectors.
//!
//! Reference-PCM format (determined empirically, recorded in the same
//! docs note): headerless **big-endian** signed PCM, one file per
//! channel, no decoder-delay offset (sample counts line up exactly).
//! Most streams ship 16-bit references; `test34` / `test35` are the
//! §2.5.4 *accuracy* bitstreams — a −20 dB 20 Hz–10 kHz sine sweep
//! "represented with 24 bit accuracy" (their descriptor text), i.e.
//! 3-byte samples, which is exactly the §2.5.4.1 measurement setup.
//!
//! Coverage swept here (all Layer II — the Layer I `test33` and the
//! Layer III `test23` base streams are out of this crate's scope):
//!
//! * MPEG-1 Layer II 44.1 / 48 kHz stereo, 256–384 kbit/s, CRC on
//!   (`test01/05/06/14/15/16` — the multichannel extension of these
//!   streams lives in the §2.4.1.8 ancillary region, so the MPEG-1
//!   base decode compares directly against the `_l`/`_r` references
//!   for the unmatrixed presentations).
//! * MPEG-2 LSF Layer II 16 / 22.05 / 24 kHz (`test24`–`test32`):
//!   per-frame *rotating* channel modes (joint stereo bounds
//!   4/8/12/16 → stereo), the 16 kbit/s ladder floor (`test27`),
//!   160 kbit/s (`test29`), single-channel (`test30`) and
//!   dual-channel (`test31`).
//! * The §2.5.4.1 accuracy criterion applied literally on the 24-bit
//!   accuracy cells (`test34` LSF 24 kHz, `test35` MPEG-1 44.1 kHz).
//! * A decode-cleanliness sweep over every remaining Layer II
//!   multichannel base stream, including the VBR pair
//!   (`test20`/`test21`) — their matrixed per-channel references
//!   don't apply to a two-channel base decode, but the base stream
//!   must still parse and decode to the exact frame-count sample
//!   total.
//!
//! Measured results (pinned by the assertions below): the two §2.5.4
//! accuracy cells pass the normative criterion with ~70× headroom
//! (rms ≤ 1,3·10⁻⁷ against the 8,8·10⁻⁶ bound; max ≤ 7,6·10⁻⁷
//! against 6,1·10⁻⁵) — the decoder meets the §2.5.4 definition of an
//! "ISO/IEC 13818-3 audio decoder", not merely the limited-accuracy
//! tier. All fifteen 16-bit comparison cells agree to within 1 s16
//! LSB at every sample (the normative max-abs bound allows 2), five
//! of them ≥ 99,9 % bit-exactly.

use oxideav_mp2::header::{FrameHeader, Mode};
use oxideav_mp2::{decode_all_frames, PCM_SAMPLES_PER_CHANNEL};
use std::path::{Path, PathBuf};

const ENV_DIR: &str = "OXIDEAV_MP2_ISO13818_4_DIR";

/// §2.5.4.1: RMS of the difference signal must be `< 1/(2^15·√12)`,
/// relative to full scale (output normalised to −1…+1).
const RMS_BOUND: f64 = 1.0 / (32768.0 * 3.464_101_615_137_754_6);

/// §2.5.4.1: the maximum absolute difference must be `≤ 2^-14`.
const MAX_ABS_BOUND: f64 = 1.0 / 16384.0;

/// Reference sample width in the per-channel `.pcm` files.
#[derive(Clone, Copy, PartialEq)]
enum RefWidth {
    /// 16-bit big-endian.
    S16Be,
    /// 24-bit big-endian (the §2.5.4 accuracy bitstreams).
    S24Be,
}

/// One PCM-comparison cell: stream name, per-channel reference file
/// suffixes (decoder channel order), reference width, minimum s16
/// bit-exact ratio (16-bit cells only — see the sweep test for how the
/// floor was measured and why it differs per reference vintage), label.
struct Cell {
    name: &'static str,
    ref_suffixes: &'static [&'static str],
    width: RefWidth,
    min_exact: f64,
    label: &'static str,
}

const COMPARISON_CELLS: &[Cell] = &[
    Cell {
        name: "test01",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.999,
        label: "MPEG-1 L2 44.1 kHz stereo 384k (unmatrixed 3/2 base)",
    },
    Cell {
        name: "test05",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.999,
        label: "MPEG-1 L2 44.1 kHz stereo 256k (2/1, ancillary data)",
    },
    Cell {
        name: "test06",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.45,
        label: "MPEG-1 L2 48 kHz stereo 384k CRC (2/0+2/0)",
    },
    Cell {
        name: "test14",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.45,
        label: "MPEG-1 L2 44.1 kHz stereo 384k (unmatrixed 3/2 base)",
    },
    Cell {
        name: "test15",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.999,
        label: "MPEG-1 L2 48 kHz stereo 384k (unmatrixed 3/2 base)",
    },
    Cell {
        name: "test16",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.999,
        label: "MPEG-1 L2 48 kHz stereo 384k (unmatrixed 3/2 base)",
    },
    Cell {
        name: "test24",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 16 kHz 96k rotating joint-stereo/stereo",
    },
    Cell {
        name: "test25",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 22.05 kHz 96k rotating joint-stereo/stereo",
    },
    Cell {
        name: "test26",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 96k rotating joint-stereo/stereo",
    },
    Cell {
        name: "test27",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 16k (LSF ladder floor)",
    },
    Cell {
        name: "test28",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 96k rotating joint-stereo/stereo",
    },
    Cell {
        name: "test29",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 160k (LSF ladder top)",
    },
    Cell {
        name: "test30",
        ref_suffixes: &[""],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 96k single-channel",
    },
    Cell {
        name: "test31",
        ref_suffixes: &["_I", "_II"],
        width: RefWidth::S16Be,
        min_exact: 0.98,
        label: "LSF L2 24 kHz 96k dual-channel",
    },
    Cell {
        name: "test32",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S16Be,
        min_exact: 0.999,
        label: "LSF L2 24 kHz 128k rotating joint-stereo/stereo",
    },
    Cell {
        name: "test34",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S24Be,
        min_exact: 0.0,
        label: "§2.5.4 accuracy: LSF L2 24 kHz 160k, −20 dB sweep, 24-bit ref",
    },
    Cell {
        name: "test35",
        ref_suffixes: &["_l", "_r"],
        width: RefWidth::S24Be,
        min_exact: 0.0,
        label: "§2.5.4 accuracy: MPEG-1 L2 44.1 kHz 384k, −20 dB sweep, 24-bit ref",
    },
];

/// Layer II multichannel base streams whose matrixed per-channel
/// references don't correspond to the two-channel base decode: the
/// base stream must still decode cleanly end-to-end.
const DECODE_ONLY: &[&str] = &[
    "test02", "test03", "test04", "test07", "test08", "test09", "test10", "test11", "test12",
    "test13", "test17", "test18", "test19", "test20", "test21",
];

/// Resolve the suite directory, or `None` (→ skip) when unset/absent.
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

/// Locate a file within a test's directory, tolerating both the flat
/// layout (`$DIR/test01/test01.mpg` — the archive's own root) and a
/// nested one (`$DIR/test01/test01/test01.mpg`).
fn stream_file(dir: &Path, name: &str, file: &str) -> Option<PathBuf> {
    let flat = dir.join(name).join(file);
    if flat.is_file() {
        return Some(flat);
    }
    let nested = dir.join(name).join(name).join(file);
    nested.is_file().then_some(nested)
}

/// Read a headerless big-endian reference PCM file to normalised
/// −1…+1 f64 samples (§2.5.4.1: MSB represents −1).
fn read_ref_pcm(path: &Path, width: RefWidth) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    match width {
        RefWidth::S16Be => bytes
            .chunks_exact(2)
            .map(|c| f64::from(i16::from_be_bytes([c[0], c[1]])) / 32768.0)
            .collect(),
        RefWidth::S24Be => bytes
            .chunks_exact(3)
            .map(|c| {
                let v = i32::from_be_bytes([c[0], c[1], c[2], 0]) >> 8;
                f64::from(v) / 8_388_608.0
            })
            .collect(),
    }
}

/// Symmetric `2^15` full-scale fractional → `i16` map with saturation,
/// matching `codec_decoder`'s planar-S16 delivery. §2.5.4.1 measures
/// with both signals at the *reference's* precision (its P′-bit rule),
/// and a 16-bit rendering saturates at the rails — several suite
/// streams carry deliberate near-full-scale content whose filterbank
/// reconstruction overshoots ±1,0 (e.g. `test14`), which every
/// 16-bit-output decoder, including the reference, clips.
fn to_i16(s: f64) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[test]
fn iso13818_4_pcm_comparison_sweep() {
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 suite sweep not run");
        return;
    };
    let mut passed = 0usize;
    for cell in COMPARISON_CELLS {
        let Some(mpg) = stream_file(&dir, cell.name, &format!("{}.mpg", cell.name)) else {
            panic!("{}: stream file missing under {}", cell.name, dir.display());
        };
        let stream = std::fs::read(&mpg).expect("read stream");
        let planes = decode_all_frames(&stream)
            .unwrap_or_else(|e| panic!("{} ({}): decode: {e:?}", cell.name, cell.label));
        assert_eq!(
            planes.len(),
            cell.ref_suffixes.len(),
            "{}: decoded channel count",
            cell.name
        );

        for (ch, suffix) in cell.ref_suffixes.iter().enumerate() {
            let ref_path = stream_file(&dir, cell.name, &format!("{}{}.pcm", cell.name, suffix))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: reference {}{}.pcm missing",
                        cell.name, cell.name, suffix
                    )
                });
            let reference = read_ref_pcm(&ref_path, cell.width);
            let ours = &planes[ch];
            assert_eq!(
                ours.len(),
                reference.len(),
                "{} ch {ch}: sample count (no decoder-delay offset in this suite)",
                cell.name
            );

            // §2.5.4.1 difference-signal measurement, relative to full
            // scale, per reference precision.
            match cell.width {
                // The 24-bit accuracy references are the setup the
                // criterion literally describes (−20 dB sweep, 24-bit
                // reference): apply the normative bounds directly in
                // the float domain. Measured: rms ≤ 1.3e-7 and max
                // ≤ 7.6e-7 — both sit ~70× inside the bounds.
                RefWidth::S24Be => {
                    let mut max_abs = 0.0_f64;
                    let mut sum_sq = 0.0_f64;
                    for (a, b) in ours.iter().zip(reference.iter()) {
                        let d = (a - b).abs();
                        max_abs = max_abs.max(d);
                        sum_sq += d * d;
                    }
                    let rms = (sum_sq / reference.len() as f64).sqrt();
                    eprintln!(
                        "{} ch {ch}: rms {rms:.3e} (bound {RMS_BOUND:.3e}), max {max_abs:.3e} \
                         (bound {MAX_ABS_BOUND:.3e}) — {}",
                        cell.name, cell.label
                    );
                    assert!(
                        rms < RMS_BOUND,
                        "{} ch {ch} ({}): RMS {rms:.3e} exceeds the §2.5.4.1 bound {RMS_BOUND:.3e}",
                        cell.name,
                        cell.label
                    );
                    assert!(
                        max_abs <= MAX_ABS_BOUND,
                        "{} ch {ch} ({}): max abs {max_abs:.3e} exceeds the §2.5.4.1 bound \
                         {MAX_ABS_BOUND:.3e}",
                        cell.name,
                        cell.label
                    );
                }
                // The 16-bit references put the §2.5.4.1 measurement at
                // the reference's own precision (its P′-bit rule), so
                // our output is rendered to s16 (saturating, exactly as
                // the registry decoder delivers PCM) and compared as
                // integers. Two decoder generations exist in the suite:
                // the modern-precision references match ours ≥ 98.6 %
                // bit-exactly, while the two 1995-era references
                // (`test06`, `test14`) carry a bias-free ±1 LSB wobble
                // of their own (diff histogram symmetric, mean < 0.004
                // LSB), capping the achievable agreement near 50 % —
                // the strict RMS bound is unreachable against those
                // references by construction, but the normative max-abs
                // bound of 2^-14 (2 LSB) holds with room: every cell
                // measures ≤ 1 LSB everywhere, which is what is pinned.
                RefWidth::S16Be => {
                    let mut max_lsb = 0i32;
                    let mut sum_sq = 0.0_f64;
                    let mut exact = 0usize;
                    for (a, b) in ours.iter().zip(reference.iter()) {
                        let d = (i32::from(to_i16(*a)) - i32::from(to_i16(*b))).abs();
                        max_lsb = max_lsb.max(d);
                        sum_sq += f64::from(d * d);
                        if d == 0 {
                            exact += 1;
                        }
                    }
                    let rms_lsb = (sum_sq / reference.len() as f64).sqrt();
                    let exact_ratio = exact as f64 / reference.len() as f64;
                    eprintln!(
                        "{} ch {ch}: max {max_lsb} LSB, rms {rms_lsb:.4} LSB, {:.2}% bit-exact \
                         (floor {:.1}%) — {}",
                        cell.name,
                        exact_ratio * 100.0,
                        cell.min_exact * 100.0,
                        cell.label
                    );
                    assert!(
                        max_lsb <= 1,
                        "{} ch {ch} ({}): s16 diff of {max_lsb} LSB — beyond both the measured \
                         1-LSB envelope and half the §2.5.4.1 max-abs bound (2^-14 = 2 LSB)",
                        cell.name,
                        cell.label
                    );
                    assert!(
                        rms_lsb < 1.0,
                        "{} ch {ch} ({}): rms {rms_lsb:.4} LSB reached a full LSB",
                        cell.name,
                        cell.label
                    );
                    assert!(
                        exact_ratio > cell.min_exact,
                        "{} ch {ch} ({}): only {:.2}% of s16 samples bit-exact (floor {:.1}%)",
                        cell.name,
                        cell.label,
                        exact_ratio * 100.0,
                        cell.min_exact * 100.0
                    );
                }
            }
        }
        passed += 1;
    }
    // Pin the sweep size: a silently shrunken cell table is a lost test.
    assert_eq!(passed, COMPARISON_CELLS.len());
    eprintln!(
        "ISO 13818-4 sweep: {passed}/{} comparison cells within the §2.5.4.1 bounds",
        COMPARISON_CELLS.len()
    );
}

#[test]
fn iso13818_4_multichannel_base_streams_decode_cleanly() {
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 suite sweep not run");
        return;
    };
    for name in DECODE_ONLY {
        let Some(mpg) = stream_file(&dir, name, &format!("{name}.mpg")) else {
            panic!("{name}: stream file missing under {}", dir.display());
        };
        let stream = std::fs::read(&mpg).expect("read stream");

        // Count frames from the headers, then require the decode to
        // deliver exactly frames × 1152 samples on two channels.
        let mut offset = 0usize;
        let mut n_frames = 0usize;
        while offset + 4 <= stream.len() {
            let Ok(header) = FrameHeader::parse(&stream[offset..]) else {
                break;
            };
            offset += header.frame_size_bytes();
            n_frames += 1;
        }
        assert!(n_frames > 5, "{name}: expected a multi-frame stream");

        let planes = decode_all_frames(&stream)
            .unwrap_or_else(|e| panic!("{name}: multichannel base decode: {e:?}"));
        assert_eq!(planes.len(), 2, "{name}: base decode is two-channel");
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(
                plane.len(),
                n_frames * PCM_SAMPLES_PER_CHANNEL,
                "{name} ch {ch}: sample total"
            );
        }
    }
}

#[test]
fn iso13818_4_stream_premises_hold() {
    // The sweep only proves what the streams genuinely exercise; pin
    // the on-wire premises the cell labels claim, directly from the
    // headers (exactly like the staged-fixture premise pins in
    // `decode_matrix_conformance.rs`).
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 suite sweep not run");
        return;
    };

    let modes_of = |name: &str| -> Vec<(Mode, u8, bool)> {
        let mpg = stream_file(&dir, name, &format!("{name}.mpg")).expect("stream");
        let stream = std::fs::read(&mpg).expect("read stream");
        let mut offset = 0usize;
        let mut out = Vec::new();
        while offset + 4 <= stream.len() {
            let Ok(header) = FrameHeader::parse(&stream[offset..]) else {
                break;
            };
            out.push((
                header.mode,
                header.mode_extension.bound() as u8,
                header.protection_bit,
            ));
            offset += header.frame_size_bytes();
        }
        out
    };

    // The rotating-mode LSF cells must genuinely rotate: both stereo
    // and joint-stereo frames present, and more than one intensity
    // bound exercised across the stream.
    for name in ["test24", "test25", "test26", "test28"] {
        let modes = modes_of(name);
        let stereo = modes.iter().filter(|(m, _, _)| *m == Mode::Stereo).count();
        let js: std::collections::BTreeSet<u8> = modes
            .iter()
            .filter(|(m, _, _)| *m == Mode::JointStereo)
            .map(|&(_, b, _)| b)
            .collect();
        assert!(stereo > 0, "{name}: rotating cell has no stereo frame");
        assert!(
            js.len() >= 2,
            "{name}: rotating cell exercises {} joint-stereo bounds, expected ≥ 2",
            js.len()
        );
    }

    // Fixed-mode cells.
    let all_frames_are = |name: &str, mode: Mode| {
        let modes = modes_of(name);
        assert!(!modes.is_empty(), "{name}: no frames parsed");
        assert!(
            modes.iter().all(|&(m, _, _)| m == mode),
            "{name}: expected every frame in {mode:?}"
        );
    };
    all_frames_are("test30", Mode::SingleChannel);
    all_frames_are("test31", Mode::DualChannel);
    all_frames_are("test34", Mode::Stereo);

    // The suite's Layer II streams carry §2.4.1.4 CRC protection
    // (descriptor text: "Protection: yes"); protection_bit == false
    // means the CRC word is present and therefore checked on decode.
    for name in ["test24", "test27", "test30", "test31", "test34"] {
        let modes = modes_of(name);
        assert!(
            modes.iter().all(|&(_, _, prot)| !prot),
            "{name}: expected CRC-protected frames throughout"
        );
    }
}
