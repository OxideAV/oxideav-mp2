//! CCITT / ITU-T J.17 emphasis curve (`emphasis == '11'`, §2.4.2.4).
//!
//! The frame header's `'11'` emphasis value selects the CCITT J.17
//! pre-/de-emphasis characteristic (ISO/IEC 11172-3 §2.4.2.3 table;
//! reused by ISO/IEC 13818-3). The curve itself is staged in
//! `docs/audio/mp3/mpeg-audio-emphasis-j17-deemphasis.md` (the #236
//! J.17 note): a **first-order shelf** whose *pre-emphasis* has an
//! s-plane zero at ≈ 477.5 Hz and a pole at ≈ 4134 Hz, with the
//! insertion-loss magnitude
//!
//! ```text
//!                  75 + (ω/ω0)²
//! L(ω) = 10·log10 --------------  dB,   ω = 2πf,  ω0 = 3000 rad/s
//!                   1 + (ω/ω0)²
//! ```
//!
//! spanning `10·log10(75) = 18.75 dB` between its DC and
//! high-frequency asymptotes (corner frequencies `ω0/2π = 477.5 Hz`
//! and `ω0·√75/2π = 4134 Hz`). Measured curves must stay within
//! **± 0.25 dB** of the theoretical characteristic once levels are
//! aligned. The **de-emphasis** a decoder applies is the reciprocal
//! shelf — a high-frequency roll-off (pole 477.5 Hz, zero 4134 Hz).
//!
//! ## Gain normalisation
//!
//! The staged note fixes the curve's *shape* (the formula above, with
//! the ± 0.25 dB tolerance applied "once the 800 Hz levels are
//! aligned") but the absolute level convention — the note quotes a
//! "6.5 dB @ 800 Hz" sound-programme alignment figure whose reference
//! level is the (TIES-gated) J.17 network's own — is a flat gain
//! constant that cancels in any matched pre-/de-emphasis pair. This
//! implementation normalises the **de-emphasis to unity gain at DC**
//! (and the pre-emphasis to its exact inverse), the same convention
//! the crate's 50/15 µs filter uses: a constant signal passes
//! unchanged and high frequencies are shelved down by up to 18.75 dB.
//!
//! ## Digital realisation
//!
//! A single first-order digital section cannot track the analog shelf
//! within the ± 0.25 dB tolerance across the Layer II rates — the
//! bilinear transform's frequency warping bends the top octave (at
//! 16 kHz the 4134 Hz corner sits past half Nyquist and a plain
//! bilinear shelf is ≈ 1 dB off there). The staged note's own
//! reference digital fit (§3.3) therefore uses an order-3 cascade of
//! *real* poles/zeros: the shelf pair plus two near-cancelling
//! correction sections. This module derives the same structure
//! independently, **per sample rate**, by a damped Gauss–Newton
//! (Levenberg–Marquardt) least-squares fit of the order-3 cascade's
//! log-magnitude to the analytic curve on a log-spaced frequency grid
//! — plain textbook numerical optimisation, seeded from the bilinear
//! transform of the analog shelf. The fit residual is asserted to stay
//! under 0.02 dB at every rate — more than an order of magnitude
//! inside the ± 0.25 dB tolerance, and tighter against the staged
//! formula than the note's own 44.1 kHz reference fit (≈ 0.008 dB) —
//! and at 44.1 kHz the result is cross-checked against that reference
//! fit.
//!
//! All poles and zeros are constrained inside the unit circle, so the
//! de-emphasis is stable *and* minimum-phase — its exact inverse (the
//! encoder's pre-emphasis) is stable too.

use crate::deemphasis::Section;
use std::sync::Mutex;

/// The J.17 shelf's characteristic angular frequency `ω0 = 3000 rad/s`
/// (lower corner `ω0/2π ≈ 477.5 Hz`).
pub const J17_OMEGA0_RAD_PER_S: f64 = 3000.0;

