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
//! The J.17 curve's exact response is defined by CCITT Recommendation
//! J.17, which is **not** part of the staged ISO/IEC 11172-3 /
//! 13818-3 material, so its filter coefficients cannot be derived
//! clean-room here. A J.17-flagged stream is therefore delivered
//! unfiltered (the historical behaviour of this decoder for *all*
//! emphasis modes) and [`DeEmphasis::for_header`] returns [`None`] for
//! it; see the crate README "Not yet supported" note. Enabling J.17
//! de-emphasis requires staging Recommendation J.17.

use crate::header::{Emphasis, FrameHeader};

/// First-order §2.4.2.4 de-emphasis IIR, one instance per output
/// channel. State (`x[n−1]`, `y[n−1]`) persists across frames so the
/// filter has no per-frame discontinuity — a single logical stream
/// shares one filter per channel, re-created only on a decoder reset.
#[derive(Debug, Clone, Copy)]
pub struct DeEmphasis {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

/// 50/15 µs first time constant (the `τ1 = 50 µs` de-emphasis pole).
pub const TAU1_50US: f64 = 50e-6;
/// 50/15 µs second time constant (the `τ2 = 15 µs` zero).
pub const TAU2_15US: f64 = 15e-6;

impl DeEmphasis {
    /// Build the 50/15 µs de-emphasis filter for output sample rate
    /// `fs` (Hz), deriving the coefficients from the time constants via
    /// the bilinear transform (see the module docs). State starts at
    /// rest (`x[−1] = y[−1] = 0`).
    #[must_use]
    pub fn fifty_fifteen(fs: u32) -> Self {
        let k = 2.0 * f64::from(fs);
        let t1k = TAU1_50US * k;
        let t2k = TAU2_15US * k;
        let denom = 1.0 + t1k;
        DeEmphasis {
            b0: (1.0 + t2k) / denom,
            b1: (1.0 - t2k) / denom,
            a1: (1.0 - t1k) / denom,
            x1: 0.0,
            y1: 0.0,
        }
    }

    /// Build the de-emphasis filter a `header` calls for, or [`None`]
    /// when no clean-room-implementable de-emphasis applies:
    ///
    /// * [`Emphasis::None`] → `None` (deliver PCM unfiltered);
    /// * [`Emphasis::FiftyFifteen`] → the 50/15 µs filter at
    ///   `header.sample_rate`;
    /// * [`Emphasis::CcittJ17`] → `None` (docs gap — Recommendation
    ///   J.17 is not staged; see module docs).
    #[must_use]
    pub fn for_header(header: &FrameHeader) -> Option<Self> {
        match header.emphasis {
            Emphasis::FiftyFifteen => Some(Self::fifty_fifteen(header.sample_rate)),
            Emphasis::None | Emphasis::CcittJ17 => None,
        }
    }

    /// Filter one sample, advancing the `x[n−1]` / `y[n−1]` state.
    #[must_use]
    #[inline]
    pub fn process_sample(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    /// Filter a whole channel's PCM block in place.
    pub fn process_in_place(&mut self, pcm: &mut [f64]) {
        for s in pcm.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// The direct-form-I coefficients `(b0, b1, a1)` (a0 is 1).
    #[must_use]
    pub fn coefficients(&self) -> (f64, f64, f64) {
        (self.b0, self.b1, self.a1)
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
#[derive(Debug, Clone, Copy)]
pub struct PreEmphasis {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl PreEmphasis {
    /// Build the 50/15 µs pre-emphasis filter for sample rate `fs`
    /// (Hz). State starts at rest.
    #[must_use]
    pub fn fifty_fifteen(fs: u32) -> Self {
        let k = 2.0 * f64::from(fs);
        let t1k = TAU1_50US * k;
        let t2k = TAU2_15US * k;
        let denom = 1.0 + t2k;
        PreEmphasis {
            b0: (1.0 + t1k) / denom,
            b1: (1.0 - t1k) / denom,
            a1: (1.0 - t2k) / denom,
            x1: 0.0,
            y1: 0.0,
        }
    }

    /// Filter one sample, advancing state.
    #[must_use]
    #[inline]
    pub fn process_sample(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    /// Filter a whole channel's PCM block in place.
    pub fn process_in_place(&mut self, pcm: &mut [f64]) {
        for s in pcm.iter_mut() {
            *s = self.process_sample(*s);
        }
    }

    /// The direct-form-I coefficients `(b0, b1, a1)` (a0 is 1).
    #[must_use]
    pub fn coefficients(&self) -> (f64, f64, f64) {
        (self.b0, self.b1, self.a1)
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
    fn none_and_j17_yield_no_filter() {
        assert!(DeEmphasis::for_header(&header_with(Emphasis::None, 48_000)).is_none());
        assert!(DeEmphasis::for_header(&header_with(Emphasis::CcittJ17, 48_000)).is_none());
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
}
