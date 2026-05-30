//! §C.1.3 Annex C polyphase analysis subband filterbank (encoder side).
//!
//! Clean-room: every step here is transcribed directly from ISO/IEC
//! 11172-3 (1993) §C.1.3 ("Analysis subband filter", PDF pages 67 and
//! 76-77 of the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`). No third-party MP2
//! source was consulted.
//!
//! # §C.1.3 algorithm (Figure C.4 "Analysis Subband Filter Flow Chart")
//!
//! Each call consumes 32 PCM input samples and produces 32 subband
//! samples `S_i`. The 512-entry X ring buffer evolves per the flow
//! chart:
//!
//! 1. **Shift X.** `X[i] = X[i - 32]` for `i = 511..=32` ("the 32
//!    oldest elements are shifted out").
//! 2. **Insert 32 audio samples.** The most recent one at position 0,
//!    the others at positions 1..32 in time-reverse order (per the
//!    §C.1.3 prose "shifted in at positions 0 to 31, the most recent
//!    one at position 0").
//! 3. **Window.** `Z[i] = X[i] * C[i]` for `i = 0..512`, with `C[i]`
//!    from [`crate::tables_analysis::C`].
//! 4. **Compact to 64 values.**
//!
//!    ```text
//!    Y_i = sum_{j = 0..8} Z[i + 64 * j]   for i = 0..64
//!    ```
//!
//! 5. **Matrix to 32 subband samples.**
//!
//!    ```text
//!    S_i = sum_{k = 0..64} M_ik * Y_k     for i = 0..32
//!    ```
//!
//!    with `M_ik` per §C.1.3:
//!
//!    ```text
//!    M_ik = cos[(2i + 1)(k - 16) * pi / 64]   0 <= i <= 31, 0 <= k <= 63
//!    ```
//!
//! Step 1 is "see footnote 1: X to be initialised with zeroes during
//! startup" — same cold-start convention as the §2.4.3.3.5 synthesis
//! filterbank's V buffer.
//!
//! # Symmetry with the §2.4.3.3.5 synthesis filterbank
//!
//! The encoder's §C.1.3 analysis filterbank is the time-reversed dual
//! of the decoder's §2.4.3.3.5 synthesis filterbank, with windows
//! related by `D[i] = 32 * C[i]` (cross-checked in
//! [`crate::tables_analysis`]). The matrix coefficients are related by
//! `M_ik = cos[(2i + 1)(k - 16) * pi / 64]` (analysis) versus
//! `N_ik = cos[(16 + i)(2k + 1) * pi / 64]` (synthesis); the two are
//! the §C.1.3 / §2.4.3.3.5 closed forms verbatim.
//!
//! # Numerical scale
//!
//! Per Figure C.4 the subband samples are produced at the same `[-1,
//! +1]` nominal range as the PCM input — the C[] window's small
//! magnitudes (peak `0.035780907` at i=256, secondary peak
//! `0.000108719` at i=69, i=70) combined with the matrix's
//! cosine-bounded entries keep the output bounded against unit-norm
//! input.

use crate::tables_analysis::{C, C_LEN};

/// Number of subbands per Layer I/II frame slot (§2.4.2.5 / Figure C.4).
pub const NUM_SUBBANDS: usize = 32;

/// X ring-buffer depth per Figure C.4 ("Build an input sample vector X
/// of 512 elements"). 16 shifts of 32 = 512 retained samples.
pub const X_BUF_LEN: usize = C_LEN;

/// One §C.1.3 analysis filterbank instance — a single encoder channel's
/// X ring buffer + the precomputed M_ik matrix.
///
/// The constructor seeds X with zeros, matching the §2.4.3.3.5
/// synthesis-side cold-start convention.
#[derive(Debug, Clone)]
pub struct AnalysisFilterbank {
    /// 512-entry X ring buffer (Figure C.4 "Build input vector" +
    /// "Window" steps).
    x: Box<[f64; X_BUF_LEN]>,
    /// Precomputed M_ik matrix laid out row-major as
    /// `m[i * 64 + k] = cos[(2i + 1)(k - 16) * pi / 64]`.
    m: Box<[f64; NUM_SUBBANDS * 64]>,
}