/// The J.17 shelf's asymptote power ratio: the pre-emphasis boosts (and
/// the de-emphasis attenuates) high frequencies by `10·log10(75) =
/// 18.75 dB` relative to DC. The upper corner is `ω0·√75/2π ≈ 4134 Hz`.
pub const J17_SHELF_POWER_RATIO: f64 = 75.0;

/// Analytic J.17 **de-emphasis** power gain at analog frequency `f_hz`,
/// normalised to unity at DC (see the module docs): the reciprocal of
/// the staged note's pre-emphasis insertion-loss formula, re-anchored so
/// `gain(0) = 1` and `gain(∞) = 1/75`.
#[must_use]
pub fn deemphasis_power_gain(f_hz: f64) -> f64 {
    let x = 2.0 * std::f64::consts::PI * f_hz / J17_OMEGA0_RAD_PER_S;
    let x2 = x * x;
    (J17_SHELF_POWER_RATIO + x2) / (J17_SHELF_POWER_RATIO * (1.0 + x2))
}

/// [`deemphasis_power_gain`] in decibels: `0 dB` at DC falling to
/// `−18.75 dB` at the high-frequency asymptote.
#[must_use]
pub fn deemphasis_gain_db(f_hz: f64) -> f64 {
    10.0 * deemphasis_power_gain(f_hz).log10()
}

/// Number of first-order sections in the J.17 digital realisation.
pub const J17_SECTIONS: usize = 3;

/// The J.17 de-emphasis as [`J17_SECTIONS`] cascaded first-order
/// sections for output sample rate `fs` (Hz), fitted to
/// [`deemphasis_gain_db`] as described in the module docs. Results are
/// cached per rate (the fit is deterministic, so re-derivation would
/// yield identical sections).
pub(crate) fn deemphasis_sections(fs: u32) -> [Section; J17_SECTIONS] {
    static CACHE: Mutex<Vec<(u32, [Section; J17_SECTIONS])>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().expect("J.17 section cache poisoned");
    if let Some((_, s)) = cache.iter().find(|(rate, _)| *rate == fs) {
        return *s;
    }
    let fit = fit_deemphasis(f64::from(fs));
    let sections = fit.into_sections();
    cache.push((fs, sections));
    sections
}

/// A fitted order-3 real-coefficient cascade: `gain_db` plus three
/// zero/pole pairs (all strictly inside the unit circle).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CascadeFit {
    pub gain_db: f64,
    pub zeros: [f64; J17_SECTIONS],
    pub poles: [f64; J17_SECTIONS],
}

impl CascadeFit {
    /// Cascade log-magnitude (dB) at digital frequency `theta` (rad,
    /// `θ = 2πf/fs`), with `c = cos θ` precomputed by the caller:
    /// `|1 − r·e^{−jθ}|² = 1 − 2r·cosθ + r²` per zero/pole `r`.
    fn magnitude_db(&self, c: f64) -> f64 {
        let mut db = self.gain_db;
        for i in 0..J17_SECTIONS {
            db += 10.0 * (1.0 - 2.0 * self.zeros[i] * c + self.zeros[i] * self.zeros[i]).log10();
            db -= 10.0 * (1.0 - 2.0 * self.poles[i] * c + self.poles[i] * self.poles[i]).log10();
        }
        db
    }

    /// Pair the fitted zeros/poles into first-order [`Section`]s
    /// (sorted so near-cancelling correction pairs align), folding the
    /// overall linear gain into the first section.
    fn into_sections(mut self) -> [Section; J17_SECTIONS] {
        self.zeros.sort_by(|a, b| a.total_cmp(b));
        self.poles.sort_by(|a, b| a.total_cmp(b));
        let g = 10f64.powf(self.gain_db / 20.0);
        let mut out = [Section {
            b0: 1.0,
            b1: 0.0,
            a1: 0.0,
        }; J17_SECTIONS];
        let pairs = out.iter_mut().zip(self.zeros.iter().zip(&self.poles));
        for (i, (slot, (&zero, &pole))) in pairs.enumerate() {
            let gain = if i == 0 { g } else { 1.0 };
            // H_i(z) = gain·(1 − q·z⁻¹)/(1 − p·z⁻¹)
            *slot = Section {
                b0: gain,
                b1: -gain * zero,
                a1: -pole,
            };
        }
        out
    }
}

