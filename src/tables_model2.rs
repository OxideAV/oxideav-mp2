//! ISO/IEC 11172-3:1993 Annex D clause **D.2** (Psychoacoustic
//! *Model 2*) — the *calculation partition table* (Table D.3a), the
//! Model-2 *spreading function* `sprdngf(i, j)`, and the Model 1 + 2
//! *Layer I / Layer II coder partition table* (Table D.5).
//!
//! Annex D is informative; it gives two worked example psychoacoustic
//! models. Model 1 (clause D.1) groups the FFT lines into critical
//! bands (Tables D.1 / D.2, transcribed in [`crate::tables_d2`] and
//! [`crate::psy`]). Model 2 (clause D.2) instead groups the lines of
//! the 1024-point analysis FFT into **threshold-calculation
//! partitions** and convolves the partition energies with a Bark-domain
//! spreading function.
//!
//! ## Table D.3a — calculation partition table, Fs = 32 kHz
//!
//! Each row gives one threshold-calculation partition `n` (1-based)
//! with the spec column headings:
//!
//! * `ωlow` / `ωhigh` — first / last FFT line of the partition
//!   (1-based; the last partition's `ωhigh = 513` is the Nyquist line
//!   of the 1024-point analysis FFT).
//! * `bval` — the median Bark value of the partition.
//! * `minval` — the minimum masking-spread value, dB.
//! * `tmn` — the tone-masking-noise offset, dB.
//!
//! The 32 kHz table has **49 partitions** (`bmax`); the 44,1 kHz and
//! 48 kHz tables (D.3b / D.3c) have 57 and 58 partitions respectively.
//! All three are transcribed below; [`calc_partition_table_for_rate`]
//! dispatches on the [`SamplingRate`] enum.
//!
//! ## Spreading function (clause D.2.3)
//!
//! [`spreading_function`] reproduces the verbatim Model-2 spreading
//! function used to convolve partition energies across the Bark axis.
//! For a partition pair `(i, j)` with Bark values `bval_i` (the
//! partition being spread *from*) and `bval_j` (the partition being
//! spread *into*):
//!
//! ```text
//! tmpx = 1,05 * (j - i)
//! x    = 8 * min( (tmpx - 0,5)^2 - 2*(tmpx - 0,5), 0 )
//! tmpy = 15,811389 + 7,5*(tmpx + 0,474) - 17,5*sqrt(1 + (tmpx + 0,474)^2)
//! sprdngf = if tmpy < -100 { 0 } else { 10^((x + tmpy)/10) }
//! ```
//!
//! where `i = bval` of the source partition and `j = bval` of the
//! target partition (both in Bark). The envelope exponent combines
//! both the parabolic term `x` and the asymmetric Bark spreading term
//! `tmpy`.
//!
//! ## Threshold-calculation loop (clause D.2.4 steps f…n)
//!
//! After the step-(f) spreading convolution
//! ([`convolve_partition_spreading`] + [`normalize_spread_energy`] +
//! [`renormalize_unpredictability`]) the model walks the per-partition
//! threshold loop to the per-FFT-line threshold of audibility and then
//! to the per-coder-partition signal-to-mask ratio:
//!
//! * step (g) [`tonality_index`] — `tb_b = −0,299 − 0,43·ln(cb_b)`,
//!   clamped to `[0, 1]`.
//! * step (h) [`required_snr_db`] — `SNR_b = max(minval_b, tb_b·TMN_b +
//!   (1 − tb_b)·NMT_b)` with [`NMT_DB`] = 5,5 dB.
//! * step (i) [`power_ratio`] — `bc_b = 10^(−SNR_b/10)`.
//! * step (j) [`actual_energy_threshold`] — `nb_b = en_b · bc_b`.
//! * step (k) per-line spread — folded into [`line_energy_threshold`],
//!   which runs (g)…(k) for every partition of one rate and emits
//!   `nb_ω = nb_b / line_count_b` per FFT line.
//! * step (l) [`include_absolute_threshold`] — `thr_ω =
//!   max(nb_ω, absthr_ω)` (the caller supplies `absthr_ω` already in the
//!   energy domain; the Table D.4 per-line absolute thresholds are not
//!   yet transcribed into Rust — see the followup note below).
//! * step (n) [`signal_to_mask_ratio_db`] — `SMR_n = 10·log10(epart_n /
//!   npart_n)` per Table D.5 coder partition, with the narrow-band
//!   (`width = 1`) threshold-sum vs wide-band (`width = 0`) min-times-
//!   count rule.
//!
//! Step (m) (pre-echo control) is Layer III only and omitted for
//! Layers I/II per the spec.
//!
//! ## Decimal-comma convention
//!
//! The spec PDF uses European decimal notation (`0,63` Bark = 0.63;
//! `24,5` dB = 24.5 dB). Constants below carry the period equivalents
//! (idiomatic Rust `f64` literals); no value has been altered.
//!
//! ## Source
//!
//! Direct transcription from the staged ISO/IEC 11172-3:1993 PDF
//! (`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`, SHA-256
//! `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`,
//! Table D.3a at PDF page 139 / printed page 133; spreading function
//! at PDF page 129 / printed page 123; the D.2.4 step-g…n threshold
//! loop at PDF pages 131–132 / printed 125–126), cross-checked against
//! the markdown extract
//! `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`.
//!
//! ## MPEG-2 LSF (ISO/IEC 13818-3) Model-2 tables
//!
//! The 13818-3 Annex D clause **D.2** ("Psychoacoustic Model 2 for
//! Lower Sampling Frequencies") states the model "is identical to the
//! psychoacoustic model 2 as described in ISO/IEC 11172-3, with some
//! exceptions", and prints replacement partition tables for the LSF
//! rates: Tables **D.3.a / D.3.b / D.3.c** (24 / 22,05 / 16 kHz,
//! "long blocks"). Those tables use the Layer III column layout
//! (`no. / FFT-lines / minval / qthr / norm / bval`); the
//! `TABLE_LSF_D_3*` constants below carry them re-expressed in the
//! [`CalcPartition`] form the Layer I/II §D.2.4 chain consumes — see
//! the per-table docs for the exact (documented, test-pinned)
//! column derivations. The step-(l) per-line absolute thresholds for
//! the LSF rates are served from the 13818-3 D.1d/e/f tables
//! ([`lsf_abs_threshold_layer2_16`] and friends): 13818-3 prints no
//! D.4-style table, and at the MPEG-1 rates the D.4 absolute
//! thresholds match the Layer II D.1 threshold-in-quiet column
//! entry-for-entry (sole divergences: the documented last-digit
//! print errata plus the 44,1 kHz top-of-band ceiling, where D.1e
//! clamps at 68,00 dB and D.4b prints 69,13 dB) — the same data in
//! the same dB convention.

use crate::tables_d2::SamplingRate;

/// One row of an Annex D Table D.3 Model-2 *calculation partition
/// table*.
///
/// Field names reproduce the spec column headings (clause D.2). FFT
/// line indices are the published 1-based values; callers that index a
/// 0-based FFT-line buffer must subtract 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalcPartition {
    /// `ωlow` — first FFT line of the partition (1-based).
    pub omega_low: u32,
    /// `ωhigh` — last FFT line of the partition (1-based, inclusive).
    pub omega_high: u32,
    /// `bval` — median Bark value of the partition.
    pub bval: f64,
    /// `minval` — minimum masking-spread value, dB.
    pub minval: f64,
    /// `tmn` — tone-masking-noise offset, dB.
    pub tmn: f64,
}

impl CalcPartition {
    /// Number of FFT lines covered by this partition (`ωhigh − ωlow +
    /// 1`).
    #[must_use]
    pub const fn line_count(self) -> u32 {
        self.omega_high - self.omega_low + 1
    }
}

/// Annex D Table **D.3a** — Model-2 calculation partition table,
/// Fs = 32 kHz. 49 partitions (`bmax = 49`). Source: PDF page 139
/// (printed 133); the markdown extract
/// `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md` is the
/// cross-checked secondary copy.
pub const TABLE_D_3A_CALC_PARTITION_32KHZ: [CalcPartition; 49] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 1,
        bval: 0.00,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 2,
        omega_high: 4,
        bval: 0.63,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 5,
        omega_high: 7,
        bval: 1.56,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 8,
        omega_high: 10,
        bval: 2.50,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 11,
        omega_high: 13,
        bval: 3.44,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 14,
        omega_high: 16,
        bval: 4.34,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 17,
        omega_high: 19,
        bval: 5.17,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 20,
        omega_high: 22,
        bval: 5.94,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 23,
        omega_high: 25,
        bval: 6.63,
        minval: 17.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 26,
        omega_high: 28,
        bval: 7.28,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 29,
        omega_high: 31,
        bval: 7.90,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 32,
        omega_high: 34,
        bval: 8.50,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 35,
        omega_high: 37,
        bval: 9.06,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 38,
        omega_high: 41,
        bval: 9.65,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 42,
        omega_high: 45,
        bval: 10.28,
        minval: 4.4,
        tmn: 24.8,
    },
    CalcPartition {
        omega_low: 46,
        omega_high: 49,
        bval: 10.87,
        minval: 4.4,
        tmn: 25.4,
    },
    CalcPartition {
        omega_low: 50,
        omega_high: 53,
        bval: 11.41,
        minval: 4.5,
        tmn: 25.9,
    },
    CalcPartition {
        omega_low: 54,
        omega_high: 57,
        bval: 11.92,
        minval: 4.5,
        tmn: 26.4,
    },
    CalcPartition {
        omega_low: 58,
        omega_high: 61,
        bval: 12.39,
        minval: 4.5,
        tmn: 26.9,
    },
    CalcPartition {
        omega_low: 62,
        omega_high: 65,
        bval: 12.83,
        minval: 4.5,
        tmn: 27.3,
    },
    CalcPartition {
        omega_low: 66,
        omega_high: 70,
        bval: 13.29,
        minval: 4.5,
        tmn: 27.8,
    },
    CalcPartition {
        omega_low: 71,
        omega_high: 75,
        bval: 13.78,
        minval: 4.5,
        tmn: 28.3,
    },
    CalcPartition {
        omega_low: 76,
        omega_high: 81,
        bval: 14.27,
        minval: 4.5,
        tmn: 28.8,
    },
    CalcPartition {
        omega_low: 82,
        omega_high: 87,
        bval: 14.76,
        minval: 4.5,
        tmn: 29.3,
    },
    CalcPartition {
        omega_low: 88,
        omega_high: 93,
        bval: 15.22,
        minval: 4.5,
        tmn: 29.7,
    },
    CalcPartition {
        omega_low: 94,
        omega_high: 99,
        bval: 15.63,
        minval: 4.5,
        tmn: 30.1,
    },
    CalcPartition {
        omega_low: 100,
        omega_high: 106,
        bval: 16.06,
        minval: 4.5,
        tmn: 30.6,
    },
    CalcPartition {
        omega_low: 107,
        omega_high: 113,
        bval: 16.47,
        minval: 4.5,
        tmn: 31.0,
    },
    CalcPartition {
        omega_low: 114,
        omega_high: 120,
        bval: 16.86,
        minval: 4.5,
        tmn: 31.4,
    },
    CalcPartition {
        omega_low: 121,
        omega_high: 129,
        bval: 17.25,
        minval: 4.5,
        tmn: 31.8,
    },
    CalcPartition {
        omega_low: 130,
        omega_high: 138,
        bval: 17.65,
        minval: 4.5,
        tmn: 32.2,
    },
    CalcPartition {
        omega_low: 139,
        omega_high: 148,
        bval: 18.05,
        minval: 4.5,
        tmn: 32.5,
    },
    CalcPartition {
        omega_low: 149,
        omega_high: 159,
        bval: 18.42,
        minval: 4.5,
        tmn: 32.9,
    },
    CalcPartition {
        omega_low: 160,
        omega_high: 170,
        bval: 18.81,
        minval: 4.5,
        tmn: 33.3,
    },
    CalcPartition {
        omega_low: 171,
        omega_high: 183,
        bval: 19.18,
        minval: 4.5,
        tmn: 33.7,
    },
    CalcPartition {
        omega_low: 184,
        omega_high: 196,
        bval: 19.55,
        minval: 4.5,
        tmn: 34.1,
    },
    CalcPartition {
        omega_low: 197,
        omega_high: 210,
        bval: 19.93,
        minval: 4.5,
        tmn: 34.4,
    },
    CalcPartition {
        omega_low: 211,
        omega_high: 225,
        bval: 20.29,
        minval: 4.5,
        tmn: 34.8,
    },
    CalcPartition {
        omega_low: 226,
        omega_high: 240,
        bval: 20.65,
        minval: 4.5,
        tmn: 35.2,
    },
    CalcPartition {
        omega_low: 241,
        omega_high: 258,
        bval: 21.02,
        minval: 4.5,
        tmn: 35.5,
    },
    CalcPartition {
        omega_low: 259,
        omega_high: 279,
        bval: 21.38,
        minval: 4.5,
        tmn: 35.9,
    },
    CalcPartition {
        omega_low: 280,
        omega_high: 300,
        bval: 21.74,
        minval: 4.5,
        tmn: 36.2,
    },
    CalcPartition {
        omega_low: 301,
        omega_high: 326,
        bval: 22.10,
        minval: 4.5,
        tmn: 36.6,
    },
    CalcPartition {
        omega_low: 327,
        omega_high: 354,
        bval: 22.44,
        minval: 4.5,
        tmn: 36.9,
    },
    CalcPartition {
        omega_low: 355,
        omega_high: 382,
        bval: 22.79,
        minval: 4.5,
        tmn: 37.3,
    },
    CalcPartition {
        omega_low: 383,
        omega_high: 420,
        bval: 23.14,
        minval: 4.5,
        tmn: 37.6,
    },
    CalcPartition {
        omega_low: 421,
        omega_high: 458,
        bval: 23.49,
        minval: 4.5,
        tmn: 38.0,
    },
    CalcPartition {
        omega_low: 459,
        omega_high: 496,
        bval: 23.83,
        minval: 4.5,
        tmn: 38.3,
    },
    CalcPartition {
        omega_low: 497,
        omega_high: 513,
        bval: 24.07,
        minval: 4.5,
        tmn: 38.6,
    },
];

/// Annex D Table **D.3b** — Model-2 calculation partition table,
/// Fs = 44,1 kHz. 57 partitions (`bmax = 57`). The 44,1 kHz analysis
/// has finer Bark resolution at low frequency, so the first 16
/// partitions each cover a single FFT line, and the final partition
/// reaches the Nyquist line `ωhigh = 513` of the 1024-point analysis
/// FFT. Source: ISO/IEC 11172-3:1993 Annex D Table D.3b, transcribed
/// from the staged CSV
/// `docs/audio/mp3/annex-d-table-D3b-calc-partition-44k1Hz.csv`
/// (itself extracted from the PDF page following Table D.3a; the
/// markdown extract `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`
/// is the cross-checked secondary copy).
pub const TABLE_D_3B_CALC_PARTITION_44K1HZ: [CalcPartition; 57] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 1,
        bval: 0.00,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 2,
        omega_high: 2,
        bval: 0.43,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 3,
        omega_high: 3,
        bval: 0.86,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 4,
        omega_high: 4,
        bval: 1.29,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 5,
        omega_high: 5,
        bval: 1.72,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 6,
        omega_high: 6,
        bval: 2.15,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 7,
        omega_high: 7,
        bval: 2.58,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 8,
        omega_high: 8,
        bval: 3.01,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 9,
        omega_high: 9,
        bval: 3.45,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 10,
        omega_high: 10,
        bval: 3.88,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 11,
        omega_high: 11,
        bval: 4.28,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 12,
        omega_high: 12,
        bval: 4.67,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 13,
        omega_high: 13,
        bval: 5.06,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 14,
        omega_high: 14,
        bval: 5.42,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 15,
        omega_high: 15,
        bval: 5.77,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 16,
        omega_high: 16,
        bval: 6.11,
        minval: 17.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 17,
        omega_high: 19,
        bval: 6.73,
        minval: 17.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 20,
        omega_high: 22,
        bval: 7.61,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 23,
        omega_high: 25,
        bval: 8.44,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 26,
        omega_high: 28,
        bval: 9.21,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 29,
        omega_high: 31,
        bval: 9.88,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 32,
        omega_high: 34,
        bval: 10.51,
        minval: 4.4,
        tmn: 25.0,
    },
    CalcPartition {
        omega_low: 35,
        omega_high: 37,
        bval: 11.11,
        minval: 4.5,
        tmn: 25.6,
    },
    CalcPartition {
        omega_low: 38,
        omega_high: 40,
        bval: 11.65,
        minval: 4.5,
        tmn: 26.2,
    },
    CalcPartition {
        omega_low: 41,
        omega_high: 44,
        bval: 12.24,
        minval: 4.5,
        tmn: 26.7,
    },
    CalcPartition {
        omega_low: 45,
        omega_high: 48,
        bval: 12.85,
        minval: 4.5,
        tmn: 27.4,
    },
    CalcPartition {
        omega_low: 49,
        omega_high: 52,
        bval: 13.41,
        minval: 4.5,
        tmn: 27.9,
    },
    CalcPartition {
        omega_low: 53,
        omega_high: 56,
        bval: 13.94,
        minval: 4.5,
        tmn: 28.4,
    },
    CalcPartition {
        omega_low: 57,
        omega_high: 60,
        bval: 14.42,
        minval: 4.5,
        tmn: 28.9,
    },
    CalcPartition {
        omega_low: 61,
        omega_high: 64,
        bval: 14.86,
        minval: 4.5,
        tmn: 29.4,
    },
    CalcPartition {
        omega_low: 65,
        omega_high: 69,
        bval: 15.32,
        minval: 4.5,
        tmn: 29.8,
    },
    CalcPartition {
        omega_low: 70,
        omega_high: 74,
        bval: 15.79,
        minval: 4.5,
        tmn: 30.3,
    },
    CalcPartition {
        omega_low: 75,
        omega_high: 80,
        bval: 16.26,
        minval: 4.5,
        tmn: 30.8,
    },
    CalcPartition {
        omega_low: 81,
        omega_high: 86,
        bval: 16.73,
        minval: 4.5,
        tmn: 31.2,
    },
    CalcPartition {
        omega_low: 87,
        omega_high: 93,
        bval: 17.19,
        minval: 4.5,
        tmn: 31.7,
    },
    CalcPartition {
        omega_low: 94,
        omega_high: 100,
        bval: 17.62,
        minval: 4.5,
        tmn: 32.1,
    },
    CalcPartition {
        omega_low: 101,
        omega_high: 108,
        bval: 18.05,
        minval: 4.5,
        tmn: 32.5,
    },
    CalcPartition {
        omega_low: 109,
        omega_high: 116,
        bval: 18.45,
        minval: 4.5,
        tmn: 32.9,
    },
    CalcPartition {
        omega_low: 117,
        omega_high: 124,
        bval: 18.83,
        minval: 4.5,
        tmn: 33.3,
    },
    CalcPartition {
        omega_low: 125,
        omega_high: 134,
        bval: 19.21,
        minval: 4.5,
        tmn: 33.7,
    },
    CalcPartition {
        omega_low: 135,
        omega_high: 144,
        bval: 19.60,
        minval: 4.5,
        tmn: 34.1,
    },
    CalcPartition {
        omega_low: 145,
        omega_high: 155,
        bval: 20.00,
        minval: 4.5,
        tmn: 34.5,
    },
    CalcPartition {
        omega_low: 156,
        omega_high: 166,
        bval: 20.38,
        minval: 4.5,
        tmn: 34.9,
    },
    CalcPartition {
        omega_low: 167,
        omega_high: 177,
        bval: 20.74,
        minval: 4.5,
        tmn: 35.2,
    },
    CalcPartition {
        omega_low: 178,
        omega_high: 192,
        bval: 21.12,
        minval: 4.5,
        tmn: 35.6,
    },
    CalcPartition {
        omega_low: 193,
        omega_high: 207,
        bval: 21.48,
        minval: 4.5,
        tmn: 36.0,
    },
    CalcPartition {
        omega_low: 208,
        omega_high: 222,
        bval: 21.84,
        minval: 4.5,
        tmn: 36.3,
    },
    CalcPartition {
        omega_low: 223,
        omega_high: 243,
        bval: 22.20,
        minval: 4.5,
        tmn: 36.7,
    },
    CalcPartition {
        omega_low: 244,
        omega_high: 264,
        bval: 22.56,
        minval: 4.5,
        tmn: 37.1,
    },
    CalcPartition {
        omega_low: 265,
        omega_high: 286,
        bval: 22.91,
        minval: 4.5,
        tmn: 37.4,
    },
    CalcPartition {
        omega_low: 287,
        omega_high: 314,
        bval: 23.26,
        minval: 4.5,
        tmn: 37.8,
    },
    CalcPartition {
        omega_low: 315,
        omega_high: 342,
        bval: 23.60,
        minval: 4.5,
        tmn: 38.1,
    },
    CalcPartition {
        omega_low: 343,
        omega_high: 371,
        bval: 23.95,
        minval: 4.5,
        tmn: 38.4,
    },
    CalcPartition {
        omega_low: 372,
        omega_high: 401,
        bval: 24.30,
        minval: 4.5,
        tmn: 38.8,
    },
    CalcPartition {
        omega_low: 402,
        omega_high: 431,
        bval: 24.65,
        minval: 4.5,
        tmn: 39.1,
    },
    CalcPartition {
        omega_low: 432,
        omega_high: 469,
        bval: 25.00,
        minval: 4.5,
        tmn: 39.5,
    },
    CalcPartition {
        omega_low: 470,
        omega_high: 513,
        bval: 25.33,
        minval: 3.5,
        tmn: 39.8,
    },
];

