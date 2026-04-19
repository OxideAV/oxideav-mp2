//! MPEG Audio Layer II (MP2 / MUSICAM) codec.
//!
//! Implements the full Layer II decode pipeline per ISO/IEC 11172-3 and
//! 13818-3: frame header + CRC skip → bit-allocation decode (tables
//! B.2a–d for MPEG-1, consolidated LSF table for MPEG-2) → SCFSI +
//! scalefactor decode → 3-/5-/9-level grouped-sample ungrouping and
//! per-sample requantisation → 32-band polyphase subband synthesis.
//!
//! The encoder targets CBR Layer II at any of the supported sampling
//! rates (32 / 44.1 / 48 kHz MPEG-1 or 16 / 22.05 / 24 kHz MPEG-2 LSF),
//! mono or plain stereo, no CRC, no joint stereo.
//!
//! Supports MPEG-1 sample rates (32 / 44.1 / 48 kHz) and MPEG-2 LSF
//! (16 / 22.05 / 24 kHz); every stereo mode (mono / stereo / joint-stereo
//! / dual-channel) on decode, plain stereo / mono on encode.
//!
//! MPEG-2.5 is rejected with `Unsupported`.

#![allow(
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items,
    clippy::excessive_precision
)]

pub mod analysis;
pub mod bitalloc;
pub mod decoder;
pub mod encoder;
pub mod header;
pub mod requant;
pub mod synth;
pub mod tables;

use oxideav_codec::{CodecInfo, CodecRegistry, Decoder, Encoder};
use oxideav_core::{CodecCapabilities, CodecId, CodecParameters, CodecTag, Result};

pub const CODEC_ID_STR: &str = "mp2";

pub fn register(reg: &mut CodecRegistry) {
    let cid = CodecId::new(CODEC_ID_STR);
    let dec_caps = CodecCapabilities::audio("mp2_sw_dec")
        .with_lossy(true)
        .with_intra_only(true)
        .with_max_channels(2)
        .with_max_sample_rate(48_000);
    // AVI / WAVEFORMATEX tag: WAVE_FORMAT_MPEG = 0x0050 — MPEG-1 Audio
    // Layer I/II. Layer III (MP3) has its own tag 0x0055 owned by the
    // oxideav-mp3 crate.
    reg.register(
        CodecInfo::new(cid.clone())
            .capabilities(dec_caps)
            .decoder(make_decoder)
            .tag(CodecTag::wave_format(0x0050)),
    );

    let enc_caps = CodecCapabilities::audio("mp2_sw_enc")
        .with_lossy(true)
        .with_intra_only(true)
        .with_max_channels(2)
        .with_max_sample_rate(48_000);
    reg.register(
        CodecInfo::new(cid)
            .capabilities(enc_caps)
            .encoder(make_encoder),
    );
}

fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    decoder::make_decoder(params)
}

fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    encoder::make_encoder(params)
}
