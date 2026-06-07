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
}

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
}