/// Annex D Table **D.3c** — Model-2 calculation partition table,
/// Fs = 48 kHz. 58 partitions (`bmax = 58`). Like D.3b the first 17
/// partitions cover single FFT lines; the final partition reaches the
/// Nyquist line `ωhigh = 513`. Source: ISO/IEC 11172-3:1993 Annex D
/// Table D.3c, transcribed from the staged CSV
/// `docs/audio/mp3/annex-d-table-D3c-calc-partition-48kHz.csv`,
/// cross-checked against
/// `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`.
pub const TABLE_D_3C_CALC_PARTITION_48KHZ: [CalcPartition; 58] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 1,
        bval: 0.00,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 2,
        omega_high: 2,
        bval: 0.47,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 3,
        omega_high: 3,
        bval: 0.94,
        minval: 0.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 4,
        omega_high: 4,
        bval: 1.41,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 5,
        omega_high: 5,
        bval: 1.88,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 6,
        omega_high: 6,
        bval: 2.34,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 7,
        omega_high: 7,
        bval: 2.81,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 8,
        omega_high: 8,
        bval: 3.28,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 9,
        omega_high: 9,
        bval: 3.75,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 10,
        omega_high: 10,
        bval: 4.20,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 11,
        omega_high: 11,
        bval: 4.63,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 12,
        omega_high: 12,
        bval: 5.05,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 13,
        omega_high: 13,
        bval: 5.44,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 14,
        omega_high: 14,
        bval: 5.83,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 15,
        omega_high: 15,
        bval: 6.19,
        minval: 20.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 16,
        omega_high: 16,
        bval: 6.52,
        minval: 17.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 17,
        omega_high: 17,
        bval: 6.86,
        minval: 17.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 18,
        omega_high: 20,
        bval: 7.49,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 21,
        omega_high: 23,
        bval: 8.40,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 24,
        omega_high: 26,
        bval: 9.24,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 27,
        omega_high: 29,
        bval: 9.97,
        minval: 7.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 30,
        omega_high: 32,
        bval: 10.65,
        minval: 4.4,
        tmn: 25.1,
    },
    CalcPartition {
        omega_low: 33,
        omega_high: 35,
        bval: 11.28,
        minval: 4.5,
        tmn: 25.8,
    },
    CalcPartition {
        omega_low: 36,
        omega_high: 38,
        bval: 11.86,
        minval: 4.5,
        tmn: 26.4,
    },
    CalcPartition {
        omega_low: 39,
        omega_high: 41,
        bval: 12.39,
        minval: 4.5,
        tmn: 26.9,
    },
    CalcPartition {
        omega_low: 42,
        omega_high: 45,
        bval: 12.96,
        minval: 4.5,
        tmn: 27.5,
    },
    CalcPartition {
        omega_low: 46,
        omega_high: 49,
        bval: 13.56,
        minval: 4.5,
        tmn: 28.1,
    },
    CalcPartition {
        omega_low: 50,
        omega_high: 53,
        bval: 14.12,
        minval: 4.5,
        tmn: 28.6,
    },
    CalcPartition {
        omega_low: 54,
        omega_high: 57,
        bval: 14.62,
        minval: 4.5,
        tmn: 29.1,
    },
    CalcPartition {
        omega_low: 58,
        omega_high: 62,
        bval: 15.14,
        minval: 4.5,
        tmn: 29.6,
    },
    CalcPartition {
        omega_low: 63,
        omega_high: 67,
        bval: 15.67,
        minval: 4.5,
        tmn: 30.2,
    },
    CalcPartition {
        omega_low: 68,
        omega_high: 72,
        bval: 16.15,
        minval: 4.5,
        tmn: 30.7,
    },
    CalcPartition {
        omega_low: 73,
        omega_high: 77,
        bval: 16.58,
        minval: 4.5,
        tmn: 31.1,
    },
    CalcPartition {
        omega_low: 78,
        omega_high: 83,
        bval: 17.02,
        minval: 4.5,
        tmn: 31.5,
    },
    CalcPartition {
        omega_low: 84,
        omega_high: 89,
        bval: 17.44,
        minval: 4.5,
        tmn: 31.9,
    },
    CalcPartition {
        omega_low: 90,
        omega_high: 95,
        bval: 17.84,
        minval: 4.5,
        tmn: 32.3,
    },
    CalcPartition {
        omega_low: 96,
        omega_high: 103,
        bval: 18.24,
        minval: 4.5,
        tmn: 32.7,
    },
    CalcPartition {
        omega_low: 104,
        omega_high: 111,
        bval: 18.66,
        minval: 4.5,
        tmn: 33.2,
    },
    CalcPartition {
        omega_low: 112,
        omega_high: 120,
        bval: 19.07,
        minval: 4.5,
        tmn: 33.6,
    },
    CalcPartition {
        omega_low: 121,
        omega_high: 129,
        bval: 19.47,
        minval: 4.5,
        tmn: 34.0,
    },
    CalcPartition {
        omega_low: 130,
        omega_high: 138,
        bval: 19.85,
        minval: 4.5,
        tmn: 34.3,
    },
    CalcPartition {
        omega_low: 139,
        omega_high: 149,
        bval: 20.23,
        minval: 4.5,
        tmn: 34.7,
    },
    CalcPartition {
        omega_low: 150,
        omega_high: 160,
        bval: 20.63,
        minval: 4.5,
        tmn: 35.1,
    },
    CalcPartition {
        omega_low: 161,
        omega_high: 173,
        bval: 21.02,
        minval: 4.5,
        tmn: 35.5,
    },
    CalcPartition {
        omega_low: 174,
        omega_high: 187,
        bval: 21.40,
        minval: 4.5,
        tmn: 35.9,
    },
    CalcPartition {
        omega_low: 188,
        omega_high: 201,
        bval: 21.76,
        minval: 4.5,
        tmn: 36.3,
    },
    CalcPartition {
        omega_low: 202,
        omega_high: 219,
        bval: 22.12,
        minval: 4.5,
        tmn: 36.6,
    },
    CalcPartition {
        omega_low: 220,
        omega_high: 238,
        bval: 22.47,
        minval: 4.5,
        tmn: 37.0,
    },
    CalcPartition {
        omega_low: 239,
        omega_high: 257,
        bval: 22.83,
        minval: 4.5,
        tmn: 37.3,
    },
    CalcPartition {
        omega_low: 258,
        omega_high: 283,
        bval: 23.18,
        minval: 4.5,
        tmn: 37.7,
    },
    CalcPartition {
        omega_low: 284,
        omega_high: 309,
        bval: 23.53,
        minval: 4.5,
        tmn: 38.0,
    },
    CalcPartition {
        omega_low: 310,
        omega_high: 335,
        bval: 23.88,
        minval: 4.5,
        tmn: 38.4,
    },
    CalcPartition {
        omega_low: 336,
        omega_high: 363,
        bval: 24.23,
        minval: 4.5,
        tmn: 38.7,
    },
    CalcPartition {
        omega_low: 364,
        omega_high: 391,
        bval: 24.58,
        minval: 4.5,
        tmn: 39.1,
    },
    CalcPartition {
        omega_low: 392,
        omega_high: 423,
        bval: 24.93,
        minval: 4.5,
        tmn: 39.4,
    },
    CalcPartition {
        omega_low: 424,
        omega_high: 465,
        bval: 25.27,
        minval: 4.5,
        tmn: 39.8,
    },
    CalcPartition {
        omega_low: 466,
        omega_high: 507,
        bval: 25.61,
        minval: 3.5,
        tmn: 40.1,
    },
    CalcPartition {
        omega_low: 508,
        omega_high: 513,
        bval: 25.81,
        minval: 3.5,
        tmn: 40.3,
    },
];

/// Returns the Model-2 calculation-partition table (Annex D Table
/// D.3a / D.3b / D.3c) for the given sampling rate.
///
/// This is the rate dispatcher for the clause D.2.4 threshold loop:
/// the step-(f) spreading convolution
/// ([`convolve_partition_spreading`]) and the per-partition threshold
/// functions all take a `&[CalcPartition]`, so a caller wires the
/// whole Model-2 chain for an arbitrary Layer II sampling rate by
/// fetching the table here once and threading it through.
///
/// The three Layer II sampling rates have **49 / 57 / 58** partitions
/// respectively (the `bmax` of each table); all three terminate at the
/// Nyquist line `ωhigh = 513` of the 1024-point analysis FFT.
#[must_use]
pub fn calc_partition_table_for_rate(rate: SamplingRate) -> &'static [CalcPartition] {
    match rate {
        SamplingRate::Fs32kHz => &TABLE_D_3A_CALC_PARTITION_32KHZ,
        SamplingRate::Fs44k1Hz => &TABLE_D_3B_CALC_PARTITION_44K1HZ,
        SamplingRate::Fs48kHz => &TABLE_D_3C_CALC_PARTITION_48KHZ,
    }
}

/// Annex D clause **D.2.3** Model-2 spreading function `sprdngf(i, j)`.
///
/// Convolves partition energies across the Bark axis. `bval_from` is
/// the Bark value of the source partition `i` (the partition whose
/// energy is being spread) and `bval_into` is the Bark value of the
/// target partition `j` (the partition being spread into). Verbatim
/// from PDF page 129 (printed 123):
///
/// ```text
/// tmpx = 1,05 * (j - i)
/// x    = 8 * min( (tmpx - 0,5)^2 - 2*(tmpx - 0,5), 0 )
/// tmpy = 15,811389 + 7,5*(tmpx + 0,474) - 17,5*sqrt(1 + (tmpx + 0,474)^2)
/// if tmpy < -100 { sprdngf = 0 } else { sprdngf = 10^((x + tmpy)/10) }
/// ```
///
/// The envelope exponent `(x + tmpy)` combines the parabolic component
/// `x` with the asymmetric Bark spreading term `tmpy`.
#[must_use]
pub fn spreading_function(bval_from: f64, bval_into: f64) -> f64 {
    // i = bval of the source partition, j = bval of the target.
    let tmpx = 1.05 * (bval_into - bval_from);
    let x = 8.0 * ((tmpx - 0.5) * (tmpx - 0.5) - 2.0 * (tmpx - 0.5)).min(0.0);
    let tmpy =
        15.811389 + 7.5 * (tmpx + 0.474) - 17.5 * (1.0 + (tmpx + 0.474) * (tmpx + 0.474)).sqrt();
    if tmpy < -100.0 {
        0.0
    } else {
        10.0_f64.powf((x + tmpy) / 10.0)
    }
}

/// Annex D clause **D.2.4 step (f)** Model-2 *partition-domain
/// spreading convolution*.
///
/// Convolves a per-calculation-partition quantity with the clause
/// D.2.3 [`spreading_function`] across the Bark axis. Verbatim from
/// PDF page 130 (printed 124):
///
/// ```text
/// ecb_b = Σ_{bb=1..bmax}  e_bb       · sprdngf(bval_bb, bval_b)
/// cf_b  = Σ_{bb=1..bmax}  e_bb·c_bb  · sprdngf(bval_bb, bval_b)
/// ```
///
/// `quantity[bb]` is the source value for partition `bb` (the partition
/// energy `e_bb` for the `ecb` convolution, or the energy-weighted
/// unpredictability product `e_bb·c_bb` for the `cf` convolution — the
/// spec performs the identical convolution on both, so a single
/// primitive serves). `table` supplies the per-partition Bark values
/// `bval` (the calculation-partition table for the active sampling
/// rate, e.g. [`TABLE_D_3A_CALC_PARTITION_32KHZ`]).
///
/// The returned vector is indexed by the same 0-based partition index
/// as `table`; entry `b` is the convolution result for target
/// partition `b`. A length mismatch between `quantity` and `table` is a
/// caller error and yields an empty vector as the documented safe
/// response.
///
/// The spec's `bb=1..bmax` summation is over every partition (the
/// "Partition numbering starts at 1" convention of clause D.2.2); this
/// 0-based implementation sums over every row of `table`.
#[must_use]
pub fn convolve_partition_spreading(table: &[CalcPartition], quantity: &[f64]) -> Vec<f64> {
    if quantity.len() != table.len() {
        return Vec::new();
    }
    table
        .iter()
        .map(|target| {
            table
                .iter()
                .zip(quantity)
                .map(|(source, &q)| q * spreading_function(source.bval, target.bval))
                .sum()
        })
        .collect()
}

/// Annex D clause **D.2.4 step (f)** Model-2 spreading-function
/// *normalization coefficient* `rnorm_b`.
///
/// Because the [`spreading_function`] is not normalized, the spec
/// renormalizes the spread energy. Verbatim from PDF page 131
/// (printed 125):
///
/// ```text
/// rnorm_b = 1 / Σ_{bb=1..bmax} sprdngf(bval_bb, bval_b)
/// ```
///
/// `b` is the 0-based target-partition index into `table`. Returns the
/// reciprocal of the spreading row-sum into partition `b`; returns
/// `None` for `b` out of range. The row-sum is strictly positive for
/// every partition (the self-spread `sprdngf(bval_b, bval_b)` is
/// always > 0), so the reciprocal is always finite.
#[must_use]
pub fn rnorm_coefficient(table: &[CalcPartition], b: usize) -> Option<f64> {
    let target = table.get(b)?;
    let row_sum: f64 = table
        .iter()
        .map(|source| spreading_function(source.bval, target.bval))
        .sum();
    Some(1.0 / row_sum)
}

/// Annex D clause **D.2.4 step (f)** Model-2 *spread-energy
/// normalization*.
///
/// Applies the [`rnorm_coefficient`] to the convolved partition energy
/// `ecb` to produce the normalized energy `en`. Verbatim from PDF
/// page 131 (printed 125):
///
/// ```text
/// en_b = ecb_b · rnorm_b
/// ```
///
/// `ecb` is the output of [`convolve_partition_spreading`] applied to
/// the partition energies; `table` supplies the Bark values that
/// determine each partition's `rnorm`. The returned vector is indexed
/// by the same 0-based partition index. A length mismatch between `ecb`
/// and `table` yields an empty vector as the documented safe response.
#[must_use]
pub fn normalize_spread_energy(table: &[CalcPartition], ecb: &[f64]) -> Vec<f64> {
    if ecb.len() != table.len() {
        return Vec::new();
    }
    ecb.iter()
        .enumerate()
        .map(|(b, &e)| e * rnorm_coefficient(table, b).unwrap_or(0.0))
        .collect()
}

/// Annex D clause **D.2.4 step (f)** Model-2 *unpredictability
/// renormalization* `cb_b`.
///
/// The convolved unpredictability `cf` is weighted by the signal
/// energy, so the spec renormalizes it to the convolved energy `ecb`.
/// Verbatim from PDF page 130–131 (printed 124–125):
///
/// ```text
/// cb_b = cf_b / ecb_b
/// ```
///
/// `cf` is [`convolve_partition_spreading`] applied to the
/// energy-weighted unpredictability product `e·c`; `ecb` is the same
/// convolution applied to the partition energy `e`. Both are indexed by
/// the 0-based partition index. A partition whose convolved energy
/// `ecb_b` is zero (a silent partition) yields `cb_b = 0.0` — the
/// documented safe response, since no energy means no defined
/// unpredictability. A length mismatch between `cf` and `ecb` yields an
/// empty vector.
#[must_use]
pub fn renormalize_unpredictability(cf: &[f64], ecb: &[f64]) -> Vec<f64> {
    if cf.len() != ecb.len() {
        return Vec::new();
    }
    cf.iter()
        .zip(ecb)
        .map(|(&c, &e)| if e == 0.0 { 0.0 } else { c / e })
        .collect()
}

/// Annex D clause **D.2.4 step (h)** Model-2 *noise-masking-tone*
/// offset `NMT`, in dB.
///
/// Verbatim from PDF page 131 (printed 125): "`NMT_b = 5,5 dB` for all
/// `b`. `NMT_b` is the value for noise masking tone (in dB) for the
/// partition." The spec gives a single rate-independent constant, so it
/// is a `const` rather than a per-partition column.
pub const NMT_DB: f64 = 5.5;

/// Annex D clause **D.2.4 step (g)** Model-2 *tonality-index* conversion
/// `tb_b`.
///
/// Converts the renormalized convolved unpredictability `cb_b` (output
/// of [`renormalize_unpredictability`]) into the partition tonality
/// index. Verbatim from PDF page 131 (printed 125):
///
/// ```text
/// tb_b = -0,299 - 0,43 · log_e(cb_b)
/// ```
///
/// with the spec's clamp "`Each tb_b is limited to the range of
/// 0 < tb_b < 1`" applied: `tb_b ∈ [0, 1]`. A masker that is perfectly
/// predictable (`cb_b → 0`, very tonal) drives the raw expression to
/// `+∞`, clamped to 1; a fully unpredictable partition (`cb_b ≥ 1`,
/// noise-like) drives it negative, clamped to 0. A non-positive `cb_b`
/// (a silent partition, where `cb_b = 0` per
/// [`renormalize_unpredictability`]) has no defined logarithm and is
/// treated as maximally tonal (`tb_b = 1`) — the documented safe
/// response, matching the "very tonal" limit of `cb_b → 0⁺`.
#[must_use]
pub fn tonality_index(cb_b: f64) -> f64 {
    if cb_b <= 0.0 {
        return 1.0;
    }
    let tb = -0.299 - 0.43 * cb_b.ln();
    tb.clamp(0.0, 1.0)
}

/// Annex D clause **D.2.4 step (h)** Model-2 *required signal-to-noise
/// ratio* `SNR_b`, in dB.
///
/// Verbatim from PDF page 131 (printed 125):
///
/// ```text
/// SNR_b = maximum( minval_b, tb_b · TMN_b + (1 - tb_b) · NMT_b )
/// ```
///
/// where "`maximum(a, b)` is a function returning the least negative of
/// `a` or `b`" (the spec's wording, i.e. the ordinary numeric maximum).
/// `tb_b` is the [`tonality_index`]; `TMN_b` (tone-masking-noise) and
/// `minval_b` (the stereo-unmasking SNR floor) are the per-partition
/// columns of the calculation-partition table ([`CalcPartition::tmn`] /
/// [`CalcPartition::minval`]); [`NMT_DB`] is the noise-masking-tone
/// constant. A purely tonal partition (`tb_b = 1`) requires `TMN_b` dB
/// of SNR; a purely noise-like partition (`tb_b = 0`) requires `NMT_b`
/// dB; the `minval_b` floor overrides both when larger.
#[must_use]
pub fn required_snr_db(tb_b: f64, partition: &CalcPartition) -> f64 {
    let masking = tb_b * partition.tmn + (1.0 - tb_b) * NMT_DB;
    masking.max(partition.minval)
}

/// Annex D clause **D.2.4 step (i)** Model-2 *power ratio* `bc_b`.
///
/// Verbatim from PDF page 131 (printed 125):
///
/// ```text
/// bc_b = 10^( -SNR_b / 10 )
/// ```
///
/// Converts the required SNR (dB, from [`required_snr_db`]) into the
/// linear power ratio applied to the normalized partition energy in
/// step (j). The result is in `(0, 1]` for non-negative SNR.
#[must_use]
pub fn power_ratio(snr_b_db: f64) -> f64 {
    10.0_f64.powf(-snr_b_db / 10.0)
}

/// Annex D clause **D.2.4 step (j)** Model-2 *actual energy threshold*
/// per partition `nb_b`.
///
/// Verbatim from PDF page 131 (printed 125):
///
/// ```text
/// nb_b = en_b · bc_b
/// ```
///
/// `en_b` is the normalized spread energy (output of
/// [`normalize_spread_energy`]); `bc_b` is the [`power_ratio`]. The
/// product is the masked-threshold energy carried by calculation
/// partition `b`.
#[must_use]
pub fn actual_energy_threshold(en_b: f64, bc_b: f64) -> f64 {
    en_b * bc_b
}

/// Annex D clause **D.2.4 steps (g)…(k)** Model-2 *per-partition
/// threshold loop*, producing the per-FFT-line energy threshold `nb_ω`.
///
/// Runs the threshold-calculation steps that follow the step-(f)
/// spreading convolution, for every calculation partition of one
/// sampling rate, and spreads each partition's threshold energy
/// uniformly over its FFT lines (step k). For each partition `b`:
///
/// ```text
/// (g) tb_b  = tonality_index(cb_b)
/// (h) SNR_b = maximum(minval_b, tb_b·TMN_b + (1 - tb_b)·NMT_b)
/// (i) bc_b  = 10^(-SNR_b / 10)
/// (j) nb_b  = en_b · bc_b
/// (k) nb_ω  = nb_b / (ωhigh_b - ωlow_b + 1)   for every line ω in b
/// ```
///
/// `table` is the calculation-partition table for the active rate (e.g.
/// [`TABLE_D_3A_CALC_PARTITION_32KHZ`]). `en` is the normalized spread
/// energy per partition (step f); `cb` is the renormalized convolved
/// unpredictability per partition (step f). Both are indexed by the same
/// 0-based partition index as `table`.
///
/// The returned vector is indexed by **0-based FFT line** `ω - 1`,
/// length `ωhigh` of the last partition (513 for the 1024-point analysis
/// FFT). Every line of partition `b` receives the same `nb_ω = nb_b /
/// line_count_b`, per the step-(k) uniform spread.
///
/// A length mismatch among `en`, `cb`, and `table` is a caller error and
/// yields an empty vector as the documented safe response.
#[must_use]
pub fn line_energy_threshold(table: &[CalcPartition], en: &[f64], cb: &[f64]) -> Vec<f64> {
    if en.len() != table.len() || cb.len() != table.len() {
        return Vec::new();
    }
    let line_count = table.last().map_or(0, |p| p.omega_high as usize);
    let mut nb_omega = vec![0.0_f64; line_count];
    for (b, part) in table.iter().enumerate() {
        let tb = tonality_index(cb[b]);
        let snr = required_snr_db(tb, part);
        let bc = power_ratio(snr);
        let nb_b = actual_energy_threshold(en[b], bc);
        let lines = part.line_count();
        if lines == 0 {
            continue;
        }
        let per_line = nb_b / f64::from(lines);
        // ωlow / ωhigh are 1-based, inclusive; map to the 0-based buffer.
        for omega in part.omega_low..=part.omega_high {
            nb_omega[(omega - 1) as usize] = per_line;
        }
    }
    nb_omega
}

/// Annex D clause **D.2.4 step (l)** Model-2 *threshold-of-audibility*
/// floor, `thr_ω = max(nb_ω, absthr_ω)`.
///
/// Verbatim from PDF page 131 (printed 125): "Include absolute
/// thresholds, yielding the final energy threshold of audibility
/// `thr_ω = max(nb_ω, absthr_ω)`." `nb_omega` is the per-FFT-line
/// threshold energy from [`line_energy_threshold`]; `absthr_omega` is
/// the per-FFT-line absolute threshold (the Table D.4 *absolute
/// threshold table* values, already converted into the same energy
/// domain as `nb_ω` by the caller — the spec's note that the dB values
/// "must be converted into the energy domain after considering the FFT
/// normalization actually used" is the caller's responsibility). Both
/// are indexed by the same 0-based FFT line. A length mismatch yields an
/// empty vector.
#[must_use]
pub fn include_absolute_threshold(nb_omega: &[f64], absthr_omega: &[f64]) -> Vec<f64> {
    if nb_omega.len() != absthr_omega.len() {
        return Vec::new();
    }
    nb_omega
        .iter()
        .zip(absthr_omega)
        .map(|(&nb, &abs)| nb.max(abs))
        .collect()
}

