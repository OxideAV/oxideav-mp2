//! MPEG-1 Audio Layer II audio-data parser — ISO/IEC 11172-3 (1993)
//! §2.4.1.6 ("Audio data, Layer II") and §2.4.3.3.1..3 (bit
//! allocation, scfsi, scalefactor decode).
//!
//! Clean-room: the §2.4.1.6 loop structure and field semantics in
//! this module are derived directly from the staged ISO/IEC PDF
//! (`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`, SHA-256
//! `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`),
//! pages 16, 22-25 (syntax + field-meaning prose). No third-party MP2
//! source was consulted.
//!
//! This module covers the first half of the audio-data path:
//!
//! 1. **§2.4.3.3.1 bit allocation** — `allocation[ch][sb]` is parsed
//!    out of the bitstream with the per-subband `nbal` widths from
//!    [`crate::bitalloc::BitAllocTable`], and translated to the
//!    per-subband number of quantization steps. Subbands above the
//!    `bound` in `joint_stereo` mode share a single allocation
//!    across the two channels (the §2.4.1.6 prose
//!    "`allocation[1][sb] = allocation[0][sb]`").
//! 2. **§2.4.3.3.2 scfsi** — for every subband whose allocation is
//!    non-zero, a 2-bit `scfsi[ch][sb]` field is parsed. The
//!    [`Scfsi`] enum models the four §2.4.2.3 schedule codes.
//! 3. **§2.4.3.3.3 scalefactor decode** — the scfsi schedule dictates
//!    how many of the three 6-bit `scalefactor[ch][sb][part]` indices
//!    actually appear on the wire (1, 2, or 3); the missing parts are
//!    re-filled from the present ones per the §2.4.2.3 schedule
//!    semantics ("scalefactors transmitted for granule 0 are also
//!    valid for granule 1" etc.).
//!
//! The §2.4.3.3.4 sample requantization (which consumes the
//! [`crate::bitalloc::QuantClass`] / Table 3-B.4 constants) and the
//! §2.4.3.2 polyphase synthesis filter (which consumes Table 3-B.3)
//! are NOT yet wired up here. They are the next coherent step in the
//! rebuild.

use core::fmt;

use oxideav_core::bits::BitReader;

use crate::bitalloc::{select_table, BitAllocTable, NUM_SUBBANDS};
use crate::header::{FrameHeader, Mode};

/// Errors that can arise while parsing the §2.4.1.6 audio-data
/// section of a Layer II frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDataError {
    /// Bitstream ran out of bits before the §2.4.1.6 loop completed.
    UnexpectedEnd,
    /// `bitrate_index` / `sampling_frequency` did not map to one of
    /// Tables 3-B.2a..d. The frame header validation already rejects
    /// the disallowed `(bitrate, mode)` combinations, so this is
    /// effectively an internal-consistency failure.
    NoBitallocTable,
    /// A parsed `allocation[ch][sb]` field decoded to an index that is
    /// out of range for the active sub-table column for that subband
    /// (indices beyond `1 << nbal` cannot happen because exactly
    /// `nbal` bits are read, but this guards against table layout
    /// bugs).
    InvalidAllocationIndex {
        /// The active B.2 sub-table.
        table: BitAllocTable,
        /// Subband index.
        sb: usize,
        /// The decoded allocation index that failed the table lookup.
        index: u32,
    },
    /// A parsed `scalefactor[ch][sb][part]` index decoded to a value
    /// `≥ 63` (the §2.4.2.5 "scalefactor index" reserves index 63 as
    /// undefined; only indices 0..=62 select an entry of
    /// [`crate::tables::SCALEFACTORS`]).
    ReservedScalefactorIndex {
        ch: usize,
        sb: usize,
        part: usize,
        index: u32,
    },
}

impl fmt::Display for AudioDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioDataError::UnexpectedEnd => write!(f, "audio-data: bitstream ended prematurely"),
            AudioDataError::NoBitallocTable => write!(
                f,
                "audio-data: (Fs, bitrate) does not select any Layer II bit-allocation sub-table"
            ),
            AudioDataError::InvalidAllocationIndex { table, sb, index } => write!(
                f,
                "audio-data: allocation index {index} out of range for table {table:?} subband {sb}"
            ),
            AudioDataError::ReservedScalefactorIndex {
                ch,
                sb,
                part,
                index,
            } => write!(
                f,
                "audio-data: scalefactor[ch={ch}][sb={sb}][part={part}] index {index} is reserved (only 0..=62 are defined)"
            ),
        }
    }
}

