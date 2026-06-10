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
//! This module lands the spec-text-only halves of §D.1 Model 1:
//!
//! * **Step 1 Hann window** ([`hann_window_layer2`]) — the verbatim
//!   spec equation `h(i) = sqrt(8/3) * 0.5 * (1 - cos(2 * pi * i /
//!   N))` for the Layer II 1024-sample FFT (`N == 1024`); no table.
//! * **Step 4(a) local-maxima labelling** ([`is_local_maximum`]) —
//!   the verbatim spec rule `X(k) > X(k - 1) AND X(k) >= X(k + 1)`.
//! * **Step 4(b) Layer II tonality test** ([`is_tonal_layer2`]) — the
//!   verbatim spec rule `X(k) - X(k + j) >= 7 dB` for every `j` in
//!   the per-`k` neighbourhood ([`tonal_neighbourhood_layer2`]); the
//!   neighbourhood widths are the verbatim spec table at PDF p.117
//!   (printed 111).
//! * **Step 4(b) tonal-component SPL** ([`tonal_spl_db`]) — the
//!   three-line power sum `X_tm = 10 * log10(10^(X(k-1)/10) +
//!   10^(X(k)/10) + 10^(X(k+1)/10))`.
//! * **Step 4(b) zero-out of the examined frequency range**
//!   ([`zero_tonal_neighbourhood_layer2`]) — the spec sentence "all
//!   spectral lines within the examined frequency range are set to
//!   −∞ dB" applied to the tonality-test neighbourhood around a
//!   confirmed tonal line.
//! * **Step 4(b) tonal-component listing sweep**
//!   ([`list_tonal_layer2`]) — the per-spectrum loop that combines
//!   [`is_tonal_layer2`] + [`tonal_spl_db`] +
//!   [`zero_tonal_neighbourhood_layer2`] across `2 < k <= 500`,
//!   emitting a [`TonalCandidate`] for every confirmed line and
//!   leaving the input spectrum with each confirmed tonal's
//!   "examined frequency range" set to −∞ dB ready for Step 4(c).
//!   The carrier intentionally omits the masker Bark position; the
//!   FFT-line → Bark mapping is part of the §D.1 Step 6 input
//!   transformation and is gated on the PNG-only D.1 tables (cf.
//!   `#1262`).
//! * **Step 4(c) listing of non-tonal components**
//!   ([`non_tonal_spl_db`] for a single critical band;
//!   [`list_non_tonal_layer2`] for the per-sampling-rate sweep
//!   using the text-extracted Annex D Tables D.2d / D.2e / D.2f).
//!   Each band is power-summed across its `(prev_top, top]` FFT-line
//!   run; the representative index of the resulting non-tonal masker
//!   is the spec's "nearest to the geometric mean of the critical
//!   band" rule, applied directly on FFT-line indices.
//! * **Step 5(b) tonal-masker decimation within 0.5 Bark**
//!   ([`decimate_tonal_maskers`]) — the verbatim spec procedure
//!   "Decimation of two or more tonal components within a distance
//!   of less than 0.5 Bark: Keep the component with the highest
//!   power, and remove the smaller component(s) from the list of
//!   tonal components. For this operation, a sliding window in the
//!   critical band domain is used with a width of 0.5 Bark."
//! * **Step 6 masker indices and masking function `vf`**
//!   ([`masking_index_tonal`], [`masking_index_non_tonal`],
//!   [`masking_function_vf`]) and the per-masker individual masking
//!   threshold ([`individual_masking_threshold_db`]).
//! * **Step 7 global masking threshold `LTg`**
//!   ([`global_masking_threshold_db`]) plus the spec optimisation
//!   pre-filter ([`masker_in_target_window`] /
//!   [`relevant_maskers_for_target_line`]) implementing the
//!   verbatim "For a given `i` the range of `j` may be reduced to
//!   maskers within `-8…+3` Bark of `i`" sentence (PDF p.120,
//!   printed 114). The pre-filter is equivalence-preserving on
//!   `LTg`; it only trims maskers that `masking_function_vf` would
//!   have collapsed to `-inf` anyway.
//! * **Step 8 minimum masking threshold per subband**
//!   ([`minimum_masking_threshold_subband`]) — the verbatim spec
//!   reduction `LT_min(n) = MIN[ LT_g(i) ]` over `f(i) in subband n`,
//!   driven by a caller-supplied `line_subband` map (the spec's
//!   `f(i)` frequency vector lives in the PNG-only Table D.1 inner
//!   rows; the caller produces the FFT-line → subband mapping from
//!   whatever source they have and the primitive runs the bare
//!   minimum-over-mask reduction).
//! * **Step 9 signal-to-mask ratio per subband**
//!   ([`signal_to_mask_ratio_subband`]) — the verbatim spec
//!   subtraction `SMR_sb(n) = L_sb(n) - LT_min(n)`.
//!
//! Step 5(a) "threshold-in-quiet decimation" requires the LTq
//! values from Annex D Table D.1d/e/f and Step 2/3 also depend on
//! those PNG-only rows (the LTq threshold-in-quiet curves for
//! Layer II at 32 / 44.1 / 48 kHz, plus the per-subband range
//! mapping). They are tracked under the docs-collaborator Tables
//! D.1a–f / D.3a–c / D.4a–c PNG → text extraction gap (note
//! `#1262`).
//!
//! Step 4(c) and the Layer-II critical-band-boundary tables
//! D.2d / D.2e / D.2f are independent of that gap — those tables
//! survive `pdftotext` cleanly and are transcribed verbatim in
//! [`crate::tables_d2`]. The Layer-II band-count discrepancy noted
//! there (D.1 Step 4(c) prose "24 / 26 / 26" vs. the published
//! 25 / 27 / 27-row tables) is left unresolved at this layer; the
//! primitive iterates every published row and a future round will
//! decide whether downstream callers trim the topmost entry.
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

/// Length of the §D.1 Step 1 Layer II FFT window — verbatim from the
/// "Technical data of the FFT" table on PDF page 116 (printed 110):
/// the Layer II FFT is 1024 samples (the Layer I FFT is 512).
pub const LAYER2_FFT_LEN: usize = 1024;

/// Number of FFT bins produced by the §D.1 Step 1 Layer II FFT that
/// the psychoacoustic-model passes look at: `k = 0 .. N / 2` per the
/// `X(k)` definition, i.e. the DC bin through the Nyquist bin
/// inclusive. The downstream §D.1 Step 4 prose runs `2 < k <= 500`
/// for Layer II, so the working range is comfortably inside this.
pub const LAYER2_FFT_BINS: usize = LAYER2_FFT_LEN / 2 + 1;

/// §D.1 Step 4(b) tonality test threshold: `X(k) - X(k+j) >= 7 dB`
/// for every `j` in the per-`k` neighbourhood. The 7 dB value is the
/// verbatim spec constant — a local maximum is classified as tonal
/// (sinusoid-like) only if it stands at least 7 dB above every
/// surrounding bin in the windowed neighbourhood.
pub const TONALITY_THRESHOLD_DB: f64 = 7.0;

/// §D.1 Step 1 Hann window for the Layer II 1024-sample FFT. The
/// verbatim spec equation (PDF page 116, printed 110):
///
/// ```text
/// h(i) = sqrt(8/3) * 0.5 * (1 - cos(2 * pi * i / N))   for 0 <= i <= N - 1
/// ```
///
/// where `N == LAYER2_FFT_LEN == 1024`. The `sqrt(8/3) ≈ 1.6329932`
/// front coefficient is the spec's normalization for the Hann
/// window's power gain — the windowed power-density estimate
/// matches the unwindowed signal's RMS power on broadband input.
///
/// The slot at `i = 0` is `0.0` exactly (because `1 - cos(0) = 0`);
/// the slot at `i = N - 1` is `0.5 * sqrt(8/3) * (1 - cos(2 * pi *
/// (N - 1) / N))`, which is non-zero — the window is **not**
/// periodic in the DFT sense (the spec writes `cos[2 * pi * i / N]`
/// and the index range is `0 <= i <= N - 1`, so the window does not
/// reach the next-period zero crossing).
#[must_use]
pub fn hann_window_layer2() -> [f64; LAYER2_FFT_LEN] {
    let mut window = [0.0_f64; LAYER2_FFT_LEN];
    // sqrt(8/3) — the spec's front coefficient. Computed inline so
    // the constant is derived from the spec equation rather than a
    // baked-in numeric literal.
    let coeff = (8.0_f64 / 3.0).sqrt() * 0.5;
    let two_pi_over_n = 2.0 * core::f64::consts::PI / (LAYER2_FFT_LEN as f64);
    let mut i = 0;
    while i < LAYER2_FFT_LEN {
        window[i] = coeff * (1.0 - (two_pi_over_n * i as f64).cos());
        i += 1;
    }
    window
}

/// §D.1 Step 4(a) local-maximum test for the SPL spectrum `X(k)`.
/// Verbatim spec rule (PDF page 117, printed 111):
///
/// ```text
/// A spectral line X(k) is labelled as a local maximum if
///     X(k) > X(k - 1) and X(k) >= X(k + 1)
/// ```
///
/// Note the asymmetry: strict `>` on the lower side, non-strict
/// `>=` on the upper side. This deterministically picks the
/// left-most index when a plateau spans several adjacent bins.
///
/// `k` outside the open interval `0 < k < spl_db.len() - 1` returns
/// `false` — the edges of the spectrum have no defined neighbour.
#[must_use]
pub fn is_local_maximum(spl_db: &[f64], k: usize) -> bool {
    if k == 0 || k + 1 >= spl_db.len() {
        return false;
    }
    spl_db[k] > spl_db[k - 1] && spl_db[k] >= spl_db[k + 1]
}

/// §D.1 Step 4(b) Layer II tonality-neighbourhood widths. Verbatim
/// spec table for the 1024-point Layer II FFT (PDF page 117,
/// printed 111):
///
/// ```text
/// j = -2, +2                       for   2 < k <  63
/// j = -3, -2, +2, +3               for  63 <= k < 127
/// j = -6, ..., -2, +2, ..., +6     for 127 <= k < 255
/// j = -12, ..., -2, +2, ..., +12   for 255 <= k <= 500
/// ```
///
/// Returns `None` for `k <= 2` or `k > 500` (the spec leaves
/// tonality undefined at the spectrum's edges). The returned slice
/// is the set of strictly-non-zero offsets — `j = 0` (the line
/// itself) is excluded by construction.
#[must_use]
pub fn tonal_neighbourhood_layer2(k: usize) -> Option<&'static [i32]> {
    // Static neighbourhood slices, one per spec row.
    static N_TWO: &[i32] = &[-2, 2];
    static N_THREE: &[i32] = &[-3, -2, 2, 3];
    static N_SIX: &[i32] = &[-6, -5, -4, -3, -2, 2, 3, 4, 5, 6];
    static N_TWELVE: &[i32] = &[
        -12, -11, -10, -9, -8, -7, -6, -5, -4, -3, -2, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    ];
    if k <= 2 {
        None
    } else if k < 63 {
        Some(N_TWO)
    } else if k < 127 {
        Some(N_THREE)
    } else if k < 255 {
        Some(N_SIX)
    } else if k <= 500 {
        Some(N_TWELVE)
    } else {
        None
    }
}

/// §D.1 Step 4(b) tonality test. A local maximum `X(k)` is tonal
/// iff for **every** `j` in the per-`k` neighbourhood returned by
/// [`tonal_neighbourhood_layer2`],
///
/// ```text
/// X(k) - X(k + j) >= 7 dB
/// ```
///
/// (verbatim spec inequality, [`TONALITY_THRESHOLD_DB`] is the spec
/// constant). Returns `false` for `k` outside the tonality-defined
/// range and for any `k + j` that would fall outside `spl_db`.
#[must_use]
pub fn is_tonal_layer2(spl_db: &[f64], k: usize) -> bool {
    let Some(neighbourhood) = tonal_neighbourhood_layer2(k) else {
        return false;
    };
    // The X(k) reference must itself be a local maximum per the
    // spec's two-step procedure — Step 4(a) labels then Step 4(b)
    // tests. Enforce the precondition here so callers that drive
    // `is_tonal_layer2` directly without going through Step 4(a)
    // still get the spec-defined classification.
    if !is_local_maximum(spl_db, k) {
        return false;
    }
    for &j in neighbourhood {
        // i32 arithmetic to safely handle negative j around small k.
        let probe = k as i32 + j;
        if probe < 0 || (probe as usize) >= spl_db.len() {
            return false;
        }
        if spl_db[k] - spl_db[probe as usize] < TONALITY_THRESHOLD_DB {
            return false;
        }
    }
    true
}

/// §D.1 Step 4(b) tonal-component SPL aggregation. After a `(k, X(k))`
/// has been classified as tonal, the spec replaces the bin's SPL with
/// the three-line power sum centered on `k`:
///
/// ```text
/// X_tm(k) = 10 * log10( 10^(X(k-1)/10) + 10^(X(k)/10) + 10^(X(k+1)/10) )
/// ```
///
/// Returns `None` when `k` is at an edge (no `X(k - 1)` or
/// `X(k + 1)` is defined).
#[must_use]
pub fn tonal_spl_db(spl_db: &[f64], k: usize) -> Option<f64> {
    if k == 0 || k + 1 >= spl_db.len() {
        return None;
    }
    let p_lo = (10.0_f64).powf(spl_db[k - 1] / 10.0);
    let p_at = (10.0_f64).powf(spl_db[k] / 10.0);
    let p_hi = (10.0_f64).powf(spl_db[k + 1] / 10.0);
    Some(10.0 * (p_lo + p_at + p_hi).log10())
}

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

