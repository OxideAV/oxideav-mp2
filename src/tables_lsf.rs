//! ISO/IEC 13818-3:1997 Annex D clause **D.1** — the *Layer II*
//! psychoacoustic Model-1 tables for the MPEG-2 **Lower Sampling
//! Frequencies** (16 / 22,05 / 24 kHz): Tables **D.1d / D.1e / D.1f**
//! ("Frequencies, critical band rates and absolute threshold") and
//! Tables **D.2d / D.2e / D.2f** ("Critical band boundaries").
//!
//! ISO/IEC 13818-3 carries its **own** Annex D: "A description of the
//! psychoacoustic model 1 is repeated here, with the necessary
//! adaptations in respect to the lower sampling frequencies." Its
//! Layer II tables reuse the same letters (D.1d/e/f, D.2d/e/f) as the
//! ISO/IEC 11172-3 tables transcribed in [`crate::tables_d2`], but
//! cover the LSF rates — the constant names below carry an `LSF_`
//! prefix to keep the two spec families apart. The row structures are
//! identical, so the carriers ([`LtqEntry`], [`CriticalBandBoundary`])
//! are shared with `tables_d2`.
//!
//! ## Layer II LSF subsampling map
//!
//! Per the 13818-3 §D.1 Step 6 prose ("no subsampling for the first
//! three subbands, every second spectral line for the next three,
//! every fourth for the next six, every eighth for the next 18;
//! n equals 132"), the D.1 index `i` maps to the 1024-point
//! analysis-FFT line as:
//!
//! ```text
//! i in   1..=48  ->  line = i                 (subbands  0..3, every line)
//! i in  49..=72  ->  line = 48 + 2*(i - 48)   (subbands  3..6, every 2nd)
//! i in  73..=96  ->  line = 96 + 4*(i - 72)   (subbands  6..12, every 4th)
//! i in  97..=132 ->  line = 192 + 8*(i - 96)  (subbands 12..30, every 8th)
//! ```
//!
//! topping out at line 480 (= 15/16 of the audio band; 7 500 Hz at
//! 16 kHz, 10 335,94 Hz at 22,05 kHz, 11 250 Hz at 24 kHz). All three
//! LSF tables carry exactly 132 entries; every printed `Frequency
//! [Hz]` cell equals `line * Fs/1024` to the table's two-decimal
//! print precision (enforced by test), which fixes the
//! `top_line_index` values deterministically.
//!
//! The **D.2** boundary tables print the `index of Table F&CB` column
//! — a 1-based index into the same rate's D.1 table (**not** a raw
//! FFT line); 21 critical bands at 16 kHz, 23 at 22,05 and 24 kHz,
//! matching the §D.1 Step 4(c) prose band counts exactly (unlike the
//! 11172-3 tables, which print one more boundary row than the prose
//! count). The `psy::critical_band_line_ranges` resolver applies
//! unchanged.
//!
//! ## Step 3 note — no overall-bit-rate offset at the LSF rates
//!
//! The 11172-3 §D.1 Step 3 adds a −12 dB offset to the absolute
//! threshold for bit rates ≥ 96 kbit/s per channel. The 13818-3
//! §D.1 Step 3 text ("Considering the threshold in quiet") repeats
//! the model *without* that offset sentence; the LSF driver in
//! [`crate::psy`] therefore applies a 0 dB offset at these rates, as
//! printed.
//!
//! ## Decimal-comma convention
//!
//! The spec PDF uses European decimal notation (`15,63` Hz = 15.63;
//! `0,154` = 0.154; `10 335,94` = 10335.94). The constants below
//! carry the period equivalents (idiomatic Rust `f64` literals); no
//! value has been altered.
//!
//! ## Source
//!
//! Direct transcription from the staged ISO/IEC 13818-3:1997 PDF
//! (`docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`, SHA-256
//! `25ebf438988fced761b79adcb108c0a59acc68a0f38be36017c334abb8582df5`;
//! Tables D.1a–D.1f at printed pages 91–97, Tables D.2a–D.2f at
//! printed pages 98–100). Every row was machine-validated during
//! transcription against the `line * Fs/1024` frequency grid, Bark
//! monotonicity, and the D.2 → D.1 row cross-print; the same checks
//! run as unit tests below. No third-party MP2 source was consulted.

use crate::tables_d2::{CriticalBandBoundary, LtqEntry};

