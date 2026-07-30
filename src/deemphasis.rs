//! §2.4.2.4 output de-emphasis.
//!
//! The Layer II frame header carries a two-bit `emphasis` field
//! (ISO/IEC 11172-3 §2.4.2.3, table on PDF page 23) whose §2.4.2.4
//! semantics are: *"emphasis — indicates the type of de-emphasis that
//! shall be used."* When the encoder applied pre-emphasis, a conforming
//! decoder undoes it on the reconstructed PCM before delivery:
//!
//! | `emphasis` | meaning |
//! |------------|---------|
//! | `'00'`     | none — output is delivered unfiltered |
//! | `'01'`     | 50/15 µs — the two-time-constant shelving de-emphasis |
//! | `'10'`     | reserved (rejected at [`crate::header::FrameHeader::parse`]) |
//! | `'11'`     | CCITT J.17 |
//!
//! ## 50/15 µs — derivation
//!
//! The `'01'` curve is the classic two-time-constant emphasis defined
//! by a pole/zero pair at time constants `τ1 = 50 µs` and `τ2 = 15 µs`.
//! The pre-emphasis boost applied at encode time is the analog transfer
//! function
//!
//! ```text
//!            1 + s·τ1
//!   H_pre(s) = --------      (τ1 = 50 µs, τ2 = 15 µs)
//!            1 + s·τ2
//! ```
//!
//! so the de-emphasis a decoder applies is its exact inverse
//!
//! ```text
//!              1 + s·τ2
//!   H_deemph(s) = --------.
//!              1 + s·τ1
//! ```
//!
//! This is realised as a first-order digital filter by the standard
//! bilinear transform `s = k·(1 − z⁻¹)/(1 + z⁻¹)` with `k = 2·fs`
//! (`fs` = the frame's output sample rate). Substituting and dividing
//! numerator and denominator by `(1 + τ1·k)` gives the direct-form-I
//! difference equation
//!
//! ```text
//!   y[n] = b0·x[n] + b1·x[n−1] − a1·y[n−1]
//! ```
//!
//! with
//!
//! ```text
//!   b0 = (1 + τ2·k) / (1 + τ1·k)
//!   b1 = (1 − τ2·k) / (1 + τ1·k)
//!   a1 = (1 − τ1·k) / (1 + τ1·k).
//! ```
//!
//! By construction the DC gain `H(z=1) = (b0 + b1)/(1 + a1) = 1` (the
//! de-emphasis leaves a DC / constant signal untouched) and the
//! high-frequency asymptote is `τ2/τ1 = 0.3` (−10.458 dB), so a
//! pre-emphasised high-frequency boost is exactly cancelled.
//!
//! Every constant above is derived from the `50 µs` / `15 µs` time
//! constants and the textbook bilinear transform — no numeric table is
//! read from any source.
//!
//! ## CCITT J.17 (`'11'`)
//!
//! The J.17 characteristic — a first-order shelf with the pre-emphasis
//! zero at ≈ 477.5 Hz and pole at ≈ 4134 Hz, 18.75 dB asymptote span —
//! is read from the staged ITU-T Rec. J.17 (11/88) PDF
//! (`docs/audio/mp3/T-REC-J.17-198811-I.pdf`, clean-room note
//! `mpeg-audio-emphasis-j17-deemphasis.md`) and implemented in
//! [`crate::j17`], normalised to **unity gain at DC** — the resolved
//! ask-#256 convention, the only one consistent with ISO/IEC 11172-3's
//! −1.0 … +1.0 decoder-output-range clause (see [`crate::j17`]).
//! Because a single bilinear first-order section cannot hold the
//! curve's ± 0.25 dB tolerance across the Layer II rates, the digital
//! realisation is an order-3 cascade of real-pole/zero sections fitted
//! per sample rate (see the [`crate::j17`] module docs for the
//! derivation and the Table 1/J.17 pinning tests).
//! Both [`DeEmphasis::ccitt_j17`] (decode) and
//! [`PreEmphasis::ccitt_j17`] (its exact inverse, encode) are exposed,
//! and [`DeEmphasis::for_header`] selects the curve for
//! `emphasis == '11'` frames.

use crate::header::{Emphasis, FrameHeader};

/// One first-order direct-form-I stage of an emphasis filter:
/// `H(z) = (b0 + b1·z⁻¹) / (1 + a1·z⁻¹)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Section {
    /// Numerator tap on `x[n]`.
    pub b0: f64,
    /// Numerator tap on `x[n−1]`.
    pub b1: f64,
    /// Denominator tap on `y[n−1]` (`a0` is 1).
    pub a1: f64,
}

