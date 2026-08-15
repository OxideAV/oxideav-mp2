//! Official ISO/IEC 13818-4 **multichannel** conformance sweep — the
//! §2.5 multichannel-extension decode (`oxideav_mp2::mc`) against the
//! suite's matrixed per-channel reference PCM.
//!
//! The companion `iso13818_4_conformance.rs` sweep validates the
//! MPEG-1-compatible *base* decode; this file validates the extension
//! itself: every Layer II multichannel stream in the suite is decoded
//! to its full presentation-channel set (dematrixing, dynamic
//! crosstalk, multichannel prediction, phantom centre, LFE,
//! multilingual channels, extension bit streams, both fixed and
//! variable bit rate) and compared channel-for-channel against the
//! archives' own reference PCM.
//!
//! Same gating and conventions as the base sweep: the vectors are
//! ISO-licensed for use, never committed; set
//! `OXIDEAV_MP2_ISO13818_4_DIR` to the extracted archives (recipe +
//! SHA-256 manifest: `docs/audio/mp3/iso-13818-4-audio-conformance.md`)
//! or the tests skip silently. References are headerless big-endian
//! PCM, one file per channel, 16-bit except the §2.5.4 accuracy
//! stream `test35` (24-bit, all five channels).
//!
//! Measured results (pinned below):
//!
//! * All twenty Layer II multichannel streams decode end-to-end with
//!   **max abs ≤ 1 s16 LSB on every channel** — full-bandwidth,
//!   LFE, and multilingual alike (the §2.5.4.1 max-abs bound allows
//!   2 LSB). Six streams are 100 % bit-exact on every full-bandwidth
//!   channel.
//! * `test35` (3/2 at 44,1 kHz, 24-bit references) meets the §2.5.4.1
//!   normative accuracy criterion on **all five dematrixed
//!   channels** with ≈ 80× headroom (measured rms ≤ 1,2·10⁻⁷ vs the
//!   8,8·10⁻⁶ bound; max ≤ 6,6·10⁻⁷ vs 6,1·10⁻⁵).
//! * The suite premises are pinned from the wire: all four
//!   `dematrix_procedure` codes, dynamic crosstalk, multichannel
//!   prediction, phantom centre, mono/stereo surround, second stereo
//!   programme, LFE, 7 multilingual channels at both full and half
//!   sampling frequency, extension bit streams, and the VBR pair.

use oxideav_mp2::header::FrameHeader;
use oxideav_mp2::mc::{decode_mc_stream, Centre, McChannel, McDecodedStream, McError, Surround};
use std::path::{Path, PathBuf};

const ENV_DIR: &str = "OXIDEAV_MP2_ISO13818_4_DIR";

/// §2.5.4.1 normative RMS bound: `1/(2^15·√12)`.
const RMS_BOUND: f64 = 1.0 / (32768.0 * 3.464_101_615_137_754_6);
/// §2.5.4.1 max-abs bound: `2^-14`.
const MAX_ABS_BOUND: f64 = 1.0 / 16384.0;

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

/// Tolerate both the flat (`$DIR/test01/test01.mpg`) and nested
/// (`$DIR/test01/test01/test01.mpg`) archive layouts.
fn stream_file(dir: &Path, name: &str, file: &str) -> Option<PathBuf> {
    let flat = dir.join(name).join(file);
    if flat.is_file() {
        return Some(flat);
    }
    let nested = dir.join(name).join(name).join(file);
    nested.is_file().then_some(nested)
}

fn read_ref_s16be(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_be_bytes([c[0], c[1]]))
        .collect()
}

fn read_ref_s24be(path: &Path) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    bytes
        .chunks_exact(3)
        .map(|c| {
            let v = i32::from_be_bytes([c[0], c[1], c[2], 0]) >> 8;
            f64::from(v) / 8_388_608.0
        })
        .collect()
}

fn to_i16(s: f64) -> i16 {
    (s * 32768.0)
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

/// Reference-file suffix for a presentation channel.
fn suffix_of(ch: McChannel) -> &'static str {
    match ch {
        McChannel::Left => "l",
        McChannel::Right => "r",
        McChannel::Centre => "c",
        McChannel::LeftSurround => "ls",
        McChannel::RightSurround => "rs",
        McChannel::MonoSurround => "s",
        McChannel::SecondLeft => "l2",
        McChannel::SecondRight => "r2",
    }
}

