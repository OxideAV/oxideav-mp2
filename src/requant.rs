//! MPEG-1 Audio Layer II sample requantizer — ISO/IEC 11172-3 (1993)
//! §2.4.3.3.4 ("Requantization of subband samples").
//!
//! Clean-room: the degrouping algorithm, the MSB-inversion /
//! two's-complement-fractional interpretation, and the
//! `s'' = C * (s''' + D)` linear formula are transcribed directly from
//! the staged ISO/IEC PDF (`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`,
//! SHA-256
//! `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`),
//! §2.4.3.3.4 on PDF page 32 (full-PDF page 38). The requantization
//! constants `C` and `D` come from Table 3-B.4 (PDF page 50), already
//! tabulated in [`crate::bitalloc::QuantClass`]. No third-party MP2
//! source was consulted.
//!
//! # §2.4.3.3.4 algorithm
//!
//! The coded samples appear as *triplets* — for each (subband, granule)
//! a single code covers three consecutive subband samples. Table 3-B.4
//! says, per quantization class, how many bits make up that code and
//! whether the three samples are **grouped** into one combined code or
//! carried as three separable codes.
//!
//! 1. **Degrouping** (grouped classes `nb_steps ∈ {3, 5, 9}` only). The
//!    combined code `c` (an unsigned integer of
//!    [`QuantClass::bits_per_codeword`] bits) is split with the spec's
//!    radix-`nlevels` algorithm:
//!
//!    ```text
//!    for (i = 0; i < 3; i++) {
//!        s[i] = c % nlevels;
//!        c    = c DIV nlevels;
//!    }
//!    ```
//!
//!    Each resulting `s[i]` is an unsigned value in `0 ..= nlevels - 1`,
//!    occupying [`QuantClass::bits_per_sample`] bits.
//!
//! 2. **Separable classes** read three independent codes, each
//!    [`QuantClass::bits_per_codeword`] bits (which for an ungrouped
//!    class equals [`QuantClass::bits_per_sample`]).
//!
//! 3. **MSB inversion + fractional interpretation.** "The first bit of
//!    each of the three codes has to be inverted, and the resulting
//!    numbers should be regarded as two's complement fractional numbers,
//!    where the MSB represents the value -1." With `n =
//!    bits_per_sample`, inverting the top bit and reading the result as
//!    an `n`-bit two's complement integer `v ∈ [-2^(n-1), 2^(n-1)-1]`,
//!    the fractional value is `s''' = v / 2^(n-1)`.
//!
//! 4. **Linear formula** `s'' = C * (s''' + D)` (PDF page 32). `C` and
//!    `D` are the Table 3-B.4 row constants.
//!
//! Scalefactor rescaling (`s' = factor * s''`, factor from Table
//! 3-B.1) is a separate §2.4.3.3.3 step layered on top by
//! [`requantize_scaled`].

use oxideav_core::bits::BitReader;

use crate::bitalloc::QuantClass;
use crate::tables::SCALEFACTORS;

/// Errors raised by the §2.4.3.3.4 requantizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequantError {
    /// The bitstream ran out of bits before a sample code could be read.
    UnexpectedEnd,
    /// A degrouped sample code landed outside `0 ..= nb_steps - 1`. For
    /// a spec-conformant stream every combined code is `< nlevels^3`,
    /// so this only fires on a corrupt/over-long combined code.
    DegroupedSampleOutOfRange {
        /// `nb_steps` (== `nlevels`) of the active class.
        nb_steps: u32,
        /// The degrouped code that exceeded the level count.
        code: u32,
    },
    /// A scalefactor index `≥ 63` was supplied to [`requantize_scaled`];
    /// only indices `0..=62` select a Table 3-B.1 multiplier.
    ReservedScalefactorIndex(u8),
}

