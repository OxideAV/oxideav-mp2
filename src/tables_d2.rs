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
/// threshold-in-quiet (absolute threshold) table.
///
/// The spec tabulates, per index `i`, the frequency, critical-band
/// rate and the absolute threshold `LTq` in dB. For the §D.1 Step 5(a)
/// "threshold-in-quiet decimation" the only two columns needed are the
/// FFT line each `i` covers and the threshold value, so this carrier
/// keeps:
///
/// * [`Self::top_line_index`] — the top 1024-point-analysis-FFT line of
///   the range covered by index `i`. The lower bound is implicit
///   (previous entry's `top_line_index + 1`; the first entry's range
///   starts at line 1). These ranges are the deterministic
///   `higher = round(frequency_Hz / (Fs/1024))` mapping (they coincide
///   with the Annex D Table D.4 line ranges at the same rate).
/// * [`Self::threshold_db`] — the `Absolute threshold [dB]` column of
///   the Layer II D.1 table (D.1d / D.1e / D.1f), *not* the Model-2
///   D.4 column (which diverges by the documented last-digit / ceiling
///   errata at 32 kHz and 44.1 kHz). Step 5(a) cites D.1d/e/f by name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LtqEntry {
    /// Top FFT-line index of the range this threshold entry covers.
    pub top_line_index: u32,
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
/// Band 17 carries an `[illegible]` Bark value in the staged PDF —
/// the printed cell renders `16,11` with a clipped final digit. By
/// the regular ~0,9–1,1-Bark spacing of the surrounding rows and the
/// matching D.1 index (62) the value is almost certainly **16,116**
/// (cf. Table D.2c band 16 = 16,124), but the last digit is not
/// physically legible in the PDF. This crate reproduces the
/// best-fit value `16.116`; the exact last digit is tracked as a
/// docs-collaborator follow-up (per the `[illegible]` annotation in
/// `docs/audio/mp3/mp3-annex-d-psychoacoustic-extracts.md` lines
/// 277-282).
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
    // Band 17: `16,11[illegible]` in PDF — best-fit `16.116` (see module-level note).
    CriticalBandBoundary {
        top_line_index: 62,
        top_frequency_hz: 3273.047,
        top_bark: 16.116,
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
/// threshold in quiet (absolute threshold) LTq, Fs = 32000 Hz.
/// 132 entries. Each entry carries the top FFT line of its
/// 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// and the threshold-in-quiet value in dB. Thresholds read from
/// the Table D.1d `Absolute threshold [dB]` column; the
/// FFT-line ranges from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
// One spec threshold value (6.28 dB) happens to coincide with the
// decimal expansion of TAU; it is a verbatim Annex D table cell, not a
// mathematical constant, so the approx_constant lint is suppressed.
#[allow(clippy::approx_constant)]
pub static TABLE_D_1D_LTQ_LAYER_II_32: [LtqEntry; 132] = [
    LtqEntry {
        top_line_index: 1,
        threshold_db: 58.23,
    },
    LtqEntry {
        top_line_index: 2,
        threshold_db: 33.44,
    },
    LtqEntry {
        top_line_index: 3,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 4,
        threshold_db: 19.2,
    },
    LtqEntry {
        top_line_index: 5,
        threshold_db: 16.05,
    },
    LtqEntry {
        top_line_index: 6,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 7,
        threshold_db: 12.26,
    },
    LtqEntry {
        top_line_index: 8,
        threshold_db: 11.01,
    },
    LtqEntry {
        top_line_index: 9,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 10,
        threshold_db: 9.2,
    },
    LtqEntry {
        top_line_index: 11,
        threshold_db: 8.52,
    },
    LtqEntry {
        top_line_index: 12,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 13,
        threshold_db: 7.44,
    },
    LtqEntry {
        top_line_index: 14,
        threshold_db: 7.0,
    },
    LtqEntry {
        top_line_index: 15,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 16,
        threshold_db: 6.28,
    },
    LtqEntry {
        top_line_index: 17,
        threshold_db: 5.97,
    },
    LtqEntry {
        top_line_index: 18,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 19,
        threshold_db: 5.44,
    },
    LtqEntry {
        top_line_index: 20,
        threshold_db: 5.21,
    },
    LtqEntry {
        top_line_index: 21,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 22,
        threshold_db: 4.8,
    },
    LtqEntry {
        top_line_index: 23,
        threshold_db: 4.62,
    },
    LtqEntry {
        top_line_index: 24,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 25,
        threshold_db: 4.29,
    },
    LtqEntry {
        top_line_index: 26,
        threshold_db: 4.14,
    },
    LtqEntry {
        top_line_index: 27,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 28,
        threshold_db: 3.86,
    },
    LtqEntry {
        top_line_index: 29,
        threshold_db: 3.73,
    },
    LtqEntry {
        top_line_index: 30,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 31,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 32,
        threshold_db: 3.37,
    },
    LtqEntry {
        top_line_index: 33,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 34,
        threshold_db: 3.15,
    },
    LtqEntry {
        top_line_index: 35,
        threshold_db: 3.04,
    },
    LtqEntry {
        top_line_index: 36,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 37,
        threshold_db: 2.83,
    },
    LtqEntry {
        top_line_index: 38,
        threshold_db: 2.73,
    },
    LtqEntry {
        top_line_index: 39,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 40,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 41,
        threshold_db: 2.42,
    },
    LtqEntry {
        top_line_index: 42,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 43,
        threshold_db: 2.22,
    },
    LtqEntry {
        top_line_index: 44,
        threshold_db: 2.12,
    },
    LtqEntry {
        top_line_index: 45,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 46,
        threshold_db: 1.92,
    },
    LtqEntry {
        top_line_index: 47,
        threshold_db: 1.81,
    },
    LtqEntry {
        top_line_index: 48,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 50,
        threshold_db: 1.49,
    },
    LtqEntry {
        top_line_index: 52,
        threshold_db: 1.27,
    },
    LtqEntry {
        top_line_index: 54,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 56,
        threshold_db: 0.8,
    },
    LtqEntry {
        top_line_index: 58,
        threshold_db: 0.55,
    },
    LtqEntry {
        top_line_index: 60,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 62,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 64,
        threshold_db: -0.25,
    },
    LtqEntry {
        top_line_index: 66,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 68,
        threshold_db: -0.83,
    },
    LtqEntry {
        top_line_index: 70,
        threshold_db: -1.12,
    },
    LtqEntry {
        top_line_index: 72,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 74,
        threshold_db: -1.73,
    },
    LtqEntry {
        top_line_index: 76,
        threshold_db: -2.04,
    },
    LtqEntry {
        top_line_index: 78,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 80,
        threshold_db: -2.64,
    },
    LtqEntry {
        top_line_index: 82,
        threshold_db: -2.93,
    },
    LtqEntry {
        top_line_index: 84,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 86,
        threshold_db: -3.49,
    },
    LtqEntry {
        top_line_index: 88,
        threshold_db: -3.74,
    },
    LtqEntry {
        top_line_index: 90,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 92,
        threshold_db: -4.2,
    },
    LtqEntry {
        top_line_index: 94,
        threshold_db: -4.4,
    },
    LtqEntry {
        top_line_index: 96,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 100,
        threshold_db: -4.82,
    },
    LtqEntry {
        top_line_index: 104,
        threshold_db: -4.96,
    },
    LtqEntry {
        top_line_index: 108,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 112,
        threshold_db: -4.86,
    },
    LtqEntry {
        top_line_index: 116,
        threshold_db: -4.63,
    },
    LtqEntry {
        top_line_index: 120,
        threshold_db: -4.29,
    },
    LtqEntry {
        top_line_index: 124,
        threshold_db: -3.87,
    },
    LtqEntry {
        top_line_index: 128,
        threshold_db: -3.39,
    },
    LtqEntry {
        top_line_index: 132,
        threshold_db: -2.86,
    },
    LtqEntry {
        top_line_index: 136,
        threshold_db: -2.31,
    },
    LtqEntry {
        top_line_index: 140,
        threshold_db: -1.77,
    },
    LtqEntry {
        top_line_index: 144,
        threshold_db: -1.24,
    },
    LtqEntry {
        top_line_index: 148,
        threshold_db: -0.74,
    },
    LtqEntry {
        top_line_index: 152,
        threshold_db: -0.29,
    },
    LtqEntry {
        top_line_index: 156,
        threshold_db: 0.12,
    },
    LtqEntry {
        top_line_index: 160,
        threshold_db: 0.48,
    },
    LtqEntry {
        top_line_index: 164,
        threshold_db: 0.79,
    },
    LtqEntry {
        top_line_index: 168,
        threshold_db: 1.06,
    },
    LtqEntry {
        top_line_index: 172,
        threshold_db: 1.29,
    },
    LtqEntry {
        top_line_index: 176,
        threshold_db: 1.49,
    },
    LtqEntry {
        top_line_index: 180,
        threshold_db: 1.66,
    },
    LtqEntry {
        top_line_index: 184,
        threshold_db: 1.81,
    },
    LtqEntry {
        top_line_index: 188,
        threshold_db: 1.95,
    },
    LtqEntry {
        top_line_index: 192,
        threshold_db: 2.08,
    },
    LtqEntry {
        top_line_index: 200,
        threshold_db: 2.33,
    },
    LtqEntry {
        top_line_index: 208,
        threshold_db: 2.59,
    },
    LtqEntry {
        top_line_index: 216,
        threshold_db: 2.86,
    },
    LtqEntry {
        top_line_index: 224,
        threshold_db: 3.17,
    },
    LtqEntry {
        top_line_index: 232,
        threshold_db: 3.51,
    },
    LtqEntry {
        top_line_index: 240,
        threshold_db: 3.89,
    },
    LtqEntry {
        top_line_index: 248,
        threshold_db: 4.31,
    },
    LtqEntry {
        top_line_index: 256,
        threshold_db: 4.79,
    },
    LtqEntry {
        top_line_index: 264,
        threshold_db: 5.31,
    },
    LtqEntry {
        top_line_index: 272,
        threshold_db: 5.88,
    },
    LtqEntry {
        top_line_index: 280,
        threshold_db: 6.5,
    },
    LtqEntry {
        top_line_index: 288,
        threshold_db: 7.19,
    },
    LtqEntry {
        top_line_index: 296,
        threshold_db: 7.93,
    },
    LtqEntry {
        top_line_index: 304,
        threshold_db: 8.75,
    },
    LtqEntry {
        top_line_index: 312,
        threshold_db: 9.63,
    },
    LtqEntry {
        top_line_index: 320,
        threshold_db: 10.58,
    },
    LtqEntry {
        top_line_index: 328,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 336,
        threshold_db: 12.71,
    },
    LtqEntry {
        top_line_index: 344,
        threshold_db: 13.9,
    },
    LtqEntry {
        top_line_index: 352,
        threshold_db: 15.18,
    },
    LtqEntry {
        top_line_index: 360,
        threshold_db: 16.54,
    },
    LtqEntry {
        top_line_index: 368,
        threshold_db: 18.01,
    },
    LtqEntry {
        top_line_index: 376,
        threshold_db: 19.57,
    },
    LtqEntry {
        top_line_index: 384,
        threshold_db: 21.23,
    },
    LtqEntry {
        top_line_index: 392,
        threshold_db: 23.01,
    },
    LtqEntry {
        top_line_index: 400,
        threshold_db: 24.9,
    },
    LtqEntry {
        top_line_index: 408,
        threshold_db: 26.9,
    },
    LtqEntry {
        top_line_index: 416,
        threshold_db: 29.03,
    },
    LtqEntry {
        top_line_index: 424,
        threshold_db: 31.28,
    },
    LtqEntry {
        top_line_index: 432,
        threshold_db: 33.67,
    },
    LtqEntry {
        top_line_index: 440,
        threshold_db: 36.19,
    },
    LtqEntry {
        top_line_index: 448,
        threshold_db: 38.86,
    },
    LtqEntry {
        top_line_index: 456,
        threshold_db: 41.67,
    },
    LtqEntry {
        top_line_index: 464,
        threshold_db: 44.63,
    },
    LtqEntry {
        top_line_index: 472,
        threshold_db: 47.76,
    },
    LtqEntry {
        top_line_index: 480,
        threshold_db: 51.04,
    },
];

/// ISO/IEC 11172-3:1993 Annex D Table D.1e — Layer II
/// threshold in quiet (absolute threshold) LTq, Fs = 44100 Hz.
/// 130 entries. Each entry carries the top FFT line of its
/// 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// and the threshold-in-quiet value in dB. Thresholds read from
/// the Table D.1e `Absolute threshold [dB]` column; the
/// FFT-line ranges from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
pub static TABLE_D_1E_LTQ_LAYER_II_44_1HZ: [LtqEntry; 130] = [
    LtqEntry {
        top_line_index: 1,
        threshold_db: 45.05,
    },
    LtqEntry {
        top_line_index: 2,
        threshold_db: 25.87,
    },
    LtqEntry {
        top_line_index: 3,
        threshold_db: 18.7,
    },
    LtqEntry {
        top_line_index: 4,
        threshold_db: 14.85,
    },
    LtqEntry {
        top_line_index: 5,
        threshold_db: 12.41,
    },
    LtqEntry {
        top_line_index: 6,
        threshold_db: 10.72,
    },
    LtqEntry {
        top_line_index: 7,
        threshold_db: 9.47,
    },
    LtqEntry {
        top_line_index: 8,
        threshold_db: 8.5,
    },
    LtqEntry {
        top_line_index: 9,
        threshold_db: 7.73,
    },
    LtqEntry {
        top_line_index: 10,
        threshold_db: 7.1,
    },
    LtqEntry {
        top_line_index: 11,
        threshold_db: 6.56,
    },
    LtqEntry {
        top_line_index: 12,
        threshold_db: 6.11,
    },
    LtqEntry {
        top_line_index: 13,
        threshold_db: 5.72,
    },
    LtqEntry {
        top_line_index: 14,
        threshold_db: 5.37,
    },
    LtqEntry {
        top_line_index: 15,
        threshold_db: 5.07,
    },
    LtqEntry {
        top_line_index: 16,
        threshold_db: 4.79,
    },
    LtqEntry {
        top_line_index: 17,
        threshold_db: 4.55,
    },
    LtqEntry {
        top_line_index: 18,
        threshold_db: 4.32,
    },
    LtqEntry {
        top_line_index: 19,
        threshold_db: 4.11,
    },
    LtqEntry {
        top_line_index: 20,
        threshold_db: 3.92,
    },
    LtqEntry {
        top_line_index: 21,
        threshold_db: 3.74,
    },
    LtqEntry {
        top_line_index: 22,
        threshold_db: 3.57,
    },
    LtqEntry {
        top_line_index: 23,
        threshold_db: 3.4,
    },
    LtqEntry {
        top_line_index: 24,
        threshold_db: 3.25,
    },
    LtqEntry {
        top_line_index: 25,
        threshold_db: 3.1,
    },
    LtqEntry {
        top_line_index: 26,
        threshold_db: 2.95,
    },
    LtqEntry {
        top_line_index: 27,
        threshold_db: 2.81,
    },
    LtqEntry {
        top_line_index: 28,
        threshold_db: 2.67,
    },
    LtqEntry {
        top_line_index: 29,
        threshold_db: 2.53,
    },
    LtqEntry {
        top_line_index: 30,
        threshold_db: 2.39,
    },
    LtqEntry {
        top_line_index: 31,
        threshold_db: 2.25,
    },
    LtqEntry {
        top_line_index: 32,
        threshold_db: 2.11,
    },
    LtqEntry {
        top_line_index: 33,
        threshold_db: 1.97,
    },
    LtqEntry {
        top_line_index: 34,
        threshold_db: 1.83,
    },
    LtqEntry {
        top_line_index: 35,
        threshold_db: 1.68,
    },
    LtqEntry {
        top_line_index: 36,
        threshold_db: 1.53,
    },
    LtqEntry {
        top_line_index: 37,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 38,
        threshold_db: 1.23,
    },
    LtqEntry {
        top_line_index: 39,
        threshold_db: 1.07,
    },
    LtqEntry {
        top_line_index: 40,
        threshold_db: 0.9,
    },
    LtqEntry {
        top_line_index: 41,
        threshold_db: 0.74,
    },
    LtqEntry {
        top_line_index: 42,
        threshold_db: 0.56,
    },
    LtqEntry {
        top_line_index: 43,
        threshold_db: 0.39,
    },
    LtqEntry {
        top_line_index: 44,
        threshold_db: 0.21,
    },
    LtqEntry {
        top_line_index: 45,
        threshold_db: 0.02,
    },
    LtqEntry {
        top_line_index: 46,
        threshold_db: -0.17,
    },
    LtqEntry {
        top_line_index: 47,
        threshold_db: -0.36,
    },
    LtqEntry {
        top_line_index: 48,
        threshold_db: -0.56,
    },
    LtqEntry {
        top_line_index: 50,
        threshold_db: -0.96,
    },
    LtqEntry {
        top_line_index: 52,
        threshold_db: -1.38,
    },
    LtqEntry {
        top_line_index: 54,
        threshold_db: -1.79,
    },
    LtqEntry {
        top_line_index: 56,
        threshold_db: -2.21,
    },
    LtqEntry {
        top_line_index: 58,
        threshold_db: -2.63,
    },
    LtqEntry {
        top_line_index: 60,
        threshold_db: -3.03,
    },
    LtqEntry {
        top_line_index: 62,
        threshold_db: -3.41,
    },
    LtqEntry {
        top_line_index: 64,
        threshold_db: -3.77,
    },
    LtqEntry {
        top_line_index: 66,
        threshold_db: -4.09,
    },
    LtqEntry {
        top_line_index: 68,
        threshold_db: -4.37,
    },
    LtqEntry {
        top_line_index: 70,
        threshold_db: -4.6,
    },
    LtqEntry {
        top_line_index: 72,
        threshold_db: -4.78,
    },
    LtqEntry {
        top_line_index: 74,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 76,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 78,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 80,
        threshold_db: -4.92,
    },
    LtqEntry {
        top_line_index: 82,
        threshold_db: -4.81,
    },
    LtqEntry {
        top_line_index: 84,
        threshold_db: -4.65,
    },
    LtqEntry {
        top_line_index: 86,
        threshold_db: -4.43,
    },
    LtqEntry {
        top_line_index: 88,
        threshold_db: -4.17,
    },
    LtqEntry {
        top_line_index: 90,
        threshold_db: -3.87,
    },
    LtqEntry {
        top_line_index: 92,
        threshold_db: -3.54,
    },
    LtqEntry {
        top_line_index: 94,
        threshold_db: -3.19,
    },
    LtqEntry {
        top_line_index: 96,
        threshold_db: -2.82,
    },
    LtqEntry {
        top_line_index: 100,
        threshold_db: -2.06,
    },
    LtqEntry {
        top_line_index: 104,
        threshold_db: -1.32,
    },
    LtqEntry {
        top_line_index: 108,
        threshold_db: -0.64,
    },
    LtqEntry {
        top_line_index: 112,
        threshold_db: -0.04,
    },
    LtqEntry {
        top_line_index: 116,
        threshold_db: 0.47,
    },
    LtqEntry {
        top_line_index: 120,
        threshold_db: 0.89,
    },
    LtqEntry {
        top_line_index: 124,
        threshold_db: 1.23,
    },
    LtqEntry {
        top_line_index: 128,
        threshold_db: 1.51,
    },
    LtqEntry {
        top_line_index: 132,
        threshold_db: 1.74,
    },
    LtqEntry {
        top_line_index: 136,
        threshold_db: 1.93,
    },
    LtqEntry {
        top_line_index: 140,
        threshold_db: 2.11,
    },
    LtqEntry {
        top_line_index: 144,
        threshold_db: 2.28,
    },
    LtqEntry {
        top_line_index: 148,
        threshold_db: 2.46,
    },
    LtqEntry {
        top_line_index: 152,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 156,
        threshold_db: 2.82,
    },
    LtqEntry {
        top_line_index: 160,
        threshold_db: 3.03,
    },
    LtqEntry {
        top_line_index: 164,
        threshold_db: 3.25,
    },
    LtqEntry {
        top_line_index: 168,
        threshold_db: 3.49,
    },
    LtqEntry {
        top_line_index: 172,
        threshold_db: 3.74,
    },
    LtqEntry {
        top_line_index: 176,
        threshold_db: 4.02,
    },
    LtqEntry {
        top_line_index: 180,
        threshold_db: 4.32,
    },
    LtqEntry {
        top_line_index: 184,
        threshold_db: 4.64,
    },
    LtqEntry {
        top_line_index: 188,
        threshold_db: 4.98,
    },
    LtqEntry {
        top_line_index: 192,
        threshold_db: 5.35,
    },
    LtqEntry {
        top_line_index: 200,
        threshold_db: 6.15,
    },
    LtqEntry {
        top_line_index: 208,
        threshold_db: 7.07,
    },
    LtqEntry {
        top_line_index: 216,
        threshold_db: 8.1,
    },
    LtqEntry {
        top_line_index: 224,
        threshold_db: 9.25,
    },
    LtqEntry {
        top_line_index: 232,
        threshold_db: 10.54,
    },
    LtqEntry {
        top_line_index: 240,
        threshold_db: 11.97,
    },
    LtqEntry {
        top_line_index: 248,
        threshold_db: 13.56,
    },
    LtqEntry {
        top_line_index: 256,
        threshold_db: 15.31,
    },
    LtqEntry {
        top_line_index: 264,
        threshold_db: 17.23,
    },
    LtqEntry {
        top_line_index: 272,
        threshold_db: 19.34,
    },
    LtqEntry {
        top_line_index: 280,
        threshold_db: 21.64,
    },
    LtqEntry {
        top_line_index: 288,
        threshold_db: 24.15,
    },
    LtqEntry {
        top_line_index: 296,
        threshold_db: 26.88,
    },
    LtqEntry {
        top_line_index: 304,
        threshold_db: 29.84,
    },
    LtqEntry {
        top_line_index: 312,
        threshold_db: 33.05,
    },
    LtqEntry {
        top_line_index: 320,
        threshold_db: 36.52,
    },
    LtqEntry {
        top_line_index: 328,
        threshold_db: 40.25,
    },
    LtqEntry {
        top_line_index: 336,
        threshold_db: 44.27,
    },
    LtqEntry {
        top_line_index: 344,
        threshold_db: 48.59,
    },
    LtqEntry {
        top_line_index: 352,
        threshold_db: 53.22,
    },
    LtqEntry {
        top_line_index: 360,
        threshold_db: 58.18,
    },
    LtqEntry {
        top_line_index: 368,
        threshold_db: 63.49,
    },
    LtqEntry {
        top_line_index: 376,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 384,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 392,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 400,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 408,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 416,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 424,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 432,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 440,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 448,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 456,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 464,
        threshold_db: 68.0,
    },
];

/// ISO/IEC 11172-3:1993 Annex D Table D.1f — Layer II
/// threshold in quiet (absolute threshold) LTq, Fs = 48000 Hz.
/// 126 entries. Each entry carries the top FFT line of its
/// 1024-point analysis-FFT line range (`index_higher`; the lower
/// bound is the previous entry's `top_line_index + 1`, first = 1)
/// and the threshold-in-quiet value in dB. Thresholds read from
/// the Table D.1f `Absolute threshold [dB]` column; the
/// FFT-line ranges from the deterministic
/// `higher = round(frequency_Hz / (Fs/1024))` mapping (matching the
/// Annex D Table D.4 line ranges at this rate).
pub static TABLE_D_1F_LTQ_LAYER_II_48: [LtqEntry; 126] = [
    LtqEntry {
        top_line_index: 1,
        threshold_db: 42.1,
    },
    LtqEntry {
        top_line_index: 2,
        threshold_db: 24.17,
    },
    LtqEntry {
        top_line_index: 3,
        threshold_db: 17.47,
    },
    LtqEntry {
        top_line_index: 4,
        threshold_db: 13.87,
    },
    LtqEntry {
        top_line_index: 5,
        threshold_db: 11.6,
    },
    LtqEntry {
        top_line_index: 6,
        threshold_db: 10.01,
    },
    LtqEntry {
        top_line_index: 7,
        threshold_db: 8.84,
    },
    LtqEntry {
        top_line_index: 8,
        threshold_db: 7.94,
    },
    LtqEntry {
        top_line_index: 9,
        threshold_db: 7.22,
    },
    LtqEntry {
        top_line_index: 10,
        threshold_db: 6.62,
    },
    LtqEntry {
        top_line_index: 11,
        threshold_db: 6.12,
    },
    LtqEntry {
        top_line_index: 12,
        threshold_db: 5.7,
    },
    LtqEntry {
        top_line_index: 13,
        threshold_db: 5.33,
    },
    LtqEntry {
        top_line_index: 14,
        threshold_db: 5.0,
    },
    LtqEntry {
        top_line_index: 15,
        threshold_db: 4.71,
    },
    LtqEntry {
        top_line_index: 16,
        threshold_db: 4.45,
    },
    LtqEntry {
        top_line_index: 17,
        threshold_db: 4.21,
    },
    LtqEntry {
        top_line_index: 18,
        threshold_db: 4.0,
    },
    LtqEntry {
        top_line_index: 19,
        threshold_db: 3.79,
    },
    LtqEntry {
        top_line_index: 20,
        threshold_db: 3.61,
    },
    LtqEntry {
        top_line_index: 21,
        threshold_db: 3.43,
    },
    LtqEntry {
        top_line_index: 22,
        threshold_db: 3.26,
    },
    LtqEntry {
        top_line_index: 23,
        threshold_db: 3.09,
    },
    LtqEntry {
        top_line_index: 24,
        threshold_db: 2.93,
    },
    LtqEntry {
        top_line_index: 25,
        threshold_db: 2.78,
    },
    LtqEntry {
        top_line_index: 26,
        threshold_db: 2.63,
    },
    LtqEntry {
        top_line_index: 27,
        threshold_db: 2.47,
    },
    LtqEntry {
        top_line_index: 28,
        threshold_db: 2.32,
    },
    LtqEntry {
        top_line_index: 29,
        threshold_db: 2.17,
    },
    LtqEntry {
        top_line_index: 30,
        threshold_db: 2.02,
    },
    LtqEntry {
        top_line_index: 31,
        threshold_db: 1.86,
    },
    LtqEntry {
        top_line_index: 32,
        threshold_db: 1.71,
    },
    LtqEntry {
        top_line_index: 33,
        threshold_db: 1.55,
    },
    LtqEntry {
        top_line_index: 34,
        threshold_db: 1.38,
    },
    LtqEntry {
        top_line_index: 35,
        threshold_db: 1.21,
    },
    LtqEntry {
        top_line_index: 36,
        threshold_db: 1.04,
    },
    LtqEntry {
        top_line_index: 37,
        threshold_db: 0.86,
    },
    LtqEntry {
        top_line_index: 38,
        threshold_db: 0.67,
    },
    LtqEntry {
        top_line_index: 39,
        threshold_db: 0.49,
    },
    LtqEntry {
        top_line_index: 40,
        threshold_db: 0.29,
    },
    LtqEntry {
        top_line_index: 41,
        threshold_db: 0.09,
    },
    LtqEntry {
        top_line_index: 42,
        threshold_db: -0.11,
    },
    LtqEntry {
        top_line_index: 43,
        threshold_db: -0.32,
    },
    LtqEntry {
        top_line_index: 44,
        threshold_db: -0.54,
    },
    LtqEntry {
        top_line_index: 45,
        threshold_db: -0.75,
    },
    LtqEntry {
        top_line_index: 46,
        threshold_db: -0.97,
    },
    LtqEntry {
        top_line_index: 47,
        threshold_db: -1.2,
    },
    LtqEntry {
        top_line_index: 48,
        threshold_db: -1.43,
    },
    LtqEntry {
        top_line_index: 50,
        threshold_db: -1.88,
    },
    LtqEntry {
        top_line_index: 52,
        threshold_db: -2.34,
    },
    LtqEntry {
        top_line_index: 54,
        threshold_db: -2.79,
    },
    LtqEntry {
        top_line_index: 56,
        threshold_db: -3.22,
    },
    LtqEntry {
        top_line_index: 58,
        threshold_db: -3.62,
    },
    LtqEntry {
        top_line_index: 60,
        threshold_db: -3.98,
    },
    LtqEntry {
        top_line_index: 62,
        threshold_db: -4.3,
    },
    LtqEntry {
        top_line_index: 64,
        threshold_db: -4.57,
    },
    LtqEntry {
        top_line_index: 66,
        threshold_db: -4.77,
    },
    LtqEntry {
        top_line_index: 68,
        threshold_db: -4.91,
    },
    LtqEntry {
        top_line_index: 70,
        threshold_db: -4.98,
    },
    LtqEntry {
        top_line_index: 72,
        threshold_db: -4.97,
    },
    LtqEntry {
        top_line_index: 74,
        threshold_db: -4.9,
    },
    LtqEntry {
        top_line_index: 76,
        threshold_db: -4.76,
    },
    LtqEntry {
        top_line_index: 78,
        threshold_db: -4.55,
    },
    LtqEntry {
        top_line_index: 80,
        threshold_db: -4.29,
    },
    LtqEntry {
        top_line_index: 82,
        threshold_db: -3.99,
    },
    LtqEntry {
        top_line_index: 84,
        threshold_db: -3.64,
    },
    LtqEntry {
        top_line_index: 86,
        threshold_db: -3.26,
    },
    LtqEntry {
        top_line_index: 88,
        threshold_db: -2.86,
    },
    LtqEntry {
        top_line_index: 90,
        threshold_db: -2.45,
    },
    LtqEntry {
        top_line_index: 92,
        threshold_db: -2.04,
    },
    LtqEntry {
        top_line_index: 94,
        threshold_db: -1.63,
    },
    LtqEntry {
        top_line_index: 96,
        threshold_db: -1.24,
    },
    LtqEntry {
        top_line_index: 100,
        threshold_db: -0.51,
    },
    LtqEntry {
        top_line_index: 104,
        threshold_db: 0.12,
    },
    LtqEntry {
        top_line_index: 108,
        threshold_db: 0.64,
    },
    LtqEntry {
        top_line_index: 112,
        threshold_db: 1.06,
    },
    LtqEntry {
        top_line_index: 116,
        threshold_db: 1.39,
    },
    LtqEntry {
        top_line_index: 120,
        threshold_db: 1.66,
    },
    LtqEntry {
        top_line_index: 124,
        threshold_db: 1.88,
    },
    LtqEntry {
        top_line_index: 128,
        threshold_db: 2.08,
    },
    LtqEntry {
        top_line_index: 132,
        threshold_db: 2.27,
    },
    LtqEntry {
        top_line_index: 136,
        threshold_db: 2.46,
    },
    LtqEntry {
        top_line_index: 140,
        threshold_db: 2.65,
    },
    LtqEntry {
        top_line_index: 144,
        threshold_db: 2.86,
    },
    LtqEntry {
        top_line_index: 148,
        threshold_db: 3.09,
    },
    LtqEntry {
        top_line_index: 152,
        threshold_db: 3.33,
    },
    LtqEntry {
        top_line_index: 156,
        threshold_db: 3.6,
    },
    LtqEntry {
        top_line_index: 160,
        threshold_db: 3.89,
    },
    LtqEntry {
        top_line_index: 164,
        threshold_db: 4.2,
    },
    LtqEntry {
        top_line_index: 168,
        threshold_db: 4.54,
    },
    LtqEntry {
        top_line_index: 172,
        threshold_db: 4.91,
    },
    LtqEntry {
        top_line_index: 176,
        threshold_db: 5.31,
    },
    LtqEntry {
        top_line_index: 180,
        threshold_db: 5.73,
    },
    LtqEntry {
        top_line_index: 184,
        threshold_db: 6.18,
    },
    LtqEntry {
        top_line_index: 188,
        threshold_db: 6.67,
    },
    LtqEntry {
        top_line_index: 192,
        threshold_db: 7.19,
    },
    LtqEntry {
        top_line_index: 200,
        threshold_db: 8.33,
    },
    LtqEntry {
        top_line_index: 208,
        threshold_db: 9.63,
    },
    LtqEntry {
        top_line_index: 216,
        threshold_db: 11.08,
    },
    LtqEntry {
        top_line_index: 224,
        threshold_db: 12.71,
    },
    LtqEntry {
        top_line_index: 232,
        threshold_db: 14.53,
    },
    LtqEntry {
        top_line_index: 240,
        threshold_db: 16.54,
    },
    LtqEntry {
        top_line_index: 248,
        threshold_db: 18.77,
    },
    LtqEntry {
        top_line_index: 256,
        threshold_db: 21.23,
    },
    LtqEntry {
        top_line_index: 264,
        threshold_db: 23.94,
    },
    LtqEntry {
        top_line_index: 272,
        threshold_db: 26.9,
    },
    LtqEntry {
        top_line_index: 280,
        threshold_db: 30.14,
    },
    LtqEntry {
        top_line_index: 288,
        threshold_db: 33.67,
    },
    LtqEntry {
        top_line_index: 296,
        threshold_db: 37.51,
    },
    LtqEntry {
        top_line_index: 304,
        threshold_db: 41.67,
    },
    LtqEntry {
        top_line_index: 312,
        threshold_db: 46.17,
    },
    LtqEntry {
        top_line_index: 320,
        threshold_db: 51.04,
    },
    LtqEntry {
        top_line_index: 328,
        threshold_db: 56.29,
    },
    LtqEntry {
        top_line_index: 336,
        threshold_db: 61.94,
    },
    LtqEntry {
        top_line_index: 344,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 352,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 360,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 368,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 376,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 384,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 392,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 400,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 408,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 416,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 424,
        threshold_db: 68.0,
    },
    LtqEntry {
        top_line_index: 432,
        threshold_db: 68.0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

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
