#![no_main]

//! Encode→decode **conformance** fuzzer for the oxideav-mp2 write
//! side (r453 depth lane).
//!
//! The `decode` target treats the decoder as an attacker surface; this
//! target treats the *encoder* as the system under test: from
//! arbitrary bytes it derives a legal header + configuration + PCM
//! material, runs the two-channel Layer II encode and the ISO/IEC
//! 13818-3 §2.5 multichannel encode (dynamic crosstalk, adaptive
//! `tc_allocation`, prediction, phantom centre, second stereo, LFE,
//! multilingual, extension bit stream — every encode-side election),
//! and asserts the **conformance contract**, not mere panic-freedom:
//!
//! * an `Ok` encode MUST decode through this crate's own decoders —
//!   `decode_all_frames` on the base bytes (the §2.5.1.3
//!   backward-compatibility guarantee) and `decode_mc_stream` on the
//!   multichannel stream — with the emitted frame count, presentation
//!   layout, LFE presence and multilingual count all matching the
//!   configuration;
//! * an `Err` encode must be one of the *declared* rejections
//!   (`BudgetTooSmall` / `Base` allocator failures on starved
//!   budgets) — shape errors cannot occur because the harness feeds
//!   well-formed shapes.
//!
//! PCM samples are derived from the fuzz bytes (roughly full-scale,
//! never NaN/inf), so scalefactor extremes, silence runs and
//! clipping-adjacent material all reach the quantizers.
//!
//! Spec basis: ISO/IEC 11172-3 §2.4 (base layer), ISO/IEC 13818-3
//! §2.5.1 / §2.5.2 / §2.5.3 / §C.2 (multichannel extension).

use libfuzzer_sys::fuzz_target;
use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::mc::decode_mc_stream;
use oxideav_mp2::mc_encode::{
    encode_mc_all_frames_ext, McEncodeConfig, McEncodeError, LFE_SAMPLES_PER_FRAME,
};
use oxideav_mp2::{decode_all_frames, encode_all_frames, FrameHeader, PCM_SAMPLES_PER_CHANNEL};

const RATES: [u32; 3] = [32_000, 44_100, 48_000];
/// Bitrates with a §2.4.3.1 allocation table for a 2-ch base at every
/// MPEG-1 rate (≥ 96 kbit/s per pair covers 44,1/32 kHz too), plus a
/// deliberately starved 64 kbit/s row so the declared budget
/// rejections stay reachable.
const BITRATES: [u32; 5] = [64_000, 128_000, 192_000, 256_000, 384_000];