impl Section {
    /// The exact inverse stage: `1/H(z) = (1 + a1·z⁻¹)/(b0 + b1·z⁻¹)`,
    /// i.e. `(b0, b1, a1) ↦ (1/b0, a1/b0, b1/b0)`. Stable whenever the
    /// original section is minimum-phase (its zero inside the unit
    /// circle).
    #[must_use]
    pub fn inverse(&self) -> Section {
        Section {
            b0: 1.0 / self.b0,
            b1: self.a1 / self.b0,
            a1: self.b1 / self.b0,
        }
    }
}

/// Maximum number of cascaded first-order sections an emphasis filter
/// uses (the J.17 realisation needs [`crate::j17::J17_SECTIONS`] = 3;
/// 50/15 µs needs one).
pub const MAX_SECTIONS: usize = crate::j17::J17_SECTIONS;

/// §2.4.2.4 de-emphasis IIR — a short cascade of first-order sections,
/// one instance per output channel. Per-section state (`x[n−1]`,
/// `y[n−1]`) persists across frames so the filter has no per-frame
/// discontinuity — a single logical stream shares one filter per
/// channel, re-created only on a decoder reset.
#[derive(Debug, Clone, Copy)]
pub struct DeEmphasis {
    sections: [Section; MAX_SECTIONS],
    state: [(f64, f64); MAX_SECTIONS],
    len: usize,
}

/// 50/15 µs first time constant (the `τ1 = 50 µs` de-emphasis pole).
pub const TAU1_50US: f64 = 50e-6;
/// 50/15 µs second time constant (the `τ2 = 15 µs` zero).
pub const TAU2_15US: f64 = 15e-6;

/// Compute one direct-form-I step through a section cascade, advancing
/// each section's `(x[n−1], y[n−1])` state.
#[inline]
fn cascade_step(
    sections: &[Section; MAX_SECTIONS],
    state: &mut [(f64, f64); MAX_SECTIONS],
    len: usize,
    x: f64,
) -> f64 {
    let mut v = x;
    for i in 0..len {
        let s = &sections[i];
        let (x1, y1) = state[i];
        let y = s.b0 * v + s.b1 * x1 - s.a1 * y1;
        state[i] = (v, y);
        v = y;
    }
    v
}

/// The single 50/15 µs de-emphasis section (bilinear transform of the
/// two-time-constant curve — see the module docs).
fn fifty_fifteen_deemphasis_section(fs: u32) -> Section {
    let k = 2.0 * f64::from(fs);
    let t1k = TAU1_50US * k;
    let t2k = TAU2_15US * k;
    let denom = 1.0 + t1k;
    Section {
        b0: (1.0 + t2k) / denom,
        b1: (1.0 - t2k) / denom,
        a1: (1.0 - t1k) / denom,
    }
}

impl DeEmphasis {
    fn from_sections(sections: &[Section]) -> Self {
        let mut all = [Section {
            b0: 1.0,
            b1: 0.0,
            a1: 0.0,
        }; MAX_SECTIONS];
        all[..sections.len()].copy_from_slice(sections);
        DeEmphasis {
            sections: all,
            state: [(0.0, 0.0); MAX_SECTIONS],
            len: sections.len(),
        }
    }

    /// Build the 50/15 µs de-emphasis filter for output sample rate
    /// `fs` (Hz), deriving the coefficients from the time constants via
    /// the bilinear transform (see the module docs). State starts at
    /// rest (`x[−1] = y[−1] = 0`).
    #[must_use]
    pub fn fifty_fifteen(fs: u32) -> Self {
        Self::from_sections(&[fifty_fifteen_deemphasis_section(fs)])
    }

    /// Build the CCITT J.17 de-emphasis filter for output sample rate
    /// `fs` (Hz) — the order-3 minimum-phase cascade fitted to the
    /// staged J.17 shelf (see [`crate::j17`]). State starts at rest.
    #[must_use]
    pub fn ccitt_j17(fs: u32) -> Self {
        Self::from_sections(&crate::j17::deemphasis_sections(fs))
    }