fn decode_stream(dir: &Path, name: &str) -> McDecodedStream {
    let mpg = stream_file(dir, name, &format!("{name}.mpg"))
        .unwrap_or_else(|| panic!("{name}: stream file missing under {}", dir.display()));
    let base = std::fs::read(&mpg).expect("read base stream");
    let ext = stream_file(dir, name, &format!("{name}.ext"))
        .map(|p| std::fs::read(&p).expect("read ext stream"));
    decode_mc_stream(&base, ext.as_deref())
        .unwrap_or_else(|e| panic!("{name}: multichannel decode failed: {e}"))
}

/// One 16-bit sweep cell: stream name, whether an extension bit
/// stream must be present, expected layout size, expected LFE / ml
/// presence, and the measured bit-exactness floor being pinned.
struct McCell {
    name: &'static str,
    ext: bool,
    n_channels: usize,
    lfe: bool,
    n_ml: usize,
    /// Minimum per-channel s16 bit-exact ratio to pin. Two reference
    /// vintages exist in the suite (same finding as the base sweep):
    /// modern-precision references agree ≥ 98 %, the 1995/96-era ones
    /// carry a bias-free ±1 LSB wobble capping agreement near 50 %
    /// (34 % on the two rate-starved 384k 3/2 cells).
    min_exact: f64,
    label: &'static str,
}

const MC_CELLS: &[McCell] = &[
    McCell {
        name: "test01",
        ext: false,
        n_channels: 5,
        lfe: true,
        n_ml: 0,
        min_exact: 1.0,
        label: "3/2 + LFE, 44,1 kHz, dematrix '11' (unmatrixed)",
    },
    McCell {
        name: "test02",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.30,
        label: "3/2, 48 kHz, dematrix '00', tc switching",
    },
    McCell {
        name: "test03",
        ext: false,
        n_channels: 4,
        lfe: false,
        n_ml: 0,
        min_exact: 1.0,
        label: "3/1 mono surround, 32 kHz, dematrix '01'",
    },
    McCell {
        name: "test04",
        ext: false,
        n_channels: 3,
        lfe: false,
        n_ml: 0,
        min_exact: 0.98,
        label: "3/0 phantom centre, 48 kHz, dematrix '00'",
    },
    McCell {
        name: "test05",
        ext: false,
        n_channels: 3,
        lfe: false,
        n_ml: 0,
        min_exact: 0.999,
        label: "2/1 mono surround, 44,1 kHz, dematrix '11'",
    },
    McCell {
        name: "test06",
        ext: false,
        n_channels: 4,
        lfe: false,
        n_ml: 0,
        min_exact: 0.45,
        label: "2/0 + second stereo, 48 kHz",
    },
    McCell {
        name: "test07",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.45,
        label: "3/0 + second stereo, 48 kHz, extension bit stream",
    },
    McCell {
        name: "test08",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.999,
        label: "3/2, 48 kHz, dynamic crosstalk, extension bit stream",
    },
    McCell {
        name: "test09",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.30,
        label: "3/2, 48 kHz, dematrix '00', tc switching",
    },
    McCell {
        name: "test10",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 7,
        min_exact: 0.45,
        label: "3/2 + 7 multilingual @ full Fs, 48 kHz, extension bit stream",
    },
    McCell {
        name: "test11",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.99,
        label: "3/2, 44,1 kHz, dematrix '10' (phase-mixed surround)",
    },
    McCell {
        name: "test12",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 7,
        min_exact: 0.99,
        label: "3/2 + 7 multilingual @ half Fs (24 kHz), extension bit stream",
    },
    McCell {
        name: "test13",
        ext: true,
        n_channels: 5,
        lfe: true,
        n_ml: 0,
        min_exact: 1.0,
        label: "3/2 + LFE, phantom centre, prediction, dyn crosstalk, ext stream",
    },
    McCell {
        name: "test14",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.45,
        label: "3/2, 44,1 kHz, dematrix '11' (1995-era reference)",
    },
    McCell {
        name: "test15",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.999,
        label: "3/2, 48 kHz, dematrix '11'",
    },
    McCell {
        name: "test16",
        ext: false,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 1.0,
        label: "3/2, 48 kHz, dematrix '11'",
    },
    McCell {
        name: "test17",
        ext: true,
        n_channels: 5,
        lfe: true,
        n_ml: 0,
        min_exact: 0.45,
        label: "3/2 + LFE, 48 kHz, dematrix '01', extension bit stream",
    },
    McCell {
        name: "test18",
        ext: false,
        n_channels: 4,
        lfe: false,
        n_ml: 0,
        min_exact: 0.45,
        label: "3/1 mono surround, 48 kHz, dynamic crosstalk",
    },
    McCell {
        name: "test19",
        ext: false,
        n_channels: 3,
        lfe: false,
        n_ml: 0,
        min_exact: 0.45,
        label: "2/1 mono surround, 48 kHz, dynamic crosstalk",
    },
    McCell {
        name: "test20",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.98,
        label: "3/2, 44,1 kHz, VBR, extension bit stream, −6 dB sweep",
    },
    McCell {
        name: "test21",
        ext: true,
        n_channels: 5,
        lfe: false,
        n_ml: 0,
        min_exact: 0.99,
        label: "3/2, 44,1 kHz, VBR, extension bit stream",
    },
];