fn pcm_from(data: &[u8], seed: usize, len: usize) -> Vec<f64> {
    if data.is_empty() {
        return vec![0.0; len];
    }
    (0..len)
        .map(|i| {
            let b = data[(seed + i) % data.len()];
            // [-0.9921875, +0.984375] — inside nominal range, hits
            // both signs and zero.
            (f64::from(b) - 127.0) / 128.0 * 0.9921875
        })
        .collect()
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let (c0, c1, c2, c3) = (data[0], data[1], data[2], data[3]);
    let sample_rate = RATES[usize::from(c0) % RATES.len()];
    let bit_rate = BITRATES[usize::from(c0 >> 2) % BITRATES.len()];
    let header = FrameHeader {
        lsf: false,
        protection_bit: c2 & 1 != 0,
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    };

    // ---- two-channel base encode → decode ---------------------------
    let n_frames = 1 + usize::from(c1 & 1);
    let base_pcm: Vec<Vec<f64>> = (0..2)
        .map(|ch| pcm_from(data, 4 + ch * 7, n_frames * PCM_SAMPLES_PER_CHANNEL))
        .collect();
    match encode_all_frames(&header, &base_pcm, 0) {
        Ok(bytes) => {
            let pcm = decode_all_frames(&bytes).expect("2-ch encode must decode");
            assert_eq!(pcm.len(), 2);
            assert_eq!(pcm[0].len(), n_frames * PCM_SAMPLES_PER_CHANNEL);
        }
        Err(_) => {
            // Starved budgets / off-table (rate, bitrate) rows are
            // declared rejections.
        }
    }

    // ---- §2.5 multichannel encode → decode --------------------------
    let second_stereo = c1 & 0b10 != 0;
    let (front, surround) = if second_stereo {
        (2 + (c1 >> 2) % 2, 0)
    } else {
        match (c1 >> 2) % 5 {
            0 => (3, 2),
            1 => (3, 1),
            2 => (3, 0),
            3 => (2, 2),
            _ => (2, 1),
        }
    };
    let dematrix = match (c1 >> 5) % 4 {
        2 if front == 3 && surround >= 1 => 2,
        3 => 3,
        1 => 1,
        _ => 0,
    };
    let phantom = c2 & 0b10 != 0 && front == 3 && dematrix != 3;
    let cfg = McEncodeConfig {
        front,
        surround,
        lfe: c2 & 0b100 != 0,
        dematrix_procedure: dematrix,
        lfe_allocation: 2 + (c3 % 14),
        mc_bits: None,
        tc_allocation: 0,
        adaptive_tc: c2 & 0b1000 != 0 && dematrix != 3,
        dyn_cross: c2 & 0b1_0000 != 0,
        phantom_centre: phantom,
        second_stereo,
        multilingual: (c3 >> 4) % 3,
        multilingual_fs_half: c3 & 0b1000_0000 != 0,
        ext_bit_stream: c2 & 0b10_0000 != 0,
        prediction: c2 & 0b100_0000 != 0,
    };
    if cfg.validate().is_err() {
        unreachable!("harness built an invalid configuration: {cfg:?}");
    }
    let mc_frames = 1usize;
    let pcm: Vec<Vec<f64>> = (0..cfg.presentation_channels())
        .map(|ch| pcm_from(data, 5 + ch * 11, mc_frames * PCM_SAMPLES_PER_CHANNEL))
        .collect();
    let lfe_buf;
    let lfe = if cfg.lfe {
        lfe_buf = pcm_from(data, 6, mc_frames * LFE_SAMPLES_PER_FRAME);
        Some(lfe_buf.as_slice())
    } else {
        None
    };
    let ml: Vec<Vec<f64>> = (0..usize::from(cfg.multilingual))
        .map(|ch| {
            pcm_from(
                data,
                7 + ch * 13,
                mc_frames * cfg.multilingual_samples_per_frame(),
            )
        })
        .collect();
    match encode_mc_all_frames_ext(&header, &cfg, &pcm, lfe, &ml) {
        Ok(stream) => {
            assert_eq!(stream.ext.is_some(), cfg.ext_bit_stream);
            // Conformance: the emitted stream must decode.
            let decoded = decode_mc_stream(&stream.base, stream.ext.as_deref())
                .expect("mc encode must decode");
            assert_eq!(decoded.frames, mc_frames);
            assert_eq!(decoded.channels.len(), cfg.presentation_channels());
            assert_eq!(decoded.lfe.is_some(), cfg.lfe);
            assert_eq!(
                decoded.multilingual.len(),
                usize::from(cfg.multilingual),
                "multilingual count"
            );
            assert_eq!(
                decoded.mc_header.dematrix_procedure,
                cfg.dematrix_procedure
            );
            // …and the base bytes stay an ordinary Layer II stream.
            let base = decode_all_frames(&stream.base).expect("compatible base must decode");
            assert_eq!(base.len(), 2);
        }
        Err(
            McEncodeError::BudgetTooSmall { .. }
            | McEncodeError::Base(_)
            | McEncodeError::ExtFrameTooLarge { .. },
        ) => {
            // Declared budget rejections (starved bitrates).
        }
        Err(other) => panic!("undeclared mc encode rejection: {other}"),
    }
});
