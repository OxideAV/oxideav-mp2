//! §2.4.3.3.3 / Annex C encoder-side scalefactor extraction.
//!
//! ISO/IEC 11172-3 defines a Layer II frame as carrying, per channel,
//! three scalefactor-granules; each scalefactor-granule covers 12
//! consecutive sub-band samples (one §2.4.1.6 inner-loop iteration). A
//! 1152-PCM-sample frame, after the §2.4.3.2 analysis filterbank
//! ([`crate::AnalysisFilterbank`]), yields 36 sub-band samples per
//! sub-band — exactly `3 granules × 12 samples`.
//!
//! For each `(granule, sub-band)` the encoder picks a 6-bit Table 3-B.1
//! scalefactor index. This is the inverse of the §2.4.3.3.3 decode path:
//! decode reads an index, looks up [`crate::tables::SCALEFACTORS`], and
//! multiplies; encode looks at the samples and finds the smallest table
//! multiplier that still covers the largest sample.
//!
//! Per Annex C §C.1.5.2.6 (paraphrased from the standard): for each of
//! the 32 sub-bands and for each of the three groups of 12 samples, the
//! maximum of the absolute value of the 12 normalized samples is
//! determined; the scalefactor is the smallest value from Table 3-B.1
//! that is larger than or equal to this maximum. Because Table 3-B.1 is
//! monotonically *decreasing* (entry 0 is the largest multiplier, 2.0;
//! entry 62 the smallest), "smallest value `>= max_abs`" is equivalently
//! the *largest index* whose entry still satisfies `entry >= max_abs`.
//!
//! No external encoder or decoder source was consulted; the procedure is
//! the documented inverse of the normative decode definition.

use crate::tables::SCALEFACTORS;

/// Number of sub-bands carried by a Layer II frame.
const SUBBANDS: usize = 32;

/// Scalefactor-granules per channel per Layer II frame (§2.4.1.3).
pub const SCALEFACTOR_GRANULES: usize = 3;

/// Sub-band samples covered by one scalefactor-granule (§2.4.1.6).
pub const GRANULE_LEN: usize = 12;

/// Sub-band samples per sub-band per frame: `3 granules × 12 samples`.
pub const SUBBAND_SAMPLES_PER_FRAME: usize = SCALEFACTOR_GRANULES * GRANULE_LEN;

/// Pick the Table 3-B.1 scalefactor index covering `max_abs`.
///
/// Returns the largest index `i` whose entry `SCALEFACTORS[i] >= max_abs`
/// — equivalently the smallest table multiplier still large enough to
/// cover `max_abs`, per Annex C §C.1.5.2.6.
///
/// `max_abs` is expected to be the non-negative maximum absolute value of
/// a granule's 12 normalized sub-band samples.
///
/// # Edge cases
///
/// * `max_abs == 0.0` selects index 62 (the smallest multiplier): every
///   entry is `>= 0`, so the largest qualifying index is the last one.
///   Rescaling `0.0` by any multiplier is `0.0`, so this is harmless.
/// * `max_abs` exactly equal to an entry selects that entry's index (the
///   comparison is inclusive); a value microscopically below selects the
///   same index; microscopically above selects the next-smaller index
///   (`i - 1`, the larger multiplier).
/// * `max_abs > SCALEFACTORS[0]` (2.0) cannot occur for conformant input:
///   the normalized samples of a Layer II encoder lie in `[-1, +1)` per
///   §2.4.3.4.7.1, so no entry being large enough is a precondition
///   violation. It is clamped to index 0 (the largest multiplier).
pub fn pick_scalefactor_index(max_abs: f64) -> u8 {
    // Precondition clamp: nothing in the table covers a sample beyond the
    // largest multiplier, so fall back to the largest multiplier (index 0).
    if max_abs > SCALEFACTORS[0] {
        return 0;
    }

    // Table 3-B.1 decreases monotonically, so the qualifying entries form
    // a prefix `0..=idx`. Walk upward (toward smaller multipliers) and keep
    // the last index that still covers `max_abs`; stop at the first entry
    // too small to cover it (all later entries are smaller still).
    let mut idx = 0u8;
    for (i, &sf) in SCALEFACTORS.iter().enumerate() {
        if sf >= max_abs {
            idx = i as u8;
        } else {
            break;
        }
    }
    idx
}

/// Extract the Table 3-B.1 scalefactor index for one granule.
///
/// Computes `max_abs = max(|s(t)|)` over the 12 normalized sub-band
/// samples and returns its scalefactor index via [`pick_scalefactor_index`].
pub fn extract_scalefactor_index(samples: &[f64; GRANULE_LEN]) -> u8 {
    let max_abs = samples.iter().fold(0.0f64, |m, &s| m.max(s.abs()));
    pick_scalefactor_index(max_abs)
}

