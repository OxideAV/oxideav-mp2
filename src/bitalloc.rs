//! MPEG-1 Audio Layer II bit-allocation tables — ISO/IEC 11172-3 (1993)
//! Annex B, Table 3-B.2 ("Layer II bit allocation tables") and Table
//! 3-B.4 ("Layer II classes of quantization").
//!
//! Clean-room: every numeric value in this module is transcribed
//! directly from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (157-page edition with
//! Annex B; SHA-256
//! `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`),
//! PDF pages 46-50 (Tables 3-B.2a..d and 3-B.4). No third-party MP2
//! source was consulted.
//!
//! # §2.4.2.3 — table-selection rule
//!
//! The Layer II bit-allocation table is chosen by the (Fs,
//! `bitrate_per_channel`) pair, where `bitrate_per_channel = total /
//! channels` for `stereo` / `joint_stereo` / `dual_channel` and equals
//! the total for `single_channel` (the §2.4.2.3 "For Layer II, not all
//! combinations of total bitrate and mode are allowed" matrix is
//! already enforced by [`crate::header::is_layer2_bitrate_mode_allowed`]
//! before this lookup is reached). The four B.2 sub-tables list their
//! own `(Fs, per-channel bitrate)` coverage on their respective PDF
//! page headers:
//!
//! | sub-table | Fs = 48 kHz                                    | Fs = 44,1 kHz                     | Fs = 32 kHz                       |
//! |-----------|------------------------------------------------|-----------------------------------|-----------------------------------|
//! | B.2a      | 56, 64, 80, 96, 112, 128, 160, 192 (+ free)    | 56, 64, 80                        | 56, 64, 80                        |
//! | B.2b      | _not relevant_                                 | 96, 112, 128, 160, 192 (+ free)   | 96, 112, 128, 160, 192 (+ free)   |
//! | B.2c      | 32, 48                                         | 32, 48                            | _not relevant_                    |
//! | B.2d      | _not relevant_                                 | _not relevant_                    | 32, 48                            |
//!
//! Each sub-table fixes a `sblimit` (the highest active subband index +
//! 1; subbands `≥ sblimit` carry no allocation field), a per-subband
//! `nbal` width (2, 3, or 4 bits — the bit width of the
//! `allocation[ch][sb]` field for that subband), and the mapping from
//! an `nbal`-bit allocation index to the **number of quantization
//! steps** for that subband. The §2.4.2.3 sentinel allocation
//! `index == 0` means "no bits allocated for this subband", which
//! decodes to `nb_steps = 0`.
//!
//! # §2.4.2.3 — Table 3-B.4 classes of quantization
//!
//! Table 3-B.4 maps a `nb_steps` value (3, 5, 7, 9, 15, 31, 63, …,
//! 65535) to the per-class decode parameters: the requantization
//! coefficients `C` and `D` (PDF page 50; consumed in a follow-up
//! round by the §2.4.3.3.4 requantizer), whether the three subband
//! samples are **grouped** into a single codeword (`yes` only when
//! `nb_steps ∈ {3, 5, 9}`), the number of samples encoded per codeword
//! (3 when grouped, 1 otherwise), and the codeword bit width.

use crate::header::{FrameHeader, Mode};

/// MPEG-1 Layer II has 32 subbands (§2.4.1.6 prose; the
/// `samplecode[ch][sb][gr]` loop runs `sb` from `0` to `sblimit-1`
/// where `sblimit ≤ 32`).
pub const NUM_SUBBANDS: usize = 32;

/// The four §2.4.2.3 Layer II bit-allocation sub-tables, as named in
/// the ISO/IEC 11172-3 PDF page headers (Tables 3-B.2a, 3-B.2b,
/// 3-B.2c, 3-B.2d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitAllocTable {
    /// Table 3-B.2a — high-rate at 48 kHz, mid-rate at 44.1 / 32 kHz.
    B2a,
    /// Table 3-B.2b — mid-rate at 44.1 / 32 kHz.
    B2b,
    /// Table 3-B.2c — low-rate at 48 / 44.1 kHz.
    B2c,
    /// Table 3-B.2d — low-rate at 32 kHz.
    B2d,
}

impl BitAllocTable {
    /// Each B.2 sub-table fixes its own `sblimit` (§2.4.2.3 prose; PDF
    /// page footers under each table).
    pub fn sblimit(self) -> usize {
        match self {
            BitAllocTable::B2a => 27,
            BitAllocTable::B2b => 30,
            BitAllocTable::B2c => 8,
            BitAllocTable::B2d => 12,
        }
    }

    /// Per-subband `nbal` (the width in bits of the
    /// `allocation[ch][sb]` field for this subband). Subbands `≥
    /// sblimit` return `0` — there is no allocation field for them.
    pub fn nbal(self, sb: usize) -> u32 {
        if sb >= NUM_SUBBANDS {
            return 0;
        }
        match self {
            BitAllocTable::B2a => match sb {
                0..=10 => 4,
                11..=22 => 3,
                23..=26 => 2,
                _ => 0,
            },
            BitAllocTable::B2b => match sb {
                0..=10 => 4,
                11..=22 => 3,
                23..=29 => 2,
                _ => 0,
            },
            BitAllocTable::B2c => match sb {
                0 | 1 => 4,
                2..=7 => 3,
                _ => 0,
            },
            BitAllocTable::B2d => match sb {
                0 | 1 => 4,
                2..=11 => 3,
                _ => 0,
            },
        }
    }

