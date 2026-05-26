//! Layer I / Layer II §2.4.3.2 / §2.4.3.3.5 polyphase synthesis
//! subband filter.
//!
//! Clean-room: every step here is transcribed directly from ISO/IEC
//! 11172-3 (1993) Annex A Figure A.2 ("Synthesis subband filter flow
//! chart", PDF page 39 / full-PDF page 45) and §2.4.3.3.5 (PDF page 32
//! / full-PDF page 38). No third-party MP2 source was consulted.
//!
//! # §2.4.3.3.5 algorithm
//!
//! Each call consumes one 32-vector `S` of decoded subband samples and
//! produces 32 reconstructed PCM samples `out`, while the 1024-entry V
//! ring buffer evolves per Figure A.2:
//!
//! 1. **Shift V.** `V[i] = V[i - 64]` for `i = 1023..=64`.
//! 2. **Matrix V.** `V[i] = sum_{k = 0..32} N_ik * S[k]` for
//!    `i = 0..64`, with N_ik per §2.4.3.3.5:
//!
//!    ```text
//!    N_ik = cos[(16 + i)(2k + 1) * pi / 64]    0 <= i <= 63, 0 <= k <= 31
//!    ```
//!
//! 3. **Build U.** A 512-entry vector picked from V:
//!
//!    ```text
//!    for i = 0..=7
//!        for j = 0..=31
//!            U[i * 64 + j]      = V[i * 128 + j]
//!            U[i * 64 + j + 32] = V[i * 128 + 96 + j]
//!    ```
//!
//! 4. **Window U.** `W[i] = U[i] * D[i]` with D[i] from
//!    [`crate::tables_synthesis::D`].
//! 5. **Sum W.** `S_j = sum_{i = 0..16} W[j + 32 * i]` for `j = 0..32`.
//!
//! Step 1 is "see footnote 1: V to be initialized with zeroes during
//! startup" (PDF page 39).
//!
//! # Numerical scale
//!
//! The PDF prescribes output PCM samples in `[-1.0, +1.0]` (see
//! §2.4.3.4.7.1, PDF page 35); the §2.4.3.3.4 requantizer in
//! [`crate::requant`] already produces fractional values in that range,
//! so the filterbank output is consumed directly (callers convert to
//! `i16` / `i32` as needed).
//!
//! # No reference-implementation contact
//!
//! The N_ik matrix coefficients are *not* tabulated in Annex B; the
//! spec defines them only by the cosine closed form above. We compute
//! them at startup (filling a 64x32 = 2048-entry `[f64]`) and reuse the
//! table across all invocations of one filterbank instance — this is
//! O(2048 multiplies per frame at filterbank construction, then the
//! per-frame cost is 32 * 64 = 2048 mul-adds for matrixing + 512
//! mul-adds for windowing + 32 * 16 = 512 adds for summing).

use crate::tables_synthesis::{D, D_LEN};

/// Number of subbands per Layer I/II frame slot (§2.4.2.5 / Figure A.2).
pub const NUM_SUBBANDS: usize = 32;

/// V ring-buffer depth per Figure A.2 ("Shifting: for i = 1023 down to
/// 64 do V[i] = V[i - 64]"). 16 shifts of 64 = 1024 retained samples.
pub const V_BUF_LEN: usize = 1024;

/// One polyphase synthesis filterbank instance — a single decoder
/// channel's V ring buffer + the precomputed N_ik matrix.
///
/// The constructor seeds V with zeros per Annex A Figure A.2's footnote
/// 1 ("V to be initialized with zeroes during startup").
#[derive(Debug, Clone)]
pub struct SynthesisFilterbank {
    /// 1024-entry V ring buffer (Figure A.2 "Shifting" + "Matrixing"
    /// steps).
    v: Box<[f64; V_BUF_LEN]>,
    /// Precomputed N_ik matrix laid out row-major as
    /// `n[i * NUM_SUBBANDS + k] = cos[(16 + i)(2k + 1) * pi / 64]`.
    n: Box<[f64; 64 * NUM_SUBBANDS]>,
}

