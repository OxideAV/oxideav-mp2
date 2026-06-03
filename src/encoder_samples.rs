//! §2.4.3.3.4 encoder-side sub-band sample quantization.
//!
//! ISO/IEC 11172-3 (1993) §2.4.3.3.4 defines the *decoder* mapping
//! from an `n`-bit raw sample code to a normalized fractional value
//! `s''` in three steps: (a) invert the MSB; (b) read the result as
//! an `n`-bit two's-complement number whose MSB weight is `-1`,
//! yielding a fractional `s''' ∈ [-1, 1 − 2^(1−n)]`; (c) apply the
//! linear formula `s'' = C · (s''' + D)` with the Table 3-B.4 `C`,
//! `D` constants. The encoder runs the same three steps backwards:
//! divide out the scalefactor multiplier (Table 3-B.1) to recover
//! `s''`, invert the linear formula to recover `s'''`, clamp into
//! the representable interval, re-encode as `n`-bit two's
//! complement, then re-invert the MSB.
//!
//! For grouped classes (`nb_steps ∈ {3, 5, 9}` per §2.4.2.3) three
//! consecutive `s'''` codes are packed into one codeword with the
//! radix-`nlevels` rule `combined = s[0] + nlevels·s[1] +
//! nlevels²·s[2]`, exactly the inverse of
//! [`crate::requant::degroup`]. For ungrouped classes (the
//! remaining 14 Table 3-B.4 rows) each sample is written as a
//! standalone `bits_per_codeword`-bit code (which for those rows
//! equals `bits_per_sample`).
//!
//! No third-party MP2 encoder source was consulted; the procedure
//! is the documented arithmetic inverse of the §2.4.3.3.4 decode
//! definition. The `C` and `D` constants come from Table 3-B.4
//! (PDF page 50), already tabulated in [`crate::bitalloc::QuantClass`].
//!
//! # Round-trip guarantee
//!
//! [`quantize_sample`] composed with [`crate::requant::requantize_code`]
//! is the encoder/decoder identity within the inherent quantization
//! step: feeding a value `s''` through `quantize_sample` then
//! `requantize_code` returns the nearest representable `s''` (the
//! quantizer's bin centre). Round-tripping the *quantized*
//! `s''` is exact; round-tripping an arbitrary `s''` introduces at
//! most `class.c / 2^(n-1)` of additive error (one quantization
//! step). The same property holds for [`write_triplet`] /
//! [`crate::requant::read_triplet`] and
//! [`write_triplet_scaled`] / [`crate::requant::requantize_scaled`].

use oxideav_core::bits::BitWriter;

use crate::bitalloc::QuantClass;
use crate::tables::{SCALEFACTORS, SCALEFACTOR_COUNT};

/// Errors raised by the §2.4.3.3.4 encoder sample writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleWriteError {
    /// A scalefactor index `≥ 63` was supplied to
    /// [`write_triplet_scaled`] or [`quantize_scaled`]; only indices
    /// `0..=62` select a Table 3-B.1 multiplier (index 63 is reserved
    /// per §2.4.2.5).
    ReservedScalefactorIndex(u8),
}

impl core::fmt::Display for SampleWriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SampleWriteError::ReservedScalefactorIndex(idx) => write!(
                f,
                "encoder_samples: scalefactor index {idx} is reserved (only 0..=62 are defined)"
            ),
        }
    }
}

impl std::error::Error for SampleWriteError {}

