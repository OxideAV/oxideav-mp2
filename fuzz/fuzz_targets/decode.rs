#![no_main]

//! Drive attacker-supplied bytes through the oxideav-mp2 Layer II
//! decode surface: the `decode_frame` free function, the streaming
//! `decode_all_frames` chain, and the registered `Decoder` trait
//! object.
//!
//! Round 296 depth-mode lane: a panic-freedom fuzzer over the Layer
//! II decoder's attacker surface. The contract under test is purely
//! that every public decode entry point returns a `Result` (never
//! panics, never integer-overflows in a debug build, never indexes out
//! of bounds) on arbitrary input, across a multi-frame buffer so the
//! sync-resync loop, the multi-frame chaining, and the cross-frame
//! `FrameDecodeState` filterbank carry-over are exercised — not just a
//! single cold-start frame.
//!
//! ## Why structurally-valid headers
//!
//! A free-form byte fuzzer almost never produces the 12-bit frame sync
//! (ISO/IEC 11172-3 §2.4.2.3) followed by the `layer == '10'` and a
//! legal `bitrate_index` / `sampling_frequency` pair, so it spends all
//! its energy on the `BadSync` / `UnsupportedLayer` rejection and never
//! reaches bit-allocation parse, scfsi expansion, scalefactor read,
//! sample requantisation, or the polyphase synthesis filterbank. This
//! target instead *constructs* a valid 4-byte header whose every field
//! (MPEG-1 vs MPEG-2 LSF, bitrate index, sample-rate index, channel
//! mode, mode extension, CRC flag, padding) is attacker-chosen, then
//! fills the remainder of the header-implied frame length with attacker
//! bytes for the optional CRC slot, the §2.4.1.6 audio-data side info,
//! and the §2.4.3.3.4 sample codewords. The deep decode chain is
//! therefore reached on essentially every iteration, with the bytes
//! past the header fully attacker-controlled.
//!
//! ## Cross-frame state
//!
//! The polyphase synthesis filterbank carries its V ring buffer across
//! successive frames (Annex A Figure A.2 footnote 1: V is zeroed only
//! at startup). A single cold-start frame never exercises the
//! carry-over. This target stitches several crafted frames into one
//! buffer fed to `decode_all_frames` and one `Decoder` session so those
//! transitions are reachable, and injects a mid-stream `reset()` +
//! `flush()` so the trait state machine transitions through both calls.
//!
//! ## Surfaces driven on every iteration
//!
//! 1. `oxideav_mp2::decode_frame` on the first crafted frame (cold
//!    start) plus a verbatim raw-byte slice (so the `< 4 bytes` /
//!    `BadSync` / `Truncated` rejections are hit).
//! 2. `oxideav_mp2::decode_all_frames` on the full multi-frame buffer
//!    (sync-resync skip path + chaining + filterbank carry-over).
//! 3. `oxideav_mp2::register_codecs` → `first_decoder` factory surface,
//!    then `Decoder::send_packet` / `receive_frame` / `flush` / `reset`
//!    across the crafted frames.
//!
//! Output PCM is discarded — the fuzzer only cares about *return*,
//! never sample-correctness (that is the integration tests' job).
//!
//! Spec basis: ISO/IEC 11172-3 §2.4.1.3 / §2.4.2.3 (frame header /
//! sync), §2.4.1.6 / §2.4.3.3.1..4 (bit allocation, scfsi,
//! scalefactors, sample requantisation), §2.4.3.2 / §2.4.3.3.5
//! (polyphase synthesis filterbank), §2.4.3.1 (CRC-16 verify), plus
//! ISO/IEC 13818-3 §2.4.2.3 (LSF bitrate / sampling ladders).

use libfuzzer_sys::fuzz_target;
use oxideav_core::{CodecId, CodecParameters, CodecRegistry, Decoder, Packet, Rational, TimeBase};
use oxideav_mp2::{decode_all_frames, decode_frame, FrameHeader};