impl Default for SynthesisFilterbank {
    fn default() -> Self {
        Self::new()
    }
}

impl SynthesisFilterbank {
    /// Build a fresh filterbank with V zeroed and the N_ik matrix
    /// precomputed.
    pub fn new() -> Self {
        let mut n = Box::new([0.0_f64; 64 * NUM_SUBBANDS]);
        for i in 0..64 {
            for k in 0..NUM_SUBBANDS {
                // §2.4.3.3.5: N_ik = cos[(16 + i)(2k + 1) * pi / 64]
                let arg =
                    (16.0_f64 + i as f64) * (2.0 * k as f64 + 1.0) * core::f64::consts::PI / 64.0;
                n[i * NUM_SUBBANDS + k] = arg.cos();
            }
        }
        SynthesisFilterbank {
            v: Box::new([0.0_f64; V_BUF_LEN]),
            n,
        }
    }

    /// Reset the V ring buffer to all zeros (cold restart per Figure
    /// A.2 footnote 1).
    pub fn reset(&mut self) {
        for slot in self.v.iter_mut() {
            *slot = 0.0;
        }
    }

    /// Borrow the precomputed N_ik matrix for diagnostics / test
    /// cross-checks. Layout is row-major: `n_matrix()[i * 32 + k] =
    /// cos[(16 + i)(2k + 1) * pi / 64]`.
    pub fn n_matrix(&self) -> &[f64] {
        &self.n[..]
    }

