//! MPEG-1 / MPEG-2 LSF Audio Layer II frame header parsing — ISO/IEC
//! 11172-3 (1993), §2.4.1.3 (header syntax) and §2.4.2.3 (header field
//! semantics), extended per ISO/IEC 13818-3 (1997), §2.4.2.3 (low-rate
//! `ID == 0` bitrate ladder + 16/22.05/24 kHz sampling-frequency table).
//!
//! Clean-room: every numeric value in this module is derived from the
//! staged `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (157-page edition,
//! SHA-256 `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`)
//! and the staged `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`
//! (PDF pages 20-21 for the §2.4.2.3 LSF bitrate / sampling-frequency
//! tables). No third-party MP2 implementation source was consulted.
//!
//! ## Field layout (§2.4.1.3)
//!
//! The Layer II frame header is 32 bits, transmitted bslbf (MSB first):
//!
//! | bits | field                | width |
//! |-----:|----------------------|------:|
//! | 31..20 | `syncword`           | 12 |
//! | 19   | `ID`                   | 1 |
//! | 18..17 | `layer`              | 2 |
//! | 16   | `protection_bit`       | 1 |
//! | 15..12 | `bitrate_index`      | 4 |
//! | 11..10 | `sampling_frequency` | 2 |
//! | 9    | `padding_bit`          | 1 |
//! | 8    | `private_bit`          | 1 |
//! | 7..6 | `mode`                 | 2 |
//! | 5..4 | `mode_extension`       | 2 |
//! | 3    | `copyright`            | 1 |
//! | 2    | `original/copy`        | 1 |
//! | 1..0 | `emphasis`             | 2 |
//!
//! ## Tables (§2.4.2.3, ISO 11172-3 PDF page 21)
//!
//! For Layer II at `ID == 1` (MPEG-1) the `bitrate_index` ladder is
//! (kbit/s):
//!
//! | code | rate | code | rate |
//! |-----:|-----:|-----:|-----:|
//! | 0000 | free | 1000 | 128 |
//! | 0001 | 32   | 1001 | 160 |
//! | 0010 | 48   | 1010 | 192 |
//! | 0011 | 56   | 1011 | 224 |
//! | 0100 | 64   | 1100 | 256 |
//! | 0101 | 80   | 1101 | 320 |
//! | 0110 | 96   | 1110 | 384 |
//! | 0111 | 112  | 1111 | forbidden |
//!
//! The MPEG-1 `sampling_frequency` table (PDF page 21) is:
//!
//! | code | rate (kHz) |
//! |-----:|-----------:|
//! | 00 | 44,1 |
//! | 01 | 48 |
//! | 10 | 32 |
//! | 11 | reserved |
//!
//! ## LSF tables (§2.4.2.3, ISO 13818-3 PDF page 21)
//!
//! For Layer II at `ID == 0` (low-sampling-rate extension) the
//! `bitrate_index` ladder is (kbit/s):
//!
//! | code | rate | code | rate |
//! |-----:|-----:|-----:|-----:|
//! | 0000 | free | 1000 | 64  |
//! | 0001 | 8    | 1001 | 80  |
//! | 0010 | 16   | 1010 | 96  |
//! | 0011 | 24   | 1011 | 112 |
//! | 0100 | 32   | 1100 | 128 |
//! | 0101 | 40   | 1101 | 144 |
//! | 0110 | 48   | 1110 | 160 |
//! | 0111 | 56   | 1111 | forbidden |
//!
//! The LSF `sampling_frequency` table (PDF page 21) is:
//!
//! | code | rate (kHz) |
//! |-----:|-----------:|
//! | 00 | 22,05 |
//! | 01 | 24    |
//! | 10 | 16    |
//! | 11 | reserved |
//!
//! Frame size in slots (§2.4.3.1):
//!
//! ```text
//! N = floor(144 * bitrate / sampling_frequency) + padding_bit
//! ```
//!
//! and Layer II uses **1-byte** slots (§2.4.2.1), so the byte length of
//! the Layer II frame equals `N` bytes.

use core::fmt;

/// 12-bit syncword (§2.4.2.3: "the bit string '1111 1111 1111'").
pub const SYNCWORD: u16 = 0xFFF;

/// MPEG-1 Layer II frame header parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderError {
    /// Buffer too short to contain a 4-byte header.
    BufferTooShort,
    /// Top 12 bits were not the §2.4.2.3 syncword (`0xFFF`).
    BadSync,
    /// `layer` field decoded to a value other than `'10'` (Layer II).
    /// The crate is dedicated to Layer II; Layers I and III are
    /// dispatched by their own crates.
    UnsupportedLayer(u8),
    /// `bitrate_index == '1111'` — the §2.4.2.3 forbidden value.
    ForbiddenBitrate,
    /// `bitrate_index == '0000'` — free-format streams require external
    /// signalling to determine the frame size.
    FreeFormat,
    /// `sampling_frequency == '11'` — the §2.4.2.3 reserved value.
    ReservedSamplingFrequency,
    /// `emphasis == '10'` — the §2.4.2.3 reserved value.
    ReservedEmphasis,
    /// The (bitrate, mode) pair is not in the §2.4.2.3 "For Layer II,
    /// not all combinations of total bitrate and mode are allowed"
    /// matrix.
    DisallowedBitrateModeCombination { bit_rate: u32, mode: Mode },
    /// Encoder-side: `bit_rate` is not one of the 14 §2.4.2.3 Layer II
    /// ladder values (32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224,
    /// 256, 320, 384 kbit/s). Free-format encoding is out of scope.
    UnsupportedBitrate(u32),
    /// Encoder-side: `sample_rate` is not one of the three §2.4.2.3
    /// sampling-frequency table values (32_000, 44_100, 48_000 Hz).
    UnsupportedSamplingFrequency(u32),
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeaderError::BufferTooShort => {
                write!(f, "buffer is shorter than the 4-byte Layer II header")
            }
            HeaderError::BadSync => write!(f, "syncword is not 0xFFF"),
            HeaderError::UnsupportedLayer(l) => write!(
                f,
                "layer field decoded to {} (only Layer II / code '10' supported)",
                l
            ),
            HeaderError::ForbiddenBitrate => {
                write!(f, "bitrate_index '1111' is forbidden by §2.4.2.3")
            }
            HeaderError::FreeFormat => {
                write!(f, "bitrate_index '0000' is free format; frame size unknown")
            }
            HeaderError::ReservedSamplingFrequency => {
                write!(f, "sampling_frequency '11' is reserved by §2.4.2.3")
            }
            HeaderError::ReservedEmphasis => {
                write!(f, "emphasis '10' is reserved by §2.4.2.3")
            }
            HeaderError::DisallowedBitrateModeCombination { bit_rate, mode } => write!(
                f,
                "bitrate {} kbit/s is not permitted with mode {:?} per §2.4.2.3",
                bit_rate / 1000,
                mode
            ),
            HeaderError::UnsupportedBitrate(rate) => write!(
                f,
                "bit_rate {} bit/s is not in the §2.4.2.3 Layer II ladder",
                rate
            ),
            HeaderError::UnsupportedSamplingFrequency(rate) => write!(
                f,
                "sample_rate {} Hz is not in the §2.4.2.3 sampling-frequency table",
                rate
            ),
        }
    }
}