const MAX_FRAMES_PER_ITER: usize = 12;
/// Bound the crafted-frame body so a malicious bitrate/sample-rate
/// combination can't pin a worker on a large allocation per frame. The
/// largest legal MPEG-1 Layer II frame is 384 kbit/s @ 32 kHz =
/// floor(144 * 384000 / 32000) + 1 = 1729 bytes; this cap covers it
/// with margin.
const MAX_FRAME_BYTES: usize = 2048;
/// Bytes consumed from the attacker buffer to craft one header.
const HEADER_CTL_BYTES: usize = 6;

/// Assemble a structurally-valid 4-byte Layer II header from
/// attacker-chosen field bits, returning the four header bytes.
///
/// The field choices are constrained to the *accepted* ranges so the
/// header actually parses (the decoder only reaches the deep chain on a
/// parsing header); within those ranges every field is
/// attacker-controlled. `id` selects MPEG-1 (`'1'`) vs MPEG-2 LSF
/// (`'0'`); `br` the bitrate index (1..=14, never 0 free-format or 15
/// forbidden); `sr` the sample-rate index (0..=2, 3 reserved/rejected);
/// `mode` the channel mode; `ext` the mode extension; and `flags` packs
/// the CRC, padding, private, copyright, and original bits. Layer is
/// fixed to II (`'10'`) and emphasis to none (`'00'`) so those fields
/// never trip the up-front rejection.
fn build_header(id: u8, sr: u8, br: u8, mode: u8, ext: u8, flags: u8) -> [u8; 4] {
    // 12-bit sync (bits 31..20 all ones).
    let mut raw: u32 = 0xFFF << 20;
    // ID: '1' MPEG-1, '0' MPEG-2 LSF.
    raw |= u32::from(id & 1) << 19;
    // Layer II = '10'.
    raw |= 0b10u32 << 17;
    // protection_bit: '0' = CRC present, '1' = no CRC.
    let crc_present = flags & 1;
    raw |= u32::from(crc_present ^ 1) << 16;
    // bitrate index 1..=14 (avoid 0 free-format and 15 forbidden).
    let br_idx = (br % 14) + 1;
    raw |= u32::from(br_idx) << 12;
    // sample-rate index 0..=2 (3 is reserved/rejected).
    let sr_idx = sr % 3;
    raw |= u32::from(sr_idx) << 10;
    // padding bit.
    raw |= u32::from((flags >> 1) & 1) << 9;
    // private bit.
    raw |= u32::from((flags >> 2) & 1) << 8;
    // mode 0..=3.
    raw |= u32::from(mode & 0b11) << 6;
    // mode extension 0..=3.
    raw |= u32::from(ext & 0b11) << 4;
    // copyright / original.
    raw |= u32::from((flags >> 3) & 1) << 3;
    raw |= u32::from((flags >> 4) & 1) << 2;
    // emphasis '00' = none (the '10' reserved code is rejected).
    raw.to_be_bytes()
}

/// Craft one frame: a valid header drawn from `ctl` plus an
/// attacker-byte body sized to the header-implied frame length. Returns
/// `None` only when the header (against all odds) fails to parse, in
/// which case the caller treats the slot as raw bytes.
fn craft_frame(ctl: &[u8; HEADER_CTL_BYTES], body_src: &[u8]) -> Option<Vec<u8>> {
    let hdr = build_header(ctl[0], ctl[1], ctl[2], ctl[3], ctl[4], ctl[5]);
    let header = FrameHeader::parse(&hdr).ok()?;
    let frame_len = header.frame_size_bytes().clamp(4, MAX_FRAME_BYTES);
    let mut frame = vec![0u8; frame_len];
    frame[..4].copy_from_slice(&hdr);
    // Fill the rest from attacker bytes, walking the source buffer so
    // each byte position draws a distinct attacker byte where possible.
    if !body_src.is_empty() {
        for (j, slot) in frame.iter_mut().enumerate().skip(4) {
            *slot = body_src[(j - 4) % body_src.len()];
        }
    }
    Some(frame)
}

fn build_decoder() -> Option<Box<dyn Decoder>> {
    let mut reg = CodecRegistry::new();
    oxideav_mp2::register_codecs(&mut reg);
    let id = CodecId::new(oxideav_mp2::CODEC_ID_STR);
    let mut params = CodecParameters::audio(id);
    params.channels = Some(2);
    params.sample_rate = Some(44_100);
    reg.first_decoder(&params).ok()
}