#[test]
fn iso13818_4_multichannel_pcm_sweep() {
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 MC sweep not run");
        return;
    };
    let mut passed = 0usize;
    for cell in MC_CELLS {
        let decoded = decode_stream(&dir, cell.name);
        assert_eq!(
            decoded.channels.len(),
            cell.n_channels,
            "{}: presentation channel count",
            cell.name
        );
        assert_eq!(
            decoded.lfe.is_some(),
            cell.lfe,
            "{}: LFE presence",
            cell.name
        );
        assert_eq!(
            decoded.mc_header.ext_bit_stream_present, cell.ext,
            "{}: extension-bit-stream presence",
            cell.name
        );
        assert_eq!(
            decoded.multilingual.len(),
            cell.n_ml,
            "{}: multilingual channel count",
            cell.name
        );

        let compare = |suffix: &str, ours: &[f64]| {
            let ref_path = stream_file(&dir, cell.name, &format!("{}_{suffix}.pcm", cell.name))
                .unwrap_or_else(|| panic!("{}: reference _{suffix}.pcm missing", cell.name));
            let reference = read_ref_s16be(&ref_path);
            assert_eq!(
                ours.len(),
                reference.len(),
                "{} {suffix}: sample count (no decoder-delay offset in this suite)",
                cell.name
            );
            let mut max_lsb = 0i32;
            let mut exact = 0usize;
            for (a, b) in ours.iter().zip(reference.iter()) {
                let d = (i32::from(to_i16(*a)) - i32::from(*b)).abs();
                max_lsb = max_lsb.max(d);
                if d == 0 {
                    exact += 1;
                }
            }
            let exact_ratio = exact as f64 / reference.len() as f64;
            eprintln!(
                "{} {suffix:>3}: max {max_lsb} LSB, {:.2}% bit-exact (floor {:.1}%) — {}",
                cell.name,
                exact_ratio * 100.0,
                cell.min_exact * 100.0,
                cell.label
            );
            assert!(
                max_lsb <= 1,
                "{} {suffix} ({}): s16 diff of {max_lsb} LSB — beyond the measured 1-LSB \
                 envelope (§2.5.4.1 max-abs bound is 2 LSB)",
                cell.name,
                cell.label
            );
            assert!(
                exact_ratio >= cell.min_exact,
                "{} {suffix} ({}): only {:.2}% bit-exact (floor {:.1}%)",
                cell.name,
                cell.label,
                exact_ratio * 100.0,
                cell.min_exact * 100.0
            );
        };

        for (label, pcm) in decoded.layout.iter().zip(&decoded.channels) {
            compare(suffix_of(*label), pcm);
        }
        if let Some(lfe) = &decoded.lfe {
            // The LFE reference of the 1995-era archives carries the
            // same ±1 LSB rounding wobble as their full-bandwidth
            // channels; the envelope bound (≤ 1 LSB) is the pin.
            let ref_path = stream_file(&dir, cell.name, &format!("{}_lfe.pcm", cell.name))
                .unwrap_or_else(|| panic!("{}: reference _lfe.pcm missing", cell.name));
            let reference = read_ref_s16be(&ref_path);
            assert_eq!(
                lfe.len(),
                reference.len(),
                "{}: LFE sample count",
                cell.name
            );
            let mut max_lsb = 0i32;
            for (a, b) in lfe.iter().zip(reference.iter()) {
                max_lsb = max_lsb.max((i32::from(to_i16(*a)) - i32::from(*b)).abs());
            }
            eprintln!(
                "{} lfe: max {max_lsb} LSB over {} samples",
                cell.name,
                lfe.len()
            );
            assert!(
                max_lsb <= 1,
                "{}: LFE exceeded the 1-LSB envelope",
                cell.name
            );
        }
        for (i, ml) in decoded.multilingual.iter().enumerate() {
            compare(&format!("m{}", i + 1), ml);
        }
        passed += 1;
    }
    assert_eq!(passed, MC_CELLS.len());
    eprintln!(
        "ISO 13818-4 MC sweep: {passed}/{} multichannel streams within the 1-LSB envelope",
        MC_CELLS.len()
    );
}