impl std::error::Error for AudioDataError {}

/// §2.4.2.3 scfsi schedule for a single (ch, sb). The two-bit
/// `scfsi[ch][sb]` field selects which of the three granules of
/// 12-sample subband data carry an independent scalefactor.
///
/// The §2.4.2.3 prose maps:
///
/// | scfsi | parts on the wire | granule 0 | granule 1 | granule 2 |
/// |-------|--------------------|-----------|-----------|-----------|
/// | `'00'` | 3 (part 0, part 1, part 2) | scalefactor\[0] | scalefactor\[1] | scalefactor\[2] |
/// | `'01'` | 2 (part 0, part 2) | scalefactor\[0] | scalefactor\[0] | scalefactor\[2] |
/// | `'10'` | 1 (part 0) | scalefactor\[0] | scalefactor\[0] | scalefactor\[0] |
/// | `'11'` | 2 (part 0, part 2) | scalefactor\[0] | scalefactor\[2] | scalefactor\[2] |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scfsi {
    /// `'00'` — three scalefactors, one per granule.
    ThreePerGranule,
    /// `'01'` — two scalefactors; first covers granules 0 & 1,
    /// second covers granule 2.
    Share01Then2,
    /// `'10'` — one scalefactor; reused across all three granules.
    ShareAll,
    /// `'11'` — two scalefactors; first covers granule 0,
    /// second covers granules 1 & 2.
    Share0Then12,
}

impl Scfsi {
    fn from_bits(bits: u32) -> Self {
        match bits & 0b11 {
            0b00 => Scfsi::ThreePerGranule,
            0b01 => Scfsi::Share01Then2,
            0b10 => Scfsi::ShareAll,
            _ => Scfsi::Share0Then12,
        }
    }

    /// Number of 6-bit scalefactor indices that are physically read
    /// from the bitstream for this scfsi value.
    pub fn parts_on_wire(self) -> usize {
        match self {
            Scfsi::ThreePerGranule => 3,
            Scfsi::Share01Then2 | Scfsi::Share0Then12 => 2,
            Scfsi::ShareAll => 1,
        }
    }
}

/// Maximum number of channels in MPEG-1 audio.
pub const MAX_CHANNELS: usize = 2;

/// Decoded §2.4.1.6 audio-data side info (bit allocation + scfsi +
/// scalefactor triplets) for a single Layer II frame.
///
/// Sample requantization (§2.4.3.3.4) is the next step; this struct is
/// the input it will consume.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioData {
    /// The B.2 sub-table the §2.4.1.6 loop indexed against.
    pub table: BitAllocTable,
    /// `sblimit` from the active sub-table.
    pub sblimit: usize,
    /// `bound` from §2.4.2.3 ("subbands `[bound, sblimit)` carry one
    /// allocation across both channels in `joint_stereo`; the
    /// `bound` equals `sblimit` for the non-joint stereo modes").
    pub bound: usize,
    /// Number of channels (1 or 2) — drawn from the frame header
    /// `mode` field per §2.2.6.
    pub channels: usize,
    /// Per-channel, per-subband number of quantization steps. `0`
    /// signifies "no bits allocated" (§2.4.2.3 sentinel). Entries
    /// beyond `sblimit` are always `0`.
    pub nb_steps: [[u32; NUM_SUBBANDS]; MAX_CHANNELS],
    /// Per-channel, per-subband scfsi schedule. Only meaningful where
    /// the corresponding `nb_steps` entry is non-zero;
    /// [`Scfsi::ThreePerGranule`] for the rest.
    pub scfsi: [[Scfsi; NUM_SUBBANDS]; MAX_CHANNELS],
    /// Per-channel, per-subband, per-granule scalefactor index
    /// (`0..=62`, indexing [`crate::tables::SCALEFACTORS`]). Slots
    /// not on the wire are filled per the §2.4.2.3 scfsi schedule.
    /// Slots whose subband carries no allocation are left at `0`.
    pub scalefactor: [[[u8; 3]; NUM_SUBBANDS]; MAX_CHANNELS],
}