impl std::error::Error for HeaderError {}

/// Stereo / mono mode field (§2.4.2.3, table on PDF page 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `'00'` — full stereo (independent left/right subbands).
    Stereo,
    /// `'01'` — joint stereo (intensity stereo for Layer II).
    JointStereo,
    /// `'10'` — two independent mono channels.
    DualChannel,
    /// `'11'` — single mono channel.
    SingleChannel,
}

impl Mode {
    /// Number of audio channels in the decoded output (§2.2.6 `nch`).
    pub fn channels(self) -> usize {
        match self {
            Mode::SingleChannel => 1,
            _ => 2,
        }
    }
}

/// Joint-stereo mode extension (§2.4.2.3, PDF page 22).
///
/// In Layer II these two bits set the subband `bound` above which the
/// upper subbands are intensity-stereo coded. The numeric mapping is
/// `bound = (mode_extension + 1) * 4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeExtension {
    /// `'00'` — bound = 4.
    Bound4,
    /// `'01'` — bound = 8.
    Bound8,
    /// `'10'` — bound = 12.
    Bound12,
    /// `'11'` — bound = 16.
    Bound16,
}

impl ModeExtension {
    /// The §2.4.2.3 intensity-stereo bound (a subband index in 0..32).
    /// Bands `[0, bound)` are stereo-coded; bands `[bound, sblimit)`
    /// share one allocation across both channels.
    pub fn bound(self) -> usize {
        match self {
            ModeExtension::Bound4 => 4,
            ModeExtension::Bound8 => 8,
            ModeExtension::Bound12 => 12,
            ModeExtension::Bound16 => 16,
        }
    }
}

/// De-emphasis type (§2.4.2.3, table on PDF page 23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// `'00'` — none.
    None,
    /// `'01'` — 50/15 µs pre-emphasis was applied at encode time.
    FiftyFifteen,
    /// `'11'` — CCITT J.17 pre-emphasis was applied.
    CcittJ17,
}

/// Parsed MPEG-1 / MPEG-2 LSF Layer II frame header
/// (§2.4.1.3 + §2.4.2.3 of ISO/IEC 11172-3, extended by §2.4.2.3 of
/// ISO/IEC 13818-3 for the low sampling-rate (`ID == 0`) variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// True when the on-wire `ID` bit decoded to `'0'` — the ISO/IEC
    /// 13818-3 §2.4.2.3 low-sampling-rate (LSF) extension. False for
    /// the ISO/IEC 11172-3 baseline (MPEG-1, `ID == '1'`).
    ///
    /// `lsf` selects the §2.4.2.3 bitrate ladder, the
    /// `sampling_frequency` table, and (downstream of
    /// [`FrameHeader::parse`]) the active Annex B bit-allocation table
    /// per ISO/IEC 13818-3 §2.4.3.1 ("For Layer II, instead of tables
    /// B.2 ..., table B.1 ... of this part of ISO/IEC 13818 should be
    /// used.").
    pub lsf: bool,
    /// Bitrate in bit/s (i.e. `kbit/s * 1000`), per the §2.4.2.3
    /// `bitrate_index` ladder column "Layer II" (ISO/IEC 11172-3 page
    /// 21 for `lsf == false`, ISO/IEC 13818-3 page 21 for
    /// `lsf == true`).
    pub bit_rate: u32,
    /// Sampling frequency in Hz. When `lsf` is false the value is one
    /// of 32000 / 44100 / 48000; when `lsf` is true it is one of
    /// 16000 / 22050 / 24000.
    pub sample_rate: u32,
    /// `padding_bit` — adds one extra slot (= one extra byte for
    /// Layer II) to the frame when set.
    pub padding: bool,
    /// `private_bit` — application-defined, not used by ISO/IEC.
    pub private_bit: bool,
    /// Channel mode.
    pub mode: Mode,
    /// Mode extension. The bits are present in the header for **all**
    /// modes but per §2.4.2.3 only meaningful when `mode` is
    /// `JointStereo`; for the other modes the field is parsed as-is
    /// and exposed verbatim.
    pub mode_extension: ModeExtension,
    /// `copyright` — true if copyright-protected.
    pub copyright: bool,
    /// `original/copy` — true if this stream is an original (not a copy).
    pub original: bool,
    /// De-emphasis type.
    pub emphasis: Emphasis,
    /// `protection_bit` — true (§2.4.2.3: `'1'`) means no CRC. False
    /// means a 16-bit CRC follows the header per §2.4.1.4.
    pub protection_bit: bool,
}