/// One row of Annex D Table **D.5** — the Model 1 + Model 2 *Layer I
/// and Layer II coder partition table*.
///
/// Clause D.2 (printed page 138) describes the columns as: "1. the
/// index `n` of the coder partition; 2. the lower index `ωlow_{n+1}`
/// (equivalently the upper index `ωhigh_n`) — the FFT-line boundary
/// of the partition; 3. `width_n`." The boundary column is shared
/// between consecutive partitions: the value listed for index `n` is
/// both `ωhigh_n` (the last FFT line of coder partition `n`) and
/// `ωlow_{n+1}` (one past the start of coder partition `n + 1`).
///
/// FFT-line indices are the published 1-based values; callers indexing
/// a 0-based FFT-line buffer must subtract 1.
///
/// The table is common to **all three sampling rates** (32 / 44,1 /
/// 48 kHz) and to **both Layer I and Layer II** — unlike the
/// calculation-partition table (D.3a–c), which is per-rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoderPartition {
    /// `ωhigh_n` / `ωlow_{n+1}` — the partition boundary FFT line
    /// (1-based). It is the last FFT line of coder partition `n` and
    /// the line one before the start of coder partition `n + 1`.
    pub boundary: u32,
    /// `width_n` — the spec's per-partition width flag (`0` for the
    /// low-frequency partitions `n ≤ 12`, `1` from `n = 13` onward).
    pub width: u32,
}

/// Annex D Table **D.5** — Layer I / Layer II coder partition table.
///
/// 33 rows indexed by coder partition `n = 0..=32`. Source: PDF page
/// 145 (printed 139); cross-checked against the markdown extract
/// `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`.
///
/// The `boundary` column rises in steps of 16 FFT lines: partition 0
/// ends at line 1, partition 1 at line 17, …, partition 32 at line
/// 513 (the Nyquist line of the 1024-point analysis FFT). Coder
/// partition `n` (for `n ≥ 1`) therefore covers FFT lines
/// `boundary(n-1) + 1 ..= boundary(n)`; partition 0 is the single DC
/// line 1.
pub const TABLE_D_5_CODER_PARTITION: [CoderPartition; 33] = [
    CoderPartition {
        boundary: 1,
        width: 0,
    },
    CoderPartition {
        boundary: 17,
        width: 0,
    },
    CoderPartition {
        boundary: 33,
        width: 0,
    },
    CoderPartition {
        boundary: 49,
        width: 0,
    },
    CoderPartition {
        boundary: 65,
        width: 0,
    },
    CoderPartition {
        boundary: 81,
        width: 0,
    },
    CoderPartition {
        boundary: 97,
        width: 0,
    },
    CoderPartition {
        boundary: 113,
        width: 0,
    },
    CoderPartition {
        boundary: 129,
        width: 0,
    },
    CoderPartition {
        boundary: 145,
        width: 0,
    },
    CoderPartition {
        boundary: 161,
        width: 0,
    },
    CoderPartition {
        boundary: 177,
        width: 0,
    },
    CoderPartition {
        boundary: 193,
        width: 0,
    },
    CoderPartition {
        boundary: 209,
        width: 1,
    },
    CoderPartition {
        boundary: 225,
        width: 1,
    },
    CoderPartition {
        boundary: 241,
        width: 1,
    },
    CoderPartition {
        boundary: 257,
        width: 1,
    },
    CoderPartition {
        boundary: 273,
        width: 1,
    },
    CoderPartition {
        boundary: 289,
        width: 1,
    },
    CoderPartition {
        boundary: 305,
        width: 1,
    },
    CoderPartition {
        boundary: 321,
        width: 1,
    },
    CoderPartition {
        boundary: 337,
        width: 1,
    },
    CoderPartition {
        boundary: 353,
        width: 1,
    },
    CoderPartition {
        boundary: 369,
        width: 1,
    },
    CoderPartition {
        boundary: 385,
        width: 1,
    },
    CoderPartition {
        boundary: 401,
        width: 1,
    },
    CoderPartition {
        boundary: 417,
        width: 1,
    },
    CoderPartition {
        boundary: 433,
        width: 1,
    },
    CoderPartition {
        boundary: 449,
        width: 1,
    },
    CoderPartition {
        boundary: 465,
        width: 1,
    },
    CoderPartition {
        boundary: 481,
        width: 1,
    },
    CoderPartition {
        boundary: 497,
        width: 1,
    },
    CoderPartition {
        boundary: 513,
        width: 1,
    },
];

/// The number of coder partitions in Table D.5 (`n = 0..=32`).
pub const CODER_PARTITION_COUNT: usize = TABLE_D_5_CODER_PARTITION.len();

/// FFT-line span `(ωlow, ωhigh)` covered by coder partition `n`
/// (1-based, inclusive), per Annex D Table D.5.
///
/// Coder partition 0 is the single DC FFT line 1; for `n ≥ 1` the
/// span is `boundary(n-1) + 1 ..= boundary(n)`. Returns `None` for
/// `n > 32` (out of table).
#[must_use]
pub fn coder_partition_span(n: usize) -> Option<(u32, u32)> {
    let row = TABLE_D_5_CODER_PARTITION.get(n)?;
    let low = if n == 0 {
        1
    } else {
        TABLE_D_5_CODER_PARTITION[n - 1].boundary + 1
    };
    Some((low, row.boundary))
}

/// The coder partition index `n` containing FFT line `omega`
/// (1-based), per Annex D Table D.5.
///
/// Returns `None` for `omega == 0` or `omega > 513` (outside the
/// 1024-point analysis FFT's `1..=513` working range).
#[must_use]
pub fn coder_partition_of_line(omega: u32) -> Option<usize> {
    if omega == 0 || omega > 513 {
        return None;
    }
    TABLE_D_5_CODER_PARTITION
        .iter()
        .position(|p| omega <= p.boundary)
}

/// Annex D clause **D.2.4 step (n)** Model-2 *signal-to-mask ratio*
/// `SMR_n` for one coder partition.
///
/// Computes the SMR (dB) for coder partition `n` of Table D.5, per the
/// verbatim step-(n) procedure (PDF page 132 / printed 126). `r2` is the
/// per-FFT-line **signal energy** `r_ω²` indexed by 0-based FFT line
/// (the `r_ω²` summed in step e); `thr` is the per-FFT-line **threshold
/// of audibility** `thr_ω` (output of [`include_absolute_threshold`]),
/// same indexing. The partition's FFT-line span and width flag come from
/// Table D.5 ([`coder_partition_span`] / [`CoderPartition::width`]).
///
/// The spec:
///
/// ```text
/// epart_n = Σ_{ω=ωlow..ωhigh} r_ω²
///
/// if width_n == 1 (narrow):  npart_n = Σ_{ω=ωlow..ωhigh} thr_ω
/// else (wide):               npart_n = min(thr_ωlow … thr_ωhigh)
///                                        · (ωhigh - ωlow + 1)
///   where min(…) returns the smallest *positive* argument.
///
/// SMR_n = 10 · log10( epart_n / npart_n )
/// ```
///
/// Returns `None` for `n` out of Table D.5's range (`n > 32`), or when
/// the partition's FFT-line span exceeds the supplied buffers. A
/// partition with `npart_n == 0` (no positive threshold, e.g. a fully
/// silent band) has no finite ratio and yields `None` as the documented
/// safe response, leaving the caller to apply its own fallback rather
/// than propagating an infinity into the bit allocator.
#[must_use]
pub fn signal_to_mask_ratio_db(n: usize, r2: &[f64], thr: &[f64]) -> Option<f64> {
    let (omega_low, omega_high) = coder_partition_span(n)?;
    let width = TABLE_D_5_CODER_PARTITION.get(n)?.width;
    let hi = omega_high as usize;
    if hi > r2.len() || hi > thr.len() {
        return None;
    }
    // 1-based inclusive [ωlow, ωhigh] → 0-based half-open slice.
    let lo0 = (omega_low - 1) as usize;
    let r2_span = &r2[lo0..hi];
    let thr_span = &thr[lo0..hi];

    let epart: f64 = r2_span.iter().sum();

    let npart = if width == 1 {
        // Psychoacoustically narrow band: sum the per-line thresholds.
        thr_span.iter().sum::<f64>()
    } else {
        // Psychoacoustically wide band: smallest *positive* per-line
        // threshold times the line count.
        let min_pos = thr_span
            .iter()
            .copied()
            .filter(|&t| t > 0.0)
            .fold(f64::INFINITY, f64::min);
        if !min_pos.is_finite() {
            0.0
        } else {
            min_pos * thr_span.len() as f64
        }
    };

    if npart <= 0.0 {
        return None;
    }
    Some(10.0 * (epart / npart).log10())
}

/// One row of an Annex D Table **D.4a / D.4b / D.4c** — the Model 2
/// *absolute threshold table* (per FFT line).
///
/// Clause D.2 tabulates, per range of 1024-point-analysis-FFT lines, a
/// single absolute-threshold (threshold-in-quiet) value in dB. The
/// table is laid out as `index lower`, `index higher`, `absthr [dB]`:
/// `(line_lower ..= line_higher)` is the inclusive, 1-based range of FFT
/// lines that share the one `threshold_db` entry. The ranges tile the
/// low FFT lines one-per-row and widen to 2-, 4- and 8-line groups
/// toward the top of the band. The spec page note:
///
/// > A value of 0 dB represents a level in the absolute threshold
/// > calculation of 96 dB below the energy of a sine wave of amplitude
/// > ±32 760.
///
/// — fixes the dB→energy anchor the step-(l) [`include_absolute_threshold`]
/// caller uses when converting `threshold_db` into the energy domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsThrEntry {
    /// First FFT line (1-based, inclusive) covered by this entry.
    pub line_lower: u32,
    /// Last FFT line (1-based, inclusive) covered by this entry.
    pub line_higher: u32,
    /// Absolute threshold (threshold in quiet) for the line range, in dB.
    pub threshold_db: f64,
}

impl AbsThrEntry {
    /// Number of FFT lines this entry covers (`line_higher − line_lower + 1`).
    #[must_use]
    pub const fn line_count(self) -> u32 {
        self.line_higher - self.line_lower + 1
    }
}

/// Annex D Table **D.4a** — Model 2 absolute threshold table, Fs = 32 kHz.
///
/// 132 rows; the FFT-line ranges run 1…480 (the topmost row covers lines
/// 473–480). Source: the staged CSV
/// `docs/audio/mp3/annex-d-table-D4a-absolute-threshold-32kHz.csv`
/// (text transcription of ISO/IEC 11172-3:1993 Annex D Table D.4a, PDF
/// printed page 136 / PDF p.142), cross-checked against the page render
/// `docs/audio/mp3/annex-d-renders/Table-D.4a-absolute-threshold-32kHz-p136.png`.
///
/// The `threshold_db` column matches the Layer II D.1d threshold-in-quiet
/// sequence entry-for-entry **except** the last entry (lines 473–480),
/// which D.4a prints `51.03` dB where D.1d's i = 132 prints `51.04` dB —
/// a documented ISO last-digit print divergence. The value below is the
/// as-printed **D.4a** figure (`51.03`).
// The `6.28` dB threshold at line 15 is a verbatim spec table value, not
// an approximation of `f64::consts::TAU`; the clippy lint is a false
// positive on transcribed numeric data.
#[allow(clippy::approx_constant)]
pub static TABLE_D_4A_ABSTHR_LAYER2_32: [AbsThrEntry; 132] = [
    AbsThrEntry {
        line_lower: 1,
        line_higher: 1,
        threshold_db: 58.23,
    },
    AbsThrEntry {
        line_lower: 2,
        line_higher: 2,
        threshold_db: 33.44,
    },
    AbsThrEntry {
        line_lower: 3,
        line_higher: 3,
        threshold_db: 24.17,
    },
    AbsThrEntry {
        line_lower: 4,
        line_higher: 4,
        threshold_db: 19.20,
    },
    AbsThrEntry {
        line_lower: 5,
        line_higher: 5,
        threshold_db: 16.05,
    },
    AbsThrEntry {
        line_lower: 6,
        line_higher: 6,
        threshold_db: 13.87,
    },
    AbsThrEntry {
        line_lower: 7,
        line_higher: 7,
        threshold_db: 12.26,
    },
    AbsThrEntry {
        line_lower: 8,
        line_higher: 8,
        threshold_db: 11.01,
    },
    AbsThrEntry {
        line_lower: 9,
        line_higher: 9,
        threshold_db: 10.01,
    },
    AbsThrEntry {
        line_lower: 10,
        line_higher: 10,
        threshold_db: 9.20,
    },
    AbsThrEntry {
        line_lower: 11,
        line_higher: 11,
        threshold_db: 8.52,
    },
    AbsThrEntry {
        line_lower: 12,
        line_higher: 12,
        threshold_db: 7.94,
    },
    AbsThrEntry {
        line_lower: 13,
        line_higher: 13,
        threshold_db: 7.44,
    },
    AbsThrEntry {
        line_lower: 14,
        line_higher: 14,
        threshold_db: 7.00,
    },
    AbsThrEntry {
        line_lower: 15,
        line_higher: 15,
        threshold_db: 6.62,
    },
    AbsThrEntry {
        line_lower: 16,
        line_higher: 16,
        threshold_db: 6.28,
    },
    AbsThrEntry {
        line_lower: 17,
        line_higher: 17,
        threshold_db: 5.97,
    },
    AbsThrEntry {
        line_lower: 18,
        line_higher: 18,
        threshold_db: 5.70,
    },
    AbsThrEntry {
        line_lower: 19,
        line_higher: 19,
        threshold_db: 5.44,
    },
    AbsThrEntry {
        line_lower: 20,
        line_higher: 20,
        threshold_db: 5.21,
    },
    AbsThrEntry {
        line_lower: 21,
        line_higher: 21,
        threshold_db: 5.00,
    },
    AbsThrEntry {
        line_lower: 22,
        line_higher: 22,
        threshold_db: 4.80,
    },
    AbsThrEntry {
        line_lower: 23,
        line_higher: 23,
        threshold_db: 4.62,
    },
    AbsThrEntry {
        line_lower: 24,
        line_higher: 24,
        threshold_db: 4.45,
    },
    AbsThrEntry {
        line_lower: 25,
        line_higher: 25,
        threshold_db: 4.29,
    },
    AbsThrEntry {
        line_lower: 26,
        line_higher: 26,
        threshold_db: 4.14,
    },
    AbsThrEntry {
        line_lower: 27,
        line_higher: 27,
        threshold_db: 4.00,
    },
    AbsThrEntry {
        line_lower: 28,
        line_higher: 28,
        threshold_db: 3.86,
    },
    AbsThrEntry {
        line_lower: 29,
        line_higher: 29,
        threshold_db: 3.73,
    },
    AbsThrEntry {
        line_lower: 30,
        line_higher: 30,
        threshold_db: 3.61,
    },
    AbsThrEntry {
        line_lower: 31,
        line_higher: 31,
        threshold_db: 3.49,
    },
    AbsThrEntry {
        line_lower: 32,
        line_higher: 32,
        threshold_db: 3.37,
    },
    AbsThrEntry {
        line_lower: 33,
        line_higher: 33,
        threshold_db: 3.26,
    },
    AbsThrEntry {
        line_lower: 34,
        line_higher: 34,
        threshold_db: 3.15,
    },
    AbsThrEntry {
        line_lower: 35,
        line_higher: 35,
        threshold_db: 3.04,
    },
    AbsThrEntry {
        line_lower: 36,
        line_higher: 36,
        threshold_db: 2.93,
    },
    AbsThrEntry {
        line_lower: 37,
        line_higher: 37,
        threshold_db: 2.83,
    },
    AbsThrEntry {
        line_lower: 38,
        line_higher: 38,
        threshold_db: 2.73,
    },
    AbsThrEntry {
        line_lower: 39,
        line_higher: 39,
        threshold_db: 2.63,
    },
    AbsThrEntry {
        line_lower: 40,
        line_higher: 40,
        threshold_db: 2.53,
    },
    AbsThrEntry {
        line_lower: 41,
        line_higher: 41,
        threshold_db: 2.42,
    },
    AbsThrEntry {
        line_lower: 42,
        line_higher: 42,
        threshold_db: 2.32,
    },
    AbsThrEntry {
        line_lower: 43,
        line_higher: 43,
        threshold_db: 2.22,
    },
    AbsThrEntry {
        line_lower: 44,
        line_higher: 44,
        threshold_db: 2.12,
    },
    AbsThrEntry {
        line_lower: 45,
        line_higher: 45,
        threshold_db: 2.02,
    },
    AbsThrEntry {
        line_lower: 46,
        line_higher: 46,
        threshold_db: 1.92,
    },
    AbsThrEntry {
        line_lower: 47,
        line_higher: 47,
        threshold_db: 1.81,
    },
    AbsThrEntry {
        line_lower: 48,
        line_higher: 48,
        threshold_db: 1.71,
    },
    AbsThrEntry {
        line_lower: 49,
        line_higher: 50,
        threshold_db: 1.49,
    },
    AbsThrEntry {
        line_lower: 51,
        line_higher: 52,
        threshold_db: 1.27,
    },
    AbsThrEntry {
        line_lower: 53,
        line_higher: 54,
        threshold_db: 1.04,
    },
    AbsThrEntry {
        line_lower: 55,
        line_higher: 56,
        threshold_db: 0.80,
    },
    AbsThrEntry {
        line_lower: 57,
        line_higher: 58,
        threshold_db: 0.55,
    },
    AbsThrEntry {
        line_lower: 59,
        line_higher: 60,
        threshold_db: 0.29,
    },
    AbsThrEntry {
        line_lower: 61,
        line_higher: 62,
        threshold_db: 0.02,
    },
    AbsThrEntry {
        line_lower: 63,
        line_higher: 64,
        threshold_db: -0.25,
    },
    AbsThrEntry {
        line_lower: 65,
        line_higher: 66,
        threshold_db: -0.54,
    },
    AbsThrEntry {
        line_lower: 67,
        line_higher: 68,
        threshold_db: -0.83,
    },
    AbsThrEntry {
        line_lower: 69,
        line_higher: 70,
        threshold_db: -1.12,
    },
    AbsThrEntry {
        line_lower: 71,
        line_higher: 72,
        threshold_db: -1.43,
    },
    AbsThrEntry {
        line_lower: 73,
        line_higher: 74,
        threshold_db: -1.73,
    },
    AbsThrEntry {
        line_lower: 75,
        line_higher: 76,
        threshold_db: -2.04,
    },
    AbsThrEntry {
        line_lower: 77,
        line_higher: 78,
        threshold_db: -2.34,
    },
    AbsThrEntry {
        line_lower: 79,
        line_higher: 80,
        threshold_db: -2.64,
    },
    AbsThrEntry {
        line_lower: 81,
        line_higher: 82,
        threshold_db: -2.93,
    },
    AbsThrEntry {
        line_lower: 83,
        line_higher: 84,
        threshold_db: -3.22,
    },
    AbsThrEntry {
        line_lower: 85,
        line_higher: 86,
        threshold_db: -3.49,
    },
    AbsThrEntry {
        line_lower: 87,
        line_higher: 88,
        threshold_db: -3.74,
    },
    AbsThrEntry {
        line_lower: 89,
        line_higher: 90,
        threshold_db: -3.98,
    },
    AbsThrEntry {
        line_lower: 91,
        line_higher: 92,
        threshold_db: -4.20,
    },
    AbsThrEntry {
        line_lower: 93,
        line_higher: 94,
        threshold_db: -4.40,
    },
    AbsThrEntry {
        line_lower: 95,
        line_higher: 96,
        threshold_db: -4.57,
    },
    AbsThrEntry {
        line_lower: 97,
        line_higher: 100,
        threshold_db: -4.82,
    },
    AbsThrEntry {
        line_lower: 101,
        line_higher: 104,
        threshold_db: -4.96,
    },
    AbsThrEntry {
        line_lower: 105,
        line_higher: 108,
        threshold_db: -4.97,
    },
    AbsThrEntry {
        line_lower: 109,
        line_higher: 112,
        threshold_db: -4.86,
    },
    AbsThrEntry {
        line_lower: 113,
        line_higher: 116,
        threshold_db: -4.63,
    },
    AbsThrEntry {
        line_lower: 117,
        line_higher: 120,
        threshold_db: -4.29,
    },
    AbsThrEntry {
        line_lower: 121,
        line_higher: 124,
        threshold_db: -3.87,
    },
    AbsThrEntry {
        line_lower: 125,
        line_higher: 128,
        threshold_db: -3.39,
    },
    AbsThrEntry {
        line_lower: 129,
        line_higher: 132,
        threshold_db: -2.86,
    },
    AbsThrEntry {
        line_lower: 133,
        line_higher: 136,
        threshold_db: -2.31,
    },
    AbsThrEntry {
        line_lower: 137,
        line_higher: 140,
        threshold_db: -1.77,
    },
    AbsThrEntry {
        line_lower: 141,
        line_higher: 144,
        threshold_db: -1.24,
    },
    AbsThrEntry {
        line_lower: 145,
        line_higher: 148,
        threshold_db: -0.74,
    },
    AbsThrEntry {
        line_lower: 149,
        line_higher: 152,
        threshold_db: -0.29,
    },
    AbsThrEntry {
        line_lower: 153,
        line_higher: 156,
        threshold_db: 0.12,
    },
    AbsThrEntry {
        line_lower: 157,
        line_higher: 160,
        threshold_db: 0.48,
    },
    AbsThrEntry {
        line_lower: 161,
        line_higher: 164,
        threshold_db: 0.79,
    },
    AbsThrEntry {
        line_lower: 165,
        line_higher: 168,
        threshold_db: 1.06,
    },
    AbsThrEntry {
        line_lower: 169,
        line_higher: 172,
        threshold_db: 1.29,
    },
    AbsThrEntry {
        line_lower: 173,
        line_higher: 176,
        threshold_db: 1.49,
    },
    AbsThrEntry {
        line_lower: 177,
        line_higher: 180,
        threshold_db: 1.66,
    },
    AbsThrEntry {
        line_lower: 181,
        line_higher: 184,
        threshold_db: 1.81,
    },
    AbsThrEntry {
        line_lower: 185,
        line_higher: 188,
        threshold_db: 1.95,
    },
    AbsThrEntry {
        line_lower: 189,
        line_higher: 192,
        threshold_db: 2.08,
    },
    AbsThrEntry {
        line_lower: 193,
        line_higher: 200,
        threshold_db: 2.33,
    },
    AbsThrEntry {
        line_lower: 201,
        line_higher: 208,
        threshold_db: 2.59,
    },
    AbsThrEntry {
        line_lower: 209,
        line_higher: 216,
        threshold_db: 2.86,
    },
    AbsThrEntry {
        line_lower: 217,
        line_higher: 224,
        threshold_db: 3.17,
    },
    AbsThrEntry {
        line_lower: 225,
        line_higher: 232,
        threshold_db: 3.51,
    },
    AbsThrEntry {
        line_lower: 233,
        line_higher: 240,
        threshold_db: 3.89,
    },
    AbsThrEntry {
        line_lower: 241,
        line_higher: 248,
        threshold_db: 4.31,
    },
    AbsThrEntry {
        line_lower: 249,
        line_higher: 256,
        threshold_db: 4.79,
    },
    AbsThrEntry {
        line_lower: 257,
        line_higher: 264,
        threshold_db: 5.31,
    },
    AbsThrEntry {
        line_lower: 265,
        line_higher: 272,
        threshold_db: 5.88,
    },
    AbsThrEntry {
        line_lower: 273,
        line_higher: 280,
        threshold_db: 6.50,
    },
    AbsThrEntry {
        line_lower: 281,
        line_higher: 288,
        threshold_db: 7.19,
    },
    AbsThrEntry {
        line_lower: 289,
        line_higher: 296,
        threshold_db: 7.93,
    },
    AbsThrEntry {
        line_lower: 297,
        line_higher: 304,
        threshold_db: 8.75,
    },
    AbsThrEntry {
        line_lower: 305,
        line_higher: 312,
        threshold_db: 9.63,
    },
    AbsThrEntry {
        line_lower: 313,
        line_higher: 320,
        threshold_db: 10.58,
    },
    AbsThrEntry {
        line_lower: 321,
        line_higher: 328,
        threshold_db: 11.60,
    },
    AbsThrEntry {
        line_lower: 329,
        line_higher: 336,
        threshold_db: 12.71,
    },
    AbsThrEntry {
        line_lower: 337,
        line_higher: 344,
        threshold_db: 13.90,
    },
    AbsThrEntry {
        line_lower: 345,
        line_higher: 352,
        threshold_db: 15.18,
    },
    AbsThrEntry {
        line_lower: 353,
        line_higher: 360,
        threshold_db: 16.54,
    },
    AbsThrEntry {
        line_lower: 361,
        line_higher: 368,
        threshold_db: 18.01,
    },
    AbsThrEntry {
        line_lower: 369,
        line_higher: 376,
        threshold_db: 19.57,
    },
    AbsThrEntry {
        line_lower: 377,
        line_higher: 384,
        threshold_db: 21.23,
    },
    AbsThrEntry {
        line_lower: 385,
        line_higher: 392,
        threshold_db: 23.01,
    },
    AbsThrEntry {
        line_lower: 393,
        line_higher: 400,
        threshold_db: 24.90,
    },
    AbsThrEntry {
        line_lower: 401,
        line_higher: 408,
        threshold_db: 26.90,
    },
    AbsThrEntry {
        line_lower: 409,
        line_higher: 416,
        threshold_db: 29.03,
    },
    AbsThrEntry {
        line_lower: 417,
        line_higher: 424,
        threshold_db: 31.28,
    },
    AbsThrEntry {
        line_lower: 425,
        line_higher: 432,
        threshold_db: 33.67,
    },
    AbsThrEntry {
        line_lower: 433,
        line_higher: 440,
        threshold_db: 36.19,
    },
    AbsThrEntry {
        line_lower: 441,
        line_higher: 448,
        threshold_db: 38.86,
    },
    AbsThrEntry {
        line_lower: 449,
        line_higher: 456,
        threshold_db: 41.67,
    },
    AbsThrEntry {
        line_lower: 457,
        line_higher: 464,
        threshold_db: 44.63,
    },
    AbsThrEntry {
        line_lower: 465,
        line_higher: 472,
        threshold_db: 47.76,
    },
    AbsThrEntry {
        line_lower: 473,
        line_higher: 480,
        threshold_db: 51.03,
    },
];

