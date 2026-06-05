//! ISO/IEC 11172-3:1993 Annex D *Psychoacoustic models* — Model 1
//! masker primitives (§D.1).
//!
//! Annex D is informative — it presents two example encoder
//! psychoacoustic models common to all three Layers (Model 1 in
//! clause D.1; Model 2 in clause D.2). Neither is required of a
//! decoder; an encoder must drive its §C.1.5.2.7 iterative
//! bit-allocator with *some* per-(channel, sub-band) signal-to-mask
//! ratio (SMR) table, and Models 1 / 2 are the spec's worked
//! examples for producing one.
//!
//! This module lands the §D.1 Step 6 *masking-function* `vf` and the
//! §D.1 Step 7 *global-masking-threshold* `LTg` primitives — pure
//! functions of the (masker SPL, masker Bark position, target Bark
//! position) tuple that compose into the per-target-line individual
//! masking threshold `LT` and the per-target-line global threshold
//! `LTg`. Steps 1..5 of Model 1 (the 1024-sample FFT, the
//! SPL-conversion, the tonality classifier, the decimation /
//! reorganisation, and the masker-selection passes) read the
//! PNG-only inner rows of Annex D Tables D.1 / D.2 / D.3 / D.4 and
//! are not landed yet — they are tracked under the docs-collaborator
//! Tables D.1a–f / D.3a–c / D.4a–c PNG → text extraction gap (note
//! `#1262`).
//!
//! ## Spec context (clause D.1, ISO/IEC 11172-3:1993, informative)
//!
//! The individual masking threshold of a tonal / non-tonal masker is
//!
//! ```text
//! LT_tm[z(j), z(i)] = X_tm[z(j)] + av_tm[z(j)] + vf[z(j), z(i)]   dB
//! LT_nm[z(j), z(i)] = X_nm[z(j)] + av_nm[z(j)] + vf[z(j), z(i)]   dB
//! ```
//!
//! Masking index `av` (verbatim, clause D.1 Step 6):
//!
//! ```text
//! tonal     : av_tm = -1,525 - 0,275 * z(j) - 4,5   dB
//! non-tonal : av_nm = -1,525 - 0,175 * z(j) - 0,5   dB
//! ```
//!
//! Masking function `vf` (same for tonal and non-tonal; `dz =
//! z(i) - z(j)` is the Bark distance from masker `j` to line `i`;
//! `X` is the SPL of the masker in dB):
//!
//! ```text
//! vf = 17 * (dz + 1) - (0,4 * X[z(j)] + 6)   dB     for -3 <= dz < -1 Bark
//! vf = (0,4 * X[z(j)] + 6) * dz              dB     for -1 <= dz <  0 Bark
//! vf = -17 * dz                              dB     for  0 <= dz <  1 Bark
//! vf = -(dz - 1) * (17 - 0,15 * X[z(j)]) - 17 dB    for  1 <= dz <  8 Bark
//! ```
//!
//! Outside `-3 <= dz < 8` the masker is ignored (`LT` set to
//! `-inf dB` — the masker contributes nothing to the global sum).
//!
//! Global masking threshold (clause D.1 Step 7), summing the powers
//! of the `m` tonal and `n` non-tonal individual thresholds with the
//! threshold in quiet `LTq`:
//!
//! ```text
//! LTg(i) = 10 * log10( 10^(LTq(i) / 10)
//!                    + Sum 10^(LT_tm[z(j), z(i)] / 10)
//!                    + Sum 10^(LT_nm[z(j), z(i)] / 10) )   dB
//! ```
//!
//! For a given `i` the range of `j` may be reduced to maskers within
//! `-8..+3` Bark of `i` — this is the *symmetric* read of the same
//! `vf` window (a masker at `z(j)` only contributes to lines in
//! `[z(j) - 3, z(j) + 8)`, equivalently a target line at `z(i)` is
//! only influenced by maskers in `(z(i) - 8, z(i) + 3]`). The
//! implementation here applies the `vf` window directly on each
//! `(masker, target)` pair, preserving the spec range exactly
//! without the optional outer-loop short-circuit.
//!
//! ## Decimal-comma convention
//!
//! The spec uses European decimal notation (`0,617` = 0.617). The
//! constants below are reproduced with the period equivalents
//! (`0.617`) consistent with idiomatic Rust `f64` literals; no value
//! has been rounded or altered from the spec.
//!
//! ## Source
//!
//! Only the textually-transcribed equations from
//! `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md` were
//! consulted; the staged ISO PDF
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` is the upstream
//! authority. The PNG-only Annex D table rows are not read this
//! round.

/// Classification of a Model 1 masker per ISO/IEC 11172-3:1993 §D.1
/// Step 4 (tonal vs non-tonal). The two carry different
/// masking-index constants — a tonal masker has a deeper masking
/// floor than a non-tonal masker at the same SPL and Bark distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskerKind {
    /// Tonal masker (Step 4 selection: local maxima of the SPL
    /// spectrum surrounded by clearly lower neighbours). Carries
    /// `av_tm = -1.525 - 0.275 * z(j) - 4.5` dB.
    Tonal,
    /// Non-tonal masker (Step 4: per-critical-band energy sum of all
    /// non-tonal FFT lines, lumped to a single representative SPL +
    /// Bark position). Carries `av_nm = -1.525 - 0.175 * z(j) - 0.5`
    /// dB.
    NonTonal,
}

/// A single Model 1 masker carrying its SPL (`X[z(j)]` in dB) and
/// its Bark position (`z(j)`). Produced by §D.1 Step 4 (tonal /
/// non-tonal selection) and consumed by §D.1 Step 6 (individual
/// masking-threshold computation) and §D.1 Step 7 (global-threshold
/// summation).
///
/// This is a pure data carrier — the primitive functions on this
/// module read `spl_db` and `z_bark` directly. The intermediate
/// "tonal / non-tonal" Bark-coordinate transformation done by
/// Step 4 is the caller's responsibility (Steps 1..5 of Model 1 are
/// not implemented this round; see the module-level docs-collaborator
/// gap note on the PNG-only D.1 / D.2 / D.3 / D.4 tables).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Masker {
    /// Tonal / non-tonal classification per §D.1 Step 4.
    pub kind: MaskerKind,
    /// Masker Bark position `z(j)`, in Bark units (≈ 0..26 across
    /// the 32 / 44.1 / 48 kHz audio band).
    pub z_bark: f64,
    /// Masker SPL `X[z(j)]`, in dB.
    pub spl_db: f64,
}

/// Lower bound of the Bark-distance window in which a masker
/// contributes a non-`-inf` individual masking threshold (verbatim
/// §D.1 Step 6: the `vf` piecewise function is defined for
/// `-3 <= dz < 8`).
pub const MASKING_FUNCTION_DZ_LO: f64 = -3.0;

/// Upper bound (exclusive) of the Bark-distance window in which a
/// masker contributes a non-`-inf` individual masking threshold
/// (verbatim §D.1 Step 6).
pub const MASKING_FUNCTION_DZ_HI: f64 = 8.0;

/// §D.1 Step 6 masking index for a **tonal** masker at Bark
/// position `z_j`:
///
/// ```text
/// av_tm = -1.525 - 0.275 * z(j) - 4.5   dB
/// ```
///
/// (Verbatim spec equation. The constant `-4.5` is the tonal /
/// non-tonal differential — the tonal masker's individual masking
/// threshold sits ~4 dB lower for the same SPL + Bark position
/// because a tone is a more efficient masker than noise.)
#[inline]
#[must_use]
pub fn masking_index_tonal(z_j_bark: f64) -> f64 {
    -1.525 - 0.275 * z_j_bark - 4.5
}