impl FrameHeader {
    /// Parse a 4-byte MPEG-1 or MPEG-2 LSF Layer II header from the
    /// front of `buf`.
    ///
    /// Performs the §2.4.2.3 validation up-front: rejects bad syncwords,
    /// non-Layer-II frames, forbidden / reserved table codes, and the
    /// (MPEG-1-only) §2.4.2.3 disallowed `(bitrate, mode)` combinations.
    /// `ID == 0` is decoded as ISO/IEC 13818-3 §2.4.2.3 LSF; the
    /// `lsf` flag on the returned [`FrameHeader`] selects the LSF
    /// bitrate ladder and sampling-frequency table.
    pub fn parse(buf: &[u8]) -> Result<Self, HeaderError> {
        if buf.len() < 4 {
            return Err(HeaderError::BufferTooShort);
        }
        let word = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

        let sync = ((word >> 20) & 0xFFF) as u16;
        if sync != SYNCWORD {
            return Err(HeaderError::BadSync);
        }

        let id = (word >> 19) & 0x1;
        let lsf = id == 0;

        let layer_bits = ((word >> 17) & 0x3) as u8;
        // §2.4.2.3: '11' = Layer I, '10' = Layer II, '01' = Layer III,
        // '00' = reserved.
        if layer_bits != 0b10 {
            return Err(HeaderError::UnsupportedLayer(layer_bits));
        }

        let protection_bit = ((word >> 16) & 0x1) == 1;

        let bitrate_index = ((word >> 12) & 0xF) as u8;
        let bit_rate = if lsf {
            decode_bitrate_lsf(bitrate_index)?
        } else {
            decode_bitrate(bitrate_index)?
        };

        let sf_index = ((word >> 10) & 0x3) as u8;
        let sample_rate = if lsf {
            decode_sampling_frequency_lsf(sf_index)?
        } else {
            decode_sampling_frequency(sf_index)?
        };

        let padding = ((word >> 9) & 0x1) == 1;
        let private_bit = ((word >> 8) & 0x1) == 1;

        let mode_bits = ((word >> 6) & 0x3) as u8;
        let mode = match mode_bits {
            0b00 => Mode::Stereo,
            0b01 => Mode::JointStereo,
            0b10 => Mode::DualChannel,
            _ => Mode::SingleChannel,
        };

        let mode_extension_bits = ((word >> 4) & 0x3) as u8;
        let mode_extension = match mode_extension_bits {
            0b00 => ModeExtension::Bound4,
            0b01 => ModeExtension::Bound8,
            0b10 => ModeExtension::Bound12,
            _ => ModeExtension::Bound16,
        };

        let copyright = ((word >> 3) & 0x1) == 1;
        let original = ((word >> 2) & 0x1) == 1;

        let emphasis_bits = (word & 0x3) as u8;
        let emphasis = match emphasis_bits {
            0b00 => Emphasis::None,
            0b01 => Emphasis::FiftyFifteen,
            0b10 => return Err(HeaderError::ReservedEmphasis),
            _ => Emphasis::CcittJ17,
        };

        // §2.4.2.3 "For Layer II, not all combinations of total bitrate
        // and mode are allowed" — table on ISO 11172-3 PDF page 21.
        // The ISO/IEC 13818-3 §2.4.2.3 LSF extension does not restate
        // this matrix (the LSF bitrate ladder spans 8..160 kbit/s in a
        // single column for Layer II / Layer III); the LSF rates are
        // accepted for every mode.
        if !lsf && !is_layer2_bitrate_mode_allowed(bit_rate, mode) {
            return Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode });
        }

        Ok(FrameHeader {
            lsf,
            bit_rate,
            sample_rate,
            padding,
            private_bit,
            mode,
            mode_extension,
            copyright,
            original,
            emphasis,
            protection_bit,
        })
    }

    /// Length of this frame in bytes (§2.4.3.1 with §2.4.2.1 "one byte
    /// per Layer II slot"):
    ///
    /// ```text
    /// frame_bytes = floor(144 * bit_rate / sample_rate) + padding_bit
    /// ```
    pub fn frame_size_bytes(&self) -> usize {
        let n = (144u64 * self.bit_rate as u64) / self.sample_rate as u64;
        n as usize + if self.padding { 1 } else { 0 }
    }

    /// Number of decoded PCM samples per channel per frame.
    /// Layer II emits 1152 samples per channel per frame (§2.4.2.1
    /// "1 152 for Layer II", §2.4.1.6 "for (gr=0; gr<12; gr++)" with
    /// 3 samples per granule per subband × 32 subbands).
    pub fn samples_per_channel(&self) -> usize {
        1152
    }

    /// Number of channels in the decoded output.
    pub fn channels(&self) -> usize {
        self.mode.channels()
    }

    /// Emit this header as the 4-byte big-endian §2.4.1.3 word.
    ///
    /// This is the symmetric inverse of [`FrameHeader::parse`]. The same
    /// §2.4.2.3 validation is re-applied on the encoder side so a
    /// malformed encoder construction cannot escape the type system:
    ///
    /// - `bit_rate` must be one of the 14 Layer II ladder values
    ///   (32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ///   384 kbit/s); otherwise [`HeaderError::UnsupportedBitrate`].
    /// - `sample_rate` must be one of 32_000 / 44_100 / 48_000 Hz;
    ///   otherwise [`HeaderError::UnsupportedSamplingFrequency`].
    /// - The (`bit_rate`, `mode`) pair must satisfy the §2.4.2.3
    ///   "For Layer II, not all combinations of total bitrate and mode
    ///   are allowed" matrix; otherwise
    ///   [`HeaderError::DisallowedBitrateModeCombination`].
    ///
    /// The §2.4.2.3 reserved `emphasis = '10'` and reserved
    /// `sampling_frequency = '11'` cannot be produced by this method
    /// because they have no [`Emphasis`] / valid `sample_rate`
    /// counterpart — the type system already excludes them.
    ///
    /// The output is bit-exact identical to what `parse` would have
    /// read: bits 31..20 = `0xFFF` syncword, bit 19 = `'1'` (MPEG-1
    /// ID), bits 18..17 = `'10'` (Layer II), bit 16 = `protection_bit`
    /// (the spec convention: `'1'` = no CRC; `'0'` = CRC follows the
    /// header per §2.4.1.4), then the rest of the §2.4.1.3 layout.
    pub fn emit_bytes(&self) -> Result<[u8; 4], HeaderError> {
        // Resolve the §2.4.2.3 ladder codes FIRST so an off-ladder
        // `bit_rate` (e.g. 200 kbit/s) is reported as
        // `UnsupportedBitrate` rather than getting swept into the
        // §2.4.2.3 "disallowed (bitrate, mode)" matrix (which only
        // covers the ladder rows themselves). The LSF flag selects
        // between the ISO 11172-3 and ISO 13818-3 ladders.
        let bitrate_index = if self.lsf {
            encode_bitrate_lsf(self.bit_rate)? as u32
        } else {
            encode_bitrate(self.bit_rate)? as u32
        };
        let sf_index = if self.lsf {
            encode_sampling_frequency_lsf(self.sample_rate)? as u32
        } else {
            encode_sampling_frequency(self.sample_rate)? as u32
        };

        // Now the (bit_rate, mode) pair sits inside the §2.4.2.3
        // matrix on PDF page 21; enforce the "not all combinations are
        // allowed" rule. The 13818-3 LSF extension does not restate
        // this matrix and accepts every (LSF bitrate, mode) pair.
        if !self.lsf && !is_layer2_bitrate_mode_allowed(self.bit_rate, self.mode) {
            return Err(HeaderError::DisallowedBitrateModeCombination {
                bit_rate: self.bit_rate,
                mode: self.mode,
            });
        }

        let mode_bits: u32 = match self.mode {
            Mode::Stereo => 0b00,
            Mode::JointStereo => 0b01,
            Mode::DualChannel => 0b10,
            Mode::SingleChannel => 0b11,
        };
        let mode_ext_bits: u32 = match self.mode_extension {
            ModeExtension::Bound4 => 0b00,
            ModeExtension::Bound8 => 0b01,
            ModeExtension::Bound12 => 0b10,
            ModeExtension::Bound16 => 0b11,
        };
        let emphasis_bits: u32 = match self.emphasis {
            Emphasis::None => 0b00,
            Emphasis::FiftyFifteen => 0b01,
            Emphasis::CcittJ17 => 0b11,
        };

        // §2.4.1.3 packed layout, transmitted MSB-first:
        //   sync(12) | ID(1) | layer(2)='10' | protection(1) |
        //   bitrate(4) | sf(2) | pad(1) | priv(1) | mode(2) |
        //   mode_ext(2) | copyright(1) | original(1) | emph(2)
        // ID = '1' for MPEG-1 (§2.4.2.3 of 11172-3); ID = '0' for the
        // LSF extension (§2.4.2.3 of 13818-3).
        let id_bit: u32 = if self.lsf { 0 } else { 1 };
        let word: u32 = ((SYNCWORD as u32) << 20)
            | (id_bit << 19)
            | (0b10 << 17) // layer = '10' (Layer II, §2.4.2.3)
            | ((self.protection_bit as u32) << 16)
            | (bitrate_index << 12)
            | (sf_index << 10)
            | ((self.padding as u32) << 9)
            | ((self.private_bit as u32) << 8)
            | (mode_bits << 6)
            | (mode_ext_bits << 4)
            | ((self.copyright as u32) << 3)
            | ((self.original as u32) << 2)
            | emphasis_bits;
        Ok(word.to_be_bytes())
    }
}

