//! Generator for the joint-stereo / dual-channel / CRC cells of the
//! decode-conformance corpus under `tests/fixtures/`.
//!
//! The mono/stereo cells of the corpus come from black-box encoder
//! binaries (see `tests/fixtures/GENERATION.md`), but no available
//! black-box encoder emits Layer II `joint_stereo` (§2.4.1.6 intensity
//! stereo), `dual_channel`, or CRC-protected frames — so those streams
//! are produced by **this crate's own encoder** through its public
//! batch API, then reference-decoded by an *independent* black-box
//! decoder. Comparing our decode of these streams against that
//! independent decode breaks the encoder/decoder symmetry that a pure
//! round-trip test would have: a shared misreading of the §2.4.1.6
//! intensity wire syntax (or of the §2.4.1.4 CRC coverage) would show
//! up as a divergence against the reference.
//!
//! Usage:
//!
//! ```sh
//! cargo run --example gen_conformance_fixtures -- <output-dir>
//! ```
//!
//! then reference-decode each emitted `<stem>.mp2` to a float WAV with
//! an independent decoder (commands in `tests/fixtures/GENERATION.md`).
//!
//! The synthetic PCM is the same rate-relative multi-tone with slow
//! amplitude modulation as the black-box-encoded cells, so scalefactors
//! and scfsi patterns vary across frames, and components land in high
//! subbands (0.19·Fs, 0.36·Fs) to keep the §2.4.1.6 intensity region
//! genuinely populated above every bound.

use oxideav_mp2::encoder_frame::encode_all_frames;
use oxideav_mp2::header::{Emphasis, FrameHeader, Mode, ModeExtension};

/// 0.6 s of the corpus multi-tone, rounded down to whole 1152-sample
/// frames (the batch encoder rejects a partial tail by design).
fn tone(rate: u32, channels: usize) -> Vec<Vec<f64>> {
    let n_frames = (rate as f64 * 0.6 / 1152.0).floor() as usize;
    let n = n_frames * 1152;
    let fs = f64::from(rate);
    let mut planes = Vec::with_capacity(channels);
    for ch in 0..channels {
        let mut p = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / fs;
            let tau = std::f64::consts::TAU;
            let s = if ch == 0 {
                (0.6 + 0.4 * (tau * 3.0 * t).sin())
                    * (0.32 * (tau * 0.011 * fs * t).sin()
                        + 0.22 * (tau * 0.07 * fs * t).sin()
                        + 0.18 * (tau * 0.19 * fs * t).sin()
                        + 0.12 * (tau * 0.36 * fs * t).sin())
            } else {
                (0.7 + 0.3 * (tau * 5.0 * t).sin())
                    * (0.30 * (tau * 0.017 * fs * t).sin()
                        + 0.24 * (tau * 0.09 * fs * t).sin()
                        + 0.16 * (tau * 0.23 * fs * t).sin()
                        + 0.10 * (tau * 0.41 * fs * t).sin())
            };
            p.push(s);
        }
        planes.push(p);
    }
    planes
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: gen_conformance_fixtures <output-dir>");
    std::fs::create_dir_all(&out).expect("create output dir");

    // (stem, sample rate, bitrate kbit/s, mode, mode_extension, crc)
    #[rustfmt::skip]
    let cells: &[(&str, u32, u32, Mode, ModeExtension, bool)] = &[
        // Live §2.4.1.6 intensity region at every bound, wide tables:
        ("js_b4_44k_128",  44_100, 128, Mode::JointStereo, ModeExtension::Bound4,  false),
        ("js_b8_48k_192",  48_000, 192, Mode::JointStereo, ModeExtension::Bound8,  false),
        ("js_b12_32k_192", 32_000, 192, Mode::JointStereo, ModeExtension::Bound12, false),
        ("js_b16_44k_256", 44_100, 256, Mode::JointStereo, ModeExtension::Bound16, false),
        // Narrow tables: B.2d (sblimit 12) live, and the B.2c
        // bound-clamp edge (sblimit 8 ≤ bound 8 → empty region):
        ("js_b4_32k_64",   32_000, 64,  Mode::JointStereo, ModeExtension::Bound4,  false),
        ("js_b8_48k_96",   48_000, 96,  Mode::JointStereo, ModeExtension::Bound8,  false),
        // MPEG-2 LSF joint stereo (13818-3 Table B.1, sblimit 30):
        ("js_b4_22k_64",   22_050, 64,  Mode::JointStereo, ModeExtension::Bound4,  false),
        // Two independent programmes:
        ("dual_44k_128",   44_100, 128, Mode::DualChannel, ModeExtension::Bound4,  false),
        ("dual_24k_64",    24_000, 64,  Mode::DualChannel, ModeExtension::Bound4,  false),
        // §2.4.1.4 CRC-16 emitted in every frame:
        ("crc_48k_192",    48_000, 192, Mode::Stereo,      ModeExtension::Bound4,  true),
    ];

    for &(stem, rate, kbps, mode, mode_extension, crc) in cells {
        let lsf = rate < 32_000;
        let header = FrameHeader {
            lsf,
            bit_rate: kbps * 1000,
            sample_rate: rate,
            padding: false, // per-frame scheduled by encode_all_frames
            private_bit: false,
            mode,
            mode_extension,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
            protection_bit: !crc,
        };
        let channels = if mode == Mode::SingleChannel { 1 } else { 2 };
        let pcm = tone(rate, channels);
        let bytes =
            encode_all_frames(&header, &pcm, 0).unwrap_or_else(|e| panic!("{stem}: encode: {e:?}"));
        let path = format!("{out}/{stem}.mp2");
        std::fs::write(&path, &bytes).expect("write .mp2");
        println!(
            "{stem}: {} bytes, {} frames",
            bytes.len(),
            pcm[0].len() / 1152
        );
    }
}
