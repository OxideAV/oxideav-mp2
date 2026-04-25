//! MP2 packet → AudioFrame decoder, wired into [`oxideav_core::Decoder`].
//!
//! # Layout of one Layer II frame (ISO/IEC 11172-3 §2.4.1 + §2.4.2):
//! ```text
//!   32-bit header  [ + 16-bit CRC ]
//!   bit-allocation (variable)
//!   SCFSI          (2 bits per transmitted subband/channel)
//!   scalefactors   (0..3 × 6 bits per transmitted subband/channel)
//!   samples        (3 × 4 × per-triple codewords — sbgroup / triple / sb/ch)
//!   ancillary data (padding to frame end)
//! ```
//!
//! The synthesis filter bank produces 32 PCM samples per input 32-subband
//! granule; Layer II has 36 subband samples per subband per frame, split
//! into 3 × 12 "sbgroups" each sharing one scalefactor. 36 matrix pulls →
//! 36 × 32 = 1152 PCM samples per channel per frame.
//!
//! # Limitations
//! - MPEG-1 (32/44.1/48 kHz) and MPEG-2 LSF (16/22.05/24 kHz) are both
//!   supported. MPEG-2.5 (8/11.025/12 kHz) is rejected with
//!   `Error::Unsupported` at sync check time.
//! - CRC-16 is accepted (bits advanced) but not verified.
//! - Free-format and reserved bitrate/sample-rate indices are rejected at
//!   header parse time.

use oxideav_core::Decoder;
use oxideav_core::{
    AudioFrame, CodecId, CodecParameters, Error, Frame, Packet, Result, SampleFormat, TimeBase,
};

use crate::bitalloc::{read_layer2_side, validate_allocations};
use crate::header::{parse_header, Mode, Version};
use crate::requant::{read_samples, ReadState};
use crate::synth::SynthesisState;
use crate::tables::{select_alloc_table, TABLE_LSF};
use oxideav_core::bits::BitReader;

/// Build a Layer II decoder. The codec parameters are consulted for the
/// canonical `codec_id` only — everything else is derived from the
/// incoming frame headers.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    Ok(Box::new(Mp2Decoder {
        codec_id: params.codec_id.clone(),
        time_base: TimeBase::new(1, 48_000),
        pending: None,
        synth: [SynthesisState::new(), SynthesisState::new()],
        eof: false,
    }))
}

struct Mp2Decoder {
    codec_id: CodecId,
    time_base: TimeBase,
    /// Current input packet + byte offset of the next unparsed MP2 frame.
    /// AVI (and a few other containers) pack multiple 1152-sample MP2
    /// frames into a single container chunk; we need to iterate them
    /// rather than decode only the first.
    pending: Option<(Packet, usize)>,
    synth: [SynthesisState; 2],
    eof: bool,
}

impl Decoder for Mp2Decoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::other(
                "MP2 decoder: receive_frame must be called before sending another packet",
            ));
        }
        self.pending = Some((packet.clone(), 0));
        Ok(())
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        let (pkt, mut offset) = match self.pending.take() {
            Some(p) => p,
            None => {
                return if self.eof {
                    Err(Error::Eof)
                } else {
                    Err(Error::NeedMore)
                }
            }
        };

        // Skip any zero padding between frames — some muxers pad to
        // an even byte boundary or up to block_align.
        while offset < pkt.data.len() && pkt.data[offset] == 0 {
            offset += 1;
        }
        if offset >= pkt.data.len() {
            return if self.eof {
                Err(Error::Eof)
            } else {
                Err(Error::NeedMore)
            };
        }

        match self.decode_one(&pkt, offset) {
            Ok((frame, consumed)) => {
                let new_offset = offset + consumed;
                if new_offset < pkt.data.len() {
                    self.pending = Some((pkt, new_offset));
                }
                Ok(frame)
            }
            Err(e) => {
                // Parse failure anywhere after the first frame of a
                // multi-frame chunk: drop the rest of the packet rather
                // than poison the stream.
                if offset == 0 {
                    return Err(e);
                }
                if self.eof {
                    Err(Error::Eof)
                } else {
                    Err(Error::NeedMore)
                }
            }
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.eof = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        // The MP2 polyphase synthesis filter holds a 1024-sample FIFO per
        // channel that's updated sample-by-sample across frames. Without
        // wiping this, the first ~32 output samples after a seek are
        // convolved against pre-seek content. Rebuild both channel
        // synthesis states and drop the buffered packet.
        self.synth = [SynthesisState::new(), SynthesisState::new()];
        self.pending = None;
        self.eof = false;
        Ok(())
    }
}