impl AudioData {
    fn new(table: BitAllocTable, bound: usize, channels: usize) -> Self {
        AudioData {
            table,
            sblimit: table.sblimit(),
            bound,
            channels,
            nb_steps: [[0; NUM_SUBBANDS]; MAX_CHANNELS],
            scfsi: [[Scfsi::ThreePerGranule; NUM_SUBBANDS]; MAX_CHANNELS],
            scalefactor: [[[0u8; 3]; NUM_SUBBANDS]; MAX_CHANNELS],
        }
    }
}

/// Parse the §2.4.1.6 audio-data side info (bit allocation + scfsi +
/// scalefactor triplets) from `reader` against the supplied
/// [`FrameHeader`].
///
/// The reader is advanced past the parsed fields; it stops at the
/// boundary where the §2.4.1.6 `samplecode[]` / `sample[]` loop would
/// begin. That sample loop is consumed by the §2.4.3.3.4 requantizer
/// in a follow-up step.
pub fn parse_audio_data(
    header: &FrameHeader,
    reader: &mut BitReader<'_>,
) -> Result<AudioData, AudioDataError> {
    let table = select_table(header).ok_or(AudioDataError::NoBitallocTable)?;
    let channels = header.channels();

    // §2.4.2.3 prose (pages 22-23):
    //   "In Layer I, in all modes except joint stereo, the value of
    //    bound equals 32. In layer II, in all modes except
    //    joint-stereo, the value of bound equals sblimit. In
    //    joint-stereo mode the bound is determined by the
    //    mode_extension."
    let bound = match header.mode {
        Mode::JointStereo => header.mode_extension.bound().min(table.sblimit()),
        _ => table.sblimit(),
    };

    let mut data = AudioData::new(table, bound, channels);

    parse_allocation(&mut data, reader)?;
    parse_scfsi(&mut data, reader)?;
    parse_scalefactors(&mut data, reader)?;

    Ok(data)
}

fn read_bits(reader: &mut BitReader<'_>, n: u32) -> Result<u32, AudioDataError> {
    reader
        .read_u32(n)
        .map_err(|_| AudioDataError::UnexpectedEnd)
}

/// §2.4.1.6 bit-allocation loop:
///
/// ```text
/// for (sb = 0; sb < bound; sb++)
///     for (ch = 0; ch < nch; ch++)
///         allocation[ch][sb]   2..4 uimsbf
/// for (sb = bound; sb < sblimit; sb++) {
///     allocation[0][sb]        2..4 uimsbf
///     allocation[1][sb] = allocation[0][sb]
/// }
/// ```
fn parse_allocation(
    data: &mut AudioData,
    reader: &mut BitReader<'_>,
) -> Result<(), AudioDataError> {
    let table = data.table;
    let channels = data.channels;

    for sb in 0..data.bound {
        let nbal = table.nbal(sb);
        for ch in 0..channels {
            let idx = read_bits(reader, nbal)?;
            let nb = table
                .nb_steps(sb, idx)
                .ok_or(AudioDataError::InvalidAllocationIndex {
                    table,
                    sb,
                    index: idx,
                })?;
            data.nb_steps[ch][sb] = nb;
        }
    }
    for sb in data.bound..data.sblimit {
        let nbal = table.nbal(sb);
        let idx = read_bits(reader, nbal)?;
        let nb = table
            .nb_steps(sb, idx)
            .ok_or(AudioDataError::InvalidAllocationIndex {
                table,
                sb,
                index: idx,
            })?;
        // Intensity-stereo subbands: same allocation for both channels.
        for ch in 0..channels {
            data.nb_steps[ch][sb] = nb;
        }
    }
    Ok(())
}