/// 13818-3 Annex D Table **D.1d** — "Frequencies, critical band rates
/// and absolute threshold", Layer II, Fs = 16 kHz. 132 entries; top
/// FFT line 480 (7 500 Hz).
// One spec threshold value (6.28 dB) happens to coincide with the
// decimal expansion of TAU; it is a verbatim Annex D table cell, not a
// mathematical constant, so the approx_constant lint is suppressed.
#[allow(clippy::approx_constant)]
pub static TABLE_LSF_D_1D_LTQ_LAYER_II_16: [LtqEntry; 132] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 15.63,
        bark: 0.154,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 31.25,
        bark: 0.309,
        threshold_db: 58.23,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 46.88,
        bark: 0.463,
        threshold_db: 42.1,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 62.5,
        bark: 0.617,
        threshold_db: 33.44,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 78.13,
        bark: 0.771,
        threshold_db: 27.97,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 93.75,
        bark: 0.925,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 109.38,
        bark: 1.079,
        threshold_db: 21.36,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 125.0,
        bark: 1.232,
        threshold_db: 19.2,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 140.63,
        bark: 1.385,
        threshold_db: 17.47,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 156.25,
        bark: 1.538,
        threshold_db: 16.05,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 171.88,
        bark: 1.69,
        threshold_db: 14.87,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 187.5,
        bark: 1.842,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 203.13,
        bark: 1.994,
        threshold_db: 13.01,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 218.75,
        bark: 2.145,
        threshold_db: 12.26,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 234.38,
        bark: 2.295,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 250.0,
        bark: 2.445,
        threshold_db: 11.01,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 265.63,
        bark: 2.594,
        threshold_db: 10.49,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 281.25,
        bark: 2.742,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 296.88,
        bark: 2.89,
        threshold_db: 9.59,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 312.5,
        bark: 3.037,
        threshold_db: 9.2,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 328.13,
        bark: 3.184,
        threshold_db: 8.84,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 343.75,
        bark: 3.329,
        threshold_db: 8.52,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 359.38,
        bark: 3.474,
        threshold_db: 8.22,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 375.0,
        bark: 3.618,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 390.63,
        bark: 3.761,
        threshold_db: 7.68,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 406.25,
        bark: 3.903,
        threshold_db: 7.44,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 421.88,
        bark: 4.045,
        threshold_db: 7.22,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 437.5,
        bark: 4.185,
        threshold_db: 7.0,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 453.13,
        bark: 4.324,
        threshold_db: 6.81,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 468.75,
        bark: 4.463,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 484.38,
        bark: 4.6,
        threshold_db: 6.44,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 500.0,
        bark: 4.736,
        threshold_db: 6.28,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 515.63,
        bark: 4.872,
        threshold_db: 6.12,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 531.25,
        bark: 5.006,
        threshold_db: 5.97,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 546.88,
        bark: 5.139,
        threshold_db: 5.83,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 562.5,
        bark: 5.272,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 578.13,
        bark: 5.403,
        threshold_db: 5.57,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 593.75,
        bark: 5.533,
        threshold_db: 5.44,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 609.38,
        bark: 5.661,
        threshold_db: 5.33,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 625.0,
        bark: 5.789,
        threshold_db: 5.21,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 640.63,
        bark: 5.916,
        threshold_db: 5.1,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 656.25,
        bark: 6.041,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 671.88,
        bark: 6.166,
        threshold_db: 4.9,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 687.5,
        bark: 6.289,
        threshold_db: 4.8,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 703.13,
        bark: 6.411,
        threshold_db: 4.71,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 718.75,
        bark: 6.532,
        threshold_db: 4.62,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 734.38,
        bark: 6.651,
        threshold_db: 4.53,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 750.0,
        bark: 6.77,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 781.25,
        bark: 7.004,
        threshold_db: 4.29,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 812.5,
        bark: 7.233,
        threshold_db: 4.14,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 843.75,
        bark: 7.457,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 875.0,
        bark: 7.677,
        threshold_db: 3.86,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 906.25,
        bark: 7.892,
        threshold_db: 3.73,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 937.5,
        bark: 8.103,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 968.75,
        bark: 8.309,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 1000.0,
        bark: 8.511,
        threshold_db: 3.37,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 1031.25,
        bark: 8.708,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 1062.5,
        bark: 8.901,
        threshold_db: 3.15,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 1093.75,
        bark: 9.09,
        threshold_db: 3.04,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 1125.0,
        bark: 9.275,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 1156.25,
        bark: 9.456,
        threshold_db: 2.83,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 1187.5,
        bark: 9.632,
        threshold_db: 2.73,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 1218.75,
        bark: 9.805,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 1250.0,
        bark: 9.974,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 1281.25,
        bark: 10.139,
        threshold_db: 2.42,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 1312.5,
        bark: 10.301,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 1343.75,
        bark: 10.459,
        threshold_db: 2.22,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 1375.0,
        bark: 10.614,
        threshold_db: 2.12,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 1406.25,
        bark: 10.765,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 1437.5,
        bark: 10.913,
        threshold_db: 1.92,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 1468.75,
        bark: 11.058,
        threshold_db: 1.81,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 1500.0,
        bark: 11.199,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 1562.5,
        bark: 11.474,
        threshold_db: 1.49,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 1625.0,
        bark: 11.736,
        threshold_db: 1.27,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 1687.5,
        bark: 11.988,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 1750.0,
        bark: 12.23,
        threshold_db: 0.8,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 1812.5,
        bark: 12.461,
        threshold_db: 0.55,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 1875.0,
        bark: 12.684,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 1937.5,
        bark: 12.898,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 2000.0,
        bark: 13.104,
        threshold_db: -0.25,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 2062.5,
        bark: 13.302,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 2125.0,
        bark: 13.493,
        threshold_db: -0.83,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 2187.5,
        bark: 13.678,
        threshold_db: -1.12,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 2250.0,
        bark: 13.855,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 2312.5,
        bark: 14.027,
        threshold_db: -1.73,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 2375.0,
        bark: 14.193,
        threshold_db: -2.04,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 2437.5,
        bark: 14.354,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 2500.0,
        bark: 14.509,
        threshold_db: -2.64,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 2562.5,
        bark: 14.66,
        threshold_db: -2.93,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 2625.0,
        bark: 14.807,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 2687.5,
        bark: 14.949,
        threshold_db: -3.49,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 2750.0,
        bark: 15.087,
        threshold_db: -3.74,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 2812.5,
        bark: 15.221,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 2875.0,
        bark: 15.351,
        threshold_db: -4.2,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 2937.5,
        bark: 15.478,
        threshold_db: -4.4,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 3000.0,
        bark: 15.602,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 3125.0,
        bark: 15.841,
        threshold_db: -4.82,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 3250.0,
        bark: 16.069,
        threshold_db: -4.96,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 3375.0,
        bark: 16.287,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 3500.0,
        bark: 16.496,
        threshold_db: -4.88,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 3625.0,
        bark: 16.697,
        threshold_db: -4.66,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 3750.0,
        bark: 16.891,
        threshold_db: -4.34,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 3875.0,
        bark: 17.078,
        threshold_db: -3.93,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 4000.0,
        bark: 17.259,
        threshold_db: -3.45,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 4125.0,
        bark: 17.434,
        threshold_db: -2.93,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 4250.0,
        bark: 17.605,
        threshold_db: -2.38,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 4375.0,
        bark: 17.77,
        threshold_db: -1.83,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 4500.0,
        bark: 17.932,
        threshold_db: -1.3,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 4625.0,
        bark: 18.089,
        threshold_db: -0.8,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 4750.0,
        bark: 18.242,
        threshold_db: -0.34,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 4875.0,
        bark: 18.392,
        threshold_db: 0.07,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 5000.0,
        bark: 18.539,
        threshold_db: 0.44,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 5125.0,
        bark: 18.682,
        threshold_db: 0.76,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 5250.0,
        bark: 18.823,
        threshold_db: 1.03,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 5375.0,
        bark: 18.96,
        threshold_db: 1.26,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 5500.0,
        bark: 19.095,
        threshold_db: 1.47,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 5625.0,
        bark: 19.226,
        threshold_db: 1.64,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 5750.0,
        bark: 19.356,
        threshold_db: 1.8,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 5875.0,
        bark: 19.482,
        threshold_db: 1.94,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 6000.0,
        bark: 19.606,
        threshold_db: 2.07,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 6125.0,
        bark: 19.728,
        threshold_db: 2.19,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 6250.0,
        bark: 19.847,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 6375.0,
        bark: 19.964,
        threshold_db: 2.44,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 6500.0,
        bark: 20.079,
        threshold_db: 2.57,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 6625.0,
        bark: 20.191,
        threshold_db: 2.7,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 6750.0,
        bark: 20.3,
        threshold_db: 2.84,
    },
    LtqEntry {
        top_line_index: 440,
        frequency_hz: 6875.0,
        bark: 20.408,
        threshold_db: 2.99,
    },
    LtqEntry {
        top_line_index: 448,
        frequency_hz: 7000.0,
        bark: 20.513,
        threshold_db: 3.15,
    },
    LtqEntry {
        top_line_index: 456,
        frequency_hz: 7125.0,
        bark: 20.616,
        threshold_db: 3.31,
    },
    LtqEntry {
        top_line_index: 464,
        frequency_hz: 7250.0,
        bark: 20.717,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 472,
        frequency_hz: 7375.0,
        bark: 20.815,
        threshold_db: 3.67,
    },
    LtqEntry {
        top_line_index: 480,
        frequency_hz: 7500.0,
        bark: 20.912,
        threshold_db: 3.87,
    },
];