impl Mp2Decoder {
    /// Parse and decode one MP2 frame starting at `start` inside `pkt.data`.
    /// Returns the decoded audio frame + the number of bytes consumed (the
    /// frame's declared length). The caller advances by that many bytes
    /// before calling again.
    fn decode_one(&mut self, pkt: &Packet, start: usize) -> Result<(Frame, usize)> {
        let full_data = &pkt.data;
        let data = &full_data[start..];
        let hdr = parse_header(data)?;
        let frame_len = hdr.frame_length();
        if data.len() < frame_len {
            return Err(Error::invalid(format!(
                "mp2: short frame: need {frame_len} bytes, got {}",
                data.len()
            )));
        }
        let channels = hdr.channels() as usize;

        // Skip past the header and optional CRC-16.
        let mut offset = 4usize;
        if hdr.protection {
            if data.len() < offset + 2 {
                return Err(Error::invalid("mp2: truncated frame (missing CRC)"));
            }
            // CRC not verified.
            offset += 2;
        }

        // MPEG-1 picks one of four tables from (sample_rate, mode, bitrate);
        // MPEG-2 LSF always uses the consolidated TABLE_LSF (sblimit = 30,
        // ISO/IEC 13818-3 §2.4.3.3).
        let stereo = !matches!(hdr.mode, Mode::Mono);
        let table = match hdr.version {
            Version::Mpeg1 => {
                let bri = bitrate_to_index(hdr.bitrate_kbps).ok_or_else(|| {
                    Error::invalid(format!(
                        "mp2: unexpected bitrate {} kbps for MPEG-1 table lookup",
                        hdr.bitrate_kbps
                    ))
                })?;
                select_alloc_table(hdr.sample_rate, stereo, bri)
            }
            Version::Mpeg2Lsf => &TABLE_LSF,
        };

        // The joint-stereo bound must be clamped to sblimit for the
        // allocation reader.
        let bound = (hdr.bound as usize).min(table.sblimit);

        let mut br = BitReader::new(&data[offset..frame_len]);

        // --- 1. Bit allocation, SCFSI, scalefactors ---
        let side = read_layer2_side(&mut br, table, hdr.mode, bound)?;
        validate_allocations(&side, table)?;

        // --- 2. Sample payload (36 samples × sblimit subbands × channels) ---
        let rs = ReadState {
            table,
            allocation: &side.allocation,
            scalefactor: &side.scalefactor,
            channels: side.channels,
            sblimit: table.sblimit,
            bound,
        };
        let subband_samples = read_samples(&mut br, &rs)?;

        // --- 3. 36 synthesis passes per channel → 1152 PCM samples/channel ---
        self.time_base = TimeBase::new(1, hdr.sample_rate as i64);
        let mut pcm = vec![[0.0f32; 1152]; channels];
        for step in 0..36 {
            for ch in 0..channels {
                let mut sb = [0.0f32; 32];
                for (sb_idx, item) in sb.iter_mut().enumerate().take(table.sblimit) {
                    *item = subband_samples[ch][sb_idx][step];
                }
                let mut out = [0.0f32; 32];
                self.synth[ch].synthesize(&sb, &mut out);
                pcm[ch][step * 32..(step + 1) * 32].copy_from_slice(&out);
            }
        }

        // Interleave & quantise to s16.
        let total_samples = 1152u32;
        let mut out_bytes = Vec::with_capacity(total_samples as usize * channels * 2);
        for i in 0..total_samples as usize {
            for ch_samples in pcm.iter().take(channels) {
                let f = ch_samples[i].clamp(-1.0, 1.0);
                let s = (f * 32767.0) as i16;
                out_bytes.extend_from_slice(&s.to_le_bytes());
            }
        }

        // PTS: the container's packet pts applies to the first frame
        // in the chunk. Subsequent frames step by the MP2 constant of
        // 1152 samples (in this sample-rate time_base). AVI's
        // per-frame pts is counted in samples (see avi demuxer) so
        // this produces monotonically-correct values.
        let frame_pts = pkt
            .pts
            .map(|p| p + (start as i64 / frame_len as i64) * 1152);
        let _ = full_data;

        Ok((
            Frame::Audio(AudioFrame {
                format: SampleFormat::S16,
                channels: channels as u16,
                sample_rate: hdr.sample_rate,
                samples: total_samples,
                pts: frame_pts,
                time_base: self.time_base,
                data: vec![out_bytes],
            }),
            frame_len,
        ))
    }
}

/// Reverse-map a bitrate in kbps to its header-field index (1..=14 for
/// MPEG-1 Layer II).
fn bitrate_to_index(bitrate_kbps: u32) -> Option<u32> {
    const LUT: [u32; 15] = [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ];
    LUT.iter()
        .position(|&v| v == bitrate_kbps)
        .map(|idx| idx as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_index_lookup() {
        assert_eq!(bitrate_to_index(128), Some(8));
        assert_eq!(bitrate_to_index(192), Some(10));
        assert_eq!(bitrate_to_index(32), Some(1));
        assert_eq!(bitrate_to_index(384), Some(14));
        assert_eq!(bitrate_to_index(999), None);
    }
}
