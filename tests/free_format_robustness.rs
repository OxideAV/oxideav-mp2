//! Panic-freedom robustness for the §2.4.2.3 free-format decode surface.
//!
//! The free-format path measures frame sizes from sync-to-sync distances
//! in attacker-influenced data, so it must never panic, never index out of
//! bounds, and never integer-overflow (debug builds) on arbitrary input —
//! it must always return a `Result`. These tests drive the free-format
//! entry points (`measure_base_slots`, `resolve`, `decode_free_format_stream`)
//! with adversarial byte patterns: dense sync runs, truncated buffers,
//! every single-byte prefix of a synthesized free-format frame, and a
//! deterministic pseudo-random corpus.

use oxideav_mp2::{decode_free_format_stream, measure_base_slots, resolve, FrameHeader};

/// A 4-byte free-format header: 44.1 kHz, stereo, unpadded, no CRC.
/// bitrate_index '0000' and sampling_frequency '00' are all-zero.
fn ff_header() -> [u8; 4] {
    let word: u32 = (0xFFFu32 << 20) | (1 << 19) | (0b10 << 17) | (1 << 16);
    word.to_be_bytes()
}

#[test]
fn measure_and_resolve_never_panic_on_dense_sync_runs() {
    // A buffer that is almost entirely the 0xFF 0xFx sync pattern is the
    // worst case for the sync-lock scanner: every other position looks
    // like a frame boundary. It must terminate with a Result, not loop or
    // panic.
    for fill in [0xFFu8, 0xF0, 0xFE, 0x00, 0x5A] {
        let mut buf = ff_header().to_vec();
        buf.resize(4096, fill);
        // Whatever it decides, it must not panic.
        let _ = measure_base_slots(&buf);
        let _ = resolve(&buf);
        let _ = decode_free_format_stream(&buf);
    }
}

#[test]
fn every_prefix_of_a_free_format_frame_is_safe() {
    // Build a plausible 2-frame free-format buffer (417-byte frames), then
    // feed every single-byte prefix of it to all three entry points.
    let mut buf = ff_header().to_vec();
    buf.resize(417, 0xAB);
    buf.extend_from_slice(&ff_header());
    buf.resize(417 + 417, 0xCD);

    for len in 0..=buf.len() {
        let prefix = &buf[..len];
        let _ = measure_base_slots(prefix);
        let _ = resolve(prefix);
        let _ = decode_free_format_stream(prefix);
    }
}

#[test]
fn pseudo_random_corpus_never_panics() {
    // xorshift64* deterministic PRNG — reproducible adversarial corpus.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };

    for _ in 0..2000 {
        let len = (next() % 1200) as usize;
        let mut buf = vec![0u8; len];
        for b in buf.iter_mut() {
            *b = (next() & 0xFF) as u8;
        }
        // Half the time, prime the buffer with a real free-format header so
        // the parser reaches deeper into the size-measurement logic.
        if len >= 4 && (next() & 1) == 0 {
            buf[..4].copy_from_slice(&ff_header());
        }
        let _ = measure_base_slots(&buf);
        let _ = resolve(&buf);
        let _ = decode_free_format_stream(&buf);
        // The free-format-tolerant header parser must also never panic.
        let _ = FrameHeader::parse_allow_free_format(&buf);
    }
}

#[test]
fn empty_and_tiny_buffers_are_safe() {
    for len in 0..6usize {
        let buf = vec![0xFFu8; len];
        let _ = measure_base_slots(&buf);
        let _ = resolve(&buf);
        let _ = decode_free_format_stream(&buf);
        let _ = FrameHeader::parse_allow_free_format(&buf);
    }
}

#[test]
fn decode_free_format_stream_on_non_free_format_input_is_graceful() {
    // A buffer whose first frame is a normal (non-free-format) frame:
    // decode_free_format_stream resolves against it and either decodes or
    // returns an error — never panics. Construct a standard 128 kbit/s
    // header (bitrate_index '1000').
    let word: u32 = (0xFFFu32 << 20) | (1 << 19) | (0b10 << 17) | (1 << 16) | (0b1000 << 12);
    let mut buf = word.to_be_bytes().to_vec();
    buf.resize(417, 0x11);
    buf.extend_from_slice(&word.to_be_bytes());
    buf.resize(417 * 2, 0x22);
    // resolve() requires a free-format header; on a standard header it must
    // return NotFreeFormat, and decode_free_format_stream surfaces that.
    let _ = resolve(&buf);
    let _ = decode_free_format_stream(&buf);
}