/// 13818-3 Annex D Table **D.1e** — "Frequencies, critical band rates
/// and absolute threshold", Layer II, Fs = 22,05 kHz. 132 entries; top
/// FFT line 480 (10 335,94 Hz).
pub static TABLE_LSF_D_1E_LTQ_LAYER_II_22K05: [LtqEntry; 132] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 21.53,
        bark: 0.213,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 43.07,
        bark: 0.425,
        threshold_db: 45.05,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 64.6,
        bark: 0.638,
        threshold_db: 32.57,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 86.13,
        bark: 0.85,
        threshold_db: 25.87,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 107.67,
        bark: 1.062,
        threshold_db: 21.63,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 129.2,
        bark: 1.273,
        threshold_db: 18.7,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 150.73,
        bark: 1.484,
        threshold_db: 16.52,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 172.27,
        bark: 1.694,
        threshold_db: 14.85,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 193.8,
        bark: 1.903,
        threshold_db: 13.51,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 215.33,
        bark: 2.112,
        threshold_db: 12.41,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 236.87,
        bark: 2.319,
        threshold_db: 11.5,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 258.4,
        bark: 2.525,
        threshold_db: 10.72,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 279.93,
        bark: 2.73,
        threshold_db: 10.05,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 301.46,
        bark: 2.934,
        threshold_db: 9.47,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 323.0,
        bark: 3.136,
        threshold_db: 8.96,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 344.53,
        bark: 3.337,
        threshold_db: 8.5,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 366.06,
        bark: 3.536,
        threshold_db: 8.1,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 387.6,
        bark: 3.733,
        threshold_db: 7.73,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 409.13,
        bark: 3.929,
        threshold_db: 7.4,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 430.66,
        bark: 4.124,
        threshold_db: 7.1,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 452.2,
        bark: 4.316,
        threshold_db: 6.82,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 473.73,
        bark: 4.507,
        threshold_db: 6.56,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 495.26,
        bark: 4.695,
        threshold_db: 6.33,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 516.8,
        bark: 4.882,
        threshold_db: 6.11,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 538.33,
        bark: 5.067,
        threshold_db: 5.91,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 559.86,
        bark: 5.249,
        threshold_db: 5.72,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 581.4,
        bark: 5.43,
        threshold_db: 5.54,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 602.93,
        bark: 5.608,
        threshold_db: 5.37,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 624.46,
        bark: 5.785,
        threshold_db: 5.22,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 646.0,
        bark: 5.959,
        threshold_db: 5.07,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 667.53,
        bark: 6.131,
        threshold_db: 4.93,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 689.06,
        bark: 6.301,
        threshold_db: 4.79,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 710.6,
        bark: 6.469,
        threshold_db: 4.67,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 732.13,
        bark: 6.634,
        threshold_db: 4.55,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 753.66,
        bark: 6.798,
        threshold_db: 4.43,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 775.2,
        bark: 6.959,
        threshold_db: 4.32,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 796.73,
        bark: 7.118,
        threshold_db: 4.21,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 818.26,
        bark: 7.274,
        threshold_db: 4.11,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 839.79,
        bark: 7.429,
        threshold_db: 4.01,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 861.33,
        bark: 7.581,
        threshold_db: 3.92,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 882.86,
        bark: 7.731,
        threshold_db: 3.83,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 904.39,
        bark: 7.879,
        threshold_db: 3.74,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 925.93,
        bark: 8.025,
        threshold_db: 3.65,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 947.46,
        bark: 8.169,
        threshold_db: 3.57,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 968.99,
        bark: 8.31,
        threshold_db: 3.48,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 990.53,
        bark: 8.45,
        threshold_db: 3.4,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 1012.06,
        bark: 8.587,
        threshold_db: 3.33,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 1033.59,
        bark: 8.723,
        threshold_db: 3.25,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 1076.66,
        bark: 8.987,
        threshold_db: 3.1,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 1119.73,
        bark: 9.244,
        threshold_db: 2.95,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 1162.79,
        bark: 9.493,
        threshold_db: 2.81,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 1205.86,
        bark: 9.734,
        threshold_db: 2.67,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 1248.93,
        bark: 9.968,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 1291.99,
        bark: 10.195,
        threshold_db: 2.39,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 1335.06,
        bark: 10.416,
        threshold_db: 2.25,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 1378.13,
        bark: 10.629,
        threshold_db: 2.11,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 1421.19,
        bark: 10.836,
        threshold_db: 1.97,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 1464.26,
        bark: 11.037,
        threshold_db: 1.83,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 1507.32,
        bark: 11.232,
        threshold_db: 1.68,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 1550.39,
        bark: 11.421,
        threshold_db: 1.53,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 1593.46,
        bark: 11.605,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 1636.52,
        bark: 11.783,
        threshold_db: 1.23,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 1679.59,
        bark: 11.957,
        threshold_db: 1.07,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 1722.66,
        bark: 12.125,
        threshold_db: 0.9,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 1765.72,
        bark: 12.289,
        threshold_db: 0.74,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 1808.79,
        bark: 12.448,
        threshold_db: 0.56,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 1851.86,
        bark: 12.603,
        threshold_db: 0.39,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 1894.92,
        bark: 12.753,
        threshold_db: 0.21,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 1937.99,
        bark: 12.9,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 1981.05,
        bark: 13.042,
        threshold_db: -0.17,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 2024.12,
        bark: 13.181,
        threshold_db: -0.36,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 2067.19,
        bark: 13.317,
        threshold_db: -0.56,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 2153.32,
        bark: 13.578,
        threshold_db: -0.96,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 2239.45,
        bark: 13.826,
        threshold_db: -1.38,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 2325.59,
        bark: 14.062,
        threshold_db: -1.79,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 2411.72,
        bark: 14.288,
        threshold_db: -2.21,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 2497.85,
        bark: 14.504,
        threshold_db: -2.63,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 2583.98,
        bark: 14.711,
        threshold_db: -3.03,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 2670.12,
        bark: 14.909,
        threshold_db: -3.41,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 2756.25,
        bark: 15.1,
        threshold_db: -3.77,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 2842.38,
        bark: 15.284,
        threshold_db: -4.09,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 2928.52,
        bark: 15.46,
        threshold_db: -4.37,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 3014.65,
        bark: 15.631,
        threshold_db: -4.6,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 3100.78,
        bark: 15.796,
        threshold_db: -4.78,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 3186.91,
        bark: 15.955,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 3273.05,
        bark: 16.11,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 3359.18,
        bark: 16.26,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 3445.31,
        bark: 16.406,
        threshold_db: -4.94,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 3531.45,
        bark: 16.547,
        threshold_db: -4.85,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 3617.58,
        bark: 16.685,
        threshold_db: -4.69,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 3703.71,
        bark: 16.82,
        threshold_db: -4.49,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 3789.84,
        bark: 16.951,
        threshold_db: -4.24,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 3875.98,
        bark: 17.079,
        threshold_db: -3.95,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 3962.11,
        bark: 17.205,
        threshold_db: -3.63,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 4048.24,
        bark: 17.327,
        threshold_db: -3.28,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 4134.38,
        bark: 17.447,
        threshold_db: -2.91,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 4306.64,
        bark: 17.68,
        threshold_db: -2.16,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 4478.91,
        bark: 17.905,
        threshold_db: -1.41,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 4651.17,
        bark: 18.121,
        threshold_db: -0.72,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 4823.44,
        bark: 18.331,
        threshold_db: -0.11,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 4995.7,
        bark: 18.534,
        threshold_db: 0.41,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 5167.97,
        bark: 18.731,
        threshold_db: 0.84,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 5340.23,
        bark: 18.922,
        threshold_db: 1.19,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 5512.5,
        bark: 19.108,
        threshold_db: 1.48,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 5684.77,
        bark: 19.289,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 5857.03,
        bark: 19.464,
        threshold_db: 1.91,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 6029.3,
        bark: 19.635,
        threshold_db: 2.09,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 6201.56,
        bark: 19.801,
        threshold_db: 2.26,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 6373.83,
        bark: 19.963,
        threshold_db: 2.43,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 6546.09,
        bark: 20.12,
        threshold_db: 2.61,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 6718.36,
        bark: 20.273,
        threshold_db: 2.8,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 6890.63,
        bark: 20.421,
        threshold_db: 3.0,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 7062.89,
        bark: 20.565,
        threshold_db: 3.22,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 7235.16,
        bark: 20.705,
        threshold_db: 3.46,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 7407.42,
        bark: 20.84,
        threshold_db: 3.71,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 7579.69,
        bark: 20.972,
        threshold_db: 3.98,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 7751.95,
        bark: 21.099,
        threshold_db: 4.28,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 7924.22,
        bark: 21.222,
        threshold_db: 4.6,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 8096.48,
        bark: 21.342,
        threshold_db: 4.94,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 8268.75,
        bark: 21.457,
        threshold_db: 5.3,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 8441.02,
        bark: 21.569,
        threshold_db: 5.69,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 8613.28,
        bark: 21.677,
        threshold_db: 6.1,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 8785.55,
        bark: 21.781,
        threshold_db: 6.54,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 8957.81,
        bark: 21.882,
        threshold_db: 7.01,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 9130.08,
        bark: 21.98,
        threshold_db: 7.5,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 9302.34,
        bark: 22.074,
        threshold_db: 8.03,
    },
    LtqEntry {
        top_line_index: 440,
        frequency_hz: 9474.61,
        bark: 22.165,
        threshold_db: 8.59,
    },
    LtqEntry {
        top_line_index: 448,
        frequency_hz: 9646.88,
        bark: 22.253,
        threshold_db: 9.18,
    },
    LtqEntry {
        top_line_index: 456,
        frequency_hz: 9819.14,
        bark: 22.338,
        threshold_db: 9.8,
    },
    LtqEntry {
        top_line_index: 464,
        frequency_hz: 9991.41,
        bark: 22.42,
        threshold_db: 10.46,
    },
    LtqEntry {
        top_line_index: 472,
        frequency_hz: 10163.67,
        bark: 22.499,
        threshold_db: 11.15,
    },
    LtqEntry {
        top_line_index: 480,
        frequency_hz: 10335.94,
        bark: 22.576,
        threshold_db: 11.88,
    },
];