/// Compute the per-frame scalefactor indices for one channel.
///
/// `subband_samples[sb]` holds the 36 sub-band samples produced for
/// sub-band `sb` over one frame (most recent last), grouped as three
/// consecutive granules of 12. `sblimit` bounds the active sub-bands
/// (`0..sblimit`); sub-bands at or above it carry no allocation and are
/// left at index 62 (the neutral smallest multiplier).
///
/// The result is indexed `[granule][sub-band]`, matching the bitstream
/// ordering in which scalefactors are written (§2.4.1.7).
pub fn compute_scalefactors(
    subband_samples: &[[f64; SUBBAND_SAMPLES_PER_FRAME]; SUBBANDS],
    sblimit: usize,
) -> [[u8; SUBBANDS]; SCALEFACTOR_GRANULES] {
    let mut out = [[62u8; SUBBANDS]; SCALEFACTOR_GRANULES];
    let active = sblimit.min(SUBBANDS);
    for sb in 0..active {
        for (g, row) in out.iter_mut().enumerate() {
            let base = g * GRANULE_LEN;
            let mut window = [0.0f64; GRANULE_LEN];
            window.copy_from_slice(&subband_samples[sb][base..base + GRANULE_LEN]);
            row[sb] = extract_scalefactor_index(&window);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic LCG so the round-trip property test is reproducible
    /// (no `rand` dep, no wall-clock seeding).
    struct Lcg(u64);
    impl Lcg {
        fn new(seed: u64) -> Self {
            Lcg(seed)
        }
        /// Next f64 in `[-1.0, 1.0)` — the normalized sample range.
        fn next_sample(&mut self) -> f64 {
            // Numerical Recipes LCG constants.
            let stepped = self.0.wrapping_mul(6_364_136_223_846_793_005);
            self.0 = stepped.wrapping_add(1_442_695_040_888_963_407);
            let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64; // [0,1)
            unit * 2.0 - 1.0
        }
    }

    #[test]
    fn exact_entry_selects_its_index() {
        // A sample whose |value| equals SCALEFACTORS[i] exactly picks i.
        for (i, &sf) in SCALEFACTORS.iter().enumerate() {
            assert_eq!(pick_scalefactor_index(sf), i as u8, "exact entry {i}");
        }
    }

    #[test]
    fn slightly_below_entry_selects_same_index() {
        // A value microscopically below SCALEFACTORS[i] still selects i.
        for (i, &sf) in SCALEFACTORS.iter().enumerate() {
            let below = sf * (1.0 - 1e-9);
            assert_eq!(pick_scalefactor_index(below), i as u8, "below entry {i}");
        }
    }

    #[test]
    fn slightly_above_entry_selects_smaller_index() {
        // A value microscopically above SCALEFACTORS[i] selects i-1 (the
        // larger multiplier) for every interior entry.
        for (i, &sf) in SCALEFACTORS.iter().enumerate().skip(1) {
            let above = sf * (1.0 + 1e-9);
            assert_eq!(
                pick_scalefactor_index(above),
                (i - 1) as u8,
                "above entry {i}"
            );
        }
    }

    #[test]
    fn all_zero_granule_selects_index_62() {
        let zero = [0.0f64; GRANULE_LEN];
        assert_eq!(extract_scalefactor_index(&zero), 62);
        assert_eq!(pick_scalefactor_index(0.0), 62);
    }

    #[test]
    fn top_of_table_and_clamp() {
        // Exactly the largest multiplier (2.0) selects index 0.
        assert_eq!(pick_scalefactor_index(SCALEFACTORS[0]), 0);
        // Beyond the table (precondition violation) clamps to index 0.
        assert_eq!(pick_scalefactor_index(2.5), 0);
        assert_eq!(pick_scalefactor_index(f64::MAX), 0);
    }

    #[test]
    fn index_is_always_in_range() {
        for &v in &[0.0, 1e-12, 0.5, 1.0, 1.9999, 2.0, 3.0] {
            let idx = pick_scalefactor_index(v);
            assert!(idx <= 62, "idx {idx} out of range for {v}");
        }
    }

    #[test]
    fn window_round_trip_envelope_covers_samples() {
        // For random granules in [-1,1), the chosen multiplier must be an
        // envelope: SCALEFACTORS[idx] >= |sample| for every sample.
        let mut lcg = Lcg::new(0x1234_5678_9abc_def0);
        for _ in 0..1000 {
            let mut window = [0.0f64; GRANULE_LEN];
            for w in window.iter_mut() {
                *w = lcg.next_sample();
            }
            let idx = extract_scalefactor_index(&window) as usize;
            let multiplier = SCALEFACTORS[idx];
            for &s in &window {
                assert!(
                    multiplier >= s.abs(),
                    "multiplier {multiplier} (idx {idx}) fails to cover {s}"
                );
            }
            // And it must be the *tightest* such envelope: a larger index
            // (smaller multiplier) would fail to cover at least one sample,
            // unless we are already at the smallest multiplier (idx 62).
            if idx < 62 {
                let tighter = SCALEFACTORS[idx + 1];
                let max_abs = window.iter().fold(0.0f64, |m, &s| m.max(s.abs()));
                assert!(
                    tighter < max_abs,
                    "idx {idx} not tightest: {tighter} still covers {max_abs}"
                );
            }
        }
    }

    #[test]
    fn compute_scalefactors_respects_sblimit() {
        let mut streams = [[0.0f64; SUBBAND_SAMPLES_PER_FRAME]; SUBBANDS];
        // Sub-band 0: granule 1 has a peak at exactly SCALEFACTORS[3] (1.0).
        streams[0][GRANULE_LEN] = 1.0;
        // Sub-band 5: a peak in granule 2 at -SCALEFACTORS[6] (0.5).
        streams[5][2 * GRANULE_LEN + 4] = -SCALEFACTORS[6];

        let sf = compute_scalefactors(&streams, 8);

        // Sub-band 0, granule 1 -> index 3; its other granules are zero -> 62.
        assert_eq!(sf[1][0], 3);
        assert_eq!(sf[0][0], 62);
        assert_eq!(sf[2][0], 62);
        // Sub-band 5, granule 2 -> index 6.
        assert_eq!(sf[2][5], 6);
        // Inactive sub-bands (>= sblimit) stay at the neutral default.
        for row in sf.iter().take(SCALEFACTOR_GRANULES) {
            assert_eq!(row[8], 62);
            assert_eq!(row[31], 62);
        }
    }
}
