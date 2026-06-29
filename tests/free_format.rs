//! §2.4.2.3 free-format (`bitrate_index == '0000'`) decode coverage.
//!
//! A free-format Layer II stream signals no bitrate in the header; the
//! decoder recovers the constant frame size by measuring the distance
//! between consecutive syncwords and recovers the bitrate by matching that
//! size against the §2.4.2.3 ladder. These tests synthesise a free-format
//! stream by taking the crate's own standard-bitrate encoder output and
//! rewriting only the `bitrate_index` field to `'0000'`, leaving the
//! frame size, payload, and all other header fields untouched. Decoding
//! that stream through `decode_free_format_stream` must reproduce exactly
//! the same PCM as decoding the original standard-bitrate stream through
//! `decode_all_frames` — because the recovered bitrate equals the original
//! and the payloads are byte-identical.

use oxideav_mp2::encoder_frame::encode_frame;
use oxideav_mp2::{
    decode_all_frames, decode_free_format_stream, measure_base_slots, resolve, Emphasis,
    FrameHeader, FreeFormatError, Mode, ModeExtension, SmrTable, NUM_SUBBANDS,
    PCM_SAMPLES_PER_CHANNEL,
};

/// Rewrite a 4-byte Layer II header's `bitrate_index` field (bits 15..12,
/// counting from MSB=31) to `'0000'` (free format), leaving every other
/// field intact.
fn make_header_free_format(frame: &mut [u8]) {
    // bitrate_index occupies bits 12..15 of the 32-bit big-endian header,
    // i.e. the high nibble of byte 2.
    frame[2] &= 0x0F;
}

/// Encode a multi-frame standard-bitrate stream, then return both the
/// original bytes and a free-format-rewritten copy.
fn build_streams(
    bit_rate: u32,
    sample_rate: u32,
    mode: Mode,
    n_frames: usize,
) -> (Vec<u8>, Vec<u8>) {
    let channels = mode.channels();
    let header = FrameHeader {
        lsf: false,
        protection_bit: true, // no CRC (simpler payload-identity argument)
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    };
    let smr: SmrTable = [[20.0f64; NUM_SUBBANDS]; 2];

    let mut standard = Vec::new();
    for f in 0..n_frames {
        // A mild per-frame-varying tone so frames differ.
        let freq = 400.0 + 50.0 * f as f64;
        let pcm: Vec<Vec<f64>> = (0..channels)
            .map(|_| {
                (0..PCM_SAMPLES_PER_CHANNEL)
                    .map(|n| {
                        0.4 * (2.0 * std::f64::consts::PI * freq * n as f64 / sample_rate as f64)
                            .sin()
                    })
                    .collect()
            })
            .collect();
        let frame = encode_frame(&header, &pcm, &smr, 0).expect("encode standard frame");
        standard.extend_from_slice(&frame);
    }

    // Build the free-format copy: same bytes, but each frame's
    // bitrate_index nibble cleared to '0000'.
    let frame_size = header.frame_size_bytes();
    let mut free = standard.clone();
    let mut off = 0;
    while off + frame_size <= free.len() {
        make_header_free_format(&mut free[off..off + frame_size]);
        off += frame_size;
    }
    (standard, free)
}

#[test]
fn free_format_stream_decodes_identically_to_standard() {
    // 128 kbit/s stereo at 44.1 kHz → 417-byte unpadded base frame.
    let (standard, free) = build_streams(128_000, 44_100, Mode::Stereo, 4);

    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = decode_free_format_stream(&free).expect("free-format decode");

    assert_eq!(ff_pcm.len(), std_pcm.len(), "channel count matches");
    for ch in 0..std_pcm.len() {
        assert_eq!(
            ff_pcm[ch].len(),
            std_pcm[ch].len(),
            "ch {ch} sample count matches"
        );
        // The recovered bitrate equals the original and the payloads are
        // byte-identical, so the reconstructed PCM must be bit-exact equal.
        for (n, (a, b)) in ff_pcm[ch].iter().zip(std_pcm[ch].iter()).enumerate() {
            assert_eq!(a, b, "ch {ch} sample {n} diverged");
        }
    }
}

#[test]
fn free_format_resolve_recovers_the_right_bitrate() {
    let (_standard, free) = build_streams(192_000, 48_000, Mode::Stereo, 3);
    // 192 kbit/s at 48 kHz → floor(144*192000/48000) = 576-byte base.
    let layout = resolve(&free).expect("resolve free-format layout");
    assert_eq!(layout.base_slots, 576);
    assert_eq!(layout.bit_rate, 192_000);
    assert_eq!(layout.frame_size, 576, "unpadded frame");
}