    /// Build the de-emphasis filter a `header` calls for, or [`None`]
    /// when the PCM is delivered unfiltered:
    ///
    /// * [`Emphasis::None`] → `None` (deliver PCM unfiltered);
    /// * [`Emphasis::FiftyFifteen`] → the 50/15 µs filter at
    ///   `header.sample_rate`;
    /// * [`Emphasis::CcittJ17`] → the J.17 filter at
    ///   `header.sample_rate`.
    #[must_use]
    pub fn for_header(header: &FrameHeader) -> Option<Self> {
        match header.emphasis {
            Emphasis::FiftyFifteen => Some(Self::fifty_fifteen(header.sample_rate)),
            Emphasis::CcittJ17 => Some(Self::ccitt_j17(header.sample_rate)),
            Emphasis::None => None,
        }
    }

    /// Filter one sample, advancing each section's `x[n−1]` / `y[n−1]`
    /// state.
    #[must_use]
    #[inline]
    pub fn process_sample(&mut self, x: f64) -> f64 {
        cascade_step(&self.sections, &mut self.state, self.len, x)
    }

    /// Filter a whole channel's PCM block in place.
    pub fn process_in_place(&mut self, pcm: &mut [f64]) {
        for s in pcm.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// The direct-form-I coefficients `(b0, b1, a1)` of the **first**
    /// (for 50/15 µs: the only) section; `a0` is 1. See [`Self::sections`]
    /// for the whole cascade.
    #[must_use]
    pub fn coefficients(&self) -> (f64, f64, f64) {
        (
            self.sections[0].b0,
            self.sections[0].b1,
            self.sections[0].a1,
        )
    }

    /// The active first-order sections of the cascade, in processing
    /// order. Comparing two filters' section slices tells whether they
    /// realise the same curve at the same rate (state excluded).
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections[..self.len]
    }
}

/// Encoder-side §2.4.2.4 **pre-emphasis** — the exact inverse of
/// [`DeEmphasis`]. Applying pre-emphasis before quantization and
/// signalling the matching `emphasis` field lets a decoder recover the
/// original spectral balance via [`DeEmphasis`]. Same clean-room
/// bilinear derivation, with the numerator / denominator time constants
/// swapped:
///
/// ```text
///            1 + s·τ1
///   H_pre(s) = --------      (τ1 = 50 µs, τ2 = 15 µs)
///            1 + s·τ2
/// ```
///
/// so `b0 = (1 + τ1·k)/(1 + τ2·k)`, `b1 = (1 − τ1·k)/(1 + τ2·k)`,
/// `a1 = (1 − τ2·k)/(1 + τ2·k)` with `k = 2·fs`. DC gain is again 1 and
/// the high-frequency asymptote is `τ1/τ2 = 3.33` (+10.458 dB boost).
///
/// The CCITT J.17 pre-emphasis ([`Self::ccitt_j17`]) is likewise the
/// exact section-by-section inverse of [`DeEmphasis::ccitt_j17`] —
/// well-defined because the fitted de-emphasis cascade is minimum-phase
/// (see [`crate::j17`]).
#[derive(Debug, Clone, Copy)]
pub struct PreEmphasis {
    sections: [Section; MAX_SECTIONS],
    state: [(f64, f64); MAX_SECTIONS],
    len: usize,
}

impl PreEmphasis {
    fn from_sections(sections: &[Section]) -> Self {
        let mut all = [Section {
            b0: 1.0,
            b1: 0.0,
            a1: 0.0,
        }; MAX_SECTIONS];
        all[..sections.len()].copy_from_slice(sections);
        PreEmphasis {
            sections: all,
            state: [(0.0, 0.0); MAX_SECTIONS],
            len: sections.len(),
        }
    }

    /// Build the 50/15 µs pre-emphasis filter for sample rate `fs`
    /// (Hz). State starts at rest.
    #[must_use]
    pub fn fifty_fifteen(fs: u32) -> Self {
        Self::from_sections(&[fifty_fifteen_deemphasis_section(fs).inverse()])
    }

    /// Build the CCITT J.17 pre-emphasis filter for sample rate `fs`
    /// (Hz): the section-by-section inverse of
    /// [`DeEmphasis::ccitt_j17`], so the pre → de cascade is identity.
    /// State starts at rest.
    #[must_use]
    pub fn ccitt_j17(fs: u32) -> Self {
        let de = crate::j17::deemphasis_sections(fs);
        let mut inv = de;
        for (dst, src) in inv.iter_mut().zip(&de) {
            *dst = src.inverse();
        }
        Self::from_sections(&inv)
    }

    /// Build the pre-emphasis filter a `header` calls for, or [`None`]
    /// when the PCM is encoded unaltered (`emphasis == '00'`).
    #[must_use]
    pub fn for_header(header: &FrameHeader) -> Option<Self> {
        match header.emphasis {
            Emphasis::FiftyFifteen => Some(Self::fifty_fifteen(header.sample_rate)),
            Emphasis::CcittJ17 => Some(Self::ccitt_j17(header.sample_rate)),
            Emphasis::None => None,
        }
    }

