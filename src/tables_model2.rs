//! ISO/IEC 11172-3:1993 Annex D clause **D.2** (Psychoacoustic
//! *Model 2*) — the *calculation partition table* (Table D.3a) and the
//! Model-2 *spreading function* `sprdngf(i, j)`.
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
//! `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`. No
//! third-party MP2 source was consulted.

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
}