/// Log-spaced grid points for the least-squares fit (1 Hz → Nyquist).
const FIT_GRID_LOG: usize = 320;
/// Additional linear grid points across the top octave (Nyquist/2 →
/// Nyquist), where the bilinear warp concentrates the residual.
const FIT_GRID_TOP: usize = 192;
/// Maximum Levenberg–Marquardt iterations per start.
const FIT_MAX_ITERS: usize = 400;

/// Fit the order-3 de-emphasis cascade for sample rate `fs` (Hz).
///
/// Free parameters: overall gain (dB) plus three zeros and three poles,
/// each parametrised as `r = tanh(u)` so the optimiser cannot leave the
/// unit circle. Several starts are tried (the correction-section
/// locations are mildly non-convex) and the lowest-residual result
/// wins.
pub(crate) fn fit_deemphasis(fs: f64) -> CascadeFit {
    let nyquist = fs / 2.0;
    // Digital-frequency grid with the analytic target: log-spaced over
    // the whole band plus a linear top-octave refinement.
    let f_min = 1.0_f64;
    let mut freqs = Vec::with_capacity(FIT_GRID_LOG + FIT_GRID_TOP);
    for j in 0..FIT_GRID_LOG {
        let frac = j as f64 / (FIT_GRID_LOG - 1) as f64;
        freqs.push(f_min * (nyquist / f_min).powf(frac));
    }
    for j in 1..=FIT_GRID_TOP {
        let frac = j as f64 / FIT_GRID_TOP as f64;
        freqs.push(nyquist * (0.5 + 0.5 * frac) * (1.0 - 1e-12));
    }
    let cos_theta: Vec<f64> = freqs
        .iter()
        .map(|f| (std::f64::consts::PI * f / nyquist).cos())
        .collect();
    let target_db: Vec<f64> = freqs.iter().map(|&f| deemphasis_gain_db(f)).collect();

    // Bilinear-transform seed for the main shelf: analog de-emphasis
    // zero at ω0·√75 (4134 Hz), pole at ω0 (477.5 Hz), k = 2·fs.
    let k = 2.0 * fs;
    let omega_zero = J17_OMEGA0_RAD_PER_S * J17_SHELF_POWER_RATIO.sqrt();
    let omega_pole = J17_OMEGA0_RAD_PER_S;
    let shelf_zero = (k - omega_zero) / (k + omega_zero);
    let shelf_pole = (k - omega_pole) / (k + omega_pole);

    // Candidate starts for the two near-cancelling correction pairs
    // (their converged locations shift with the rate's warp severity).
    let starts: [[(f64, f64); 2]; 6] = [
        [(-0.60, -0.63), (-0.20, -0.23)],
        [(-0.45, -0.50), (-0.10, -0.14)],
        [(-0.75, -0.78), (-0.35, -0.38)],
        [(-0.85, -0.87), (-0.55, -0.58)],
        [(0.25, 0.20), (-0.40, -0.44)],
        [(0.10, 0.06), (-0.65, -0.68)],
    ];

    let max_err = |fit: &CascadeFit| {
        cos_theta
            .iter()
            .zip(&target_db)
            .map(|(&c, &t)| (fit.magnitude_db(c) - t).abs())
            .fold(0.0_f64, f64::max)
    };

    let mut best: Option<(f64, CascadeFit)> = None;
    for start in starts {
        let mut fit = CascadeFit {
            gain_db: 0.0,
            zeros: [shelf_zero, start[0].0, start[1].0],
            poles: [shelf_pole, start[0].1, start[1].1],
        };
        // Match the DC (θ = 0, c = 1) gain exactly at the seed.
        fit.gain_db = target_db[0] - fit.magnitude_db(1.0);
        levenberg_marquardt(&mut fit, &cos_theta, &target_db);
        let err = max_err(&fit);
        if best.map_or(true, |(b, _)| err < b) {
            best = Some((err, fit));
        }
    }
    let mut fit = best.expect("at least one fit start").1;
    // Re-anchor the DC gain exactly: the least-squares optimum leaves a
    // (sub-millidecibel) offset at θ = 0; shift the whole curve so the
    // de-emphasis is *exactly* unity at DC, the documented
    // normalisation (a flat shift far below the fit residual).
    fit.gain_db += deemphasis_gain_db(0.0) - fit.magnitude_db(1.0);
    fit
}