/// Decode the §2.4.2.3 Layer II `bitrate_index` ladder.
///
/// Returns `bit/s` (i.e. `kbit/s × 1000`). `0` (free format) and `0xF`
/// (forbidden) are rejected.
pub fn decode_bitrate(index: u8) -> Result<u32, HeaderError> {
    // PDF page 21, "Layer II" column. Encoded in `bit/s`.
    let kbps = match index {
        0b0000 => return Err(HeaderError::FreeFormat),
        0b0001 => 32,
        0b0010 => 48,
        0b0011 => 56,
        0b0100 => 64,
        0b0101 => 80,
        0b0110 => 96,
        0b0111 => 112,
        0b1000 => 128,
        0b1001 => 160,
        0b1010 => 192,
        0b1011 => 224,
        0b1100 => 256,
        0b1101 => 320,
        0b1110 => 384,
        0b1111 => return Err(HeaderError::ForbiddenBitrate),
        _ => unreachable!("bitrate_index is a 4-bit field"),
    };
    Ok(kbps * 1000)
}

/// Decode the §2.4.2.3 `sampling_frequency` table (PDF page 21).
pub fn decode_sampling_frequency(index: u8) -> Result<u32, HeaderError> {
    match index {
        0b00 => Ok(44_100),
        0b01 => Ok(48_000),
        0b10 => Ok(32_000),
        0b11 => Err(HeaderError::ReservedSamplingFrequency),
        _ => unreachable!("sampling_frequency is a 2-bit field"),
    }
}

/// Encode a §2.4.2.3 Layer II bitrate to its 4-bit `bitrate_index`.
///
/// This is the inverse of [`decode_bitrate`]. `bit_rate` is the bitrate
/// in bit/s (i.e. `kbit/s × 1000`). Free format (`'0000'`) and the
/// forbidden code (`'1111'`) are intentionally not producible — both
/// are rejected as [`HeaderError::UnsupportedBitrate`] since this
/// crate's encoder only emits the §2.4.2.3 ladder.
pub fn encode_bitrate(bit_rate: u32) -> Result<u8, HeaderError> {
    if bit_rate == 0 || bit_rate % 1000 != 0 {
        return Err(HeaderError::UnsupportedBitrate(bit_rate));
    }
    let kbps = bit_rate / 1000;
    let index = match kbps {
        32 => 0b0001,
        48 => 0b0010,
        56 => 0b0011,
        64 => 0b0100,
        80 => 0b0101,
        96 => 0b0110,
        112 => 0b0111,
        128 => 0b1000,
        160 => 0b1001,
        192 => 0b1010,
        224 => 0b1011,
        256 => 0b1100,
        320 => 0b1101,
        384 => 0b1110,
        _ => return Err(HeaderError::UnsupportedBitrate(bit_rate)),
    };
    Ok(index)
}

/// Encode a §2.4.2.3 sampling frequency to its 2-bit
/// `sampling_frequency` code (PDF page 21).
///
/// This is the inverse of [`decode_sampling_frequency`]. The §2.4.2.3
/// reserved code (`'11'`) is not producible because there is no
/// matching `sample_rate` value; arbitrary other values are rejected
/// as [`HeaderError::UnsupportedSamplingFrequency`].
pub fn encode_sampling_frequency(sample_rate: u32) -> Result<u8, HeaderError> {
    match sample_rate {
        44_100 => Ok(0b00),
        48_000 => Ok(0b01),
        32_000 => Ok(0b10),
        _ => Err(HeaderError::UnsupportedSamplingFrequency(sample_rate)),
    }
}

/// §2.4.2.3 "For Layer II, not all combinations of total bitrate and
/// mode are allowed" — PDF page 21.
///
/// | bit_rate (kbit/s) | allowed modes |
/// |------------------:|---------------|
/// | 32, 48, 56, 80    | single_channel only |
/// | 64, 96, 112, 128, 160, 192 | all modes |
/// | 224, 256, 320, 384 | stereo, joint_stereo, dual_channel (no single_channel) |
pub fn is_layer2_bitrate_mode_allowed(bit_rate: u32, mode: Mode) -> bool {
    let kbps = bit_rate / 1000;
    match kbps {
        32 | 48 | 56 | 80 => matches!(mode, Mode::SingleChannel),
        64 | 96 | 112 | 128 | 160 | 192 => true,
        224 | 256 | 320 | 384 => !matches!(mode, Mode::SingleChannel),
        _ => false,
    }
}

/// Decode the ISO/IEC 13818-3 §2.4.2.3 LSF Layer II `bitrate_index`
/// ladder (PDF page 21, "Layer II, Layer III" column).
///
/// Returns `bit/s` (i.e. `kbit/s × 1000`). `0` (free format) and `0xF`
/// (forbidden) are rejected. The LSF Layer II ladder spans
/// 8..160 kbit/s in single-kbit/s steps from the §2.4.2.3 table.
pub fn decode_bitrate_lsf(index: u8) -> Result<u32, HeaderError> {
    let kbps = match index {
        0b0000 => return Err(HeaderError::FreeFormat),
        0b0001 => 8,
        0b0010 => 16,
        0b0011 => 24,
        0b0100 => 32,
        0b0101 => 40,
        0b0110 => 48,
        0b0111 => 56,
        0b1000 => 64,
        0b1001 => 80,
        0b1010 => 96,
        0b1011 => 112,
        0b1100 => 128,
        0b1101 => 144,
        0b1110 => 160,
        0b1111 => return Err(HeaderError::ForbiddenBitrate),
        _ => unreachable!("bitrate_index is a 4-bit field"),
    };
    Ok(kbps * 1000)
}