/// Annex D Table **D.4b** — Model 2 absolute threshold table, Fs = 44,1 kHz.
///
/// 130 rows; the FFT-line ranges run 1…464 (the topmost row covers lines
/// 457–464). Source: the staged CSV
/// `docs/audio/mp3/annex-d-table-D4b-absolute-threshold-44k1Hz.csv`
/// (text transcription of ISO/IEC 11172-3:1993 Annex D Table D.4b, PDF
/// printed page 137 / PDF p.143), cross-checked against the page render
/// `docs/audio/mp3/annex-d-renders/Table-D.4b-absolute-threshold-44k1Hz-p137.png`.
///
/// The `threshold_db` column matches the Layer II D.1e threshold-in-quiet
/// sequence through i ≈ 103, then diverges: a run of cells print `0.01` dB
/// **lower** than their D.1e twins, and the saturation ceiling differs —
/// D.4b caps at `69.13` dB (the top of the band, lines ≈ 369–464) where
/// D.1e caps at `68.00` dB. The values below are the as-printed **D.4b**
/// figures, not the D.1e twins; this is a documented ISO last-digit /
/// ceiling print divergence.
pub static TABLE_D_4B_ABSTHR_LAYER2_44K1: [AbsThrEntry; 130] = [
    AbsThrEntry {
        line_lower: 1,
        line_higher: 1,
        threshold_db: 45.05,
    },
    AbsThrEntry {
        line_lower: 2,
        line_higher: 2,
        threshold_db: 25.87,
    },
    AbsThrEntry {
        line_lower: 3,
        line_higher: 3,
        threshold_db: 18.70,
    },
    AbsThrEntry {
        line_lower: 4,
        line_higher: 4,
        threshold_db: 14.85,
    },
    AbsThrEntry {
        line_lower: 5,
        line_higher: 5,
        threshold_db: 12.41,
    },
    AbsThrEntry {
        line_lower: 6,
        line_higher: 6,
        threshold_db: 10.72,
    },
    AbsThrEntry {
        line_lower: 7,
        line_higher: 7,
        threshold_db: 9.47,
    },
    AbsThrEntry {
        line_lower: 8,
        line_higher: 8,
        threshold_db: 8.50,
    },
    AbsThrEntry {
        line_lower: 9,
        line_higher: 9,
        threshold_db: 7.73,
    },
    AbsThrEntry {
        line_lower: 10,
        line_higher: 10,
        threshold_db: 7.10,
    },
    AbsThrEntry {
        line_lower: 11,
        line_higher: 11,
        threshold_db: 6.56,
    },
    AbsThrEntry {
        line_lower: 12,
        line_higher: 12,
        threshold_db: 6.11,
    },
    AbsThrEntry {
        line_lower: 13,
        line_higher: 13,
        threshold_db: 5.72,
    },
    AbsThrEntry {
        line_lower: 14,
        line_higher: 14,
        threshold_db: 5.37,
    },
    AbsThrEntry {
        line_lower: 15,
        line_higher: 15,
        threshold_db: 5.07,
    },
    AbsThrEntry {
        line_lower: 16,
        line_higher: 16,
        threshold_db: 4.79,
    },
    AbsThrEntry {
        line_lower: 17,
        line_higher: 17,
        threshold_db: 4.55,
    },
    AbsThrEntry {
        line_lower: 18,
        line_higher: 18,
        threshold_db: 4.32,
    },
    AbsThrEntry {
        line_lower: 19,
        line_higher: 19,
        threshold_db: 4.11,
    },
    AbsThrEntry {
        line_lower: 20,
        line_higher: 20,
        threshold_db: 3.92,
    },
    AbsThrEntry {
        line_lower: 21,
        line_higher: 21,
        threshold_db: 3.74,
    },
    AbsThrEntry {
        line_lower: 22,
        line_higher: 22,
        threshold_db: 3.57,
    },
    AbsThrEntry {
        line_lower: 23,
        line_higher: 23,
        threshold_db: 3.40,
    },
    AbsThrEntry {
        line_lower: 24,
        line_higher: 24,
        threshold_db: 3.25,
    },
    AbsThrEntry {
        line_lower: 25,
        line_higher: 25,
        threshold_db: 3.10,
    },
    AbsThrEntry {
        line_lower: 26,
        line_higher: 26,
        threshold_db: 2.95,
    },
    AbsThrEntry {
        line_lower: 27,
        line_higher: 27,
        threshold_db: 2.81,
    },
    AbsThrEntry {
        line_lower: 28,
        line_higher: 28,
        threshold_db: 2.67,
    },
    AbsThrEntry {
        line_lower: 29,
        line_higher: 29,
        threshold_db: 2.53,
    },
    AbsThrEntry {
        line_lower: 30,
        line_higher: 30,
        threshold_db: 2.39,
    },
    AbsThrEntry {
        line_lower: 31,
        line_higher: 31,
        threshold_db: 2.25,
    },
    AbsThrEntry {
        line_lower: 32,
        line_higher: 32,
        threshold_db: 2.11,
    },
    AbsThrEntry {
        line_lower: 33,
        line_higher: 33,
        threshold_db: 1.97,
    },
    AbsThrEntry {
        line_lower: 34,
        line_higher: 34,
        threshold_db: 1.83,
    },
    AbsThrEntry {
        line_lower: 35,
        line_higher: 35,
        threshold_db: 1.68,
    },
    AbsThrEntry {
        line_lower: 36,
        line_higher: 36,
        threshold_db: 1.53,
    },
    AbsThrEntry {
        line_lower: 37,
        line_higher: 37,
        threshold_db: 1.38,
    },
    AbsThrEntry {
        line_lower: 38,
        line_higher: 38,
        threshold_db: 1.23,
    },
    AbsThrEntry {
        line_lower: 39,
        line_higher: 39,
        threshold_db: 1.07,
    },
    AbsThrEntry {
        line_lower: 40,
        line_higher: 40,
        threshold_db: 0.90,
    },
    AbsThrEntry {
        line_lower: 41,
        line_higher: 41,
        threshold_db: 0.74,
    },
    AbsThrEntry {
        line_lower: 42,
        line_higher: 42,
        threshold_db: 0.56,
    },
    AbsThrEntry {
        line_lower: 43,
        line_higher: 43,
        threshold_db: 0.39,
    },
    AbsThrEntry {
        line_lower: 44,
        line_higher: 44,
        threshold_db: 0.21,
    },
    AbsThrEntry {
        line_lower: 45,
        line_higher: 45,
        threshold_db: 0.02,
    },
    AbsThrEntry {
        line_lower: 46,
        line_higher: 46,
        threshold_db: -0.17,
    },
    AbsThrEntry {
        line_lower: 47,
        line_higher: 47,
        threshold_db: -0.36,
    },
    AbsThrEntry {
        line_lower: 48,
        line_higher: 48,
        threshold_db: -0.56,
    },
    AbsThrEntry {
        line_lower: 49,
        line_higher: 50,
        threshold_db: -0.96,
    },
    AbsThrEntry {
        line_lower: 51,
        line_higher: 52,
        threshold_db: -1.38,
    },
    AbsThrEntry {
        line_lower: 53,
        line_higher: 54,
        threshold_db: -1.79,
    },
    AbsThrEntry {
        line_lower: 55,
        line_higher: 56,
        threshold_db: -2.21,
    },
    AbsThrEntry {
        line_lower: 57,
        line_higher: 58,
        threshold_db: -2.63,
    },
    AbsThrEntry {
        line_lower: 59,
        line_higher: 60,
        threshold_db: -3.03,
    },
    AbsThrEntry {
        line_lower: 61,
        line_higher: 62,
        threshold_db: -3.41,
    },
    AbsThrEntry {
        line_lower: 63,
        line_higher: 64,
        threshold_db: -3.77,
    },
    AbsThrEntry {
        line_lower: 65,
        line_higher: 66,
        threshold_db: -4.09,
    },
    AbsThrEntry {
        line_lower: 67,
        line_higher: 68,
        threshold_db: -4.37,
    },
    AbsThrEntry {
        line_lower: 69,
        line_higher: 70,
        threshold_db: -4.60,
    },
    AbsThrEntry {
        line_lower: 71,
        line_higher: 72,
        threshold_db: -4.78,
    },
    AbsThrEntry {
        line_lower: 73,
        line_higher: 74,
        threshold_db: -4.91,
    },
    AbsThrEntry {
        line_lower: 75,
        line_higher: 76,
        threshold_db: -4.97,
    },
    AbsThrEntry {
        line_lower: 77,
        line_higher: 78,
        threshold_db: -4.98,
    },
    AbsThrEntry {
        line_lower: 79,
        line_higher: 80,
        threshold_db: -4.92,
    },
    AbsThrEntry {
        line_lower: 81,
        line_higher: 82,
        threshold_db: -4.81,
    },
    AbsThrEntry {
        line_lower: 83,
        line_higher: 84,
        threshold_db: -4.65,
    },
    AbsThrEntry {
        line_lower: 85,
        line_higher: 86,
        threshold_db: -4.43,
    },
    AbsThrEntry {
        line_lower: 87,
        line_higher: 88,
        threshold_db: -4.17,
    },
    AbsThrEntry {
        line_lower: 89,
        line_higher: 90,
        threshold_db: -3.87,
    },
    AbsThrEntry {
        line_lower: 91,
        line_higher: 92,
        threshold_db: -3.54,
    },
    AbsThrEntry {
        line_lower: 93,
        line_higher: 94,
        threshold_db: -3.19,
    },
    AbsThrEntry {
        line_lower: 95,
        line_higher: 96,
        threshold_db: -2.82,
    },
    AbsThrEntry {
        line_lower: 97,
        line_higher: 100,
        threshold_db: -2.06,
    },
    AbsThrEntry {
        line_lower: 101,
        line_higher: 104,
        threshold_db: -1.32,
    },
    AbsThrEntry {
        line_lower: 105,
        line_higher: 108,
        threshold_db: -0.64,
    },
    AbsThrEntry {
        line_lower: 109,
        line_higher: 112,
        threshold_db: -0.04,
    },
    AbsThrEntry {
        line_lower: 113,
        line_higher: 116,
        threshold_db: 0.47,
    },
    AbsThrEntry {
        line_lower: 117,
        line_higher: 120,
        threshold_db: 0.89,
    },
    AbsThrEntry {
        line_lower: 121,
        line_higher: 124,
        threshold_db: 1.23,
    },
    AbsThrEntry {
        line_lower: 125,
        line_higher: 128,
        threshold_db: 1.51,
    },
    AbsThrEntry {
        line_lower: 129,
        line_higher: 132,
        threshold_db: 1.74,
    },
    AbsThrEntry {
        line_lower: 133,
        line_higher: 136,
        threshold_db: 1.93,
    },
    AbsThrEntry {
        line_lower: 137,
        line_higher: 140,
        threshold_db: 2.11,
    },
    AbsThrEntry {
        line_lower: 141,
        line_higher: 144,
        threshold_db: 2.28,
    },
    AbsThrEntry {
        line_lower: 145,
        line_higher: 148,
        threshold_db: 2.46,
    },
    AbsThrEntry {
        line_lower: 149,
        line_higher: 152,
        threshold_db: 2.63,
    },
    AbsThrEntry {
        line_lower: 153,
        line_higher: 156,
        threshold_db: 2.82,
    },
    AbsThrEntry {
        line_lower: 157,
        line_higher: 160,
        threshold_db: 3.03,
    },
    AbsThrEntry {
        line_lower: 161,
        line_higher: 164,
        threshold_db: 3.25,
    },
    AbsThrEntry {
        line_lower: 165,
        line_higher: 168,
        threshold_db: 3.49,
    },
    AbsThrEntry {
        line_lower: 169,
        line_higher: 172,
        threshold_db: 3.74,
    },
    AbsThrEntry {
        line_lower: 173,
        line_higher: 176,
        threshold_db: 4.02,
    },
    AbsThrEntry {
        line_lower: 177,
        line_higher: 180,
        threshold_db: 4.32,
    },
    AbsThrEntry {
        line_lower: 181,
        line_higher: 184,
        threshold_db: 4.64,
    },
    AbsThrEntry {
        line_lower: 185,
        line_higher: 188,
        threshold_db: 4.98,
    },
    AbsThrEntry {
        line_lower: 189,
        line_higher: 192,
        threshold_db: 5.35,
    },
    AbsThrEntry {
        line_lower: 193,
        line_higher: 200,
        threshold_db: 6.15,
    },
    AbsThrEntry {
        line_lower: 201,
        line_higher: 208,
        threshold_db: 7.07,
    },
    AbsThrEntry {
        line_lower: 209,
        line_higher: 216,
        threshold_db: 8.10,
    },
    AbsThrEntry {
        line_lower: 217,
        line_higher: 224,
        threshold_db: 9.25,
    },
    AbsThrEntry {
        line_lower: 225,
        line_higher: 232,
        threshold_db: 10.54,
    },
    AbsThrEntry {
        line_lower: 233,
        line_higher: 240,
        threshold_db: 11.97,
    },
    AbsThrEntry {
        line_lower: 241,
        line_higher: 248,
        threshold_db: 13.56,
    },
    AbsThrEntry {
        line_lower: 249,
        line_higher: 256,
        threshold_db: 15.30,
    },
    AbsThrEntry {
        line_lower: 257,
        line_higher: 264,
        threshold_db: 17.23,
    },
    AbsThrEntry {
        line_lower: 265,
        line_higher: 272,
        threshold_db: 19.33,
    },
    AbsThrEntry {
        line_lower: 273,
        line_higher: 280,
        threshold_db: 21.64,
    },
    AbsThrEntry {
        line_lower: 281,
        line_higher: 288,
        threshold_db: 24.15,
    },
    AbsThrEntry {
        line_lower: 289,
        line_higher: 296,
        threshold_db: 26.88,
    },
    AbsThrEntry {
        line_lower: 297,
        line_higher: 304,
        threshold_db: 29.84,
    },
    AbsThrEntry {
        line_lower: 305,
        line_higher: 312,
        threshold_db: 33.04,
    },
    AbsThrEntry {
        line_lower: 313,
        line_higher: 320,
        threshold_db: 36.51,
    },
    AbsThrEntry {
        line_lower: 321,
        line_higher: 328,
        threshold_db: 40.24,
    },
    AbsThrEntry {
        line_lower: 329,
        line_higher: 336,
        threshold_db: 44.26,
    },
    AbsThrEntry {
        line_lower: 337,
        line_higher: 344,
        threshold_db: 48.58,
    },
    AbsThrEntry {
        line_lower: 345,
        line_higher: 352,
        threshold_db: 53.21,
    },
    AbsThrEntry {
        line_lower: 353,
        line_higher: 360,
        threshold_db: 58.17,
    },
    AbsThrEntry {
        line_lower: 361,
        line_higher: 368,
        threshold_db: 63.48,
    },
    AbsThrEntry {
        line_lower: 369,
        line_higher: 376,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 377,
        line_higher: 384,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 385,
        line_higher: 392,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 393,
        line_higher: 400,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 401,
        line_higher: 408,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 409,
        line_higher: 416,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 417,
        line_higher: 424,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 425,
        line_higher: 432,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 433,
        line_higher: 440,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 441,
        line_higher: 448,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 449,
        line_higher: 456,
        threshold_db: 69.13,
    },
    AbsThrEntry {
        line_lower: 457,
        line_higher: 464,
        threshold_db: 69.13,
    },
];

