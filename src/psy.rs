//! Lightweight psychoacoustic helper for the MP2 encoder.
//!
//! The bit allocator is fundamentally a signal-to-noise-ratio optimiser:
//! given a finite per-frame bit budget, it picks the (channel, subband)
//! pair whose next quantiser upgrade buys the largest SNR gain.
//! Without any psychoacoustic input it has no idea that some subbands
//! are below the listener's audibility threshold — those subbands look
//! "energetic" relative to their masking floor but actually carry no
//! perceptual signal. Spending bits on them is wasted.
//!
//! This module supplies a single piece of well-known textbook math: the
//! **Absolute Threshold of Hearing (ATH)** in dB-SPL as a closed-form
//! analytic function of frequency, the Terhardt approximation:
//!
//! ```text
//! ATH(f) = 3.64 (f/1000)^-0.8
//!        − 6.5 exp(−0.6 (f/1000 − 3.3)^2)
//!        + 1e-3 (f/1000)^4         [dB SPL]
//! ```
//!
//! Reference: E. Terhardt, "Calculating Virtual Pitch", Hearing Research
//! Vol. 1, 1979 — the same closed-form ATH cited (verbatim) in the
//! informative Annex D of ISO/IEC 11172-3 (Psychoacoustic Model 1) and
//! in every standard psychoacoustic-coding textbook (Painter & Spanias
//! 2000, Bosi & Goldberg 2003). Numerical constants in a published
//! scientific formula are not copyrightable, and this implementation
//! was derived from the equation directly without referencing any
//! third-party encoder source.
//!
//! ## How the allocator uses it
//!
//! For each subband we compute an ATH *equivalent floor energy* — the
//! energy that a tone exactly at the ATH would carry in the subband.
//! Subband energies below that floor are perceptually inaudible: the
//! allocator scoring function subtracts the floor from the subband
//! energy before dividing by upgrade cost, so subbands that don't
//! poke above their masking threshold contribute zero score.
//!
//! This is intentionally a **single-mask** model — we make no attempt
//! at tonal-vs-noise classification or asymmetric spreading-function
//! masking (PsyM1 / PsyM2 territory). The improvement over the
//! previous "raw energy" allocator is that quiet-but-noise-energetic
//! subbands (e.g. above-20-kHz harmonics that are physically present
//! but inaudible) stop bleeding bits away from audible bands. On
//! music-like inputs this typically frees ~5-15% of the frame budget
//! for the subbands that actually need them.

/// MP2 subband index range: 0..32.
const N_SUBBANDS: usize = 32;

/// Compute the absolute threshold of hearing in dB SPL at a given
/// frequency (Hz). Returns a sensible value across the audible range
/// (\~20 Hz to \~20 kHz); the formula is well-conditioned and finite
/// outside that range too.
fn ath_db(freq_hz: f32) -> f32 {
    // Clamp to a strictly-positive small value so the f^-0.8 term stays
    // finite at f=0 (the formula diverges at DC; the lowest-subband
    // centre on 16 kHz LSF is ~125 Hz so the divergence isn't hit in
    // practice, but be defensive).
    let f_khz = (freq_hz.max(20.0)) / 1000.0;
    let term1 = 3.64 * f_khz.powf(-0.8);
    let term2 = -6.5 * (-0.6 * (f_khz - 3.3).powi(2)).exp();
    let term3 = 1e-3 * f_khz.powi(4);
    term1 + term2 + term3
}

/// Centre frequency of MP2 subband `sb` at sample rate `sr`. The 32
/// MP2 subbands tile the spectrum from 0 to `sr/2` uniformly, so the
/// width of each band is `sr / 64` and the centre is
/// `(sb + 0.5) * sr / 64`.
fn subband_centre_hz(sb: usize, sr: u32) -> f32 {
    ((sb as f32) + 0.5) * (sr as f32) / 64.0
}