/// §D.1 Step 6 masking index for a **non-tonal** masker at Bark
/// position `z_j`:
///
/// ```text
/// av_nm = -1.525 - 0.175 * z(j) - 0.5   dB
/// ```
///
/// (Verbatim spec equation. The slope `-0.175` is gentler than the
/// tonal `-0.275`, so the difference between tonal and non-tonal
/// masking indices widens with Bark — a tonal masker high in the
/// spectrum gets a relatively deeper threshold floor than the same
/// SPL non-tonal masker does.)
#[inline]
#[must_use]
pub fn masking_index_non_tonal(z_j_bark: f64) -> f64 {
    -1.525 - 0.175 * z_j_bark - 0.5
}

/// §D.1 Step 6 masking function `vf(dz, X)` (verbatim spec text).
/// `dz = z(i) - z(j)` is the Bark distance from the masker `j` to
/// the target line `i`; `x_db` is the SPL `X[z(j)]` of the masker
/// in dB.
///
/// ```text
/// vf =  17 * (dz + 1) - (0.4 * X + 6)     dB   for -3 <= dz < -1
/// vf =  (0.4 * X + 6) * dz                dB   for -1 <= dz <  0
/// vf = -17 * dz                           dB   for  0 <= dz <  1
/// vf = -(dz - 1) * (17 - 0.15 * X) - 17   dB   for  1 <= dz <  8
/// ```
///
/// Outside `-3 <= dz < 8` the masker is ignored — this function
/// returns `None` (the caller treats the masker as `LT = -inf dB`,
/// i.e. it contributes nothing to the global threshold sum).
///
/// At the boundary `dz = 0` the second and third branches agree
/// (both produce `0` dB), so the line co-located with the masker
/// itself returns the unattenuated masking-index + SPL.
#[inline]
#[must_use]
pub fn masking_function_vf(dz_bark: f64, x_db: f64) -> Option<f64> {
    // Out-of-range guard. The spec uses half-open `[-3, 8)`; preserve
    // that exactly so `dz = 8.0` produces `None`.
    if !(MASKING_FUNCTION_DZ_LO..MASKING_FUNCTION_DZ_HI).contains(&dz_bark) {
        return None;
    }
    let vf = if dz_bark < -1.0 {
        17.0 * (dz_bark + 1.0) - (0.4 * x_db + 6.0)
    } else if dz_bark < 0.0 {
        (0.4 * x_db + 6.0) * dz_bark
    } else if dz_bark < 1.0 {
        -17.0 * dz_bark
    } else {
        -(dz_bark - 1.0) * (17.0 - 0.15 * x_db) - 17.0
    };
    Some(vf)
}