/// §D.1 Step 7 spec optimisation predicate: does masker `z(j)`
/// contribute a non-`-inf` individual masking threshold to the target
/// line at `z(i)`?
///
/// The spec phrasing (PDF page 120, printed 114):
///
/// > For a given `i` the range of `j` may be reduced to maskers
/// > within `-8…+3` Bark of `i`.
///
/// This is the symmetric read of the §D.1 Step 6 masking-function
/// window. The piecewise `vf(dz, X)` (cf. [`masking_function_vf`])
/// is defined for `dz = z(i) - z(j) ∈ [-3, 8)`, so equivalently the
/// masker at `z(j)` only contributes to lines in
/// `[z(j) - 3, z(j) + 8)`, equivalently a target line at `z(i)` is
/// only influenced by maskers in `(z(i) - 8, z(i) + 3]`.
///
/// The predicate exactly mirrors the half-open / half-closed pattern
/// of `vf`'s `[-3, 8)` reading — a masker at `z(j) = z(i) - 8` is
/// **excluded** (it would correspond to `dz = 8` which `vf` rejects)
/// and a masker at `z(j) = z(i) + 3` is **included** (corresponds to
/// `dz = -3` which `vf` accepts as its lower endpoint). The
/// half-open / half-closed asymmetry preserves the spec wording
/// without modification.
///
/// `NaN` `z_j_bark` returns `false` — a `NaN` masker is treated as
/// "outside the window" rather than propagating into the energy sum.
/// This matches the semantics of [`masking_function_vf`], whose
/// `Range::contains` guard rejects `NaN` inputs.
#[inline]
#[must_use]
pub fn masker_in_target_window(z_j_bark: f64, z_i_bark: f64) -> bool {
    // The window in `z(j)` is `(z(i) - 8, z(i) + 3]`. Rewriting in
    // terms of `dz = z(i) - z(j)` gives `dz ∈ [-3, 8)` — the
    // identical predicate `masking_function_vf` applies to its `dz`
    // argument. Reuse that wording exactly to keep both spellings in
    // lock-step.
    let dz = z_i_bark - z_j_bark;
    (MASKING_FUNCTION_DZ_LO..MASKING_FUNCTION_DZ_HI).contains(&dz)
}

/// §D.1 Step 7 spec optimisation: filter a masker list to just
/// the entries that lie within the `-8…+3` Bark window around the
/// target line `z(i)`. The returned entries are exactly the maskers
/// that produce a non-`-inf` individual masking threshold at `z(i)`
/// per [`individual_masking_threshold_db`], i.e. the ones whose
/// power contribution `10^(LT / 10)` is non-zero in the
/// [`global_masking_threshold_db`] energy sum.
///
/// The spec phrasing (PDF page 120, printed 114):
///
/// > For a given `i` the range of `j` may be reduced to maskers
/// > within `-8…+3` Bark of `i`.
///
/// Pre-filtering is purely an optimisation — the unfiltered
/// [`global_masking_threshold_db`] sum already drops out-of-window
/// maskers via [`masking_function_vf`] returning `None`. The
/// invariant tested by [`tests::pre_filter_preserves_global_masking_threshold_db`]
/// holds: feeding the filtered list to `global_masking_threshold_db`
/// produces the same `LTg(i)` (to within `f64::EPSILON`) as feeding
/// the full list.
///
/// The returned vector preserves input order: maskers are emitted
/// left-to-right in the same order they appear in `maskers`. No
/// implicit sort or deduplication is applied — the predicate is
/// stateless and pointwise. Callers that need an unallocated path
/// can use [`masker_in_target_window`] directly.
#[must_use]
pub fn relevant_maskers_for_target_line(maskers: &[Masker], z_i_bark: f64) -> Vec<Masker> {
    maskers
        .iter()
        .copied()
        .filter(|m| masker_in_target_window(m.z_bark, z_i_bark))
        .collect()
}

/// §D.1 Step 4(b) zero-out of the **examined frequency range** for a
/// confirmed tonal line.
///
/// The spec phrasing (PDF page 112, printed 112):
///
/// > Next, all spectral lines within the examined frequency range
/// > are set to −∞ dB.
///
/// The "examined frequency range" is the neighbourhood used by the
/// Step 4(b) tonality test — for FFT-line `k` at Layer II this is
/// `[k + j_min, k + j_max]` where `j_min`, `j_max` are the spec's
/// per-`k` `j` table (cf. [`tonal_neighbourhood_layer2`]). The line
/// itself (`j = 0`) is also included; the tonal masker's SPL has
/// already been folded into `X_tm(k)` by [`tonal_spl_db`] and the
/// raw FFT line is no longer used downstream.
///
/// The "set to −∞ dB" sentinel is represented here as
/// [`f64::NEG_INFINITY`]; the Step 4(c) power sum
/// (`10^(X(k)/10)`) then evaluates `10^(-inf / 10) = 0` and the
/// zeroed lines contribute nothing.
///
/// Out-of-range `k` (no tonality neighbourhood defined for `k <= 2`
/// or `k > 500`) is a no-op — the spec only zeroes the neighbourhood
/// of a confirmed tonal component and confirmed components live
/// inside the `tonal_neighbourhood_layer2` definition domain.
pub fn zero_tonal_neighbourhood_layer2(spl_db: &mut [f64], k: usize) {
    let Some(neighbourhood) = tonal_neighbourhood_layer2(k) else {
        return;
    };
    // Zero the line itself first; downstream Step 4(c) reads
    // `spl_db[k]` and must see the −∞ sentinel.
    if k < spl_db.len() {
        spl_db[k] = f64::NEG_INFINITY;
    }
    for &j in neighbourhood {
        let probe = k as i32 + j;
        if probe < 0 {
            continue;
        }
        let idx = probe as usize;
        if idx < spl_db.len() {
            spl_db[idx] = f64::NEG_INFINITY;
        }
    }
}

/// A single Step 4(b) tonal candidate: the FFT-line index `k` at
/// which a tonal masker was detected, and the three-line power-sum
/// SPL `X_tm(k)` (in dB) computed at that line.
///
/// This carrier does **not** include the masker's Bark position
/// `z(j)` — the FFT-line → Bark mapping lives in the PNG-only
/// Annex D Table D.1d / D.1e / D.1f Layer II columns and is not
/// available in the staged text extracts this round (note `#1262`).
/// When that material lands, the caller may convert each
/// [`TonalCandidate`] into a [`Masker`] of [`MaskerKind::Tonal`]
/// kind by looking up `z(j) = z[k]` from the table and using
/// `spl_db = candidate.spl_db`.
///
/// The two-stage carrier (line-index now, Bark later) keeps the
/// Step 4(b) sweep self-contained: every primitive it depends on
/// (`is_local_maximum`, `is_tonal_layer2`, `tonal_spl_db`,
/// `zero_tonal_neighbourhood_layer2`) is spec-text-only, and the
/// Bark assignment that *would* be PNG-blocked is pushed to the
/// Step 6 input transformation where it belongs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TonalCandidate {
    /// FFT-line index at which the tonal masker was detected
    /// (the `k` of [`is_tonal_layer2`]). Always satisfies
    /// `2 < k <= 500` per the §D.1 Step 4(b) tonality-defined
    /// range (cf. [`tonal_neighbourhood_layer2`]).
    pub k: usize,
    /// Three-line tonal-component power sum `X_tm(k) = 10 * log10(
    /// 10^(X(k-1)/10) + 10^(X(k)/10) + 10^(X(k+1)/10) )` in dB
    /// (the result of [`tonal_spl_db`] at this `k`).
    pub spl_db: f64,
}

/// §D.1 Step 4(b) tonal-component listing sweep for Layer II.
///
/// The spec phrasing (PDF page 112, printed 112) describes the
/// per-FFT-line procedure: "A spectral line `X(k)` is labelled
/// as tonal if it is a local maximum [Step 4(a)] and …
/// `X(k) - X(k + j) >= 7 dB` for every `j` in the per-`k`
/// neighbourhood [Step 4(b) inequality table]. When `X(k)` is
/// tonal the SPL is computed by `X_tm(k) = 10 * log10( … )` …
/// Next, all spectral lines within the examined frequency range
/// are set to −∞ dB." This sweep is the loop that drives those
/// per-`k` primitives across the spectrum.
///
/// `spl_db` is the §D.1 Step 1 windowed-FFT SPL spectrum in dB,
/// passed by mutable reference. On return:
///
/// * The output `Vec<TonalCandidate>` lists every confirmed tonal
///   line in ascending `k` order (`2 < k <= 500`), carrying the
///   FFT-line index and the three-line tonal SPL `X_tm(k)`.
/// * The `spl_db` slice has had each confirmed tonal line's
///   neighbourhood (per [`zero_tonal_neighbourhood_layer2`]) set
///   to `f64::NEG_INFINITY` — the post-zero-out spectrum suitable
///   for the §D.1 Step 4(c) non-tonal listing
///   ([`list_non_tonal_layer2`]).
///
/// The sweep visits `k` in ascending order from 3 (the lowest
/// tonality-defined index — `2 < k` per the spec inequality) up
/// to `min(500, spl_db.len() - 1)` inclusive. At each `k`:
///
/// 1. The §D.1 Step 4(b) tonality test ([`is_tonal_layer2`]) is
///    invoked. Because that primitive precondition-checks the
///    Step 4(a) local-maximum rule itself, no separate
///    [`is_local_maximum`] call is needed here.
/// 2. On a positive classification, [`tonal_spl_db`] computes the
///    three-line `X_tm(k)`; the `(k, X_tm)` pair is pushed onto
///    the output list.
/// 3. [`zero_tonal_neighbourhood_layer2`] then sets the
///    neighbourhood (including `k` itself) to −∞ dB. The sweep
///    continues at `k + 1` against the now-modified `spl_db`.
///
/// Subsequent `k`s that fell within an earlier confirmed tonal's
/// neighbourhood see those bins at −∞ dB. They cannot satisfy
/// either the local-maximum rule (the line at −∞ cannot exceed
/// its −∞ neighbours by 7 dB) or the tonality inequality. This
/// matches the spec's "examined frequency range … set to −∞ dB"
/// instruction: the sweep naturally skips lines inside an
/// already-claimed neighbourhood without an explicit "skip" list.
///
/// Edge handling:
/// * `spl_db.len() < 4` returns an empty list with no mutation
///   (the spec's `2 < k` precondition is unreachable).
/// * `k > 500` is silently skipped per
///   [`tonal_neighbourhood_layer2`]'s definition domain.
/// * The very first and very last entries of `spl_db` are never
///   classified as tonal (the local-maximum rule needs both
///   neighbours; `k = 0` and `k == spl_db.len() - 1` fail
///   [`is_local_maximum`] by construction).
///
/// The output capacity is bounded by the per-neighbourhood width
/// of the spec table: the densest region (`2 < k < 63`) places
/// neighbourhood ends two bins apart, so an upper bound of
/// `spl_db.len() / 3` is sufficient for any input.
pub fn list_tonal_layer2(spl_db: &mut [f64]) -> Vec<TonalCandidate> {
    let mut out: Vec<TonalCandidate> = Vec::new();
    if spl_db.len() < 4 {
        return out;
    }
    // The §D.1 Step 4(b) tonality test is defined on `2 < k <= 500`
    // (cf. `tonal_neighbourhood_layer2`). Iterate the intersection
    // of that domain with the spectrum length; the inclusive upper
    // bound is `spl_db.len() - 2` because `is_local_maximum`
    // requires both `k - 1` and `k + 1` to exist.
    let k_end = core::cmp::min(500usize, spl_db.len() - 2);
    let mut k = 3_usize;
    while k <= k_end {
        // `is_tonal_layer2` enforces Step 4(a) internally; on a
        // positive result the three-line SPL is well-defined.
        if is_tonal_layer2(spl_db, k) {
            if let Some(x_tm) = tonal_spl_db(spl_db, k) {
                out.push(TonalCandidate { k, spl_db: x_tm });
                // Apply the spec's "set to −∞ dB" instruction
                // before advancing — the next-step lines inside
                // this neighbourhood are now ineligible for their
                // own tonal classification, matching the spec's
                // "examined frequency range" exclusion rule.
                zero_tonal_neighbourhood_layer2(spl_db, k);
            }
        }
        k += 1;
    }
    out
}

/// §D.1 Step 4(c) non-tonal SPL `X_nm(k)` for a single critical band.
///
/// The spec phrasing (PDF page 112, printed 112):
///
/// > Within each critical band, the power of the spectral lines
/// > (remaining after the tonal components have been zeroed) are
/// > summed to form the sound pressure level of the new non-tonal
/// > component X_nm(k) corresponding to that critical band.
///
/// `spl_db` carries the post-Step-4(b)-zero-out spectrum in dB.
/// `lo` is the first FFT-line index of the critical band (inclusive)
/// and `hi` is the top FFT-line index (inclusive) — the Annex D
/// Table D.2 `index F&CB` column. A pair from successive D.2
/// rows defines one critical band as
/// `[prev_top + 1, top]` (the very first band of the table runs
/// from FFT-line 0 to the first table row's `top_line_index`).
///
/// The aggregation is the same power-sum-then-log used in
/// [`tonal_spl_db`] and [`global_masking_threshold_db`]:
///
/// ```text
/// X_nm = 10 * log10( Sum 10^(X(k)/10) )      dB
///        k in [lo, hi]
/// ```
///
/// Returns `None` when the band is empty (`lo > hi` or `lo` past the
/// spectrum end) or every line in the band is `-inf dB` (the
/// band carried only tonal-zeroed content; `10^(-inf/10) = 0` and
/// `log10(0)` is `-inf`, which the caller usually wants to skip
/// rather than carry as a finite masker — represent this as
/// `None`).
#[must_use]
pub fn non_tonal_spl_db(spl_db: &[f64], lo: usize, hi: usize) -> Option<f64> {
    if lo > hi || lo >= spl_db.len() {
        return None;
    }
    let upper = hi.min(spl_db.len() - 1);
    let mut energy_sum = 0.0_f64;
    let mut any_finite = false;
    for &x in &spl_db[lo..=upper] {
        if x.is_finite() {
            energy_sum += (10.0_f64).powf(x / 10.0);
            any_finite = true;
        }
    }
    if !any_finite || energy_sum <= 0.0 {
        return None;
    }
    Some(10.0 * energy_sum.log10())
}