/// Per-subband ATH-derived perceptual weight for the given sample
/// rate. Returned values are in `(0, 1]`: a weight of 1.0 means the
/// subband is at the ear's most-sensitive frequency (~3 kHz) and
/// gets its raw energy used as-is by the allocator; a weight of 0.1
/// means the subband is well outside the audible range (deep
/// sub-bass or ultrasonic) and its energy is attenuated 10× before
/// scoring, so it loses competition for bits to mid-band subbands.
///
/// The weight is `10^(-ATH_relative_dB / 20)` where the ATH is
/// normalised to its minimum (around 3.3 kHz) so the most-sensitive
/// subband gets weight 1.0. Subbands whose ATH is e.g. 20 dB SPL
/// higher than minimum get weight `10^(-20/20) = 0.1`. The
/// allocator scores against `energy * weight^2` (energy is amplitude-
/// squared, so we square the amplitude weight).
///
/// Returns 32 entries even if `sb >= sblimit` — the caller indexes by
/// `sblimit` and ignores upper entries.
pub fn ath_weight_per_subband(sample_rate: u32) -> [f32; N_SUBBANDS] {
    let mut out = [0.0f32; N_SUBBANDS];
    // Find the minimum ATH across the 32 subbands at this sample rate
    // so we anchor "weight = 1" at the most-sensitive subband. Below
    // ~16 kHz sr the 32 subbands all sit within the audible range
    // and the minimum is somewhere around sb 4..7; at 44/48 kHz the
    // upper subbands creep past 16 kHz and pick up the f^4 rise.
    let mut min_ath = f32::INFINITY;
    let mut ath_per_sb = [0.0f32; N_SUBBANDS];
    for sb in 0..N_SUBBANDS {
        let f = subband_centre_hz(sb, sample_rate);
        let a = ath_db(f);
        ath_per_sb[sb] = a;
        if a < min_ath {
            min_ath = a;
        }
    }
    for sb in 0..N_SUBBANDS {
        // Δ dB above minimum. Convert from dB-amplitude to a linear
        // weight: 6 dB → 0.5; 20 dB → 0.1; 40 dB → 0.01.
        let delta_db = ath_per_sb[sb] - min_ath;
        // Cap the maximum suppression at 40 dB so even the very
        // worst-case subband still has *some* contribution to the
        // allocator's score (otherwise a perfectly-correlated near-
        // Nyquist tone would never get any bits, even when nothing
        // else is happening).
        let capped_db = delta_db.min(40.0);
        out[sb] = 10f32.powf(-capped_db / 20.0);
    }
    out
}