impl core::fmt::Display for RequantError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RequantError::UnexpectedEnd => {
                write!(f, "requant: bitstream ended before a sample code")
            }
            RequantError::DegroupedSampleOutOfRange { nb_steps, code } => write!(
                f,
                "requant: degrouped code {code} out of range 0..{nb_steps}"
            ),
            RequantError::ReservedScalefactorIndex(idx) => write!(
                f,
                "requant: scalefactor index {idx} is reserved (only 0..=62 are defined)"
            ),
        }
    }
}

impl std::error::Error for RequantError {}

/// Apply the §2.4.3.3.4 MSB-inversion + two's-complement-fractional
/// interpretation + `s'' = C * (s''' + D)` linear formula to a single
/// `n`-bit sample code.
///
/// `code` is the raw `class.bits_per_sample()`-bit unsigned value (the
/// degrouped `s[i]` for a grouped class, or the directly read code for
/// an ungrouped class).
///
/// This is the lowest-level primitive; it performs no bitstream reads.
pub fn requantize_code(class: &QuantClass, code: u32) -> f64 {
    let n = class.bits_per_sample();
    debug_assert!((1..=16).contains(&n));
    let msb = 1u32 << (n - 1);
    // §2.4.3.3.4: "the first bit ... has to be inverted". The first bit
    // is the MSB of the n-bit code.
    let inverted = code ^ msb;
    // Read the inverted n-bit pattern as a two's complement integer:
    // values with the (now) top bit set are negative. `v` ranges over
    // [-2^(n-1), 2^(n-1)-1].
    let v = if inverted & msb != 0 {
        inverted as i32 - (1i32 << n)
    } else {
        inverted as i32
    };
    // "two's complement fractional number, where the MSB represents the
    // value -1": divide by 2^(n-1) so the MSB weight is exactly -1.
    let fractional = v as f64 / msb as f64;
    // Linear formula s'' = C * (s''' + D).
    class.c * (fractional + class.d)
}

/// Degroup a combined code into its three constituent sample codes per
/// the §2.4.3.3.4 radix-`nlevels` algorithm.
///
/// `combined` is the [`QuantClass::bits_per_codeword`]-bit unsigned
/// value read for a grouped class; each returned code is in
/// `0 ..= nb_steps - 1`.
pub fn degroup(class: &QuantClass, mut combined: u32) -> Result<[u32; 3], RequantError> {
    let nlevels = class.nb_steps;
    let mut out = [0u32; 3];
    for slot in out.iter_mut() {
        let code = combined % nlevels;
        combined /= nlevels;
        if code >= nlevels {
            return Err(RequantError::DegroupedSampleOutOfRange {
                nb_steps: nlevels,
                code,
            });
        }
        *slot = code;
    }
    Ok(out)
}

/// Read one triplet of subband samples for a given quantization class
/// from `reader` and requantize them per §2.4.3.3.4, returning the
/// three normalized fractional values `s''`.
///
/// For a grouped class one combined codeword is read and degrouped;
/// for a separable class three independent codes are read. The reader
/// is advanced past exactly the bits §2.4.1.6 prescribes for one
/// (subband, channel) triplet.
pub fn read_triplet(
    class: &QuantClass,
    reader: &mut BitReader<'_>,
) -> Result<[f64; 3], RequantError> {
    let codes = read_triplet_codes(class, reader)?;
    Ok([
        requantize_code(class, codes[0]),
        requantize_code(class, codes[1]),
        requantize_code(class, codes[2]),
    ])
}

/// Like [`read_triplet`] but additionally rescales by the §2.4.3.3.3
/// scalefactor (Table 3-B.1) selected by `scalefactor_index`:
/// `s' = factor * s''`.
///
/// `scalefactor_index` is the 6-bit index parsed for the relevant
/// granule; values `≥ 63` are rejected (index 63 is reserved).
pub fn requantize_scaled(
    class: &QuantClass,
    scalefactor_index: u8,
    reader: &mut BitReader<'_>,
) -> Result<[f64; 3], RequantError> {
    if scalefactor_index as usize >= SCALEFACTORS.len() {
        return Err(RequantError::ReservedScalefactorIndex(scalefactor_index));
    }
    let factor = SCALEFACTORS[scalefactor_index as usize];
    let raw = read_triplet(class, reader)?;
    Ok([raw[0] * factor, raw[1] * factor, raw[2] * factor])
}

