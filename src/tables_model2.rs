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
//! 48 kHz tables (D.3b / D.3c) have 57 and 58 partitions respectively
//! and are not yet transcribed here — see the followup note below.
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
//! at PDF page 129 / printed page 123), cross-checked against the
//! markdown extract
//! `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`.

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
}