/// §2.4.1.6 scfsi loop:
///
/// ```text
/// for (sb = 0; sb < sblimit; sb++)
///     for (ch = 0; ch < nch; ch++)
///         if (allocation[ch][sb] != 0)
///             scfsi[ch][sb]    2 bslbf
/// ```
fn parse_scfsi(data: &mut AudioData, reader: &mut BitReader<'_>) -> Result<(), AudioDataError> {
    for sb in 0..data.sblimit {
        for ch in 0..data.channels {
            if data.nb_steps[ch][sb] != 0 {
                let bits = read_bits(reader, 2)?;
                data.scfsi[ch][sb] = Scfsi::from_bits(bits);
            }
        }
    }
    Ok(())
}

/// §2.4.1.6 scalefactor loop:
///
/// ```text
/// for (sb = 0; sb < sblimit; sb++)
///     for (ch = 0; ch < nch; ch++)
///         if (allocation[ch][sb] != 0) {
///             if (scfsi[ch][sb] == 0) {
///                 scalefactor[ch][sb][0]  6 uimsbf
///                 scalefactor[ch][sb][1]  6 uimsbf
///                 scalefactor[ch][sb][2]  6 uimsbf
///             }
///             if (scfsi[ch][sb] == 1 || scfsi[ch][sb] == 3) {
///                 scalefactor[ch][sb][0]  6 uimsbf
///                 scalefactor[ch][sb][2]  6 uimsbf
///             }
///             if (scfsi[ch][sb] == 2)
///                 scalefactor[ch][sb][0]  6 uimsbf
///         }
/// ```
///
/// The on-wire indices are then expanded across granules per the
/// §2.4.2.3 schedule encoded in [`Scfsi`].
fn parse_scalefactors(
    data: &mut AudioData,
    reader: &mut BitReader<'_>,
) -> Result<(), AudioDataError> {
    for sb in 0..data.sblimit {
        for ch in 0..data.channels {
            if data.nb_steps[ch][sb] == 0 {
                continue;
            }
            let scfsi = data.scfsi[ch][sb];
            let sf = &mut data.scalefactor[ch][sb];
            match scfsi {
                Scfsi::ThreePerGranule => {
                    let s0 = read_scalefactor(reader, ch, sb, 0)?;
                    let s1 = read_scalefactor(reader, ch, sb, 1)?;
                    let s2 = read_scalefactor(reader, ch, sb, 2)?;
                    sf[0] = s0;
                    sf[1] = s1;
                    sf[2] = s2;
                }
                Scfsi::Share01Then2 => {
                    let s0 = read_scalefactor(reader, ch, sb, 0)?;
                    let s2 = read_scalefactor(reader, ch, sb, 2)?;
                    sf[0] = s0;
                    sf[1] = s0;
                    sf[2] = s2;
                }
                Scfsi::Share0Then12 => {
                    let s0 = read_scalefactor(reader, ch, sb, 0)?;
                    let s2 = read_scalefactor(reader, ch, sb, 2)?;
                    sf[0] = s0;
                    sf[1] = s2;
                    sf[2] = s2;
                }
                Scfsi::ShareAll => {
                    let s0 = read_scalefactor(reader, ch, sb, 0)?;
                    sf[0] = s0;
                    sf[1] = s0;
                    sf[2] = s0;
                }
            }
        }
    }
    Ok(())
}