    /// Decode an `nbal`-bit `allocation[ch][sb]` field to the number
    /// of quantization steps for subband `sb`. `index == 0` returns
    /// `Ok(0)` (the §2.4.2.3 sentinel for "no bits allocated"); other
    /// indices return the tabulated `nb_steps` per Tables 3-B.2a..d.
    ///
    /// Returns `None` for an out-of-range index given the active
    /// `nbal(sb)`, or for `sb ≥ sblimit`.
    pub fn nb_steps(self, sb: usize, index: u32) -> Option<u32> {
        if sb >= self.sblimit() {
            return None;
        }
        let nbal = self.nbal(sb);
        if nbal == 0 || index >= (1u32 << nbal) {
            return None;
        }
        if index == 0 {
            return Some(0);
        }
        let row = self.row(sb)?;
        // Indices 1..(1<<nbal) map into row[1..(1<<nbal)] — index 0
        // is the "-" sentinel and the row's first entry is unused.
        row.get(index as usize).copied()
    }

    /// Encoder-side inverse of [`Self::nb_steps`]: given the
    /// `nb_steps` value the encoder wants to record for subband `sb`,
    /// return the `nbal`-bit `allocation[ch][sb]` field code that
    /// [`Self::nb_steps`] would decode back to that same `nb_steps`.
    ///
    /// `nb_steps == 0` returns `Some(0)` — the §2.4.2.3 "no bits
    /// allocated" sentinel — since the row's entry-0 slot is fixed by
    /// the spec irrespective of subband.
    ///
    /// Returns `None` when:
    /// * `sb ≥ sblimit` (no allocation field exists for the subband
    ///   under this sub-table), or
    /// * `nb_steps` does not appear in the row for subband `sb` — the
    ///   §2.4.2.3 prose constrains the encoder to one of the
    ///   tabulated column values, so an off-row value is not
    ///   representable and must be rejected.
    ///
    /// The mapping is well-defined: every B.2 row carries each
    /// `nb_steps` value at most once (the columns are strictly
    /// monotonically increasing in the PDF), so the inverse is a
    /// total function on the row's range and the empty function
    /// elsewhere.
    pub fn allocation_index(self, sb: usize, nb_steps: u32) -> Option<u32> {
        if sb >= self.sblimit() {
            return None;
        }
        if nb_steps == 0 {
            return Some(0);
        }
        let row = self.row(sb)?;
        // Skip the unused entry-0 slot (§2.4.2.3 "-" sentinel) — only
        // indices 1..row.len() are tabulated values.
        row.iter()
            .enumerate()
            .skip(1)
            .find(|(_, &steps)| steps == nb_steps)
            .map(|(idx, _)| idx as u32)
    }

    /// Per-subband row of the active B.2 sub-table.
    ///
    /// The row is `1 << nbal(sb)` entries wide. Entry `0` is unused
    /// (sentinel "no allocation"); entries `1 ..= (1<<nbal)-1` are the
    /// `nb_steps` values transcribed from the PDF.
    fn row(self, sb: usize) -> Option<&'static [u32]> {
        let table: &[&[u32]] = match self {
            BitAllocTable::B2a => &B2A_ROWS,
            BitAllocTable::B2b => &B2B_ROWS,
            BitAllocTable::B2c => &B2C_ROWS,
            BitAllocTable::B2d => &B2D_ROWS,
        };
        table.get(sb).copied()
    }
}

/// §2.4.2.3 / §2.4.2.5 table-selection: given the parsed [`FrameHeader`]
/// pick the active Layer II bit-allocation sub-table.
///
/// Returns `None` if the (Fs, per-channel bitrate) pair is not covered
/// by any of the four B.2 sub-tables. With
/// [`crate::header::is_layer2_bitrate_mode_allowed`] enforced at parse
/// time, every header that survives [`FrameHeader::parse`] is covered;
/// callers may treat `None` as an internal-consistency failure.
pub fn select_table(header: &FrameHeader) -> Option<BitAllocTable> {
    let per_ch = bitrate_per_channel_kbps(header)?;
    match header.sample_rate {
        48_000 => match per_ch {
            32 | 48 => Some(BitAllocTable::B2c),
            56 | 64 | 80 | 96 | 112 | 128 | 160 | 192 => Some(BitAllocTable::B2a),
            _ => None,
        },
        44_100 => match per_ch {
            32 | 48 => Some(BitAllocTable::B2c),
            56 | 64 | 80 => Some(BitAllocTable::B2a),
            96 | 112 | 128 | 160 | 192 => Some(BitAllocTable::B2b),
            _ => None,
        },
        32_000 => match per_ch {
            32 | 48 => Some(BitAllocTable::B2d),
            56 | 64 | 80 => Some(BitAllocTable::B2a),
            96 | 112 | 128 | 160 | 192 => Some(BitAllocTable::B2b),
            _ => None,
        },
        _ => None,
    }
}