    /// Filter one sample, advancing state.
    #[must_use]
    #[inline]
    pub fn process_sample(&mut self, x: f64) -> f64 {
        cascade_step(&self.sections, &mut self.state, self.len, x)
    }

    /// Filter a whole channel's PCM block in place.
    pub fn process_in_place(&mut self, pcm: &mut [f64]) {
        for s in pcm.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// The direct-form-I coefficients `(b0, b1, a1)` of the **first**
    /// (for 50/15 µs: the only) section; `a0` is 1. See [`Self::sections`]
    /// for the whole cascade.
    #[must_use]
    pub fn coefficients(&self) -> (f64, f64, f64) {
        (
            self.sections[0].b0,
            self.sections[0].b1,
            self.sections[0].a1,
        )
    }

    /// The active first-order sections of the cascade, in processing
    /// order (state excluded).
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{Emphasis, Mode, ModeExtension};

    fn header_with(emphasis: Emphasis, sample_rate: u32) -> FrameHeader {
        FrameHeader {
            lsf: false,
            bit_rate: 192_000,
            sample_rate,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis,
            protection_bit: true,
        }
    }

    #[test]
    fn none_yields_no_filter_and_j17_yields_the_j17_cascade() {
        assert!(DeEmphasis::for_header(&header_with(Emphasis::None, 48_000)).is_none());
        assert!(PreEmphasis::for_header(&header_with(Emphasis::None, 48_000)).is_none());
        let j17 = DeEmphasis::for_header(&header_with(Emphasis::CcittJ17, 48_000)).unwrap();
        assert_eq!(j17.sections(), DeEmphasis::ccitt_j17(48_000).sections());
        assert_eq!(j17.sections().len(), crate::j17::J17_SECTIONS);
        let pre = PreEmphasis::for_header(&header_with(Emphasis::CcittJ17, 48_000)).unwrap();
        assert_eq!(pre.sections(), PreEmphasis::ccitt_j17(48_000).sections());
    }

    #[test]
    fn fifty_fifteen_selected_at_header_rate() {
        let f = DeEmphasis::for_header(&header_with(Emphasis::FiftyFifteen, 44_100)).unwrap();
        let (b0, b1, a1) = f.coefficients();
        let (rb0, rb1, ra1) = DeEmphasis::fifty_fifteen(44_100).coefficients();
        assert_eq!((b0, b1, a1), (rb0, rb1, ra1));
    }

    #[test]
    fn dc_gain_is_unity() {
        // A constant (DC) input must pass through unchanged once the
        // filter has warmed up, since H(z=1) == 1 by construction.
        for fs in [16_000, 22_050, 24_000, 32_000, 44_100, 48_000] {
            let mut f = DeEmphasis::fifty_fifteen(fs);
            let mut y = 0.0;
            for _ in 0..2000 {
                y = f.process_sample(0.5);
            }
            assert!((y - 0.5).abs() < 1e-9, "fs={fs} dc gain drifted: {y}");
        }
    }

    #[test]
    fn high_frequency_shelf_asymptote() {
        // A full-scale Nyquist alternation ±1 settles to the shelf
        // asymptote τ2/τ1 = 0.3 in magnitude (−10.458 dB).
        let mut f = DeEmphasis::fifty_fifteen(48_000);
        let mut last = 0.0;
        for n in 0..4000 {
            let x = if n % 2 == 0 { 1.0 } else { -1.0 };
            last = f.process_sample(x);
        }
        // The steady-state |output| at Nyquist equals |H(-1)| = τ2/τ1.
        assert!(
            (last.abs() - 0.3).abs() < 1e-3,
            "nyquist shelf magnitude {} != 0.3",
            last.abs()
        );
    }

    #[test]
    fn coefficients_match_bilinear_derivation() {
        // Spot-check one rate against the closed-form derivation.
        let f = DeEmphasis::fifty_fifteen(32_000);
        let (b0, b1, a1) = f.coefficients();
        let k = 2.0 * 32_000.0;
        let denom = 1.0 + TAU1_50US * k;
        assert!((b0 - (1.0 + TAU2_15US * k) / denom).abs() < 1e-12);
        assert!((b1 - (1.0 - TAU2_15US * k) / denom).abs() < 1e-12);
        assert!((a1 - (1.0 - TAU1_50US * k) / denom).abs() < 1e-12);
    }

    #[test]
    fn zero_input_stays_zero() {
        let mut f = DeEmphasis::fifty_fifteen(44_100);
        for _ in 0..100 {
            assert_eq!(f.process_sample(0.0), 0.0);
        }
    }

    #[test]
    fn pre_then_de_is_identity() {
        // Pre-emphasis followed by de-emphasis reconstructs the input to
        // machine precision (they are exact inverses).
        for fs in [16_000, 22_050, 24_000, 32_000, 44_100, 48_000] {
            let mut pre = PreEmphasis::fifty_fifteen(fs);
            let mut de = DeEmphasis::fifty_fifteen(fs);
            // deterministic pseudo-random-ish signal
            let mut acc = 0.123_456_f64;
            for n in 0..3000 {
                acc = (acc * 1.1 + 0.017).fract();
                let x = 2.0 * acc - 1.0;
                let recon = de.process_sample(pre.process_sample(x));
                if n > 50 {
                    assert!(
                        (recon - x).abs() < 1e-9,
                        "fs={fs} n={n}: cascade not identity ({recon} != {x})"
                    );
                }
            }
        }
    }

    #[test]
    fn pre_emphasis_dc_gain_is_unity() {
        let mut f = PreEmphasis::fifty_fifteen(48_000);
        let mut y = 0.0;
        for _ in 0..2000 {
            y = f.process_sample(0.25);
        }
        assert!((y - 0.25).abs() < 1e-9, "pre-emphasis DC gain drifted: {y}");
    }

    #[test]
    fn j17_dc_gain_is_unity() {
        // The J.17 de-emphasis (and its inverse) are normalised to
        // unity DC gain, like the 50/15 µs pair — see `crate::j17`.
        for fs in [16_000, 22_050, 24_000, 32_000, 44_100, 48_000] {
            let mut de = DeEmphasis::ccitt_j17(fs);
            let mut pre = PreEmphasis::ccitt_j17(fs);
            let (mut yd, mut yp) = (0.0, 0.0);
            for _ in 0..8000 {
                yd = de.process_sample(0.5);
                yp = pre.process_sample(0.5);
            }
            assert!((yd - 0.5).abs() < 1e-6, "fs={fs}: de-emphasis DC {yd}");
            assert!((yp - 0.5).abs() < 1e-6, "fs={fs}: pre-emphasis DC {yp}");
        }
    }

    #[test]
    fn j17_nyquist_shelf_matches_the_analytic_curve() {
        // A full-scale Nyquist alternation settles to the analytic
        // shelf value |H(f = fs/2)| (≈ −18.6 dB at 48 kHz — close to
        // the 10·log10(75) = 18.75 dB asymptote).
        let fs = 48_000;
        let mut f = DeEmphasis::ccitt_j17(fs);
        let mut last = 0.0;
        for n in 0..8000 {
            let x = if n % 2 == 0 { 1.0 } else { -1.0 };
            last = f.process_sample(x);
        }
        let expected = crate::j17::deemphasis_power_gain(f64::from(fs) / 2.0).sqrt();
        assert!(
            (last.abs() - expected).abs() < 1e-3,
            "nyquist magnitude {} != analytic {expected}",
            last.abs()
        );
    }

    #[test]
    fn j17_pre_then_de_is_identity() {
        // The J.17 pre-emphasis is the exact section-by-section inverse
        // of the de-emphasis, so the cascade reconstructs the input.
        for fs in [16_000, 22_050, 24_000, 32_000, 44_100, 48_000] {
            let mut pre = PreEmphasis::ccitt_j17(fs);
            let mut de = DeEmphasis::ccitt_j17(fs);
            let mut acc = 0.123_456_f64;
            for n in 0..3000 {
                acc = (acc * 1.1 + 0.017).fract();
                let x = 2.0 * acc - 1.0;
                let recon = de.process_sample(pre.process_sample(x));
                if n > 50 {
                    assert!(
                        (recon - x).abs() < 1e-9,
                        "fs={fs} n={n}: J.17 cascade not identity ({recon} != {x})"
                    );
                }
            }
        }
    }

    #[test]
    fn section_inverse_round_trips() {
        let s = fifty_fifteen_deemphasis_section(44_100);
        let inv = s.inverse();
        let back = inv.inverse();
        assert!((back.b0 - s.b0).abs() < 1e-12);
        assert!((back.b1 - s.b1).abs() < 1e-12);
        assert!((back.a1 - s.a1).abs() < 1e-12);
    }
}
