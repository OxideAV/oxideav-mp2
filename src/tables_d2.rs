//! ISO/IEC 11172-3:1993 Annex D Tables **D.2d / D.2e / D.2f** —
//! *Critical band boundaries* for Layer II at the three primary
//! MPEG-1 sampling rates (32 / 44,1 / 48 kHz).
//!
//! Annex D is informative. Each row of a D.2 table gives one critical
//! band of the spec's example Model 1 psychoacoustic model:
//!
//! * `no` — band number.
//! * `index F&CB` — the index `i` into the corresponding D.1
//!   "Frequencies, critical band rates and absolute threshold" table
//!   identifying the **top FFT-line index** that falls into this
//!   critical band.
//! * `frequency [Hz]` — the frequency of that top line.
//! * `Bark [z]` — the critical-band rate of that top line, in Bark
//!   units.
//!
//! The first three columns are normative-by-reference for §D.1
//! Step 4(c) ("Listing of non-tonal components and calculation of the
//! power") — the step iterates the boundaries in order and, within
//! each `(prev_index + 1 ..= index)` half-open run of FFT lines,
//! power-sums the remaining (non-tonal) lines into a single
//! non-tonal masker representing that critical band. The Bark column
//! is used downstream by §D.1 Step 6 to position the masker on the
//! Bark axis for the `vf` masking-function lookup; it is reproduced
//! here only for parity with the published table.
//!
//! ## Decimal-comma convention
//!
//! The spec PDF uses European decimal notation (`62,500` Hz =
//! 62.5 Hz; `0,925` = 0.925). The constants below are reproduced
//! with the period equivalents (idiomatic Rust `f64` literals); no
//! value has been altered from the spec.
//!
//! ## Source
//!
//! Direct transcription from the staged ISO/IEC 11172-3:1993 PDF
//! (`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`, SHA-256
//! `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`,
//! Tables D.2d / D.2e / D.2f at printed pages 122 / 124 / 126),
//! cross-checked against the markdown extract
//! `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`. No third-party
//! MP2 source was consulted.
//!
//! ## Layer II band-count note
//!
//! The §D.1 Step 4(c) prose (PDF page 112) says Layer II uses
//! **24 critical bands at 32 kHz and 26 critical bands at 44,1 kHz
//! and 48 kHz**. The Table D.2d / D.2e / D.2f tables themselves
//! list **25 / 27 / 27 entries** respectively — one more boundary
//! row than the prose-stated band count at each sampling rate. The
//! extra entry is the topmost row (which carries the absolute upper
//! end of the audio band at that sampling rate); the spec itself
//! does not resolve which row the consumer should drop. The
//! constants below reproduce the published tables **as-is, in
//! full**, and the Layer-II non-tonal-listing primitive in `psy`
//! iterates every row — a downstream caller may trim the topmost
//! row if it must follow the prose count strictly. This deferred
//! reconciliation is noted in the module-level docs of `psy`.

/// One row of an Annex D Table D.2 critical-band-boundary table.
///
/// The three fields are reproduced verbatim from the spec column
/// headings:
///
/// * [`Self::top_line_index`] — `index F&CB` (top FFT line of the
///   band).
/// * [`Self::top_frequency_hz`] — `frequency [Hz]` of that line.
/// * [`Self::top_bark`] — `Bark [z]` of that line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CriticalBandBoundary {
    /// Top FFT-line index of the critical band — the `i` index into
    /// the corresponding D.1 table.
    pub top_line_index: u32,
    /// Frequency of the top FFT line in hertz.
    pub top_frequency_hz: f64,
    /// Bark-rate (critical-band rate) of the top FFT line.
    pub top_bark: f64,
}

/// One row of an Annex D Table D.1d / D.1e / D.1f *Layer II*
/// "Frequencies, critical band rates and absolute threshold" table
/// (ISO/IEC 11172-3 for 32 / 44,1 / 48 kHz; ISO/IEC 13818-3 for the
/// LSF 16 / 22,05 / 24 kHz rates — both standards print the same
/// four-column layout under the same table letters).
///
/// The spec tabulates, per index `i`: the frequency `f(i)`, the
/// critical-band rate `z(i)` and the absolute threshold `LTq(i)` in
/// dB. All four printed columns are carried:
///
/// * [`Self::top_line_index`] — the top 1024-point-analysis-FFT line of
///   the range covered by index `i`. The lower bound is implicit
///   (previous entry's `top_line_index + 1`; the first entry's range
///   starts at line 1). These ranges are the deterministic
///   `higher = round(frequency_Hz / (Fs/1024))` mapping (they coincide
///   with the Annex D Table D.4 line ranges at the same rate).
/// * [`Self::frequency_hz`] — the printed `Frequency [Hz]` column, the
///   §D.1 Step 8 `f(i)` ("The f(i) are tabulated in tables D.1…").
/// * [`Self::bark`] — the printed `Crit.Band Rate [z]` column, the
///   §D.1 Step 6 `z(i)` ("The critical band rates z(j) and z(i) can be
///   found in tables D.1…").
/// * [`Self::threshold_db`] — the `Absolute threshold [dB]` column of
///   the Layer II D.1 table (D.1d / D.1e / D.1f), *not* the Model-2
///   D.4 column (which diverges by the documented last-digit / ceiling
///   errata at 32 kHz and 44.1 kHz). Step 5(a) cites D.1d/e/f by name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LtqEntry {
    /// Top FFT-line index of the range this threshold entry covers.
    pub top_line_index: u32,
    /// Frequency `f(i)` of the top FFT line, in hertz (printed
    /// `Frequency [Hz]` column).
    pub frequency_hz: f64,
    /// Critical-band rate `z(i)` of the top FFT line, in Bark (printed
    /// `Crit.Band Rate [z]` column).
    pub bark: f64,
    /// Threshold in quiet `LTq` (absolute threshold), in dB, before the
    /// §D.1 Step 3 overall-bit-rate offset is applied.
    pub threshold_db: f64,
}

/// Annex D Table **D.2d** — Critical band boundaries, Layer II,
/// Fs = 32 kHz. 25 rows, `no = 0 .. 24`. Source: PDF printed page
/// 122 (PDF p.128 in the staged 11172-3 edition); the markdown
/// extract `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`
/// is the cross-checked secondary copy.
pub const TABLE_D_2D_LAYER_II_32KHZ: [CriticalBandBoundary; 25] = [
    CriticalBandBoundary {
        top_line_index: 1,
        top_frequency_hz: 31.250,
        top_bark: 0.309,
    },
    CriticalBandBoundary {
        top_line_index: 3,
        top_frequency_hz: 93.750,
        top_bark: 0.925,
    },
    CriticalBandBoundary {
        top_line_index: 6,
        top_frequency_hz: 187.500,
        top_bark: 1.842,
    },
    CriticalBandBoundary {
        top_line_index: 10,
        top_frequency_hz: 312.500,
        top_bark: 3.037,
    },
    CriticalBandBoundary {
        top_line_index: 13,
        top_frequency_hz: 406.250,
        top_bark: 3.903,
    },
    CriticalBandBoundary {
        top_line_index: 17,
        top_frequency_hz: 531.250,
        top_bark: 5.006,
    },
    CriticalBandBoundary {
        top_line_index: 21,
        top_frequency_hz: 656.250,
        top_bark: 6.041,
    },
    CriticalBandBoundary {
        top_line_index: 25,
        top_frequency_hz: 781.250,
        top_bark: 7.004,
    },
    CriticalBandBoundary {
        top_line_index: 30,
        top_frequency_hz: 937.500,
        top_bark: 8.103,
    },
    CriticalBandBoundary {
        top_line_index: 35,
        top_frequency_hz: 1093.750,
        top_bark: 9.090,
    },
    CriticalBandBoundary {
        top_line_index: 41,
        top_frequency_hz: 1281.250,
        top_bark: 10.139,
    },
    CriticalBandBoundary {
        top_line_index: 47,
        top_frequency_hz: 1468.750,
        top_bark: 11.058,
    },
    CriticalBandBoundary {
        top_line_index: 51,
        top_frequency_hz: 1687.500,
        top_bark: 11.988,
    },
    CriticalBandBoundary {
        top_line_index: 56,
        top_frequency_hz: 2000.000,
        top_bark: 13.104,
    },
    CriticalBandBoundary {
        top_line_index: 61,
        top_frequency_hz: 2312.500,
        top_bark: 14.027,
    },
    CriticalBandBoundary {
        top_line_index: 68,
        top_frequency_hz: 2750.000,
        top_bark: 15.087,
    },
    CriticalBandBoundary {
        top_line_index: 74,
        top_frequency_hz: 3250.000,
        top_bark: 16.069,
    },
    CriticalBandBoundary {
        top_line_index: 79,
        top_frequency_hz: 3875.000,
        top_bark: 17.078,
    },
    CriticalBandBoundary {
        top_line_index: 85,
        top_frequency_hz: 4625.000,
        top_bark: 18.089,
    },
    CriticalBandBoundary {
        top_line_index: 92,
        top_frequency_hz: 5500.000,
        top_bark: 19.095,
    },
    CriticalBandBoundary {
        top_line_index: 98,
        top_frequency_hz: 6500.000,
        top_bark: 20.079,
    },
    CriticalBandBoundary {
        top_line_index: 103,
        top_frequency_hz: 7750.000,
        top_bark: 21.098,
    },
    CriticalBandBoundary {
        top_line_index: 109,
        top_frequency_hz: 9250.000,
        top_bark: 22.046,
    },
    CriticalBandBoundary {
        top_line_index: 118,
        top_frequency_hz: 11500.000,
        top_bark: 23.030,
    },
    CriticalBandBoundary {
        top_line_index: 132,
        top_frequency_hz: 15000.000,
        top_bark: 23.923,
    },
];

