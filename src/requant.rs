//! MPEG-1 / MPEG-2 LSF Audio Layer II sample-group unpack + requantisation.
//!
//! Layer II splits each subband's 36 samples into three 12-sample groups
//! ("sbgroups" in the spec); each 12-sample group is further split into
//! four 3-sample "triples". For each triple the decoder reads either one
//! codeword per sample (ungrouped quantisers) or a single codeword
//! encoding the triple (the 3-, 5-, or 9-level grouped quantisers —
//! §2.4.3.2, Table 3-B.4).
//!
//! # Requantisation (ISO/IEC 11172-3 §2.4.3.4 eqns 2-9..2-11)
//!
//! For an *ungrouped* `b`-bit codeword `v`:
//!   s' = (v + D) * C
//!   s  = s' * scalefactor
//! with `C = 2 / (2^b - 1)` and `D = -(2^(b-1) - 1)`.
//!
//! For *grouped* quantisers (L ∈ {3, 5, 9}) the triple codeword is first
//! unpacked into three per-sample indices `i ∈ {0..L-1}`; each index
//! maps to a fractional amplitude
//!   s' = (2*i - (L-1)) / L
//! which is then multiplied by the scalefactor as usual.
//!
//! # Joint stereo / intensity stereo (§2.4.3.3)
//!
//! Layer II joint-stereo is intensity stereo: for subbands at/above the
//! `bound` (determined by `mode_extension`, §2.4.2.3 Table 3-B.3), a
//! single allocation index and a single set of sample codewords are
//! transmitted for both channels, but each channel carries its own
//! scalefactors. The per-channel reconstruction is done by
//! [`read_triple_shared`] — it requantises the shared codeword once and
//! multiplies by `sf_L` and `sf_R` separately. This applies verbatim to
//! MPEG-2 LSF (ISO/IEC 13818-3 §2.4.3.3 inherits the layout; LSF simply
//! uses the consolidated `TABLE_LSF` with `sblimit = 30`).

use crate::tables::{scalefactor_magnitude, AllocEntry, AllocTable};
use oxideav_core::bits::BitReader;
use oxideav_core::{Error, Result};

/// Decoded subband-sample buffer: `samples[ch][sb][i]`, `i = 0..36`.
pub type SubbandSamples = Vec<Vec<[f32; 36]>>;

/// Reader state for one frame's sample-payload parsing.
pub struct ReadState<'a> {
    pub table: &'a AllocTable,
    pub allocation: &'a [[u8; 32]; 2],
    pub scalefactor: &'a [[[u8; 3]; 32]; 2],
    pub channels: usize,
    pub sblimit: usize,
    /// Joint-stereo bound — subbands at-or-above use one shared set of
    /// sample codewords, requantised into each channel with that
    /// channel's own scalefactor.
    pub bound: usize,
}

/// Read the sample payload from the bitstream, ungroup/requantise, and
/// return `samples[ch][sb][0..36]`.
pub fn read_samples(br: &mut BitReader<'_>, st: &ReadState<'_>) -> Result<SubbandSamples> {
    let mut samples: SubbandSamples = (0..st.channels).map(|_| vec![[0.0f32; 36]; 32]).collect();

    for gr in 0..3 {
        for tr in 0..4 {
            let base_idx = gr * 12 + tr * 3;
            // Independent-allocation subbands (ch 0 + ch 1 each transmit).
            for sb in 0..st.bound.min(st.sblimit) {
                for ch in 0..st.channels {
                    read_triple(br, st, ch, sb, gr, base_idx, &mut samples[ch][sb])?;
                }
            }
            // Shared-allocation subbands (joint stereo upper band).
            for sb in st.bound..st.sblimit {
                read_triple_shared(br, st, sb, gr, base_idx, &mut samples)?;
            }
        }
    }

    Ok(samples)
}

/// Read one 3-sample triple into `out_row[base_idx..base_idx+3]`.
fn read_triple(
    br: &mut BitReader<'_>,
    st: &ReadState<'_>,
    ch: usize,
    sb: usize,
    gr: usize,
    base_idx: usize,
    out_row: &mut [f32; 36],
) -> Result<()> {
    let alloc = st.allocation[ch][sb];
    if alloc == 0 {
        return Ok(());
    }
    let entry = class_entry(st.table, sb, alloc);
    let q = decode_entry(entry);
    let sf_mag = scalefactor_magnitude(st.scalefactor[ch][sb][gr]);

    match q {
        QuantCase::Grouped { levels, bits } => {
            let code = br.read_u32(bits)?;
            let triple = ungroup(code, levels)?;
            for i in 0..3 {
                out_row[base_idx + i] = grouped_fraction(triple[i], levels) * sf_mag;
            }
        }
        QuantCase::Ungrouped { bits, c, d } => {
            for i in 0..3 {
                let v = br.read_u32(bits)? as i32;
                out_row[base_idx + i] = ((v + d) as f32) * c * sf_mag;
            }
        }
    }
    Ok(())
}