/// Annex D Table **D.4c** — Model 2 absolute threshold table, Fs = 48 kHz.
///
/// 126 rows; the FFT-line ranges run 1…432 (the topmost row covers lines
/// 425–432). Source: the staged CSV
/// `docs/audio/mp3/annex-d-table-D4c-absolute-threshold-48kHz.csv`
/// (text transcription of ISO/IEC 11172-3:1993 Annex D Table D.4c, PDF
/// printed page 138 / PDF p.144), cross-checked against the page render
/// `docs/audio/mp3/annex-d-renders/Table-D.4c-absolute-threshold-48kHz-p138.png`.
///
/// The `threshold_db` column matches the Layer II D.1f threshold-in-quiet
/// sequence entry-for-entry, including the `68.00` dB ceiling at the top
/// of the band (lines ≈ 333–432).
pub static TABLE_D_4C_ABSTHR_LAYER2_48: [AbsThrEntry; 126] = [
    AbsThrEntry {
        line_lower: 1,
        line_higher: 1,
        threshold_db: 42.10,
    },
    AbsThrEntry {
        line_lower: 2,
        line_higher: 2,
        threshold_db: 24.17,
    },
    AbsThrEntry {
        line_lower: 3,
        line_higher: 3,
        threshold_db: 17.47,
    },
    AbsThrEntry {
        line_lower: 4,
        line_higher: 4,
        threshold_db: 13.87,
    },
    AbsThrEntry {
        line_lower: 5,
        line_higher: 5,
        threshold_db: 11.60,
    },
    AbsThrEntry {
        line_lower: 6,
        line_higher: 6,
        threshold_db: 10.01,
    },
    AbsThrEntry {
        line_lower: 7,
        line_higher: 7,
        threshold_db: 8.84,
    },
    AbsThrEntry {
        line_lower: 8,
        line_higher: 8,
        threshold_db: 7.94,
    },
    AbsThrEntry {
        line_lower: 9,
        line_higher: 9,
        threshold_db: 7.22,
    },
    AbsThrEntry {
        line_lower: 10,
        line_higher: 10,
        threshold_db: 6.62,
    },
    AbsThrEntry {
        line_lower: 11,
        line_higher: 11,
        threshold_db: 6.12,
    },
    AbsThrEntry {
        line_lower: 12,
        line_higher: 12,
        threshold_db: 5.70,
    },
    AbsThrEntry {
        line_lower: 13,
        line_higher: 13,
        threshold_db: 5.33,
    },
    AbsThrEntry {
        line_lower: 14,
        line_higher: 14,
        threshold_db: 5.00,
    },
    AbsThrEntry {
        line_lower: 15,
        line_higher: 15,
        threshold_db: 4.71,
    },
    AbsThrEntry {
        line_lower: 16,
        line_higher: 16,
        threshold_db: 4.45,
    },
    AbsThrEntry {
        line_lower: 17,
        line_higher: 17,
        threshold_db: 4.21,
    },
    AbsThrEntry {
        line_lower: 18,
        line_higher: 18,
        threshold_db: 4.00,
    },
    AbsThrEntry {
        line_lower: 19,
        line_higher: 19,
        threshold_db: 3.79,
    },
    AbsThrEntry {
        line_lower: 20,
        line_higher: 20,
        threshold_db: 3.61,
    },
    AbsThrEntry {
        line_lower: 21,
        line_higher: 21,
        threshold_db: 3.43,
    },
    AbsThrEntry {
        line_lower: 22,
        line_higher: 22,
        threshold_db: 3.26,
    },
    AbsThrEntry {
        line_lower: 23,
        line_higher: 23,
        threshold_db: 3.09,
    },
    AbsThrEntry {
        line_lower: 24,
        line_higher: 24,
        threshold_db: 2.93,
    },
    AbsThrEntry {
        line_lower: 25,
        line_higher: 25,
        threshold_db: 2.78,
    },
    AbsThrEntry {
        line_lower: 26,
        line_higher: 26,
        threshold_db: 2.63,
    },
    AbsThrEntry {
        line_lower: 27,
        line_higher: 27,
        threshold_db: 2.47,
    },
    AbsThrEntry {
        line_lower: 28,
        line_higher: 28,
        threshold_db: 2.32,
    },
    AbsThrEntry {
        line_lower: 29,
        line_higher: 29,
        threshold_db: 2.17,
    },
    AbsThrEntry {
        line_lower: 30,
        line_higher: 30,
        threshold_db: 2.02,
    },
    AbsThrEntry {
        line_lower: 31,
        line_higher: 31,
        threshold_db: 1.86,
    },
    AbsThrEntry {
        line_lower: 32,
        line_higher: 32,
        threshold_db: 1.71,
    },
    AbsThrEntry {
        line_lower: 33,
        line_higher: 33,
        threshold_db: 1.55,
    },
    AbsThrEntry {
        line_lower: 34,
        line_higher: 34,
        threshold_db: 1.38,
    },
    AbsThrEntry {
        line_lower: 35,
        line_higher: 35,
        threshold_db: 1.21,
    },
    AbsThrEntry {
        line_lower: 36,
        line_higher: 36,
        threshold_db: 1.04,
    },
    AbsThrEntry {
        line_lower: 37,
        line_higher: 37,
        threshold_db: 0.86,
    },
    AbsThrEntry {
        line_lower: 38,
        line_higher: 38,
        threshold_db: 0.67,
    },
    AbsThrEntry {
        line_lower: 39,
        line_higher: 39,
        threshold_db: 0.49,
    },
    AbsThrEntry {
        line_lower: 40,
        line_higher: 40,
        threshold_db: 0.29,
    },
    AbsThrEntry {
        line_lower: 41,
        line_higher: 41,
        threshold_db: 0.09,
    },
    AbsThrEntry {
        line_lower: 42,
        line_higher: 42,
        threshold_db: -0.11,
    },
    AbsThrEntry {
        line_lower: 43,
        line_higher: 43,
        threshold_db: -0.32,
    },
    AbsThrEntry {
        line_lower: 44,
        line_higher: 44,
        threshold_db: -0.54,
    },
    AbsThrEntry {
        line_lower: 45,
        line_higher: 45,
        threshold_db: -0.75,
    },
    AbsThrEntry {
        line_lower: 46,
        line_higher: 46,
        threshold_db: -0.97,
    },
    AbsThrEntry {
        line_lower: 47,
        line_higher: 47,
        threshold_db: -1.20,
    },
    AbsThrEntry {
        line_lower: 48,
        line_higher: 48,
        threshold_db: -1.43,
    },
    AbsThrEntry {
        line_lower: 49,
        line_higher: 50,
        threshold_db: -1.88,
    },
    AbsThrEntry {
        line_lower: 51,
        line_higher: 52,
        threshold_db: -2.34,
    },
    AbsThrEntry {
        line_lower: 53,
        line_higher: 54,
        threshold_db: -2.79,
    },
    AbsThrEntry {
        line_lower: 55,
        line_higher: 56,
        threshold_db: -3.22,
    },
    AbsThrEntry {
        line_lower: 57,
        line_higher: 58,
        threshold_db: -3.62,
    },
    AbsThrEntry {
        line_lower: 59,
        line_higher: 60,
        threshold_db: -3.98,
    },
    AbsThrEntry {
        line_lower: 61,
        line_higher: 62,
        threshold_db: -4.30,
    },
    AbsThrEntry {
        line_lower: 63,
        line_higher: 64,
        threshold_db: -4.57,
    },
    AbsThrEntry {
        line_lower: 65,
        line_higher: 66,
        threshold_db: -4.77,
    },
    AbsThrEntry {
        line_lower: 67,
        line_higher: 68,
        threshold_db: -4.91,
    },
    AbsThrEntry {
        line_lower: 69,
        line_higher: 70,
        threshold_db: -4.98,
    },
    AbsThrEntry {
        line_lower: 71,
        line_higher: 72,
        threshold_db: -4.97,
    },
    AbsThrEntry {
        line_lower: 73,
        line_higher: 74,
        threshold_db: -4.90,
    },
    AbsThrEntry {
        line_lower: 75,
        line_higher: 76,
        threshold_db: -4.76,
    },
    AbsThrEntry {
        line_lower: 77,
        line_higher: 78,
        threshold_db: -4.55,
    },
    AbsThrEntry {
        line_lower: 79,
        line_higher: 80,
        threshold_db: -4.29,
    },
    AbsThrEntry {
        line_lower: 81,
        line_higher: 82,
        threshold_db: -3.99,
    },
    AbsThrEntry {
        line_lower: 83,
        line_higher: 84,
        threshold_db: -3.64,
    },
    AbsThrEntry {
        line_lower: 85,
        line_higher: 86,
        threshold_db: -3.26,
    },
    AbsThrEntry {
        line_lower: 87,
        line_higher: 88,
        threshold_db: -2.86,
    },
    AbsThrEntry {
        line_lower: 89,
        line_higher: 90,
        threshold_db: -2.45,
    },
    AbsThrEntry {
        line_lower: 91,
        line_higher: 92,
        threshold_db: -2.04,
    },
    AbsThrEntry {
        line_lower: 93,
        line_higher: 94,
        threshold_db: -1.63,
    },
    AbsThrEntry {
        line_lower: 95,
        line_higher: 96,
        threshold_db: -1.24,
    },
    AbsThrEntry {
        line_lower: 97,
        line_higher: 100,
        threshold_db: -0.51,
    },
    AbsThrEntry {
        line_lower: 101,
        line_higher: 104,
        threshold_db: 0.12,
    },
    AbsThrEntry {
        line_lower: 105,
        line_higher: 108,
        threshold_db: 0.64,
    },
    AbsThrEntry {
        line_lower: 109,
        line_higher: 112,
        threshold_db: 1.06,
    },
    AbsThrEntry {
        line_lower: 113,
        line_higher: 116,
        threshold_db: 1.39,
    },
    AbsThrEntry {
        line_lower: 117,
        line_higher: 120,
        threshold_db: 1.66,
    },
    AbsThrEntry {
        line_lower: 121,
        line_higher: 124,
        threshold_db: 1.88,
    },
    AbsThrEntry {
        line_lower: 125,
        line_higher: 128,
        threshold_db: 2.08,
    },
    AbsThrEntry {
        line_lower: 129,
        line_higher: 132,
        threshold_db: 2.27,
    },
    AbsThrEntry {
        line_lower: 133,
        line_higher: 136,
        threshold_db: 2.46,
    },
    AbsThrEntry {
        line_lower: 137,
        line_higher: 140,
        threshold_db: 2.65,
    },
    AbsThrEntry {
        line_lower: 141,
        line_higher: 144,
        threshold_db: 2.86,
    },
    AbsThrEntry {
        line_lower: 145,
        line_higher: 148,
        threshold_db: 3.09,
    },
    AbsThrEntry {
        line_lower: 149,
        line_higher: 152,
        threshold_db: 3.33,
    },
    AbsThrEntry {
        line_lower: 153,
        line_higher: 156,
        threshold_db: 3.60,
    },
    AbsThrEntry {
        line_lower: 157,
        line_higher: 160,
        threshold_db: 3.89,
    },
    AbsThrEntry {
        line_lower: 161,
        line_higher: 164,
        threshold_db: 4.20,
    },
    AbsThrEntry {
        line_lower: 165,
        line_higher: 168,
        threshold_db: 4.54,
    },
    AbsThrEntry {
        line_lower: 169,
        line_higher: 172,
        threshold_db: 4.91,
    },
    AbsThrEntry {
        line_lower: 173,
        line_higher: 176,
        threshold_db: 5.31,
    },
    AbsThrEntry {
        line_lower: 177,
        line_higher: 180,
        threshold_db: 5.73,
    },
    AbsThrEntry {
        line_lower: 181,
        line_higher: 184,
        threshold_db: 6.18,
    },
    AbsThrEntry {
        line_lower: 185,
        line_higher: 188,
        threshold_db: 6.67,
    },
    AbsThrEntry {
        line_lower: 189,
        line_higher: 192,
        threshold_db: 7.19,
    },
    AbsThrEntry {
        line_lower: 193,
        line_higher: 200,
        threshold_db: 8.33,
    },
    AbsThrEntry {
        line_lower: 201,
        line_higher: 208,
        threshold_db: 9.63,
    },
    AbsThrEntry {
        line_lower: 209,
        line_higher: 216,
        threshold_db: 11.08,
    },
    AbsThrEntry {
        line_lower: 217,
        line_higher: 224,
        threshold_db: 12.71,
    },
    AbsThrEntry {
        line_lower: 225,
        line_higher: 232,
        threshold_db: 14.53,
    },
    AbsThrEntry {
        line_lower: 233,
        line_higher: 240,
        threshold_db: 16.54,
    },
    AbsThrEntry {
        line_lower: 241,
        line_higher: 248,
        threshold_db: 18.77,
    },
    AbsThrEntry {
        line_lower: 249,
        line_higher: 256,
        threshold_db: 21.23,
    },
    AbsThrEntry {
        line_lower: 257,
        line_higher: 264,
        threshold_db: 23.94,
    },
    AbsThrEntry {
        line_lower: 265,
        line_higher: 272,
        threshold_db: 26.90,
    },
    AbsThrEntry {
        line_lower: 273,
        line_higher: 280,
        threshold_db: 30.14,
    },
    AbsThrEntry {
        line_lower: 281,
        line_higher: 288,
        threshold_db: 33.67,
    },
    AbsThrEntry {
        line_lower: 289,
        line_higher: 296,
        threshold_db: 37.51,
    },
    AbsThrEntry {
        line_lower: 297,
        line_higher: 304,
        threshold_db: 41.67,
    },
    AbsThrEntry {
        line_lower: 305,
        line_higher: 312,
        threshold_db: 46.17,
    },
    AbsThrEntry {
        line_lower: 313,
        line_higher: 320,
        threshold_db: 51.04,
    },
    AbsThrEntry {
        line_lower: 321,
        line_higher: 328,
        threshold_db: 56.29,
    },
    AbsThrEntry {
        line_lower: 329,
        line_higher: 336,
        threshold_db: 61.94,
    },
    AbsThrEntry {
        line_lower: 337,
        line_higher: 344,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 345,
        line_higher: 352,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 353,
        line_higher: 360,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 361,
        line_higher: 368,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 369,
        line_higher: 376,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 377,
        line_higher: 384,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 385,
        line_higher: 392,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 393,
        line_higher: 400,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 401,
        line_higher: 408,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 409,
        line_higher: 416,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 417,
        line_higher: 424,
        threshold_db: 68.00,
    },
    AbsThrEntry {
        line_lower: 425,
        line_higher: 432,
        threshold_db: 68.00,
    },
];

/// Returns the Model-2 absolute-threshold table (Annex D Table D.4a /
/// D.4b / D.4c) for the given sampling rate.
///
/// This is the per-rate dispatcher for the clause D.2.4 step-(l)
/// absolute-threshold floor: [`absolute_threshold_db_per_line`] expands
/// the chosen table into a per-FFT-line dB slice, and the caller converts
/// that into the energy domain before handing it to
/// [`include_absolute_threshold`].
///
/// The three Layer II sampling rates tabulate **132 / 130 / 126** entries
/// respectively, the ranges topping out at FFT lines **480 / 464 / 432**
/// (the audio band of interest at each rate; the remaining lines up to
/// the Nyquist line 513 are above every tabulated range — see
/// [`absolute_threshold_db_per_line`] for how those trailing lines are
/// filled).
#[must_use]
pub fn abs_threshold_table_for_rate(rate: SamplingRate) -> &'static [AbsThrEntry] {
    match rate {
        SamplingRate::Fs32kHz => &TABLE_D_4A_ABSTHR_LAYER2_32,
        SamplingRate::Fs44k1Hz => &TABLE_D_4B_ABSTHR_LAYER2_44K1,
        SamplingRate::Fs48kHz => &TABLE_D_4C_ABSTHR_LAYER2_48,
    }
}

/// Expands an Annex D Table D.4 absolute-threshold table into a
/// per-FFT-line threshold-in-dB slice over the 1024-point-analysis-FFT
/// working range (FFT lines `1 ..= line_count`, returned 0-based so
/// `out[ω − 1]` is the threshold of FFT line `ω`).
///
/// Each table row `(line_lower ..= line_higher, threshold_db)` broadcasts
/// its single `threshold_db` across every FFT line in its inclusive
/// range — the spec shares one absolute-threshold value across a range of
/// lines exactly as Table D.4 is printed. The output length is
/// `line_count` (pass `513` to cover the full analysis FFT through the
/// Nyquist line so the result lines up with
/// [`line_energy_threshold`]'s per-line buffer).
///
/// **Trailing lines.** The D.4 tables top out below the Nyquist line
/// (480 / 464 / 432 at 32 / 44,1 / 48 kHz). FFT lines above the last
/// tabulated range have no spec-provided threshold; this function fills
/// them by holding the **last tabulated `threshold_db`** (the top-of-band
/// ceiling — `51.03` / `69.13` / `68.00` dB respectively). The held
/// ceiling is the most conservative (highest) absolute threshold in each
/// table, so the step-(l) `max(nb_ω, absthr_ω)` floor remains a valid
/// audibility floor for those lines; the caller's energy-domain
/// conversion applies uniformly.
///
/// The result is still in **dB** — the caller performs the
/// dB→energy conversion (per the table's `0 dB = 96 dB below a ±32 760
/// sine`) before calling [`include_absolute_threshold`]. An empty input
/// table, or `line_count == 0`, yields an empty vector.
#[must_use]
pub fn absolute_threshold_db_per_line(table: &[AbsThrEntry], line_count: usize) -> Vec<f64> {
    if table.is_empty() || line_count == 0 {
        return Vec::new();
    }
    // Hold the top-of-band ceiling for any FFT line above the last
    // tabulated range.
    let ceiling = table[table.len() - 1].threshold_db;
    let mut out = vec![ceiling; line_count];
    for entry in table {
        // 1-based inclusive [line_lower, line_higher] → 0-based buffer.
        let lo = entry.line_lower as usize;
        let hi = entry.line_higher as usize;
        for omega in lo..=hi {
            if omega >= 1 && omega <= line_count {
                out[omega - 1] = entry.threshold_db;
            }
        }
    }
    out
}

/// ISO/IEC 13818-3:1997 Annex D Table **D.3.a** — Model-2 calculation
/// partitions, Fs = 24 kHz ("long blocks"). 60 partitions covering
/// FFT lines 1–491 of the 1024-point analysis FFT (the table tops out
/// just below the Nyquist line, unlike the 11172-3 Layer II tables
/// which run to 513; step-(l) keeps the uncovered top lines floored
/// at the absolute threshold).
///
/// Column derivation from the printed long-block layout
/// (`no. / FFT-lines / minval / qthr / norm / bval`) into the
/// Layer I/II [`CalcPartition`] form:
///
/// * `omega_low` / `omega_high` — running cumulative sum of the
///   printed per-partition `FFT-lines` counts (1-based; partition 0
///   starts at line 1). Contiguity and the coverage total are
///   test-pinned.
/// * `bval`, `minval` — printed verbatim.
/// * `tmn` — the long-block layout prints no TMN column; the value is
///   derived by the relation `TMN = max(24,5, bval + 14,5) dB`
///   (rounded half-up to one decimal), which reproduces the printed
///   TMN column of **all 164 partitions** of the 11172-3 Layer II
///   Tables D.3a/b/c from their printed bval to within the 1-decimal
///   print rounding (pinned by
///   `tmn_relation_reproduces_mpeg1_printed_column`). The LSF bval
///   column carries 9 significant digits, so the derived value is
///   rounding-stable.
/// * `qthr` / `norm` — not carried: they serve the Layer III variant
///   of the threshold loop (`norm` replaces the [`rnorm_coefficient`]
///   computation; `qthr` is the per-partition threshold in quiet).
///   The Layer I/II §D.2.4 chain computes `rnorm` from the spreading
///   convolution and takes its step-(l) absolute threshold per FFT
///   line — served for the LSF rates by the D.1-derived
///   [`lsf_abs_threshold_layer2_24`].
///
/// Source: the staged ISO/IEC 13818-3:1997 PDF
/// (`docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`, SHA-256
/// `25ebf438988fced761b79adcb108c0a59acc68a0f38be36017c334abb8582df5`,
/// Table D.3.a at printed page 101).
pub static TABLE_LSF_D_3A_CALC_PARTITION_24KHZ: [CalcPartition; 60] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 2,
        bval: 0.236874461,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 3,
        omega_high: 4,
        bval: 0.71016103,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 5,
        omega_high: 6,
        bval: 1.18193281,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 7,
        omega_high: 8,
        bval: 1.65102732,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 9,
        omega_high: 10,
        bval: 2.11632562,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 11,
        omega_high: 12,
        bval: 2.57676744,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 13,
        omega_high: 14,
        bval: 3.03136396,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 15,
        omega_high: 16,
        bval: 3.47920918,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 17,
        omega_high: 18,
        bval: 3.91948748,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 19,
        omega_high: 20,
        bval: 4.35147953,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 21,
        omega_high: 22,
        bval: 4.77456427,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 23,
        omega_high: 24,
        bval: 5.18822002,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 25,
        omega_high: 26,
        bval: 5.59202194,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 27,
        omega_high: 28,
        bval: 5.98564005,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 29,
        omega_high: 30,
        bval: 6.36883163,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 31,
        omega_high: 32,
        bval: 6.74143791,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 33,
        omega_high: 34,
        bval: 7.10337448,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 35,
        omega_high: 36,
        bval: 7.45462418,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 37,
        omega_high: 38,
        bval: 7.79523182,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 39,
        omega_high: 41,
        bval: 8.20455742,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 42,
        omega_high: 44,
        bval: 8.67640114,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 45,
        omega_high: 47,
        bval: 9.12561035,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 48,
        omega_high: 50,
        bval: 9.55298138,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 51,
        omega_high: 53,
        bval: 9.95940971,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 54,
        omega_high: 56,
        bval: 10.3458519,
        minval: 0.0,
        tmn: 24.8,
    },
    CalcPartition {
        omega_low: 57,
        omega_high: 59,
        bval: 10.7132998,
        minval: 0.0,
        tmn: 25.2,
    },
    CalcPartition {
        omega_low: 60,
        omega_high: 62,
        bval: 11.0627575,
        minval: 0.0,
        tmn: 25.6,
    },
    CalcPartition {
        omega_low: 63,
        omega_high: 66,
        bval: 11.447506,
        minval: 0.0,
        tmn: 25.9,
    },
    CalcPartition {
        omega_low: 67,
        omega_high: 70,
        bval: 11.8627586,
        minval: 0.0,
        tmn: 26.4,
    },
    CalcPartition {
        omega_low: 71,
        omega_high: 74,
        bval: 12.2520256,
        minval: 0.0,
        tmn: 26.8,
    },
    CalcPartition {
        omega_low: 75,
        omega_high: 78,
        bval: 12.6173973,
        minval: 0.0,
        tmn: 27.1,
    },
    CalcPartition {
        omega_low: 79,
        omega_high: 82,
        bval: 12.9608269,
        minval: 0.0,
        tmn: 27.5,
    },
    CalcPartition {
        omega_low: 83,
        omega_high: 87,
        bval: 13.3219252,
        minval: 0.0,
        tmn: 27.8,
    },
    CalcPartition {
        omega_low: 88,
        omega_high: 92,
        bval: 13.6976833,
        minval: 0.0,
        tmn: 28.2,
    },
    CalcPartition {
        omega_low: 93,
        omega_high: 97,
        bval: 14.047802,
        minval: 0.0,
        tmn: 28.5,
    },
    CalcPartition {
        omega_low: 98,
        omega_high: 103,
        bval: 14.405302,
        minval: 0.0,
        tmn: 28.9,
    },
    CalcPartition {
        omega_low: 104,
        omega_high: 109,
        bval: 14.7684803,
        minval: 0.0,
        tmn: 29.3,
    },
    CalcPartition {
        omega_low: 110,
        omega_high: 116,
        bval: 15.1315956,
        minval: 0.0,
        tmn: 29.6,
    },
    CalcPartition {
        omega_low: 117,
        omega_high: 123,
        bval: 15.4940481,
        minval: 0.0,
        tmn: 30.0,
    },
    CalcPartition {
        omega_low: 124,
        omega_high: 131,
        bval: 15.8516159,
        minval: 0.0,
        tmn: 30.4,
    },
    CalcPartition {
        omega_low: 132,
        omega_high: 139,
        bval: 16.204628,
        minval: 0.0,
        tmn: 30.7,
    },
    CalcPartition {
        omega_low: 140,
        omega_high: 148,
        bval: 16.5502281,
        minval: 0.0,
        tmn: 31.1,
    },
    CalcPartition {
        omega_low: 149,
        omega_high: 158,
        bval: 16.9067478,
        minval: 0.0,
        tmn: 31.4,
    },
    CalcPartition {
        omega_low: 159,
        omega_high: 168,
        bval: 17.2537231,
        minval: 0.0,
        tmn: 31.8,
    },
    CalcPartition {
        omega_low: 169,
        omega_high: 179,
        bval: 17.5901108,
        minval: 0.0,
        tmn: 32.1,
    },
    CalcPartition {
        omega_low: 180,
        omega_high: 191,
        bval: 17.931406,
        minval: 0.0,
        tmn: 32.4,
    },
    CalcPartition {
        omega_low: 192,
        omega_high: 204,
        bval: 18.2750721,
        minval: 0.0,
        tmn: 32.8,
    },
    CalcPartition {
        omega_low: 205,
        omega_high: 218,
        bval: 18.6191597,
        minval: 0.0,
        tmn: 33.1,
    },
    CalcPartition {
        omega_low: 219,
        omega_high: 233,
        bval: 18.9621754,
        minval: 0.0,
        tmn: 33.5,
    },
    CalcPartition {
        omega_low: 234,
        omega_high: 249,
        bval: 19.3029613,
        minval: 0.0,
        tmn: 33.8,
    },
    CalcPartition {
        omega_low: 250,
        omega_high: 266,
        bval: 19.6405869,
        minval: 0.0,
        tmn: 34.1,
    },
    CalcPartition {
        omega_low: 267,
        omega_high: 284,
        bval: 19.9742699,
        minval: 0.0,
        tmn: 34.5,
    },
    CalcPartition {
        omega_low: 285,
        omega_high: 304,
        bval: 20.3115921,
        minval: 0.0,
        tmn: 34.8,
    },
    CalcPartition {
        omega_low: 305,
        omega_high: 325,
        bval: 20.6507797,
        minval: 0.0,
        tmn: 35.2,
    },
    CalcPartition {
        omega_low: 326,
        omega_high: 348,
        bval: 20.9890823,
        minval: 0.0,
        tmn: 35.5,
    },
    CalcPartition {
        omega_low: 349,
        omega_high: 372,
        bval: 21.3251152,
        minval: 0.0,
        tmn: 35.8,
    },
    CalcPartition {
        omega_low: 373,
        omega_high: 398,
        bval: 21.6565971,
        minval: 0.0,
        tmn: 36.2,
    },
    CalcPartition {
        omega_low: 399,
        omega_high: 426,
        bval: 21.9881554,
        minval: 0.0,
        tmn: 36.5,
    },
    CalcPartition {
        omega_low: 427,
        omega_high: 457,
        bval: 22.3222847,
        minval: 0.0,
        tmn: 36.8,
    },
    CalcPartition {
        omega_low: 458,
        omega_high: 491,
        bval: 22.6605186,
        minval: 0.0,
        tmn: 37.2,
    },
];