impl Default for AnalysisFilterbank {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisFilterbank {
    /// Build a fresh filterbank with X zeroed and the M_ik matrix
    /// precomputed.
    pub fn new() -> Self {
        let mut m = Box::new([0.0_f64; NUM_SUBBANDS * 64]);
        for i in 0..NUM_SUBBANDS {
            for k in 0..64 {
                // §C.1.3: M_ik = cos[(2i + 1)(k - 16) * pi / 64]
                let arg = (2.0 * i as f64 + 1.0) * (k as f64 - 16.0) * core::f64::consts::PI / 64.0;
                m[i * 64 + k] = arg.cos();
            }
        }
        AnalysisFilterbank {
            x: Box::new([0.0_f64; X_BUF_LEN]),
            m,
        }
    }

    /// Reset the X ring buffer to all zeros (cold restart per the
    /// Figure C.4 startup convention).
    pub fn reset(&mut self) {
        for slot in self.x.iter_mut() {
            *slot = 0.0;
        }
    }

    /// Borrow the precomputed M_ik matrix for diagnostics / test
    /// cross-checks. Layout is row-major: `m_matrix()[i * 64 + k] =
    /// cos[(2i + 1)(k - 16) * pi / 64]`.
    pub fn m_matrix(&self) -> &[f64] {
        &self.m[..]
    }