fn read_scalefactor(
    reader: &mut BitReader<'_>,
    ch: usize,
    sb: usize,
    part: usize,
) -> Result<u8, AudioDataError> {
    let raw = read_bits(reader, 6)?;
    if raw >= crate::tables::SCALEFACTOR_COUNT as u32 {
        return Err(AudioDataError::ReservedScalefactorIndex {
            ch,
            sb,
            part,
            index: raw,
        });
    }
    Ok(raw as u8)
}

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

    /// MSB-first bit writer to assemble test bitstreams.
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
                // Take the high `take` bits of `value`.
                let chunk = (value >> (bits - take)) & ((1u32 << take) - 1);
                let last = self.bytes.last_mut().unwrap();
                *last |= (chunk as u8) << shift;
                self.bit_in_byte = (self.bit_in_byte + take) % 8;
                bits -= take;
                value &= (1u32 << bits) - 1;
            }
        }

        fn finish(self) -> Vec<u8> {
            self.bytes
        }
    }

    #[test]
    fn scfsi_decodes_all_four_codes() {
        assert_eq!(Scfsi::from_bits(0b00), Scfsi::ThreePerGranule);
        assert_eq!(Scfsi::from_bits(0b01), Scfsi::Share01Then2);
        assert_eq!(Scfsi::from_bits(0b10), Scfsi::ShareAll);
        assert_eq!(Scfsi::from_bits(0b11), Scfsi::Share0Then12);

        assert_eq!(Scfsi::ThreePerGranule.parts_on_wire(), 3);
        assert_eq!(Scfsi::Share01Then2.parts_on_wire(), 2);
        assert_eq!(Scfsi::Share0Then12.parts_on_wire(), 2);
        assert_eq!(Scfsi::ShareAll.parts_on_wire(), 1);
    }

    /// Synthesises a §2.4.1.6 audio-data segment where every subband
    /// in the active sub-table is allocated to a small non-zero index
    /// (`1`) for both channels, the scfsi is `'10'` (share-all), and
    /// the single scalefactor is `0`. This drives the full
    /// bit-allocation + scfsi + scalefactor loops in a deterministic
    /// way for round-trip testing.
    fn build_uniform_allocation_one_payload(table: BitAllocTable, channels: usize) -> Vec<u8> {
        let mut bw = BitWriter::new();
        // Bound = sblimit (we drive a non-joint-stereo header in this
        // helper's callers).
        for sb in 0..table.sblimit() {
            let nbal = table.nbal(sb);
            for _ch in 0..channels {
                bw.write(1, nbal); // allocation index 1
            }
        }
        // scfsi: 2 bits per (ch, sb) where allocation != 0 (always
        // true in this synthesised case).
        for _sb in 0..table.sblimit() {
            for _ch in 0..channels {
                bw.write(0b10, 2); // ShareAll
            }
        }
        // scalefactor: one 6-bit index per (ch, sb) for ShareAll.
        for _sb in 0..table.sblimit() {
            for _ch in 0..channels {
                bw.write(0, 6); // index 0 → scalefactor 2.0
            }
        }
        bw.finish()
    }

    #[test]
    fn parses_uniform_192kbps_44100_stereo_audio_data() {
        let header = make_header(192_000, 44_100, Mode::Stereo);
        // 192 / 2 = 96 per-channel kbit/s → B.2b at 44.1 kHz.
        let table = BitAllocTable::B2b;
        let bytes = build_uniform_allocation_one_payload(table, 2);
        let mut reader = BitReader::new(&bytes);
        let data = parse_audio_data(&header, &mut reader).expect("parse");

        assert_eq!(data.table, table);
        assert_eq!(data.sblimit, table.sblimit());
        assert_eq!(data.bound, table.sblimit());
        assert_eq!(data.channels, 2);

        for ch in 0..2 {
            for sb in 0..data.sblimit {
                let expected_nb = table.nb_steps(sb, 1).unwrap();
                assert_eq!(data.nb_steps[ch][sb], expected_nb, "ch={ch} sb={sb}");
                assert_eq!(data.scfsi[ch][sb], Scfsi::ShareAll);
                assert_eq!(data.scalefactor[ch][sb], [0, 0, 0]);
            }
            // Subbands past sblimit untouched.
            for sb in data.sblimit..NUM_SUBBANDS {
                assert_eq!(data.nb_steps[ch][sb], 0);
            }
        }
    }

    #[test]
    fn parses_uniform_64kbps_32khz_single_channel() {
        // 64 kbit/s single_channel at 32 kHz → per-channel = 64 → B.2a.
        let header = make_header(64_000, 32_000, Mode::SingleChannel);
        let table = BitAllocTable::B2a;
        let bytes = build_uniform_allocation_one_payload(table, 1);
        let mut reader = BitReader::new(&bytes);
        let data = parse_audio_data(&header, &mut reader).unwrap();
        assert_eq!(data.table, table);
        assert_eq!(data.channels, 1);
        // sb=0 has allocation index 1 in B.2a row 0..=2 = nb_steps 3.
        assert_eq!(data.nb_steps[0][0], 3);
        // sb=11 has allocation index 1 in nbal=3 row = nb_steps 3.
        assert_eq!(data.nb_steps[0][11], 3);
        // sb=23 has allocation index 1 in nbal=2 row = nb_steps 3.
        assert_eq!(data.nb_steps[0][23], 3);
    }

    #[test]
    fn joint_stereo_shares_allocation_above_bound() {
        // 192 kbit/s joint-stereo at 44.1 kHz → per-channel 96 → B.2b.
        let mut header = make_header(192_000, 44_100, Mode::JointStereo);
        header.mode_extension = ModeExtension::Bound8;
        let table = BitAllocTable::B2b;

        // Build a payload where every subband below bound gets a
        // distinct per-channel allocation, and every subband at/above
        // bound has a shared allocation. We use index 1 for
        // ch=0 and index 2 for ch=1 below bound; index 1 (shared)
        // above bound. scfsi is 0b10 (ShareAll), scalefactor index 0.
        let bound = ModeExtension::Bound8.bound();
        let mut bw = BitWriter::new();
        for sb in 0..bound {
            let nbal = table.nbal(sb);
            bw.write(1, nbal); // ch=0
            bw.write(2, nbal); // ch=1
        }
        for sb in bound..table.sblimit() {
            let nbal = table.nbal(sb);
            bw.write(1, nbal); // shared
        }
        // scfsi: every (sb, ch) where allocation != 0.
        for _sb in 0..table.sblimit() {
            bw.write(0b10, 2);
            bw.write(0b10, 2);
        }
        // scalefactor: 6 bits per (sb, ch) since ShareAll = 1 part.
        for _sb in 0..table.sblimit() {
            bw.write(0, 6);
            bw.write(0, 6);
        }
        let bytes = bw.finish();
        let mut reader = BitReader::new(&bytes);
        let data = parse_audio_data(&header, &mut reader).unwrap();

        assert_eq!(data.bound, bound);
        for sb in 0..bound {
            let nb0 = table.nb_steps(sb, 1).unwrap();
            let nb1 = table.nb_steps(sb, 2).unwrap();
            assert_eq!(data.nb_steps[0][sb], nb0, "below-bound ch=0 sb={sb}");
            assert_eq!(data.nb_steps[1][sb], nb1, "below-bound ch=1 sb={sb}");
        }
        for sb in bound..data.sblimit {
            let shared = table.nb_steps(sb, 1).unwrap();
            assert_eq!(data.nb_steps[0][sb], shared, "above-bound ch=0 sb={sb}");
            assert_eq!(data.nb_steps[1][sb], shared, "above-bound ch=1 sb={sb}");
        }
    }

    #[test]
    fn zero_allocation_skips_scfsi_and_scalefactor_reads() {
        // 192 kbit/s stereo at 44.1 → B.2b.
        let header = make_header(192_000, 44_100, Mode::Stereo);
        let table = BitAllocTable::B2b;
        // All zeros — every allocation is 0; no scfsi or scalefactor
        // bits should be read.
        let bits_for_alloc: u32 = (0..table.sblimit()).map(|sb| table.nbal(sb) * 2).sum();
        let byte_count = bits_for_alloc.div_ceil(8) as usize;
        let bytes = vec![0u8; byte_count];
        let mut reader = BitReader::new(&bytes);
        let data = parse_audio_data(&header, &mut reader).unwrap();
        for ch in 0..2 {
            for sb in 0..NUM_SUBBANDS {
                assert_eq!(data.nb_steps[ch][sb], 0);
                assert_eq!(data.scalefactor[ch][sb], [0, 0, 0]);
            }
        }
    }

    #[test]
    fn scfsi_schedules_fill_scalefactor_triplet_correctly() {
        // Mono frame, one subband allocated. Drive each scfsi value
        // and confirm the expansion of the on-wire 1/2/3 scalefactor
        // indices across the three granules.
        // We use Fs=32 kHz single_channel at 32 kbit/s → B.2d.
        let header = make_header(32_000, 32_000, Mode::SingleChannel);
        let table = BitAllocTable::B2d;
        let nbal0 = table.nbal(0); // 4

        // Helper: build a payload allocating sb=0 to index 1 and all
        // other subbands to index 0, then encoding `scfsi` with the
        // given on-wire indices.
        let build = |scfsi: u32, indices: &[u32]| -> Vec<u8> {
            let mut bw = BitWriter::new();
            // Allocation loop: sb=0 → 1; others → 0.
            bw.write(1, nbal0);
            for sb in 1..table.sblimit() {
                bw.write(0, table.nbal(sb));
            }
            // scfsi: only sb=0 has allocation != 0.
            bw.write(scfsi, 2);
            // scalefactor: parts dictated by scfsi.
            for &idx in indices {
                bw.write(idx, 6);
            }
            bw.finish()
        };

        // scfsi = '00' → three on-wire (a, b, c) → triplet (a, b, c).
        let bytes = build(0b00, &[10, 20, 30]);
        let data = parse_audio_data(&header, &mut BitReader::new(&bytes)).unwrap();
        assert_eq!(data.scfsi[0][0], Scfsi::ThreePerGranule);
        assert_eq!(data.scalefactor[0][0], [10, 20, 30]);

        // scfsi = '01' → two on-wire (a, c) → triplet (a, a, c).
        let bytes = build(0b01, &[5, 17]);
        let data = parse_audio_data(&header, &mut BitReader::new(&bytes)).unwrap();
        assert_eq!(data.scfsi[0][0], Scfsi::Share01Then2);
        assert_eq!(data.scalefactor[0][0], [5, 5, 17]);

        // scfsi = '10' → one on-wire (a) → triplet (a, a, a).
        let bytes = build(0b10, &[42]);
        let data = parse_audio_data(&header, &mut BitReader::new(&bytes)).unwrap();
        assert_eq!(data.scfsi[0][0], Scfsi::ShareAll);
        assert_eq!(data.scalefactor[0][0], [42, 42, 42]);

        // scfsi = '11' → two on-wire (a, c) → triplet (a, c, c).
        let bytes = build(0b11, &[7, 50]);
        let data = parse_audio_data(&header, &mut BitReader::new(&bytes)).unwrap();
        assert_eq!(data.scfsi[0][0], Scfsi::Share0Then12);
        assert_eq!(data.scalefactor[0][0], [7, 50, 50]);
    }

    #[test]
    fn reserved_scalefactor_index_63_is_rejected() {
        let header = make_header(32_000, 32_000, Mode::SingleChannel);
        let table = BitAllocTable::B2d;
        let nbal0 = table.nbal(0);
        let mut bw = BitWriter::new();
        bw.write(1, nbal0);
        for sb in 1..table.sblimit() {
            bw.write(0, table.nbal(sb));
        }
        bw.write(0b10, 2); // ShareAll
        bw.write(63, 6); // reserved scalefactor index
        let bytes = bw.finish();
        let mut reader = BitReader::new(&bytes);
        match parse_audio_data(&header, &mut reader) {
            Err(AudioDataError::ReservedScalefactorIndex {
                ch: 0,
                sb: 0,
                part: 0,
                index: 63,
            }) => {}
            other => panic!("expected ReservedScalefactorIndex, got {other:?}"),
        }
    }

    #[test]
    fn unexpected_end_when_payload_is_short() {
        let header = make_header(192_000, 44_100, Mode::Stereo);
        // No bytes at all — should fail immediately when the very
        // first allocation field is read.
        let bytes: [u8; 0] = [];
        let mut reader = BitReader::new(&bytes);
        assert_eq!(
            parse_audio_data(&header, &mut reader),
            Err(AudioDataError::UnexpectedEnd)
        );
    }

    #[test]
    fn allocation_bit_budget_matches_table_sum() {
        // Cross-check: the total bits consumed by the allocation loop
        // equals 2 * sum_of_nbal for stereo (bound == sblimit modes).
        let header = make_header(192_000, 44_100, Mode::Stereo);
        let table = BitAllocTable::B2b;
        let alloc_bits: u32 = (0..table.sblimit()).map(|sb| table.nbal(sb)).sum::<u32>() * 2;
        // Allocation 0 for everything → 0 scfsi / scalefactor bits.
        let bytes = vec![0u8; alloc_bits.div_ceil(8) as usize];
        let mut reader = BitReader::new(&bytes);
        let pos_before = reader.bit_position();
        let _data = parse_audio_data(&header, &mut reader).unwrap();
        let consumed = reader.bit_position() - pos_before;
        assert_eq!(consumed as u32, alloc_bits);
    }
}