/// Annex D Table **D.2e** — Critical band boundaries, Layer II,
/// Fs = 44,1 kHz. 27 rows, `no = 0 .. 26`. Source: PDF printed page
/// 124.
///
/// Band 17's Bark cell prints `16,11` (two decimals where every
/// other cell carries three). The staged extract's resolved errata
/// (`docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md`,
/// "Errata — D.2e band 17 (resolved, #119)") pins the intended value
/// via the cross-print: the row's *index of Table F&CB* is 62, and
/// Table D.1e row i = 62 reads Crit.Band Rate **16,110** — the D.2
/// Bark column is by construction the D.1 critical-band rate of that
/// same index, so the print merely dropped a trailing zero. The
/// value below is the verified `16.110` (an earlier `16.116`
/// best-fit guess predating the errata resolution was replaced; the
/// in-module `d2_boundary_rows_match_d1_entries` cross-check now
/// enforces D.2 ≡ D.1 row-for-row at all rates).
pub const TABLE_D_2E_LAYER_II_44K1HZ: [CriticalBandBoundary; 27] = [
    CriticalBandBoundary {
        top_line_index: 1,
        top_frequency_hz: 43.066,
        top_bark: 0.425,
    },
    CriticalBandBoundary {
        top_line_index: 2,
        top_frequency_hz: 86.133,
        top_bark: 0.850,
    },
    CriticalBandBoundary {
        top_line_index: 3,
        top_frequency_hz: 129.199,
        top_bark: 1.273,
    },
    CriticalBandBoundary {
        top_line_index: 5,
        top_frequency_hz: 215.332,
        top_bark: 2.112,
    },
    CriticalBandBoundary {
        top_line_index: 7,
        top_frequency_hz: 301.465,
        top_bark: 2.934,
    },
    CriticalBandBoundary {
        top_line_index: 10,
        top_frequency_hz: 430.664,
        top_bark: 4.124,
    },
    CriticalBandBoundary {
        top_line_index: 13,
        top_frequency_hz: 559.863,
        top_bark: 5.249,
    },
    CriticalBandBoundary {
        top_line_index: 16,
        top_frequency_hz: 689.063,
        top_bark: 6.301,
    },
    CriticalBandBoundary {
        top_line_index: 19,
        top_frequency_hz: 818.262,
        top_bark: 7.274,
    },
    CriticalBandBoundary {
        top_line_index: 22,
        top_frequency_hz: 947.461,
        top_bark: 8.169,
    },
    CriticalBandBoundary {
        top_line_index: 26,
        top_frequency_hz: 1119.727,
        top_bark: 9.244,
    },
    CriticalBandBoundary {
        top_line_index: 30,
        top_frequency_hz: 1291.992,
        top_bark: 10.195,
    },
    CriticalBandBoundary {
        top_line_index: 35,
        top_frequency_hz: 1507.324,
        top_bark: 11.232,
    },
    CriticalBandBoundary {
        top_line_index: 40,
        top_frequency_hz: 1722.656,
        top_bark: 12.125,
    },
    CriticalBandBoundary {
        top_line_index: 46,
        top_frequency_hz: 1981.055,
        top_bark: 13.042,
    },
    CriticalBandBoundary {
        top_line_index: 51,
        top_frequency_hz: 2325.586,
        top_bark: 14.062,
    },
    CriticalBandBoundary {
        top_line_index: 56,
        top_frequency_hz: 2756.250,
        top_bark: 15.100,
    },
    // Band 17: printed `16,11` = 16,110 per the resolved errata (D.1e
    // i = 62 cross-print; see the table-level doc note).
    CriticalBandBoundary {
        top_line_index: 62,
        top_frequency_hz: 3273.047,
        top_bark: 16.110,
    },
    CriticalBandBoundary {
        top_line_index: 69,
        top_frequency_hz: 3875.977,
        top_bark: 17.079,
    },
    CriticalBandBoundary {
        top_line_index: 74,
        top_frequency_hz: 4478.906,
        top_bark: 17.904,
    },
    CriticalBandBoundary {
        top_line_index: 79,
        top_frequency_hz: 5340.234,
        top_bark: 18.922,
    },
    CriticalBandBoundary {
        top_line_index: 85,
        top_frequency_hz: 6373.828,
        top_bark: 19.963,
    },
    CriticalBandBoundary {
        top_line_index: 92,
        top_frequency_hz: 7579.688,
        top_bark: 20.971,
    },
    CriticalBandBoundary {
        top_line_index: 99,
        top_frequency_hz: 9302.344,
        top_bark: 22.074,
    },
    CriticalBandBoundary {
        top_line_index: 105,
        top_frequency_hz: 11369.531,
        top_bark: 22.984,
    },
    CriticalBandBoundary {
        top_line_index: 117,
        top_frequency_hz: 15503.906,
        top_bark: 24.013,
    },
    CriticalBandBoundary {
        top_line_index: 130,
        top_frequency_hz: 19982.813,
        top_bark: 24.573,
    },
];

/// Annex D Table **D.2f** — Critical band boundaries, Layer II,
/// Fs = 48 kHz. 27 rows, `no = 0 .. 26`. Source: PDF printed page 126.
pub const TABLE_D_2F_LAYER_II_48KHZ: [CriticalBandBoundary; 27] = [
    CriticalBandBoundary {
        top_line_index: 1,
        top_frequency_hz: 46.875,
        top_bark: 0.463,
    },
    CriticalBandBoundary {
        top_line_index: 2,
        top_frequency_hz: 93.750,
        top_bark: 0.925,
    },
    CriticalBandBoundary {
        top_line_index: 3,
        top_frequency_hz: 140.625,
        top_bark: 1.385,
    },
    CriticalBandBoundary {
        top_line_index: 5,
        top_frequency_hz: 234.375,
        top_bark: 2.295,
    },
    CriticalBandBoundary {
        top_line_index: 7,
        top_frequency_hz: 328.125,
        top_bark: 3.184,
    },
    CriticalBandBoundary {
        top_line_index: 9,
        top_frequency_hz: 421.875,
        top_bark: 4.045,
    },
    CriticalBandBoundary {
        top_line_index: 12,
        top_frequency_hz: 562.500,
        top_bark: 5.272,
    },
    CriticalBandBoundary {
        top_line_index: 14,
        top_frequency_hz: 656.250,
        top_bark: 6.041,
    },
    CriticalBandBoundary {
        top_line_index: 17,
        top_frequency_hz: 796.875,
        top_bark: 7.119,
    },
    CriticalBandBoundary {
        top_line_index: 20,
        top_frequency_hz: 937.500,
        top_bark: 8.103,
    },
    CriticalBandBoundary {
        top_line_index: 24,
        top_frequency_hz: 1125.000,
        top_bark: 9.275,
    },
    CriticalBandBoundary {
        top_line_index: 27,
        top_frequency_hz: 1265.625,
        top_bark: 10.057,
    },
    CriticalBandBoundary {
        top_line_index: 32,
        top_frequency_hz: 1500.000,
        top_bark: 11.199,
    },
    CriticalBandBoundary {
        top_line_index: 37,
        top_frequency_hz: 1734.375,
        top_bark: 12.170,
    },
    CriticalBandBoundary {
        top_line_index: 42,
        top_frequency_hz: 1968.750,
        top_bark: 13.002,
    },
    CriticalBandBoundary {
        top_line_index: 49,
        top_frequency_hz: 2343.750,
        top_bark: 14.111,
    },
    CriticalBandBoundary {
        top_line_index: 53,
        top_frequency_hz: 2718.750,
        top_bark: 15.018,
    },
    CriticalBandBoundary {
        top_line_index: 59,
        top_frequency_hz: 3281.250,
        top_bark: 16.124,
    },
    CriticalBandBoundary {
        top_line_index: 65,
        top_frequency_hz: 3843.750,
        top_bark: 17.032,
    },
    CriticalBandBoundary {
        top_line_index: 73,
        top_frequency_hz: 4687.500,
        top_bark: 18.166,
    },
    CriticalBandBoundary {
        top_line_index: 77,
        top_frequency_hz: 5437.500,
        top_bark: 19.028,
    },
    CriticalBandBoundary {
        top_line_index: 82,
        top_frequency_hz: 6375.000,
        top_bark: 19.964,
    },
    CriticalBandBoundary {
        top_line_index: 89,
        top_frequency_hz: 7687.500,
        top_bark: 21.052,
    },
    CriticalBandBoundary {
        top_line_index: 97,
        top_frequency_hz: 9375.000,
        top_bark: 22.113,
    },
    CriticalBandBoundary {
        top_line_index: 103,
        top_frequency_hz: 11625.000,
        top_bark: 23.072,
    },
    CriticalBandBoundary {
        top_line_index: 113,
        top_frequency_hz: 15375.000,
        top_bark: 23.991,
    },
    CriticalBandBoundary {
        top_line_index: 126,
        top_frequency_hz: 20250.000,
        top_bark: 24.597,
    },
];

