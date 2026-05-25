//! Numeric tables read directly from ISO/IEC 11172-3 (1993) Annex B.
//!
//! Clean-room: every value here is the literal Annex B PDF reading,
//! transcribed under the workspace clean-room policy from the staged
//! PDF (`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`) and the
//! markdown extract `docs/audio/mp3/mp1-annex-b-iso-extracts.md`. No
//! third-party MP2 source was consulted.
//!
//! Currently this module carries Table 3-B.1 ("Layer I, II scalefactors").
//! The remaining Layer II tables (3-B.2a..d "Possible quantization per
//! subband", 3-B.3 "Coefficients D[i] of the synthesis window", 3-B.4
//! "Layer II classes of quantization") are staged in the same documents
//! and will be added in subsequent rounds alongside the corresponding
//! decode steps.

/// Number of valid scalefactor indices defined by Annex B Table 3-B.1.
///
/// The §2.4.2.5 / §2.4.2.6 `scalefactor` field is 6 bits wide so 64
/// values are encodable, but only 63 are defined (indices 0..62);
/// index 63 is reserved and a §2.4.2.3-compliant encoder MUST NOT
/// emit it.
pub const SCALEFACTOR_COUNT: usize = 63;

/// Annex B Table 3-B.1 "Layer I, II scalefactors" — the 63 multiplier
/// values selected by a 6-bit `scalefactor` index per §2.4.2.5 /
/// §2.4.2.6 / §2.4.3.3.3.
///
/// All 63 values follow the closed-form relation
/// `scalefactor[i] = 2^((3 − i) / 3)` (PDF page 51; index 0 = 2, index 3
/// = 1, index 6 = 1/2, …). Note: the markdown extract
/// `docs/audio/mp3/mp1-annex-b-iso-extracts.md` documents this relation
/// as `2^((1 − i) / 3)` — that formula does not reproduce the tabulated
/// values (it gives `scalefactor[0] ≈ 1.260` instead of the tabulated
/// `2.000`). The PDF table itself is correct; the markdown's verifier
/// formula is off by 2 in the exponent numerator. See round-126 report.
pub const SCALEFACTORS: [f64; SCALEFACTOR_COUNT] = [
    2.000_000_000_000_00,
    1.587_401_051_968_20,
    1.259_921_049_894_87,
    1.000_000_000_000_00,
    0.793_700_525_984_10,
    0.629_960_524_947_44,
    0.500_000_000_000_00,
    0.396_850_262_992_05,
    0.314_980_262_473_72,
    0.250_000_000_000_00,
    0.198_425_131_496_02,
    0.157_490_131_236_86,
    0.125_000_000_000_00,
    0.099_212_565_748_01,
    0.078_745_065_618_43,
    0.062_500_000_000_00,
    0.049_606_282_874_01,
    0.039_372_532_809_21,
    0.031_250_000_000_00,
    0.024_803_141_437_00,
    0.019_686_266_404_61,
    0.015_625_000_000_00,
    0.012_401_570_718_50,
    0.009_843_133_202_30,
    0.007_812_500_000_00,
    0.006_200_785_359_25,
    0.004_921_566_601_15,
    0.003_906_250_000_00,
    0.003_100_392_679_63,
    0.002_460_783_300_58,
    0.001_953_125_000_00,
    0.001_550_196_339_81,
    0.001_230_391_650_29,
    0.000_976_562_500_00,
    0.000_775_098_169_91,
    0.000_615_195_825_14,
    0.000_488_281_250_00,
    0.000_387_549_084_95,
    0.000_307_597_912_57,
    0.000_244_140_625_00,
    0.000_193_774_542_48,
    0.000_153_798_956_29,
    0.000_122_070_312_50,
    0.000_096_887_271_24,
    0.000_076_899_478_14,
    0.000_061_035_156_25,
    0.000_048_443_635_62,
    0.000_038_449_739_07,
    0.000_030_517_578_13,
    0.000_024_221_817_81,
    0.000_019_224_869_54,
    0.000_015_258_789_06,
    0.000_012_110_908_90,
    0.000_009_612_434_77,
    0.000_007_629_394_53,
    0.000_006_055_454_45,
    0.000_004_806_217_38,
    0.000_003_814_697_27,
    0.000_003_027_727_23,
    0.000_002_403_108_69,
    0.000_001_907_348_63,
    0.000_001_513_863_61,
    0.000_001_201_554_35,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalefactor_table_has_the_expected_size() {
        assert_eq!(SCALEFACTORS.len(), SCALEFACTOR_COUNT);
        assert_eq!(SCALEFACTOR_COUNT, 63);
    }

    #[test]
    fn scalefactor_endpoints_match_pdf() {
        // PDF page 51 cross-checks: index 0 = 2,00000000000000;
        // index 3 = 1; index 62 = 0,00000120155435.
        assert_eq!(SCALEFACTORS[0], 2.0);
        assert_eq!(SCALEFACTORS[3], 1.0);
        // The 62-th entry is rounded at 14 decimal digits in the PDF.
        assert!((SCALEFACTORS[62] - 1.201_554_35e-6).abs() < 1e-15);
    }

    #[test]
    fn scalefactor_values_satisfy_closed_form() {
        // Correct closed form: scalefactor[i] = 2 ** ((3 - i) / 3).
        // Verified against the tabulated endpoints: index 0 = 2.0,
        // index 3 = 1.0, index 62 ≈ 1.2015e-6.
        for (i, &got) in SCALEFACTORS.iter().enumerate() {
            let expected = 2.0_f64.powf((3.0 - i as f64) / 3.0);
            // The PDF tabulates ~14 decimal digits, so the relative
            // tolerance reflects that quantization precision.
            let rel = (got - expected).abs() / expected;
            assert!(
                rel < 1e-7,
                "scalefactor[{i}] = {got} vs expected {expected} (rel {rel:e})"
            );
        }
    }

    #[test]
    fn scalefactor_values_are_strictly_decreasing() {
        for i in 1..SCALEFACTOR_COUNT {
            assert!(
                SCALEFACTORS[i] < SCALEFACTORS[i - 1],
                "scalefactor[{}] = {} should be less than [{}] = {}",
                i,
                SCALEFACTORS[i],
                i - 1,
                SCALEFACTORS[i - 1]
            );
        }
    }

    #[test]
    fn scalefactor_powers_of_two_are_exact() {
        // Every third index is an exact power of two and should be
        // stored to bit-exact double precision.
        let powers = [(0usize, 2.0), (3, 1.0), (6, 0.5), (9, 0.25), (12, 0.125)];
        for (i, expected) in powers {
            assert_eq!(SCALEFACTORS[i], expected);
        }
    }
}
