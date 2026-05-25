//! MPEG-1 Audio Layer II frame header parsing — ISO/IEC 11172-3 (1993),
//! §2.4.1.3 (header syntax) and §2.4.2.3 (header field semantics).
//!
//! Clean-room: every numeric value in this module is derived from the
//! staged `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (157-page edition,
//! SHA-256 `ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`).
//! No third-party MP2 implementation source was consulted.
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
//! For Layer II the `bitrate_index` ladder is (kbit/s):
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
//! The `sampling_frequency` table (PDF page 21) is:
//!
//! | code | rate (kHz) |
//! |-----:|-----------:|
//! | 00 | 44,1 |
//! | 01 | 48 |
//! | 10 | 32 |
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
    /// `ID` was `'0'`. The §2.4.2.3 prose marks `'0'` as reserved for
    /// MPEG-1 audio; ISO/IEC 13818-3 later reuses it for LSF, but this
    /// crate currently scopes to MPEG-1 Layer II.
    LsfNotSupported,
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
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeaderError::BufferTooShort => {
                write!(f, "buffer is shorter than the 4-byte Layer II header")
            }
            HeaderError::BadSync => write!(f, "syncword is not 0xFFF"),
            HeaderError::LsfNotSupported => write!(
                f,
                "ID == 0 (ISO/IEC 13818-3 LSF) not yet supported by this crate"
            ),
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

/// Parsed MPEG-1 Layer II frame header (§2.4.1.3 + §2.4.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// Bitrate in bit/s (i.e. `kbit/s * 1000`), per the §2.4.2.3
    /// `bitrate_index` ladder column "Layer II".
    pub bit_rate: u32,
    /// Sampling frequency in Hz (44100, 48000 or 32000).
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
    /// Parse a 4-byte MPEG-1 Layer II header from the front of `buf`.
    ///
    /// Performs the §2.4.2.3 validation up-front: rejects bad syncwords,
    /// LSF (`ID == 0`), non-Layer-II frames, forbidden / reserved table
    /// codes, and the §2.4.2.3 disallowed `(bitrate, mode)` combinations.
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
        if id == 0 {
            return Err(HeaderError::LsfNotSupported);
        }

        let layer_bits = ((word >> 17) & 0x3) as u8;
        // §2.4.2.3: '11' = Layer I, '10' = Layer II, '01' = Layer III,
        // '00' = reserved.
        if layer_bits != 0b10 {
            return Err(HeaderError::UnsupportedLayer(layer_bits));
        }

        let protection_bit = ((word >> 16) & 0x1) == 1;

        let bitrate_index = ((word >> 12) & 0xF) as u8;
        let bit_rate = decode_bitrate(bitrate_index)?;

        let sf_index = ((word >> 10) & 0x3) as u8;
        let sample_rate = decode_sampling_frequency(sf_index)?;

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
        // and mode are allowed" — table on PDF page 21.
        if !is_layer2_bitrate_mode_allowed(bit_rate, mode) {
            return Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode });
        }

        Ok(FrameHeader {
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
    fn rejects_lsf_id_bit() {
        // ID = 0 by setting bit 19 = 0 (i.e. the byte after sync top is
        // 0xF5 with ID bit cleared but layer '10' protection '1').
        // Sync 0xFFF (12 bits), then ID=0, layer=10, protection=1 →
        // byte 1 low nibble = 0b0101 = 0x5.
        let bytes = [0xFF, 0xF5, 0xA0, 0x04];
        assert_eq!(
            FrameHeader::parse(&bytes),
            Err(HeaderError::LsfNotSupported)
        );
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
}