/// Quantize one normalized sample value into the `n`-bit raw code
/// the §2.4.3.3.4 decoder would consume for a given Table 3-B.4
/// class.
///
/// `s_double_prime` is the post-linear-formula value `s'' = C ·
/// (s''' + D)` (the same `s''` produced by
/// [`crate::requant::requantize_code`]). The returned code is the
/// `class.bits_per_sample()`-bit unsigned integer that, when fed
/// back through `requantize_code`, yields the closest representable
/// `s''` (the bin centre).
///
/// Out-of-range inputs are clamped to the representable interval
/// `[C · (−1 + D), C · (1 − 2^(1−n) + D)]` before quantization, so
/// the result is always in `0 ..= 2^n − 1` and never causes the
/// decoder to fault.
pub fn quantize_sample(class: &QuantClass, s_double_prime: f64) -> u32 {
    let n = class.bits_per_sample();
    debug_assert!((1..=16).contains(&n));

    // §2.4.3.3.4 linear formula inverse: s''' = s''/C − D.
    // (C is always strictly positive in Table 3-B.4.)
    let s_triple_prime = s_double_prime / class.c - class.d;

    // The decoder's s''' grid: with n-bit two's complement and MSB
    // weight −1, the integer k = s''' · 2^(n-1) ranges over the codes
    // that are actually consumed downstream.
    //
    // * For an ungrouped class, the codeword spans all 2^n raw codes,
    //   so k ∈ [−2^(n-1), 2^(n-1) − 1].
    // * For a grouped class, §2.4.3.3.4 degrouping yields three digits
    //   in `[0, nb_steps)`, so only the first `nb_steps` raw codes are
    //   in the image. Per `requantize_code`'s MSB-inversion table, raw
    //   codes 0 .. nb_steps − 1 correspond to k = −msb .. nb_steps −
    //   1 − msb. Clamping past `nb_steps − 1 − msb` would write a
    //   digit ≥ nb_steps and overflow the radix-nlevels pack.
    let msb = 1i64 << (n - 1);
    let k_max = if class.grouping {
        class.nb_steps as i64 - 1 - msb
    } else {
        msb - 1
    };
    let scaled = (s_triple_prime * msb as f64).round() as i64;
    // Clamp to the legal k range: out-of-range inputs cannot be
    // represented by any code; we map them to the nearest endpoint.
    let clamped = scaled.clamp(-msb, k_max);
    let k = clamped as i32;

    // Reduce k into [0, 2^n) as an unsigned n-bit two's complement
    // representation, then invert the MSB to undo §2.4.3.3.4 "the
    // first bit of each of the three codes has to be inverted". The
    // returned `code` is exactly what `requantize_code` would consume;
    // for a grouped class it is the radix-nlevels digit in
    // `[0, nb_steps)`, for an ungrouped class it is the n-bit raw
    // bitstream code.
    let mask: u32 = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
    let twos = (k as u32) & mask;
    let inverted_msb = (1u32 << (n - 1)) & mask;
    twos ^ inverted_msb
}

/// Quantize one sample after dividing out a §2.4.3.3.3 scalefactor.
///
/// `s_prime` is the rescaled sample value `s' = factor · s''` the
/// decoder would emit after Table 3-B.1 rescaling. The encoder
/// divides by the same Table 3-B.1 multiplier to recover `s''`,
/// then runs [`quantize_sample`].
///
/// `scalefactor_index` is the 6-bit Table 3-B.1 index the encoder
/// has chosen for this granule (typically from
/// [`crate::encoder_scalefactors::compute_scalefactors`]); index 63
/// is reserved per §2.4.2.5 and yields
/// [`SampleWriteError::ReservedScalefactorIndex`].
pub fn quantize_scaled(
    class: &QuantClass,
    scalefactor_index: u8,
    s_prime: f64,
) -> Result<u32, SampleWriteError> {
    if scalefactor_index as usize >= SCALEFACTOR_COUNT {
        return Err(SampleWriteError::ReservedScalefactorIndex(
            scalefactor_index,
        ));
    }
    let factor = SCALEFACTORS[scalefactor_index as usize];
    // factor > 0 for every defined Table 3-B.1 entry.
    let s_double_prime = s_prime / factor;
    Ok(quantize_sample(class, s_double_prime))
}

/// Pack three single-sample codes into one combined codeword per the
/// §2.4.3.3.4 grouping rule `combined = s[0] + nlevels·s[1] +
/// nlevels²·s[2]`. Inverse of [`crate::requant::degroup`].
///
/// Each input is expected to be in `0 ..= nb_steps − 1`. Values at
/// or above `nb_steps` would not be producible by [`quantize_sample`]
/// on a grouped class, so the function is a debug-asserted bijection
/// over the valid range.
fn group_combined(class: &QuantClass, codes: [u32; 3]) -> u32 {
    let nlevels = class.nb_steps;
    debug_assert!(class.grouping);
    debug_assert!(codes[0] < nlevels);
    debug_assert!(codes[1] < nlevels);
    debug_assert!(codes[2] < nlevels);
    codes[0] + nlevels * codes[1] + nlevels * nlevels * codes[2]
}