/// Per-channel bitrate in kbit/s used by [`select_table`]. For
/// `single_channel` mode the per-channel bitrate equals the total
/// bitrate; for the two-channel modes (`stereo`, `joint_stereo`,
/// `dual_channel`) the total is split evenly across the two channels.
pub fn bitrate_per_channel_kbps(header: &FrameHeader) -> Option<u32> {
    let total_kbps = header.bit_rate / 1000;
    match header.mode {
        Mode::SingleChannel => Some(total_kbps),
        Mode::Stereo | Mode::JointStereo | Mode::DualChannel => {
            if total_kbps % 2 == 0 {
                Some(total_kbps / 2)
            } else {
                // No Layer II ladder rate is odd; defensive None.
                None
            }
        }
    }
}

/// §2.4.2.3: "Grouping[ch][sb] is true, if in the Bit Allocation table
/// currently in use (see B.2) the value found under the sb (row) and
/// the allocation[sb] (column) is either 3, 5, or 9."
///
/// Equivalently: per Table 3-B.4 the classes of quantization where the
/// `grouping` column is `yes` are exactly `nb_steps ∈ {3, 5, 9}`.
pub fn is_grouped(nb_steps: u32) -> bool {
    matches!(nb_steps, 3 | 5 | 9)
}

impl QuantClass {
    /// Number of bits each *individual* requantized sample occupies once
    /// any §2.4.3.3.4 grouping has been undone.
    ///
    /// For an ungrouped class this is exactly [`Self::bits_per_codeword`]
    /// (one codeword == one sample). For a grouped class the codeword
    /// packs three samples, each carrying one of `nb_steps` levels, so a
    /// single sample spans `ceil(log2(nb_steps))` bits. The relation
    /// `bits_per_codeword == ceil(log2(nb_steps))` already holds for
    /// every ungrouped row, so this single closed form is correct for
    /// all 17 Table 3-B.4 classes:
    ///
    /// | nb_steps | grouping | bits/codeword | bits/sample |
    /// |---------:|:--------:|--------------:|------------:|
    /// | 3        | yes      | 5             | 2           |
    /// | 5        | yes      | 7             | 3           |
    /// | 9        | yes      | 10            | 4           |
    /// | 7        | no       | 3             | 3           |
    /// | 15       | no       | 4             | 4           |
    /// | 65535    | no       | 16            | 16          |
    pub fn bits_per_sample(self) -> u32 {
        // ceil(log2(nb_steps)) — the width needed to hold one degrouped
        // code in `0 ..= nb_steps - 1`.
        debug_assert!(self.nb_steps >= 2);
        32 - (self.nb_steps - 1).leading_zeros()
    }
}

/// Table 3-B.4 class of quantization for a given `nb_steps` value.
///
/// Returns `None` for the §2.4.2.3 sentinel `nb_steps == 0` ("no bits
/// allocated for this subband") and for any value not in the
/// 17-entry Table 3-B.4 column.
pub fn class_of_quantization(nb_steps: u32) -> Option<QuantClass> {
    QUANT_CLASSES
        .iter()
        .find(|c| c.nb_steps == nb_steps)
        .copied()
}

/// A row of Table 3-B.4 "Layer II classes of quantization" (PDF page
/// 50): the requantization constants `C` and `D` (consumed by
/// §2.4.3.3.4 in a follow-up round), the §2.4.2.3 `grouping` flag,
/// and the codeword shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuantClass {
    /// Number of quantization steps (the column header for B.2a..d
    /// and the lookup key for B.4).
    pub nb_steps: u32,
    /// Requantization constant `C` (PDF page 50).
    pub c: f64,
    /// Requantization constant `D` (PDF page 50).
    pub d: f64,
    /// `grouping` flag (§2.4.2.3 / Table 3-B.4 column "grouping").
    /// True when three subband samples are encoded as a single
    /// `samplecode` codeword.
    pub grouping: bool,
    /// Samples carried per codeword (`3` when `grouping`, else `1`).
    pub samples_per_codeword: u32,
    /// Codeword width in bits.
    pub bits_per_codeword: u32,
}