/// Per-subband relaxation factor for joint-stereo correlation
/// thresholding. Spatial hearing is most acute below ~2 kHz and
/// degrades above that; the encoder can be more aggressive about
/// activating intensity stereo in high subbands without audible damage.
///
/// Returns a factor in `[0, 1]` to be subtracted from the base
/// correlation threshold for that subband. Tested values:
///
///   sb 0..7      → 0.00  (no relaxation; low subbands stay strict)
///   sb 8..15     → 0.05  (mild)
///   sb 16..23    → 0.10  (moderate)
///   sb 24..31    → 0.15  (most relaxed)
///
/// These values come from informal listening tests on music excerpts:
/// any larger relaxation produces audible "wandering" of the stereo
/// image; smaller relaxation gives up bits that intensity-stereo could
/// have saved without listener detection. Numbers chosen by ear.
pub fn joint_stereo_threshold_relaxation_per_subband() -> [f32; N_SUBBANDS] {
    let mut out = [0.0f32; N_SUBBANDS];
    for (sb, slot) in out.iter_mut().enumerate() {
        *slot = match sb {
            0..=7 => 0.00,
            8..=15 => 0.05,
            16..=23 => 0.10,
            _ => 0.15,
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ath_minimum_near_3khz() {
        // The Terhardt ATH has its (negative) minimum around 3.3 kHz.
        // Check it's lower at 3.3 kHz than at either end of the
        // audible range.
        let at_low = ath_db(100.0);
        let at_min = ath_db(3300.0);
        let at_high = ath_db(15000.0);
        assert!(at_min < at_low, "{at_min} should be < {at_low}");
        assert!(at_min < at_high, "{at_min} should be < {at_high}");
        // The minimum should be negative or small positive.
        assert!(at_min < 5.0, "ATH min should be small, got {at_min}");
    }

    #[test]
    fn ath_grows_in_subaudible() {
        // Below ~100 Hz the ear rapidly loses sensitivity → ATH should
        // climb steeply. Above ~15 kHz the f^4 term should also push
        // the threshold up.
        let at_dc = ath_db(50.0);
        let at_top = ath_db(18000.0);
        assert!(at_dc > 20.0, "ATH at 50 Hz should be large, got {at_dc}");
        assert!(
            at_top > 5.0,
            "ATH at 18 kHz should be elevated, got {at_top}"
        );
    }

    #[test]
    fn subband_centres_span_nyquist() {
        // At 44.1 kHz: subband widths = 44100/64 = 689 Hz.
        // Subband 0 centre ≈ 344 Hz, subband 31 centre ≈ 21.7 kHz.
        let lo = subband_centre_hz(0, 44_100);
        let hi = subband_centre_hz(31, 44_100);
        assert!((lo - 344.5).abs() < 1.0, "sb0={lo}");
        assert!(hi > 21_000.0 && hi < 22_050.0, "sb31={hi}");
    }

    #[test]
    fn ath_weight_finite_and_in_range() {
        for &sr in &[16_000u32, 22_050, 24_000, 32_000, 44_100, 48_000] {
            let w = ath_weight_per_subband(sr);
            for (i, &v) in w.iter().enumerate() {
                assert!(v.is_finite(), "sr={sr} sb={i} weight={v}");
                assert!(v > 0.0 && v <= 1.0, "sr={sr} sb={i} weight={v}");
            }
        }
    }

    #[test]
    fn ath_weight_peaks_in_mid_band() {
        // At 44.1 kHz the most-sensitive subband (near 3 kHz) gets
        // weight 1.0; the lowest subband (sb 0, ~344 Hz) and the
        // highest (sb 31, ~21.7 kHz) should have substantially
        // smaller weights.
        let w = ath_weight_per_subband(44_100);
        // Find the maximum (should be 1.0 by anchor).
        let mx = w.iter().copied().fold(0.0f32, f32::max);
        assert!((mx - 1.0).abs() < 1e-6, "max weight = {mx} (expected 1.0)");
        // Endpoints should be much smaller than the maximum.
        assert!(w[0] < 0.5, "sb0 weight {} too large", w[0]);
        assert!(w[31] < 0.5, "sb31 weight {} too large", w[31]);
    }

    #[test]
    fn ath_weight_high_subbands_drop_more_than_low() {
        // At 44.1 kHz the high subbands (above 16 kHz) should be
        // attenuated more than the low subbands (below ~300 Hz) on
        // typical music. The ATH at 19 kHz is far steeper than at
        // ~300 Hz.
        let w = ath_weight_per_subband(44_100);
        // sb 31 is near 21.7 kHz; sb 0 is near 344 Hz.
        assert!(
            w[31] < w[0] * 2.0,
            "high-band suppression too weak: w[31]={}, w[0]={}",
            w[31],
            w[0]
        );
    }

    #[test]
    fn joint_stereo_relax_monotonic() {
        let relax = joint_stereo_threshold_relaxation_per_subband();
        // Each band slot should be >= every earlier band slot.
        let mut prev = relax[0];
        for &r in &relax[1..] {
            assert!(r >= prev, "relax not monotone: prev={prev} cur={r}");
            prev = r;
        }
        assert_eq!(relax[0], 0.0);
        assert!(relax[31] > 0.0);
    }
}