/// §D.1 Step 4(c) **representative FFT-line index** for a critical
/// band, picked as the line **nearest to the geometric mean** of the
/// band's FFT-line index range.
///
/// The spec phrasing (PDF page 113, printed 113):
///
/// > Index number k of the spectral line nearest to the geometric
/// > mean of the critical band.
///
/// The geometric mean of `[lo, hi]` is `sqrt(lo * hi)` (with `lo == 0`
/// special-cased to `1` — the DC bin is excluded from the geometric
/// mean because `sqrt(0 * hi) = 0` collapses to band lo regardless of
/// hi). The returned index is the integer in `[lo, hi]` closest to
/// that mean — ties round down (the lower index wins) because the
/// spec doesn't define a tie-break and the lower-index choice is
/// stable under floating-point rounding.
///
/// Returns `None` when the band is empty (`lo > hi`) — the caller is
/// then expected to skip this band entirely.
#[must_use]
pub fn non_tonal_band_index(lo: usize, hi: usize) -> Option<usize> {
    if lo > hi {
        return None;
    }
    if lo == hi {
        return Some(lo);
    }
    // Geometric mean. The DC bin (lo == 0) is excluded — the
    // geometric mean of [0, hi] is identically 0, which collapses
    // the representative line to the band's lower edge regardless
    // of how wide the band actually is. Substitute lo = 1 for
    // this single case (the spec treats DC as below the tonality
    // domain, cf. `2 < k` in Step 4(b)).
    let lo_eff = if lo == 0 { 1 } else { lo };
    let geo = ((lo_eff as f64) * (hi as f64)).sqrt();
    // Pick the integer index in [lo, hi] closest to `geo`. Tie
    // breaks downward (round-half-down) — at fractional 0.5 the
    // lower index is returned.
    let floor = geo.floor() as usize;
    let ceil = (floor + 1).min(hi);
    let d_floor = (geo - floor as f64).abs();
    let d_ceil = (geo - ceil as f64).abs();
    let picked = if d_ceil < d_floor { ceil } else { floor };
    Some(picked.clamp(lo, hi))
}

/// §D.1 Step 4(c) non-tonal listing pass for Layer II, sweeping
/// every critical band of the supplied sampling rate using the
/// text-extracted Annex D Tables D.2d / D.2e / D.2f (see
/// [`crate::tables_d2`]).
///
/// `spl_db` is the post-Step-4(b)-zero-out spectrum in dB; the
/// function reads it without mutating it. For each critical band
/// `[prev_top + 1, top]` (the first band runs `[0, first_top]`) the
/// pass produces one [`Masker`] of [`MaskerKind::NonTonal`] kind:
///
/// * `spl_db` = [`non_tonal_spl_db`] across the band's FFT lines,
/// * `z_bark` = the Annex D Table D.2 `Bark [z]` column of the
///   band's top line (the spec doesn't tabulate Bark at every line
///   index, so the boundary's Bark is the convention used here for
///   the Bark-axis placement consumed by Step 6 `vf`),
/// * The representative FFT-line index (per
///   [`non_tonal_band_index`]) is not carried on `Masker` directly —
///   it lives only inside Step 4(c) and is dropped before Step 6.
///   For callers that need the line index alongside the masker,
///   use [`non_tonal_band_index`] separately on the same `(lo, hi)`
///   pair.
///
/// Bands that carry only `-inf dB` lines (the whole band was
/// tonal-zeroed) are silently dropped — `non_tonal_spl_db` returns
/// `None` and that band contributes no entry. The returned vector
/// therefore has at most `boundaries.len()` entries (typically all
/// of them; a clean tonal-zero-out only removes one band when the
/// entire critical band is occupied by a single tonal neighbourhood,
/// which is rare in practice).
///
/// The output vector is allocated by the function; no scratch buffer
/// argument is needed because the per-call allocation is bounded by
/// the small Layer II band count (25 / 27 / 27).
#[must_use]
pub fn list_non_tonal_layer2(spl_db: &[f64], fs: crate::tables_d2::SamplingRate) -> Vec<Masker> {
    let boundaries = fs.critical_band_boundaries();
    let mut out = Vec::with_capacity(boundaries.len());
    // First band: FFT lines [0, boundaries[0].top_line_index].
    // Subsequent bands: [prev_top + 1, top].
    let mut lo = 0_usize;
    for boundary in boundaries {
        let hi = boundary.top_line_index as usize;
        if let Some(x_nm) = non_tonal_spl_db(spl_db, lo, hi) {
            out.push(Masker {
                kind: MaskerKind::NonTonal,
                z_bark: boundary.top_bark,
                spl_db: x_nm,
            });
        }
        lo = hi + 1;
    }
    out
}

/// §D.1 Step 5(b) sliding-window width in Bark for tonal-masker
/// decimation. The verbatim spec phrasing (PDF page 113, printed
/// 113):
///
/// > Decimation of two or more tonal components within a distance
/// > of less than 0.5 Bark: Keep the component with the highest
/// > power, and remove the smaller component(s) from the list of
/// > tonal components. For this operation, a sliding window in the
/// > critical band domain is used with a width of 0.5 Bark.
pub const TONAL_DECIMATION_WINDOW_BARK: f64 = 0.5;

/// §D.1 Step 5(b) tonal-masker decimation: collapse clusters of
/// tonal maskers within [`TONAL_DECIMATION_WINDOW_BARK`] of each
/// other on the Bark axis, keeping the highest-SPL member of each
/// cluster and removing the others. The verbatim spec procedure
/// (PDF page 113, printed 113) reads as a sliding window of width
/// `0.5 Bark` over the critical-band axis; this implementation
/// realises it by sorting the input by `z_bark` and merging any
/// run of adjacent tonal entries whose `z_bark` span is
/// strictly less than `0.5 Bark`, keeping the highest-`spl_db`
/// member of each run and dropping the rest.
///
/// The operation is applied **only to tonal maskers** per the spec
/// ("Decimation of two or more **tonal** components") — non-tonal
/// maskers are left untouched. Their relative order with the
/// surviving tonal maskers is preserved: the function reads
/// `maskers` left-to-right and rebuilds the list so that the
/// emitted vector keeps the spec's "combined decimated list"
/// semantics (clause D.1 step 5 closing sentence). Non-tonal
/// maskers are emitted in the order they appeared in the input;
/// the surviving tonal maskers are emitted in ascending Bark order
/// (the spec doesn't require a particular order for the combined
/// list — Step 6 / Step 7 are order-invariant — but the ascending
/// Bark order is convenient for downstream debug-printing and the
/// 0.5-Bark merge is most naturally expressed on a sorted run).
///
/// The "strictly less than 0.5 Bark" wording is reproduced
/// exactly: two maskers at Bark positions `z1 < z2` are in the
/// same window iff `z2 - z1 < 0.5`. A pair at exactly `0.5 Bark`
/// distance is **not** merged (the sliding-window endpoint is
/// half-open per the standard one-sided "within a distance of
/// less than 0.5 Bark" reading).
///
/// Ties (`spl_db` exactly equal between two tonal entries within
/// the window) keep the **first** one in input order and drop the
/// later ones — the spec doesn't specify a tie-break but the
/// deterministic first-wins choice matches the behaviour of
/// `is_local_maximum`'s left-most pick on plateaus (cf. Step
/// 4(a)). This keeps the decimation a pure function of the input.
///
/// This primitive does **not** implement Step 5(a) — the
/// threshold-in-quiet comparison `X_tm(k) >= LT_q(k)` /
/// `X_nm(k) >= LT_q(k)` requires the Annex D Table D.1d / D.1e /
/// D.1f Layer II `LT_q` curves which are PNG-only in the staged
/// PDF (note `#1262`). When that material is text-extracted, a
/// companion `decimate_below_threshold_in_quiet` primitive can be
/// added and the two passes composed end-to-end per the spec
/// ordering (5(a) first, then 5(b)).
#[must_use]
pub fn decimate_tonal_maskers(maskers: &[Masker]) -> Vec<Masker> {
    // Step 1: split maskers into the tonal and non-tonal lists,
    // preserving input order in each.
    let mut tonal: Vec<Masker> = Vec::new();
    let mut non_tonal: Vec<Masker> = Vec::new();
    for &m in maskers {
        match m.kind {
            MaskerKind::Tonal => tonal.push(m),
            MaskerKind::NonTonal => non_tonal.push(m),
        }
    }
    // Step 2: sort the tonal list by Bark position so adjacent
    // entries are window candidates. Use `total_cmp` so the sort
    // is total even on NaN inputs (which would be a caller error
    // anyway — Bark positions come from finite Annex D tables).
    tonal.sort_by(|a, b| a.z_bark.total_cmp(&b.z_bark));

    // Step 3: walk the sorted tonal list and emit the survivors.
    // A "run" is a maximal subsequence of entries whose pairwise
    // Bark spread is < 0.5: as long as the next entry's z_bark is
    // within 0.5 Bark of the run's *start*, it joins the run.
    // (Reading the spec as "sliding window of width 0.5 Bark on
    // the critical band axis": every pair in the window is within
    // 0.5 Bark of every other, so anchoring on the run's start
    // produces the same survivors as anchoring on any other
    // window position — but only if the survivors' own spread is
    // also < 0.5. We additionally check the run's own width below
    // to keep the predicate the canonical "every pair < 0.5".)
    let mut surviving_tonal: Vec<Masker> = Vec::new();
    let mut i = 0;
    while i < tonal.len() {
        let start_z = tonal[i].z_bark;
        let mut best_idx = i;
        let mut best_spl = tonal[i].spl_db;
        let mut j = i + 1;
        while j < tonal.len() {
            let z = tonal[j].z_bark;
            if z - start_z >= TONAL_DECIMATION_WINDOW_BARK {
                break;
            }
            // Strict ">" so the *first* entry wins on equal SPL
            // (deterministic left-most pick).
            if tonal[j].spl_db > best_spl {
                best_spl = tonal[j].spl_db;
                best_idx = j;
            }
            j += 1;
        }
        surviving_tonal.push(tonal[best_idx]);
        i = j;
    }

    // Step 4: build the "combined decimated list" — non-tonal
    // maskers in input order, then surviving tonal maskers in
    // ascending Bark order. Order is irrelevant for Steps 6 / 7
    // but the deterministic interleave makes the output
    // round-tripable for tests.
    let mut out = non_tonal;
    out.extend(surviving_tonal);
    out
}

/// §D.1 Step 8 minimum masking threshold per Layer II subband.
/// The verbatim spec equation (PDF page 114, printed 114):
///
/// ```text
/// LT_min(n) = MIN[ LT_g(i) ]   dB
///             f(i) in subband n
/// ```
///
/// A minimum masking level `LT_min(n)` is computed for every
/// subband. `f(i)` is the frequency of the i'th frequency sample,
/// tabulated in the Annex D Table D.1d / D.1e / D.1f Layer II
/// frequency column (PNG-only this round); the caller produces
/// the equivalent `line_subband` map from whatever source they
/// have. Slot `i` of `line_subband` is the §2.4.1.5 subband index
/// the spec's `f(i)` would land in (`0 ..= 31`); `usize::MAX` is
/// the documented "this FFT line is outside the audio band and
/// contributes to no subband" sentinel.
///
/// `ltg_db` carries the §D.1 Step 7 [`global_masking_threshold_db`]
/// output for each subsampled FFT line (the spec's `n` indexing,
/// 108 / 106 / 102 entries for Layer I and 132 / 130 / 126 for
/// Layer II per the §D.1 Step 6 table). It must be the same
/// length as `line_subband` — a length mismatch is a caller error
/// and returns an all-`None` result.
///
/// The returned `[Option<f64>; NUM_SUBBANDS]` slot `n` is the
/// minimum of `ltg_db[i]` across every `i` with `line_subband[i]
/// == n`, or `None` for subbands that received no FFT line (no
/// `LT_min` defined — the §D.1 Step 9 SMR computation must then
/// fall back to the absolute-threshold curve per §C.1.5.2.4
/// "subbands without masking lines").
#[must_use]
pub fn minimum_masking_threshold_subband(
    ltg_db: &[f64],
    line_subband: &[usize],
) -> [Option<f64>; NUM_SUBBANDS_LAYER2] {
    let mut out = [None; NUM_SUBBANDS_LAYER2];
    if ltg_db.len() != line_subband.len() {
        return out;
    }
    for (i, &sb) in line_subband.iter().enumerate() {
        if sb >= NUM_SUBBANDS_LAYER2 {
            // Sentinel (usize::MAX in particular) or invalid; skip.
            continue;
        }
        let value = ltg_db[i];
        // NaN-safe min via total_cmp on f64 (caller-supplied LTg
        // values are finite in practice but we guard the
        // primitive). For comparison purposes the sentinel must
        // never replace a finite minimum; reject NaN.
        if value.is_nan() {
            continue;
        }
        out[sb] = Some(match out[sb] {
            None => value,
            Some(prev) => prev.min(value),
        });
    }
    out
}

/// Layer II subband count (32 subbands per §2.4.1.5 / §2.4.3.2).
/// Re-exposed locally so the Step 8 / Step 9 primitives don't have
/// to take a `bitalloc::NUM_SUBBANDS` dependency for an
/// independent psychoacoustic-module use.
pub const NUM_SUBBANDS_LAYER2: usize = 32;