/// ISO/IEC 13818-3:1997 Annex D Table **D.3.b** — Model-2 calculation
/// partitions, Fs = 22,05 kHz ("long blocks"). 60 partitions covering
/// FFT lines 1–489.
///
/// Column derivation from the printed long-block layout
/// (`no. / FFT-lines / minval / qthr / norm / bval`) into the
/// Layer I/II [`CalcPartition`] form:
///
/// * `omega_low` / `omega_high` — running cumulative sum of the
///   printed per-partition `FFT-lines` counts (1-based; partition 0
///   starts at line 1). Contiguity and the coverage total are
///   test-pinned.
/// * `bval`, `minval` — printed verbatim.
/// * `tmn` — the long-block layout prints no TMN column; the value is
///   derived by the relation `TMN = max(24,5, bval + 14,5) dB`
///   (rounded half-up to one decimal), which reproduces the printed
///   TMN column of **all 164 partitions** of the 11172-3 Layer II
///   Tables D.3a/b/c from their printed bval to within the 1-decimal
///   print rounding (pinned by
///   `tmn_relation_reproduces_mpeg1_printed_column`). The LSF bval
///   column carries 9 significant digits, so the derived value is
///   rounding-stable.
/// * `qthr` / `norm` — not carried: they serve the Layer III variant
///   of the threshold loop (`norm` replaces the [`rnorm_coefficient`]
///   computation; `qthr` is the per-partition threshold in quiet).
///   The Layer I/II §D.2.4 chain computes `rnorm` from the spreading
///   convolution and takes its step-(l) absolute threshold per FFT
///   line — served for the LSF rates by the D.1-derived
///   [`lsf_abs_threshold_layer2_22k05`].
///
/// Source: the staged 13818-3 PDF (same hash as Table D.3.a), Table
/// D.3.b at printed page 102.
pub static TABLE_LSF_D_3B_CALC_PARTITION_22K05HZ: [CalcPartition; 60] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 2,
        bval: 0.217637643,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 3,
        omega_high: 4,
        bval: 0.652563453,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 5,
        omega_high: 6,
        bval: 1.08633137,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 7,
        omega_high: 8,
        bval: 1.51803517,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 9,
        omega_high: 10,
        bval: 1.94679713,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 11,
        omega_high: 12,
        bval: 2.37177849,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 13,
        omega_high: 14,
        bval: 2.79218864,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 15,
        omega_high: 16,
        bval: 3.20729256,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 17,
        omega_high: 18,
        bval: 3.61641645,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 19,
        omega_high: 20,
        bval: 4.01895428,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 21,
        omega_high: 22,
        bval: 4.4143672,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 23,
        omega_high: 24,
        bval: 4.80218887,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 25,
        omega_high: 26,
        bval: 5.18202305,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 27,
        omega_high: 28,
        bval: 5.55354261,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 29,
        omega_high: 30,
        bval: 5.91648674,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 31,
        omega_high: 32,
        bval: 6.27065945,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 33,
        omega_high: 34,
        bval: 6.61592293,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 35,
        omega_high: 36,
        bval: 6.95219517,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 37,
        omega_high: 39,
        bval: 7.3584404,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 40,
        omega_high: 42,
        bval: 7.8290925,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 43,
        omega_high: 45,
        bval: 8.27975655,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 46,
        omega_high: 48,
        bval: 8.71083069,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 49,
        omega_high: 51,
        bval: 9.12284088,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 52,
        omega_high: 54,
        bval: 9.51640987,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 55,
        omega_high: 57,
        bval: 9.89222908,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 58,
        omega_high: 60,
        bval: 10.2510386,
        minval: 0.0,
        tmn: 24.8,
    },
    CalcPartition {
        omega_low: 61,
        omega_high: 63,
        bval: 10.5936022,
        minval: 0.0,
        tmn: 25.1,
    },
    CalcPartition {
        omega_low: 64,
        omega_high: 67,
        bval: 10.9723492,
        minval: 0.0,
        tmn: 25.5,
    },
    CalcPartition {
        omega_low: 68,
        omega_high: 71,
        bval: 11.38272,
        minval: 0.0,
        tmn: 25.9,
    },
    CalcPartition {
        omega_low: 72,
        omega_high: 75,
        bval: 11.7689981,
        minval: 0.0,
        tmn: 26.3,
    },
    CalcPartition {
        omega_low: 76,
        omega_high: 79,
        bval: 12.1329184,
        minval: 0.0,
        tmn: 26.6,
    },
    CalcPartition {
        omega_low: 80,
        omega_high: 83,
        bval: 12.4761295,
        minval: 0.0,
        tmn: 27.0,
    },
    CalcPartition {
        omega_low: 84,
        omega_high: 88,
        bval: 12.8381901,
        minval: 0.0,
        tmn: 27.3,
    },
    CalcPartition {
        omega_low: 89,
        omega_high: 93,
        bval: 13.2160273,
        minval: 0.0,
        tmn: 27.7,
    },
    CalcPartition {
        omega_low: 94,
        omega_high: 98,
        bval: 13.5690479,
        minval: 0.0,
        tmn: 28.1,
    },
    CalcPartition {
        omega_low: 99,
        omega_high: 104,
        bval: 13.9303951,
        minval: 0.0,
        tmn: 28.4,
    },
    CalcPartition {
        omega_low: 105,
        omega_high: 110,
        bval: 14.298193,
        minval: 0.0,
        tmn: 28.8,
    },
    CalcPartition {
        omega_low: 111,
        omega_high: 117,
        bval: 14.666563,
        minval: 0.0,
        tmn: 29.2,
    },
    CalcPartition {
        omega_low: 118,
        omega_high: 124,
        bval: 15.0346909,
        minval: 0.0,
        tmn: 29.5,
    },
    CalcPartition {
        omega_low: 125,
        omega_high: 132,
        bval: 15.398139,
        minval: 0.0,
        tmn: 29.9,
    },
    CalcPartition {
        omega_low: 133,
        omega_high: 140,
        bval: 15.7570457,
        minval: 0.0,
        tmn: 30.3,
    },
    CalcPartition {
        omega_low: 141,
        omega_high: 149,
        bval: 16.1083431,
        minval: 0.0,
        tmn: 30.6,
    },
    CalcPartition {
        omega_low: 150,
        omega_high: 158,
        bval: 16.4528522,
        minval: 0.0,
        tmn: 31.0,
    },
    CalcPartition {
        omega_low: 159,
        omega_high: 168,
        bval: 16.7886105,
        minval: 0.0,
        tmn: 31.3,
    },
    CalcPartition {
        omega_low: 169,
        omega_high: 179,
        bval: 17.132,
        minval: 0.0,
        tmn: 31.6,
    },
    CalcPartition {
        omega_low: 180,
        omega_high: 191,
        bval: 17.4796028,
        minval: 0.0,
        tmn: 32.0,
    },
    CalcPartition {
        omega_low: 192,
        omega_high: 204,
        bval: 17.8287659,
        minval: 0.0,
        tmn: 32.3,
    },
    CalcPartition {
        omega_low: 205,
        omega_high: 218,
        bval: 18.1774921,
        minval: 0.0,
        tmn: 32.7,
    },
    CalcPartition {
        omega_low: 219,
        omega_high: 233,
        bval: 18.5243168,
        minval: 0.0,
        tmn: 33.0,
    },
    CalcPartition {
        omega_low: 234,
        omega_high: 249,
        bval: 18.8681736,
        minval: 0.0,
        tmn: 33.4,
    },
    CalcPartition {
        omega_low: 250,
        omega_high: 266,
        bval: 19.2082729,
        minval: 0.0,
        tmn: 33.7,
    },
    CalcPartition {
        omega_low: 267,
        omega_high: 284,
        bval: 19.5440025,
        minval: 0.0,
        tmn: 34.0,
    },
    CalcPartition {
        omega_low: 285,
        omega_high: 304,
        bval: 19.8831882,
        minval: 0.0,
        tmn: 34.4,
    },
    CalcPartition {
        omega_low: 305,
        omega_high: 325,
        bval: 20.224247,
        minval: 0.0,
        tmn: 34.7,
    },
    CalcPartition {
        omega_low: 326,
        omega_high: 348,
        bval: 20.5646286,
        minval: 0.0,
        tmn: 35.1,
    },
    CalcPartition {
        omega_low: 349,
        omega_high: 372,
        bval: 20.903141,
        minval: 0.0,
        tmn: 35.4,
    },
    CalcPartition {
        omega_low: 373,
        omega_high: 398,
        bval: 21.2376747,
        minval: 0.0,
        tmn: 35.7,
    },
    CalcPartition {
        omega_low: 399,
        omega_high: 426,
        bval: 21.5730591,
        minval: 0.0,
        tmn: 36.1,
    },
    CalcPartition {
        omega_low: 427,
        omega_high: 456,
        bval: 21.9066811,
        minval: 0.0,
        tmn: 36.4,
    },
    CalcPartition {
        omega_low: 457,
        omega_high: 489,
        bval: 22.2411156,
        minval: 0.0,
        tmn: 36.7,
    },
];

/// ISO/IEC 13818-3:1997 Annex D Table **D.3.c** — Model-2 calculation
/// partitions, Fs = 16 kHz ("long blocks"). 56 partitions covering
/// FFT lines 1–509.
///
/// Column derivation from the printed long-block layout
/// (`no. / FFT-lines / minval / qthr / norm / bval`) into the
/// Layer I/II [`CalcPartition`] form:
///
/// * `omega_low` / `omega_high` — running cumulative sum of the
///   printed per-partition `FFT-lines` counts (1-based; partition 0
///   starts at line 1). Contiguity and the coverage total are
///   test-pinned.
/// * `bval`, `minval` — printed verbatim.
/// * `tmn` — the long-block layout prints no TMN column; the value is
///   derived by the relation `TMN = max(24,5, bval + 14,5) dB`
///   (rounded half-up to one decimal), which reproduces the printed
///   TMN column of **all 164 partitions** of the 11172-3 Layer II
///   Tables D.3a/b/c from their printed bval to within the 1-decimal
///   print rounding (pinned by
///   `tmn_relation_reproduces_mpeg1_printed_column`). The LSF bval
///   column carries 9 significant digits, so the derived value is
///   rounding-stable.
/// * `qthr` / `norm` — not carried: they serve the Layer III variant
///   of the threshold loop (`norm` replaces the [`rnorm_coefficient`]
///   computation; `qthr` is the per-partition threshold in quiet).
///   The Layer I/II §D.2.4 chain computes `rnorm` from the spreading
///   convolution and takes its step-(l) absolute threshold per FFT
///   line — served for the LSF rates by the D.1-derived
///   [`lsf_abs_threshold_layer2_16`].
///
/// Source: the staged 13818-3 PDF (same hash as Table D.3.a), Table
/// D.3.c at printed page 103.
pub static TABLE_LSF_D_3C_CALC_PARTITION_16KHZ: [CalcPartition; 56] = [
    CalcPartition {
        omega_low: 1,
        omega_high: 3,
        bval: 0.236874461,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 4,
        omega_high: 6,
        bval: 0.71016103,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 7,
        omega_high: 9,
        bval: 1.18193281,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 10,
        omega_high: 12,
        bval: 1.65102732,
        minval: 15.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 13,
        omega_high: 15,
        bval: 2.11632562,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 16,
        omega_high: 18,
        bval: 2.57676744,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 19,
        omega_high: 21,
        bval: 3.03136396,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 22,
        omega_high: 24,
        bval: 3.47920918,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 25,
        omega_high: 27,
        bval: 3.91948748,
        minval: 13.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 28,
        omega_high: 30,
        bval: 4.35147953,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 31,
        omega_high: 33,
        bval: 4.77456427,
        minval: 12.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 34,
        omega_high: 36,
        bval: 5.18822002,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 37,
        omega_high: 39,
        bval: 5.59202194,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 40,
        omega_high: 42,
        bval: 5.98564005,
        minval: 10.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 43,
        omega_high: 45,
        bval: 6.36883163,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 46,
        omega_high: 48,
        bval: 6.74143791,
        minval: 9.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 49,
        omega_high: 51,
        bval: 7.10337448,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 52,
        omega_high: 54,
        bval: 7.45462418,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 55,
        omega_high: 57,
        bval: 7.79523182,
        minval: 6.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 58,
        omega_high: 61,
        bval: 8.17827797,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 62,
        omega_high: 65,
        bval: 8.59994984,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 66,
        omega_high: 69,
        bval: 9.00363636,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 70,
        omega_high: 73,
        bval: 9.38988018,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 74,
        omega_high: 77,
        bval: 9.7592926,
        minval: 3.0,
        tmn: 24.5,
    },
    CalcPartition {
        omega_low: 78,
        omega_high: 81,
        bval: 10.1125278,
        minval: 0.0,
        tmn: 24.6,
    },
    CalcPartition {
        omega_low: 82,
        omega_high: 85,
        bval: 10.4502735,
        minval: 0.0,
        tmn: 25.0,
    },
    CalcPartition {
        omega_low: 86,
        omega_high: 90,
        bval: 10.811614,
        minval: 0.0,
        tmn: 25.3,
    },
    CalcPartition {
        omega_low: 91,
        omega_high: 95,
        bval: 11.1935263,
        minval: 0.0,
        tmn: 25.7,
    },
    CalcPartition {
        omega_low: 96,
        omega_high: 100,
        bval: 11.5549288,
        minval: 0.0,
        tmn: 26.1,
    },
    CalcPartition {
        omega_low: 101,
        omega_high: 105,
        bval: 11.8971443,
        minval: 0.0,
        tmn: 26.4,
    },
    CalcPartition {
        omega_low: 106,
        omega_high: 111,
        bval: 12.2520256,
        minval: 0.0,
        tmn: 26.8,
    },
    CalcPartition {
        omega_low: 112,
        omega_high: 117,
        bval: 12.6173973,
        minval: 0.0,
        tmn: 27.1,
    },
    CalcPartition {
        omega_low: 118,
        omega_high: 123,
        bval: 12.9608269,
        minval: 0.0,
        tmn: 27.5,
    },
    CalcPartition {
        omega_low: 124,
        omega_high: 130,
        bval: 13.3093863,
        minval: 0.0,
        tmn: 27.8,
    },
    CalcPartition {
        omega_low: 131,
        omega_high: 137,
        bval: 13.6617231,
        minval: 0.0,
        tmn: 28.2,
    },
    CalcPartition {
        omega_low: 138,
        omega_high: 145,
        bval: 14.0134668,
        minval: 0.0,
        tmn: 28.5,
    },
    CalcPartition {
        omega_low: 146,
        omega_high: 153,
        bval: 14.3639784,
        minval: 0.0,
        tmn: 28.9,
    },
    CalcPartition {
        omega_low: 154,
        omega_high: 162,
        bval: 14.7098465,
        minval: 0.0,
        tmn: 29.2,
    },
    CalcPartition {
        omega_low: 163,
        omega_high: 172,
        bval: 15.0686541,
        minval: 0.0,
        tmn: 29.6,
    },
    CalcPartition {
        omega_low: 173,
        omega_high: 182,
        bval: 15.4191036,
        minval: 0.0,
        tmn: 29.9,
    },
    CalcPartition {
        omega_low: 183,
        omega_high: 193,
        bval: 15.7594051,
        minval: 0.0,
        tmn: 30.3,
    },
    CalcPartition {
        omega_low: 194,
        omega_high: 205,
        bval: 16.104557,
        minval: 0.0,
        tmn: 30.6,
    },
    CalcPartition {
        omega_low: 206,
        omega_high: 218,
        bval: 16.451416,
        minval: 0.0,
        tmn: 31.0,
    },
    CalcPartition {
        omega_low: 219,
        omega_high: 232,
        bval: 16.7975388,
        minval: 0.0,
        tmn: 31.3,
    },
    CalcPartition {
        omega_low: 233,
        omega_high: 247,
        bval: 17.1411018,
        minval: 0.0,
        tmn: 31.6,
    },
    CalcPartition {
        omega_low: 248,
        omega_high: 263,
        bval: 17.4808159,
        minval: 0.0,
        tmn: 32.0,
    },
    CalcPartition {
        omega_low: 264,
        omega_high: 280,
        bval: 17.8158207,
        minval: 0.0,
        tmn: 32.3,
    },
    CalcPartition {
        omega_low: 281,
        omega_high: 299,
        bval: 18.1543369,
        minval: 0.0,
        tmn: 32.7,
    },
    CalcPartition {
        omega_low: 300,
        omega_high: 319,
        bval: 18.4948578,
        minval: 0.0,
        tmn: 33.0,
    },
    CalcPartition {
        omega_low: 320,
        omega_high: 341,
        bval: 18.8350143,
        minval: 0.0,
        tmn: 33.3,
    },
    CalcPartition {
        omega_low: 342,
        omega_high: 364,
        bval: 19.1740704,
        minval: 0.0,
        tmn: 33.7,
    },
    CalcPartition {
        omega_low: 365,
        omega_high: 389,
        bval: 19.5104179,
        minval: 0.0,
        tmn: 34.0,
    },
    CalcPartition {
        omega_low: 390,
        omega_high: 416,
        bval: 19.8497677,
        minval: 0.0,
        tmn: 34.3,
    },
    CalcPartition {
        omega_low: 417,
        omega_high: 445,
        bval: 20.1900635,
        minval: 0.0,
        tmn: 34.7,
    },
    CalcPartition {
        omega_low: 446,
        omega_high: 476,
        bval: 20.5294952,
        minval: 0.0,
        tmn: 35.0,
    },
    CalcPartition {
        omega_low: 477,
        omega_high: 509,
        bval: 20.8664398,
        minval: 0.0,
        tmn: 35.4,
    },
];

/// Builds the step-(l) per-FFT-line absolute-threshold table for one
/// LSF rate from the corresponding 13818-3 Table D.1 transcription in
/// [`crate::tables_lsf`].
///
/// 13818-3 prints no Model-2 D.4-style absolute-threshold table for
/// the LSF rates. At the MPEG-1 rates the 11172-3 D.4 tables match
/// the Layer II D.1 threshold-in-quiet column entry-for-entry (see
/// the module docs), so the LSF D.1d/e/f `Absolute Thresh. [dB]`
/// column is the same data in the same dB convention; each D.1 entry
/// becomes one [`AbsThrEntry`] whose line range is the entry's
/// implicit `(prev_top + 1 ..= top)` run.
fn absthr_from_lsf_d1(d1: &'static [crate::tables_d2::LtqEntry]) -> Vec<AbsThrEntry> {
    let mut out = Vec::with_capacity(d1.len());
    let mut lo = 1_u32;
    for entry in d1 {
        out.push(AbsThrEntry {
            line_lower: lo,
            line_higher: entry.top_line_index,
            threshold_db: entry.threshold_db,
        });
        lo = entry.top_line_index + 1;
    }
    out
}

/// Step-(l) absolute-threshold table for Fs = 16 kHz, derived from
/// the 13818-3 Table D.1d transcription (see [`absthr_from_lsf_d1`]).
#[must_use]
pub fn lsf_abs_threshold_layer2_16() -> &'static [AbsThrEntry] {
    static T: std::sync::OnceLock<Vec<AbsThrEntry>> = std::sync::OnceLock::new();
    T.get_or_init(|| absthr_from_lsf_d1(&crate::tables_lsf::TABLE_LSF_D_1D_LTQ_LAYER_II_16))
}

/// Step-(l) absolute-threshold table for Fs = 22,05 kHz, derived from
/// the 13818-3 Table D.1e transcription (see [`absthr_from_lsf_d1`]).
#[must_use]
pub fn lsf_abs_threshold_layer2_22k05() -> &'static [AbsThrEntry] {
    static T: std::sync::OnceLock<Vec<AbsThrEntry>> = std::sync::OnceLock::new();
    T.get_or_init(|| absthr_from_lsf_d1(&crate::tables_lsf::TABLE_LSF_D_1E_LTQ_LAYER_II_22K05))
}

