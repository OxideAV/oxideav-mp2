//! MPEG audio frame header (ISO/IEC 11172-3 §2.4.1 + ISO/IEC 13818-3 §2.4.1).
//!
//! A 32-bit frame header in big-endian bit order:
//!
//! ```text
//!  syncword          12  0xFFF
//!  ID                 1  1 = MPEG-1, 0 = MPEG-2 LSF
//!  (MPEG-1/2 use the 1-bit form; the 2-bit version_id is `0b11`=MPEG-1,
//!   `0b10`=MPEG-2 LSF, `0b00`=MPEG-2.5, `0b01`=reserved. We only accept
//!   MPEG-1 and MPEG-2 LSF.)
//!  layer              2  `10` = Layer II
//!  protection_bit     1  0 = CRC-16 follows
//!  bitrate_index      4
//!  sampling_frequency 2
//!  padding_bit        1
//!  private_bit        1
//!  mode               2  00=stereo 01=JS 10=dual 11=mono
//!  mode_extension     2  (joint stereo only — bound subband index)
//!  copyright          1
//!  original           1
//!  emphasis           2
//! ```
//!
//! This decoder accepts:
//! - MPEG-1 Layer II at 32 / 44.1 / 48 kHz (ISO/IEC 11172-3).
//! - MPEG-2 Layer II LSF (Lower Sampling Frequencies) at 16 / 22.05 /
//!   24 kHz (ISO/IEC 13818-3 §2.4).
//!
//! MPEG-2.5 (`version_id == 0b00`, 8 / 11.025 / 12 kHz) is refused.

use oxideav_core::{Error, Result};

/// MPEG audio channel mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stereo,
    JointStereo,
    DualChannel,
    Mono,
}

impl Mode {
    pub fn channels(self) -> u16 {
        match self {
            Mode::Mono => 1,
            _ => 2,
        }
    }
}

/// MPEG audio version. Selects the sample-rate table, bitrate ladder, and
/// bit-allocation tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// MPEG-1 (ISO/IEC 11172-3), sample rates 32 / 44.1 / 48 kHz.
    Mpeg1,
    /// MPEG-2 LSF (ISO/IEC 13818-3 §2.4), sample rates 16 / 22.05 / 24 kHz.
    Mpeg2Lsf,
}

/// Parsed Layer II frame header (MPEG-1 or MPEG-2 LSF).
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub version: Version,
    /// `true` when the stream carries a 16-bit CRC immediately after the header.
    pub protection: bool,
    /// Audio bitrate in kilobits per second.
    pub bitrate_kbps: u32,
    /// Sampling frequency in Hz.
    pub sample_rate: u32,
    /// Padding slot present (1 additional byte for Layer II).
    pub padding: bool,
    pub mode: Mode,
    /// Index of the first intensity-stereo subband (joint stereo only).
    pub bound: u32,
}

impl Header {
    /// Total frame length in bytes, including the header.
    pub fn frame_length(&self) -> usize {
        // Layer II: frame_length = 144 * bitrate / sample_rate + padding
        let base = 144 * self.bitrate_kbps as usize * 1000 / self.sample_rate as usize;
        base + self.padding as usize
    }

    pub fn channels(&self) -> u16 {
        self.mode.channels()
    }

    /// Number of subbands that carry stereo data. Subbands from `bound` to 32
    /// are intensity-stereo coded (joint stereo only).
    pub fn sblimit(&self, allocation_table: &crate::tables::AllocTable) -> usize {
        allocation_table.sblimit
    }
}

/// MPEG-1 Layer II bitrate table (kbps). Index 0 = "free", index 15 = reserved.
/// ISO/IEC 11172-3 Table 3-B.2 ("bitrate_index" column for Layer II).
pub(crate) const BITRATE_LAYER2_MPEG1_KBPS: [u32; 15] = [
    0, // free format
    32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];

/// MPEG-2 LSF Layer II bitrate table (kbps). Index 0 = "free", index 15 =
/// reserved. ISO/IEC 13818-3 §2.4.2.3, Table in that section. (For LSF,
/// Layers I, II and III share a single table whose Layer-II column is the
/// sequence 0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160.)
pub(crate) const BITRATE_LAYER2_MPEG2_KBPS: [u32; 15] = [
    0, // free format
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160,
];