/// Layer II sampling rate enumeration carried by §D.1 Step 4(c) so
/// that the non-tonal listing primitive in `psy` can pick the right
/// critical-band-boundary table (D.2d / D.2e / D.2f) without the
/// caller having to know which constant to import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingRate {
    /// 32 kHz — Table D.2d (25 boundary entries).
    Fs32kHz,
    /// 44.1 kHz — Table D.2e (27 boundary entries).
    Fs44k1Hz,
    /// 48 kHz — Table D.2f (27 boundary entries).
    Fs48kHz,
}

impl SamplingRate {
    /// Returns the Layer II Annex D Table D.2 critical-band-boundary
    /// slice for this sampling rate.
    #[must_use]
    pub fn critical_band_boundaries(self) -> &'static [CriticalBandBoundary] {
        match self {
            SamplingRate::Fs32kHz => &TABLE_D_2D_LAYER_II_32KHZ,
            SamplingRate::Fs44k1Hz => &TABLE_D_2E_LAYER_II_44K1HZ,
            SamplingRate::Fs48kHz => &TABLE_D_2F_LAYER_II_48KHZ,
        }
    }

    /// Returns the Layer II Annex D Table D.1d / D.1e / D.1f
    /// threshold-in-quiet (`LTq`) slice for this sampling rate, used by
    /// the §D.1 Step 5(a) threshold-in-quiet decimation in `psy`.
    #[must_use]
    pub fn ltq_table_layer2(self) -> &'static [LtqEntry] {
        match self {
            SamplingRate::Fs32kHz => &TABLE_D_1D_LTQ_LAYER_II_32,
            SamplingRate::Fs44k1Hz => &TABLE_D_1E_LTQ_LAYER_II_44_1HZ,
            SamplingRate::Fs48kHz => &TABLE_D_1F_LTQ_LAYER_II_48,
        }
    }
}

/// ISO/IEC 11172-3:1993 Annex D Table D.1d — Layer II
/// "Frequencies, critical band rates and absolute threshold",
/// Fs = 32000 Hz. 132 entries. Each entry carries the top FFT line of
/// its 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// plus the three printed value columns: `Frequency [Hz]`,
/// `Crit.Band Rate [z]` and `Absolute threshold [dB]` (all
/// transcribed from the staged CSV
/// `docs/audio/mp3/annex-d-table-D1d-threshold-32kHz-LayerII.csv`).
/// The FFT-line ranges come from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
// One spec threshold value (6.28 dB) happens to coincide with the
// decimal expansion of TAU; it is a verbatim Annex D table cell, not a
// mathematical constant, so the approx_constant lint is suppressed.
#[allow(clippy::approx_constant)]
pub static TABLE_D_1D_LTQ_LAYER_II_32: [LtqEntry; 132] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 31.25,
        bark: 0.309,
        threshold_db: 58.23,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 62.5,
        bark: 0.617,
        threshold_db: 33.44,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 93.75,
        bark: 0.925,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 125.0,
        bark: 1.232,
        threshold_db: 19.2,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 156.25,
        bark: 1.538,
        threshold_db: 16.05,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 187.5,
        bark: 1.842,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 218.75,
        bark: 2.145,
        threshold_db: 12.26,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 250.0,
        bark: 2.445,
        threshold_db: 11.01,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 281.25,
        bark: 2.742,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 312.5,
        bark: 3.037,
        threshold_db: 9.2,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 343.75,
        bark: 3.329,
        threshold_db: 8.52,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 375.0,
        bark: 3.618,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 406.25,
        bark: 3.903,
        threshold_db: 7.44,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 437.5,
        bark: 4.185,
        threshold_db: 7.0,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 468.75,
        bark: 4.463,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 500.0,
        bark: 4.736,
        threshold_db: 6.28,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 531.25,
        bark: 5.006,
        threshold_db: 5.97,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 562.5,
        bark: 5.272,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 593.75,
        bark: 5.533,
        threshold_db: 5.44,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 625.0,
        bark: 5.789,
        threshold_db: 5.21,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 656.25,
        bark: 6.041,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 687.5,
        bark: 6.289,
        threshold_db: 4.8,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 718.75,
        bark: 6.532,
        threshold_db: 4.62,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 750.0,
        bark: 6.77,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 781.25,
        bark: 7.004,
        threshold_db: 4.29,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 812.5,
        bark: 7.233,
        threshold_db: 4.14,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 843.75,
        bark: 7.457,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 875.0,
        bark: 7.677,
        threshold_db: 3.86,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 906.25,
        bark: 7.892,
        threshold_db: 3.73,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 937.5,
        bark: 8.103,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 968.75,
        bark: 8.309,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 1000.0,
        bark: 8.511,
        threshold_db: 3.37,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 1031.25,
        bark: 8.708,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 1062.5,
        bark: 8.901,
        threshold_db: 3.15,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 1093.75,
        bark: 9.09,
        threshold_db: 3.04,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 1125.0,
        bark: 9.275,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 1156.25,
        bark: 9.456,
        threshold_db: 2.83,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 1187.5,
        bark: 9.632,
        threshold_db: 2.73,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 1218.75,
        bark: 9.805,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 1250.0,
        bark: 9.974,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 1281.25,
        bark: 10.139,
        threshold_db: 2.42,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 1312.5,
        bark: 10.301,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 1343.75,
        bark: 10.459,
        threshold_db: 2.22,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 1375.0,
        bark: 10.614,
        threshold_db: 2.12,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 1406.25,
        bark: 10.765,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 1437.5,
        bark: 10.913,
        threshold_db: 1.92,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 1468.75,
        bark: 11.058,
        threshold_db: 1.81,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 1500.0,
        bark: 11.199,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 1562.5,
        bark: 11.474,
        threshold_db: 1.49,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 1625.0,
        bark: 11.736,
        threshold_db: 1.27,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 1687.5,
        bark: 11.988,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 1750.0,
        bark: 12.23,
        threshold_db: 0.8,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 1812.5,
        bark: 12.461,
        threshold_db: 0.55,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 1875.0,
        bark: 12.684,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 1937.5,
        bark: 12.898,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 2000.0,
        bark: 13.104,
        threshold_db: -0.25,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 2062.5,
        bark: 13.302,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 2125.0,
        bark: 13.493,
        threshold_db: -0.83,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 2187.5,
        bark: 13.678,
        threshold_db: -1.12,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 2250.0,
        bark: 13.855,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 2312.5,
        bark: 14.027,
        threshold_db: -1.73,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 2375.0,
        bark: 14.193,
        threshold_db: -2.04,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 2437.5,
        bark: 14.354,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 2500.0,
        bark: 14.509,
        threshold_db: -2.64,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 2562.5,
        bark: 14.66,
        threshold_db: -2.93,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 2625.0,
        bark: 14.807,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 2687.5,
        bark: 14.949,
        threshold_db: -3.49,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 2750.0,
        bark: 15.087,
        threshold_db: -3.74,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 2812.5,
        bark: 15.221,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 2875.0,
        bark: 15.351,
        threshold_db: -4.2,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 2937.5,
        bark: 15.478,
        threshold_db: -4.4,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 3000.0,
        bark: 15.602,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 3125.0,
        bark: 15.841,
        threshold_db: -4.82,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 3250.0,
        bark: 16.069,
        threshold_db: -4.96,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 3375.0,
        bark: 16.287,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 3500.0,
        bark: 16.496,
        threshold_db: -4.86,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 3625.0,
        bark: 16.697,
        threshold_db: -4.63,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 3750.0,
        bark: 16.891,
        threshold_db: -4.29,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 3875.0,
        bark: 17.078,
        threshold_db: -3.87,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 4000.0,
        bark: 17.259,
        threshold_db: -3.39,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 4125.0,
        bark: 17.434,
        threshold_db: -2.86,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 4250.0,
        bark: 17.605,
        threshold_db: -2.31,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 4375.0,
        bark: 17.77,
        threshold_db: -1.77,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 4500.0,
        bark: 17.932,
        threshold_db: -1.24,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 4625.0,
        bark: 18.089,
        threshold_db: -0.74,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 4750.0,
        bark: 18.242,
        threshold_db: -0.29,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 4875.0,
        bark: 18.392,
        threshold_db: 0.12,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 5000.0,
        bark: 18.539,
        threshold_db: 0.48,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 5125.0,
        bark: 18.682,
        threshold_db: 0.79,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 5250.0,
        bark: 18.823,
        threshold_db: 1.06,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 5375.0,
        bark: 18.96,
        threshold_db: 1.29,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 5500.0,
        bark: 19.095,
        threshold_db: 1.49,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 5625.0,
        bark: 19.226,
        threshold_db: 1.66,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 5750.0,
        bark: 19.356,
        threshold_db: 1.81,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 5875.0,
        bark: 19.482,
        threshold_db: 1.95,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 6000.0,
        bark: 19.606,
        threshold_db: 2.08,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 6250.0,
        bark: 19.847,
        threshold_db: 2.33,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 6500.0,
        bark: 20.079,
        threshold_db: 2.59,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 6750.0,
        bark: 20.3,
        threshold_db: 2.86,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 7000.0,
        bark: 20.513,
        threshold_db: 3.17,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 7250.0,
        bark: 20.717,
        threshold_db: 3.51,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 7500.0,
        bark: 20.912,
        threshold_db: 3.89,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 7750.0,
        bark: 21.098,
        threshold_db: 4.31,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 8000.0,
        bark: 21.275,
        threshold_db: 4.79,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 8250.0,
        bark: 21.445,
        threshold_db: 5.31,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 8500.0,
        bark: 21.606,
        threshold_db: 5.88,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 8750.0,
        bark: 21.76,
        threshold_db: 6.5,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 9000.0,
        bark: 21.906,
        threshold_db: 7.19,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 9250.0,
        bark: 22.046,
        threshold_db: 7.93,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 9500.0,
        bark: 22.178,
        threshold_db: 8.75,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 9750.0,
        bark: 22.304,
        threshold_db: 9.63,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 10000.0,
        bark: 22.424,
        threshold_db: 10.58,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 10250.0,
        bark: 22.538,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 10500.0,
        bark: 22.646,
        threshold_db: 12.71,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 10750.0,
        bark: 22.749,
        threshold_db: 13.9,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 11000.0,
        bark: 22.847,
        threshold_db: 15.18,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 11250.0,
        bark: 22.941,
        threshold_db: 16.54,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 11500.0,
        bark: 23.03,
        threshold_db: 18.01,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 11750.0,
        bark: 23.114,
        threshold_db: 19.57,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 12000.0,
        bark: 23.195,
        threshold_db: 21.23,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 12250.0,
        bark: 23.272,
        threshold_db: 23.01,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 12500.0,
        bark: 23.345,
        threshold_db: 24.9,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 12750.0,
        bark: 23.415,
        threshold_db: 26.9,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 13000.0,
        bark: 23.482,
        threshold_db: 29.03,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 13250.0,
        bark: 23.546,
        threshold_db: 31.28,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 13500.0,
        bark: 23.607,
        threshold_db: 33.67,
    },
    LtqEntry {
        top_line_index: 440,
        frequency_hz: 13750.0,
        bark: 23.666,
        threshold_db: 36.19,
    },
    LtqEntry {
        top_line_index: 448,
        frequency_hz: 14000.0,
        bark: 23.722,
        threshold_db: 38.86,
    },
    LtqEntry {
        top_line_index: 456,
        frequency_hz: 14250.0,
        bark: 23.775,
        threshold_db: 41.67,
    },
    LtqEntry {
        top_line_index: 464,
        frequency_hz: 14500.0,
        bark: 23.827,
        threshold_db: 44.63,
    },
    LtqEntry {
        top_line_index: 472,
        frequency_hz: 14750.0,
        bark: 23.876,
        threshold_db: 47.76,
    },
    LtqEntry {
        top_line_index: 480,
        frequency_hz: 15000.0,
        bark: 23.923,
        threshold_db: 51.04,
    },
];