/// Step-(l) absolute-threshold table for Fs = 24 kHz, derived from
/// the 13818-3 Table D.1f transcription (see [`absthr_from_lsf_d1`]).
#[must_use]
pub fn lsf_abs_threshold_layer2_24() -> &'static [AbsThrEntry] {
    static T: std::sync::OnceLock<Vec<AbsThrEntry>> = std::sync::OnceLock::new();
    T.get_or_init(|| absthr_from_lsf_d1(&crate::tables_lsf::TABLE_LSF_D_1F_LTQ_LAYER_II_24))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_d3a_has_49_partitions() {
        assert_eq!(TABLE_D_3A_CALC_PARTITION_32KHZ.len(), 49);
    }

    #[test]
    fn table_d3a_partitions_are_contiguous_and_cover_to_nyquist() {
        // The partitions must tile the FFT-line axis with no gaps or
        // overlaps: each partition's ωlow is exactly one past the
        // previous ωhigh, starting at line 1.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        assert_eq!(
            table[0].omega_low, 1,
            "first partition starts at FFT line 1"
        );
        for w in table.windows(2) {
            assert_eq!(
                w[1].omega_low,
                w[0].omega_high + 1,
                "partition boundaries must be contiguous"
            );
        }
        // Last partition reaches the Nyquist line of the 1024-point FFT.
        assert_eq!(
            table[48].omega_high, 513,
            "last ωhigh is the Nyquist line 513"
        );
    }

    #[test]
    fn table_d3a_bval_monotonic_nondecreasing() {
        // The median Bark value rises across the partitions (Bark is a
        // monotone function of frequency, and partitions ascend in
        // frequency).
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        for w in table.windows(2) {
            assert!(
                w[1].bval >= w[0].bval,
                "bval must be non-decreasing: {} then {}",
                w[0].bval,
                w[1].bval
            );
        }
        assert_eq!(table[0].bval, 0.00);
        assert_eq!(table[48].bval, 24.07);
    }

    #[test]
    fn table_d3a_minval_settles_to_4_5_from_partition_17() {
        // The extract notes "from partition 17 onward minval is a
        // constant 4,5 dB" (1-based index 17 == array index 16).
        for p in &TABLE_D_3A_CALC_PARTITION_32KHZ[16..] {
            assert_eq!(p.minval, 4.5, "minval is 4.5 dB from partition 17 onward");
        }
    }

    #[test]
    fn table_d3a_tmn_rises_monotonically_in_the_tail() {
        // From partition 15 onward (1-based), TMN rises monotonically.
        for w in TABLE_D_3A_CALC_PARTITION_32KHZ[14..].windows(2) {
            assert!(
                w[1].tmn >= w[0].tmn,
                "TMN must be non-decreasing in the tail: {} then {}",
                w[0].tmn,
                w[1].tmn
            );
        }
        assert_eq!(TABLE_D_3A_CALC_PARTITION_32KHZ[48].tmn, 38.6);
    }

    #[test]
    fn line_count_matches_omega_span() {
        let p = TABLE_D_3A_CALC_PARTITION_32KHZ[1]; // ωlow=2 ωhigh=4
        assert_eq!(p.line_count(), 3);
        assert_eq!(TABLE_D_3A_CALC_PARTITION_32KHZ[0].line_count(), 1);
        assert_eq!(TABLE_D_3A_CALC_PARTITION_32KHZ[48].line_count(), 17);
    }

    #[test]
    fn total_line_count_reaches_nyquist() {
        // Summing every partition's line count must equal 513 (lines
        // 1..=513), since the partitions tile the axis exactly.
        let total: u32 = TABLE_D_3A_CALC_PARTITION_32KHZ
            .iter()
            .map(|p| p.line_count())
            .sum();
        assert_eq!(total, 513);
    }

    #[test]
    fn table_d3b_d3c_have_expected_partition_counts() {
        assert_eq!(TABLE_D_3B_CALC_PARTITION_44K1HZ.len(), 57);
        assert_eq!(TABLE_D_3C_CALC_PARTITION_48KHZ.len(), 58);
    }

    #[test]
    fn table_d3b_d3c_are_contiguous_and_cover_to_nyquist() {
        for table in [
            &TABLE_D_3B_CALC_PARTITION_44K1HZ[..],
            &TABLE_D_3C_CALC_PARTITION_48KHZ[..],
        ] {
            assert_eq!(
                table[0].omega_low, 1,
                "first partition starts at FFT line 1"
            );
            for w in table.windows(2) {
                assert_eq!(
                    w[1].omega_low,
                    w[0].omega_high + 1,
                    "partition boundaries must be contiguous"
                );
            }
            assert_eq!(
                table[table.len() - 1].omega_high,
                513,
                "last ωhigh is the Nyquist line 513"
            );
            let total: u32 = table.iter().map(|p| p.line_count()).sum();
            assert_eq!(total, 513, "partitions must tile lines 1..=513 exactly");
        }
    }

    #[test]
    fn table_d3b_d3c_bval_monotonic_nondecreasing() {
        for table in [
            &TABLE_D_3B_CALC_PARTITION_44K1HZ[..],
            &TABLE_D_3C_CALC_PARTITION_48KHZ[..],
        ] {
            assert_eq!(table[0].bval, 0.00);
            for w in table.windows(2) {
                assert!(
                    w[1].bval >= w[0].bval,
                    "bval must be non-decreasing: {} then {}",
                    w[0].bval,
                    w[1].bval
                );
            }
        }
        // Highest-Bark partition of each table (last row of the CSV).
        assert_eq!(TABLE_D_3B_CALC_PARTITION_44K1HZ[56].bval, 25.33);
        assert_eq!(TABLE_D_3C_CALC_PARTITION_48KHZ[57].bval, 25.81);
    }

    #[test]
    fn table_d3b_d3c_low_partitions_are_single_lines() {
        // The finer-resolution 44,1 / 48 kHz tables open with a run of
        // single-FFT-line partitions: 16 for D.3b, 17 for D.3c.
        for p in &TABLE_D_3B_CALC_PARTITION_44K1HZ[..16] {
            assert_eq!(p.line_count(), 1);
        }
        assert!(TABLE_D_3B_CALC_PARTITION_44K1HZ[16].line_count() > 1);
        for p in &TABLE_D_3C_CALC_PARTITION_48KHZ[..17] {
            assert_eq!(p.line_count(), 1);
        }
        assert!(TABLE_D_3C_CALC_PARTITION_48KHZ[17].line_count() > 1);
    }

    #[test]
    fn table_d3b_d3c_spot_check_tail_cells() {
        // Verbatim final rows of the staged CSVs.
        let b = TABLE_D_3B_CALC_PARTITION_44K1HZ[56];
        assert_eq!((b.omega_low, b.omega_high), (470, 513));
        assert_eq!((b.minval, b.tmn), (3.5, 39.8));
        let c = TABLE_D_3C_CALC_PARTITION_48KHZ[57];
        assert_eq!((c.omega_low, c.omega_high), (508, 513));
        assert_eq!((c.minval, c.tmn), (3.5, 40.3));
    }

    #[test]
    fn calc_partition_table_for_rate_dispatches_each_rate() {
        assert_eq!(
            calc_partition_table_for_rate(SamplingRate::Fs32kHz).len(),
            49
        );
        assert_eq!(
            calc_partition_table_for_rate(SamplingRate::Fs44k1Hz).len(),
            57
        );
        assert_eq!(
            calc_partition_table_for_rate(SamplingRate::Fs48kHz).len(),
            58
        );
        // Each rate routes to its own table (content equality — the
        // tables are `const`, so the compiler may duplicate them and
        // pointer identity is not guaranteed).
        assert_eq!(
            calc_partition_table_for_rate(SamplingRate::Fs44k1Hz),
            &TABLE_D_3B_CALC_PARTITION_44K1HZ[..]
        );
        assert_eq!(
            calc_partition_table_for_rate(SamplingRate::Fs48kHz),
            &TABLE_D_3C_CALC_PARTITION_48KHZ[..]
        );
    }

    #[test]
    fn d3_threshold_loop_runs_for_all_three_rates() {
        // End-to-end smoke: the step-(f) spreading convolution must
        // accept the 44,1 / 48 kHz partition tables, not only 32 kHz.
        for rate in [
            SamplingRate::Fs32kHz,
            SamplingRate::Fs44k1Hz,
            SamplingRate::Fs48kHz,
        ] {
            let table = calc_partition_table_for_rate(rate);
            let energy = vec![1.0_f64; table.len()];
            let spread = convolve_partition_spreading(table, &energy);
            assert_eq!(spread.len(), table.len());
            assert!(spread.iter().all(|v| v.is_finite() && *v >= 0.0));
        }
    }

    #[test]
    fn spreading_function_peaks_at_self() {
        // The spreading function is maximal when source and target Bark
        // coincide (j - i = 0): tmpx = 0, x = 8*min(0.25,0) = 0,
        // tmpy = 15.811389 + 7.5*0.474 - 17.5*sqrt(1+0.474^2).
        let at_self = spreading_function(10.0, 10.0);
        let off = spreading_function(10.0, 13.0);
        let below = spreading_function(10.0, 7.0);
        assert!(at_self > off, "spread into a higher Bark must be weaker");
        assert!(at_self > below, "spread into a lower Bark must be weaker");
        assert!(at_self > 0.0);
    }

    #[test]
    fn spreading_function_self_value_matches_formula() {
        // Verbatim evaluation at j - i = 0 from the clause D.2.3 formula.
        let tmpx = 0.0_f64;
        let x = 8.0 * ((tmpx - 0.5) * (tmpx - 0.5) - 2.0 * (tmpx - 0.5)).min(0.0);
        let tmpy = 15.811389 + 7.5 * (tmpx + 0.474)
            - 17.5 * (1.0 + (tmpx + 0.474) * (tmpx + 0.474)).sqrt();
        let expected = 10.0_f64.powf((x + tmpy) / 10.0);
        assert!((spreading_function(10.0, 10.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn spreading_function_is_asymmetric() {
        // The Model-2 spread is asymmetric: spreading upward in Bark
        // (j > i) differs from spreading downward (j < i) by the same
        // distance.
        let up = spreading_function(10.0, 12.0);
        let down = spreading_function(10.0, 8.0);
        assert!((up - down).abs() > 1e-9, "spreading must be asymmetric");
    }

    #[test]
    fn spreading_function_decays_far_below() {
        // Far below the masker (large negative j - i) the asymmetric
        // term drives tmpy below -100, clamping the spread to zero.
        assert_eq!(spreading_function(24.0, 0.0), 0.0);
    }

    #[test]
    fn table_d5_has_33_coder_partitions() {
        assert_eq!(TABLE_D_5_CODER_PARTITION.len(), 33);
        assert_eq!(CODER_PARTITION_COUNT, 33);
    }

    #[test]
    fn table_d5_boundaries_match_printed_rows() {
        // Spot-check the literal PDF page-145 (printed 139) boundary
        // column at its endpoints and a couple of interior rows.
        assert_eq!(TABLE_D_5_CODER_PARTITION[0].boundary, 1);
        assert_eq!(TABLE_D_5_CODER_PARTITION[1].boundary, 17);
        assert_eq!(TABLE_D_5_CODER_PARTITION[12].boundary, 193);
        assert_eq!(TABLE_D_5_CODER_PARTITION[13].boundary, 209);
        assert_eq!(TABLE_D_5_CODER_PARTITION[32].boundary, 513);
    }

    #[test]
    fn table_d5_boundaries_step_by_16_after_partition_0() {
        // Partition 0 ends at line 1; every subsequent boundary is
        // exactly 16 FFT lines past the previous one (the spec's
        // uniform 16-line coder-partition geometry above DC).
        let table = &TABLE_D_5_CODER_PARTITION;
        assert_eq!(table[1].boundary - table[0].boundary, 16);
        for w in table[1..].windows(2) {
            assert_eq!(
                w[1].boundary - w[0].boundary,
                16,
                "coder-partition boundaries step by 16 FFT lines"
            );
        }
    }

    #[test]
    fn table_d5_boundaries_strictly_increase_to_nyquist() {
        let table = &TABLE_D_5_CODER_PARTITION;
        for w in table.windows(2) {
            assert!(
                w[1].boundary > w[0].boundary,
                "boundaries must strictly increase: {} then {}",
                w[0].boundary,
                w[1].boundary
            );
        }
        // Last boundary reaches the Nyquist line of the 1024-point FFT.
        assert_eq!(table[32].boundary, 513);
    }

    #[test]
    fn table_d5_width_flag_flips_at_partition_13() {
        // width_n is 0 for the low-frequency partitions n ≤ 12 and 1
        // from n = 13 onward (verbatim from the printed table).
        for (n, p) in TABLE_D_5_CODER_PARTITION.iter().enumerate() {
            let expected = if n <= 12 { 0 } else { 1 };
            assert_eq!(p.width, expected, "width_n at partition {n}");
        }
    }

    #[test]
    fn coder_partition_span_covers_axis_contiguously() {
        // Partition 0 is the single DC line; for n ≥ 1 the span begins
        // one past the previous boundary and ends at this boundary, so
        // the spans tile 1..=513 with no gap or overlap.
        assert_eq!(coder_partition_span(0), Some((1, 1)));
        assert_eq!(coder_partition_span(1), Some((2, 17)));
        assert_eq!(coder_partition_span(32), Some((498, 513)));
        let mut next_expected = 1;
        for n in 0..CODER_PARTITION_COUNT {
            let (low, high) = coder_partition_span(n).unwrap();
            assert_eq!(low, next_expected, "span {n} starts contiguously");
            assert!(high >= low, "span {n} is non-empty");
            next_expected = high + 1;
        }
        assert_eq!(next_expected, 514, "spans cover lines 1..=513 exactly");
    }

    #[test]
    fn coder_partition_span_out_of_table_is_none() {
        assert_eq!(coder_partition_span(33), None);
        assert_eq!(coder_partition_span(usize::MAX), None);
    }

    #[test]
    fn coder_partition_of_line_inverts_the_span() {
        // Every FFT line in a partition's span maps back to that
        // partition index.
        for n in 0..CODER_PARTITION_COUNT {
            let (low, high) = coder_partition_span(n).unwrap();
            for omega in low..=high {
                assert_eq!(
                    coder_partition_of_line(omega),
                    Some(n),
                    "line {omega} must map to coder partition {n}"
                );
            }
        }
    }

    #[test]
    fn coder_partition_of_line_boundary_anchors() {
        assert_eq!(coder_partition_of_line(1), Some(0));
        assert_eq!(coder_partition_of_line(2), Some(1));
        assert_eq!(coder_partition_of_line(17), Some(1));
        assert_eq!(coder_partition_of_line(18), Some(2));
        assert_eq!(coder_partition_of_line(513), Some(32));
    }

    #[test]
    fn coder_partition_of_line_out_of_range_is_none() {
        assert_eq!(coder_partition_of_line(0), None);
        assert_eq!(coder_partition_of_line(514), None);
        assert_eq!(coder_partition_of_line(u32::MAX), None);
    }

    // ----- Model 2 step (f): spreading convolution + normalization -----

    /// Independent reference implementation of the step (f) convolution,
    /// written straight from the spec sum so the production routine is
    /// cross-checked against a second formulation rather than itself.
    fn reference_convolve(table: &[CalcPartition], quantity: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; table.len()];
        for (b, target) in table.iter().enumerate() {
            let mut acc = 0.0;
            for (bb, source) in table.iter().enumerate() {
                acc += quantity[bb] * spreading_function(source.bval, target.bval);
            }
            out[b] = acc;
        }
        out
    }

    #[test]
    fn convolve_matches_independent_reference_on_d3a() {
        // A deterministic per-partition energy profile (no RNG, no
        // external data) convolved through both the production routine
        // and the from-spec reference must agree bin-for-bin.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let energy: Vec<f64> = (0..table.len()).map(|n| 1.0 + (n as f64) * 0.5).collect();
        let got = convolve_partition_spreading(table, &energy);
        let want = reference_convolve(table, &energy);
        assert_eq!(got.len(), table.len());
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 1e-12, "convolution mismatch: {g} vs {w}");
        }
    }

    #[test]
    fn convolve_of_unit_impulse_is_the_spreading_row() {
        // Convolving an impulse at source partition s reproduces the
        // spreading function from s into every target partition b — the
        // defining property of the convolution.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let s = 20usize;
        let mut energy = vec![0.0; table.len()];
        energy[s] = 1.0;
        let ecb = convolve_partition_spreading(table, &energy);
        for (b, target) in table.iter().enumerate() {
            let expected = spreading_function(table[s].bval, target.bval);
            assert!(
                (ecb[b] - expected).abs() < 1e-12,
                "impulse at {s} into {b}: {} vs {}",
                ecb[b],
                expected
            );
        }
    }

    #[test]
    fn convolve_is_linear_in_the_source_quantity() {
        // The convolution is a linear operator: conv(a·x + b·y) =
        // a·conv(x) + b·conv(y).
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let x: Vec<f64> = (0..table.len()).map(|n| (n as f64) * 0.3 + 0.1).collect();
        let y: Vec<f64> = (0..table.len()).map(|n| 10.0 - (n as f64) * 0.2).collect();
        let (a, b) = (2.5_f64, -1.5_f64);
        let combined: Vec<f64> = x.iter().zip(&y).map(|(&xi, &yi)| a * xi + b * yi).collect();
        let lhs = convolve_partition_spreading(table, &combined);
        let cx = convolve_partition_spreading(table, &x);
        let cy = convolve_partition_spreading(table, &y);
        for ((l, &px), &py) in lhs.iter().zip(&cx).zip(&cy) {
            assert!((l - (a * px + b * py)).abs() < 1e-9);
        }
    }

    #[test]
    fn convolve_length_mismatch_returns_empty() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        assert!(convolve_partition_spreading(table, &[1.0, 2.0]).is_empty());
        assert!(convolve_partition_spreading(table, &[]).is_empty());
    }

    #[test]
    fn rnorm_is_reciprocal_of_the_spreading_row_sum() {
        // rnorm_b = 1 / Σ_bb sprdngf(bval_bb, bval_b), verbatim.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        for b in 0..table.len() {
            let row_sum: f64 = table
                .iter()
                .map(|s| spreading_function(s.bval, table[b].bval))
                .sum();
            let got = rnorm_coefficient(table, b).unwrap();
            assert!((got - 1.0 / row_sum).abs() < 1e-12, "rnorm at {b}");
            assert!(got.is_finite() && got > 0.0, "rnorm must be finite > 0");
        }
    }

    #[test]
    fn rnorm_out_of_range_is_none() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        assert_eq!(rnorm_coefficient(table, table.len()), None);
        assert_eq!(rnorm_coefficient(table, usize::MAX), None);
    }

    #[test]
    fn rnorm_normalizes_a_flat_spread_to_unity() {
        // Feeding a flat unit energy through convolve then normalize
        // must return exactly 1.0 in every partition: en_b =
        // (Σ_bb 1·sprdngf) · (1 / Σ_bb sprdngf) = 1.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let flat = vec![1.0; table.len()];
        let ecb = convolve_partition_spreading(table, &flat);
        let en = normalize_spread_energy(table, &ecb);
        assert_eq!(en.len(), table.len());
        for (b, &v) in en.iter().enumerate() {
            assert!((v - 1.0).abs() < 1e-12, "en[{b}] = {v}, expected 1.0");
        }
    }

    #[test]
    fn normalize_spread_energy_applies_rnorm_pointwise() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let ecb: Vec<f64> = (0..table.len()).map(|n| 3.0 + (n as f64)).collect();
        let en = normalize_spread_energy(table, &ecb);
        for (b, &e) in ecb.iter().enumerate() {
            let expected = e * rnorm_coefficient(table, b).unwrap();
            assert!((en[b] - expected).abs() < 1e-12, "en at {b}");
        }
    }

    #[test]
    fn normalize_spread_energy_length_mismatch_returns_empty() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        assert!(normalize_spread_energy(table, &[1.0]).is_empty());
    }

    #[test]
    fn renormalize_unpredictability_divides_cf_by_ecb() {
        // cb_b = cf_b / ecb_b for the energy-weighted convolution.
        // Build cf as the convolution of e·c and ecb as the convolution
        // of e, then check the quotient against direct division.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let e: Vec<f64> = (0..table.len()).map(|n| 1.0 + (n as f64) * 0.7).collect();
        let c: Vec<f64> = (0..table.len())
            .map(|n| 0.1 + ((n % 5) as f64) * 0.15)
            .collect();
        let ec: Vec<f64> = e.iter().zip(&c).map(|(&ei, &ci)| ei * ci).collect();
        let ecb = convolve_partition_spreading(table, &e);
        let cf = convolve_partition_spreading(table, &ec);
        let cb = renormalize_unpredictability(&cf, &ecb);
        assert_eq!(cb.len(), table.len());
        for b in 0..table.len() {
            assert!((cb[b] - cf[b] / ecb[b]).abs() < 1e-12, "cb at {b}");
            // A weighted average of c-values lies within their range.
            assert!(cb[b] >= -1e-9 && cb[b] <= 1.0 + 1e-9, "cb[{b}] = {}", cb[b]);
        }
    }

    #[test]
    fn renormalize_unpredictability_silent_partition_is_zero() {
        // ecb_b == 0 (silent partition) yields cb_b = 0, not NaN.
        let cf = [0.0, 5.0, 0.0];
        let ecb = [0.0, 2.5, 0.0];
        let cb = renormalize_unpredictability(&cf, &ecb);
        assert_eq!(cb, vec![0.0, 2.0, 0.0]);
        assert!(cb.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn renormalize_unpredictability_length_mismatch_returns_empty() {
        assert!(renormalize_unpredictability(&[1.0, 2.0], &[1.0]).is_empty());
    }

    #[test]
    fn step_f_pipeline_preserves_constant_unpredictability() {
        // A physically meaningful end-to-end check: if every partition
        // has the same unpredictability c0, then after the energy-weighted
        // convolution and renormalization cb_b must equal c0 everywhere
        // (a weighted mean of identical values is that value), for any
        // energy profile.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let c0 = 0.42_f64;
        let e: Vec<f64> = (0..table.len()).map(|n| 0.5 + (n as f64) * 1.3).collect();
        let ec: Vec<f64> = e.iter().map(|&ei| ei * c0).collect();
        let ecb = convolve_partition_spreading(table, &e);
        let cf = convolve_partition_spreading(table, &ec);
        let cb = renormalize_unpredictability(&cf, &ecb);
        for (b, &v) in cb.iter().enumerate() {
            assert!((v - c0).abs() < 1e-12, "cb[{b}] = {v}, expected {c0}");
        }
    }

    // --- D.2.4 steps (g)…(n): threshold-calculation loop ---

    #[test]
    fn tonality_index_matches_formula_in_open_interval() {
        // For cb values that map strictly inside (0,1), tb is the
        // verbatim expression with no clamp engaged. The raw expression
        // crosses 0 near cb ≈ 0,5 (where −0,43·ln(cb) ≈ 0,299) and
        // crosses 1 near cb ≈ 0,051, so these three stay interior.
        for &cb in &[0.06_f64, 0.1, 0.3] {
            let tb = tonality_index(cb);
            let expected = -0.299 - 0.43 * cb.ln();
            assert!((tb - expected).abs() < 1e-12, "tb({cb}) = {tb}");
            assert!(tb > 0.0 && tb < 1.0, "tb({cb}) = {tb} must be in (0,1)");
        }
    }

    #[test]
    fn tonality_index_clamps_to_unit_range() {
        // Very tonal (cb → 0⁺) saturates the raw expression above 1 → 1.
        assert_eq!(tonality_index(1e-6), 1.0);
        // Fully unpredictable (cb ≥ 1) drives the raw expression ≤ 0 → 0.
        assert_eq!(tonality_index(1.0), 0.0);
        assert_eq!(tonality_index(5.0), 0.0);
        // Silent partition (cb == 0, no defined log) is treated as
        // maximally tonal.
        assert_eq!(tonality_index(0.0), 1.0);
        assert_eq!(tonality_index(-0.0), 1.0);
    }

    #[test]
    fn required_snr_interpolates_between_nmt_and_tmn() {
        // tb = 1 (purely tonal) requires TMN dB; tb = 0 (purely noise-
        // like) requires NMT dB; provided minval does not override.
        let part = CalcPartition {
            omega_low: 50,
            omega_high: 53,
            bval: 11.41,
            minval: 4.5,
            tmn: 25.9,
        };
        assert!((required_snr_db(1.0, &part) - part.tmn).abs() < 1e-12);
        assert!((required_snr_db(0.0, &part) - NMT_DB).abs() < 1e-12);
        // Halfway: arithmetic mean of TMN and NMT (both above minval).
        let mid = required_snr_db(0.5, &part);
        assert!((mid - 0.5 * (part.tmn + NMT_DB)).abs() < 1e-12);
    }

    #[test]
    fn required_snr_minval_floor_overrides() {
        // A low-frequency partition with a high minval floor (e.g. the
        // 20 dB stereo-unmasking floor) overrides the masking estimate
        // when the latter is smaller.
        let part = CalcPartition {
            omega_low: 5,
            omega_high: 7,
            bval: 1.56,
            minval: 20.0,
            tmn: 24.5,
        };
        // tb = 0 → masking estimate is NMT = 5.5 dB < 20 dB floor.
        assert_eq!(required_snr_db(0.0, &part), 20.0);
    }

    #[test]
    fn power_ratio_is_inverse_db() {
        assert!((power_ratio(0.0) - 1.0).abs() < 1e-12);
        assert!((power_ratio(10.0) - 0.1).abs() < 1e-12);
        assert!((power_ratio(20.0) - 0.01).abs() < 1e-12);
        // Positive SNR always attenuates the energy (ratio in (0,1]).
        assert!(power_ratio(38.6) > 0.0 && power_ratio(38.6) < 1.0);
    }

    #[test]
    fn actual_energy_threshold_is_product() {
        assert!((actual_energy_threshold(8.0, 0.25) - 2.0).abs() < 1e-12);
        assert_eq!(actual_energy_threshold(0.0, 0.5), 0.0);
    }

    #[test]
    fn line_energy_threshold_spreads_uniformly_over_partition_lines() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        // Unit normalized energy and a fixed cb everywhere keeps the
        // arithmetic legible: every line of partition b carries nb_b /
        // line_count_b, with nb_b = en_b · bc_b.
        let en = vec![1.0_f64; table.len()];
        let cb = vec![0.3_f64; table.len()];
        let nb_omega = line_energy_threshold(table, &en, &cb);
        assert_eq!(nb_omega.len(), 513, "one entry per FFT line 1..=513");

        let tb = tonality_index(0.3);
        for (b, part) in table.iter().enumerate() {
            let snr = required_snr_db(tb, part);
            let bc = power_ratio(snr);
            let nb_b = actual_energy_threshold(en[b], bc);
            let per_line = nb_b / f64::from(part.line_count());
            // Every line in [ωlow, ωhigh] gets the same per-line value.
            for omega in part.omega_low..=part.omega_high {
                let got = nb_omega[(omega - 1) as usize];
                assert!(
                    (got - per_line).abs() < 1e-12,
                    "partition {b} line {omega}: got {got}, want {per_line}"
                );
            }
        }
    }

    #[test]
    fn line_energy_threshold_conserves_partition_energy() {
        // Step (k) merely *spreads* nb_b across the partition's lines, so
        // summing the per-line values back over a partition must recover
        // nb_b exactly.
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let en: Vec<f64> = (0..table.len()).map(|n| 0.5 + n as f64 * 0.7).collect();
        let cb = vec![0.42_f64; table.len()];
        let nb_omega = line_energy_threshold(table, &en, &cb);
        let tb = tonality_index(0.42);
        for (b, part) in table.iter().enumerate() {
            let nb_b = actual_energy_threshold(en[b], power_ratio(required_snr_db(tb, part)));
            let sum: f64 = (part.omega_low..=part.omega_high)
                .map(|w| nb_omega[(w - 1) as usize])
                .sum();
            assert!((sum - nb_b).abs() < 1e-9, "partition {b}: {sum} vs {nb_b}");
        }
    }

    #[test]
    fn line_energy_threshold_length_mismatch_returns_empty() {
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        assert!(line_energy_threshold(table, &[1.0, 2.0], &vec![0.3; table.len()]).is_empty());
        assert!(line_energy_threshold(table, &vec![1.0; table.len()], &[0.3]).is_empty());
    }

    #[test]
    fn include_absolute_threshold_takes_pointwise_max() {
        let nb = [1.0, 5.0, 0.0, 2.0];
        let abs = [3.0, 2.0, 0.5, 2.0];
        assert_eq!(
            include_absolute_threshold(&nb, &abs),
            vec![3.0, 5.0, 0.5, 2.0]
        );
    }

    #[test]
    fn include_absolute_threshold_length_mismatch_returns_empty() {
        assert!(include_absolute_threshold(&[1.0, 2.0], &[1.0]).is_empty());
    }

    #[test]
    fn smr_narrow_partition_sums_thresholds() {
        // Coder partition 13 (n=13) has width 1 (narrow) and spans
        // ωlow=194..ωhigh=209 per Table D.5.
        let (lo, hi) = coder_partition_span(13).unwrap();
        assert_eq!((lo, hi), (194, 209));
        assert_eq!(TABLE_D_5_CODER_PARTITION[13].width, 1);

        let mut r2 = vec![0.0_f64; 513];
        let mut thr = vec![0.0_f64; 513];
        for w in lo..=hi {
            r2[(w - 1) as usize] = 4.0;
            thr[(w - 1) as usize] = 2.0;
        }
        let lines = (hi - lo + 1) as f64;
        let epart = 4.0 * lines;
        let npart = 2.0 * lines; // narrow: sum of thresholds
        let expected = 10.0 * (epart / npart).log10();
        let got = signal_to_mask_ratio_db(13, &r2, &thr).unwrap();
        assert!((got - expected).abs() < 1e-12, "got {got}, want {expected}");
    }

    #[test]
    fn smr_wide_partition_uses_min_threshold_times_count() {
        // Coder partition 1 (n=1) has width 0 (wide) and spans
        // ωlow=2..ωhigh=17 per Table D.5.
        let (lo, hi) = coder_partition_span(1).unwrap();
        assert_eq!((lo, hi), (2, 17));
        assert_eq!(TABLE_D_5_CODER_PARTITION[1].width, 0);

        let mut r2 = vec![0.0_f64; 513];
        let mut thr = vec![0.0_f64; 513];
        for w in lo..=hi {
            r2[(w - 1) as usize] = 3.0;
            thr[(w - 1) as usize] = 5.0;
        }
        // Plant a single smaller positive threshold — the wide-band rule
        // must pick this minimum, not the sum.
        thr[(lo - 1) as usize] = 1.0;
        let lines = (hi - lo + 1) as f64;
        let epart = 3.0 * lines;
        let npart = 1.0 * lines; // wide: min positive thr × line count
        let expected = 10.0 * (epart / npart).log10();
        let got = signal_to_mask_ratio_db(1, &r2, &thr).unwrap();
        assert!((got - expected).abs() < 1e-12, "got {got}, want {expected}");
    }

    #[test]
    fn smr_wide_partition_ignores_zero_thresholds_in_min() {
        // The wide-band min is over *positive* thresholds only; a zero
        // (silent) line must not collapse npart to zero while other lines
        // carry energy.
        let (lo, hi) = coder_partition_span(1).unwrap();
        let mut r2 = vec![0.0_f64; 513];
        let mut thr = vec![0.0_f64; 513];
        for w in lo..=hi {
            r2[(w - 1) as usize] = 2.0;
            thr[(w - 1) as usize] = 4.0;
        }
        thr[(lo - 1) as usize] = 0.0; // one silent line
        let lines = (hi - lo + 1) as f64;
        let expected = 10.0 * ((2.0 * lines) / (4.0 * lines)).log10();
        let got = signal_to_mask_ratio_db(1, &r2, &thr).unwrap();
        assert!((got - expected).abs() < 1e-12);
    }

    #[test]
    fn smr_all_silent_thresholds_returns_none() {
        // No positive threshold anywhere in the partition → npart 0 →
        // no finite ratio.
        let r2 = vec![1.0_f64; 513];
        let thr = vec![0.0_f64; 513];
        assert!(signal_to_mask_ratio_db(5, &r2, &thr).is_none());
    }

    #[test]
    fn smr_out_of_range_partition_returns_none() {
        let r2 = vec![1.0_f64; 513];
        let thr = vec![1.0_f64; 513];
        assert!(signal_to_mask_ratio_db(33, &r2, &thr).is_none());
    }

    #[test]
    fn smr_short_buffers_return_none() {
        // ωhigh of the requested partition exceeds the buffer length.
        let r2 = vec![1.0_f64; 100];
        let thr = vec![1.0_f64; 100];
        // Partition 32 reaches line 513 — well past 100.
        assert!(signal_to_mask_ratio_db(32, &r2, &thr).is_none());
    }

    #[test]
    fn smr_dc_partition_zero_threshold_returns_none() {
        // Coder partition 0 is the single DC line 1; with a zero
        // threshold there is no finite SMR.
        assert_eq!(coder_partition_span(0).unwrap(), (1, 1));
        let mut r2 = vec![0.0_f64; 513];
        let thr = vec![0.0_f64; 513];
        r2[0] = 9.0;
        assert!(signal_to_mask_ratio_db(0, &r2, &thr).is_none());
    }

    #[test]
    fn end_to_end_steps_g_to_n_produce_finite_smr() {
        // A coherent walk through steps (f)…(n) on a synthetic spectrum:
        // give every calculation partition unit energy and a mid-range
        // unpredictability, run the partition threshold loop, floor it
        // with a small absolute threshold, then compute SMR over a
        // mid-band coder partition fed with a matching synthetic r².
        let table = &TABLE_D_3A_CALC_PARTITION_32KHZ;
        let en = vec![1.0_f64; table.len()];
        let cb = vec![0.25_f64; table.len()];
        let nb_omega = line_energy_threshold(table, &en, &cb);
        let absthr = vec![1e-3_f64; nb_omega.len()];
        let thr = include_absolute_threshold(&nb_omega, &absthr);
        assert_eq!(thr.len(), 513);
        assert!(thr.iter().all(|t| t.is_finite() && *t > 0.0));

        // Signal energy: a tone planted in coder partition 20's span.
        let (lo, hi) = coder_partition_span(20).unwrap();
        let mut r2 = vec![0.0_f64; 513];
        for w in lo..=hi {
            r2[(w - 1) as usize] = 10.0;
        }
        let smr = signal_to_mask_ratio_db(20, &r2, &thr).unwrap();
        assert!(smr.is_finite());
    }

    // ---- Annex D Table D.4a / D.4b / D.4c absolute-threshold tables ----

    #[test]
    fn table_d4_row_counts_match_spec() {
        // Entry counts from Annex D Tables D.4a / D.4b / D.4c
        // (docs/audio/mp3/annex-d-table-D4{a,b,c}-*.csv).
        assert_eq!(TABLE_D_4A_ABSTHR_LAYER2_32.len(), 132);
        assert_eq!(TABLE_D_4B_ABSTHR_LAYER2_44K1.len(), 130);
        assert_eq!(TABLE_D_4C_ABSTHR_LAYER2_48.len(), 126);
    }

    #[test]
    fn table_d4_ranges_are_contiguous_and_one_based() {
        // Each table tiles the FFT-line axis with no gaps:
        // line_lower[0] == 1 and line_lower[k] == line_higher[k-1] + 1.
        for table in [
            &TABLE_D_4A_ABSTHR_LAYER2_32[..],
            &TABLE_D_4B_ABSTHR_LAYER2_44K1[..],
            &TABLE_D_4C_ABSTHR_LAYER2_48[..],
        ] {
            assert_eq!(table[0].line_lower, 1);
            for w in table.windows(2) {
                assert!(w[0].line_higher >= w[0].line_lower);
                assert_eq!(w[1].line_lower, w[0].line_higher + 1);
            }
        }
    }

    #[test]
    fn table_d4_top_lines_match_spec() {
        // Topmost tabulated FFT line per rate: 480 / 464 / 432.
        assert_eq!(TABLE_D_4A_ABSTHR_LAYER2_32.last().unwrap().line_higher, 480);
        assert_eq!(
            TABLE_D_4B_ABSTHR_LAYER2_44K1.last().unwrap().line_higher,
            464
        );
        assert_eq!(TABLE_D_4C_ABSTHR_LAYER2_48.last().unwrap().line_higher, 432);
    }

    #[test]
    fn table_d4_low_lines_are_single_line_groups() {
        // The low FFT lines each get their own one-line entry
        // (line_lower == line_higher) in every table.
        for table in [
            &TABLE_D_4A_ABSTHR_LAYER2_32[..],
            &TABLE_D_4B_ABSTHR_LAYER2_44K1[..],
            &TABLE_D_4C_ABSTHR_LAYER2_48[..],
        ] {
            let first = table[0];
            assert_eq!(first.line_lower, first.line_higher);
            assert_eq!(first.line_count(), 1);
        }
    }

    #[test]
    fn table_d4a_spot_cells() {
        // Verbatim head / tail cells from D.4a (32 kHz).
        let head = TABLE_D_4A_ABSTHR_LAYER2_32[0];
        assert_eq!(head.line_lower, 1);
        assert_eq!(head.line_higher, 1);
        assert!((head.threshold_db - 58.23).abs() < 1e-9);
        let tail = *TABLE_D_4A_ABSTHR_LAYER2_32.last().unwrap();
        assert_eq!((tail.line_lower, tail.line_higher), (473, 480));
        // As-printed D.4a ceiling 51.03 dB (D.1d's twin prints 51.04).
        assert!((tail.threshold_db - 51.03).abs() < 1e-9);
    }

    #[test]
    fn table_d4b_uses_69_13_ceiling() {
        // The D.4b (44,1 kHz) saturation ceiling is the surprising
        // 69.13 dB (D.1e's twin caps at 68.00). The top run holds it.
        let tail = *TABLE_D_4B_ABSTHR_LAYER2_44K1.last().unwrap();
        assert_eq!((tail.line_lower, tail.line_higher), (457, 464));
        assert!((tail.threshold_db - 69.13).abs() < 1e-9);
        // First entry that reaches the ceiling covers lines 369-376.
        let first_ceiling = TABLE_D_4B_ABSTHR_LAYER2_44K1
            .iter()
            .find(|e| (e.threshold_db - 69.13).abs() < 1e-9)
            .unwrap();
        assert_eq!(
            (first_ceiling.line_lower, first_ceiling.line_higher),
            (369, 376)
        );
    }

    #[test]
    fn table_d4c_uses_68_00_ceiling() {
        // D.4c (48 kHz) matches D.1f including the 68.00 dB ceiling.
        let tail = *TABLE_D_4C_ABSTHR_LAYER2_48.last().unwrap();
        assert_eq!((tail.line_lower, tail.line_higher), (425, 432));
        assert!((tail.threshold_db - 68.00).abs() < 1e-9);
    }

    #[test]
    fn abs_threshold_dispatcher_returns_the_right_table() {
        assert!(std::ptr::eq(
            abs_threshold_table_for_rate(SamplingRate::Fs32kHz),
            &TABLE_D_4A_ABSTHR_LAYER2_32[..]
        ));
        assert!(std::ptr::eq(
            abs_threshold_table_for_rate(SamplingRate::Fs44k1Hz),
            &TABLE_D_4B_ABSTHR_LAYER2_44K1[..]
        ));
        assert!(std::ptr::eq(
            abs_threshold_table_for_rate(SamplingRate::Fs48kHz),
            &TABLE_D_4C_ABSTHR_LAYER2_48[..]
        ));
    }

    #[test]
    fn absolute_threshold_per_line_broadcasts_ranges() {
        // The single-line low entries map straight through; the wider
        // top-of-band entries broadcast one value across every line.
        let out = absolute_threshold_db_per_line(&TABLE_D_4A_ABSTHR_LAYER2_32, 513);
        assert_eq!(out.len(), 513);
        // FFT line 1 -> 58.23 dB (0-based index 0).
        assert!((out[0] - 58.23).abs() < 1e-9);
        // Lines 473..=480 all hold 51.03 dB.
        for omega in 473..=480 {
            assert!((out[omega - 1] - 51.03).abs() < 1e-9);
        }
    }

    #[test]
    fn absolute_threshold_per_line_holds_ceiling_above_last_range() {
        // D.4a tops out at line 480; lines 481..=513 hold the last
        // tabulated threshold (the 51.03 dB ceiling).
        let out = absolute_threshold_db_per_line(&TABLE_D_4A_ABSTHR_LAYER2_32, 513);
        for omega in 481..=513 {
            assert!((out[omega - 1] - 51.03).abs() < 1e-9);
        }
        // 44,1 kHz holds 69.13 above line 464; 48 kHz holds 68.00 above 432.
        let out_b = absolute_threshold_db_per_line(&TABLE_D_4B_ABSTHR_LAYER2_44K1, 513);
        assert!((out_b[512] - 69.13).abs() < 1e-9);
        let out_c = absolute_threshold_db_per_line(&TABLE_D_4C_ABSTHR_LAYER2_48, 513);
        assert!((out_c[512] - 68.00).abs() < 1e-9);
    }

    #[test]
    fn absolute_threshold_per_line_length_follows_caller() {
        // line_count drives the output length; a short request truncates
        // to the requested working range.
        let out = absolute_threshold_db_per_line(&TABLE_D_4A_ABSTHR_LAYER2_32, 64);
        assert_eq!(out.len(), 64);
        assert!((out[0] - 58.23).abs() < 1e-9);
    }

    #[test]
    fn absolute_threshold_per_line_safe_responses() {
        // Empty table or zero line_count -> empty vector.
        assert!(absolute_threshold_db_per_line(&[], 513).is_empty());
        assert!(absolute_threshold_db_per_line(&TABLE_D_4A_ABSTHR_LAYER2_32, 0).is_empty());
    }

    #[test]
    fn absolute_threshold_per_line_feeds_step_l_floor() {
        // End-to-end: the expanded per-line dB table, converted into the
        // energy domain, is a valid step-(l) absthr_omega for
        // include_absolute_threshold (both 513-long, all finite).
        let db = absolute_threshold_db_per_line(&TABLE_D_4C_ABSTHR_LAYER2_48, 513);
        let absthr_energy: Vec<f64> = db.iter().map(|&d| 10.0_f64.powf(d / 10.0)).collect();
        let table = &TABLE_D_3C_CALC_PARTITION_48KHZ;
        let en = vec![1.0_f64; table.len()];
        let cb = vec![0.5_f64; table.len()];
        let nb_omega = line_energy_threshold(table, &en, &cb);
        assert_eq!(nb_omega.len(), absthr_energy.len());
        let thr = include_absolute_threshold(&nb_omega, &absthr_energy);
        assert_eq!(thr.len(), 513);
        assert!(thr.iter().all(|t| t.is_finite()));
        for (t, a) in thr.iter().zip(&absthr_energy) {
            assert!(*t >= *a - 1e-9);
        }
    }
    #[test]
    fn tmn_relation_reproduces_mpeg1_printed_column() {
        // The LSF long-block tables print no TMN column; the LSF
        // constants derive it as max(24.5, bval + 14.5) rounded to one
        // decimal. Pin that relation against the *printed* TMN column
        // of every 11172-3 Layer II partition (all three rates): the
        // derived value must match to within the 1-decimal print
        // rounding latitude (0.05 dB; the printed bval is itself
        // rounded to two decimals, so exact half-point ties can round
        // either way).
        for table in [
            &TABLE_D_3A_CALC_PARTITION_32KHZ[..],
            &TABLE_D_3B_CALC_PARTITION_44K1HZ[..],
            &TABLE_D_3C_CALC_PARTITION_48KHZ[..],
        ] {
            for p in table {
                let derived = (24.5_f64).max(p.bval + 14.5);
                assert!(
                    (derived - p.tmn).abs() < 0.0501,
                    "bval {}: derived TMN {} vs printed {}",
                    p.bval,
                    derived,
                    p.tmn,
                );
            }
        }
    }

    #[test]
    fn lsf_d3_tables_are_contiguous_with_documented_coverage() {
        for (table, partitions, top_line) in [
            (&TABLE_LSF_D_3A_CALC_PARTITION_24KHZ[..], 60, 491),
            (&TABLE_LSF_D_3B_CALC_PARTITION_22K05HZ[..], 60, 489),
            (&TABLE_LSF_D_3C_CALC_PARTITION_16KHZ[..], 56, 509),
        ] {
            assert_eq!(table.len(), partitions);
            assert_eq!(table[0].omega_low, 1);
            for w in table.windows(2) {
                assert_eq!(w[1].omega_low, w[0].omega_high + 1);
                assert!(w[0].bval < w[1].bval, "bval must increase");
            }
            assert_eq!(table[table.len() - 1].omega_high, top_line);
        }
    }

    #[test]
    fn lsf_d3_tmn_column_follows_the_derivation() {
        // The stored tmn literals are the documented derivation —
        // max(24.5, bval + 14.5) rounded half-up to one decimal.
        for table in [
            &TABLE_LSF_D_3A_CALC_PARTITION_24KHZ[..],
            &TABLE_LSF_D_3B_CALC_PARTITION_22K05HZ[..],
            &TABLE_LSF_D_3C_CALC_PARTITION_16KHZ[..],
        ] {
            for p in table {
                let derived = ((24.5_f64).max(p.bval + 14.5) * 10.0).round() / 10.0;
                assert!(
                    (derived - p.tmn).abs() < 1e-9,
                    "bval {}: stored tmn {} vs derived {}",
                    p.bval,
                    p.tmn,
                    derived,
                );
            }
        }
    }

    #[test]
    fn lsf_absthr_tables_mirror_the_d1_transcriptions() {
        for (absthr, d1) in [
            (
                lsf_abs_threshold_layer2_16(),
                &crate::tables_lsf::TABLE_LSF_D_1D_LTQ_LAYER_II_16[..],
            ),
            (
                lsf_abs_threshold_layer2_22k05(),
                &crate::tables_lsf::TABLE_LSF_D_1E_LTQ_LAYER_II_22K05[..],
            ),
            (
                lsf_abs_threshold_layer2_24(),
                &crate::tables_lsf::TABLE_LSF_D_1F_LTQ_LAYER_II_24[..],
            ),
        ] {
            assert_eq!(absthr.len(), 132);
            assert_eq!(absthr[0].line_lower, 1);
            assert_eq!(absthr[131].line_higher, 480);
            for w in absthr.windows(2) {
                assert_eq!(w[1].line_lower, w[0].line_higher + 1);
            }
            for (a, e) in absthr.iter().zip(d1.iter()) {
                assert_eq!(a.line_higher, e.top_line_index);
                assert!((a.threshold_db - e.threshold_db).abs() < 1e-12);
            }
        }
    }
}