#[test]
fn free_format_works_for_single_channel() {
    // 64 kbit/s single_channel at 32 kHz → floor(144*64000/32000) = 288.
    let (standard, free) = build_streams(64_000, 32_000, Mode::SingleChannel, 3);
    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = decode_free_format_stream(&free).expect("free-format decode");
    assert_eq!(ff_pcm.len(), 1, "mono");
    assert_eq!(std_pcm.len(), 1);
    assert_eq!(ff_pcm[0], std_pcm[0]);
}

#[test]
fn free_format_lsf_rate_decodes_identically() {
    // MPEG-2 LSF: 64 kbit/s stereo at 24 kHz.
    let channels = 2usize;
    let header = FrameHeader {
        lsf: true,
        protection_bit: true,
        bit_rate: 64_000,
        sample_rate: 24_000,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    };
    let smr: SmrTable = [[0.0f64; NUM_SUBBANDS]; 2];
    let mut standard = Vec::new();
    for f in 0..3 {
        let freq = 300.0 + 40.0 * f as f64;
        let pcm: Vec<Vec<f64>> = (0..channels)
            .map(|_| {
                (0..PCM_SAMPLES_PER_CHANNEL)
                    .map(|n| 0.3 * (2.0 * std::f64::consts::PI * freq * n as f64 / 24_000.0).sin())
                    .collect()
            })
            .collect();
        standard.extend_from_slice(&encode_frame(&header, &pcm, &smr, 0).expect("encode lsf"));
    }
    let frame_size = header.frame_size_bytes();
    let mut free = standard.clone();
    let mut off = 0;
    while off + frame_size <= free.len() {
        free[off + 2] &= 0x0F; // clear bitrate_index nibble
        off += frame_size;
    }
    let std_pcm = decode_all_frames(&standard).expect("lsf standard decode");
    let ff_pcm = decode_free_format_stream(&free).expect("lsf free-format decode");
    assert_eq!(ff_pcm.len(), 2);
    for ch in 0..2 {
        assert_eq!(ff_pcm[ch], std_pcm[ch], "lsf ch {ch}");
    }
}

#[test]
fn off_ladder_free_format_size_is_rejected_not_guessed() {
    // Construct a free-format stream whose measured base size matches no
    // ladder bitrate: two syncwords 500 bytes apart at 44.1 kHz.
    let mut buf: Vec<u8> = Vec::new();
    // free-format header: 44.1k (sf='00'), stereo, unpadded, no CRC.
    // bitrate_index '0000' and sampling_frequency '00' are all-zero, so
    // they contribute nothing to the OR-assembled header word.
    let word: u32 = (0xFFFu32 << 20) | (1 << 19) | (0b10 << 17) | (1 << 16);
    buf.extend_from_slice(&word.to_be_bytes());
    buf.resize(500, 0x5A);
    buf.extend_from_slice(&word.to_be_bytes());

    match decode_free_format_stream(&buf) {
        Err(oxideav_mp2::FrameError::FreeFormat(FreeFormatError::UnsupportedBitrate {
            base_slots,
            sample_rate,
        })) => {
            assert_eq!(base_slots, 500);
            assert_eq!(sample_rate, 44_100);
        }
        other => panic!("expected UnsupportedBitrate, got {other:?}"),
    }
}

#[test]
fn padded_free_format_frames_are_sized_per_padding_bit() {
    // Build a 2-frame free-format stream where the first frame is padded
    // (frame_size = base + 1) and the second is not. The measurement must
    // recover the same base N from the padded first frame.
    let (_standard, mut free) = build_streams(128_000, 44_100, Mode::Stereo, 2);
    // The base frames here are unpadded (417 each). Inject a padding slot
    // into the first frame: set its padding_bit and insert one byte.
    // padding_bit is bit 9 of the 32-bit header = bit 1 of byte 2.
    free[2] |= 0x02; // set padding_bit on frame 0's header
    free.insert(417, 0x00); // grow frame 0 by one slot
                            // Frame 0 is now 418 bytes (padded), frame 1 is 417 (unpadded).
    let base = measure_base_slots(&free).expect("measure padded first frame");
    assert_eq!(base, 417, "padding removed from base recovery");
    // And the whole stream still decodes without panicking.
    let pcm = decode_free_format_stream(&free).expect("decode padded free-format stream");
    assert_eq!(pcm.len(), 2);
    // Two frames → 2 × 1152 samples per channel.
    assert_eq!(pcm[0].len(), 2 * PCM_SAMPLES_PER_CHANNEL);
}