fn read_triple_shared(
    br: &mut BitReader<'_>,
    st: &ReadState<'_>,
    sb: usize,
    gr: usize,
    base_idx: usize,
    samples: &mut SubbandSamples,
) -> Result<()> {
    let alloc = st.allocation[0][sb];
    if alloc == 0 {
        return Ok(());
    }
    let entry = class_entry(st.table, sb, alloc);
    let q = decode_entry(entry);
    let sf0 = scalefactor_magnitude(st.scalefactor[0][sb][gr]);
    let sf1 = if st.channels == 2 {
        scalefactor_magnitude(st.scalefactor[1][sb][gr])
    } else {
        0.0
    };

    match q {
        QuantCase::Grouped { levels, bits } => {
            let code = br.read_u32(bits)?;
            let triple = ungroup(code, levels)?;
            for i in 0..3 {
                let f = grouped_fraction(triple[i], levels);
                samples[0][sb][base_idx + i] = f * sf0;
                if st.channels == 2 {
                    samples[1][sb][base_idx + i] = f * sf1;
                }
            }
        }
        QuantCase::Ungrouped { bits, c, d } => {
            for i in 0..3 {
                let v = br.read_u32(bits)? as i32;
                let f = ((v + d) as f32) * c;
                samples[0][sb][base_idx + i] = f * sf0;
                if st.channels == 2 {
                    samples[1][sb][base_idx + i] = f * sf1;
                }
            }
        }
    }
    Ok(())
}

fn class_entry(table: &AllocTable, sb: usize, alloc: u8) -> AllocEntry {
    let base = table.offsets[sb];
    table.entries[base + alloc as usize]
}

enum QuantCase {
    /// Grouped 3-, 5- or 9-level quantiser: one codeword of `bits` bits
    /// encodes a triple.
    Grouped { levels: u32, bits: u32 },
    /// Ungrouped: one codeword of `bits` bits per sample. `c` is the
    /// fractional multiplier `2/(2^bits - 1)` and `d` is the additive
    /// centring offset `-(2^(bits-1) - 1)`.
    Ungrouped { bits: u32, c: f32, d: i32 },
}

fn decode_entry(entry: AllocEntry) -> QuantCase {
    let bits = entry.bits as u32;
    let d = entry.d as i32;
    if d > 0 {
        // Grouped. `d` is the level count (3, 5, or 9).
        QuantCase::Grouped {
            levels: d as u32,
            bits,
        }
    } else {
        // Ungrouped. Number of levels encoded in `bits` bits is `2^bits - 1`
        // (spec §2.4.3.4.2: only the 2^b - 1 even codewords are used, with
        // the sign convention expressed via the centring offset `d`).
        let levels = (1u32 << bits) - 1;
        let c = 2.0f64 / (levels as f64);
        QuantCase::Ungrouped {
            bits,
            c: c as f32,
            d,
        }
    }
}

/// Fractional amplitude for a grouped-quantiser sample index.
/// Returns `(2 * idx - (L - 1)) / L`.
fn grouped_fraction(idx: i32, levels: u32) -> f32 {
    let l = levels as f32;
    ((2 * idx) as f32 - (l - 1.0)) / l
}