    /// Consume one 32-vector `audio` of input PCM samples and write 32
    /// analysis-side subband samples into `out`.
    ///
    /// `audio[0]` is the earliest sample in time, `audio[31]` the most
    /// recent — matching the §2.4.3.3.5 synthesis convention so the
    /// analysis-then-synthesis chain composes naturally on time-ordered
    /// PCM. Internally the §C.1.3 "most recent one at position 0" rule
    /// is honoured by inserting `audio[31]` at `X[0]`, `audio[30]` at
    /// `X[1]`, …, `audio[0]` at `X[31]` after the 32-slot shift.
    ///
    /// `out[i]` is the §C.1.3 subband-`i` sample for `i = 0..32`.
    pub fn push_audio(&mut self, audio: &[f64; NUM_SUBBANDS], out: &mut [f64; NUM_SUBBANDS]) {
        // Step 1: shift X by 32 ("the 32 oldest elements are shifted
        // out"). After this, X[0..32] holds stale values that step 2
        // overwrites.
        for i in (NUM_SUBBANDS..X_BUF_LEN).rev() {
            self.x[i] = self.x[i - NUM_SUBBANDS];
        }

        // Step 2: insert the 32 new audio samples. §C.1.3: "the 32
        // audio samples are shifted in at positions 0 to 31, the most
        // recent one at position 0". With `audio[31]` being the most
        // recent, X[p] receives audio[31 - p] for p = 0..32.
        for p in 0..NUM_SUBBANDS {
            self.x[p] = audio[NUM_SUBBANDS - 1 - p];
        }

        // Step 3+4 fused: window X by C and compact into the 64-entry
        // Y vector in one pass.
        // Y_i = sum_{j = 0..8} (X[i + 64*j] * C[i + 64*j])
        let mut y = [0.0_f64; 64];
        for (i, y_slot) in y.iter_mut().enumerate() {
            let mut acc = 0.0_f64;
            for j in 0..8 {
                let idx = i + 64 * j;
                acc += self.x[idx] * C[idx];
            }
            *y_slot = acc;
        }

        // Step 5: matrix Y into the 32 subband samples.
        // S_i = sum_{k = 0..64} M_ik * Y_k.
        for (i, slot) in out.iter_mut().enumerate() {
            let mut acc = 0.0_f64;
            let row = &self.m[i * 64..(i + 1) * 64];
            for k in 0..64 {
                acc += row[k] * y[k];
            }
            *slot = acc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed `count` zero 32-vectors and return the last subband
    /// output.
    fn zero_input_filterbank(count: usize) -> [f64; NUM_SUBBANDS] {
        let mut fb = AnalysisFilterbank::new();
        let zero = [0.0_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        for _ in 0..count {
            fb.push_audio(&zero, &mut out);
        }
        out
    }

    #[test]
    fn fresh_filterbank_starts_with_zeroed_x() {
        let fb = AnalysisFilterbank::new();
        assert!(fb.x.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn m_ik_matrix_matches_closed_form() {
        let fb = AnalysisFilterbank::new();
        let m = fb.m_matrix();
        for i in 0..NUM_SUBBANDS {
            for k in 0..64 {
                let want = ((2.0 * i as f64 + 1.0) * (k as f64 - 16.0) * core::f64::consts::PI
                    / 64.0)
                    .cos();
                let got = m[i * 64 + k];
                assert!(
                    (got - want).abs() < 1e-15,
                    "M[{i},{k}]: got {got}, want {want}"
                );
            }
        }
    }

    #[test]
    fn m_ik_matrix_size_is_32_by_64() {
        let fb = AnalysisFilterbank::new();
        assert_eq!(fb.m_matrix().len(), NUM_SUBBANDS * 64);
    }

    #[test]
    fn m_ik_values_are_bounded_by_one() {
        // |cos x| <= 1, so every entry is in [-1, 1].
        let fb = AnalysisFilterbank::new();
        for &v in fb.m_matrix() {
            assert!(v.abs() <= 1.0, "M entry {v} exceeded ±1");
        }
    }

    #[test]
    fn m_ik_landmark_values_match_closed_form_by_hand() {
        // M[i, k] = cos[(2i + 1)(k - 16) * pi / 64].
        //   i = 0,  k = 16 -> arg = 1 * 0 * pi / 64 = 0 -> M = 1.
        //   i = 0,  k = 0  -> arg = 1 * -16 * pi / 64 = -pi/4 -> M =
        //     cos(-pi/4) = sqrt(2)/2 ≈ 0.7071067811865476.
        //   i = 8,  k = 16 -> arg = 17 * 0 * pi / 64 = 0 -> M = 1.
        //   i = 31, k = 16 -> arg = 63 * 0 * pi / 64 = 0 -> M = 1.
        let fb = AnalysisFilterbank::new();
        let m = fb.m_matrix();
        let sqrt2_over_2 = 2.0_f64.sqrt() / 2.0;
        assert!((m[16] - 1.0).abs() < 1e-15, "M[0,16] = 1, got {}", m[16]);
        assert!(
            (m[0] - sqrt2_over_2).abs() < 1e-15,
            "M[0,0] = sqrt(2)/2, got {}",
            m[0]
        );
        assert!(
            (m[8 * 64 + 16] - 1.0).abs() < 1e-15,
            "M[8,16] = 1, got {}",
            m[8 * 64 + 16]
        );
        assert!(
            (m[31 * 64 + 16] - 1.0).abs() < 1e-15,
            "M[31,16] = 1, got {}",
            m[31 * 64 + 16]
        );
    }

    #[test]
    fn zero_input_produces_zero_output() {
        // With X zeroed and audio = 0, every step is zero -> output is
        // zero. Test across enough invocations to fully cycle X (16
        // shifts of 32 = the full 512-deep buffer).
        for n_frames in [1usize, 16, 32, 100] {
            let out = zero_input_filterbank(n_frames);
            for (j, &v) in out.iter().enumerate() {
                assert!(v == 0.0, "frame {n_frames} out[{j}] = {v}, expected 0");
            }
        }
    }

    #[test]
    fn reset_zeroes_x_buffer() {
        // After a non-zero input, reset() must wipe X so a subsequent
        // zero input produces exactly zero output.
        let mut fb = AnalysisFilterbank::new();
        let mut pulse = [0.0_f64; NUM_SUBBANDS];
        pulse[0] = 0.5;
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&pulse, &mut out);
        fb.reset();
        assert!(fb.x.iter().all(|&v| v == 0.0));
        let zero = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&zero, &mut out);
        for (j, &v) in out.iter().enumerate() {
            assert!(v == 0.0, "out[{j}] = {v} after reset");
        }
    }

    #[test]
    fn most_recent_sample_lands_at_x0() {
        // §C.1.3 prose: "the 32 audio samples are shifted in at
        // positions 0 to 31, the most recent one at position 0". With
        // our caller-side `audio[31]` as the most recent sample,
        // `X[0]` must equal `audio[31]` after the push.
        let mut fb = AnalysisFilterbank::new();
        let mut audio = [0.0_f64; NUM_SUBBANDS];
        for (i, slot) in audio.iter_mut().enumerate() {
            *slot = (i + 1) as f64; // 1, 2, 3, ..., 32
        }
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&audio, &mut out);

        // Most recent input is audio[31] = 32.0; oldest input is
        // audio[0] = 1.0. After the §C.1.3 shift-and-insert these go
        // to X[0]..X[31] in the order (most recent first):
        // X[0] = 32, X[1] = 31, ..., X[31] = 1.
        for p in 0..NUM_SUBBANDS {
            let want = (NUM_SUBBANDS - p) as f64;
            assert_eq!(
                fb.x[p],
                want,
                "X[{p}] = {} but expected audio[{}] = {}",
                fb.x[p],
                NUM_SUBBANDS - 1 - p,
                want
            );
        }
    }

    #[test]
    fn shift_then_insert_preserves_old_samples_at_offset_32() {
        // After one push of audio_1, then another push of audio_2,
        // X[32]..X[63] must hold the *first* batch of inputs (because
        // they got shifted right by 32 slots and not overwritten).
        let mut fb = AnalysisFilterbank::new();
        let mut a1 = [0.0_f64; NUM_SUBBANDS];
        for (i, slot) in a1.iter_mut().enumerate() {
            *slot = 100.0 + i as f64; // 100..131
        }
        let mut a2 = [0.0_f64; NUM_SUBBANDS];
        for (i, slot) in a2.iter_mut().enumerate() {
            *slot = 200.0 + i as f64; // 200..231
        }
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&a1, &mut out);
        fb.push_audio(&a2, &mut out);

        // After two pushes:
        //   X[0..32]  = a2 in reverse-time order (most-recent at 0)
        //   X[32..64] = a1 in reverse-time order (was at X[0..32], now shifted to X[32..64])
        for p in 0..NUM_SUBBANDS {
            let want_recent = 200.0 + (NUM_SUBBANDS - 1 - p) as f64;
            assert_eq!(
                fb.x[p], want_recent,
                "X[{p}] = {} but expected {}",
                fb.x[p], want_recent
            );
            let want_prev = 100.0 + (NUM_SUBBANDS - 1 - p) as f64;
            assert_eq!(
                fb.x[NUM_SUBBANDS + p],
                want_prev,
                "X[{}] = {} but expected {}",
                NUM_SUBBANDS + p,
                fb.x[NUM_SUBBANDS + p],
                want_prev
            );
        }
    }

    #[test]
    fn output_is_finite_and_bounded_on_unit_dc_input() {
        // A unit DC input (all 32 samples = 1.0) cannot blow up: the
        // analysis window's small magnitudes (peak 0.035780907 at
        // i=256, secondary peak 0.000108719 at i=69/70) sum to at most
        // a few tenths in absolute value across the 8 polyphase taps,
        // and the matrix's cosine entries are bounded by ±1, so each
        // subband is bounded by `64 * max|Y_i|` which stays in a sane
        // numeric range.
        let mut fb = AnalysisFilterbank::new();
        let unit = [1.0_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        let mut max_mag = 0.0_f64;
        for _ in 0..64 {
            fb.push_audio(&unit, &mut out);
            for &v in &out {
                assert!(v.is_finite(), "non-finite subband sample {v}");
                max_mag = max_mag.max(v.abs());
            }
        }
        // A wide bound; we only care that nothing overflows / NaNs.
        assert!(
            max_mag < 100.0,
            "unit DC input produced large output ({max_mag})"
        );
    }

    #[test]
    fn output_size_matches_subbands() {
        // Sanity: 32 audio samples in, 32 subband samples out.
        let mut fb = AnalysisFilterbank::new();
        let audio = [0.1_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&audio, &mut out);
        assert_eq!(out.len(), NUM_SUBBANDS);
    }

    #[test]
    fn two_independent_channels_do_not_share_state() {
        // One filterbank instance per encoder channel — two instances
        // given different inputs must produce different outputs after
        // the same number of frames.
        let mut left = AnalysisFilterbank::new();
        let mut right = AnalysisFilterbank::new();
        let l_in = [0.5_f64; NUM_SUBBANDS];
        let mut r_in = [0.0_f64; NUM_SUBBANDS];
        r_in[5] = 0.8;
        let mut l_out = [0.0_f64; NUM_SUBBANDS];
        let mut r_out = [0.0_f64; NUM_SUBBANDS];
        for _ in 0..4 {
            left.push_audio(&l_in, &mut l_out);
            right.push_audio(&r_in, &mut r_out);
        }
        let any_diff = l_out.iter().zip(r_out.iter()).any(|(a, b)| a != b);
        assert!(any_diff, "independent channels produced identical output");
    }

    #[test]
    fn cold_start_output_is_zero_after_zero_input() {
        // A freshly-constructed filterbank fed exactly one block of
        // zeros must produce all-zero subband samples (X starts zero,
        // shift moves zeros, insert puts zeros at the front, every
        // product is zero).
        let mut fb = AnalysisFilterbank::new();
        let zero = [0.0_f64; NUM_SUBBANDS];
        let mut out = [0.0_f64; NUM_SUBBANDS];
        fb.push_audio(&zero, &mut out);
        for (j, &v) in out.iter().enumerate() {
            assert_eq!(v, 0.0, "out[{j}] = {v}, expected 0");
        }
    }
}