/// Table 3-B.4 transcribed verbatim from PDF page 50 (17 entries).
///
/// The ordering matches the PDF: rows go in the same order as the
/// `Number of steps` column header — `3, 5, 7, 9, 15, 31, …, 65535`.
const QUANT_CLASSES: [QuantClass; 17] = [
    QuantClass {
        nb_steps: 3,
        c: 1.333_333_333_33,
        d: 0.500_000_000_00,
        grouping: true,
        samples_per_codeword: 3,
        bits_per_codeword: 5,
    },
    QuantClass {
        nb_steps: 5,
        c: 1.600_000_000_00,
        d: 0.500_000_000_00,
        grouping: true,
        samples_per_codeword: 3,
        bits_per_codeword: 7,
    },
    QuantClass {
        nb_steps: 7,
        c: 1.142_857_142_86,
        d: 0.250_000_000_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 3,
    },
    QuantClass {
        nb_steps: 9,
        c: 1.777_777_777_77,
        d: 0.500_000_000_00,
        grouping: true,
        samples_per_codeword: 3,
        bits_per_codeword: 10,
    },
    QuantClass {
        nb_steps: 15,
        c: 1.066_666_666_66,
        d: 0.125_000_000_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 4,
    },
    QuantClass {
        nb_steps: 31,
        c: 1.032_258_064_52,
        d: 0.062_500_000_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 5,
    },
    QuantClass {
        nb_steps: 63,
        c: 1.015_873_015_87,
        d: 0.031_250_000_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 6,
    },
    QuantClass {
        nb_steps: 127,
        c: 1.007_874_015_75,
        d: 0.015_625_000_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 7,
    },
    QuantClass {
        nb_steps: 255,
        c: 1.003_921_568_63,
        d: 0.007_812_500_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 8,
    },
    QuantClass {
        nb_steps: 511,
        c: 1.001_956_947_16,
        d: 0.003_906_250_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 9,
    },
    QuantClass {
        nb_steps: 1023,
        c: 1.000_977_517_11,
        d: 0.001_953_125_00,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 10,
    },
    QuantClass {
        nb_steps: 2047,
        c: 1.000_488_519_79,
        d: 0.000_976_562_50,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 11,
    },
    QuantClass {
        nb_steps: 4095,
        c: 1.000_244_200_24,
        d: 0.000_488_281_25,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 12,
    },
    QuantClass {
        nb_steps: 8191,
        c: 1.000_122_085_22,
        d: 0.000_244_140_63,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 13,
    },
    QuantClass {
        nb_steps: 16383,
        c: 1.000_061_038_88,
        d: 0.000_122_070_31,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 14,
    },
    QuantClass {
        nb_steps: 32767,
        c: 1.000_030_518_51,
        d: 0.000_061_035_16,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 15,
    },
    QuantClass {
        nb_steps: 65535,
        c: 1.000_015_259_02,
        d: 0.000_030_517_58,
        grouping: false,
        samples_per_codeword: 1,
        bits_per_codeword: 16,
    },
];

// =========================================================================
// Table 3-B.2a — PDF page 46 (sblimit = 27, sum of nbal = 88)
// =========================================================================
//
// Per-subband rows. Each row has `1 << nbal(sb)` entries; entry [0] is
// the "-" sentinel (no allocation). The remaining entries are the
// `nb_steps` values printed in the PDF row for that subband.
//
// Subbands 0..=2: nbal=4, row = `- 3 7 15 31 63 127 255 511 1023 2047 4095 8191 16383 32767 65535`
// Subbands 3..=10: nbal=4, row = `- 3 5 7 9 15 31 63 127 255 511 1023 2047 4095 8191 65535`
// Subbands 11..=22: nbal=3, row = `- 3 5 7 9 15 31 65535`
// Subbands 23..=26: nbal=2, row = `- 3 5 65535`
// Subbands 27..=31: nbal=0 (no allocation field).

const B2A_ROW_0_TO_2: &[u32] = &[
    0, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
];
const B2A_ROW_3_TO_10: &[u32] = &[
    0, 3, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 65535,
];
const B2_NBAL3_ROW: &[u32] = &[0, 3, 5, 7, 9, 15, 31, 65535];
const B2_NBAL2_ROW: &[u32] = &[0, 3, 5, 65535];
const B2_EMPTY_ROW: &[u32] = &[];

const B2A_ROWS: [&[u32]; NUM_SUBBANDS] = [
    B2A_ROW_0_TO_2,
    B2A_ROW_0_TO_2,
    B2A_ROW_0_TO_2,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
];

// =========================================================================
// Table 3-B.2b — PDF page 47 (sblimit = 30, sum of nbal = 94)
// =========================================================================
//
// Subbands 0..=10: same rows as B.2a.
// Subbands 11..=22: nbal=3 row (same as B.2a).
// Subbands 23..=29: nbal=2 row (extends B.2a's 23..=26 by three more).
// Subbands 30..=31: nbal=0.

const B2B_ROWS: [&[u32]; NUM_SUBBANDS] = [
    B2A_ROW_0_TO_2,
    B2A_ROW_0_TO_2,
    B2A_ROW_0_TO_2,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2A_ROW_3_TO_10,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL3_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_NBAL2_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
];

// =========================================================================
// Table 3-B.2c — PDF page 48 (sblimit = 8, sum of nbal = 26)
// =========================================================================
//
// Subbands 0..=1: nbal=4, row = `- 3 5 9 15 31 63 127 255 511 1023 2047 4095 8191 16383 32767`
// Subbands 2..=7: nbal=3, row = `- 3 5 9 15 31 63 127`
// Subbands 8..=31: nbal=0.

const B2C_ROW_0_TO_1: &[u32] = &[
    0, 3, 5, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767,
];
const B2C_ROW_2_TO_7: &[u32] = &[0, 3, 5, 9, 15, 31, 63, 127];