/// 13818-3 Annex D Table **D.1f** — "Frequencies, critical band rates
/// and absolute threshold", Layer II, Fs = 24 kHz. 132 entries; top
/// FFT line 480 (11 250 Hz).
pub static TABLE_LSF_D_1F_LTQ_LAYER_II_24: [LtqEntry; 132] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 23.44,
        bark: 0.232,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 46.88,
        bark: 0.463,
        threshold_db: 42.1,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 70.31,
        bark: 0.694,
        threshold_db: 30.43,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 93.75,
        bark: 0.925,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 117.19,
        bark: 1.156,
        threshold_db: 20.22,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 140.63,
        bark: 1.385,
        threshold_db: 17.47,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 164.06,
        bark: 1.614,
        threshold_db: 15.44,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 187.5,
        bark: 1.842,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 210.94,
        bark: 2.069,
        threshold_db: 12.62,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 234.38,
        bark: 2.295,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 257.81,
        bark: 2.519,
        threshold_db: 10.74,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 281.25,
        bark: 2.742,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 304.69,
        bark: 2.964,
        threshold_db: 9.39,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 328.13,
        bark: 3.184,
        threshold_db: 8.84,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 351.56,
        bark: 3.402,
        threshold_db: 8.37,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 375.0,
        bark: 3.618,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 398.44,
        bark: 3.832,
        threshold_db: 7.56,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 421.88,
        bark: 4.045,
        threshold_db: 7.22,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 445.31,
        bark: 4.255,
        threshold_db: 6.9,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 468.75,
        bark: 4.463,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 492.19,
        bark: 4.668,
        threshold_db: 6.36,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 515.63,
        bark: 4.872,
        threshold_db: 6.12,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 539.06,
        bark: 5.073,
        threshold_db: 5.9,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 562.5,
        bark: 5.272,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 585.94,
        bark: 5.468,
        threshold_db: 5.5,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 609.38,
        bark: 5.661,
        threshold_db: 5.33,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 632.81,
        bark: 5.853,
        threshold_db: 5.16,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 656.25,
        bark: 6.041,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 679.69,
        bark: 6.227,
        threshold_db: 4.85,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 703.13,
        bark: 6.411,
        threshold_db: 4.71,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 726.56,
        bark: 6.592,
        threshold_db: 4.58,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 750.0,
        bark: 6.77,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 773.44,
        bark: 6.946,
        threshold_db: 4.33,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 796.88,
        bark: 7.119,
        threshold_db: 4.21,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 820.31,
        bark: 7.289,
        threshold_db: 4.1,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 843.75,
        bark: 7.457,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 867.19,
        bark: 7.622,
        threshold_db: 3.89,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 890.63,
        bark: 7.785,
        threshold_db: 3.79,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 914.06,
        bark: 7.945,
        threshold_db: 3.7,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 937.5,
        bark: 8.103,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 960.94,
        bark: 8.258,
        threshold_db: 3.51,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 984.38,
        bark: 8.41,
        threshold_db: 3.43,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 1007.81,
        bark: 8.56,
        threshold_db: 3.34,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 1031.25,
        bark: 8.708,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 1054.69,
        bark: 8.853,
        threshold_db: 3.17,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 1078.13,
        bark: 8.996,
        threshold_db: 3.09,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 1101.56,
        bark: 9.137,
        threshold_db: 3.01,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 1125.0,
        bark: 9.275,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 1171.88,
        bark: 9.544,
        threshold_db: 2.78,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 1218.75,
        bark: 9.805,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 1265.63,
        bark: 10.057,
        threshold_db: 2.47,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 1312.5,
        bark: 10.301,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 1359.38,
        bark: 10.537,
        threshold_db: 2.17,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 1406.25,
        bark: 10.765,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 1453.13,
        bark: 10.986,
        threshold_db: 1.86,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 1500.0,
        bark: 11.199,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 1546.88,
        bark: 11.406,
        threshold_db: 1.55,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 1593.75,
        bark: 11.606,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 1640.63,
        bark: 11.8,
        threshold_db: 1.21,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 1687.5,
        bark: 11.988,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 1734.38,
        bark: 12.17,
        threshold_db: 0.86,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 1781.25,
        bark: 12.347,
        threshold_db: 0.67,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 1828.13,
        bark: 12.518,
        threshold_db: 0.49,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 1875.0,
        bark: 12.684,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 1921.88,
        bark: 12.845,
        threshold_db: 0.09,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 1968.75,
        bark: 13.002,
        threshold_db: -0.11,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 2015.63,
        bark: 13.154,
        threshold_db: -0.32,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 2062.5,
        bark: 13.302,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 2109.38,
        bark: 13.446,
        threshold_db: -0.75,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 2156.25,
        bark: 13.586,
        threshold_db: -0.97,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 2203.13,
        bark: 13.723,
        threshold_db: -1.2,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 2250.0,
        bark: 13.855,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 2343.75,
        bark: 14.111,
        threshold_db: -1.88,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 2437.5,
        bark: 14.354,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 2531.25,
        bark: 14.585,
        threshold_db: -2.79,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 2625.0,
        bark: 14.807,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 2718.75,
        bark: 15.018,
        threshold_db: -3.62,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 2812.5,
        bark: 15.221,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 2906.25,
        bark: 15.415,
        threshold_db: -4.3,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 3000.0,
        bark: 15.602,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 3093.75,
        bark: 15.783,
        threshold_db: -4.77,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 3187.5,
        bark: 15.956,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 3281.25,
        bark: 16.124,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 3375.0,
        bark: 16.287,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 3468.75,
        bark: 16.445,
        threshold_db: -4.92,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 3562.5,
        bark: 16.598,
        threshold_db: -4.8,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 3656.25,
        bark: 16.746,
        threshold_db: -4.61,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 3750.0,
        bark: 16.891,
        threshold_db: -4.36,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 3843.75,
        bark: 17.032,
        threshold_db: -4.07,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 3937.5,
        bark: 17.169,
        threshold_db: -3.73,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 4031.25,
        bark: 17.303,
        threshold_db: -3.36,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 4125.0,
        bark: 17.434,
        threshold_db: -2.96,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 4218.75,
        bark: 17.563,
        threshold_db: -2.55,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 4312.5,
        bark: 17.688,
        threshold_db: -2.14,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 4406.25,
        bark: 17.811,
        threshold_db: -1.73,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 4500.0,
        bark: 17.932,
        threshold_db: -1.33,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 4687.5,
        bark: 18.166,
        threshold_db: -0.59,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 4875.0,
        bark: 18.392,
        threshold_db: 0.05,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 5062.5,
        bark: 18.611,
        threshold_db: 0.58,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 5250.0,
        bark: 18.823,
        threshold_db: 1.01,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 5437.5,
        bark: 19.028,
        threshold_db: 1.36,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 5625.0,
        bark: 19.226,
        threshold_db: 1.63,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 5812.5,
        bark: 19.419,
        threshold_db: 1.86,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 6000.0,
        bark: 19.606,
        threshold_db: 2.06,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 6187.5,
        bark: 19.788,
        threshold_db: 2.25,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 6375.0,
        bark: 19.964,
        threshold_db: 2.43,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 6562.5,
        bark: 20.135,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 6750.0,
        bark: 20.3,
        threshold_db: 2.83,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 6937.5,
        bark: 20.461,
        threshold_db: 3.06,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 7125.0,
        bark: 20.616,
        threshold_db: 3.3,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 7312.5,
        bark: 20.766,
        threshold_db: 3.57,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 7500.0,
        bark: 20.912,
        threshold_db: 3.85,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 7687.5,
        bark: 21.052,
        threshold_db: 4.16,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 7875.0,
        bark: 21.188,
        threshold_db: 4.5,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 8062.5,
        bark: 21.318,
        threshold_db: 4.86,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 8250.0,
        bark: 21.445,
        threshold_db: 5.25,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 8437.5,
        bark: 21.567,
        threshold_db: 5.67,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 8625.0,
        bark: 21.684,
        threshold_db: 6.12,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 8812.5,
        bark: 21.797,
        threshold_db: 6.61,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 9000.0,
        bark: 21.906,
        threshold_db: 7.12,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 9187.5,
        bark: 22.012,
        threshold_db: 7.67,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 9375.0,
        bark: 22.113,
        threshold_db: 8.26,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 9562.5,
        bark: 22.21,
        threshold_db: 8.88,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 9750.0,
        bark: 22.304,
        threshold_db: 9.54,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 9937.5,
        bark: 22.395,
        threshold_db: 10.24,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 10125.0,
        bark: 22.482,
        threshold_db: 10.98,
    },
    LtqEntry {
        top_line_index: 440,
        frequency_hz: 10312.5,
        bark: 22.566,
        threshold_db: 11.77,
    },
    LtqEntry {
        top_line_index: 448,
        frequency_hz: 10500.0,
        bark: 22.646,
        threshold_db: 12.6,
    },
    LtqEntry {
        top_line_index: 456,
        frequency_hz: 10687.5,
        bark: 22.724,
        threshold_db: 13.48,
    },
    LtqEntry {
        top_line_index: 464,
        frequency_hz: 10875.0,
        bark: 22.799,
        threshold_db: 14.41,
    },
    LtqEntry {
        top_line_index: 472,
        frequency_hz: 11062.5,
        bark: 22.871,
        threshold_db: 15.38,
    },
    LtqEntry {
        top_line_index: 480,
        frequency_hz: 11250.0,
        bark: 22.941,
        threshold_db: 16.41,
    },
];

