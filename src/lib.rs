//! MPEG Audio Layer II (MP2 / MUSICAM) codec.
//!
//! Implements the full Layer II decode pipeline per ISO/IEC 11172-3 and
//! 13818-3: frame header + CRC skip → bit-allocation decode (tables
//! B.2a–d for MPEG-1, consolidated LSF table for MPEG-2) → SCFSI +
//! scalefactor decode → 3-/5-/9-level grouped-sample ungrouping and
//! per-sample requantisation → 32-band polyphase subband synthesis.
//!
//! The encoder targets Layer II at any of the supported sampling
//! rates (32 / 44.1 / 48 kHz MPEG-1 or 16 / 22.05 / 24 kHz MPEG-2 LSF),
//! mono / plain stereo / **joint stereo** (intensity), no CRC. Both
//! **CBR** and **VBR** rate-control modes are exposed via the
//! [`options::Mp2EncoderOptions`] schema (`vbr_quality`, `joint_stereo`).
//! In VBR mode the encoder also emits a Xing/Info header as the first
//! frame of the stream so downstream players can show an accurate
//! average bitrate.
//!
//! Supports MPEG-1 sample rates (32 / 44.1 / 48 kHz) and MPEG-2 LSF
//! (16 / 22.05 / 24 kHz); every stereo mode (mono / stereo / joint-stereo
//! / dual-channel) on decode, mono / stereo / joint-stereo on encode.
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
pub mod options;
pub mod psy;
pub mod requant;
pub mod synth;
pub mod tables;

use oxideav_core::{CodecCapabilities, CodecId, CodecParameters, CodecTag, Result};
use oxideav_core::{CodecInfo, CodecRegistry, Decoder, Encoder};

pub const CODEC_ID_STR: &str = "mp2";

pub fn register_codecs(reg: &mut CodecRegistry) {
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

/// Unified registration entry point — installs MP2 into the codec
/// sub-registry of the supplied [`oxideav_core::RuntimeContext`].
pub fn register(ctx: &mut oxideav_core::RuntimeContext) {
    register_codecs(&mut ctx.codecs);
}

oxideav_core::register!("mp2", register);

fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    decoder::make_decoder(params)
}

fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    encoder::make_encoder(params)
}

#[cfg(test)]
mod register_tests {
    use super::*;

    #[test]
    fn register_via_runtime_context_installs_codec_factory() {
        let mut ctx = oxideav_core::RuntimeContext::new();
        register(&mut ctx);
        let id = CodecId::new(CODEC_ID_STR);
        assert!(
            ctx.codecs.has_decoder(&id),
            "decoder factory not installed via RuntimeContext"
        );
        assert!(
            ctx.codecs.has_encoder(&id),
            "encoder factory not installed via RuntimeContext"
        );
    }
}