/// ISO/IEC 11172-3:1993 Annex D Table D.1e — Layer II
/// "Frequencies, critical band rates and absolute threshold",
/// Fs = 44100 Hz. 130 entries. Each entry carries the top FFT line of
/// its 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// plus the three printed value columns: `Frequency [Hz]`,
/// `Crit.Band Rate [z]` and `Absolute threshold [dB]` (all
/// transcribed from the staged CSV
/// `docs/audio/mp3/annex-d-table-D1e-threshold-44k1Hz-LayerII.csv`).
/// The FFT-line ranges come from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
pub static TABLE_D_1E_LTQ_LAYER_II_44_1HZ: [LtqEntry; 130] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 43.07,
        bark: 0.425,
        threshold_db: 45.05,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 86.13,
        bark: 0.85,
        threshold_db: 25.87,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 129.2,
        bark: 1.273,
        threshold_db: 18.7,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 172.27,
        bark: 1.694,
        threshold_db: 14.85,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 215.33,
        bark: 2.112,
        threshold_db: 12.41,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 258.4,
        bark: 2.525,
        threshold_db: 10.72,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 301.46,
        bark: 2.934,
        threshold_db: 9.47,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 344.53,
        bark: 3.337,
        threshold_db: 8.5,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 387.6,
        bark: 3.733,
        threshold_db: 7.73,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 430.66,
        bark: 4.124,
        threshold_db: 7.1,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 473.73,
        bark: 4.507,
        threshold_db: 6.56,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 516.8,
        bark: 4.882,
        threshold_db: 6.11,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 559.86,
        bark: 5.249,
        threshold_db: 5.72,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 602.93,
        bark: 5.608,
        threshold_db: 5.37,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 646.0,
        bark: 5.959,
        threshold_db: 5.07,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 689.06,
        bark: 6.301,
        threshold_db: 4.79,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 732.13,
        bark: 6.634,
        threshold_db: 4.55,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 775.2,
        bark: 6.959,
        threshold_db: 4.32,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 818.26,
        bark: 7.274,
        threshold_db: 4.11,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 861.33,
        bark: 7.581,
        threshold_db: 3.92,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 904.39,
        bark: 7.879,
        threshold_db: 3.74,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 947.46,
        bark: 8.169,
        threshold_db: 3.57,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 990.53,
        bark: 8.45,
        threshold_db: 3.4,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 1033.59,
        bark: 8.723,
        threshold_db: 3.25,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 1076.66,
        bark: 8.987,
        threshold_db: 3.1,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 1119.73,
        bark: 9.244,
        threshold_db: 2.95,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 1162.79,
        bark: 9.493,
        threshold_db: 2.81,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 1205.86,
        bark: 9.734,
        threshold_db: 2.67,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 1248.93,
        bark: 9.968,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 1291.99,
        bark: 10.195,
        threshold_db: 2.39,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 1335.06,
        bark: 10.416,
        threshold_db: 2.25,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 1378.13,
        bark: 10.629,
        threshold_db: 2.11,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 1421.19,
        bark: 10.836,
        threshold_db: 1.97,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 1464.26,
        bark: 11.037,
        threshold_db: 1.83,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 1507.32,
        bark: 11.232,
        threshold_db: 1.68,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 1550.39,
        bark: 11.421,
        threshold_db: 1.53,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 1593.46,
        bark: 11.605,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 1636.52,
        bark: 11.783,
        threshold_db: 1.23,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 1679.59,
        bark: 11.957,
        threshold_db: 1.07,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 1722.66,
        bark: 12.125,
        threshold_db: 0.9,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 1765.72,
        bark: 12.289,
        threshold_db: 0.74,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 1808.79,
        bark: 12.448,
        threshold_db: 0.56,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 1851.86,
        bark: 12.603,
        threshold_db: 0.39,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 1894.92,
        bark: 12.753,
        threshold_db: 0.21,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 1937.99,
        bark: 12.9,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 1981.05,
        bark: 13.042,
        threshold_db: -0.17,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 2024.12,
        bark: 13.181,
        threshold_db: -0.36,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 2067.19,
        bark: 13.317,
        threshold_db: -0.56,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 2153.32,
        bark: 13.578,
        threshold_db: -0.96,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 2239.45,
        bark: 13.826,
        threshold_db: -1.38,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 2325.59,
        bark: 14.062,
        threshold_db: -1.79,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 2411.72,
        bark: 14.288,
        threshold_db: -2.21,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 2497.85,
        bark: 14.504,
        threshold_db: -2.63,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 2583.98,
        bark: 14.711,
        threshold_db: -3.03,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 2670.12,
        bark: 14.909,
        threshold_db: -3.41,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 2756.25,
        bark: 15.1,
        threshold_db: -3.77,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 2842.38,
        bark: 15.284,
        threshold_db: -4.09,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 2928.52,
        bark: 15.46,
        threshold_db: -4.37,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 3014.65,
        bark: 15.631,
        threshold_db: -4.6,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 3100.78,
        bark: 15.796,
        threshold_db: -4.78,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 3186.91,
        bark: 15.955,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 3273.05,
        bark: 16.11,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 3359.18,
        bark: 16.26,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 3445.31,
        bark: 16.406,
        threshold_db: -4.92,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 3531.45,
        bark: 16.547,
        threshold_db: -4.81,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 3617.58,
        bark: 16.685,
        threshold_db: -4.65,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 3703.71,
        bark: 16.82,
        threshold_db: -4.43,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 3789.84,
        bark: 16.951,
        threshold_db: -4.17,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 3875.98,
        bark: 17.079,
        threshold_db: -3.87,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 3962.11,
        bark: 17.205,
        threshold_db: -3.54,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 4048.24,
        bark: 17.327,
        threshold_db: -3.19,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 4134.38,
        bark: 17.447,
        threshold_db: -2.82,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 4306.64,
        bark: 17.68,
        threshold_db: -2.06,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 4478.91,
        bark: 17.905,
        threshold_db: -1.32,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 4651.17,
        bark: 18.121,
        threshold_db: -0.64,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 4823.44,
        bark: 18.331,
        threshold_db: -0.04,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 4995.7,
        bark: 18.534,
        threshold_db: 0.47,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 5167.97,
        bark: 18.731,
        threshold_db: 0.89,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 5340.23,
        bark: 18.922,
        threshold_db: 1.23,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 5512.5,
        bark: 19.108,
        threshold_db: 1.51,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 5684.77,
        bark: 19.289,
        threshold_db: 1.74,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 5857.03,
        bark: 19.464,
        threshold_db: 1.93,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 6029.3,
        bark: 19.635,
        threshold_db: 2.11,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 6201.56,
        bark: 19.801,
        threshold_db: 2.28,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 6373.83,
        bark: 19.963,
        threshold_db: 2.46,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 6546.09,
        bark: 20.12,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 6718.36,
        bark: 20.273,
        threshold_db: 2.82,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 6890.63,
        bark: 20.421,
        threshold_db: 3.03,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 7062.89,
        bark: 20.565,
        threshold_db: 3.25,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 7235.16,
        bark: 20.705,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 7407.42,
        bark: 20.84,
        threshold_db: 3.74,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 7579.69,
        bark: 20.972,
        threshold_db: 4.02,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 7751.95,
        bark: 21.099,
        threshold_db: 4.32,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 7924.22,
        bark: 21.222,
        threshold_db: 4.64,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 8096.48,
        bark: 21.342,
        threshold_db: 4.98,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 8268.75,
        bark: 21.457,
        threshold_db: 5.35,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 8613.28,
        bark: 21.677,
        threshold_db: 6.15,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 8957.81,
        bark: 21.882,
        threshold_db: 7.07,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 9302.34,
        bark: 22.074,
        threshold_db: 8.1,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 9646.88,
        bark: 22.253,
        threshold_db: 9.25,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 9991.41,
        bark: 22.42,
        threshold_db: 10.54,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 10335.94,
        bark: 22.576,
        threshold_db: 11.97,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 10680.47,
        bark: 22.721,
        threshold_db: 13.56,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 11025.0,
        bark: 22.857,
        threshold_db: 15.31,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 11369.53,
        bark: 22.984,
        threshold_db: 17.23,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 11714.06,
        bark: 23.102,
        threshold_db: 19.34,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 12058.59,
        bark: 23.213,
        threshold_db: 21.64,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 12403.13,
        bark: 23.317,
        threshold_db: 24.15,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 12747.66,
        bark: 23.415,
        threshold_db: 26.88,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 13092.19,
        bark: 23.506,
        threshold_db: 29.84,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 13436.72,
        bark: 23.592,
        threshold_db: 33.05,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 13781.25,
        bark: 23.673,
        threshold_db: 36.52,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 14125.78,
        bark: 23.749,
        threshold_db: 40.25,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 14470.31,
        bark: 23.821,
        threshold_db: 44.27,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 14814.84,
        bark: 23.888,
        threshold_db: 48.59,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 15159.38,
        bark: 23.952,
        threshold_db: 53.22,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 15503.91,
        bark: 24.013,
        threshold_db: 58.18,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 15848.44,
        bark: 24.07,
        threshold_db: 63.49,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 16192.97,
        bark: 24.125,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 16537.5,
        bark: 24.176,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 16882.03,
        bark: 24.225,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 17226.56,
        bark: 24.271,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 17571.09,
        bark: 24.316,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 17915.63,
        bark: 24.358,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 18260.16,
        bark: 24.398,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 18604.69,
        bark: 24.436,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 440,
        frequency_hz: 18949.22,
        bark: 24.473,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 448,
        frequency_hz: 19293.75,
        bark: 24.508,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 456,
        frequency_hz: 19638.28,
        bark: 24.542,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 464,
        frequency_hz: 19982.81,
        bark: 24.574,
        threshold_db: 68.0,
    },
];