/// Decode the ISO/IEC 13818-3 §2.4.2.3 LSF `sampling_frequency` table
/// (PDF page 21): `'00'` → 22.05, `'01'` → 24, `'10'` → 16 kHz,
/// `'11'` → reserved.
pub fn decode_sampling_frequency_lsf(index: u8) -> Result<u32, HeaderError> {
    match index {
        0b00 => Ok(22_050),
        0b01 => Ok(24_000),
        0b10 => Ok(16_000),
        0b11 => Err(HeaderError::ReservedSamplingFrequency),
        _ => unreachable!("sampling_frequency is a 2-bit field"),
    }
}

/// Encode a §2.4.2.3 LSF Layer II bitrate to its 4-bit `bitrate_index`.
///
/// Inverse of [`decode_bitrate_lsf`]. Free format (`'0000'`) and the
/// forbidden code (`'1111'`) are not producible — both are rejected
/// as [`HeaderError::UnsupportedBitrate`].
pub fn encode_bitrate_lsf(bit_rate: u32) -> Result<u8, HeaderError> {
    if bit_rate == 0 || bit_rate % 1000 != 0 {
        return Err(HeaderError::UnsupportedBitrate(bit_rate));
    }
    let kbps = bit_rate / 1000;
    let index = match kbps {
        8 => 0b0001,
        16 => 0b0010,
        24 => 0b0011,
        32 => 0b0100,
        40 => 0b0101,
        48 => 0b0110,
        56 => 0b0111,
        64 => 0b1000,
        80 => 0b1001,
        96 => 0b1010,
        112 => 0b1011,
        128 => 0b1100,
        144 => 0b1101,
        160 => 0b1110,
        _ => return Err(HeaderError::UnsupportedBitrate(bit_rate)),
    };
    Ok(index)
}

/// Encode a §2.4.2.3 LSF sampling frequency to its 2-bit
/// `sampling_frequency` code (PDF page 21 of ISO/IEC 13818-3).
///
/// Inverse of [`decode_sampling_frequency_lsf`]. The reserved code
/// (`'11'`) is not producible.
pub fn encode_sampling_frequency_lsf(sample_rate: u32) -> Result<u8, HeaderError> {
    match sample_rate {
        22_050 => Ok(0b00),
        24_000 => Ok(0b01),
        16_000 => Ok(0b10),
        _ => Err(HeaderError::UnsupportedSamplingFrequency(sample_rate)),
    }
}