/// §D.1 Step 9 signal-to-mask ratio per subband. The verbatim
/// spec equation (PDF page 115, printed 115):
///
/// ```text
/// SMR_sb(n) = L_sb(n) - LT_min(n)   dB
/// ```
///
/// is computed for every subband. `l_sb_db` is the §D.1 Step 2
/// per-subband sound pressure level (`L_sb(n) = MAX[X(k),
/// 20·log10(scf_max(n)·32768) - 10]`) supplied by the caller;
/// `lt_min_db` is the [`minimum_masking_threshold_subband`]
/// output.
///
/// Subbands whose `lt_min_db` slot is `None` (no FFT line in
/// range — see [`minimum_masking_threshold_subband`]) return
/// `None` for that slot; the §C.1.5.2.4 fallback is the
/// caller's responsibility because it depends on the encoder's
/// chosen threshold-in-quiet substitute. The bit-allocator
/// (`encoder_bit_allocator::allocate_bits`) treats the missing
/// SMR slot as the most-conservative `-inf dB` (i.e. the slot
/// has no masking margin and is unlikely to receive bits unless
/// the encoder explicitly elects to spend them).
#[must_use]
pub fn signal_to_mask_ratio_subband(
    l_sb_db: &[f64; NUM_SUBBANDS_LAYER2],
    lt_min_db: &[Option<f64>; NUM_SUBBANDS_LAYER2],
) -> [Option<f64>; NUM_SUBBANDS_LAYER2] {
    let mut out = [None; NUM_SUBBANDS_LAYER2];
    for n in 0..NUM_SUBBANDS_LAYER2 {
        if let Some(lt_min) = lt_min_db[n] {
            out[n] = Some(l_sb_db[n] - lt_min);
        }
    }
    out
}

/// §D.1 Step 2 full-scale reference: the spec's `32 768` factor in
/// `20·log10(scf_max(n)·32768) - 10` (PDF page 116, printed 110).
/// The scalefactor is a `[-1, +1)`-domain multiplier (Annex B
/// Table 3-B.1); multiplying by `32 768 = 2^15` maps it onto the
/// 16-bit PCM full-scale axis the §D.1 Step 1 "normalization to
/// the reference level of 96 dB SPL" establishes.
pub const SPL_FULL_SCALE: f64 = 32768.0;

/// §D.1 Step 2 peak-to-RMS correction: the spec's `-10 dB` term in
/// `20·log10(scf_max(n)·32768) - 10` — per the spec prose, "The
/// '-10 dB' term corrects for the difference between peak and RMS
/// level" (PDF page 116, printed 110).
pub const SPL_PEAK_RMS_CORRECTION_DB: f64 = 10.0;

/// §D.1 Step 2 scalefactor operand of the sound-pressure-level
/// `MAX`. The verbatim spec term (PDF page 116, printed 110):
///
/// ```text
/// 20·log10( scf_max(n) · 32768 ) - 10   dB
/// ```
///
/// `scf_max` is, for Layer II, "the maximum of the three
/// scalefactors of subband n within a frame" — the Annex B
/// Table 3-B.1 *multiplier value* ([`crate::tables::SCALEFACTORS`]
/// entry), not the 6-bit index. The caller takes the maximum
/// multiplier (= the smallest of the three indices, Table 3-B.1
/// being monotonically decreasing) before calling.
///
/// Precondition: `scf_max > 0`. Every Table 3-B.1 entry is
/// strictly positive (entry 62 ≈ 1.2e-6 is the smallest), so the
/// logarithm is always defined for table-sourced inputs.
#[must_use]
pub fn scalefactor_spl_term_db(scf_max: f64) -> f64 {
    20.0 * (scf_max * SPL_FULL_SCALE).log10() - SPL_PEAK_RMS_CORRECTION_DB
}

/// Which §D.1 Step 2 estimator feeds the `X` operand of the
/// sound-pressure-level `MAX` in
/// [`sound_pressure_level_subband`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubbandSplMethod {
    /// The primary method (PDF page 116, printed 110): "X(k) is
    /// the sound pressure level of the spectral line with index k
    /// of the FFT with the maximum amplitude in the frequency
    /// range corresponding to subband n".
    MaxLine,
    /// The spec's documented alternative (PDF page 117, printed
    /// 111): `X_spl(n) = 10·log10( Σ_k 10^(X(k)/10) ) dB` over
    /// `k in subband n` — flagged by the spec as offering "a
    /// potential for better encoder performance" while noting the
    /// technique "has not been subjected to a formal audio
    /// quality test".
    PowerSum,
}

/// §D.1 Step 2 determination of the sound pressure level. The
/// verbatim spec equation (PDF page 116, printed 110):
///
/// ```text
/// L_sb(n) = MAX[ X(k), 20·log10(scf_max(n)·32768) - 10 ]   dB
///           X(k) in subband n
/// ```
///
/// computed for every subband `n`. `spl_db` is the §D.1 Step 1
/// power-density spectrum `X(k)` (96 dB-normalised, full FFT-line
/// axis `k = 0 ..= N/2`); `line_subband[k]` maps FFT line `k` to
/// its §2.4.1.5 subband (`0 ..= 31`; `usize::MAX` is the
/// documented "outside the audio band" sentinel, consistent with
/// [`minimum_masking_threshold_subband`]) — see
/// [`fft_line_to_subband_layer2`] for the closed-form Layer II
/// map. `scf_max[n]` is the per-subband maximum Table 3-B.1
/// multiplier (see [`scalefactor_spl_term_db`]). `method` selects
/// the primary max-line `X` operand or the spec's alternative
/// power-sum operand.
///
/// Every output slot is defined: the scalefactor operand of the
/// `MAX` always exists, so a subband that receives no FFT line
/// (its `X` operand is the empty maximum, `-inf dB`) degenerates
/// to the scalefactor term alone. `NaN` spectral lines are
/// dropped from the `X` operand. A `spl_db` / `line_subband`
/// length mismatch is a caller error; the documented safe
/// response treats the spectrum as empty and returns the
/// scalefactor terms alone.
#[must_use]
pub fn sound_pressure_level_subband(
    spl_db: &[f64],
    line_subband: &[usize],
    scf_max: &[f64; NUM_SUBBANDS_LAYER2],
    method: SubbandSplMethod,
) -> [f64; NUM_SUBBANDS_LAYER2] {
    // X-operand accumulator per subband: maximum line for
    // `MaxLine`, linear-power sum for `PowerSum`. Empty subbands
    // stay at the additive identity (`-inf` max / `0.0` sum), both
    // of which degenerate the final MAX to the scalefactor term.
    let mut max_line = [f64::NEG_INFINITY; NUM_SUBBANDS_LAYER2];
    let mut power_sum = [0.0_f64; NUM_SUBBANDS_LAYER2];
    if spl_db.len() == line_subband.len() {
        for (k, &sb) in line_subband.iter().enumerate() {
            if sb >= NUM_SUBBANDS_LAYER2 {
                // usize::MAX sentinel or invalid index: no subband.
                continue;
            }
            let x = spl_db[k];
            if x.is_nan() {
                continue;
            }
            max_line[sb] = max_line[sb].max(x);
            power_sum[sb] += 10.0_f64.powf(x / 10.0);
        }
    }
    let mut out = [0.0_f64; NUM_SUBBANDS_LAYER2];
    for n in 0..NUM_SUBBANDS_LAYER2 {
        let x_term = match method {
            SubbandSplMethod::MaxLine => max_line[n],
            SubbandSplMethod::PowerSum => {
                // 10·log10(0) = -inf reproduces the empty-subband
                // degenerate exactly.
                10.0 * power_sum[n].log10()
            }
        };
        out[n] = x_term.max(scalefactor_spl_term_db(scf_max[n]));
    }
    out
}