const B2C_ROWS: [&[u32]; NUM_SUBBANDS] = [
    B2C_ROW_0_TO_1,
    B2C_ROW_0_TO_1,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
];

// =========================================================================
// Table 3-B.2d — PDF page 49 (sblimit = 12)
// =========================================================================
//
// Subbands 0..=1: nbal=4, same row as B.2c subbands 0..=1.
// Subbands 2..=11: nbal=3, same row as B.2c subbands 2..=7.
// Subbands 12..=31: nbal=0.

const B2D_ROWS: [&[u32]; NUM_SUBBANDS] = [
    B2C_ROW_0_TO_1,
    B2C_ROW_0_TO_1,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2C_ROW_2_TO_7,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
    B2_EMPTY_ROW,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{Emphasis, ModeExtension};

    fn make_header(bit_rate: u32, sample_rate: u32, mode: Mode) -> FrameHeader {
        FrameHeader {
            bit_rate,
            sample_rate,
            padding: false,
            private_bit: false,
            mode,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
            protection_bit: true,
        }
    }

    #[test]
    fn b2a_sblimit_and_nbal_layout_match_pdf_page_46() {
        let t = BitAllocTable::B2a;
        assert_eq!(t.sblimit(), 27);

        // Sum of nbal = 88 over all 32 subbands (PDF page 46 footer).
        let sum: u32 = (0..NUM_SUBBANDS).map(|sb| t.nbal(sb)).sum();
        assert_eq!(sum, 88);

        for sb in 0..=10 {
            assert_eq!(t.nbal(sb), 4, "B2a sb={sb} nbal");
        }
        for sb in 11..=22 {
            assert_eq!(t.nbal(sb), 3, "B2a sb={sb} nbal");
        }
        for sb in 23..=26 {
            assert_eq!(t.nbal(sb), 2, "B2a sb={sb} nbal");
        }
        for sb in 27..NUM_SUBBANDS {
            assert_eq!(t.nbal(sb), 0, "B2a sb={sb} nbal");
        }
    }

    #[test]
    fn b2b_sblimit_and_nbal_layout_match_pdf_page_47() {
        let t = BitAllocTable::B2b;
        assert_eq!(t.sblimit(), 30);
        let sum: u32 = (0..NUM_SUBBANDS).map(|sb| t.nbal(sb)).sum();
        assert_eq!(sum, 94);

        for sb in 0..=10 {
            assert_eq!(t.nbal(sb), 4);
        }
        for sb in 11..=22 {
            assert_eq!(t.nbal(sb), 3);
        }
        for sb in 23..=29 {
            assert_eq!(t.nbal(sb), 2);
        }
        for sb in 30..NUM_SUBBANDS {
            assert_eq!(t.nbal(sb), 0);
        }
    }

    #[test]
    fn b2c_sblimit_and_nbal_layout_match_pdf_page_48() {
        let t = BitAllocTable::B2c;
        assert_eq!(t.sblimit(), 8);
        let sum: u32 = (0..NUM_SUBBANDS).map(|sb| t.nbal(sb)).sum();
        // Sum of nbal = 4*2 + 3*6 = 26 (PDF page 48 footer).
        assert_eq!(sum, 26);

        assert_eq!(t.nbal(0), 4);
        assert_eq!(t.nbal(1), 4);
        for sb in 2..=7 {
            assert_eq!(t.nbal(sb), 3);
        }
        for sb in 8..NUM_SUBBANDS {
            assert_eq!(t.nbal(sb), 0);
        }
    }

    #[test]
    fn b2d_sblimit_and_nbal_layout_match_pdf_page_49() {
        let t = BitAllocTable::B2d;
        assert_eq!(t.sblimit(), 12);

        assert_eq!(t.nbal(0), 4);
        assert_eq!(t.nbal(1), 4);
        for sb in 2..=11 {
            assert_eq!(t.nbal(sb), 3);
        }
        for sb in 12..NUM_SUBBANDS {
            assert_eq!(t.nbal(sb), 0);
        }
    }

    #[test]
    fn b2a_subbands_0_to_2_decode_to_nb_steps_power_of_two_minus_one() {
        // Row: - 3 7 15 31 63 127 255 511 1023 2047 4095 8191 16383 32767 65535
        let expected = [
            0u32, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
        ];
        for sb in 0..=2 {
            for (idx, &steps) in expected.iter().enumerate() {
                assert_eq!(
                    BitAllocTable::B2a.nb_steps(sb, idx as u32),
                    Some(steps),
                    "B2a sb={sb} idx={idx}"
                );
            }
        }
    }

    #[test]
    fn b2a_subbands_3_to_10_decode_to_the_short_row() {
        // Row: - 3 5 7 9 15 31 63 127 255 511 1023 2047 4095 8191 65535
        let expected = [
            0u32, 3, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 65535,
        ];
        for sb in 3..=10 {
            for (idx, &steps) in expected.iter().enumerate() {
                assert_eq!(
                    BitAllocTable::B2a.nb_steps(sb, idx as u32),
                    Some(steps),
                    "B2a sb={sb} idx={idx}"
                );
            }
        }
    }

    #[test]
    fn b2_nbal3_and_nbal2_rows_decode_uniformly() {
        // nbal=3 rows live at B2a sb=11..22, B2b sb=11..22.
        let n3 = [0u32, 3, 5, 7, 9, 15, 31, 65535];
        for &table in &[BitAllocTable::B2a, BitAllocTable::B2b] {
            for sb in 11..=22 {
                for (idx, &steps) in n3.iter().enumerate() {
                    assert_eq!(table.nb_steps(sb, idx as u32), Some(steps));
                }
            }
        }

        // nbal=2 rows: B2a sb=23..26, B2b sb=23..29.
        let n2 = [0u32, 3, 5, 65535];
        for sb in 23..=26 {
            for (idx, &steps) in n2.iter().enumerate() {
                assert_eq!(BitAllocTable::B2a.nb_steps(sb, idx as u32), Some(steps));
            }
        }
        for sb in 23..=29 {
            for (idx, &steps) in n2.iter().enumerate() {
                assert_eq!(BitAllocTable::B2b.nb_steps(sb, idx as u32), Some(steps));
            }
        }
    }

    #[test]
    fn b2c_and_b2d_low_rate_rows_match_pdf_pages_48_and_49() {
        let nbal4 = [
            0u32, 3, 5, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767,
        ];
        let nbal3 = [0u32, 3, 5, 9, 15, 31, 63, 127];

        for sb in 0..=1 {
            for (idx, &steps) in nbal4.iter().enumerate() {
                assert_eq!(BitAllocTable::B2c.nb_steps(sb, idx as u32), Some(steps));
                assert_eq!(BitAllocTable::B2d.nb_steps(sb, idx as u32), Some(steps));
            }
        }
        for sb in 2..=7 {
            for (idx, &steps) in nbal3.iter().enumerate() {
                assert_eq!(BitAllocTable::B2c.nb_steps(sb, idx as u32), Some(steps));
                assert_eq!(BitAllocTable::B2d.nb_steps(sb, idx as u32), Some(steps));
            }
        }
        // B2d extends nbal=3 row through sb=11.
        for sb in 8..=11 {
            for (idx, &steps) in nbal3.iter().enumerate() {
                assert_eq!(BitAllocTable::B2d.nb_steps(sb, idx as u32), Some(steps));
            }
        }
    }

    #[test]
    fn nb_steps_returns_none_past_sblimit_or_for_out_of_range_index() {
        assert_eq!(BitAllocTable::B2a.nb_steps(27, 0), None);
        assert_eq!(BitAllocTable::B2c.nb_steps(8, 0), None);
        // index 16 with nbal=4 is one past the last column.
        assert_eq!(BitAllocTable::B2a.nb_steps(0, 16), None);
        // index 4 with nbal=2.
        assert_eq!(BitAllocTable::B2a.nb_steps(23, 4), None);
    }

    #[test]
    fn select_table_at_48_khz_picks_the_right_subtable() {
        // 48 kHz stereo at 192 kbit/s total = 96 per-channel → B.2a.
        let h = make_header(192_000, 48_000, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2a));
        // 48 kHz stereo at 64 kbit/s = 32 per-channel → B.2c.
        let h = make_header(64_000, 48_000, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2c));
        // 48 kHz single_channel at 48 kbit/s = 48 per-channel → B.2c.
        let h = make_header(48_000, 48_000, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2c));
        // 48 kHz single_channel at 32 kbit/s → B.2c (per-channel = 32).
        let h = make_header(32_000, 48_000, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2c));
        // 48 kHz single_channel at 56 kbit/s → B.2a (per-channel = 56).
        let h = make_header(56_000, 48_000, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2a));
    }

    #[test]
    fn select_table_at_44k1_hz_picks_the_right_subtable() {
        // 44.1 kHz stereo at 192 = 96 per-channel → B.2b.
        let h = make_header(192_000, 44_100, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2b));
        // 44.1 kHz stereo at 160 = 80 per-channel → B.2a.
        let h = make_header(160_000, 44_100, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2a));
        // 44.1 kHz single_channel at 96 → B.2b.
        let h = make_header(96_000, 44_100, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2b));
        // 44.1 kHz single_channel at 32 → B.2c.
        let h = make_header(32_000, 44_100, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2c));
    }

    #[test]
    fn select_table_at_32_khz_picks_b2d_for_low_rates() {
        // 32 kHz single_channel at 32 → B.2d.
        let h = make_header(32_000, 32_000, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2d));
        // 32 kHz single_channel at 48 → B.2d.
        let h = make_header(48_000, 32_000, Mode::SingleChannel);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2d));
        // 32 kHz stereo at 192 = 96 per-channel → B.2b.
        let h = make_header(192_000, 32_000, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2b));
        // 32 kHz stereo at 128 = 64 per-channel → B.2a.
        let h = make_header(128_000, 32_000, Mode::Stereo);
        assert_eq!(select_table(&h), Some(BitAllocTable::B2a));
    }

    #[test]
    fn grouping_flag_matches_iso_prose_three_five_or_nine() {
        for &nb in &[3u32, 5, 9] {
            assert!(is_grouped(nb), "{nb} should be grouped");
        }
        for &nb in &[0u32, 7, 15, 31, 63, 127, 65535] {
            assert!(!is_grouped(nb), "{nb} should NOT be grouped");
        }
    }

    #[test]
    fn quant_class_table_b4_lookup_matches_pdf_page_50() {
        // Spot-check several classes.
        let c3 = class_of_quantization(3).unwrap();
        assert_eq!(c3.bits_per_codeword, 5);
        assert_eq!(c3.samples_per_codeword, 3);
        assert!(c3.grouping);

        let c5 = class_of_quantization(5).unwrap();
        assert_eq!(c5.bits_per_codeword, 7);

        let c7 = class_of_quantization(7).unwrap();
        assert_eq!(c7.bits_per_codeword, 3);
        assert!(!c7.grouping);

        let c9 = class_of_quantization(9).unwrap();
        assert_eq!(c9.bits_per_codeword, 10);
        assert!(c9.grouping);

        let c15 = class_of_quantization(15).unwrap();
        assert_eq!(c15.bits_per_codeword, 4);
        assert!(!c15.grouping);

        let c65535 = class_of_quantization(65535).unwrap();
        assert_eq!(c65535.bits_per_codeword, 16);
        assert!(!c65535.grouping);

        // The 6-bits class corresponds to 63 steps.
        assert_eq!(class_of_quantization(63).unwrap().bits_per_codeword, 6);
        // Unknown nb_steps returns None.
        assert!(class_of_quantization(0).is_none());
        assert!(class_of_quantization(2).is_none());
        assert!(class_of_quantization(64).is_none());
    }

    #[test]
    fn bits_per_sample_is_ceil_log2_nb_steps() {
        // Grouped classes: codeword packs 3 samples, so the per-sample
        // width is strictly less than the codeword width.
        let g = [(3u32, 5u32, 2u32), (5, 7, 3), (9, 10, 4)];
        for (nb, cw, bps) in g {
            let c = class_of_quantization(nb).unwrap();
            assert_eq!(c.bits_per_codeword, cw, "nb_steps={nb} codeword width");
            assert_eq!(c.bits_per_sample(), bps, "nb_steps={nb} sample width");
        }
        // Ungrouped classes: one codeword == one sample.
        for nb in [7u32, 15, 31, 63, 127, 255, 511, 1023, 65535] {
            let c = class_of_quantization(nb).unwrap();
            assert_eq!(
                c.bits_per_sample(),
                c.bits_per_codeword,
                "ungrouped nb_steps={nb}"
            );
            // ceil(log2(nb_steps)) cross-check.
            let want = (nb as f64).log2().ceil() as u32;
            assert_eq!(c.bits_per_sample(), want, "nb_steps={nb} ceil(log2)");
        }
    }

    #[test]
    fn quant_class_grouping_aligns_with_is_grouped_helper() {
        for c in QUANT_CLASSES {
            assert_eq!(
                c.grouping,
                is_grouped(c.nb_steps),
                "nb_steps={}",
                c.nb_steps
            );
            assert_eq!(
                c.samples_per_codeword,
                if c.grouping { 3 } else { 1 },
                "nb_steps={}",
                c.nb_steps
            );
        }
    }

    #[test]
    fn every_b2_table_cell_resolves_to_a_known_b4_class() {
        // For every (table, sb, allocation) with allocation > 0, the
        // resulting nb_steps must appear in Table 3-B.4.
        for table in [
            BitAllocTable::B2a,
            BitAllocTable::B2b,
            BitAllocTable::B2c,
            BitAllocTable::B2d,
        ] {
            for sb in 0..table.sblimit() {
                let nbal = table.nbal(sb);
                for idx in 1..(1u32 << nbal) {
                    let nb = table.nb_steps(sb, idx).unwrap();
                    assert!(
                        class_of_quantization(nb).is_some(),
                        "table={:?} sb={sb} idx={idx} nb_steps={nb} missing in B.4",
                        table
                    );
                }
            }
        }
    }

    #[test]
    fn allocation_index_is_total_inverse_of_nb_steps_for_every_cell() {
        // The §2.4.2.3 encoder primitive: for every defined
        // (table, sb, on-wire index) triple, the encoder-side
        // `allocation_index(sb, nb_steps)` must round-trip back to
        // that same on-wire index. Coverage is exhaustive across all
        // four B.2 sub-tables: B.2a (27 subbands), B.2b (30), B.2c (8),
        // B.2d (12).
        for table in [
            BitAllocTable::B2a,
            BitAllocTable::B2b,
            BitAllocTable::B2c,
            BitAllocTable::B2d,
        ] {
            for sb in 0..table.sblimit() {
                let nbal = table.nbal(sb);
                for idx in 0..(1u32 << nbal) {
                    let nb = table.nb_steps(sb, idx).unwrap_or_else(|| {
                        panic!("nb_steps None at table={table:?} sb={sb} idx={idx}")
                    });
                    let back = table.allocation_index(sb, nb).unwrap_or_else(|| {
                        panic!("allocation_index None at table={table:?} sb={sb} nb_steps={nb}")
                    });
                    assert_eq!(
                        back, idx,
                        "round-trip table={table:?} sb={sb}: index {idx} -> nb_steps {nb} -> index {back}"
                    );
                }
            }
        }
    }

    #[test]
    fn allocation_index_zero_sentinel_returns_zero_for_every_in_range_subband() {
        // §2.4.2.3 "-" sentinel — `nb_steps = 0` always encodes as
        // the all-zero allocation field, regardless of subband or
        // sub-table.
        for table in [
            BitAllocTable::B2a,
            BitAllocTable::B2b,
            BitAllocTable::B2c,
            BitAllocTable::B2d,
        ] {
            for sb in 0..table.sblimit() {
                assert_eq!(
                    table.allocation_index(sb, 0),
                    Some(0),
                    "zero sentinel table={table:?} sb={sb}"
                );
            }
        }
    }

    #[test]
    fn allocation_index_rejects_subbands_at_or_past_sblimit() {
        // No allocation field exists for sb ≥ sblimit, so even the
        // zero sentinel is unrepresentable.
        for (table, sblimit) in [
            (BitAllocTable::B2a, 27usize),
            (BitAllocTable::B2b, 30),
            (BitAllocTable::B2c, 8),
            (BitAllocTable::B2d, 12),
        ] {
            for sb in sblimit..NUM_SUBBANDS {
                assert_eq!(
                    table.allocation_index(sb, 0),
                    None,
                    "table={table:?} sb={sb} (≥ sblimit={sblimit}) must be None"
                );
                // Off-row nb_steps similarly None.
                assert_eq!(table.allocation_index(sb, 3), None);
                assert_eq!(table.allocation_index(sb, 65535), None);
            }
        }
    }

    #[test]
    fn allocation_index_rejects_off_row_nb_steps() {
        // §2.4.2.3 constrains the encoder to one of the tabulated
        // column values; any other nb_steps is not representable.
        //
        // The nbal=4 wide row (B.2a sb=0..=2) carries
        // {3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191,
        //  16383, 32767, 65535}. `5` and `9` are explicitly NOT in
        // that row (they live in the nbal=4 short row, B.2a sb=3..=10).
        assert_eq!(BitAllocTable::B2a.allocation_index(0, 5), None);
        assert_eq!(BitAllocTable::B2a.allocation_index(0, 9), None);
        // ... but they DO appear in the short row.
        assert_eq!(BitAllocTable::B2a.allocation_index(3, 5), Some(2));
        assert_eq!(BitAllocTable::B2a.allocation_index(3, 9), Some(4));

        // The nbal=3 row carries {3, 5, 7, 9, 15, 31, 65535}; `63`
        // is one column code beyond and not representable.
        assert_eq!(BitAllocTable::B2a.allocation_index(11, 63), None);
        // ... but `31` is.
        assert_eq!(BitAllocTable::B2a.allocation_index(11, 31), Some(6));

        // The nbal=2 row carries {3, 5, 65535}. `7` is not there.
        assert_eq!(BitAllocTable::B2a.allocation_index(23, 7), None);
        assert_eq!(BitAllocTable::B2a.allocation_index(23, 65535), Some(3));

        // Arbitrary out-of-table values fail uniformly.
        for nb in [1u32, 2, 4, 6, 8, 10, 11, 16, 99] {
            assert_eq!(
                BitAllocTable::B2a.allocation_index(0, nb),
                None,
                "off-row nb_steps={nb} should be None for B2a sb=0"
            );
        }
    }

    #[test]
    fn allocation_index_matches_pdf_rows_for_b2c_and_b2d() {
        // B.2c / B.2d wide row (`- 3 5 9 15 31 63 127 255 511 1023
        // 2047 4095 8191 16383 32767`): indices 1..=15 map to those
        // 15 column values in order.
        let wide_row = [
            0u32, 3, 5, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767,
        ];
        for sb in 0..=1 {
            for (idx, &nb) in wide_row.iter().enumerate().skip(1) {
                assert_eq!(
                    BitAllocTable::B2c.allocation_index(sb, nb),
                    Some(idx as u32)
                );
                assert_eq!(
                    BitAllocTable::B2d.allocation_index(sb, nb),
                    Some(idx as u32)
                );
            }
        }

        // B.2c / B.2d short row (`- 3 5 9 15 31 63 127`): 7 column
        // values in order.
        let short_row = [0u32, 3, 5, 9, 15, 31, 63, 127];
        for sb in 2..=7 {
            for (idx, &nb) in short_row.iter().enumerate().skip(1) {
                assert_eq!(
                    BitAllocTable::B2c.allocation_index(sb, nb),
                    Some(idx as u32)
                );
                assert_eq!(
                    BitAllocTable::B2d.allocation_index(sb, nb),
                    Some(idx as u32)
                );
            }
        }
        // B.2d extends the short row through sb=11.
        for sb in 8..=11 {
            for (idx, &nb) in short_row.iter().enumerate().skip(1) {
                assert_eq!(
                    BitAllocTable::B2d.allocation_index(sb, nb),
                    Some(idx as u32)
                );
            }
        }
    }
}