/// 13818-3 Annex D Table **D.2d** — Critical band boundaries,
/// Layer II, Fs = 16 kHz. 21 bands (`no = 0..20`), matching the §D.1
/// Step 4(c) prose ("21 critical bands are used for the sampling rate
/// of 16 kHz"). `top_line_index` is the printed `index of Table F&CB`
/// — a 1-based D.1d row index, resolved to raw FFT lines by
/// `psy::critical_band_line_ranges`.
pub const TABLE_LSF_D_2D_LAYER_II_16KHZ: [CriticalBandBoundary; 21] = [
    CriticalBandBoundary {
        top_line_index: 6,
        top_frequency_hz: 93.75,
        top_bark: 0.925,
    },
    CriticalBandBoundary {
        top_line_index: 13,
        top_frequency_hz: 203.13,
        top_bark: 1.994,
    },
    CriticalBandBoundary {
        top_line_index: 20,
        top_frequency_hz: 312.5,
        top_bark: 3.037,
    },
    CriticalBandBoundary {
        top_line_index: 27,
        top_frequency_hz: 421.88,
        top_bark: 4.045,
    },
    CriticalBandBoundary {
        top_line_index: 34,
        top_frequency_hz: 531.25,
        top_bark: 5.006,
    },
    CriticalBandBoundary {
        top_line_index: 42,
        top_frequency_hz: 656.25,
        top_bark: 6.041,
    },
    CriticalBandBoundary {
        top_line_index: 49,
        top_frequency_hz: 781.25,
        top_bark: 7.004,
    },
    CriticalBandBoundary {
        top_line_index: 54,
        top_frequency_hz: 937.5,
        top_bark: 8.103,
    },
    CriticalBandBoundary {
        top_line_index: 59,
        top_frequency_hz: 1093.75,
        top_bark: 9.09,
    },
    CriticalBandBoundary {
        top_line_index: 64,
        top_frequency_hz: 1250.0,
        top_bark: 9.974,
    },
    CriticalBandBoundary {
        top_line_index: 71,
        top_frequency_hz: 1468.75,
        top_bark: 11.058,
    },
    CriticalBandBoundary {
        top_line_index: 75,
        top_frequency_hz: 1687.5,
        top_bark: 11.988,
    },
    CriticalBandBoundary {
        top_line_index: 79,
        top_frequency_hz: 1937.5,
        top_bark: 12.898,
    },
    CriticalBandBoundary {
        top_line_index: 85,
        top_frequency_hz: 2312.5,
        top_bark: 14.027,
    },
    CriticalBandBoundary {
        top_line_index: 91,
        top_frequency_hz: 2687.5,
        top_bark: 14.949,
    },
    CriticalBandBoundary {
        top_line_index: 98,
        top_frequency_hz: 3250.0,
        top_bark: 16.069,
    },
    CriticalBandBoundary {
        top_line_index: 103,
        top_frequency_hz: 3875.0,
        top_bark: 17.078,
    },
    CriticalBandBoundary {
        top_line_index: 108,
        top_frequency_hz: 4500.0,
        top_bark: 17.932,
    },
    CriticalBandBoundary {
        top_line_index: 115,
        top_frequency_hz: 5375.0,
        top_bark: 18.96,
    },
    CriticalBandBoundary {
        top_line_index: 123,
        top_frequency_hz: 6375.0,
        top_bark: 19.964,
    },
    CriticalBandBoundary {
        top_line_index: 132,
        top_frequency_hz: 7500.0,
        top_bark: 20.912,
    },
];