/// Read the three raw (pre-requantization) sample codes for one triplet
/// from the bitstream, performing degrouping for grouped classes. Each
/// returned code is `class.bits_per_sample()` bits wide.
fn read_triplet_codes(
    class: &QuantClass,
    reader: &mut BitReader<'_>,
) -> Result<[u32; 3], RequantError> {
    if class.grouping {
        let combined = reader
            .read_u32(class.bits_per_codeword)
            .map_err(|_| RequantError::UnexpectedEnd)?;
        degroup(class, combined)
    } else {
        let n = class.bits_per_codeword;
        let mut out = [0u32; 3];
        for slot in out.iter_mut() {
            *slot = reader
                .read_u32(n)
                .map_err(|_| RequantError::UnexpectedEnd)?;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitalloc::class_of_quantization;

    /// MSB-first writer mirroring the audio_data test helper.
    struct BitWriter {
        bytes: Vec<u8>,
        bit_in_byte: u32,
    }

    impl BitWriter {
        fn new() -> Self {
            BitWriter {
                bytes: Vec::new(),
                bit_in_byte: 0,
            }
        }

        fn write(&mut self, mut value: u32, mut bits: u32) {
            assert!(bits <= 32);
            while bits > 0 {
                if self.bit_in_byte == 0 {
                    self.bytes.push(0);
                }
                let space = 8 - self.bit_in_byte;
                let take = bits.min(space);
                let shift = space - take;
                let chunk = (value >> (bits - take)) & ((1u32 << take) - 1);
                let last = self.bytes.last_mut().unwrap();
                *last |= (chunk as u8) << shift;
                self.bit_in_byte = (self.bit_in_byte + take) % 8;
                bits -= take;
                value &= if bits == 0 { 0 } else { (1u32 << bits) - 1 };
            }
        }

        fn finish(self) -> Vec<u8> {
            self.bytes
        }
    }

    /// Reference requantizer expressed straight from the §2.4.3.3.4
    /// prose, independent of the bit-twiddling in `requantize_code`, to
    /// cross-check the production path.
    fn ref_requantize(class: &QuantClass, code: u32) -> f64 {
        let n = class.bits_per_sample();
        // Build the bit string, invert the first (most significant) bit.
        let mut bits: Vec<u8> = (0..n).rev().map(|k| ((code >> k) & 1) as u8).collect();
        bits[0] ^= 1;
        // Two's complement fractional: MSB weight -1, then 2^-1, 2^-2…
        let mut s = if bits[0] == 1 { -1.0 } else { 0.0 };
        for (j, &b) in bits.iter().enumerate().skip(1) {
            if b == 1 {
                s += 2.0_f64.powi(-(j as i32));
            }
        }
        class.c * (s + class.d)
    }

    #[test]
    fn three_level_class_produces_symmetric_levels() {
        // nb_steps = 3, grouped, C = 4/3, D = 1/2, 2 bits/sample.
        let c = class_of_quantization(3).unwrap();
        assert_eq!(c.bits_per_sample(), 2);
        // Codes 0,1,2 (00,01,10). After MSB inversion: 10,11,00.
        // As 2-bit two's complement: -2,-1,0 → fractional /2: -1,-0.5,0.
        // s'' = C*(s'''+D): C*(-0.5), C*0, C*0.5.
        let cc = c.c;
        let want = [cc * -0.5, 0.0, cc * 0.5];
        for (code, w) in [0u32, 1, 2].into_iter().zip(want) {
            let got = requantize_code(&c, code);
            assert!((got - w).abs() < 1e-12, "code={code} got={got} want={w}");
        }
    }

    #[test]
    fn requantize_code_matches_prose_reference_for_every_class() {
        for nb in [
            3u32, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
        ] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();
            // Exhaustive for small classes, sampled for wide ones.
            let limit = 1u32 << n;
            let step = if limit > 4096 { limit / 4096 } else { 1 };
            let mut code = 0;
            while code < limit {
                let got = requantize_code(&c, code);
                let want = ref_requantize(&c, code);
                assert!(
                    (got - want).abs() < 1e-9,
                    "nb_steps={nb} code={code} got={got} want={want}"
                );
                code += step;
            }
        }
    }

    #[test]
    fn requantized_values_stay_inside_open_interval() {
        // For every class, |s''| < C*(1+D) and the magnitude never
        // reaches the +1 endpoint (the fractional value tops out below
        // 1 because the positive extreme is 2^(n-1)-1 over 2^(n-1)).
        for nb in [3u32, 5, 7, 9, 15, 31, 63, 65535] {
            let c = class_of_quantization(nb).unwrap();
            let n = c.bits_per_sample();
            let mut max_mag = 0.0f64;
            for code in 0..(1u32 << n) {
                max_mag = max_mag.max(requantize_code(&c, code).abs());
            }
            let bound = c.c * (1.0 + c.d);
            assert!(max_mag <= bound + 1e-12, "nb_steps={nb}");
        }
    }

    #[test]
    fn degroup_matches_radix_algorithm() {
        // nb_steps = 5 grouped: combined = s0 + 5*s1 + 25*s2.
        let c = class_of_quantization(5).unwrap();
        for s2 in 0..5u32 {
            for s1 in 0..5u32 {
                for s0 in 0..5u32 {
                    let combined = s0 + 5 * s1 + 25 * s2;
                    let got = degroup(&c, combined).unwrap();
                    assert_eq!(got, [s0, s1, s2], "combined={combined}");
                }
            }
        }
    }

    #[test]
    fn degroup_nb3_covers_all_27_combinations() {
        let c = class_of_quantization(3).unwrap();
        // Combined code spans 0..27 (3^3); each decodes to base-3 digits.
        for combined in 0..27u32 {
            let got = degroup(&c, combined).unwrap();
            let want = [combined % 3, (combined / 3) % 3, (combined / 9) % 3];
            assert_eq!(got, want, "combined={combined}");
        }
    }

    #[test]
    fn read_triplet_separable_class_reads_three_codes() {
        // nb_steps = 7 → 3 bits/sample, ungrouped. Write three codes.
        let c = class_of_quantization(7).unwrap();
        let mut bw = BitWriter::new();
        bw.write(0b000, 3); // code 0
        bw.write(0b011, 3); // code 3
        bw.write(0b110, 3); // code 6
        let bytes = bw.finish();
        let mut r = BitReader::new(&bytes);
        let got = read_triplet(&c, &mut r).unwrap();
        let want = [
            requantize_code(&c, 0),
            requantize_code(&c, 3),
            requantize_code(&c, 6),
        ];
        for k in 0..3 {
            assert!((got[k] - want[k]).abs() < 1e-12, "k={k}");
        }
    }

    #[test]
    fn read_triplet_grouped_class_degroups_one_codeword() {
        // nb_steps = 9 grouped → 10-bit combined code, 4 bits/sample.
        let c = class_of_quantization(9).unwrap();
        let (s0, s1, s2) = (2u32, 5u32, 8u32);
        let combined = s0 + 9 * s1 + 81 * s2;
        let mut bw = BitWriter::new();
        bw.write(combined, c.bits_per_codeword); // 10 bits
        let bytes = bw.finish();
        let mut r = BitReader::new(&bytes);
        let got = read_triplet(&c, &mut r).unwrap();
        let want = [
            requantize_code(&c, s0),
            requantize_code(&c, s1),
            requantize_code(&c, s2),
        ];
        for k in 0..3 {
            assert!((got[k] - want[k]).abs() < 1e-12, "k={k}");
        }
    }

    #[test]
    fn read_triplet_advances_reader_by_exact_bit_count() {
        // Grouped: one codeword (bits_per_codeword). Separable: 3 *
        // bits_per_codeword.
        let grouped = class_of_quantization(5).unwrap();
        let mut bw = BitWriter::new();
        bw.write(0, grouped.bits_per_codeword);
        bw.write(0, 8); // padding so the reader has spare bits
        let bytes = bw.finish();
        let mut r = BitReader::new(&bytes);
        let before = r.bit_position();
        read_triplet(&grouped, &mut r).unwrap();
        assert_eq!(
            (r.bit_position() - before) as u32,
            grouped.bits_per_codeword
        );

        let sep = class_of_quantization(7).unwrap();
        let mut bw = BitWriter::new();
        bw.write(0, sep.bits_per_codeword * 3);
        bw.write(0, 8);
        let bytes = bw.finish();
        let mut r = BitReader::new(&bytes);
        let before = r.bit_position();
        read_triplet(&sep, &mut r).unwrap();
        assert_eq!(
            (r.bit_position() - before) as u32,
            sep.bits_per_codeword * 3
        );
    }

    #[test]
    fn requantize_scaled_applies_table_b1_multiplier() {
        // Scalefactor index 3 = 1.0 → no change; index 0 = 2.0 doubles.
        let c = class_of_quantization(7).unwrap();
        let mut bw = BitWriter::new();
        bw.write(0b101, 3);
        bw.write(0b101, 3);
        bw.write(0b101, 3);
        let bytes = bw.finish();

        let mut r = BitReader::new(&bytes);
        let unscaled = read_triplet(&c, &mut r).unwrap();

        let mut r = BitReader::new(&bytes);
        let scaled_unity = requantize_scaled(&c, 3, &mut r).unwrap();
        for k in 0..3 {
            assert!((scaled_unity[k] - unscaled[k]).abs() < 1e-12, "unity k={k}");
        }

        let mut r = BitReader::new(&bytes);
        let scaled_double = requantize_scaled(&c, 0, &mut r).unwrap();
        for k in 0..3 {
            assert!(
                (scaled_double[k] - unscaled[k] * 2.0).abs() < 1e-12,
                "double k={k}"
            );
        }
    }

    #[test]
    fn requantize_scaled_rejects_reserved_index() {
        let c = class_of_quantization(7).unwrap();
        let bytes = [0u8; 4];
        let mut r = BitReader::new(&bytes);
        assert_eq!(
            requantize_scaled(&c, 63, &mut r),
            Err(RequantError::ReservedScalefactorIndex(63))
        );
    }

    #[test]
    fn read_triplet_reports_unexpected_end() {
        let c = class_of_quantization(65535).unwrap(); // 16 bits/sample
        let bytes = [0u8; 1]; // only 8 bits, need 48
        let mut r = BitReader::new(&bytes);
        assert_eq!(read_triplet(&c, &mut r), Err(RequantError::UnexpectedEnd));
    }

    #[test]
    fn c_and_d_constants_are_those_of_table_b4() {
        // Pin the requantization coefficients the formula consumes,
        // independent of the bitalloc-module table test.
        let checks: [(u32, f64, f64); 5] = [
            (3, 1.333_333_333_33, 0.5),
            (5, 1.6, 0.5),
            (7, 1.142_857_142_86, 0.25),
            (9, 1.777_777_777_77, 0.5),
            (65535, 1.000_015_259_02, 0.000_030_517_58),
        ];
        for (nb, c_want, d_want) in checks {
            let c = class_of_quantization(nb).unwrap();
            assert!((c.c - c_want).abs() < 1e-10, "nb_steps={nb} C");
            assert!((c.d - d_want).abs() < 1e-10, "nb_steps={nb} D");
        }
    }
}