/// Damped Gauss–Newton on the cascade's log-magnitude residuals.
/// Returns the final sum of squared residuals (dB²).
fn levenberg_marquardt(fit: &mut CascadeFit, cos_theta: &[f64], target_db: &[f64]) -> f64 {
    const NPARAM: usize = 1 + 2 * J17_SECTIONS;
    let ln10 = std::f64::consts::LN_10;

    // Unconstrained parameter vector: [gain_db, u(zeros), u(poles)].
    let mut params = [0.0; NPARAM];
    params[0] = fit.gain_db;
    for i in 0..J17_SECTIONS {
        params[1 + i] = fit.zeros[i].atanh();
        params[1 + J17_SECTIONS + i] = fit.poles[i].atanh();
    }
    let decode = |p: &[f64; NPARAM]| CascadeFit {
        gain_db: p[0],
        zeros: [p[1].tanh(), p[2].tanh(), p[3].tanh()],
        poles: [p[4].tanh(), p[5].tanh(), p[6].tanh()],
    };
    let sse_of = |p: &[f64; NPARAM]| {
        let f = decode(p);
        cos_theta
            .iter()
            .zip(target_db)
            .map(|(&c, &t)| {
                let r = f.magnitude_db(c) - t;
                r * r
            })
            .sum::<f64>()
    };

    let mut sse = sse_of(&params);
    let mut lambda = 1e-3;
    for _ in 0..FIT_MAX_ITERS {
        let current = decode(&params);
        // Normal equations J'J and J'r over the grid.
        let mut jtj = [[0.0; NPARAM]; NPARAM];
        let mut jtr = [0.0; NPARAM];
        for (&c, &t) in cos_theta.iter().zip(target_db) {
            let resid = current.magnitude_db(c) - t;
            let mut grad = [0.0; NPARAM];
            grad[0] = 1.0;
            for i in 0..J17_SECTIONS {
                let q = current.zeros[i];
                grad[1 + i] = (10.0 / ln10) * (2.0 * q - 2.0 * c) / (1.0 - 2.0 * q * c + q * q)
                    * (1.0 - q * q);
                let p = current.poles[i];
                grad[1 + J17_SECTIONS + i] = -(10.0 / ln10) * (2.0 * p - 2.0 * c)
                    / (1.0 - 2.0 * p * c + p * p)
                    * (1.0 - p * p);
            }
            for a in 0..NPARAM {
                jtr[a] += grad[a] * resid;
                for b in a..NPARAM {
                    jtj[a][b] += grad[a] * grad[b];
                }
            }
        }
        // Mirror the accumulated upper triangle into the lower one.
        for a in 1..NPARAM {
            let (upper, lower) = jtj.split_at_mut(a);
            for (b, upper_row) in upper.iter().enumerate() {
                lower[0][b] = upper_row[a];
            }
        }

        // Damped step: (J'J + λ·diag(J'J))·δ = J'r, accept on improvement.
        let mut accepted = false;
        for _ in 0..24 {
            let mut a = jtj;
            for (d, row) in a.iter_mut().enumerate() {
                row[d] += lambda * jtj[d][d].max(1e-12);
            }
            let Some(delta) = solve(&mut a, &jtr) else {
                lambda *= 4.0;
                continue;
            };
            let mut trial = params;
            for (t, d) in trial.iter_mut().zip(&delta) {
                *t -= d;
            }
            let trial_sse = sse_of(&trial);
            if trial_sse < sse {
                let rel = (sse - trial_sse) / sse.max(1e-300);
                params = trial;
                sse = trial_sse;
                lambda = (lambda / 3.0).max(1e-12);
                accepted = true;
                if rel < 1e-14 {
                    // Converged to numerical precision.
                    *fit = decode(&params);
                    return sse;
                }
                break;
            }
            lambda *= 4.0;
            if lambda > 1e12 {
                break;
            }
        }
        if !accepted {
            break;
        }
    }
    *fit = decode(&params);
    sse
}