/// 13818-3 Annex D Table **D.2e** — Critical band boundaries,
/// Layer II, Fs = 22,05 kHz. 23 bands (`no = 0..22`), matching the
/// §D.1 Step 4(c) prose ("23 critical bands are used for 22,05 kHz
/// and 24 kHz"). `top_line_index` is the printed `index of Table
/// F&CB` — a 1-based D.1e row index.
pub const TABLE_LSF_D_2E_LAYER_II_22K05HZ: [CriticalBandBoundary; 23] = [
    CriticalBandBoundary {
        top_line_index: 5,
        top_frequency_hz: 107.67,
        top_bark: 1.062,
    },
    CriticalBandBoundary {
        top_line_index: 9,
        top_frequency_hz: 193.8,
        top_bark: 1.903,
    },
    CriticalBandBoundary {
        top_line_index: 14,
        top_frequency_hz: 301.46,
        top_bark: 2.934,
    },
    CriticalBandBoundary {
        top_line_index: 19,
        top_frequency_hz: 409.13,
        top_bark: 3.929,
    },
    CriticalBandBoundary {
        top_line_index: 25,
        top_frequency_hz: 538.33,
        top_bark: 5.067,
    },
    CriticalBandBoundary {
        top_line_index: 30,
        top_frequency_hz: 646.0,
        top_bark: 5.959,
    },
    CriticalBandBoundary {
        top_line_index: 36,
        top_frequency_hz: 775.2,
        top_bark: 6.959,
    },
    CriticalBandBoundary {
        top_line_index: 43,
        top_frequency_hz: 925.93,
        top_bark: 8.025,
    },
    CriticalBandBoundary {
        top_line_index: 49,
        top_frequency_hz: 1076.66,
        top_bark: 8.987,
    },
    CriticalBandBoundary {
        top_line_index: 53,
        top_frequency_hz: 1248.93,
        top_bark: 9.968,
    },
    CriticalBandBoundary {
        top_line_index: 58,
        top_frequency_hz: 1464.26,
        top_bark: 11.037,
    },
    CriticalBandBoundary {
        top_line_index: 63,
        top_frequency_hz: 1679.59,
        top_bark: 11.957,
    },
    CriticalBandBoundary {
        top_line_index: 70,
        top_frequency_hz: 1981.05,
        top_bark: 13.042,
    },
    CriticalBandBoundary {
        top_line_index: 75,
        top_frequency_hz: 2325.59,
        top_bark: 14.062,
    },
    CriticalBandBoundary {
        top_line_index: 79,
        top_frequency_hz: 2670.12,
        top_bark: 14.909,
    },
    CriticalBandBoundary {
        top_line_index: 85,
        top_frequency_hz: 3186.91,
        top_bark: 15.955,
    },
    CriticalBandBoundary {
        top_line_index: 92,
        top_frequency_hz: 3789.84,
        top_bark: 16.951,
    },
    CriticalBandBoundary {
        top_line_index: 98,
        top_frequency_hz: 4478.91,
        top_bark: 17.905,
    },
    CriticalBandBoundary {
        top_line_index: 103,
        top_frequency_hz: 5340.23,
        top_bark: 18.922,
    },
    CriticalBandBoundary {
        top_line_index: 109,
        top_frequency_hz: 6373.83,
        top_bark: 19.963,
    },
    CriticalBandBoundary {
        top_line_index: 116,
        top_frequency_hz: 7579.69,
        top_bark: 20.972,
    },
    CriticalBandBoundary {
        top_line_index: 125,
        top_frequency_hz: 9130.08,
        top_bark: 21.98,
    },
    CriticalBandBoundary {
        top_line_index: 132,
        top_frequency_hz: 10335.94,
        top_bark: 22.576,
    },
];