#[test]
fn iso13818_4_mc_accuracy_stream_meets_the_normative_bound_on_all_five_channels() {
    // `test35` is the §2.5.4 MPEG-1-rate accuracy bitstream (−20 dB
    // sine sweep, 24-bit references) *and* a 3/2 multichannel stream
    // (dematrix '11'): the base sweep already applies the criterion to
    // Lo/Ro; here the same normative bounds are applied to the whole
    // dematrixed presentation set.
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 MC sweep not run");
        return;
    };
    let decoded = decode_stream(&dir, "test35");
    assert_eq!(decoded.channels.len(), 5, "test35: 3/2 layout");
    assert_eq!(
        decoded.mc_header.dematrix_procedure, 3,
        "test35: dematrix '11'"
    );
    for (label, ours) in decoded.layout.iter().zip(&decoded.channels) {
        let suffix = suffix_of(*label);
        let ref_path = stream_file(&dir, "test35", &format!("test35_{suffix}.pcm")).unwrap();
        let reference = read_ref_s24be(&ref_path);
        assert_eq!(ours.len(), reference.len(), "test35 {suffix}: sample count");
        let mut max_abs = 0.0f64;
        let mut sum_sq = 0.0f64;
        for (a, b) in ours.iter().zip(reference.iter()) {
            let d = (a - b).abs();
            max_abs = max_abs.max(d);
            sum_sq += d * d;
        }
        let rms = (sum_sq / reference.len() as f64).sqrt();
        eprintln!(
            "test35 {suffix:>2}: rms {rms:.3e} (bound {RMS_BOUND:.3e}), max {max_abs:.3e} \
             (bound {MAX_ABS_BOUND:.3e})"
        );
        assert!(
            rms < RMS_BOUND,
            "test35 {suffix}: RMS {rms:.3e} exceeds the §2.5.4.1 bound"
        );
        assert!(
            max_abs <= MAX_ABS_BOUND,
            "test35 {suffix}: max {max_abs:.3e} exceeds the §2.5.4.1 bound"
        );
    }
}