/// §D.1 Step 6 individual masking threshold `LT` (dB) for a single
/// masker at the target Bark line `z(i)`. Combines the masking-index
/// `av` (tonal or non-tonal per `masker.kind`) and the masking
/// function `vf` per the verbatim spec equation:
///
/// ```text
/// LT_tm[z(j), z(i)] = X_tm[z(j)] + av_tm[z(j)] + vf[z(j), z(i)]   dB
/// LT_nm[z(j), z(i)] = X_nm[z(j)] + av_nm[z(j)] + vf[z(j), z(i)]   dB
/// ```
///
/// Returns `None` when the Bark distance `dz = z(i) - z(j)` is
/// outside `[-3, 8)` — the masker contributes nothing to the global
/// threshold at this line.
#[inline]
#[must_use]
pub fn individual_masking_threshold_db(masker: &Masker, z_i_bark: f64) -> Option<f64> {
    let dz = z_i_bark - masker.z_bark;
    let vf = masking_function_vf(dz, masker.spl_db)?;
    let av = match masker.kind {
        MaskerKind::Tonal => masking_index_tonal(masker.z_bark),
        MaskerKind::NonTonal => masking_index_non_tonal(masker.z_bark),
    };
    Some(masker.spl_db + av + vf)
}

/// §D.1 Step 7 global masking threshold `LTg(i)` in dB at the target
/// Bark line `z(i)`, summing the energy contributions of every
/// in-range masker with the threshold-in-quiet `LTq(i)`:
///
/// ```text
/// LTg(i) = 10 * log10( 10^(LTq(i) / 10)
///                    + Sum 10^(LT_tm[z(j), z(i)] / 10)
///                    + Sum 10^(LT_nm[z(j), z(i)] / 10) )   dB
/// ```
///
/// (Verbatim spec equation; tonal and non-tonal contributions enter
/// the sum identically — the per-classification difference lives in
/// the masking-index `av` already folded into each
/// `individual_masking_threshold_db` term.)
///
/// Maskers outside the `[-3, 8)` Bark window (per
/// [`masking_function_vf`]) contribute nothing — they are dropped
/// from the sum, equivalent to `10^(-inf / 10) = 0`.
///
/// `ltq_db` is the threshold-in-quiet at `z(i)` — the caller derives
/// it from the Annex D Table D.1d / D.1e / D.1f Layer II
/// frequency / Bark / absolute-threshold curve (PNG-only this round;
/// the caller passes the looked-up dB value here directly).
#[must_use]
pub fn global_masking_threshold_db(maskers: &[Masker], z_i_bark: f64, ltq_db: f64) -> f64 {
    // Threshold in quiet contributes 10^(LTq / 10) to the energy sum.
    let mut energy_sum = (10.0_f64).powf(ltq_db / 10.0);
    for masker in maskers {
        if let Some(lt_db) = individual_masking_threshold_db(masker, z_i_bark) {
            energy_sum += (10.0_f64).powf(lt_db / 10.0);
        }
    }
    10.0 * energy_sum.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masking_index_tonal_recovers_spec_formula() {
        // Verbatim spec: `av_tm = -1.525 - 0.275 * z(j) - 4.5`.
        // Spot-check five Bark positions across the audio band.
        for z_j in [0.0_f64, 5.0, 12.0, 20.0, 25.0] {
            let got = masking_index_tonal(z_j);
            let expected = -1.525 - 0.275 * z_j - 4.5;
            assert!(
                (got - expected).abs() < 1.0e-12,
                "av_tm({z_j}) = {got}, expected {expected}",
            );
        }
    }

    #[test]
    fn masking_index_non_tonal_recovers_spec_formula() {
        // Verbatim spec: `av_nm = -1.525 - 0.175 * z(j) - 0.5`.
        for z_j in [0.0_f64, 5.0, 12.0, 20.0, 25.0] {
            let got = masking_index_non_tonal(z_j);
            let expected = -1.525 - 0.175 * z_j - 0.5;
            assert!(
                (got - expected).abs() < 1.0e-12,
                "av_nm({z_j}) = {got}, expected {expected}",
            );
        }
    }

    #[test]
    fn masking_index_tonal_below_non_tonal_at_same_z() {
        // The tonal constant `-4.5` is deeper than the non-tonal
        // `-0.5`, so for any Bark position the tonal masking-index
        // sits below the non-tonal one (a more efficient masker
        // pushes its threshold floor further below its SPL).
        for z_j in [0.0_f64, 5.0, 12.0, 20.0] {
            let tm = masking_index_tonal(z_j);
            let nm = masking_index_non_tonal(z_j);
            assert!(
                tm < nm,
                "tonal {tm} should be < non-tonal {nm} at z(j) = {z_j}",
            );
        }
    }

    #[test]
    fn masking_function_vf_out_of_range_returns_none() {
        // `vf` is defined on `[-3, 8)`. Outside, the masker is
        // ignored — the function returns `None`.
        assert!(masking_function_vf(-3.0001, 60.0).is_none());
        assert!(masking_function_vf(-10.0, 60.0).is_none());
        assert!(masking_function_vf(8.0, 60.0).is_none());
        assert!(masking_function_vf(8.5, 60.0).is_none());
        // Boundary inside the half-open `[-3, 8)` is in-range.
        assert!(masking_function_vf(-3.0, 60.0).is_some());
        assert!(masking_function_vf(7.999, 60.0).is_some());
    }

    #[test]
    fn masking_function_vf_branch_far_left_lobe() {
        // Branch 1, `-3 <= dz < -1`:
        //   vf = 17 * (dz + 1) - (0.4 * X + 6)
        // At dz = -3, X = 60: vf = 17 * (-2) - (24 + 6) = -34 - 30
        //                       = -64.
        let v = masking_function_vf(-3.0, 60.0).unwrap();
        assert!((v - (-64.0)).abs() < 1.0e-12, "vf(-3, 60) = {v}");
        // At dz = -2, X = 80: vf = 17 * (-1) - (32 + 6) = -17 - 38
        //                       = -55.
        let v = masking_function_vf(-2.0, 80.0).unwrap();
        assert!((v - (-55.0)).abs() < 1.0e-12, "vf(-2, 80) = {v}");
    }

    #[test]
    fn masking_function_vf_branch_near_left_lobe() {
        // Branch 2, `-1 <= dz < 0`:
        //   vf = (0.4 * X + 6) * dz
        // At dz = -1, X = 60: vf = (24 + 6) * (-1) = -30.
        let v = masking_function_vf(-1.0, 60.0).unwrap();
        assert!((v - (-30.0)).abs() < 1.0e-12, "vf(-1, 60) = {v}");
        // At dz = -0.5, X = 60: vf = 30 * (-0.5) = -15.
        let v = masking_function_vf(-0.5, 60.0).unwrap();
        assert!((v - (-15.0)).abs() < 1.0e-12, "vf(-0.5, 60) = {v}");
    }

    #[test]
    fn masking_function_vf_branch_near_right_lobe() {
        // Branch 3, `0 <= dz < 1`:
        //   vf = -17 * dz
        // At dz = 0: vf = 0.
        let v = masking_function_vf(0.0, 60.0).unwrap();
        assert!(v.abs() < 1.0e-12, "vf(0, 60) = {v}");
        // At dz = 0.5: vf = -8.5.
        let v = masking_function_vf(0.5, 60.0).unwrap();
        assert!((v - (-8.5)).abs() < 1.0e-12, "vf(0.5, 60) = {v}");
        // At dz = 0.999: vf ≈ -16.983.
        let v = masking_function_vf(0.999, 60.0).unwrap();
        assert!((v - (-17.0 * 0.999)).abs() < 1.0e-12, "vf(0.999, 60) = {v}",);
    }

    #[test]
    fn masking_function_vf_branch_far_right_lobe() {
        // Branch 4, `1 <= dz < 8`:
        //   vf = -(dz - 1) * (17 - 0.15 * X) - 17
        // At dz = 1, X = 60: vf = -0 * (17 - 9) - 17 = -17.
        let v = masking_function_vf(1.0, 60.0).unwrap();
        assert!((v - (-17.0)).abs() < 1.0e-12, "vf(1, 60) = {v}");
        // At dz = 2, X = 60: vf = -1 * (17 - 9) - 17 = -8 - 17
        //                      = -25.
        let v = masking_function_vf(2.0, 60.0).unwrap();
        assert!((v - (-25.0)).abs() < 1.0e-12, "vf(2, 60) = {v}");
        // At dz = 5, X = 80: vf = -4 * (17 - 12) - 17 = -20 - 17
        //                      = -37.
        let v = masking_function_vf(5.0, 80.0).unwrap();
        assert!((v - (-37.0)).abs() < 1.0e-12, "vf(5, 80) = {v}");
    }

    #[test]
    fn masking_function_vf_continuous_at_dz_zero() {
        // Branches 2 and 3 must agree at `dz = 0`: branch 2 gives
        // `(0.4 * X + 6) * 0 = 0`, branch 3 gives `-17 * 0 = 0`.
        // Continuity at the dz = 0 boundary preserves the spec's
        // implicit "masker SPL is the unattenuated peak" property.
        for x in [40.0_f64, 60.0, 80.0, 100.0] {
            // Approach from the left (branch 2).
            let left = masking_function_vf(-1.0e-12, x).unwrap();
            // Exactly at zero (branch 3, since the if-chain checks
            // `< 0` then `< 1`).
            let at = masking_function_vf(0.0, x).unwrap();
            assert!(left.abs() < 1.0e-9, "left limit at X = {x}: {left}");
            assert!(at.abs() < 1.0e-12, "exactly at zero, X = {x}: {at}");
        }
    }

    #[test]
    fn individual_masking_threshold_db_tonal_at_self_is_spl_plus_av() {
        // At `z(i) = z(j)` (dz = 0) the masking function `vf = 0`,
        // so LT_tm = SPL + av_tm = SPL + (-1.525 - 0.275 * z - 4.5).
        let masker = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 10.0,
            spl_db: 80.0,
        };
        let lt = individual_masking_threshold_db(&masker, 10.0).unwrap();
        let expected = 80.0 + masking_index_tonal(10.0);
        assert!(
            (lt - expected).abs() < 1.0e-12,
            "LT_tm(self) = {lt}, expected {expected}",
        );
    }

    #[test]
    fn individual_masking_threshold_db_non_tonal_at_self_is_spl_plus_av() {
        // Same invariant for non-tonal maskers.
        let masker = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: 10.0,
            spl_db: 80.0,
        };
        let lt = individual_masking_threshold_db(&masker, 10.0).unwrap();
        let expected = 80.0 + masking_index_non_tonal(10.0);
        assert!(
            (lt - expected).abs() < 1.0e-12,
            "LT_nm(self) = {lt}, expected {expected}",
        );
    }

    #[test]
    fn individual_masking_threshold_db_returns_none_outside_window() {
        // Masker at z(j) = 5, target at z(i) = 14 -> dz = 9, outside
        // the `[-3, 8)` `vf` window.
        let masker = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 5.0,
            spl_db: 80.0,
        };
        assert!(individual_masking_threshold_db(&masker, 14.0).is_none());
        // Masker at z(j) = 5, target at z(i) = 1.4 -> dz = -3.6,
        // outside the window on the low side.
        assert!(individual_masking_threshold_db(&masker, 1.4).is_none());
    }

    #[test]
    fn individual_masking_threshold_db_tonal_below_non_tonal_at_same_z() {
        // Same masker position + SPL, same target z(i): the tonal
        // individual threshold sits below the non-tonal one (deeper
        // masking-index `av_tm < av_nm`).
        let z_j = 10.0;
        let spl = 80.0;
        let tm = Masker {
            kind: MaskerKind::Tonal,
            z_bark: z_j,
            spl_db: spl,
        };
        let nm = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: z_j,
            spl_db: spl,
        };
        // Test at several z(i) inside the window.
        for z_i in [9.0_f64, 10.0, 11.5, 14.0, 17.0] {
            let lt_t = individual_masking_threshold_db(&tm, z_i).unwrap();
            let lt_n = individual_masking_threshold_db(&nm, z_i).unwrap();
            assert!(
                lt_t < lt_n,
                "tonal LT {lt_t} should be < non-tonal LT {lt_n} at z(i) = {z_i}",
            );
        }
    }

    #[test]
    fn global_masking_threshold_db_no_maskers_is_ltq() {
        // With zero maskers the energy sum is just 10^(LTq/10), so
        // LTg = LTq exactly.
        let ltg = global_masking_threshold_db(&[], 10.0, -5.0);
        assert!(
            (ltg - (-5.0)).abs() < 1.0e-12,
            "LTg(no maskers) = {ltg}, expected -5.0",
        );
    }

    #[test]
    fn global_masking_threshold_db_distant_masker_drops_to_ltq() {
        // A masker outside the `[-3, 8)` window doesn't contribute,
        // so LTg collapses back to LTq.
        let masker = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 5.0,
            spl_db: 80.0,
        };
        let ltg = global_masking_threshold_db(&[masker], 20.0, 10.0);
        assert!(
            (ltg - 10.0).abs() < 1.0e-12,
            "LTg(distant masker) = {ltg}, expected LTq = 10.0",
        );
    }

    #[test]
    fn global_masking_threshold_db_strong_local_masker_dominates_ltq() {
        // A strong nearby masker should drive LTg far above LTq.
        let masker = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 10.0,
            spl_db: 80.0,
        };
        // At z(i) = z(j): LT = 80 + av_tm(10) ≈ 80 - 6.775
        //                    = 73.225 dB, dwarfing LTq = 0.
        let ltg = global_masking_threshold_db(&[masker], 10.0, 0.0);
        let lt_at_self = individual_masking_threshold_db(&masker, 10.0).unwrap();
        // The masker contribution dominates: LTg ≈ LT_at_self.
        assert!(
            (ltg - lt_at_self).abs() < 1.0,
            "LTg {ltg} should be close to LT_at_self {lt_at_self}",
        );
        assert!(ltg > 0.0, "LTg {ltg} should be > LTq = 0");
    }

    #[test]
    fn global_masking_threshold_db_sums_energies_monotonically() {
        // Two maskers stack: LTg with both is strictly above LTg
        // with either alone (power addition is monotone in number of
        // sources).
        let m1 = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 10.0,
            spl_db: 60.0,
        };
        let m2 = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: 11.0,
            spl_db: 60.0,
        };
        let ltq = -10.0;
        let z_i = 10.5;
        let ltg_m1 = global_masking_threshold_db(&[m1], z_i, ltq);
        let ltg_m2 = global_masking_threshold_db(&[m2], z_i, ltq);
        let ltg_both = global_masking_threshold_db(&[m1, m2], z_i, ltq);
        assert!(
            ltg_both > ltg_m1,
            "LTg both {ltg_both} should be > LTg m1 alone {ltg_m1}",
        );
        assert!(
            ltg_both > ltg_m2,
            "LTg both {ltg_both} should be > LTg m2 alone {ltg_m2}",
        );
    }

    #[test]
    fn global_masking_threshold_db_two_equal_powers_add_three_db() {
        // Two equal-power sources sum to exactly +3.0103 dB above
        // either one (`10 * log10(2)`). Use two co-located masker
        // contributions at z(i) = z(j) and dial LTq far below so it
        // doesn't influence the sum.
        let m = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 10.0,
            spl_db: 80.0,
        };
        let z_i = 10.0;
        let ltq = -200.0; // Effectively zero contribution.
        let single = global_masking_threshold_db(&[m], z_i, ltq);
        let double = global_masking_threshold_db(&[m, m], z_i, ltq);
        let expected = single + 10.0 * 2.0_f64.log10();
        assert!(
            (double - expected).abs() < 1.0e-9,
            "double {double} - single {single} = {} dB, expected +3.0103",
            double - single,
        );
    }
}