/// 13818-3 Annex D Table **D.2f** — Critical band boundaries,
/// Layer II, Fs = 24 kHz. 23 bands (`no = 0..22`), matching the §D.1
/// Step 4(c) prose. `top_line_index` is the printed `index of Table
/// F&CB` — a 1-based D.1f row index.
pub const TABLE_LSF_D_2F_LAYER_II_24KHZ: [CriticalBandBoundary; 23] = [
    CriticalBandBoundary {
        top_line_index: 4,
        top_frequency_hz: 93.75,
        top_bark: 0.925,
    },
    CriticalBandBoundary {
        top_line_index: 9,
        top_frequency_hz: 210.94,
        top_bark: 2.069,
    },
    CriticalBandBoundary {
        top_line_index: 13,
        top_frequency_hz: 304.69,
        top_bark: 2.964,
    },
    CriticalBandBoundary {
        top_line_index: 18,
        top_frequency_hz: 421.88,
        top_bark: 4.045,
    },
    CriticalBandBoundary {
        top_line_index: 23,
        top_frequency_hz: 539.06,
        top_bark: 5.073,
    },
    CriticalBandBoundary {
        top_line_index: 28,
        top_frequency_hz: 656.25,
        top_bark: 6.041,
    },
    CriticalBandBoundary {
        top_line_index: 33,
        top_frequency_hz: 773.44,
        top_bark: 6.946,
    },
    CriticalBandBoundary {
        top_line_index: 39,
        top_frequency_hz: 914.06,
        top_bark: 7.945,
    },
    CriticalBandBoundary {
        top_line_index: 46,
        top_frequency_hz: 1078.13,
        top_bark: 8.996,
    },
    CriticalBandBoundary {
        top_line_index: 51,
        top_frequency_hz: 1265.63,
        top_bark: 10.057,
    },
    CriticalBandBoundary {
        top_line_index: 55,
        top_frequency_hz: 1453.13,
        top_bark: 10.986,
    },
    CriticalBandBoundary {
        top_line_index: 60,
        top_frequency_hz: 1687.5,
        top_bark: 11.988,
    },
    CriticalBandBoundary {
        top_line_index: 66,
        top_frequency_hz: 1968.75,
        top_bark: 13.002,
    },
    CriticalBandBoundary {
        top_line_index: 73,
        top_frequency_hz: 2343.75,
        top_bark: 14.111,
    },
    CriticalBandBoundary {
        top_line_index: 77,
        top_frequency_hz: 2718.75,
        top_bark: 15.018,
    },
    CriticalBandBoundary {
        top_line_index: 82,
        top_frequency_hz: 3187.5,
        top_bark: 15.956,
    },
    CriticalBandBoundary {
        top_line_index: 89,
        top_frequency_hz: 3843.75,
        top_bark: 17.032,
    },
    CriticalBandBoundary {
        top_line_index: 96,
        top_frequency_hz: 4500.0,
        top_bark: 17.932,
    },
    CriticalBandBoundary {
        top_line_index: 101,
        top_frequency_hz: 5437.5,
        top_bark: 19.028,
    },
    CriticalBandBoundary {
        top_line_index: 106,
        top_frequency_hz: 6375.0,
        top_bark: 19.964,
    },
    CriticalBandBoundary {
        top_line_index: 113,
        top_frequency_hz: 7687.5,
        top_bark: 21.052,
    },
    CriticalBandBoundary {
        top_line_index: 121,
        top_frequency_hz: 9187.5,
        top_bark: 22.012,
    },
    CriticalBandBoundary {
        top_line_index: 132,
        top_frequency_hz: 11250.0,
        top_bark: 22.941,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The documented Layer II LSF subsampling map (module docs).
    fn line_for_index(i: u32) -> u32 {
        match i {
            1..=48 => i,
            49..=72 => 48 + 2 * (i - 48),
            73..=96 => 96 + 4 * (i - 72),
            97..=132 => 192 + 8 * (i - 96),
            _ => panic!("index {i} out of the 132-entry range"),
        }
    }

    #[test]
    fn lsf_d1_tables_have_132_entries_topping_at_line_480() {
        for table in [
            &TABLE_LSF_D_1D_LTQ_LAYER_II_16[..],
            &TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..],
            &TABLE_LSF_D_1F_LTQ_LAYER_II_24[..],
        ] {
            assert_eq!(table.len(), 132, "13818-3 Step 6: n equals 132");
            assert_eq!(table.last().unwrap().top_line_index, 480);
        }
    }

    #[test]
    fn lsf_d1_lines_follow_the_subsampling_map() {
        for table in [
            &TABLE_LSF_D_1D_LTQ_LAYER_II_16[..],
            &TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..],
            &TABLE_LSF_D_1F_LTQ_LAYER_II_24[..],
        ] {
            for (idx, entry) in table.iter().enumerate() {
                let i = (idx + 1) as u32;
                assert_eq!(
                    entry.top_line_index,
                    line_for_index(i),
                    "index {i} maps off the 1/2/4/8 subsampling grid"
                );
            }
        }
    }

    #[test]
    fn lsf_d1_frequency_column_matches_line_grid() {
        for (fs_hz, table) in [
            (16_000.0, &TABLE_LSF_D_1D_LTQ_LAYER_II_16[..]),
            (22_050.0, &TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..]),
            (24_000.0, &TABLE_LSF_D_1F_LTQ_LAYER_II_24[..]),
        ] {
            for entry in table {
                let expect = entry.top_line_index as f64 * fs_hz / 1024.0;
                assert!(
                    (entry.frequency_hz - expect).abs() < 0.011,
                    "Fs={fs_hz}: line {} printed {} Hz vs grid {} Hz",
                    entry.top_line_index,
                    entry.frequency_hz,
                    expect,
                );
            }
        }
    }

    #[test]
    fn lsf_d1_bark_column_strictly_increasing() {
        for table in [
            &TABLE_LSF_D_1D_LTQ_LAYER_II_16[..],
            &TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..],
            &TABLE_LSF_D_1F_LTQ_LAYER_II_24[..],
        ] {
            for w in table.windows(2) {
                assert!(w[0].bark < w[1].bark);
            }
        }
    }

    #[test]
    fn lsf_d2_band_counts_match_step4c_prose() {
        // "21 critical bands are used for the sampling rate of 16 kHz,
        // 23 critical bands are used for 22,05 kHz and 24 kHz."
        assert_eq!(TABLE_LSF_D_2D_LAYER_II_16KHZ.len(), 21);
        assert_eq!(TABLE_LSF_D_2E_LAYER_II_22K05HZ.len(), 23);
        assert_eq!(TABLE_LSF_D_2F_LAYER_II_24KHZ.len(), 23);
    }

    #[test]
    fn lsf_d2_boundary_rows_match_d1_entries() {
        // The printed `index of Table F&CB` indexes the same rate's
        // D.1 table: frequency and Bark must match that row exactly.
        for (boundaries, d1) in [
            (
                &TABLE_LSF_D_2D_LAYER_II_16KHZ[..],
                &TABLE_LSF_D_1D_LTQ_LAYER_II_16[..],
            ),
            (
                &TABLE_LSF_D_2E_LAYER_II_22K05HZ[..],
                &TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..],
            ),
            (
                &TABLE_LSF_D_2F_LAYER_II_24KHZ[..],
                &TABLE_LSF_D_1F_LTQ_LAYER_II_24[..],
            ),
        ] {
            for b in boundaries {
                let entry = d1[(b.top_line_index - 1) as usize];
                assert!(
                    (entry.frequency_hz - b.top_frequency_hz).abs() < 0.011,
                    "D.2 index {}: freq {} vs D.1 {}",
                    b.top_line_index,
                    b.top_frequency_hz,
                    entry.frequency_hz,
                );
                assert!(
                    (entry.bark - b.top_bark).abs() < 0.0011,
                    "D.2 index {}: bark {} vs D.1 {}",
                    b.top_line_index,
                    b.top_bark,
                    entry.bark,
                );
            }
            // Every table's top band reaches the D.1 table top (index
            // 132, line 480) — the full audio band is covered.
            assert_eq!(boundaries.last().unwrap().top_line_index, 132);
        }
    }
}
