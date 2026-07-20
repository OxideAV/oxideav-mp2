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

use oxideav_mp2::encoder_frame::{
    encode_all_frames, encode_all_frames_js, encode_all_frames_model2,
};
use oxideav_mp2::header::{Emphasis, FrameHeader, Mode, ModeExtension};

/// SMR source for a corpus cell (r419: the LSF rates are now
/// psychoacoustically driven, so the corpus pins reference-decodes of
/// *both* Annex D models and the Annex G.1 demand-driven joint-stereo
/// policy, not just the historical Model-1 path).
#[derive(Clone, Copy, PartialEq)]
enum Psy {
    /// §D.1 Model 1 (`encode_all_frames`).
    M1,
    /// §D.2 Model 2 (`encode_all_frames_model2`).
    M2,
    /// §D.1 Model 1 + Annex G.1 demand-driven stereo-coding selection
    /// (`encode_all_frames_js` — the header's mode/mode_extension are
    /// per-frame overridden).
    Js,
}

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
        let pcm = tone(rate, if mode == Mode::SingleChannel { 1 } else { 2 });
        emit(
            &out,
            stem,
            rate,
            kbps,
            mode,
            mode_extension,
            crc,
            Psy::M1,
            &pcm,
        );
    }

    // ── r419 cells: the LSF psychoacoustic axis ──────────────────────
    //
    // (stem, rate, kbit/s, mode, mode_extension, psy). All CRC-less;
    // the LSF rates now run the 13818-3 Annex D models, so these cells
    // pin an *independent* reference decode of psychoacoustically
    // driven LSF streams — Model 1 and Model 2, plain stereo and
    // joint-stereo intensity at several bounds, plus the Annex G.1
    // demand-driven per-frame policy and one MPEG-1 Model-2
    // joint-stereo cell (previously uncovered combination).
    #[rustfmt::skip]
    let r419: &[(&str, u32, u32, Mode, ModeExtension, Psy)] = &[
        ("psy1_16k_64",       16_000, 64,  Mode::Stereo,      ModeExtension::Bound4,  Psy::M1),
        ("psy1_22k_56",       22_050, 56,  Mode::Stereo,      ModeExtension::Bound4,  Psy::M1),
        ("psy1_24k_64",       24_000, 64,  Mode::Stereo,      ModeExtension::Bound4,  Psy::M1),
        ("psy2_16k_56",       16_000, 56,  Mode::Stereo,      ModeExtension::Bound4,  Psy::M2),
        ("psy2_24k_64",       24_000, 64,  Mode::Stereo,      ModeExtension::Bound4,  Psy::M2),
        ("psy1_js_b8_24k_64", 24_000, 64,  Mode::JointStereo, ModeExtension::Bound8,  Psy::M1),
        ("psy1_js_b12_22k_64",22_050, 64,  Mode::JointStereo, ModeExtension::Bound12, Psy::M1),
        ("psy1_js_b16_16k_64",16_000, 64,  Mode::JointStereo, ModeExtension::Bound16, Psy::M1),
        ("psy2_js_b8_44k_128",44_100, 128, Mode::JointStereo, ModeExtension::Bound8,  Psy::M2),
        ("psy1_jsauto_22k_32",22_050, 32,  Mode::Stereo,      ModeExtension::Bound4,  Psy::Js),
    ];
    for &(stem, rate, kbps, mode, mode_extension, psy) in r419 {
        let pcm = tone(rate, 2);
        emit(
            &out,
            stem,
            rate,
            kbps,
            mode,
            mode_extension,
            false,
            psy,
            &pcm,
        );
    }

    // Annex G.1 sum-signal content pin: channel 1 carries a tone at
    // 0.19·Fs (subband 12, inside every intensity region) that channel
    // 0 does not, over a shared low band. An intensity decoder that
    // mishandled the shared codeword / per-channel scalefactor split
    // would leak or lose that right-only content — the independent
    // reference decode pins the correct split.
    {
        let rate = 24_000u32;
        let n = ((rate as f64 * 0.6 / 1152.0).floor() as usize) * 1152;
        let fs = f64::from(rate);
        let tau = std::f64::consts::TAU;
        let mk = |extra: f64| -> Vec<f64> {
            (0..n)
                .map(|i| {
                    let t = i as f64 / fs;
                    (0.6 + 0.4 * (tau * 3.0 * t).sin())
                        * (0.35 * (tau * 0.013 * fs * t).sin() + 0.20 * (tau * 0.05 * fs * t).sin())
                        + extra * (tau * 0.19 * fs * t).sin()
                })
                .collect()
        };
        let pcm = vec![mk(0.0), mk(0.30)];
        emit(
            &out,
            "psy1_js_b4_24k_48_ronly",
            rate,
            48,
            Mode::JointStereo,
            ModeExtension::Bound4,
            false,
            Psy::M1,
            &pcm,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    out: &str,
    stem: &str,
    rate: u32,
    kbps: u32,
    mode: Mode,
    mode_extension: ModeExtension,
    crc: bool,
    psy: Psy,
    pcm: &[Vec<f64>],
) {
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
    let bytes = match psy {
        Psy::M1 => encode_all_frames(&header, pcm, 0),
        Psy::M2 => encode_all_frames_model2(&header, pcm, 0),
        Psy::Js => encode_all_frames_js(&header, pcm, 0),
    }
    .unwrap_or_else(|e| panic!("{stem}: encode: {e:?}"));
    let path = format!("{out}/{stem}.mp2");
    std::fs::write(&path, &bytes).expect("write .mp2");
    println!(
        "{stem}: {} bytes, {} frames",
        bytes.len(),
        pcm[0].len() / 1152
    );
}