/// MPEG-1 sample-rate table (Hz), indexed by `sampling_frequency`.
const SAMPLE_RATE_MPEG1_HZ: [u32; 3] = [44100, 48000, 32000];
/// MPEG-2 LSF sample-rate table (Hz). ISO/IEC 13818-3 §2.4.2.3: the LSF
/// rates are half the MPEG-1 rates, with the same index ordering.
const SAMPLE_RATE_MPEG2_HZ: [u32; 3] = [22050, 24000, 16000];

/// Parse a 4-byte MPEG Layer II frame header starting at `buf[0]`. Handles
/// both MPEG-1 and MPEG-2 LSF.
pub fn parse_header(buf: &[u8]) -> Result<Header> {
    if buf.len() < 4 {
        return Err(Error::invalid("mp2 header: need at least 4 bytes"));
    }
    let w = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);

    let sync = w >> 20;
    if sync != 0xFFF {
        return Err(Error::invalid("mp2 header: missing 0xFFF sync word"));
    }
    // 2-bit version field: bit 20 (low bit of sync if interpreted as 12-bit)
    // combined with the 1-bit ID at bit 19. Decoding:
    //   0b11 = MPEG-1        (id=1 with the canonical 12-bit sync 0xFFF)
    //   0b10 = MPEG-2 LSF    (id=0 with the canonical 12-bit sync 0xFFF)
    //   0b00 = MPEG-2.5      (11-bit sync 0xFFE; already rejected by the
    //                        `0xFFF` gate above)
    //   0b01 = reserved      (also filtered by the sync gate)
    // Given we've already passed the `w >> 20 == 0xFFF` test, bit 20 is 1
    // and the version is fully determined by the ID bit.
    let id_bit = (w >> 19) & 0x1;
    let version = if id_bit == 1 {
        Version::Mpeg1
    } else {
        Version::Mpeg2Lsf
    };
    let layer = (w >> 17) & 0x3; // 10 = Layer II
    let protection_bit = (w >> 16) & 0x1; // 0 = CRC present
    let bitrate_index = ((w >> 12) & 0xF) as usize;
    let sr_index = ((w >> 10) & 0x3) as usize;
    let padding = ((w >> 9) & 0x1) != 0;
    let mode_code = (w >> 6) & 0x3;
    let mode_ext = (w >> 4) & 0x3;

    if layer != 0b10 {
        return Err(Error::unsupported(format!(
            "mp2 header: layer bits {layer:02b} — only Layer II is handled"
        )));
    }
    if bitrate_index == 0 {
        return Err(Error::unsupported("mp2 header: free-format not supported"));
    }
    if bitrate_index == 15 {
        return Err(Error::invalid("mp2 header: reserved bitrate index 15"));
    }
    if sr_index >= 3 {
        return Err(Error::invalid("mp2 header: reserved sampling index"));
    }

    let (bitrate_kbps, sample_rate) = match version {
        Version::Mpeg1 => (
            BITRATE_LAYER2_MPEG1_KBPS[bitrate_index],
            SAMPLE_RATE_MPEG1_HZ[sr_index],
        ),
        Version::Mpeg2Lsf => (
            BITRATE_LAYER2_MPEG2_KBPS[bitrate_index],
            SAMPLE_RATE_MPEG2_HZ[sr_index],
        ),
    };

    let mode = match mode_code {
        0 => Mode::Stereo,
        1 => Mode::JointStereo,
        2 => Mode::DualChannel,
        _ => Mode::Mono,
    };

    // Joint stereo: mode_extension selects the bound subband
    // (§2.4.2.3, Table 3-B.3): 00→4, 01→8, 10→12, 11→16.
    let bound = match mode {
        Mode::JointStereo => match mode_ext {
            0 => 4,
            1 => 8,
            2 => 12,
            _ => 16,
        },
        Mode::Mono => 32,
        _ => 32, // stereo / dual-channel: no intensity coding
    };

    // Validate Layer II bitrate × channel-mode combination (§2.4.2.3, Table 3-B.2).
    // MPEG-1 only: mono forbids ≥ 224 kbps, stereo modes forbid 32 / 48 kbps.
    // MPEG-2 LSF has no such restrictions — all 14 bitrates work in any mode
    // (ISO/IEC 13818-3 §2.4.2.3 relaxes these rules as lower bitrates are
    // typical for the lower sampling rates).
    if matches!(version, Version::Mpeg1) {
        match mode {
            Mode::Mono if matches!(bitrate_kbps, 224 | 256 | 320 | 384) => {
                return Err(Error::invalid(format!(
                    "mp2 header: bitrate {bitrate_kbps} kbps not permitted in single-channel mode"
                )));
            }
            Mode::Mono => {}
            _ if matches!(bitrate_kbps, 32 | 48) => {
                return Err(Error::invalid(format!(
                    "mp2 header: bitrate {bitrate_kbps} kbps not permitted in stereo modes"
                )));
            }
            _ => {}
        }
    }

    let _ = protection_bit;
    Ok(Header {
        version,
        protection: protection_bit == 0,
        bitrate_kbps,
        sample_rate,
        padding,
        mode,
        bound,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stereo_192kbps_48k() {
        // sync=0xFFF, ID=1, layer=10, prot=1 (no CRC), bitrate=1010 (192),
        // sr=01 (48k), pad=0, priv=0, mode=00 (stereo), modeext=0, cp=0, orig=0, emph=00
        let w: u32 = 0xFFF_u32 << 20 | 1 << 19 | 0b10 << 17 | 1 << 16 | 0b1010 << 12 | 0b01 << 10;
        let bytes = w.to_be_bytes();
        let h = parse_header(&bytes).unwrap();
        assert_eq!(h.version, Version::Mpeg1);
        assert_eq!(h.bitrate_kbps, 192);
        assert_eq!(h.sample_rate, 48000);
        assert_eq!(h.channels(), 2);
        assert_eq!(h.mode, Mode::Stereo);
        assert!(!h.protection);
        // Layer-II length at 192 kbps / 48 kHz = 144 * 192000 / 48000 = 576.
        assert_eq!(h.frame_length(), 576);
    }

    #[test]
    fn reject_layer3() {
        let w: u32 = 0xFFF_u32 << 20 | 1 << 19 | 0b01 << 17 | 1 << 16 | 0b1010 << 12 | 0b01 << 10;
        let bytes = w.to_be_bytes();
        assert!(parse_header(&bytes).is_err());
    }

    #[test]
    fn parse_mpeg2_lsf_mono_64kbps_24k() {
        // sync=0xFFF, version=0b10 (MPEG-2 LSF) → id=0, layer=10 (Layer II),
        // prot=1 (no CRC), bitrate=1000 (64 kbps on LSF ladder),
        // sr=01 (24 kHz on LSF table), pad=0, priv=0, mode=11 (mono),
        // modeext=0, cp=0, orig=0, emph=00.
        //
        //   bits:    31..20  19   18..17  16   15..12   11..10  9  8  7..6  5..4  3  2  1..0
        //   values:  0xFFF   0    10      1    1000     01      0  0  11    00    0  0  00
        // id=0 (MPEG-2 LSF) so bit 19 is 0 — it contributes nothing; we omit
        // the `0 << 19` term that clippy flags as identity_op.
        let w: u32 = 0xFFF_u32 << 20 | 0b10 << 17 | 1 << 16 | 0b1000 << 12 | 0b01 << 10 | 0b11 << 6;
        let bytes = w.to_be_bytes();
        let h = parse_header(&bytes).unwrap();
        assert_eq!(h.version, Version::Mpeg2Lsf);
        assert_eq!(h.bitrate_kbps, 64);
        assert_eq!(h.sample_rate, 24000);
        assert_eq!(h.channels(), 1);
        assert_eq!(h.mode, Mode::Mono);
        // frame_length = 144 * 64000 / 24000 = 384 bytes.
        assert_eq!(h.frame_length(), 384);
    }

    #[test]
    fn reject_mpeg2_5_sync_mismatch() {
        // MPEG-2.5 streams use an 11-bit sync (0xFFE) with version_id bit 20
        // reset to 0. Our 12-bit sync check (`w >> 20 == 0xFFF`) already
        // rejects those at the sync stage. Verify by constructing a word
        // that would have been a valid MPEG-2.5 frame (sync 0xFFE, version
        // bits 00, layer II, bitrate 8 kbps, sr=24k→resolves to 8k for 2.5).
        let w: u32 = 0xFFE_u32 << 20 | 0b10 << 17 | 1 << 16 | 0b0001 << 12 | 0b01 << 10 | 0b11 << 6;
        let bytes = w.to_be_bytes();
        assert!(parse_header(&bytes).is_err());
    }
}