/// ISO/IEC 11172-3:1993 Annex D Table D.1f — Layer II
/// "Frequencies, critical band rates and absolute threshold",
/// Fs = 48000 Hz. 126 entries. Each entry carries the top FFT line of
/// its 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// plus the three printed value columns: `Frequency [Hz]`,
/// `Crit.Band Rate [z]` and `Absolute threshold [dB]` (all
/// transcribed from the staged CSV
/// `docs/audio/mp3/annex-d-table-D1f-threshold-48kHz-LayerII.csv`).
/// The FFT-line ranges come from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
pub static TABLE_D_1F_LTQ_LAYER_II_48: [LtqEntry; 126] = [
    LtqEntry {
        top_line_index: 1,
        frequency_hz: 46.88,
        bark: 0.463,
        threshold_db: 42.1,
    },
    LtqEntry {
        top_line_index: 2,
        frequency_hz: 93.75,
        bark: 0.925,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 3,
        frequency_hz: 140.63,
        bark: 1.385,
        threshold_db: 17.47,
    },
    LtqEntry {
        top_line_index: 4,
        frequency_hz: 187.5,
        bark: 1.842,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 5,
        frequency_hz: 234.38,
        bark: 2.295,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 6,
        frequency_hz: 281.25,
        bark: 2.742,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 7,
        frequency_hz: 328.13,
        bark: 3.184,
        threshold_db: 8.84,
    },
    LtqEntry {
        top_line_index: 8,
        frequency_hz: 375.0,
        bark: 3.618,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 9,
        frequency_hz: 421.88,
        bark: 4.045,
        threshold_db: 7.22,
    },
    LtqEntry {
        top_line_index: 10,
        frequency_hz: 468.75,
        bark: 4.463,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 11,
        frequency_hz: 515.63,
        bark: 4.872,
        threshold_db: 6.12,
    },
    LtqEntry {
        top_line_index: 12,
        frequency_hz: 562.5,
        bark: 5.272,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 13,
        frequency_hz: 609.38,
        bark: 5.661,
        threshold_db: 5.33,
    },
    LtqEntry {
        top_line_index: 14,
        frequency_hz: 656.25,
        bark: 6.041,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 15,
        frequency_hz: 703.13,
        bark: 6.411,
        threshold_db: 4.71,
    },
    LtqEntry {
        top_line_index: 16,
        frequency_hz: 750.0,
        bark: 6.77,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 17,
        frequency_hz: 796.88,
        bark: 7.119,
        threshold_db: 4.21,
    },
    LtqEntry {
        top_line_index: 18,
        frequency_hz: 843.75,
        bark: 7.457,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 19,
        frequency_hz: 890.63,
        bark: 7.785,
        threshold_db: 3.79,
    },
    LtqEntry {
        top_line_index: 20,
        frequency_hz: 937.5,
        bark: 8.103,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 21,
        frequency_hz: 984.38,
        bark: 8.41,
        threshold_db: 3.43,
    },
    LtqEntry {
        top_line_index: 22,
        frequency_hz: 1031.25,
        bark: 8.708,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 23,
        frequency_hz: 1078.13,
        bark: 8.996,
        threshold_db: 3.09,
    },
    LtqEntry {
        top_line_index: 24,
        frequency_hz: 1125.0,
        bark: 9.275,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 25,
        frequency_hz: 1171.88,
        bark: 9.544,
        threshold_db: 2.78,
    },
    LtqEntry {
        top_line_index: 26,
        frequency_hz: 1218.75,
        bark: 9.805,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 27,
        frequency_hz: 1265.63,
        bark: 10.057,
        threshold_db: 2.47,
    },
    LtqEntry {
        top_line_index: 28,
        frequency_hz: 1312.5,
        bark: 10.301,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 29,
        frequency_hz: 1359.38,
        bark: 10.537,
        threshold_db: 2.17,
    },
    LtqEntry {
        top_line_index: 30,
        frequency_hz: 1406.25,
        bark: 10.765,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 31,
        frequency_hz: 1453.13,
        bark: 10.986,
        threshold_db: 1.86,
    },
    LtqEntry {
        top_line_index: 32,
        frequency_hz: 1500.0,
        bark: 11.199,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 33,
        frequency_hz: 1546.88,
        bark: 11.406,
        threshold_db: 1.55,
    },
    LtqEntry {
        top_line_index: 34,
        frequency_hz: 1593.75,
        bark: 11.606,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 35,
        frequency_hz: 1640.63,
        bark: 11.8,
        threshold_db: 1.21,
    },
    LtqEntry {
        top_line_index: 36,
        frequency_hz: 1687.5,
        bark: 11.988,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 37,
        frequency_hz: 1734.38,
        bark: 12.17,
        threshold_db: 0.86,
    },
    LtqEntry {
        top_line_index: 38,
        frequency_hz: 1781.25,
        bark: 12.347,
        threshold_db: 0.67,
    },
    LtqEntry {
        top_line_index: 39,
        frequency_hz: 1828.13,
        bark: 12.518,
        threshold_db: 0.49,
    },
    LtqEntry {
        top_line_index: 40,
        frequency_hz: 1875.0,
        bark: 12.684,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 41,
        frequency_hz: 1921.88,
        bark: 12.845,
        threshold_db: 0.09,
    },
    LtqEntry {
        top_line_index: 42,
        frequency_hz: 1968.75,
        bark: 13.002,
        threshold_db: -0.11,
    },
    LtqEntry {
        top_line_index: 43,
        frequency_hz: 2015.63,
        bark: 13.154,
        threshold_db: -0.32,
    },
    LtqEntry {
        top_line_index: 44,
        frequency_hz: 2062.5,
        bark: 13.302,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 45,
        frequency_hz: 2109.38,
        bark: 13.446,
        threshold_db: -0.75,
    },
    LtqEntry {
        top_line_index: 46,
        frequency_hz: 2156.25,
        bark: 13.586,
        threshold_db: -0.97,
    },
    LtqEntry {
        top_line_index: 47,
        frequency_hz: 2203.13,
        bark: 13.723,
        threshold_db: -1.2,
    },
    LtqEntry {
        top_line_index: 48,
        frequency_hz: 2250.0,
        bark: 13.855,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 50,
        frequency_hz: 2343.75,
        bark: 14.111,
        threshold_db: -1.88,
    },
    LtqEntry {
        top_line_index: 52,
        frequency_hz: 2437.5,
        bark: 14.354,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 54,
        frequency_hz: 2531.25,
        bark: 14.585,
        threshold_db: -2.79,
    },
    LtqEntry {
        top_line_index: 56,
        frequency_hz: 2625.0,
        bark: 14.807,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 58,
        frequency_hz: 2718.75,
        bark: 15.018,
        threshold_db: -3.62,
    },
    LtqEntry {
        top_line_index: 60,
        frequency_hz: 2812.5,
        bark: 15.221,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 62,
        frequency_hz: 2906.25,
        bark: 15.415,
        threshold_db: -4.3,
    },
    LtqEntry {
        top_line_index: 64,
        frequency_hz: 3000.0,
        bark: 15.602,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 66,
        frequency_hz: 3093.75,
        bark: 15.783,
        threshold_db: -4.77,
    },
    LtqEntry {
        top_line_index: 68,
        frequency_hz: 3187.5,
        bark: 15.956,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 70,
        frequency_hz: 3281.25,
        bark: 16.124,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 72,
        frequency_hz: 3375.0,
        bark: 16.287,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 74,
        frequency_hz: 3468.75,
        bark: 16.445,
        threshold_db: -4.9,
    },
    LtqEntry {
        top_line_index: 76,
        frequency_hz: 3562.5,
        bark: 16.598,
        threshold_db: -4.76,
    },
    LtqEntry {
        top_line_index: 78,
        frequency_hz: 3656.25,
        bark: 16.746,
        threshold_db: -4.55,
    },
    LtqEntry {
        top_line_index: 80,
        frequency_hz: 3750.0,
        bark: 16.891,
        threshold_db: -4.29,
    },
    LtqEntry {
        top_line_index: 82,
        frequency_hz: 3843.75,
        bark: 17.032,
        threshold_db: -3.99,
    },
    LtqEntry {
        top_line_index: 84,
        frequency_hz: 3937.5,
        bark: 17.169,
        threshold_db: -3.64,
    },
    LtqEntry {
        top_line_index: 86,
        frequency_hz: 4031.25,
        bark: 17.303,
        threshold_db: -3.26,
    },
    LtqEntry {
        top_line_index: 88,
        frequency_hz: 4125.0,
        bark: 17.434,
        threshold_db: -2.86,
    },
    LtqEntry {
        top_line_index: 90,
        frequency_hz: 4218.75,
        bark: 17.563,
        threshold_db: -2.45,
    },
    LtqEntry {
        top_line_index: 92,
        frequency_hz: 4312.5,
        bark: 17.688,
        threshold_db: -2.04,
    },
    LtqEntry {
        top_line_index: 94,
        frequency_hz: 4406.25,
        bark: 17.811,
        threshold_db: -1.63,
    },
    LtqEntry {
        top_line_index: 96,
        frequency_hz: 4500.0,
        bark: 17.932,
        threshold_db: -1.24,
    },
    LtqEntry {
        top_line_index: 100,
        frequency_hz: 4687.5,
        bark: 18.166,
        threshold_db: -0.51,
    },
    LtqEntry {
        top_line_index: 104,
        frequency_hz: 4875.0,
        bark: 18.392,
        threshold_db: 0.12,
    },
    LtqEntry {
        top_line_index: 108,
        frequency_hz: 5062.5,
        bark: 18.611,
        threshold_db: 0.64,
    },
    LtqEntry {
        top_line_index: 112,
        frequency_hz: 5250.0,
        bark: 18.823,
        threshold_db: 1.06,
    },
    LtqEntry {
        top_line_index: 116,
        frequency_hz: 5437.5,
        bark: 19.028,
        threshold_db: 1.39,
    },
    LtqEntry {
        top_line_index: 120,
        frequency_hz: 5625.0,
        bark: 19.226,
        threshold_db: 1.66,
    },
    LtqEntry {
        top_line_index: 124,
        frequency_hz: 5812.5,
        bark: 19.419,
        threshold_db: 1.88,
    },
    LtqEntry {
        top_line_index: 128,
        frequency_hz: 6000.0,
        bark: 19.606,
        threshold_db: 2.08,
    },
    LtqEntry {
        top_line_index: 132,
        frequency_hz: 6187.5,
        bark: 19.788,
        threshold_db: 2.27,
    },
    LtqEntry {
        top_line_index: 136,
        frequency_hz: 6375.0,
        bark: 19.964,
        threshold_db: 2.46,
    },
    LtqEntry {
        top_line_index: 140,
        frequency_hz: 6562.5,
        bark: 20.135,
        threshold_db: 2.65,
    },
    LtqEntry {
        top_line_index: 144,
        frequency_hz: 6750.0,
        bark: 20.3,
        threshold_db: 2.86,
    },
    LtqEntry {
        top_line_index: 148,
        frequency_hz: 6937.5,
        bark: 20.461,
        threshold_db: 3.09,
    },
    LtqEntry {
        top_line_index: 152,
        frequency_hz: 7125.0,
        bark: 20.616,
        threshold_db: 3.33,
    },
    LtqEntry {
        top_line_index: 156,
        frequency_hz: 7312.5,
        bark: 20.766,
        threshold_db: 3.6,
    },
    LtqEntry {
        top_line_index: 160,
        frequency_hz: 7500.0,
        bark: 20.912,
        threshold_db: 3.89,
    },
    LtqEntry {
        top_line_index: 164,
        frequency_hz: 7687.5,
        bark: 21.052,
        threshold_db: 4.2,
    },
    LtqEntry {
        top_line_index: 168,
        frequency_hz: 7875.0,
        bark: 21.188,
        threshold_db: 4.54,
    },
    LtqEntry {
        top_line_index: 172,
        frequency_hz: 8062.5,
        bark: 21.318,
        threshold_db: 4.91,
    },
    LtqEntry {
        top_line_index: 176,
        frequency_hz: 8250.0,
        bark: 21.445,
        threshold_db: 5.31,
    },
    LtqEntry {
        top_line_index: 180,
        frequency_hz: 8437.5,
        bark: 21.567,
        threshold_db: 5.73,
    },
    LtqEntry {
        top_line_index: 184,
        frequency_hz: 8625.0,
        bark: 21.684,
        threshold_db: 6.18,
    },
    LtqEntry {
        top_line_index: 188,
        frequency_hz: 8812.5,
        bark: 21.797,
        threshold_db: 6.67,
    },
    LtqEntry {
        top_line_index: 192,
        frequency_hz: 9000.0,
        bark: 21.906,
        threshold_db: 7.19,
    },
    LtqEntry {
        top_line_index: 200,
        frequency_hz: 9375.0,
        bark: 22.113,
        threshold_db: 8.33,
    },
    LtqEntry {
        top_line_index: 208,
        frequency_hz: 9750.0,
        bark: 22.304,
        threshold_db: 9.63,
    },
    LtqEntry {
        top_line_index: 216,
        frequency_hz: 10125.0,
        bark: 22.482,
        threshold_db: 11.08,
    },
    LtqEntry {
        top_line_index: 224,
        frequency_hz: 10500.0,
        bark: 22.646,
        threshold_db: 12.71,
    },
    LtqEntry {
        top_line_index: 232,
        frequency_hz: 10875.0,
        bark: 22.799,
        threshold_db: 14.53,
    },
    LtqEntry {
        top_line_index: 240,
        frequency_hz: 11250.0,
        bark: 22.941,
        threshold_db: 16.54,
    },
    LtqEntry {
        top_line_index: 248,
        frequency_hz: 11625.0,
        bark: 23.072,
        threshold_db: 18.77,
    },
    LtqEntry {
        top_line_index: 256,
        frequency_hz: 12000.0,
        bark: 23.195,
        threshold_db: 21.23,
    },
    LtqEntry {
        top_line_index: 264,
        frequency_hz: 12375.0,
        bark: 23.309,
        threshold_db: 23.94,
    },
    LtqEntry {
        top_line_index: 272,
        frequency_hz: 12750.0,
        bark: 23.415,
        threshold_db: 26.9,
    },
    LtqEntry {
        top_line_index: 280,
        frequency_hz: 13125.0,
        bark: 23.515,
        threshold_db: 30.14,
    },
    LtqEntry {
        top_line_index: 288,
        frequency_hz: 13500.0,
        bark: 23.607,
        threshold_db: 33.67,
    },
    LtqEntry {
        top_line_index: 296,
        frequency_hz: 13875.0,
        bark: 23.694,
        threshold_db: 37.51,
    },
    LtqEntry {
        top_line_index: 304,
        frequency_hz: 14250.0,
        bark: 23.775,
        threshold_db: 41.67,
    },
    LtqEntry {
        top_line_index: 312,
        frequency_hz: 14625.0,
        bark: 23.852,
        threshold_db: 46.17,
    },
    LtqEntry {
        top_line_index: 320,
        frequency_hz: 15000.0,
        bark: 23.923,
        threshold_db: 51.04,
    },
    LtqEntry {
        top_line_index: 328,
        frequency_hz: 15375.0,
        bark: 23.991,
        threshold_db: 56.29,
    },
    LtqEntry {
        top_line_index: 336,
        frequency_hz: 15750.0,
        bark: 24.054,
        threshold_db: 61.94,
    },
    LtqEntry {
        top_line_index: 344,
        frequency_hz: 16125.0,
        bark: 24.114,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 352,
        frequency_hz: 16500.0,
        bark: 24.171,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 360,
        frequency_hz: 16875.0,
        bark: 24.224,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 368,
        frequency_hz: 17250.0,
        bark: 24.275,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 376,
        frequency_hz: 17625.0,
        bark: 24.322,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 384,
        frequency_hz: 18000.0,
        bark: 24.368,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 392,
        frequency_hz: 18375.0,
        bark: 24.411,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 400,
        frequency_hz: 18750.0,
        bark: 24.452,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 408,
        frequency_hz: 19125.0,
        bark: 24.491,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 416,
        frequency_hz: 19500.0,
        bark: 24.528,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 424,
        frequency_hz: 19875.0,
        bark: 24.564,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 432,
        frequency_hz: 20250.0,
        bark: 24.597,
        threshold_db: 68.0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn d1_frequency_column_matches_line_grid() {
        // Every D.1 entry's printed `Frequency [Hz]` must equal its
        // FFT line times the analysis-FFT resolution Fs/1024, to the
        // table's two-decimal print precision — the §D.1 Step 1
        // "Frequency resolution Fs/1024" grid.
        for (fs_hz, table) in [
            (32_000.0, SamplingRate::Fs32kHz.ltq_table_layer2()),
            (44_100.0, SamplingRate::Fs44k1Hz.ltq_table_layer2()),
            (48_000.0, SamplingRate::Fs48kHz.ltq_table_layer2()),
        ] {
            for entry in table {
                let expect = entry.top_line_index as f64 * fs_hz / 1024.0;
                assert!(
                    (entry.frequency_hz - expect).abs() < 0.011,
                    "Fs={fs_hz}: line {} printed {} Hz vs grid {} Hz",
                    entry.top_line_index,
                    entry.frequency_hz,
                    expect,
                );
            }
        }
    }

    #[test]
    fn d1_bark_column_strictly_increasing() {
        // The `Crit.Band Rate [z]` column is a monotone function of
        // frequency; a transposition or mis-keyed row would break it.
        for table in [
            SamplingRate::Fs32kHz.ltq_table_layer2(),
            SamplingRate::Fs44k1Hz.ltq_table_layer2(),
            SamplingRate::Fs48kHz.ltq_table_layer2(),
        ] {
            for w in table.windows(2) {
                assert!(
                    w[0].bark < w[1].bark,
                    "bark not strictly increasing: {} >= {}",
                    w[0].bark,
                    w[1].bark,
                );
            }
        }
    }

    #[test]
    fn d2_boundary_rows_match_d1_entries() {
        // The D.2 `index F&CB` column indexes the D.1 table of the
        // same rate (1-based): the boundary's printed frequency and
        // Bark must be exactly the D.1 row's frequency and Bark. This
        // pins the *index domain* of the boundary tables — the stored
        // `top_line_index` is a D.1 subsampled index `i`, NOT a raw
        // FFT line (they only coincide below the first subsampling
        // break).
        for (fs, boundaries) in [
            (SamplingRate::Fs32kHz, &TABLE_D_2D_LAYER_II_32KHZ[..]),
            (SamplingRate::Fs44k1Hz, &TABLE_D_2E_LAYER_II_44K1HZ[..]),
            (SamplingRate::Fs48kHz, &TABLE_D_2F_LAYER_II_48KHZ[..]),
        ] {
            let d1 = fs.ltq_table_layer2();
            for b in boundaries {
                let entry = d1[(b.top_line_index - 1) as usize];
                assert!(
                    (entry.frequency_hz - b.top_frequency_hz).abs() < 0.011,
                    "{fs:?}: D.2 index {} freq {} vs D.1 {}",
                    b.top_line_index,
                    b.top_frequency_hz,
                    entry.frequency_hz,
                );
                assert!(
                    (entry.bark - b.top_bark).abs() < 0.0011,
                    "{fs:?}: D.2 index {} bark {} vs D.1 {}",
                    b.top_line_index,
                    b.top_bark,
                    entry.bark,
                );
            }
        }
    }

    #[test]
    fn table_d2d_row_count_matches_spec() {
        // The spec column heading prints "no 0..24" for Layer II
        // 32 kHz — 25 entries.
        assert_eq!(TABLE_D_2D_LAYER_II_32KHZ.len(), 25);
    }

    #[test]
    fn table_d2e_row_count_matches_spec() {
        // "no 0..26" — 27 entries.
        assert_eq!(TABLE_D_2E_LAYER_II_44K1HZ.len(), 27);
    }

    #[test]
    fn table_d2f_row_count_matches_spec() {
        // "no 0..26" — 27 entries.
        assert_eq!(TABLE_D_2F_LAYER_II_48KHZ.len(), 27);
    }

    #[test]
    fn boundary_indices_strictly_increasing() {
        // A critical-band-boundary table must be a strictly
        // ascending sequence of top-FFT-line indices — otherwise
        // the band assignment is ambiguous. Verify across all
        // three Layer II tables.
        for table in [
            &TABLE_D_2D_LAYER_II_32KHZ[..],
            &TABLE_D_2E_LAYER_II_44K1HZ[..],
            &TABLE_D_2F_LAYER_II_48KHZ[..],
        ] {
            for window in table.windows(2) {
                assert!(
                    window[0].top_line_index < window[1].top_line_index,
                    "non-monotone top_line_index: {} >= {}",
                    window[0].top_line_index,
                    window[1].top_line_index,
                );
            }
        }
    }

    #[test]
    fn boundary_frequencies_strictly_increasing() {
        // The frequency column is a strictly monotone scan up the
        // audio band — every successive boundary is at a higher
        // frequency than the previous one.
        for table in [
            &TABLE_D_2D_LAYER_II_32KHZ[..],
            &TABLE_D_2E_LAYER_II_44K1HZ[..],
            &TABLE_D_2F_LAYER_II_48KHZ[..],
        ] {
            for window in table.windows(2) {
                assert!(
                    window[0].top_frequency_hz < window[1].top_frequency_hz,
                    "non-monotone top_frequency_hz: {} >= {}",
                    window[0].top_frequency_hz,
                    window[1].top_frequency_hz,
                );
            }
        }
    }

    #[test]
    fn boundary_bark_strictly_increasing() {
        // The Bark column is also strictly monotone — Bark is a
        // monotone function of frequency.
        for table in [
            &TABLE_D_2D_LAYER_II_32KHZ[..],
            &TABLE_D_2E_LAYER_II_44K1HZ[..],
            &TABLE_D_2F_LAYER_II_48KHZ[..],
        ] {
            for window in table.windows(2) {
                assert!(
                    window[0].top_bark < window[1].top_bark,
                    "non-monotone top_bark: {} >= {}",
                    window[0].top_bark,
                    window[1].top_bark,
                );
            }
        }
    }

    #[test]
    fn first_band_starts_in_low_bark() {
        // Bark 0 corresponds to DC; the first critical band's top
        // line therefore sits below Bark 1 at every Layer II
        // sampling rate (the spec's first boundary is at index 1
        // for all three tables).
        assert!(TABLE_D_2D_LAYER_II_32KHZ[0].top_bark < 1.0);
        assert!(TABLE_D_2E_LAYER_II_44K1HZ[0].top_bark < 1.0);
        assert!(TABLE_D_2F_LAYER_II_48KHZ[0].top_bark < 1.0);
    }

    #[test]
    fn top_band_covers_audio_band_upper_edge() {
        // The Bark scale tops out around 24-25 Bark across the audio
        // band; the last boundary of each Layer II table is in that
        // neighbourhood (~23.9 Bark at 32 kHz; ~24.5-24.6 Bark at
        // 44,1 / 48 kHz, where the boundary reaches just under
        // 20 kHz).
        assert!(TABLE_D_2D_LAYER_II_32KHZ[24].top_bark > 23.0);
        assert!(TABLE_D_2E_LAYER_II_44K1HZ[26].top_bark > 24.0);
        assert!(TABLE_D_2F_LAYER_II_48KHZ[26].top_bark > 24.0);
    }

    #[test]
    fn sampling_rate_dispatch_picks_the_right_table() {
        assert_eq!(
            SamplingRate::Fs32kHz.critical_band_boundaries().len(),
            TABLE_D_2D_LAYER_II_32KHZ.len(),
        );
        assert_eq!(
            SamplingRate::Fs44k1Hz.critical_band_boundaries().len(),
            TABLE_D_2E_LAYER_II_44K1HZ.len(),
        );
        assert_eq!(
            SamplingRate::Fs48kHz.critical_band_boundaries().len(),
            TABLE_D_2F_LAYER_II_48KHZ.len(),
        );
        // The first row of each table is uniquely identifying — the
        // top-line frequency at index 0 differs across the three
        // sampling rates (~31.25 / 43.066 / 46.875 Hz).
        assert_eq!(
            SamplingRate::Fs32kHz.critical_band_boundaries()[0].top_line_index,
            1,
        );
        assert!(
            (SamplingRate::Fs44k1Hz.critical_band_boundaries()[0].top_frequency_hz - 43.066).abs()
                < 1.0e-6,
        );
        assert!(
            (SamplingRate::Fs48kHz.critical_band_boundaries()[0].top_frequency_hz - 46.875).abs()
                < 1.0e-6,
        );
    }

    // --- Annex D Table D.1d/e/f Layer II threshold-in-quiet -----

    #[test]
    fn ltq_table_row_counts_match_spec() {
        // The Layer II D.1 tables carry 132 / 130 / 126 entries at
        // 32 / 44,1 / 48 kHz (per the psychoacoustic extracts doc).
        assert_eq!(TABLE_D_1D_LTQ_LAYER_II_32.len(), 132);
        assert_eq!(TABLE_D_1E_LTQ_LAYER_II_44_1HZ.len(), 130);
        assert_eq!(TABLE_D_1F_LTQ_LAYER_II_48.len(), 126);
    }

    #[test]
    fn ltq_line_ranges_are_contiguous_and_start_at_one() {
        // The implicit lower bound of entry i is entry (i-1)'s
        // top_line_index + 1, and the first entry's range starts at
        // line 1 (top_line_index >= 1). The ranges must therefore be
        // strictly increasing in top_line_index with no gaps.
        for table in [
            &TABLE_D_1D_LTQ_LAYER_II_32[..],
            &TABLE_D_1E_LTQ_LAYER_II_44_1HZ[..],
            &TABLE_D_1F_LTQ_LAYER_II_48[..],
        ] {
            assert_eq!(table[0].top_line_index, 1);
            for window in table.windows(2) {
                assert!(
                    window[1].top_line_index > window[0].top_line_index,
                    "non-monotone LTq top_line_index: {} >= {}",
                    window[0].top_line_index,
                    window[1].top_line_index,
                );
            }
        }
    }

    #[test]
    fn ltq_anchor_cells_match_spec() {
        // First row at each rate (lowest line, high threshold) and the
        // 32 kHz minimum-region cell. D.1d i=1 = 58.23 dB; D.1d i=2 =
        // 33.44 dB (the 62.5 Hz cell shared with the Layer I D.1a
        // orientation anchor). The 44,1 / 48 kHz first cells are the
        // distinct low-frequency thresholds 45.05 / 42.10 dB.
        assert!((TABLE_D_1D_LTQ_LAYER_II_32[0].threshold_db - 58.23).abs() < 1e-9);
        assert!((TABLE_D_1D_LTQ_LAYER_II_32[1].threshold_db - 33.44).abs() < 1e-9);
        assert!((TABLE_D_1E_LTQ_LAYER_II_44_1HZ[0].threshold_db - 45.05).abs() < 1e-9);
        assert!((TABLE_D_1F_LTQ_LAYER_II_48[0].threshold_db - 42.10).abs() < 1e-9);
    }

    #[test]
    fn ltq_uses_d1_thresholds_not_d4_errata_cells() {
        // The spec Step 5(a) cites D.1d/e/f, NOT the Model-2 D.4
        // tables. The two diverge at documented last-digit / ceiling
        // cells. The 32 kHz top entry (i = 132) is D.1d = 51.04 dB,
        // whereas the Model-2 D.4a top cell prints 51.03 dB — this
        // table must carry the D.1d value.
        assert!((TABLE_D_1D_LTQ_LAYER_II_32[131].threshold_db - 51.04).abs() < 1e-9);
        // The 44,1 kHz top entry (i = 130) is D.1e = 68.00 dB (the
        // Model-2 D.4b table caps at the surprising 69.13 dB ceiling
        // instead — this table must NOT carry 69.13).
        assert!((TABLE_D_1E_LTQ_LAYER_II_44_1HZ[129].threshold_db - 68.00).abs() < 1e-9);
        // The 48 kHz table agrees with D.4c entry-for-entry; top = 68.00.
        assert!((TABLE_D_1F_LTQ_LAYER_II_48[125].threshold_db - 68.00).abs() < 1e-9);
    }

    #[test]
    fn ltq_dispatch_picks_the_right_table() {
        assert_eq!(
            SamplingRate::Fs32kHz.ltq_table_layer2().len(),
            TABLE_D_1D_LTQ_LAYER_II_32.len(),
        );
        assert_eq!(
            SamplingRate::Fs44k1Hz.ltq_table_layer2().len(),
            TABLE_D_1E_LTQ_LAYER_II_44_1HZ.len(),
        );
        assert_eq!(
            SamplingRate::Fs48kHz.ltq_table_layer2().len(),
            TABLE_D_1F_LTQ_LAYER_II_48.len(),
        );
    }
}