/// Unpack a grouped 3-/5-/9-level codeword into three per-sample indices.
/// The codeword is a base-L little-endian integer `v = s0 + L*s1 + L²*s2`.
fn ungroup(code: u32, levels: u32) -> Result<[i32; 3]> {
    let l = levels;
    if l != 3 && l != 5 && l != 9 {
        return Err(Error::invalid(format!(
            "mp2: bad grouped-quantiser level count {l}"
        )));
    }
    let s0 = code % l;
    let r = code / l;
    let s1 = r % l;
    let s2 = r / l;
    if s2 >= l {
        return Err(Error::invalid(format!(
            "mp2: grouped codeword {code} out of range for L={l}"
        )));
    }
    Ok([s0 as i32, s1 as i32, s2 as i32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ungroup_base3() {
        assert_eq!(ungroup(0, 3).unwrap(), [0, 0, 0]);
        assert_eq!(ungroup(7, 3).unwrap(), [1, 2, 0]);
        assert_eq!(ungroup(26, 3).unwrap(), [2, 2, 2]);
    }

    #[test]
    fn grouped_fraction_endpoints() {
        // L=3: idx 0 -> -2/3, idx 1 -> 0, idx 2 -> +2/3.
        assert!((grouped_fraction(0, 3) - (-2.0 / 3.0)).abs() < 1e-6);
        assert!(grouped_fraction(1, 3).abs() < 1e-6);
        assert!((grouped_fraction(2, 3) - (2.0 / 3.0)).abs() < 1e-6);
        // L=5: idx 0 -> -4/5, idx 4 -> +4/5.
        assert!((grouped_fraction(0, 5) - (-4.0 / 5.0)).abs() < 1e-6);
        assert!((grouped_fraction(4, 5) - (4.0 / 5.0)).abs() < 1e-6);
    }

    #[test]
    fn ungrouped_center_is_zero() {
        // Decode an entry with bits=4, d=-7 (i.e. 15-level ungrouped).
        let entry = AllocEntry { bits: 4, d: -7 };
        let q = decode_entry(entry);
        match q {
            QuantCase::Ungrouped { bits, c, d } => {
                assert_eq!(bits, 4);
                assert_eq!(d, -7);
                // Center (v = 7) should map to 0.
                let frac = ((7 + d) as f32) * c;
                assert!(frac.abs() < 1e-5);
            }
            _ => panic!("expected ungrouped case"),
        }
    }

    /// Layer II intensity stereo at/above the joint-stereo bound: the
    /// allocation index and the sample codewords are shared between L and
    /// R, but each channel applies its own per-sbgroup scalefactor (ISO/IEC
    /// 11172-3 §2.4.3.3, ISO/IEC 13818-3 §2.4.3.3 inherits the same layout).
    ///
    /// Build bitstream bytes that encode a single 3-level grouped triple
    /// for subband 0 (TABLE_LSF subband 0, alloc=1 → 5-bit codeword
    /// encoding a triple of 3-level values) and verify that:
    ///  * L reconstructs with its scalefactor,
    ///  * R reconstructs with its own (different) scalefactor,
    ///  * both channels track the same quantised triple shape.
    #[test]
    fn intensity_stereo_shared_alloc_uses_per_channel_scalefactor() {
        use crate::tables::TABLE_LSF;

        // TABLE_LSF, sb 0, alloc 1 = `ae!(5, 3)` → grouped 3-level,
        // 5-bit codeword encoding one triple per sbgroup.
        let table = &TABLE_LSF;
        let mut allocation = [[0u8; 32]; 2];
        allocation[0][0] = 1;
        allocation[1][0] = 1;
        let mut scalefactor = [[[0u8; 3]; 32]; 2];
        // Different scalefactors per channel to exercise the per-channel
        // scale path.
        scalefactor[0][0][0] = 30;
        scalefactor[1][0][0] = 45;
        let st = ReadState {
            table,
            allocation: &allocation,
            scalefactor: &scalefactor,
            channels: 2,
            sblimit: 1,
            bound: 0, // everything at/above 0 → all shared (IS) subbands.
        };
        // Pick triple_code = 5 (fits in 5 bits). base-3 digits:
        //   5 % 3 = 2, 5 / 3 = 1; 1 % 3 = 1, 1 / 3 = 0; 0 % 3 = 0.
        //   → indices (2, 1, 0). fractions at L=3: (2/3, 0, -2/3).
        let triple_code: u32 = 5;
        // Pack 5 bits MSB-first into buf[0] starting at bit 7.
        let mut buf = [0u8; 8];
        buf[0] = ((triple_code & 0x1F) as u8) << 3;
        let mut br = BitReader::new(&buf);
        let mut samples: SubbandSamples = (0..2).map(|_| vec![[0.0f32; 36]; 32]).collect();
        read_triple_shared(&mut br, &st, 0, 0, 0, &mut samples).unwrap();

        let expected_frac = [2.0 / 3.0, 0.0, -2.0 / 3.0];
        let sf0 = crate::tables::scalefactor_magnitude(30);
        let sf1 = crate::tables::scalefactor_magnitude(45);
        assert_ne!(sf0, sf1, "test prerequisite: scalefactors must differ");
        for i in 0..3 {
            let l = samples[0][0][i];
            let r = samples[1][0][i];
            let expected_l = expected_frac[i] as f32 * sf0;
            let expected_r = expected_frac[i] as f32 * sf1;
            assert!(
                (l - expected_l).abs() < 1e-5,
                "L[{i}] = {l}, expected {expected_l}"
            );
            assert!(
                (r - expected_r).abs() < 1e-5,
                "R[{i}] = {r}, expected {expected_r}"
            );
        }
        // Non-zero entries (idx 0, 2) should have different L/R magnitudes
        // because the scalefactors differ.
        assert_ne!(samples[0][0][0], samples[1][0][0]);
    }
}