#[test]
fn iso13818_4_mc_stream_premises_hold() {
    // Pin, from the wire, that the suite genuinely exercises what the
    // cell labels claim — the sweep proves nothing about features the
    // streams don't use.
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 MC sweep not run");
        return;
    };

    // All four dematrix procedures are covered.
    let proc_of = |name: &str| decode_stream(&dir, name).mc_header.dematrix_procedure;
    assert_eq!(proc_of("test02"), 0, "test02: dematrix '00'");
    assert_eq!(proc_of("test03"), 1, "test03: dematrix '01'");
    assert_eq!(proc_of("test11"), 2, "test11: dematrix '10'");
    assert_eq!(proc_of("test01"), 3, "test01: dematrix '11'");

    // Configuration coverage: centre / surround / second stereo /
    // phantom / LFE / multilingual.
    let t04 = decode_stream(&dir, "test04");
    assert_eq!(
        t04.mc_header.centre,
        Centre::Phantom,
        "test04: phantom centre"
    );
    assert_eq!(t04.config.nmch, 1, "test04: 3/0");
    let t13 = decode_stream(&dir, "test13");
    assert_eq!(
        t13.mc_header.centre,
        Centre::Phantom,
        "test13: phantom centre"
    );
    assert!(t13.mc_header.lfe, "test13: LFE");
    assert!(t13.mc_header.ext_bit_stream_present, "test13: ext stream");
    assert!(t13.prediction_frames > 0, "test13: mc_prediction used");
    assert!(t13.dyn_cross_frames > 0, "test13: dynamic crosstalk used");
    for name in ["test18", "test19"] {
        let d = decode_stream(&dir, name);
        assert_eq!(
            d.mc_header.surround,
            Surround::Mono,
            "{name}: mono surround"
        );
        assert!(d.dyn_cross_frames > 0, "{name}: dynamic crosstalk used");
    }
    let t06 = decode_stream(&dir, "test06");
    assert_eq!(
        t06.mc_header.surround,
        Surround::SecondStereo,
        "test06: second stereo"
    );
    assert_eq!(t06.config.nmch, 2, "test06: 2/0 + 2/0");
    let t07 = decode_stream(&dir, "test07");
    assert_eq!(
        t07.mc_header.surround,
        Surround::SecondStereo,
        "test07: second stereo"
    );
    assert_eq!(t07.config.nmch, 3, "test07: 3/0 + 2/0");

    // Multilingual: 7 channels at full Fs (test10) and at half Fs
    // (test12 — 24 kHz against a 48 kHz main programme, per its
    // descriptor), with the half-rate frame carrying 576 samples.
    let t10 = decode_stream(&dir, "test10");
    assert_eq!(t10.mc_header.no_of_multi_lingual_ch, 7);
    assert!(
        !t10.mc_header.multi_lingual_fs_half,
        "test10: ml at full Fs"
    );
    assert_eq!(t10.multilingual[0].len(), t10.frames * 1152);
    let t12 = decode_stream(&dir, "test12");
    assert_eq!(t12.mc_header.no_of_multi_lingual_ch, 7);
    assert!(t12.mc_header.multi_lingual_fs_half, "test12: ml at half Fs");
    assert_eq!(t12.multilingual[0].len(), t12.frames * 576);

    // The VBR pair really varies its base-frame bitrate.
    for name in ["test20", "test21"] {
        let mpg = stream_file(&dir, name, &format!("{name}.mpg")).expect("stream");
        let stream = std::fs::read(&mpg).expect("read stream");
        let mut rates = std::collections::BTreeSet::new();
        let mut offset = 0usize;
        while offset + 4 <= stream.len() {
            let Ok(h) = FrameHeader::parse(&stream[offset..]) else {
                break;
            };
            rates.insert(h.bit_rate);
            offset += h.frame_size_bytes();
        }
        assert!(
            rates.len() >= 2,
            "{name}: expected a variable-bitrate base stream, saw {rates:?}"
        );
    }

    // LFE runs at Fs/96: 12 samples per frame.
    let t01 = decode_stream(&dir, "test01");
    assert_eq!(
        t01.lfe.as_ref().map(Vec::len),
        Some(t01.frames * 12),
        "test01: LFE sample total"
    );
}

#[test]
fn iso13818_4_mc_negative_controls() {
    // Streams that must NOT decode as Layer II multichannel: the LSF
    // streams (the §2.5 extension is defined on the MPEG-1 base), and
    // the Layer III multichannel stream.
    let Some(dir) = suite_dir() else {
        eprintln!("skip: {ENV_DIR} not set — ISO 13818-4 MC sweep not run");
        return;
    };
    for name in ["test24", "test30", "test34"] {
        let mpg = stream_file(&dir, name, &format!("{name}.mpg")).expect("stream");
        let stream = std::fs::read(&mpg).expect("read stream");
        match decode_mc_stream(&stream, None) {
            Err(McError::LsfBase) => {}
            other => panic!("{name}: expected LsfBase, got {other:?}"),
        }
    }
    let mpg = stream_file(&dir, "test23", "test23.mpg").expect("stream");
    let stream = std::fs::read(&mpg).expect("read stream");
    assert!(
        matches!(decode_mc_stream(&stream, None), Err(McError::Header(_))),
        "test23 (Layer III) must be rejected at the base header"
    );
}