/// Search `buf` for the §2.4.3.1 12-bit syncword on byte boundaries.
/// Returns the byte offset of the first sync, or `None`.
pub fn find_sync(buf: &[u8]) -> Option<usize> {
    // 12 bits = top byte is 0xFF, top nibble of next byte is 0xF.
    buf.windows(2)
        .position(|w| w[0] == 0xFF && (w[1] & 0xF0) == 0xF0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a 4-byte Layer II header from explicit field values, for
    /// test construction. Field widths follow §2.4.1.3.
    #[allow(clippy::too_many_arguments)]
    fn build_header(
        bitrate_index: u32,
        sf_index: u32,
        padding: u32,
        private_bit: u32,
        mode_bits: u32,
        mode_ext_bits: u32,
        copyright: u32,
        original: u32,
        emphasis: u32,
        protection_bit: u32,
    ) -> [u8; 4] {
        // sync(12) | id(1)=1 | layer(2)='10' | protection(1) | bitrate(4) |
        // sf(2) | pad(1) | priv(1) | mode(2) | mode_ext(2) | cr(1) | orig(1) | emph(2)
        let word: u32 = (0xFFF << 20)
            | (1 << 19)
            | (0b10 << 17)
            | (protection_bit << 16)
            | (bitrate_index << 12)
            | (sf_index << 10)
            | (padding << 9)
            | (private_bit << 8)
            | (mode_bits << 6)
            | (mode_ext_bits << 4)
            | (copyright << 3)
            | (original << 2)
            | emphasis;
        word.to_be_bytes()
    }

    #[test]
    fn parses_a_canonical_192kbps_44100_stereo_no_crc_header() {
        // bitrate_index 0b1010 = 192 kbit/s; sf 0b00 = 44.1 kHz;
        // mode 0b00 = stereo; mode_ext 0b00; emph 0b00; protection 1.
        let bytes = build_header(0b1010, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        let h = FrameHeader::parse(&bytes).expect("parse");
        assert_eq!(h.bit_rate, 192_000);
        assert_eq!(h.sample_rate, 44_100);
        assert!(!h.padding);
        assert_eq!(h.mode, Mode::Stereo);
        assert_eq!(h.mode_extension, ModeExtension::Bound4);
        assert_eq!(h.emphasis, Emphasis::None);
        assert!(h.protection_bit);
        assert!(!h.copyright);
        assert!(h.original);
        assert_eq!(h.channels(), 2);
        assert_eq!(h.samples_per_channel(), 1152);
        // 144 * 192_000 / 44_100 = 626 (truncated).
        assert_eq!(h.frame_size_bytes(), 626);
    }

    #[test]
    fn rejects_short_buffer() {
        assert_eq!(
            FrameHeader::parse(&[0xFF, 0xFD, 0xAA]),
            Err(HeaderError::BufferTooShort)
        );
    }

    #[test]
    fn rejects_bad_sync() {
        // Top 12 bits = 0xFFE — off by one.
        let bytes = [0xFF, 0xED, 0x00, 0x00];
        assert_eq!(FrameHeader::parse(&bytes), Err(HeaderError::BadSync));
    }

    #[test]
    fn parses_an_lsf_header_with_id_bit_zero() {
        // ID = 0 (ISO/IEC 13818-3 LSF), layer = '10' (Layer II),
        // protection = '1' (no CRC).
        // Byte 1 low nibble = ID(1)=0 | layer(2)=10 | prot(1)=1 = 0b0101 = 0x5.
        // Then bitrate_index = '1000' = 64 kbit/s for LSF Layer II,
        // sampling_frequency = '10' = 16 kHz, padding=0, private=0,
        // mode='11'=single_channel, mode_ext='00', cr=0, orig=1, emph='00'.
        // Byte 2 = 1000 1000 = 0x88; byte 3 = 1100 0100 = 0xC4.
        let bytes = [0xFF, 0xF5, 0x88, 0xC4];
        let h = FrameHeader::parse(&bytes).expect("LSF header should parse");
        assert!(h.lsf);
        assert_eq!(h.bit_rate, 64_000);
        assert_eq!(h.sample_rate, 16_000);
        assert_eq!(h.mode, Mode::SingleChannel);
        assert!(!h.padding);
        assert!(h.protection_bit);
    }

    #[test]
    fn rejects_layer_i_layer_iii_and_reserved() {
        // Layer I = '11'
        // byte 1: sync top nibble F, low nibble = 1 ID | 11 layer | 1 prot = 0b1111 = 0xF
        let bytes = [0xFF, 0xFF, 0xA0, 0x04];
        match FrameHeader::parse(&bytes) {
            Err(HeaderError::UnsupportedLayer(0b11)) => (),
            other => panic!("expected UnsupportedLayer(3), got {other:?}"),
        }
        // Layer III = '01': low nibble = 0b1011 = 0xB
        let bytes = [0xFF, 0xFB, 0xA0, 0x04];
        match FrameHeader::parse(&bytes) {
            Err(HeaderError::UnsupportedLayer(0b01)) => (),
            other => panic!("expected UnsupportedLayer(1), got {other:?}"),
        }
        // Reserved layer code = '00': low nibble = 0b1001 = 0x9
        let bytes = [0xFF, 0xF9, 0xA0, 0x04];
        match FrameHeader::parse(&bytes) {
            Err(HeaderError::UnsupportedLayer(0b00)) => (),
            other => panic!("expected UnsupportedLayer(0), got {other:?}"),
        }
    }

    #[test]
    fn rejects_forbidden_and_free_bitrate() {
        // '1111' forbidden
        let bytes = build_header(0b1111, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        assert_eq!(
            FrameHeader::parse(&bytes),
            Err(HeaderError::ForbiddenBitrate)
        );
        // '0000' free format
        let bytes = build_header(0b0000, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        assert_eq!(FrameHeader::parse(&bytes), Err(HeaderError::FreeFormat));
    }

    #[test]
    fn rejects_reserved_sampling_frequency_and_emphasis() {
        let bytes = build_header(0b1010, 0b11, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        assert_eq!(
            FrameHeader::parse(&bytes),
            Err(HeaderError::ReservedSamplingFrequency)
        );
        let bytes = build_header(0b1010, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b10, 1);
        assert_eq!(
            FrameHeader::parse(&bytes),
            Err(HeaderError::ReservedEmphasis)
        );
    }

    #[test]
    fn enforces_layer2_bitrate_mode_matrix() {
        // 32 kbit/s with stereo mode is disallowed.
        let bytes = build_header(0b0001, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        match FrameHeader::parse(&bytes) {
            Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode }) => {
                assert_eq!(bit_rate, 32_000);
                assert_eq!(mode, Mode::Stereo);
            }
            other => panic!("expected DisallowedBitrateModeCombination, got {other:?}"),
        }
        // 32 kbit/s single_channel is allowed.
        let bytes = build_header(0b0001, 0b00, 0, 0, 0b11, 0b00, 0, 1, 0b00, 1);
        let h = FrameHeader::parse(&bytes).expect("32 kbit/s single_channel is allowed");
        assert_eq!(h.mode, Mode::SingleChannel);
        assert_eq!(h.channels(), 1);

        // 224 kbit/s single_channel is disallowed (only stereo/JS/DC).
        let bytes = build_header(0b1011, 0b00, 0, 0, 0b11, 0b00, 0, 1, 0b00, 1);
        match FrameHeader::parse(&bytes) {
            Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode }) => {
                assert_eq!(bit_rate, 224_000);
                assert_eq!(mode, Mode::SingleChannel);
            }
            other => panic!("expected DisallowedBitrateModeCombination, got {other:?}"),
        }
        // 224 kbit/s stereo is allowed.
        let bytes = build_header(0b1011, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        let h = FrameHeader::parse(&bytes).expect("224 kbit/s stereo is allowed");
        assert_eq!(h.bit_rate, 224_000);
        assert_eq!(h.mode, Mode::Stereo);
    }

    #[test]
    fn decodes_full_layer2_bitrate_ladder() {
        let expected = [
            (0b0001, 32_000),
            (0b0010, 48_000),
            (0b0011, 56_000),
            (0b0100, 64_000),
            (0b0101, 80_000),
            (0b0110, 96_000),
            (0b0111, 112_000),
            (0b1000, 128_000),
            (0b1001, 160_000),
            (0b1010, 192_000),
            (0b1011, 224_000),
            (0b1100, 256_000),
            (0b1101, 320_000),
            (0b1110, 384_000),
        ];
        for (code, rate) in expected {
            assert_eq!(decode_bitrate(code).unwrap(), rate);
        }
    }

    #[test]
    fn decodes_all_sampling_frequencies() {
        assert_eq!(decode_sampling_frequency(0b00).unwrap(), 44_100);
        assert_eq!(decode_sampling_frequency(0b01).unwrap(), 48_000);
        assert_eq!(decode_sampling_frequency(0b10).unwrap(), 32_000);
        assert!(decode_sampling_frequency(0b11).is_err());
    }

    #[test]
    fn frame_size_matches_spec_examples() {
        // 192 kbit/s @ 44.1 kHz: 144*192000/44100 = 626.93 → 626; +1 if padded.
        let h =
            FrameHeader::parse(&build_header(0b1010, 0b00, 0, 0, 0b00, 0, 0, 1, 0b00, 1)).unwrap();
        assert_eq!(h.frame_size_bytes(), 626);
        let h =
            FrameHeader::parse(&build_header(0b1010, 0b00, 1, 0, 0b00, 0, 0, 1, 0b00, 1)).unwrap();
        assert_eq!(h.frame_size_bytes(), 627);
        // 128 kbit/s @ 48 kHz: 144*128000/48000 = 384.0 → 384.
        let h =
            FrameHeader::parse(&build_header(0b1000, 0b01, 0, 0, 0b00, 0, 0, 1, 0b00, 1)).unwrap();
        assert_eq!(h.frame_size_bytes(), 384);
        // 64 kbit/s @ 32 kHz: 144*64000/32000 = 288.0 → 288.
        let h =
            FrameHeader::parse(&build_header(0b0100, 0b10, 0, 0, 0b00, 0, 0, 1, 0b00, 1)).unwrap();
        assert_eq!(h.frame_size_bytes(), 288);
    }

    #[test]
    fn mode_extension_bounds_match_spec() {
        assert_eq!(ModeExtension::Bound4.bound(), 4);
        assert_eq!(ModeExtension::Bound8.bound(), 8);
        assert_eq!(ModeExtension::Bound12.bound(), 12);
        assert_eq!(ModeExtension::Bound16.bound(), 16);
    }

    #[test]
    fn mode_channel_counts() {
        assert_eq!(Mode::Stereo.channels(), 2);
        assert_eq!(Mode::JointStereo.channels(), 2);
        assert_eq!(Mode::DualChannel.channels(), 2);
        assert_eq!(Mode::SingleChannel.channels(), 1);
    }

    #[test]
    fn decodes_all_emphasis_values_and_modes() {
        // emphasis 0/1/3 valid, 2 reserved.
        for (code, expected) in [
            (0b00u32, Emphasis::None),
            (0b01, Emphasis::FiftyFifteen),
            (0b11, Emphasis::CcittJ17),
        ] {
            let bytes = build_header(0b1010, 0b00, 0, 0, 0b00, 0, 0, 1, code, 1);
            let h = FrameHeader::parse(&bytes).unwrap();
            assert_eq!(h.emphasis, expected);
        }
        // modes 0/1/2/3 — all decode (and 64 kbit/s allows all).
        for (code, expected) in [
            (0b00u32, Mode::Stereo),
            (0b01, Mode::JointStereo),
            (0b10, Mode::DualChannel),
            (0b11, Mode::SingleChannel),
        ] {
            let bytes = build_header(0b0100, 0b00, 0, 0, code, 0, 0, 1, 0b00, 1);
            let h = FrameHeader::parse(&bytes).unwrap();
            assert_eq!(h.mode, expected);
        }
        // mode_extension 0/1/2/3 all decode and map to {4,8,12,16}.
        for (code, expected_bound) in [(0b00u32, 4), (0b01, 8), (0b10, 12), (0b11, 16)] {
            let bytes = build_header(0b0100, 0b00, 0, 0, 0b01, code, 0, 1, 0b00, 1);
            let h = FrameHeader::parse(&bytes).unwrap();
            assert_eq!(h.mode_extension.bound(), expected_bound);
        }
    }

    #[test]
    fn find_sync_locates_byte_aligned_syncword() {
        let mut buf = vec![0x00, 0x00, 0xAA, 0xBB];
        buf.extend_from_slice(&build_header(0b1010, 0b00, 0, 0, 0b00, 0, 0, 1, 0b00, 1));
        assert_eq!(find_sync(&buf), Some(4));
        // No sync in a buffer of zeros.
        assert_eq!(find_sync(&[0x00; 16]), None);
        // Empty / single-byte buffers return None rather than panicking.
        assert_eq!(find_sync(&[]), None);
        assert_eq!(find_sync(&[0xFF]), None);
    }

    #[test]
    fn protection_bit_zero_indicates_crc_follows() {
        // protection_bit = 0 → CRC present per §2.4.1.4.
        let bytes = build_header(0b1010, 0b00, 0, 0, 0b00, 0, 0, 1, 0b00, 0);
        let h = FrameHeader::parse(&bytes).unwrap();
        assert!(!h.protection_bit);
    }

    // ---------- §2.4.1.3 / §2.4.2.3 encoder-side (header writer) ----------

    #[test]
    fn encode_bitrate_inverts_decode_bitrate() {
        // The 14 valid Layer II ladder codes must round-trip exactly.
        for code in 1u8..=14 {
            let rate = decode_bitrate(code).unwrap();
            assert_eq!(
                encode_bitrate(rate).unwrap(),
                code,
                "round-trip code {code}"
            );
        }
        // The two §2.4.2.3 reject paths (free format / forbidden) have
        // no matching `bit_rate` value, so they cannot round-trip in
        // either direction; that is the point.
        assert!(matches!(
            encode_bitrate(0),
            Err(HeaderError::UnsupportedBitrate(0))
        ));
        assert!(matches!(
            encode_bitrate(200_000), // 200 kbit/s is not in the ladder
            Err(HeaderError::UnsupportedBitrate(200_000))
        ));
        // A kbit/s value that happens to land mid-ladder if rounded
        // (e.g. 199 999 bit/s) must still be rejected: the encoder
        // does not silently snap.
        assert!(matches!(
            encode_bitrate(199_999),
            Err(HeaderError::UnsupportedBitrate(199_999))
        ));
    }

    #[test]
    fn encode_sampling_frequency_inverts_decode() {
        // The three §2.4.2.3 sampling-frequency codes round-trip.
        for code in 0u8..=2 {
            let rate = decode_sampling_frequency(code).unwrap();
            assert_eq!(encode_sampling_frequency(rate).unwrap(), code);
        }
        // The reserved `'11'` code has no `sample_rate` counterpart.
        assert!(matches!(
            encode_sampling_frequency(11_025),
            Err(HeaderError::UnsupportedSamplingFrequency(11_025))
        ));
        // LSF sampling rates (16/22.05/24 kHz) are not in the table
        // either — encoder rejects rather than silently emitting an
        // ID = '0' header.
        for lsf in [16_000u32, 22_050, 24_000] {
            assert!(matches!(
                encode_sampling_frequency(lsf),
                Err(HeaderError::UnsupportedSamplingFrequency(_))
            ));
        }
    }

    #[test]
    fn emit_bytes_round_trips_a_canonical_header() {
        // The fixture we already exercise on the decoder side.
        let bytes = build_header(0b1010, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        let h = FrameHeader::parse(&bytes).unwrap();
        let out = h.emit_bytes().unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn emit_bytes_round_trips_every_bitrate_sample_rate_mode_combo() {
        // Walk the entire §2.4.2.3 matrix (mode × bitrate × sf), with
        // all flag bits ON, and confirm both `parse(emit(h)) == h` and
        // byte equality with `build_header`.
        let bitrate_codes: [(u8, u32); 14] = [
            (0b0001, 32_000),
            (0b0010, 48_000),
            (0b0011, 56_000),
            (0b0100, 64_000),
            (0b0101, 80_000),
            (0b0110, 96_000),
            (0b0111, 112_000),
            (0b1000, 128_000),
            (0b1001, 160_000),
            (0b1010, 192_000),
            (0b1011, 224_000),
            (0b1100, 256_000),
            (0b1101, 320_000),
            (0b1110, 384_000),
        ];
        let sf_codes: [(u8, u32); 3] = [(0b00, 44_100), (0b01, 48_000), (0b10, 32_000)];
        let mode_codes: [(u8, Mode); 4] = [
            (0b00, Mode::Stereo),
            (0b01, Mode::JointStereo),
            (0b10, Mode::DualChannel),
            (0b11, Mode::SingleChannel),
        ];

        let mut covered = 0usize;
        for &(br_code, bit_rate) in &bitrate_codes {
            for &(sf_code, sample_rate) in &sf_codes {
                for &(mode_code, mode) in &mode_codes {
                    if !is_layer2_bitrate_mode_allowed(bit_rate, mode) {
                        // Encoder must reject the same matrix the
                        // decoder rejects.
                        let h = FrameHeader {
                            lsf: false,
                            bit_rate,
                            sample_rate,
                            padding: false,
                            private_bit: false,
                            mode,
                            mode_extension: ModeExtension::Bound4,
                            copyright: false,
                            original: false,
                            emphasis: Emphasis::None,
                            protection_bit: true,
                        };
                        match h.emit_bytes() {
                            Err(HeaderError::DisallowedBitrateModeCombination {
                                bit_rate: br,
                                mode: m,
                            }) => {
                                assert_eq!(br, bit_rate);
                                assert_eq!(m, mode);
                            }
                            other => panic!(
                                "expected disallowed (bit_rate, mode) rejection, got {other:?}"
                            ),
                        }
                        continue;
                    }
                    // Allowed: round-trip through build_header().
                    let original = build_header(
                        br_code as u32,
                        sf_code as u32,
                        1, // padding
                        1, // private_bit
                        mode_code as u32,
                        0b01, // mode_extension = Bound8
                        1,    // copyright
                        0,    // original = false
                        0b01, // emphasis = FiftyFifteen
                        0,    // protection_bit = 0 (CRC follows)
                    );
                    let h = FrameHeader::parse(&original).unwrap();
                    let emitted = h.emit_bytes().unwrap();
                    assert_eq!(emitted, original, "round-trip bytes mismatch");
                    let h2 = FrameHeader::parse(&emitted).unwrap();
                    assert_eq!(h, h2, "round-trip struct mismatch");
                    covered += 1;
                }
            }
        }
        // 14 bitrates × 3 sample-rates × 4 modes = 168 cells. The
        // §2.4.2.3 matrix forbids 4 × 3 × 3 = 36 cells at
        // {32, 48, 56, 80} kbit/s × non-single_channel modes, and
        // another 4 × 3 × 1 = 12 cells at {224, 256, 320, 384} kbit/s
        // × single_channel — 48 rejected, 120 covered.
        assert_eq!(covered, 120);
    }

    #[test]
    fn emit_bytes_walks_all_mode_extensions_and_emphases() {
        // mode_extension is parsed verbatim regardless of mode (§2.4.2.3
        // makes it meaningful only for joint stereo, but the bits are
        // present in every header). The emitter must round-trip all four
        // codes when paired with Stereo.
        for ext in [
            ModeExtension::Bound4,
            ModeExtension::Bound8,
            ModeExtension::Bound12,
            ModeExtension::Bound16,
        ] {
            let h = FrameHeader {
                lsf: false,
                bit_rate: 128_000,
                sample_rate: 44_100,
                padding: false,
                private_bit: false,
                mode: Mode::JointStereo,
                mode_extension: ext,
                copyright: false,
                original: true,
                emphasis: Emphasis::None,
                protection_bit: true,
            };
            let h2 = FrameHeader::parse(&h.emit_bytes().unwrap()).unwrap();
            assert_eq!(h.mode_extension, h2.mode_extension);
        }
        // All three valid emphasis values must round-trip.
        for emph in [Emphasis::None, Emphasis::FiftyFifteen, Emphasis::CcittJ17] {
            let h = FrameHeader {
                lsf: false,
                bit_rate: 128_000,
                sample_rate: 44_100,
                padding: false,
                private_bit: false,
                mode: Mode::Stereo,
                mode_extension: ModeExtension::Bound4,
                copyright: false,
                original: true,
                emphasis: emph,
                protection_bit: true,
            };
            let h2 = FrameHeader::parse(&h.emit_bytes().unwrap()).unwrap();
            assert_eq!(h.emphasis, h2.emphasis);
        }
    }

    #[test]
    fn emit_bytes_rejects_unsupported_bitrate_and_sample_rate() {
        let mut h = FrameHeader {
            lsf: false,
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
            protection_bit: true,
        };
        // Baseline emits cleanly.
        assert!(h.emit_bytes().is_ok());
        // 200 kbit/s is not in the §2.4.2.3 ladder.
        h.bit_rate = 200_000;
        assert!(matches!(
            h.emit_bytes(),
            Err(HeaderError::UnsupportedBitrate(200_000))
        ));
        // Restore + tweak sample_rate.
        h.bit_rate = 192_000;
        h.sample_rate = 22_050; // LSF rate, not in the MPEG-1 table.
        assert!(matches!(
            h.emit_bytes(),
            Err(HeaderError::UnsupportedSamplingFrequency(22_050))
        ));
    }

    #[test]
    fn emit_bytes_rejects_disallowed_bitrate_mode_pair() {
        // 32 kbit/s + Stereo is forbidden by §2.4.2.3.
        let h = FrameHeader {
            lsf: false,
            bit_rate: 32_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
            protection_bit: true,
        };
        match h.emit_bytes() {
            Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode }) => {
                assert_eq!(bit_rate, 32_000);
                assert_eq!(mode, Mode::Stereo);
            }
            other => panic!("expected DisallowedBitrateModeCombination, got {other:?}"),
        }
    }

    #[test]
    fn emit_bytes_sets_syncword_id_and_layer_bits_correctly() {
        let h = FrameHeader {
            lsf: false,
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
            protection_bit: true,
        };
        let bytes = h.emit_bytes().unwrap();
        let word = u32::from_be_bytes(bytes);
        // Bits 31..20: syncword = 0xFFF.
        assert_eq!((word >> 20) & 0xFFF, 0xFFF);
        // Bit 19: ID = '1' (MPEG-1, §2.4.2.3).
        assert_eq!((word >> 19) & 0x1, 1);
        // Bits 18..17: layer = '10' (Layer II, §2.4.2.3).
        assert_eq!((word >> 17) & 0x3, 0b10);
    }

    #[test]
    fn emit_bytes_padding_and_protection_bit_flip_correctly() {
        for padding in [false, true] {
            for protection_bit in [false, true] {
                let h = FrameHeader {
                    lsf: false,
                    bit_rate: 192_000,
                    sample_rate: 44_100,
                    padding,
                    private_bit: false,
                    mode: Mode::Stereo,
                    mode_extension: ModeExtension::Bound4,
                    copyright: false,
                    original: true,
                    emphasis: Emphasis::None,
                    protection_bit,
                };
                let word = u32::from_be_bytes(h.emit_bytes().unwrap());
                assert_eq!(((word >> 9) & 0x1) == 1, padding);
                assert_eq!(((word >> 16) & 0x1) == 1, protection_bit);
                // Frame-size delta: padding bumps the byte count by 1.
                let n_base = 144u64 * 192_000 / 44_100;
                let want = n_base as usize + if padding { 1 } else { 0 };
                assert_eq!(h.frame_size_bytes(), want);
            }
        }
    }
}