/// Write one (subband, granule) triplet of three §2.4.3.3.4
/// requantized values into `writer` per the active Table 3-B.4 class.
///
/// For a grouped class the three samples are first quantized
/// individually, packed via the radix-`nlevels` grouping rule, then
/// the combined `class.bits_per_codeword`-bit code is emitted as a
/// single field. For an ungrouped class three independent
/// `class.bits_per_codeword`-bit codes are emitted. The number of
/// bits the writer advances by exactly matches what
/// [`crate::requant::read_triplet`] would consume on the decoder
/// side: `class.bits_per_codeword` for grouped, `3 ·
/// class.bits_per_codeword` for ungrouped.
///
/// `s_double_primes` is the triple of post-linear-formula values
/// `s''` (the encoder has already divided out any §2.4.3.3.3
/// scalefactor — see [`write_triplet_scaled`] for the variant that
/// does that rescaling inline).
pub fn write_triplet(class: &QuantClass, s_double_primes: &[f64; 3], writer: &mut BitWriter) {
    let codes = [
        quantize_sample(class, s_double_primes[0]),
        quantize_sample(class, s_double_primes[1]),
        quantize_sample(class, s_double_primes[2]),
    ];
    if class.grouping {
        // §2.4.3.3.4 packing: for a grouped class `quantize_sample`
        // already returns the radix-nlevels digit in `[0, nb_steps)`
        // (see the `k_max = nb_steps − 1 − msb` clamp inside it). Pack
        // the three digits with the spec's `combined = s[0] +
        // nlevels·s[1] + nlevels²·s[2]` rule into one codeword.
        let combined = group_combined(class, codes);
        writer.write_u32(combined, class.bits_per_codeword);
    } else {
        for &c in &codes {
            writer.write_u32(c, class.bits_per_codeword);
        }
    }
}