    /// Process one 32-vector `s` of subband samples and write the
    /// resulting 32 reconstructed PCM samples into `out`.
    ///
    /// `out[j]` corresponds to time index `j` within the slot (so
    /// `out[0]` is the earliest sample of the 32-sample slot, per the
    /// definition `S_j = sum_{i = 0..16} W[j + 32 * i]`).
    ///
    /// PCM samples are returned in the spec's `[-1.0, +1.0]` nominal
    /// range (see §2.4.3.4.7.1); clipping is left to the caller as it
    /// is conversion-dependent.
    pub fn push_subbands(&mut self, s: &[f64; NUM_SUBBANDS], out: &mut [f64; NUM_SUBBANDS]) {
        // Step 1: shift V by 64 ("for i = 1023 down to 64 do V[i] =
        // V[i - 64]"). After this, V[0..64] holds stale values that
        // step 2 overwrites.
        for i in (64..V_BUF_LEN).rev() {
            self.v[i] = self.v[i - 64];
        }

        // Step 2: matrix into V[0..64]. V_i = sum_{k=0..31} N_ik * S_k.
        for i in 0..64 {
            let mut acc = 0.0_f64;
            let row = &self.n[i * NUM_SUBBANDS..(i + 1) * NUM_SUBBANDS];
            for k in 0..NUM_SUBBANDS {
                acc += row[k] * s[k];
            }
            self.v[i] = acc;
        }

        // Step 3: build U from V. 512 entries.
        let mut u = [0.0_f64; D_LEN];
        for i in 0..8 {
            for j in 0..32 {
                u[i * 64 + j] = self.v[i * 128 + j];
                u[i * 64 + j + 32] = self.v[i * 128 + 96 + j];
            }
        }

        // Step 4 + 5: window by D and sum 16 strided taps per output
        // sample. Fused into one loop to avoid a second 512-entry
        // intermediate.
        for (j, slot) in out.iter_mut().enumerate() {
            let mut acc = 0.0_f64;
            for i in 0..16 {
                let idx = j + 32 * i;
                acc += u[idx] * D[idx];
            }
            *slot = acc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: instantiate a filterbank and feed it `count` zero
    /// 32-vectors, returning the last PCM block.
    fn zero_input_filterbank(count: usize) -> [f64; NUM_SUBBANDS] {
        let mut fb = SynthesisFilterbank::new();
        let zero = [0.0_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        for _ in 0..count {
            fb.push_subbands(&zero, &mut out);
        }
        out
    }

    #[test]
    fn fresh_filterbank_starts_with_zeroed_v() {
        let fb = SynthesisFilterbank::new();
        assert!(fb.v.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn n_ik_matrix_matches_closed_form() {
        // Cross-check the cached matrix against the §2.4.3.3.5 closed
        // form, both at random points and at the algebraic extremes.
        let fb = SynthesisFilterbank::new();
        let n = fb.n_matrix();
        for i in 0..64 {
            for k in 0..NUM_SUBBANDS {
                let want = ((16.0_f64 + i as f64) * (2.0 * k as f64 + 1.0) * core::f64::consts::PI
                    / 64.0)
                    .cos();
                let got = n[i * NUM_SUBBANDS + k];
                assert!(
                    (got - want).abs() < 1e-15,
                    "N[{i},{k}]: got {got}, want {want}"
                );
            }
        }
    }

    #[test]
    fn n_ik_matrix_size_is_64_by_32() {
        let fb = SynthesisFilterbank::new();
        assert_eq!(fb.n_matrix().len(), 64 * NUM_SUBBANDS);
    }

    #[test]
    fn n_ik_values_are_bounded_by_one() {
        // |cos x| <= 1, so every entry is in [-1, 1].
        let fb = SynthesisFilterbank::new();
        for &v in fb.n_matrix() {
            assert!(v.abs() <= 1.0, "N entry {v} exceeded ±1");
        }
    }

    #[test]
    fn zero_input_produces_zero_output() {
        // With V zeroed and S = 0, every step is zero -> output is
        // zero. Test across enough invocations to fully cycle V (16
        // shifts of 64 = the full 1024-deep buffer).
        for n_frames in [1usize, 16, 32, 100] {
            let out = zero_input_filterbank(n_frames);
            for (j, &v) in out.iter().enumerate() {
                assert!(v == 0.0, "frame {n_frames} out[{j}] = {v}, expected 0");
            }
        }
    }

    #[test]
    fn reset_zeroes_v_buffer() {
        // After a non-zero pulse, reset() must wipe V so a subsequent
        // zero input produces exactly zero output.
        let mut fb = SynthesisFilterbank::new();
        let mut pulse = [0.0_f64; NUM_SUBBANDS];
        pulse[0] = 0.5;
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_subbands(&pulse, &mut out);
        fb.reset();
        assert!(fb.v.iter().all(|&v| v == 0.0));
        let zero = [0.0_f64; NUM_SUBBANDS];
        fb.push_subbands(&zero, &mut out);
        for (j, &v) in out.iter().enumerate() {
            assert!(v == 0.0, "out[{j}] = {v} after reset");
        }
    }

    #[test]
    fn output_is_finite_and_bounded_on_a_single_subband_excitation() {
        // The PCM nominal range of `[-1, +1]` (§2.4.3.4.7.1) applies to
        // a *physically realisable* combination of subband samples, not
        // to a worst-case all-subbands-at-the-max input — the latter
        // exceeds the nominal range by design (the matrix `N_ik` sums
        // 32 cosines, each in `[-1, +1]`). What we pin here is that a
        // single subband excited at the requantizer's bound produces a
        // finite, bounded output across a full V-buffer cycle.
        let mut fb = SynthesisFilterbank::new();
        let mut s = [0.0_f64; NUM_SUBBANDS];
        s[3] = 0.95; // §2.4.3.3.4: |s''| < 1 for the requantizer.
        let mut out = [0.0_f64; NUM_SUBBANDS];
        let mut max_mag = 0.0_f64;
        for _ in 0..64 {
            fb.push_subbands(&s, &mut out);
            for &v in &out {
                assert!(v.is_finite(), "non-finite PCM sample {v}");
                max_mag = max_mag.max(v.abs());
            }
        }
        // A single subband excited at 0.95 cannot exceed the nominal
        // PCM range by more than the cumulative weight of the §2.4.3.3.5
        // matrix row plus the windowing sum, which is well under 5.
        assert!(
            max_mag < 5.0,
            "single-subband excitation produced large output ({max_mag})"
        );
    }

    #[test]
    fn impulse_in_subband_propagates_then_decays() {
        // Feed a single impulse in subband 0 of frame 0; subsequent
        // frames feed zeros. The output should be non-zero for the
        // first few frames (the impulse circulates through the V
        // buffer's 16-deep delay line) and then decay toward zero as
        // the impulse exits the windowed support.
        let mut fb = SynthesisFilterbank::new();
        let mut pulse = [0.0_f64; NUM_SUBBANDS];
        pulse[0] = 1.0;
        let zero = [0.0_f64; NUM_SUBBANDS];

        let mut energies = Vec::new();
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_subbands(&pulse, &mut out);
        energies.push(out.iter().map(|v| v * v).sum::<f64>());
        for _ in 0..30 {
            fb.push_subbands(&zero, &mut out);
            energies.push(out.iter().map(|v| v * v).sum::<f64>());
        }

        // At least one frame in the first 16 has non-zero energy
        // (the impulse is windowed across the 16-frame V depth).
        let max_e = energies[..16].iter().cloned().fold(0.0_f64, f64::max);
        assert!(max_e > 0.0, "impulse never reached output");

        // After 17+ frames the impulse has been shifted past the
        // V[..1024] window so further frames must be exactly zero.
        for (n, &e) in energies.iter().enumerate().skip(17) {
            assert!(
                e == 0.0,
                "frame {n} has residual energy {e} after impulse should have exited"
            );
        }
    }

    #[test]
    fn output_index_count_matches_subbands() {
        // Sanity: the API contract is "32 in, 32 out, in time order".
        let mut fb = SynthesisFilterbank::new();
        let s = [1.0_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_subbands(&s, &mut out);
        assert_eq!(out.len(), NUM_SUBBANDS);
    }

    #[test]
    fn two_independent_channels_do_not_share_state() {
        // The crate-level contract is one instance per channel — verify
        // that two instances given different inputs produce different
        // outputs after the same number of frames.
        let mut left = SynthesisFilterbank::new();
        let mut right = SynthesisFilterbank::new();
        let l_in = [0.5_f64; NUM_SUBBANDS];
        let mut r_in = [0.0_f64; NUM_SUBBANDS];
        r_in[5] = 0.8;
        let mut l_out = [0.0_f64; NUM_SUBBANDS];
        let mut r_out = [0.0_f64; NUM_SUBBANDS];
        for _ in 0..4 {
            left.push_subbands(&l_in, &mut l_out);
            right.push_subbands(&r_in, &mut r_out);
        }
        let any_diff = l_out.iter().zip(r_out.iter()).any(|(a, b)| a != b);
        assert!(any_diff, "independent channels produced identical output");
    }

    #[test]
    fn n_ik_landmark_values_match_closed_form_by_hand() {
        // N[i, k] = cos[(16 + i) * (2k + 1) * pi / 64].
        // i = 0, k = 0 -> arg = 16 * pi / 64 = pi/4 -> N = cos(pi/4) =
        //     sqrt(2)/2 ≈ 0.7071067811865476.
        // i = 0, k = 4 -> arg = 16 * 9 * pi / 64 = 9 * pi / 4
        //     -> cos(9 pi / 4) = cos(pi/4) = sqrt(2)/2.
        // i = 16, k = 0 -> arg = 32 * pi / 64 = pi/2 -> N = 0.
        let fb = SynthesisFilterbank::new();
        let sqrt2_over_2 = 2.0_f64.sqrt() / 2.0;
        assert!(
            (fb.n_matrix()[0] - sqrt2_over_2).abs() < 1e-15,
            "N[0,0] should be sqrt(2)/2, got {}",
            fb.n_matrix()[0]
        );
        assert!(
            (fb.n_matrix()[4] - sqrt2_over_2).abs() < 1e-15,
            "N[0,4] should be sqrt(2)/2, got {}",
            fb.n_matrix()[4]
        );
        let n_16_0 = fb.n_matrix()[16 * NUM_SUBBANDS];
        assert!(
            n_16_0.abs() < 1e-15,
            "N[16,0] = cos(pi/2) should be ~0, got {n_16_0}"
        );
    }
}