/// Closed-form Layer II FFT-line → subband map for the §D.1
/// Step 2 `line_subband` argument. Per the §D.1 Step 1 "Technical
/// data of the FFT" table (PDF page 116, printed 110) the Layer II
/// frequency resolution is `fs / 1024`, so FFT line `k` sits at
/// frequency `k·fs/1024`; the §2.4.3.2 filterbank splits
/// `[0, fs/2)` into 32 equal-width subbands of `fs/64` each, so
/// subband `n` spans `[n·fs/64, (n+1)·fs/64)` — i.e. FFT lines
/// `16·n ..= 16·n + 15`. Lines at or above the Nyquist index
/// (`k >= 512`, frequency `>= fs/2`) fall outside every subband
/// and map to the `usize::MAX` "outside the audio band" sentinel.
/// A line landing exactly on a subband boundary (`k = 16·n`)
/// belongs to the higher subband per the half-open spans above.
#[must_use]
pub fn fft_line_to_subband_layer2(k: usize) -> usize {
    if k < LAYER2_FFT_LEN / 2 {
        k / 16
    } else {
        usize::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables_d2::SamplingRate;

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
    fn masker_in_target_window_matches_vf_window_endpoints() {
        // The window in `z(j)` is `(z(i) - 8, z(i) + 3]`. Pick a
        // target `z(i) = 10` for arithmetic convenience.
        let z_i = 10.0;
        // Interior: dz = 0 (masker at target) is inside.
        assert!(masker_in_target_window(z_i, z_i));
        // Upper-closed endpoint: z(j) = z(i) + 3 -> dz = -3, included.
        assert!(masker_in_target_window(z_i + 3.0, z_i));
        // Lower-open endpoint: z(j) = z(i) - 8 -> dz = 8, excluded.
        assert!(!masker_in_target_window(z_i - 8.0, z_i));
        // Just inside the lower-open end: z(j) = z(i) - 8 + eps -> dz < 8.
        assert!(masker_in_target_window(z_i - 8.0 + 1.0e-6, z_i));
        // Just outside the upper-closed end: z(j) = z(i) + 3 + eps -> dz = -3 - eps.
        assert!(!masker_in_target_window(z_i + 3.0 + 1.0e-6, z_i));
    }

    #[test]
    fn masker_in_target_window_agrees_with_individual_masking_threshold_db() {
        // Predicate-level invariant: for every masker, the window
        // predicate returns `true` iff
        // `individual_masking_threshold_db` returns `Some(_)` (the
        // masker contributes a finite individual threshold) and
        // `false` iff it returns `None` (out of `vf` window).
        let z_i = 12.0;
        for z_j in [
            // Below the lower-open endpoint (excluded).
            z_i - 9.0,
            z_i - 8.5,
            z_i - 8.0,
            // Inside the window.
            z_i - 7.99,
            z_i - 5.0,
            z_i - 1.0,
            z_i,
            z_i + 1.0,
            z_i + 2.99,
            z_i + 3.0,
            // Above the upper-closed endpoint (excluded).
            z_i + 3.001,
            z_i + 5.0,
        ] {
            let masker = Masker {
                kind: MaskerKind::Tonal,
                z_bark: z_j,
                spl_db: 50.0,
            };
            let pred = masker_in_target_window(z_j, z_i);
            let lt = individual_masking_threshold_db(&masker, z_i);
            assert_eq!(
                pred,
                lt.is_some(),
                "z(j) = {z_j}, z(i) = {z_i}: pred = {pred}, LT = {lt:?}",
            );
        }
    }

    #[test]
    fn masker_in_target_window_rejects_nan() {
        // A `NaN` masker is treated as "outside the window" per the
        // doc-comment guarantee: `Range::contains` rejects `NaN`.
        assert!(!masker_in_target_window(f64::NAN, 10.0));
        // Symmetrically a `NaN` target line excludes any masker.
        assert!(!masker_in_target_window(10.0, f64::NAN));
    }

    #[test]
    fn relevant_maskers_for_target_line_drops_out_of_window_entries() {
        // Mix four maskers: two inside the window of z(i) = 10 (at
        // z(j) = 8 and 12, dz = 2 and -2), two outside (z(j) = 1 and
        // 20). The filter keeps the two inside ones, in input order.
        let inside_lo = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 8.0,
            spl_db: 40.0,
        };
        let outside_lo = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 1.0,
            spl_db: 80.0,
        };
        let inside_hi = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: 12.0,
            spl_db: 45.0,
        };
        let outside_hi = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: 20.0,
            spl_db: 90.0,
        };
        let input = [outside_lo, inside_lo, outside_hi, inside_hi];
        let kept = relevant_maskers_for_target_line(&input, 10.0);
        assert_eq!(kept.len(), 2);
        // Input-order preservation: inside_lo came before inside_hi
        // in the input vector and must come first in the output.
        assert_eq!(kept[0], inside_lo);
        assert_eq!(kept[1], inside_hi);
    }

    #[test]
    fn relevant_maskers_for_target_line_empty_when_nothing_in_window() {
        // All maskers far below the window of z(i) = 25.
        let m1 = Masker {
            kind: MaskerKind::Tonal,
            z_bark: 5.0,
            spl_db: 80.0,
        };
        let m2 = Masker {
            kind: MaskerKind::NonTonal,
            z_bark: 10.0,
            spl_db: 80.0,
        };
        assert!(relevant_maskers_for_target_line(&[m1, m2], 25.0).is_empty());
    }

    #[test]
    fn pre_filter_preserves_global_masking_threshold_db() {
        // Spec invariant: pre-filtering the masker list to the
        // `-8…+3` Bark window is purely a performance optimisation —
        // `global_masking_threshold_db` already drops out-of-window
        // entries via `masking_function_vf` returning `None`. So
        // `LTg(filtered)` must match `LTg(unfiltered)` bit-for-bit.
        let maskers = [
            Masker {
                kind: MaskerKind::Tonal,
                z_bark: 2.5,
                spl_db: 70.0,
            },
            Masker {
                kind: MaskerKind::NonTonal,
                z_bark: 9.0,
                spl_db: 55.0,
            },
            Masker {
                kind: MaskerKind::Tonal,
                z_bark: 10.0,
                spl_db: 80.0,
            },
            Masker {
                kind: MaskerKind::NonTonal,
                z_bark: 11.5,
                spl_db: 60.0,
            },
            Masker {
                kind: MaskerKind::Tonal,
                z_bark: 20.0,
                spl_db: 75.0,
            },
        ];
        // Sweep target Bark across a range that exercises in-window,
        // edge, and out-of-window mixes for the masker list above.
        for z_i in [2.0_f64, 6.0, 10.0, 12.0, 15.0, 18.0, 22.0] {
            let ltq = -5.0_f64;
            let full = global_masking_threshold_db(&maskers, z_i, ltq);
            let filtered = relevant_maskers_for_target_line(&maskers, z_i);
            let pruned = global_masking_threshold_db(&filtered, z_i, ltq);
            assert!(
                (full - pruned).abs() < 1.0e-12,
                "LTg mismatch at z(i) = {z_i}: full = {full}, filtered ({} of {}) = {pruned}",
                filtered.len(),
                maskers.len(),
            );
        }
    }

    #[test]
    fn pre_filter_idempotent_under_double_application() {
        // Filtering an already-filtered list at the same `z(i)` must
        // be a no-op: every surviving masker was already in the
        // window and the predicate is pointwise.
        let maskers = [
            Masker {
                kind: MaskerKind::Tonal,
                z_bark: 9.0,
                spl_db: 70.0,
            },
            Masker {
                kind: MaskerKind::NonTonal,
                z_bark: 12.0,
                spl_db: 55.0,
            },
            Masker {
                kind: MaskerKind::Tonal,
                z_bark: 20.0,
                spl_db: 80.0,
            },
        ];
        let z_i = 11.0;
        let once = relevant_maskers_for_target_line(&maskers, z_i);
        let twice = relevant_maskers_for_target_line(&once, z_i);
        assert_eq!(once, twice);
    }

    #[test]
    fn hann_window_layer2_endpoints() {
        // h(0) = sqrt(8/3) * 0.5 * (1 - cos(0)) = sqrt(8/3) * 0.5 * 0
        //      = 0 exactly.
        let w = hann_window_layer2();
        assert!(w[0].abs() < 1.0e-15, "h(0) = {} should be 0", w[0]);
        // h(N/2) = sqrt(8/3) * 0.5 * (1 - cos(pi))
        //        = sqrt(8/3) * 0.5 * 2
        //        = sqrt(8/3).
        let expected_mid = (8.0_f64 / 3.0).sqrt();
        let mid = w[LAYER2_FFT_LEN / 2];
        assert!(
            (mid - expected_mid).abs() < 1.0e-12,
            "h(N/2) = {mid}, expected {expected_mid}",
        );
    }

    #[test]
    fn hann_window_layer2_symmetry() {
        // The spec window h(i) = sqrt(8/3) * 0.5 * (1 - cos(2 pi i /
        // N)) is symmetric around i = N/2: h(N/2 - k) = h(N/2 + k)
        // for every k in [1, N/2 - 1]. Verify for a sampling of k.
        let w = hann_window_layer2();
        for k in [1_usize, 17, 64, 128, 256, 480] {
            let lo = w[LAYER2_FFT_LEN / 2 - k];
            let hi = w[LAYER2_FFT_LEN / 2 + k];
            assert!(
                (lo - hi).abs() < 1.0e-12,
                "asymmetry at k = {k}: h({}) = {lo}, h({}) = {hi}",
                LAYER2_FFT_LEN / 2 - k,
                LAYER2_FFT_LEN / 2 + k,
            );
        }
    }

    #[test]
    fn hann_window_layer2_bounded_in_zero_to_sqrt8over3() {
        // h(i) = sqrt(8/3) * 0.5 * (1 - cos(theta)) ranges over
        // [0, sqrt(8/3)] since (1 - cos) ranges over [0, 2].
        let w = hann_window_layer2();
        let upper = (8.0_f64 / 3.0).sqrt();
        for (i, &h) in w.iter().enumerate() {
            assert!(
                h >= 0.0 && h <= upper + 1.0e-12,
                "h({i}) = {h} outside [0, sqrt(8/3) ≈ {upper}]",
            );
        }
    }

    #[test]
    fn is_local_maximum_basic_peak() {
        // Simple peak at index 2.
        let x = [0.0, 5.0, 10.0, 5.0, 0.0];
        assert!(is_local_maximum(&x, 2));
        // Index 1 is not (X(1) = 5, X(2) = 10 ⇒ X(1) < X(2)).
        assert!(!is_local_maximum(&x, 1));
        // Index 3 is not.
        assert!(!is_local_maximum(&x, 3));
    }

    #[test]
    fn is_local_maximum_left_strict_right_non_strict() {
        // Spec rule: X(k) > X(k-1) AND X(k) >= X(k+1).
        // A two-bin equal plateau: index 2 IS a maximum (>= on
        // the right), index 3 is NOT (left side is equal, not <).
        let x = [0.0, 5.0, 10.0, 10.0, 5.0];
        assert!(is_local_maximum(&x, 2));
        assert!(!is_local_maximum(&x, 3));
    }

    #[test]
    fn is_local_maximum_edge_indices_are_false() {
        // The spec leaves the spectrum edges undefined for
        // local-maximum labelling — there is no X(-1) or X(N).
        let x = [10.0, 5.0, 0.0];
        assert!(!is_local_maximum(&x, 0));
        assert!(!is_local_maximum(&x, 2));
        // Empty slice as a degenerate edge case.
        let empty: [f64; 0] = [];
        assert!(!is_local_maximum(&empty, 0));
    }

    #[test]
    fn tonal_neighbourhood_layer2_spec_rows() {
        // Spec rows for Layer II (1024-point FFT):
        //   2 < k <  63           j = -2, +2
        //  63 <= k < 127           j = -3, -2, +2, +3
        // 127 <= k < 255           j = -6 ... -2, +2 ... +6
        // 255 <= k <= 500          j = -12 ... -2, +2 ... +12
        // Sample one k in each row's interior and one at each
        // boundary to confirm the dispatch.
        assert_eq!(tonal_neighbourhood_layer2(2), None); // 2 NOT in (2, 63)
        assert_eq!(tonal_neighbourhood_layer2(3).unwrap().len(), 2);
        assert_eq!(tonal_neighbourhood_layer2(62).unwrap().len(), 2);
        assert_eq!(tonal_neighbourhood_layer2(63).unwrap().len(), 4);
        assert_eq!(tonal_neighbourhood_layer2(126).unwrap().len(), 4);
        assert_eq!(tonal_neighbourhood_layer2(127).unwrap().len(), 10);
        assert_eq!(tonal_neighbourhood_layer2(254).unwrap().len(), 10);
        assert_eq!(tonal_neighbourhood_layer2(255).unwrap().len(), 22);
        assert_eq!(tonal_neighbourhood_layer2(500).unwrap().len(), 22);
        assert_eq!(tonal_neighbourhood_layer2(501), None);
        // The j = 0 offset is never present (the X(k) bin itself
        // is not tested against itself).
        for k in [3_usize, 63, 127, 255, 500] {
            for &j in tonal_neighbourhood_layer2(k).unwrap() {
                assert_ne!(j, 0, "k = {k}: neighbourhood includes j = 0");
            }
        }
    }

    #[test]
    fn tonal_neighbourhood_layer2_symmetric() {
        // Every row's neighbourhood is symmetric around 0
        // (excluding 0 itself).
        for k in [3_usize, 63, 127, 255, 500] {
            let nb = tonal_neighbourhood_layer2(k).unwrap();
            for &j in nb {
                assert!(nb.contains(&-j), "k = {k}: j = {j} present but -{j} absent",);
            }
        }
    }

    #[test]
    fn is_tonal_layer2_clear_peak_above_threshold() {
        // Build a spectrum where bin k = 10 is 50 dB and every
        // other bin is 0 dB. Then X(k) - X(k+j) = 50 dB > 7 dB
        // for every neighbour, so the line is tonal.
        let mut x = vec![0.0_f64; LAYER2_FFT_BINS];
        x[10] = 50.0;
        assert!(is_local_maximum(&x, 10));
        assert!(is_tonal_layer2(&x, 10));
    }

    #[test]
    fn is_tonal_layer2_below_threshold_rejected() {
        // Build a spectrum where the central bin only just
        // exceeds its neighbours — below the 7 dB tonality
        // threshold. The peak is a local maximum but not tonal.
        let mut x = vec![0.0_f64; LAYER2_FFT_BINS];
        x[10] = 6.5; // central
        x[8] = 0.5; // X(k) - X(k+j=-2) = 6.0 dB < 7.0 dB
        x[12] = 0.5; // X(k) - X(k+j=+2) = 6.0 dB < 7.0 dB
        x[9] = 0.0;
        x[11] = 0.0;
        assert!(is_local_maximum(&x, 10));
        assert!(!is_tonal_layer2(&x, 10));
    }

    #[test]
    fn is_tonal_layer2_one_neighbour_below_threshold_rejected() {
        // The spec requires X(k) - X(k+j) >= 7 dB for EVERY j in
        // the neighbourhood. A single failing neighbour disqualifies
        // the line. Verify with k = 100 (row j = -3, -2, +2, +3):
        // make X(100) = 50, every j neighbour 0 dB EXCEPT j = +3
        // which is 45 dB (50 - 45 = 5 < 7).
        let mut x = vec![0.0_f64; LAYER2_FFT_BINS];
        x[100] = 50.0;
        x[103] = 45.0; // j = +3 boundary breaker
                       // Local-maximum precondition: X(100) > X(99) AND >= X(101).
                       // x[99] = x[101] = 0.0 (default), so it holds.
        assert!(is_local_maximum(&x, 100));
        // Tonality fails because of j = +3.
        assert!(!is_tonal_layer2(&x, 100));
    }

    #[test]
    fn is_tonal_layer2_not_a_local_maximum_rejected() {
        // The spec dispatches tonality only after Step 4(a) labels
        // local maxima. A non-local-maximum bin must not be flagged
        // as tonal even if the surrounding bins are quiet. Verify
        // by making X(10) = X(11) = 50 — neither is a local maximum
        // under the spec's `X(k) > X(k-1)` rule for both, and only
        // the strict-left one passes (`>= X(k+1)` is `50 >= 50`).
        let mut x = vec![0.0_f64; LAYER2_FFT_BINS];
        x[10] = 50.0;
        x[11] = 50.0;
        // X(10): X(9) = 0, X(11) = 50 ⇒ 50 > 0 AND 50 >= 50 ⇒ maximum.
        assert!(is_local_maximum(&x, 10));
        // X(11): X(10) = 50, X(12) = 0 ⇒ 50 > 50 is FALSE ⇒ not max.
        assert!(!is_local_maximum(&x, 11));
        assert!(!is_tonal_layer2(&x, 11));
    }

    #[test]
    fn is_tonal_layer2_outside_window_rejected() {
        // k <= 2 and k > 500 are outside the tonality window.
        let x = vec![100.0_f64; LAYER2_FFT_BINS];
        assert!(!is_tonal_layer2(&x, 0));
        assert!(!is_tonal_layer2(&x, 2));
        assert!(!is_tonal_layer2(&x, 501));
    }

    #[test]
    fn tonal_spl_db_three_line_power_sum() {
        // Pin the formula: X_tm = 10 * log10(10^(X(k-1)/10) +
        // 10^(X(k)/10) + 10^(X(k+1)/10)).
        // Use X(k-1) = X(k) = X(k+1) = 60 dB: three equal powers
        // sum to 60 + 10*log10(3) ≈ 60 + 4.7712 = 64.7712 dB.
        let x = [60.0_f64, 60.0, 60.0];
        let got = tonal_spl_db(&x, 1).unwrap();
        let expected = 60.0 + 10.0 * 3.0_f64.log10();
        assert!(
            (got - expected).abs() < 1.0e-12,
            "X_tm = {got}, expected {expected}",
        );
    }

    #[test]
    fn tonal_spl_db_dominated_by_centre() {
        // Centre 80 dB dominates a pair of 0 dB shoulders:
        // 10*log10(10^0 + 10^8 + 10^0) ≈ 10*log10(10^8 + 2)
        // ≈ 80.0000000868… ≈ 80 dB.
        let x = [0.0_f64, 80.0, 0.0];
        let got = tonal_spl_db(&x, 1).unwrap();
        assert!((got - 80.0).abs() < 1.0e-4, "X_tm = {got}, expected ≈ 80",);
    }

    #[test]
    fn tonal_spl_db_edge_returns_none() {
        let x = [60.0_f64, 60.0, 60.0];
        assert!(tonal_spl_db(&x, 0).is_none());
        assert!(tonal_spl_db(&x, 2).is_none());
        let empty: [f64; 0] = [];
        assert!(tonal_spl_db(&empty, 0).is_none());
    }

    #[test]
    fn tonal_spl_db_ge_centre_value() {
        // Power addition is monotone: the three-line sum is always
        // at least as large as the centre bin alone (and strictly
        // larger when either shoulder is finite).
        let cases = [
            [-100.0_f64, 80.0, -100.0],
            [50.0, 50.0, 50.0],
            [0.0, 60.0, 0.0],
            [40.0, 60.0, 30.0],
        ];
        for x in cases {
            let got = tonal_spl_db(&x, 1).unwrap();
            assert!(
                got >= x[1],
                "X_tm {got} should be >= X(k) {} for {x:?}",
                x[1],
            );
        }
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

    #[test]
    fn zero_tonal_neighbourhood_layer2_zeroes_centre_and_neighbours() {
        // Pick a centre line in the {-2, +2} neighbourhood row.
        // The spec sentence: "all spectral lines within the
        // examined frequency range are set to −∞ dB".
        let mut spectrum = vec![10.0_f64; 64];
        zero_tonal_neighbourhood_layer2(&mut spectrum, 30);
        // Centre.
        assert!(spectrum[30].is_infinite() && spectrum[30].is_sign_negative());
        // {-2, +2} neighbourhood (j = -2, +2 for 2 < k < 63).
        assert!(spectrum[28].is_infinite() && spectrum[28].is_sign_negative());
        assert!(spectrum[32].is_infinite() && spectrum[32].is_sign_negative());
        // Lines outside the neighbourhood are untouched.
        assert_eq!(spectrum[27], 10.0);
        assert_eq!(spectrum[29], 10.0);
        assert_eq!(spectrum[31], 10.0);
        assert_eq!(spectrum[33], 10.0);
    }

    #[test]
    fn zero_tonal_neighbourhood_layer2_wider_row() {
        // Step into the {-12, ..., -2, +2, ..., +12} row at k = 300.
        // Verify the inner symmetric pair {-2, +2} and the outer
        // pair {-12, +12} are both zeroed, and lines just outside
        // (j = ±13) are not.
        let mut spectrum = vec![5.0_f64; 513];
        zero_tonal_neighbourhood_layer2(&mut spectrum, 300);
        assert!(spectrum[300].is_infinite());
        assert!(spectrum[298].is_infinite()); // j = -2
        assert!(spectrum[302].is_infinite()); // j = +2
        assert!(spectrum[288].is_infinite()); // j = -12
        assert!(spectrum[312].is_infinite()); // j = +12
        assert_eq!(spectrum[287], 5.0); // j = -13 out of neighbourhood
        assert_eq!(spectrum[313], 5.0); // j = +13 out of neighbourhood
    }

    #[test]
    fn zero_tonal_neighbourhood_layer2_skips_out_of_range_k() {
        // k = 1 has no tonality neighbourhood (the spec leaves
        // it undefined for k <= 2) — the operation must be a no-op.
        let mut spectrum = vec![7.0_f64; 16];
        zero_tonal_neighbourhood_layer2(&mut spectrum, 1);
        for (i, &x) in spectrum.iter().enumerate() {
            assert_eq!(x, 7.0, "spectrum[{i}] modified by no-op call");
        }
        // k = 501 likewise.
        let mut spectrum2 = vec![7.0_f64; 600];
        zero_tonal_neighbourhood_layer2(&mut spectrum2, 501);
        for (i, &x) in spectrum2.iter().enumerate() {
            assert_eq!(x, 7.0, "spectrum2[{i}] modified by no-op call");
        }
    }

    #[test]
    fn non_tonal_spl_db_three_equal_lines_sums_in_power_domain() {
        // Three 60 dB lines power-sum to 60 + 10·log10(3) ≈ 64.7712 dB.
        let spectrum = [60.0_f64; 8];
        let got = non_tonal_spl_db(&spectrum, 2, 4).expect("non-empty band");
        let expected = 60.0 + 10.0 * 3.0_f64.log10();
        assert!(
            (got - expected).abs() < 1.0e-9,
            "X_nm = {got}, expected {expected}",
        );
    }

    #[test]
    fn non_tonal_spl_db_ignores_neg_inf_zeroed_lines() {
        // After Step 4(b) zeroing, lines marked -inf must drop out
        // of the Step 4(c) power sum exactly (10^(-inf/10) = 0).
        let spectrum = [
            f64::NEG_INFINITY,
            60.0,
            f64::NEG_INFINITY,
            60.0,
            f64::NEG_INFINITY,
        ];
        let got = non_tonal_spl_db(&spectrum, 0, 4).expect("two finite lines");
        let expected = 60.0 + 10.0 * 2.0_f64.log10();
        assert!(
            (got - expected).abs() < 1.0e-9,
            "X_nm = {got}, expected {expected} (two finite lines @ 60 dB)",
        );
    }

    #[test]
    fn non_tonal_spl_db_all_neg_inf_returns_none() {
        // A fully-zeroed band carries no non-tonal energy; the
        // primitive returns None so the caller can drop the band
        // rather than carry a -inf-dB masker.
        let spectrum = [f64::NEG_INFINITY; 10];
        assert_eq!(non_tonal_spl_db(&spectrum, 1, 5), None);
    }

    #[test]
    fn non_tonal_spl_db_rejects_empty_band() {
        let spectrum = [40.0_f64; 10];
        // lo > hi.
        assert_eq!(non_tonal_spl_db(&spectrum, 5, 4), None);
        // lo past the spectrum end.
        assert_eq!(non_tonal_spl_db(&spectrum, 11, 12), None);
    }

    #[test]
    fn non_tonal_spl_db_dominant_line_anchors_sum() {
        // One 100 dB line with shoulders 60 dB lower contributes
        // ~all of the band power — the result is within tens of mdB
        // of the dominant line.
        let spectrum = [40.0_f64, 40.0, 100.0, 40.0, 40.0];
        let got = non_tonal_spl_db(&spectrum, 0, 4).expect("non-empty band");
        assert!(
            (got - 100.0).abs() < 0.001,
            "dominant 100 dB line yielded X_nm = {got}",
        );
    }

    #[test]
    fn non_tonal_band_index_geometric_mean_simple() {
        // [4, 16]: geometric mean sqrt(64) = 8 exactly.
        assert_eq!(non_tonal_band_index(4, 16), Some(8));
        // [1, 9]: sqrt(9) = 3.
        assert_eq!(non_tonal_band_index(1, 9), Some(3));
        // [9, 25]: sqrt(225) = 15.
        assert_eq!(non_tonal_band_index(9, 25), Some(15));
    }

    #[test]
    fn non_tonal_band_index_singleton_band() {
        // Single-line band returns its single index regardless of
        // value.
        assert_eq!(non_tonal_band_index(7, 7), Some(7));
    }

    #[test]
    fn non_tonal_band_index_dc_excluded_from_geomean() {
        // [0, 10]: naïve sqrt(0) = 0, but the spec's "geometric
        // mean" only makes sense over [1, hi]. The primitive
        // substitutes lo = 1, giving sqrt(10) ≈ 3.162 — closest
        // integer = 3.
        assert_eq!(non_tonal_band_index(0, 10), Some(3));
    }

    #[test]
    fn non_tonal_band_index_picks_nearest_integer() {
        // [2, 5]: sqrt(10) ≈ 3.162 — closest int = 3.
        assert_eq!(non_tonal_band_index(2, 5), Some(3));
        // [3, 8]: sqrt(24) ≈ 4.899 — closest int = 5.
        assert_eq!(non_tonal_band_index(3, 8), Some(5));
    }

    #[test]
    fn non_tonal_band_index_empty_band_returns_none() {
        assert_eq!(non_tonal_band_index(5, 4), None);
    }

    #[test]
    fn list_non_tonal_layer2_returns_one_masker_per_band_on_flat_spectrum() {
        // Drive a flat 30 dB spectrum through the Layer II 32 kHz
        // (25-band) sweep. Every band carries finite content so the
        // output has 25 maskers, all NonTonal.
        let spectrum = vec![30.0_f64; LAYER2_FFT_BINS];
        let maskers = list_non_tonal_layer2(&spectrum, SamplingRate::Fs32kHz);
        assert_eq!(maskers.len(), 25);
        for m in &maskers {
            assert_eq!(m.kind, MaskerKind::NonTonal);
        }
    }

    #[test]
    fn list_non_tonal_layer2_skips_fully_zeroed_bands() {
        // Zero out every line: no non-tonal maskers.
        let spectrum = vec![f64::NEG_INFINITY; LAYER2_FFT_BINS];
        let maskers = list_non_tonal_layer2(&spectrum, SamplingRate::Fs44k1Hz);
        assert!(
            maskers.is_empty(),
            "fully-zeroed spectrum produced {} maskers",
            maskers.len(),
        );
    }

    #[test]
    fn list_non_tonal_layer2_bark_matches_d2_table() {
        // The masker's z_bark must come straight from the Annex D
        // Table D.2 boundary's top-line Bark column.
        let spectrum = vec![20.0_f64; LAYER2_FFT_BINS];
        let maskers = list_non_tonal_layer2(&spectrum, SamplingRate::Fs48kHz);
        let table = SamplingRate::Fs48kHz.critical_band_boundaries();
        assert_eq!(maskers.len(), table.len());
        for (m, boundary) in maskers.iter().zip(table.iter()) {
            assert!(
                (m.z_bark - boundary.top_bark).abs() < 1.0e-12,
                "masker z_bark {} mismatches boundary top_bark {}",
                m.z_bark,
                boundary.top_bark,
            );
        }
    }

    #[test]
    fn list_non_tonal_layer2_two_equal_bands_sum_equally() {
        // Drive a flat 40 dB spectrum at 32 kHz; the per-band power
        // of band b is 40 + 10·log10(width_b). Verify that two
        // bands with identical width get identical X_nm.
        let spectrum = vec![40.0_f64; LAYER2_FFT_BINS];
        let maskers = list_non_tonal_layer2(&spectrum, SamplingRate::Fs32kHz);
        let table = SamplingRate::Fs32kHz.critical_band_boundaries();
        // Find two equal-width bands; if any exist, their X_nm must
        // match exactly.
        let mut widths = Vec::with_capacity(table.len());
        let mut prev_top: i64 = -1;
        for boundary in table {
            let top = boundary.top_line_index as i64;
            widths.push(top - prev_top);
            prev_top = top;
        }
        for i in 0..widths.len() {
            for j in (i + 1)..widths.len() {
                if widths[i] == widths[j] {
                    assert!(
                        (maskers[i].spl_db - maskers[j].spl_db).abs() < 1.0e-9,
                        "bands {i} ({} wide) and {j} ({} wide) — equal width but X_nm differs: {} vs {}",
                        widths[i],
                        widths[j],
                        maskers[i].spl_db,
                        maskers[j].spl_db,
                    );
                    return;
                }
            }
        }
        // If no equal-width pair exists in the table the test
        // assertion is vacuous — still pass.
    }

    #[test]
    fn list_non_tonal_layer2_one_loud_line_dominates_its_band() {
        // Build a spectrum with one 100 dB line at FFT index 30,
        // everything else 0 dB. The Layer II 32 kHz band containing
        // index 30 has top_line_index = 30 (cf. D.2d row 8). That
        // band's X_nm must round to ~100 dB.
        let mut spectrum = vec![0.0_f64; LAYER2_FFT_BINS];
        spectrum[30] = 100.0;
        let maskers = list_non_tonal_layer2(&spectrum, SamplingRate::Fs32kHz);
        // Find the band whose top_line_index >= 30 and whose
        // previous top_line_index < 30 — that's the band containing
        // index 30.
        let table = SamplingRate::Fs32kHz.critical_band_boundaries();
        let mut hit_band = None;
        let mut prev_top: i64 = -1;
        for (i, boundary) in table.iter().enumerate() {
            let top = boundary.top_line_index as i64;
            if prev_top < 30 && top >= 30 {
                hit_band = Some(i);
                break;
            }
            prev_top = top;
        }
        let i = hit_band.expect("index 30 must land in some Layer II 32 kHz band");
        assert!(
            (maskers[i].spl_db - 100.0).abs() < 0.001,
            "band {i} X_nm = {} dB, expected ~100 dB",
            maskers[i].spl_db,
        );
    }

    // --- §D.1 Step 5(b) tonal-masker decimation -----------------

    fn tonal(z: f64, spl: f64) -> Masker {
        Masker {
            kind: MaskerKind::Tonal,
            z_bark: z,
            spl_db: spl,
        }
    }

    fn non_tonal(z: f64, spl: f64) -> Masker {
        Masker {
            kind: MaskerKind::NonTonal,
            z_bark: z,
            spl_db: spl,
        }
    }

    #[test]
    fn tonal_decimation_window_is_half_a_bark() {
        // Verbatim spec constant — pin the window width at 0.5 Bark.
        assert_eq!(TONAL_DECIMATION_WINDOW_BARK, 0.5);
    }

    #[test]
    fn decimate_tonal_maskers_keeps_loudest_in_window() {
        // Three tonal maskers within 0.5 Bark of each other:
        //   z=5.00 spl=60   z=5.10 spl=80   z=5.30 spl=70
        // All pairs strictly within 0.5 Bark → all collapse into one
        // survivor: the 80 dB peak at z=5.10.
        let input = vec![tonal(5.00, 60.0), tonal(5.10, 80.0), tonal(5.30, 70.0)];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MaskerKind::Tonal);
        assert!((out[0].z_bark - 5.10).abs() < 1.0e-12);
        assert!((out[0].spl_db - 80.0).abs() < 1.0e-12);
    }

    #[test]
    fn decimate_tonal_maskers_keeps_distant_maskers_separate() {
        // Two tonal maskers at z=5.00 and z=5.50 (exactly 0.5 Bark
        // apart). The spec's "less than 0.5 Bark" half-open window
        // means a pair at exactly 0.5 Bark is NOT merged.
        let input = vec![tonal(5.00, 60.0), tonal(5.50, 70.0)];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 2);
        // Sorted by Bark on output.
        assert!((out[0].z_bark - 5.00).abs() < 1.0e-12);
        assert!((out[1].z_bark - 5.50).abs() < 1.0e-12);
    }

    #[test]
    fn decimate_tonal_maskers_leaves_non_tonal_untouched() {
        // Two non-tonal maskers at 0.1 Bark apart MUST NOT merge —
        // the spec procedure is scoped to tonal components only.
        let input = vec![non_tonal(5.00, 60.0), non_tonal(5.10, 70.0)];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, MaskerKind::NonTonal);
        assert_eq!(out[1].kind, MaskerKind::NonTonal);
    }

    #[test]
    fn decimate_tonal_maskers_first_wins_on_equal_power() {
        // Tie-break: the first-encountered tonal masker at equal SPL
        // wins. After the internal sort by Bark this is the lower
        // z_bark entry.
        let input = vec![tonal(5.00, 70.0), tonal(5.10, 70.0), tonal(5.20, 70.0)];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 1);
        // Lowest Bark wins on ties.
        assert!((out[0].z_bark - 5.00).abs() < 1.0e-12);
    }

    #[test]
    fn decimate_tonal_maskers_handles_mixed_classes() {
        // Mix tonal and non-tonal maskers. Non-tonal preserved in
        // input order; tonal decimated then appended in Bark order.
        let input = vec![
            non_tonal(2.0, 50.0),
            tonal(5.00, 60.0),
            non_tonal(10.0, 55.0),
            tonal(5.10, 80.0), // wins over 5.00 (within 0.5 Bark)
            tonal(8.00, 65.0), // separate cluster
        ];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 4);
        // Non-tonal first, in input order.
        assert_eq!(out[0].kind, MaskerKind::NonTonal);
        assert!((out[0].z_bark - 2.0).abs() < 1.0e-12);
        assert_eq!(out[1].kind, MaskerKind::NonTonal);
        assert!((out[1].z_bark - 10.0).abs() < 1.0e-12);
        // Surviving tonal, in Bark order.
        assert_eq!(out[2].kind, MaskerKind::Tonal);
        assert!((out[2].z_bark - 5.10).abs() < 1.0e-12);
        assert!((out[2].spl_db - 80.0).abs() < 1.0e-12);
        assert_eq!(out[3].kind, MaskerKind::Tonal);
        assert!((out[3].z_bark - 8.00).abs() < 1.0e-12);
    }

    #[test]
    fn decimate_tonal_maskers_empty_in_empty_out() {
        let out = decimate_tonal_maskers(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn decimate_tonal_maskers_singleton_passes_through() {
        // A single tonal masker has no neighbours to decimate
        // against — it must come out unchanged.
        let input = vec![tonal(12.0, 65.0)];
        let out = decimate_tonal_maskers(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], input[0]);
    }

    #[test]
    fn decimate_tonal_maskers_chained_clusters_dont_merge() {
        // Three tonal maskers at z = 5.0, 5.4, 5.8. The pairs
        // (5.0, 5.4) and (5.4, 5.8) are each within 0.5 Bark, but
        // the pair (5.0, 5.8) is 0.8 Bark apart. The sliding-window
        // procedure must therefore NOT collapse all three into one
        // — that would violate the spec's "every pair in the window
        // is within 0.5 Bark" reading. Our implementation anchors
        // each run on its first entry, so the run starts at 5.0,
        // accepts 5.4 (0.4 Bark from 5.0), but rejects 5.8 (0.8 Bark
        // from 5.0). 5.8 starts a new run.
        let input = vec![tonal(5.0, 60.0), tonal(5.4, 80.0), tonal(5.8, 70.0)];
        let out = decimate_tonal_maskers(&input);
        // 5.4 wins the first run; 5.8 stands alone.
        assert_eq!(out.len(), 2);
        assert!((out[0].z_bark - 5.4).abs() < 1.0e-12);
        assert!((out[0].spl_db - 80.0).abs() < 1.0e-12);
        assert!((out[1].z_bark - 5.8).abs() < 1.0e-12);
        assert!((out[1].spl_db - 70.0).abs() < 1.0e-12);
    }

    #[test]
    fn decimate_tonal_maskers_unsorted_input_still_decimates() {
        // The function is documented to sort internally. Input
        // order shouldn't change the decimation result.
        let asc = vec![tonal(5.00, 60.0), tonal(5.10, 80.0), tonal(5.30, 70.0)];
        let desc = vec![tonal(5.30, 70.0), tonal(5.10, 80.0), tonal(5.00, 60.0)];
        let permuted = vec![tonal(5.10, 80.0), tonal(5.30, 70.0), tonal(5.00, 60.0)];
        let out_asc = decimate_tonal_maskers(&asc);
        let out_desc = decimate_tonal_maskers(&desc);
        let out_permuted = decimate_tonal_maskers(&permuted);
        assert_eq!(out_asc, out_desc);
        assert_eq!(out_asc, out_permuted);
    }

    #[test]
    fn decimate_tonal_maskers_idempotent() {
        // Decimating twice should produce the same result as once
        // — the survivors are by construction at least 0.5 Bark
        // apart (in their cluster's anchor), so the second pass
        // finds no neighbours to merge.
        let input = vec![
            tonal(2.0, 50.0),
            tonal(5.00, 60.0),
            tonal(5.10, 80.0),
            tonal(5.30, 70.0),
            tonal(8.00, 65.0),
            tonal(12.0, 75.0),
        ];
        let once = decimate_tonal_maskers(&input);
        let twice = decimate_tonal_maskers(&once);
        assert_eq!(once, twice);
    }

    // --- §D.1 Step 8 minimum masking threshold per subband ------

    #[test]
    fn step8_layer_ii_subband_count_matches_spec() {
        // §2.4.1.5 / §D.1 Step 8: the Layer II subband count is 32.
        assert_eq!(NUM_SUBBANDS_LAYER2, 32);
    }

    #[test]
    fn step8_returns_min_per_subband() {
        // Two FFT lines in subband 0 at LTg = 30 and 25 dB; one in
        // subband 1 at LTg = 45 dB; one in subband 31 at LTg = 55 dB;
        // every other subband empty.
        let ltg = vec![30.0_f64, 25.0, 45.0, 55.0];
        let map = vec![0_usize, 0, 1, 31];
        let out = minimum_masking_threshold_subband(&ltg, &map);
        assert_eq!(out[0], Some(25.0));
        assert_eq!(out[1], Some(45.0));
        for (n, slot) in out.iter().enumerate().take(31).skip(2) {
            assert_eq!(*slot, None, "subband {n} expected empty");
        }
        assert_eq!(out[31], Some(55.0));
    }

    #[test]
    fn step8_oob_subband_indices_are_ignored() {
        // usize::MAX (the sentinel for "outside audio band") and any
        // index >= 32 must not crash and must not appear in the
        // output. Documented behaviour for both.
        let ltg = vec![10.0_f64, 99.0, -5.0];
        let map = vec![0_usize, usize::MAX, 32];
        let out = minimum_masking_threshold_subband(&ltg, &map);
        assert_eq!(out[0], Some(10.0));
        for (n, slot) in out.iter().enumerate().skip(1) {
            assert_eq!(*slot, None, "subband {n} should have been ignored");
        }
    }

    #[test]
    fn step8_length_mismatch_returns_all_none() {
        // Caller error: ltg.len() != map.len(). Documented safe
        // return: every slot None.
        let ltg = vec![30.0_f64, 25.0, 45.0];
        let map = vec![0_usize, 1];
        let out = minimum_masking_threshold_subband(&ltg, &map);
        assert!(out.iter().all(|s| s.is_none()));
    }

    #[test]
    fn step8_empty_input_returns_all_none() {
        let out = minimum_masking_threshold_subband(&[], &[]);
        assert!(out.iter().all(|s| s.is_none()));
    }

    #[test]
    fn step8_nan_values_are_dropped() {
        // A NaN LTg value (caller error / signalling sentinel) is
        // dropped from the minimum reduction so the remaining
        // finite values still produce a well-defined LT_min.
        let ltg = vec![20.0_f64, f64::NAN, 15.0];
        let map = vec![0_usize, 0, 0];
        let out = minimum_masking_threshold_subband(&ltg, &map);
        assert_eq!(out[0], Some(15.0));
    }

    #[test]
    fn step8_picks_running_min_with_many_lines() {
        // 10 FFT lines all in subband 5 with descending LTg values;
        // the running minimum must be the last one (the smallest).
        let ltg: Vec<f64> = (0..10).map(|i| 100.0 - i as f64 * 7.0).collect();
        let map = vec![5_usize; 10];
        let out = minimum_masking_threshold_subband(&ltg, &map);
        // 100 - 9*7 = 37.
        assert_eq!(out[5], Some(37.0));
    }

    #[test]
    fn step8_single_line_per_subband_propagates_through() {
        // Trivial bijection: 32 FFT lines, one per subband, LTg(i)
        // = i. The minimum per subband is just LTg(i) itself.
        let ltg: Vec<f64> = (0..NUM_SUBBANDS_LAYER2).map(|i| i as f64 * 2.0).collect();
        let map: Vec<usize> = (0..NUM_SUBBANDS_LAYER2).collect();
        let out = minimum_masking_threshold_subband(&ltg, &map);
        for (n, slot) in out.iter().enumerate() {
            assert_eq!(*slot, Some(n as f64 * 2.0));
        }
    }

    // --- §D.1 Step 9 signal-to-mask ratio per subband -----------

    #[test]
    fn step9_smr_is_l_sb_minus_lt_min() {
        // Pin the verbatim spec equation across a few subbands.
        let mut l_sb = [0.0_f64; NUM_SUBBANDS_LAYER2];
        let mut lt_min = [None; NUM_SUBBANDS_LAYER2];
        l_sb[0] = 80.0;
        lt_min[0] = Some(30.0);
        l_sb[5] = 60.0;
        lt_min[5] = Some(45.0);
        l_sb[31] = 90.0;
        lt_min[31] = Some(40.0);
        let out = signal_to_mask_ratio_subband(&l_sb, &lt_min);
        assert_eq!(out[0], Some(50.0));
        assert_eq!(out[5], Some(15.0));
        assert_eq!(out[31], Some(50.0));
    }

    #[test]
    fn step9_propagates_none_lt_min_to_none_smr() {
        // §D.1 Step 9 is undefined where LT_min isn't (subbands with
        // no FFT line in range). The primitive emits None for those
        // slots so the caller's §C.1.5.2.4 fallback can substitute.
        let l_sb = [50.0_f64; NUM_SUBBANDS_LAYER2];
        let lt_min = [None; NUM_SUBBANDS_LAYER2];
        let out = signal_to_mask_ratio_subband(&l_sb, &lt_min);
        assert!(out.iter().all(|s| s.is_none()));
    }

    #[test]
    fn step9_negative_smr_passes_through() {
        // Below-threshold subband: SMR may be negative. The primitive
        // doesn't clamp.
        let mut l_sb = [0.0_f64; NUM_SUBBANDS_LAYER2];
        let mut lt_min = [None; NUM_SUBBANDS_LAYER2];
        l_sb[0] = 20.0;
        lt_min[0] = Some(50.0);
        let out = signal_to_mask_ratio_subband(&l_sb, &lt_min);
        assert_eq!(out[0], Some(-30.0));
    }

    // --- §D.1 Step 4(b) tonal listing sweep --------------------

    /// Build a synthetic SPL spectrum that's all `floor_db` except for
    /// the requested local-maximum line indices. Each peak is `peak_db`
    /// at index `k`, with the neighbours `k ± 1` left at `floor_db`
    /// so the strict `>` / non-strict `>=` local-maximum rule passes.
    fn synthetic_spectrum(len: usize, floor_db: f64, peaks: &[usize], peak_db: f64) -> Vec<f64> {
        let mut spec = vec![floor_db; len];
        for &k in peaks {
            if k < len {
                spec[k] = peak_db;
            }
        }
        spec
    }

    #[test]
    fn list_tonal_layer2_short_spectrum_returns_empty() {
        // The spec's `2 < k` precondition is unreachable below
        // `spl_db.len() == 4`; the sweep must not panic and must
        // leave the spectrum untouched.
        let mut spec = vec![10.0_f64, 20.0, 30.0];
        let snapshot = spec.clone();
        let got = list_tonal_layer2(&mut spec);
        assert!(got.is_empty());
        assert_eq!(spec, snapshot);
    }

    #[test]
    fn list_tonal_layer2_finds_single_isolated_peak() {
        // Place a 50 dB peak at k = 30 against a 0 dB floor in a
        // 1024-bin spectrum. The tonality test `X(k) - X(k+j) >= 7 dB`
        // is satisfied (50 - 0 = 50 dB) for every j in `[-2, 2]`
        // (the 2 < k < 63 neighbourhood).
        let mut spec = synthetic_spectrum(1024, 0.0, &[30], 50.0);
        let got = list_tonal_layer2(&mut spec);
        assert_eq!(got.len(), 1, "expected exactly one tonal: {got:?}");
        assert_eq!(got[0].k, 30);
        // X_tm = 10 * log10( 10^0 + 10^5 + 10^0 ) = 10 * log10(100002)
        //      ≈ 50.0000087 dB.
        let expected = 10.0_f64 * (1.0 + 10.0_f64.powi(5) + 1.0).log10();
        assert!(
            (got[0].spl_db - expected).abs() < 1.0e-9,
            "X_tm({}) = {}, expected {}",
            got[0].k,
            got[0].spl_db,
            expected,
        );
    }

    #[test]
    fn list_tonal_layer2_zeroes_neighbourhood_after_detection() {
        // After detection at k = 30, every index in `tonal_neighbourhood_layer2(30)`
        // (j ∈ {-2, +2} for 2 < k < 63) plus `k` itself must be set
        // to NEG_INFINITY in the mutated spectrum.
        let mut spec = synthetic_spectrum(1024, 0.0, &[30], 50.0);
        let _ = list_tonal_layer2(&mut spec);
        assert_eq!(spec[30], f64::NEG_INFINITY, "centre line {}", spec[30]);
        assert_eq!(spec[28], f64::NEG_INFINITY, "lower neighbour {}", spec[28]);
        assert_eq!(spec[32], f64::NEG_INFINITY, "upper neighbour {}", spec[32]);
        // Lines outside the neighbourhood are untouched.
        assert_eq!(spec[27], 0.0);
        assert_eq!(spec[33], 0.0);
    }

    #[test]
    fn list_tonal_layer2_rejects_subthreshold_local_max() {
        // A local maximum that's only 5 dB above its neighbours fails
        // the 7 dB tonality inequality and must be dropped.
        let mut spec = vec![0.0_f64; 1024];
        spec[30] = 5.0; // local max, but `5 - 0 = 5 < 7`.
        let got = list_tonal_layer2(&mut spec);
        assert!(got.is_empty(), "subthreshold should not list: {got:?}");
        // Spectrum untouched.
        assert_eq!(spec[30], 5.0);
        assert_eq!(spec[28], 0.0);
    }

    #[test]
    fn list_tonal_layer2_skips_lines_within_prior_neighbourhood() {
        // Two peaks 2 bins apart: k = 30 (50 dB) and k = 32 (40 dB)
        // against a 0 dB floor. The k = 30 detection zeroes the
        // neighbourhood `{28, 30, 32}` (j ∈ {-2, +2}), so the
        // k = 32 candidate cannot be classified tonal afterwards
        // (its centre is now -inf dB).
        let mut spec = synthetic_spectrum(1024, 0.0, &[30, 32], 50.0);
        spec[32] = 40.0; // distinct SPL so detection order matters
        let got = list_tonal_layer2(&mut spec);
        assert_eq!(got.len(), 1, "second peak should be suppressed: {got:?}");
        assert_eq!(got[0].k, 30);
        // k = 32 is in the k = 30 neighbourhood and is therefore now
        // -inf dB.
        assert_eq!(spec[32], f64::NEG_INFINITY);
    }

    #[test]
    fn list_tonal_layer2_emits_multiple_well_separated_peaks() {
        // Two peaks far enough apart that neither lies inside the
        // other's neighbourhood at any per-k width row (the densest
        // row `2 < k < 63` uses j ∈ {-2, +2}; spacing 6 bins is safe
        // for it and the other rows have wider j's at correspondingly
        // higher k). Pick `k = 30` (j ∈ {-2, +2}, neighbourhood
        // {28, 30, 32}) and `k = 80` (63 <= k < 127, j ∈ {-3, -2, +2,
        // +3}, neighbourhood {77, 78, 80, 82, 83}). Both pass the
        // tonality test against the 0 dB floor.
        let mut spec = synthetic_spectrum(1024, 0.0, &[30, 80], 50.0);
        let got = list_tonal_layer2(&mut spec);
        assert_eq!(got.len(), 2, "expected two tonals: {got:?}");
        assert_eq!(got[0].k, 30);
        assert_eq!(got[1].k, 80);
        // Both neighbourhoods zeroed.
        assert_eq!(spec[30], f64::NEG_INFINITY);
        assert_eq!(spec[80], f64::NEG_INFINITY);
        assert_eq!(spec[28], f64::NEG_INFINITY);
        assert_eq!(spec[82], f64::NEG_INFINITY);
        assert_eq!(spec[78], f64::NEG_INFINITY);
        assert_eq!(spec[83], f64::NEG_INFINITY);
    }

    #[test]
    fn list_tonal_layer2_emits_ascending_k_order() {
        // Sweep visits in ascending `k`, so the output list is
        // monotonically increasing on `k`.
        let mut spec = synthetic_spectrum(1024, 0.0, &[30, 80, 200, 400], 50.0);
        let got = list_tonal_layer2(&mut spec);
        for win in got.windows(2) {
            assert!(
                win[0].k < win[1].k,
                "non-ascending k: {} then {}",
                win[0].k,
                win[1].k,
            );
        }
    }

    #[test]
    fn list_tonal_layer2_ignores_edges_outside_tonality_domain() {
        // The tonality test is `2 < k <= 500`; a peak at k = 1 or
        // k = 700 in a 1024-bin spectrum must not list. Use spacing
        // safe for whichever neighbourhood applies.
        let mut spec = synthetic_spectrum(1024, 0.0, &[1, 600], 50.0);
        let got = list_tonal_layer2(&mut spec);
        assert!(got.is_empty(), "edge candidates should not list: {got:?}");
        // Both untouched (no zero-out applied).
        assert_eq!(spec[1], 50.0);
        assert_eq!(spec[600], 50.0);
    }

    #[test]
    fn list_tonal_layer2_composes_with_list_non_tonal_layer2() {
        // End-to-end sanity: drive Step 4(b) then Step 4(c) and
        // confirm the resulting non-tonal masker list has no entry
        // sitting on the tonal peak's neighbourhood (the bands that
        // contain only zeroed lines should drop out via
        // `non_tonal_spl_db == None`).
        let mut spec = synthetic_spectrum(1024, -20.0, &[100], 60.0);
        let tonal = list_tonal_layer2(&mut spec);
        assert_eq!(tonal.len(), 1);
        assert_eq!(tonal[0].k, 100);
        // Non-tonal listing at 44.1 kHz Layer II should still emit
        // entries — the spectrum's other bands carry finite floor
        // power and produce normal X_nm values.
        let non_tonal = list_non_tonal_layer2(&spec, SamplingRate::Fs44k1Hz);
        assert!(
            !non_tonal.is_empty(),
            "non-tonal sweep should still emit bands"
        );
        // Every non-tonal masker has finite SPL (no `NaN` / `-inf`
        // leaked from the zeroed lines).
        for m in &non_tonal {
            assert!(
                m.spl_db.is_finite(),
                "non-tonal at z={} has non-finite SPL {}",
                m.z_bark,
                m.spl_db,
            );
        }
    }

    #[test]
    fn step8_and_step9_compose_end_to_end() {
        // Drive Step 7 → Step 8 → Step 9 on a small synthetic case:
        // 4 FFT lines, two in subband 3, two in subband 7. Step 8
        // takes the min per subband; Step 9 subtracts that min from
        // L_sb to land the per-subband SMR.
        let ltg = vec![40.0_f64, 50.0, 35.0, 60.0];
        let map = vec![3_usize, 3, 7, 7];
        let lt_min = minimum_masking_threshold_subband(&ltg, &map);
        assert_eq!(lt_min[3], Some(40.0));
        assert_eq!(lt_min[7], Some(35.0));
        let mut l_sb = [0.0_f64; NUM_SUBBANDS_LAYER2];
        l_sb[3] = 75.0;
        l_sb[7] = 70.0;
        let smr = signal_to_mask_ratio_subband(&l_sb, &lt_min);
        assert_eq!(smr[3], Some(35.0));
        assert_eq!(smr[7], Some(35.0));
        // Other subbands carry no LT_min and so no SMR.
        for (n, slot) in smr.iter().enumerate() {
            if n == 3 || n == 7 {
                continue;
            }
            assert_eq!(*slot, None);
        }
    }

    // ----- §D.1 Step 2: sound pressure level per subband -----

    #[test]
    fn scalefactor_spl_term_unity_anchor() {
        // scf_max = 1.0: 20·log10(32768) - 10
        //   = 300·log10(2) - 10 = 90.30899869919435… - 10.
        let expected = 300.0 * 2.0_f64.log10() - 10.0;
        assert!((scalefactor_spl_term_db(1.0) - expected).abs() < 1e-12);
        assert!((scalefactor_spl_term_db(1.0) - 80.308_998_699_194_35).abs() < 1e-9);
    }

    #[test]
    fn scalefactor_spl_term_doubling_adds_exactly_20log2() {
        // 20·log10(2·x·32768) - 20·log10(x·32768) = 20·log10(2)
        // ≈ 6.0206 dB; Table 3-B.1 entry 0 (2.0) vs unity.
        let delta = scalefactor_spl_term_db(2.0) - scalefactor_spl_term_db(1.0);
        assert!((delta - 20.0 * 2.0_f64.log10()).abs() < 1e-12);
        // Monotone over the whole Table 3-B.1 multiplier range.
        for i in 1..crate::tables::SCALEFACTOR_COUNT {
            let hi = scalefactor_spl_term_db(crate::tables::SCALEFACTORS[i - 1]);
            let lo = scalefactor_spl_term_db(crate::tables::SCALEFACTORS[i]);
            assert!(hi > lo, "term must follow Table 3-B.1 monotonicity at {i}");
        }
    }

    #[test]
    fn spl_max_line_picks_loudest_line_in_subband() {
        // Three lines in subband 5 at 70 / 95 / 80 dB; scf small so
        // the X operand dominates the MAX → L_sb(5) = 95 dB.
        let spl = vec![70.0_f64, 95.0, 80.0];
        let map = vec![5_usize, 5, 5];
        let scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        let l_sb = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::MaxLine);
        assert!((l_sb[5] - 95.0).abs() < 1e-12);
    }

    #[test]
    fn spl_scalefactor_term_dominates_quiet_spectrum() {
        // All lines at -100 dB with scf_max = 1.0: the MAX resolves
        // to the scalefactor operand 80.3089… dB in both methods.
        let spl = vec![-100.0_f64; 32];
        let map: Vec<usize> = (0..32).map(|k| k / 16).collect();
        let mut scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        scf[0] = 1.0;
        scf[1] = 1.0;
        let expected = scalefactor_spl_term_db(1.0);
        for method in [SubbandSplMethod::MaxLine, SubbandSplMethod::PowerSum] {
            let l_sb = sound_pressure_level_subband(&spl, &map, &scf, method);
            assert!((l_sb[0] - expected).abs() < 1e-9);
            assert!((l_sb[1] - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn spl_power_sum_three_equal_lines() {
        // X_spl = 10·log10(3·10^(90/10)) = 90 + 10·log10(3)
        // ≈ 94.7712 dB; scf tiny so the power sum wins the MAX.
        let spl = vec![90.0_f64, 90.0, 90.0];
        let map = vec![12_usize, 12, 12];
        let scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        let l_sb = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::PowerSum);
        let expected = 90.0 + 10.0 * 3.0_f64.log10();
        assert!((l_sb[12] - expected).abs() < 1e-12);
    }

    #[test]
    fn spl_power_sum_never_below_max_line() {
        // Σ 10^(X/10) >= max 10^(X/10), so the PowerSum L_sb
        // dominates the MaxLine L_sb in every subband.
        let spl: Vec<f64> = (0..512).map(|k| 30.0 + ((k * 7) % 50) as f64).collect();
        let map: Vec<usize> = (0..512).map(fft_line_to_subband_layer2).collect();
        let scf = [0.5_f64; NUM_SUBBANDS_LAYER2];
        let by_max = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::MaxLine);
        let by_sum = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::PowerSum);
        for n in 0..NUM_SUBBANDS_LAYER2 {
            assert!(
                by_sum[n] >= by_max[n] - 1e-12,
                "PowerSum below MaxLine at subband {n}"
            );
        }
    }

    #[test]
    fn spl_subband_with_no_lines_falls_back_to_scf_term() {
        // Every line maps to the out-of-band sentinel: the X
        // operand is the empty maximum (-inf) and every slot
        // degenerates to its scalefactor term — in both methods.
        let spl = vec![100.0_f64; 8];
        let map = vec![usize::MAX; 8];
        let mut scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        scf[9] = 2.0;
        for method in [SubbandSplMethod::MaxLine, SubbandSplMethod::PowerSum] {
            let l_sb = sound_pressure_level_subband(&spl, &map, &scf, method);
            for (n, (slot, sf)) in l_sb.iter().zip(scf.iter()).enumerate() {
                let expected = scalefactor_spl_term_db(*sf);
                assert!(
                    (slot - expected).abs() < 1e-12,
                    "subband {n} must carry the scf term alone"
                );
            }
        }
    }

    #[test]
    fn spl_sentinel_and_out_of_range_indices_skipped() {
        // usize::MAX and any index >= 32 contribute to no subband;
        // the valid line still lands.
        let spl = vec![88.0_f64, 99.0, 77.0];
        let map = vec![4_usize, usize::MAX, 32];
        let scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        let l_sb = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::MaxLine);
        assert!((l_sb[4] - 88.0).abs() < 1e-12);
        for (n, slot) in l_sb.iter().enumerate() {
            if n == 4 {
                continue;
            }
            assert!((slot - scalefactor_spl_term_db(1e-6)).abs() < 1e-12);
        }
    }

    #[test]
    fn spl_nan_lines_dropped_from_x_operand() {
        let spl = vec![f64::NAN, 85.0, f64::NAN];
        let map = vec![2_usize, 2, 2];
        let scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        for method in [SubbandSplMethod::MaxLine, SubbandSplMethod::PowerSum] {
            let l_sb = sound_pressure_level_subband(&spl, &map, &scf, method);
            assert!(
                (l_sb[2] - 85.0).abs() < 1e-12,
                "NaN must not poison the MAX"
            );
        }
    }

    #[test]
    fn spl_length_mismatch_returns_scf_terms() {
        // Documented safe response to the caller error: the
        // spectrum is treated as empty.
        let spl = vec![100.0_f64; 10];
        let map = vec![0_usize; 9];
        let scf = [1.0_f64; NUM_SUBBANDS_LAYER2];
        let l_sb = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::MaxLine);
        for slot in &l_sb {
            assert!((slot - scalefactor_spl_term_db(1.0)).abs() < 1e-12);
        }
    }

    #[test]
    fn fft_line_to_subband_layer2_boundaries() {
        // 16 lines per subband: fs/1024 resolution vs fs/64 width.
        assert_eq!(fft_line_to_subband_layer2(0), 0);
        assert_eq!(fft_line_to_subband_layer2(15), 0);
        assert_eq!(fft_line_to_subband_layer2(16), 1);
        assert_eq!(fft_line_to_subband_layer2(255), 15);
        assert_eq!(fft_line_to_subband_layer2(256), 16);
        assert_eq!(fft_line_to_subband_layer2(511), 31);
        // Nyquist line and beyond sit outside every subband.
        assert_eq!(fft_line_to_subband_layer2(512), usize::MAX);
        assert_eq!(fft_line_to_subband_layer2(1000), usize::MAX);
        // Full sweep: every in-band line lands in k/16 and every
        // subband receives exactly 16 lines.
        let mut per_subband = [0_usize; NUM_SUBBANDS_LAYER2];
        for k in 0..LAYER2_FFT_LEN / 2 {
            let sb = fft_line_to_subband_layer2(k);
            assert_eq!(sb, k / 16);
            per_subband[sb] += 1;
        }
        assert!(per_subband.iter().all(|&c| c == 16));
    }

    #[test]
    fn step2_feeds_step9_smr_end_to_end() {
        // §D.1 Step 2 → Step 8 → Step 9 composition: a 95 dB line
        // in subband 1 over a tiny scalefactor, LTg = 60 dB on the
        // same line → SMR_sb(1) = 95 - 60 = 35 dB.
        let spl = vec![-100.0_f64, -100.0, 95.0];
        let map = vec![
            fft_line_to_subband_layer2(0),  // 0
            fft_line_to_subband_layer2(8),  // 0
            fft_line_to_subband_layer2(16), // 1
        ];
        let scf = [1e-6_f64; NUM_SUBBANDS_LAYER2];
        let l_sb = sound_pressure_level_subband(&spl, &map, &scf, SubbandSplMethod::MaxLine);
        let ltg = vec![55.0_f64, 58.0, 60.0];
        let lt_min = minimum_masking_threshold_subband(&ltg, &map);
        let smr = signal_to_mask_ratio_subband(&l_sb, &lt_min);
        assert_eq!(lt_min[1], Some(60.0));
        let smr1 = smr[1].expect("subband 1 carries an SMR");
        assert!((smr1 - 35.0).abs() < 1e-12);
    }
}