/// Solve the `n×n` system `A·x = b` by Gaussian elimination with
/// partial pivoting; `None` if singular. `A` is clobbered.
fn solve<const N: usize>(a: &mut [[f64; N]; N], b: &[f64; N]) -> Option<[f64; N]> {
    let mut x = *b;
    for col in 0..N {
        let pivot = (col..N).max_by(|&r1, &r2| a[r1][col].abs().total_cmp(&a[r2][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        x.swap(col, pivot);
        let (top, bottom) = a.split_at_mut(col + 1);
        let pivot_row = &top[col];
        let (x_top, x_bottom) = x.split_at_mut(col + 1);
        let x_pivot = x_top[col];
        for (row, xr) in bottom.iter_mut().zip(x_bottom.iter_mut()) {
            let factor = row[col] / pivot_row[col];
            for (rc, &pc) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *rc -= factor * pc;
            }
            *xr -= factor * x_pivot;
        }
    }
    for col in (0..N).rev() {
        for row in 0..col {
            let factor = a[row][col] / a[col][col];
            x[row] -= factor * x[col];
        }
        x[col] /= a[col][col];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All six Layer II output rates (MPEG-1 + MPEG-2 LSF).
    const RATES: [u32; 6] = [16_000, 22_050, 24_000, 32_000, 44_100, 48_000];

    /// Response (dB) of a section cascade at digital frequency θ.
    fn cascade_db(sections: &[Section], theta: f64) -> f64 {
        let c = theta.cos();
        let mut db = 0.0;
        for s in sections {
            let num = s.b0 * s.b0 + 2.0 * s.b0 * s.b1 * c + s.b1 * s.b1;
            let den = 1.0 + 2.0 * s.a1 * c + s.a1 * s.a1;
            db += 10.0 * (num / den).log10();
        }
        db
    }

    #[test]
    fn analytic_curve_matches_staged_anchors() {
        // Asymptote span: 10·log10(75) = 18.75 dB.
        let span = 10.0 * J17_SHELF_POWER_RATIO.log10();
        assert!((span - 18.750_612_633).abs() < 1e-6);
        assert!((deemphasis_gain_db(0.0)).abs() < 1e-12, "DC-unity");
        assert!((deemphasis_gain_db(1e9) + span).abs() < 1e-3, "HF shelf");
        // Staged note: the formula gives ≈ 13.10 dB at 800 Hz relative
        // to the high-frequency asymptote.
        let rel_hf = deemphasis_gain_db(800.0) + span;
        assert!(
            (rel_hf - 13.104_6).abs() < 1e-3,
            "800 Hz point {rel_hf} dB above the HF asymptote"
        );
        // Corner frequencies 477.5 Hz and 4134 Hz.
        assert!((J17_OMEGA0_RAD_PER_S / (2.0 * std::f64::consts::PI) - 477.46).abs() < 0.01);
        let f_high =
            J17_OMEGA0_RAD_PER_S * J17_SHELF_POWER_RATIO.sqrt() / (2.0 * std::f64::consts::PI);
        assert!((f_high - 4134.9).abs() < 0.1);
    }

    #[test]
    fn fit_is_within_tolerance_at_every_layer2_rate() {
        // The staged note requires ± 0.25 dB against the theoretical
        // curve; the order-3 fit must be far inside that at every rate,
        // measured on a dense grid that is NOT the fit grid, all the
        // way to Nyquist. The observed worst case is ≈ 0.013 dB at
        // 16 kHz (pinned at Nyquist, where a digital filter's response
        // flattens against the still-sloping analog curve) falling to
        // ≈ 0.002 dB at 48 kHz — for calibration, the staged note's own
        // 44.1 kHz reference fit sits ≈ 0.008 dB from the note's
        // analytic formula.
        for fs in RATES {
            let sections = deemphasis_sections(fs);
            let nyquist = f64::from(fs) / 2.0;
            let mut worst = 0.0_f64;
            for j in 0..=4000 {
                let f = 10.0 * (nyquist / 10.0).powf(j as f64 / 4000.0);
                let theta = std::f64::consts::PI * f / nyquist;
                let err = (cascade_db(&sections, theta) - deemphasis_gain_db(f)).abs();
                worst = worst.max(err);
            }
            assert!(
                worst < 0.02,
                "fs={fs}: max deviation {worst} dB exceeds 0.02 dB"
            );
        }
    }

    #[test]
    fn fit_is_stable_and_minimum_phase() {
        for fs in RATES {
            let fit = fit_deemphasis(f64::from(fs));
            for i in 0..J17_SECTIONS {
                assert!(
                    fit.poles[i].abs() < 1.0,
                    "fs={fs}: pole outside unit circle"
                );
                assert!(
                    fit.zeros[i].abs() < 1.0,
                    "fs={fs}: zero outside unit circle"
                );
            }
        }
    }

    #[test]
    fn fit_matches_staged_reference_three_pole_fit_at_44100() {
        // The staged J.17 note (§3.3) publishes a reference 3-pole
        // minimax fit of the 44.1 kHz *pre-emphasis*; the de-emphasis is
        // its inverse (zeros ↔ poles swapped). The note gives no gain,
        // so compare shapes with the DC levels aligned. Both that fit
        // and ours approximate the same analytic curve to well under a
        // millidecibel, so the aligned responses must agree closely.
        let ref_zeros = [-0.633_155_6, -0.210_337_3, 0.554_859_5]; // = pre-emphasis poles
        let ref_poles = [-0.631_937_6, -0.207_684_3, 0.934_295_6]; // = pre-emphasis zeros
        let reference = CascadeFit {
            gain_db: 0.0,
            zeros: ref_zeros,
            poles: ref_poles,
        };
        let ours = deemphasis_sections(44_100);
        let nyquist = 22_050.0;
        let ref_dc = reference.magnitude_db(1.0);
        let ours_dc = cascade_db(&ours, 0.0);
        let mut worst = 0.0_f64;
        for j in 0..=2000 {
            let f = 20.0 * (21_000.0_f64 / 20.0).powf(j as f64 / 2000.0);
            let theta = std::f64::consts::PI * f / nyquist;
            let ref_db = reference.magnitude_db(theta.cos()) - ref_dc;
            let ours_db = cascade_db(&ours, theta) - ours_dc;
            worst = worst.max((ref_db - ours_db).abs());
        }
        assert!(
            worst < 0.01,
            "44.1 kHz fit deviates {worst} dB from the staged reference fit"
        );
    }

    #[test]
    fn fitted_shelf_section_tracks_the_bilinear_seed_at_44100() {
        // The dominant (shelf) pole/zero at 44.1 kHz must sit close to
        // the bilinear transform of the analog corners — the correction
        // sections only bend the top octave.
        let fit = fit_deemphasis(44_100.0);
        let max_pole = fit.poles.iter().copied().max_by(f64::total_cmp).unwrap();
        let max_zero = fit.zeros.iter().copied().max_by(f64::total_cmp).unwrap();
        // Analog pole at ω0 (477.5 Hz): (k − ω0)/(k + ω0) = 0.93421…
        assert!((max_pole - 0.9342).abs() < 0.002, "shelf pole {max_pole}");
        // Analog zero at ω0·√75 (4134 Hz): ≈ 0.5449 before warp
        // correction; the fit lands within a few hundredths.
        assert!((max_zero - 0.5449).abs() < 0.05, "shelf zero {max_zero}");
    }

    #[test]
    fn sections_cache_is_consistent() {
        let a = deemphasis_sections(32_000);
        let b = deemphasis_sections(32_000);
        assert_eq!(a, b);
    }
}