fuzz_target!(|data: &[u8]| {
    // Always exercise the free-function entry points with the raw
    // attacker buffer — this covers the `< 4 bytes`, `BadSync`, and
    // `Truncated` rejection paths with zero crafted structure.
    let _ = decode_frame(data);
    let _ = decode_all_frames(data);

    if data.is_empty() {
        // A 0-byte packet still exercises the cold-decoder `< 4 bytes`
        // rejection on the trait surface.
        if let Some(mut dec) = build_decoder() {
            let tb = TimeBase(Rational::new(1, 44_100));
            let _ = dec.send_packet(&Packet::new(0, tb, Vec::new()));
        }
        return;
    }

    let frame_count = ((data[0] as usize) % MAX_FRAMES_PER_ITER) + 1;
    // Deterministic mid-stream reset / flush hooks (derived from
    // data[0] so libfuzzer's minimiser can still shrink reliably).
    let reset_at = if frame_count >= 3 {
        (data[0] as usize) % frame_count
    } else {
        usize::MAX
    };
    let flush_at = if frame_count >= 2 {
        ((data[0] >> 4) as usize) % frame_count
    } else {
        usize::MAX
    };

    // Build a multi-frame buffer of crafted frames, then run it through
    // the streaming `decode_all_frames` chain AND the trait decoder.
    let mut crafted: Vec<Vec<u8>> = Vec::with_capacity(frame_count);
    let mut cursor = 1usize;
    for _ in 0..frame_count {
        if cursor >= data.len() {
            break;
        }
        let ctl_byte = data[cursor];
        cursor += 1;

        if ctl_byte & 0b1000_0000 != 0 {
            // Raw-byte frame: a short, attacker-chosen slice fed
            // verbatim so the `BadSync` / short-frame rejections are
            // reached mid-stream (the sync-resync skip path in
            // `decode_all_frames`).
            let want = (ctl_byte as usize & 0x3f).min(data.len().saturating_sub(cursor));
            crafted.push(data[cursor..cursor + want].to_vec());
            cursor += want;
        } else if data.len() - cursor >= HEADER_CTL_BYTES {
            let ctl: [u8; HEADER_CTL_BYTES] =
                data[cursor..cursor + HEADER_CTL_BYTES].try_into().unwrap();
            cursor += HEADER_CTL_BYTES;
            // Draw the body from the remaining attacker bytes; advance
            // the cursor by a stride so successive frames draw fresh
            // body bytes.
            let body_start = cursor.min(data.len());
            match craft_frame(&ctl, &data[body_start..]) {
                Some(frame) => crafted.push(frame),
                None => crafted.push(data[body_start..].to_vec()),
            }
            cursor = (cursor + 16).min(data.len());
        } else {
            // Not enough to craft a header; feed the tail raw.
            crafted.push(data[cursor..].to_vec());
            cursor = data.len();
        }
    }

    // 1. Streaming chain over the concatenated multi-frame buffer.
    let mut stream: Vec<u8> = Vec::new();
    for frame in &crafted {
        stream.extend_from_slice(frame);
    }
    let _ = decode_all_frames(&stream);

    // 2. Trait-object decode session over the crafted frames.
    let Some(mut dec) = build_decoder() else {
        return;
    };
    let tb = TimeBase(Rational::new(1, 44_100));
    for (i, frame) in crafted.iter().enumerate() {
        let pkt = Packet::new(0, tb, frame.clone());
        // send_packet may legitimately Err on a truncated tail or an
        // unsupported header — that's the contract. Output frames are
        // drained immediately to keep `pending_frames` bounded.
        if dec.send_packet(&pkt).is_ok() {
            let _ = dec.receive_frame();
        }
        if i == flush_at {
            let _ = dec.flush();
            for _ in 0..4 {
                if dec.receive_frame().is_err() {
                    break;
                }
            }
        }
        if i == reset_at {
            let _ = dec.reset();
        }
    }

    // Final flush + drain. A forced double-flush exercises the
    // idempotent post-flush `Eof` path.
    let _ = dec.flush();
    let _ = dec.flush();
    for _ in 0..4 {
        if dec.receive_frame().is_err() {
            break;
        }
    }
});