/// Write one (subband, granule) triplet of three §2.4.3.3.3 rescaled
/// sample values `s'` into `writer`. Each sample is divided by the
/// Table 3-B.1 multiplier selected by `scalefactor_index` to recover
/// the §2.4.3.3.4 `s''` value, then passed to [`write_triplet`].
///
/// `scalefactor_index` covers all three samples in the triplet
/// (per §2.4.1.6 a single scalefactor covers 12 consecutive samples
/// = 4 triplets); index 63 is reserved and yields
/// [`SampleWriteError::ReservedScalefactorIndex`].
pub fn write_triplet_scaled(
    class: &QuantClass,
    scalefactor_index: u8,
    s_primes: &[f64; 3],
    writer: &mut BitWriter,
) -> Result<(), SampleWriteError> {
    if scalefactor_index as usize >= SCALEFACTOR_COUNT {
        return Err(SampleWriteError::ReservedScalefactorIndex(
            scalefactor_index,
        ));
    }
    let factor = SCALEFACTORS[scalefactor_index as usize];
    let s_double_primes = [
        s_primes[0] / factor,
        s_primes[1] / factor,
        s_primes[2] / factor,
    ];
    write_triplet(class, &s_double_primes, writer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitalloc::class_of_quantization;
    use crate::requant::{degroup, read_triplet, requantize_code, requantize_scaled};
    use oxideav_core::bits::BitReader;

    /// Every defined raw code feeds through (requantize -> quantize)
    /// back to itself: the bin centres are fixed points of the
    /// composition.
    #[test]
    fn quantize_inverts_requantize_for_every_code_of_every_class() {
        for nb in [
            3u32, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
        ] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();
            let limit = 1u32 << n;
            // Walk every code (exhaustive for n <= 14, stepped above).
            let step = if limit > 16384 { limit / 16384 } else { 1 };
            let mut code = 0u32;
            while code < limit {
                if c.grouping && code >= c.nb_steps {
                    // Out-of-level codes are not in the grouped
                    // class's image; skip them (the decoder would
                    // fault on them via `degroup`'s range check, and
                    // the encoder never emits them).
                    code += step;
                    continue;
                }
                let s = requantize_code(&c, code);
                let back = quantize_sample(&c, s);
                assert_eq!(
                    back, code,
                    "nb_steps={nb} code={code} s={s} round-tripped to {back}"
                );
                code += step;
            }
        }
    }

    /// For an arbitrary `s''` (not necessarily a bin centre), quantize
    /// then requantize, and verify the result is at most one
    /// quantization step (`C / 2^(n-1)`) away from the input.
    #[test]
    fn quantize_then_requantize_is_within_one_step() {
        for nb in [3u32, 5, 7, 9, 15, 31, 127] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();
            let msb = 1i64 << (n - 1);
            let step = c.c / msb as f64;
            // Sweep across the representable range of s''. For an
            // ungrouped class the codeword spans the full n-bit range,
            // so k ∈ [−msb, msb − 1]. For a grouped class only the
            // first `nb_steps` raw codes are reachable, so k ∈
            // [−msb, nb_steps − 1 − msb].
            let k_max: i64 = if c.grouping {
                c.nb_steps as i64 - 1 - msb
            } else {
                msb - 1
            };
            let lo = c.c * (-1.0 + c.d);
            let hi = c.c * (k_max as f64 / msb as f64 + c.d);
            let n_probes = 257;
            for i in 0..n_probes {
                let t = i as f64 / (n_probes - 1) as f64;
                let s = lo * (1.0 - t) + hi * t;
                let code = quantize_sample(&c, s);
                // The bin centre the decoder would yield for that code.
                let bin = requantize_code(&c, code);
                assert!(
                    (bin - s).abs() <= step / 2.0 + 1e-12,
                    "nb_steps={nb} s={s} bin={bin} step={step}"
                );
                // For a grouped class the produced digit must be in
                // [0, nb_steps): the encoder never emits a digit the
                // decoder's `degroup` would reject. `quantize_sample`
                // returns the digit directly for grouped classes.
                if c.grouping {
                    assert!(
                        code < c.nb_steps,
                        "grouped nb_steps={nb} produced digit {code} >= {}",
                        c.nb_steps
                    );
                }
            }
        }
    }

    /// Clamping property: an out-of-range positive `s''` rounds to the
    /// largest representable code; an out-of-range negative `s''` rounds
    /// to the smallest representable code.
    ///
    /// `quantize_sample` returns the bitstream code `requantize_code`
    /// consumes — for a grouped class that is the radix-`nlevels` digit
    /// in `[0, nb_steps)`; for an ungrouped class it is the `n`-bit raw
    /// codeword. In both cases the most-positive input lands on the
    /// largest legal code and the most-negative input lands on 0.
    #[test]
    fn out_of_range_inputs_clamp_to_endpoints() {
        for nb in [3u32, 7, 15, 65535] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();

            // Large positive input -> the largest legal code (= nb_steps
            // − 1 for grouped, 2^n − 1 for ungrouped).
            let huge_pos = 1e6_f64;
            let code_hi = quantize_sample(&c, huge_pos);
            if c.grouping {
                assert_eq!(code_hi, c.nb_steps - 1);
            } else {
                assert_eq!(code_hi, (1u32 << n) - 1);
            }

            // Large negative input -> code 0 (the most negative s''').
            let huge_neg = -1e6_f64;
            let code_lo = quantize_sample(&c, huge_neg);
            assert_eq!(code_lo, 0);
        }
    }

    /// `quantize_scaled(s')` produces the same code as
    /// `quantize_sample(s' / factor)`, and rejects reserved index 63.
    #[test]
    fn quantize_scaled_divides_by_table_b1_factor() {
        let c = class_of_quantization(15).unwrap();
        // Pick the unity scalefactor (index 3 = 1.0) and a doubling
        // factor (index 0 = 2.0) and check the division is applied.
        let s_prime = 0.4_f64;
        assert_eq!(
            quantize_scaled(&c, 3, s_prime).unwrap(),
            quantize_sample(&c, s_prime)
        );
        assert_eq!(
            quantize_scaled(&c, 0, s_prime).unwrap(),
            quantize_sample(&c, s_prime / 2.0)
        );
        // Reserved index is rejected.
        assert_eq!(
            quantize_scaled(&c, 63, 0.0),
            Err(SampleWriteError::ReservedScalefactorIndex(63))
        );
        assert_eq!(
            quantize_scaled(&c, 200, 0.0),
            Err(SampleWriteError::ReservedScalefactorIndex(200))
        );
    }

    /// `group_combined` is the exact inverse of
    /// [`crate::requant::degroup`].
    #[test]
    fn group_combined_inverts_degroup_for_every_nb3_combination() {
        let c = class_of_quantization(3).unwrap();
        for s2 in 0..3u32 {
            for s1 in 0..3u32 {
                for s0 in 0..3u32 {
                    let combined = group_combined(&c, [s0, s1, s2]);
                    let back = degroup(&c, combined).unwrap();
                    assert_eq!(back, [s0, s1, s2], "({s0},{s1},{s2})");
                }
            }
        }
    }

    #[test]
    fn group_combined_inverts_degroup_for_every_nb5_combination() {
        let c = class_of_quantization(5).unwrap();
        for s2 in 0..5u32 {
            for s1 in 0..5u32 {
                for s0 in 0..5u32 {
                    let combined = group_combined(&c, [s0, s1, s2]);
                    let back = degroup(&c, combined).unwrap();
                    assert_eq!(back, [s0, s1, s2], "({s0},{s1},{s2})");
                }
            }
        }
    }

    #[test]
    fn group_combined_inverts_degroup_for_every_nb9_combination() {
        let c = class_of_quantization(9).unwrap();
        for s2 in 0..9u32 {
            for s1 in 0..9u32 {
                for s0 in 0..9u32 {
                    let combined = group_combined(&c, [s0, s1, s2]);
                    let back = degroup(&c, combined).unwrap();
                    assert_eq!(back, [s0, s1, s2], "({s0},{s1},{s2})");
                }
            }
        }
    }

    /// `write_triplet` advances the writer by the spec-mandated bit
    /// count for both grouped and ungrouped classes.
    #[test]
    fn write_triplet_advances_writer_by_exact_bit_count() {
        // Grouped: one codeword (bits_per_codeword).
        let grouped = class_of_quantization(5).unwrap();
        let mut w = BitWriter::new();
        let before = w.bit_position();
        write_triplet(&grouped, &[0.0, 0.0, 0.0], &mut w);
        assert_eq!(
            (w.bit_position() - before) as u32,
            grouped.bits_per_codeword
        );

        // Ungrouped: three times bits_per_codeword.
        let sep = class_of_quantization(7).unwrap();
        let mut w = BitWriter::new();
        let before = w.bit_position();
        write_triplet(&sep, &[0.0, 0.0, 0.0], &mut w);
        assert_eq!(
            (w.bit_position() - before) as u32,
            sep.bits_per_codeword * 3
        );
    }

    /// `write_triplet` round-trips through
    /// [`crate::requant::read_triplet`] for grouped and ungrouped
    /// classes: the decoder reconstructs the bin centres of the
    /// quantized samples.
    #[test]
    fn write_triplet_round_trips_through_read_triplet() {
        // Test every class. Use s'' values that lie on bin centres so
        // the round-trip is exact (the "within one step" property is
        // covered by a separate test).
        for nb in [3u32, 5, 7, 9, 15, 31, 63, 127, 255, 511] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();
            let msb = 1u32 << (n - 1);
            // Pick three valid raw codes.
            let raw_codes: [u32; 3] = if c.grouping {
                // Levels in [0, nb_steps).
                [0, c.nb_steps / 2, c.nb_steps - 1]
            } else {
                // Full n-bit raw code space.
                [0, msb, (1u32 << n) - 1]
            };
            // Convert to the s'' the decoder would produce.
            let s_arr: [f64; 3] = [
                requantize_code(&c, raw_codes[0]),
                requantize_code(&c, raw_codes[1]),
                requantize_code(&c, raw_codes[2]),
            ];
            let mut w = BitWriter::new();
            write_triplet(&c, &s_arr, &mut w);
            // Pad to byte boundary for BitReader.
            let pad = (8 - (w.bit_position() as u32 % 8)) % 8;
            w.write_u32(0, pad);
            let bytes = w.finish();
            let mut r = BitReader::new(&bytes);
            let got = read_triplet(&c, &mut r).unwrap();
            for k in 0..3 {
                assert!(
                    (got[k] - s_arr[k]).abs() < 1e-9,
                    "nb_steps={nb} k={k} got={} want={}",
                    got[k],
                    s_arr[k]
                );
            }
        }
    }

    /// `write_triplet_scaled` is the exact inverse of
    /// [`crate::requant::requantize_scaled`]: writing three rescaled
    /// values `s'` and then reading them back through `requantize_scaled`
    /// with the same scalefactor reconstructs the bin centres.
    #[test]
    fn write_triplet_scaled_round_trips_through_requantize_scaled() {
        let c = class_of_quantization(15).unwrap();
        // Scalefactor index 3 = unity (1.0); index 0 = 2.0.
        for &sf_idx in &[3u8, 0, 10, 30, 62] {
            let factor = SCALEFACTORS[sf_idx as usize];
            // s'' bin centres for codes 0, 7, 15.
            let raw_codes = [0u32, 7, 15];
            let s_double: [f64; 3] = [
                requantize_code(&c, raw_codes[0]),
                requantize_code(&c, raw_codes[1]),
                requantize_code(&c, raw_codes[2]),
            ];
            let s_primes = [
                s_double[0] * factor,
                s_double[1] * factor,
                s_double[2] * factor,
            ];

            let mut w = BitWriter::new();
            write_triplet_scaled(&c, sf_idx, &s_primes, &mut w).unwrap();
            let pad = (8 - (w.bit_position() as u32 % 8)) % 8;
            w.write_u32(0, pad);
            let bytes = w.finish();

            let mut r = BitReader::new(&bytes);
            let got = requantize_scaled(&c, sf_idx, &mut r).unwrap();
            for k in 0..3 {
                assert!(
                    (got[k] - s_primes[k]).abs() < 1e-9,
                    "sf={sf_idx} k={k} got={} want={}",
                    got[k],
                    s_primes[k]
                );
            }
        }
    }

    #[test]
    fn write_triplet_scaled_rejects_reserved_scalefactor_index() {
        let c = class_of_quantization(7).unwrap();
        let mut w = BitWriter::new();
        let err = write_triplet_scaled(&c, 63, &[0.0, 0.0, 0.0], &mut w).unwrap_err();
        assert_eq!(err, SampleWriteError::ReservedScalefactorIndex(63));
        // Nothing was written.
        assert_eq!(w.bit_position(), 0);
    }

    /// Sanity check on the zero point and symmetry of the
    /// quantization rule for an ungrouped class.
    ///
    /// `s'' = C · D` corresponds to `s''' = 0`, which the §2.4.3.3.4
    /// inversion maps to `k = 0`. The encoder's code is then `twos(0)
    /// ^ msb = msb`, so `quantize_sample(C · D)` returns exactly `msb`
    /// for an ungrouped class. Values just above `C · D` map to codes
    /// strictly above `msb`; values just below map strictly below.
    #[test]
    fn symmetric_inputs_yield_symmetric_codes() {
        let c = class_of_quantization(15).unwrap();
        let n = c.bits_per_sample();
        let msb = 1u32 << (n - 1);

        let zero = c.c * c.d;
        let code_zero = quantize_sample(&c, zero);
        assert_eq!(code_zero, msb, "zero point lands on code msb");

        // One full quantization step in s'' is `C / 2^(n-1)`; offset
        // by at least that to land in the next-higher / next-lower bin.
        let step = c.c / msb as f64;
        let code_pos = quantize_sample(&c, zero + step);
        assert!(code_pos > msb, "code_pos={code_pos} msb={msb}");

        let code_neg = quantize_sample(&c, zero - step);
        assert!(code_neg < msb, "code_neg={code_neg} msb={msb}");
    }

    /// Exhaustive grouped-class round-trip: every level in [0,
    /// nb_steps) produces a code that, after grouping and the
    /// decoder's degrouper, yields the original level back. (This
    /// stresses the `clamp` path inside `write_triplet`'s grouped
    /// branch.)
    #[test]
    fn grouped_classes_round_trip_every_level_triplet() {
        for nb in [3u32, 5, 9] {
            let c = class_of_quantization(nb).unwrap();
            // Walk every (l0, l1, l2) tuple of levels.
            for l0 in 0..nb {
                for l1 in 0..nb {
                    for l2 in 0..nb {
                        let s_arr: [f64; 3] = [
                            requantize_code(&c, l0),
                            requantize_code(&c, l1),
                            requantize_code(&c, l2),
                        ];
                        let mut w = BitWriter::new();
                        write_triplet(&c, &s_arr, &mut w);
                        let pad = (8 - (w.bit_position() as u32 % 8)) % 8;
                        w.write_u32(0, pad);
                        let bytes = w.finish();
                        let mut r = BitReader::new(&bytes);
                        let got = read_triplet(&c, &mut r).unwrap();
                        for k in 0..3 {
                            assert!(
                                (got[k] - s_arr[k]).abs() < 1e-9,
                                "nb={nb} ({l0},{l1},{l2}) k={k} got={} want={}",
                                got[k],
                                s_arr[k]
                            );
                        }
                    }
                }
            }
        }
    }
}
