//! §2.4.2.3 free-format (`bitrate_index == '0000'`) decode coverage.
//!
//! A free-format Layer II stream signals no bitrate in the header; the
//! decoder recovers the constant frame size by measuring the distance
//! between consecutive syncwords. The Annex B bit-allocation table for a
//! free-format frame is fixed by the **sampling frequency alone** — the
//! Table 3-B.2a header lists free format at 48 kHz and the Table 3-B.2b
//! header lists it at 44,1 / 32 kHz (PDF pages 46–47) — so the identity
//! `decode(free-format rewrite) == decode(original)` holds exactly when
//! the original signalled rate selects that same table (per-channel
//! ≥ 56 kbit/s at 48 kHz, ≥ 96 kbit/s at 44,1 / 32 kHz, any LSF rate).
//! These tests synthesise such streams by taking the crate's own
//! standard-bitrate encoder output and rewriting only the
//! `bitrate_index` field to `'0000'`, leaving the frame size, payload,
//! and all other header fields untouched.

use oxideav_mp2::encoder_frame::encode_frame;
use oxideav_mp2::{
    decode_all_frames, decode_free_format_stream, measure_base_slots, resolve, to_free_format,
    Emphasis, FrameHeader, FreeFormatError, Mode, ModeExtension, SmrTable, NUM_SUBBANDS,
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
    // 192 kbit/s stereo at 44.1 kHz → 626-byte unpadded base frame;
    // 96 kbit/s per channel selects Table B.2b, the same table the
    // free-format read uses at 44.1 kHz — so decode identity holds.
    let (standard, free) = build_streams(192_000, 44_100, Mode::Stereo, 4);

    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = decode_free_format_stream(&free).expect("free-format decode");

    assert_eq!(ff_pcm.len(), std_pcm.len(), "channel count matches");
    for ch in 0..std_pcm.len() {
        assert_eq!(
            ff_pcm[ch].len(),
            std_pcm[ch].len(),
            "ch {ch} sample count matches"
        );
        // The allocation tables coincide and the payloads are
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
    // 96 kbit/s single_channel at 32 kHz → floor(144*96000/32000) = 432;
    // 96 kbit/s per channel selects Table B.2b, matching the free-format
    // table at 32 kHz.
    let (standard, free) = build_streams(96_000, 32_000, Mode::SingleChannel, 3);
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
fn off_ladder_free_format_stream_decodes() {
    // §2.4.2.3: free format carries "a fixed bitrate which does not need
    // to be in the list". Build a genuinely decodable off-ladder stream
    // by growing every frame of a table-coinciding free-format stream by
    // one trailing ancillary byte: 626 → 627 slots at 44.1 kHz sizes to
    // no ladder rate (192 kbit/s → 626, 224 kbit/s → 731), yet the
    // §2.4.1.8 ancillary tail is simply never read, so the PCM must stay
    // bit-identical to the 626-slot decode.
    let (standard, free) = build_streams(192_000, 44_100, Mode::Stereo, 4);
    let mut off_ladder = Vec::with_capacity(free.len() + 4);
    for frame in free.chunks_exact(626) {
        off_ladder.extend_from_slice(frame);
        off_ladder.push(0x00); // extra ancillary slot
    }

    let base = measure_base_slots(&off_ladder).expect("measure off-ladder base");
    assert_eq!(base, 627, "grown frame size recovered");
    let layout = resolve(&off_ladder).expect("off-ladder stream resolves");
    // Nominal metadata rate: ⌈627 · 44100 / 144⌉ = 192 019 bit/s.
    assert_eq!(layout.bit_rate, 192_019);

    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = decode_free_format_stream(&off_ladder).expect("off-ladder decode");
    assert_eq!(ff_pcm.len(), std_pcm.len());
    for ch in 0..std_pcm.len() {
        assert_eq!(ff_pcm[ch], std_pcm[ch], "ch {ch}: ancillary tail ignored");
    }
}

#[test]
fn free_format_size_above_384k_ceiling_is_rejected() {
    // §2.4.2.3: "The decoder is also not required to support bitrates
    // higher than … 384 kbits/s in respect to Layer … II … when in free
    // format mode." Two syncwords 1300 bytes apart at 44.1 kHz imply
    // ⌈1300 · 44100 / 144⌉ = 398 125 bit/s > 384 000 → refused before
    // any payload decode.
    let mut buf: Vec<u8> = Vec::new();
    // free-format header: 44.1k (sf='00'), stereo, unpadded, no CRC.
    // bitrate_index '0000' and sampling_frequency '00' are all-zero, so
    // they contribute nothing to the OR-assembled header word.
    let word: u32 = (0xFFFu32 << 20) | (1 << 19) | (0b10 << 17) | (1 << 16);
    buf.extend_from_slice(&word.to_be_bytes());
    buf.resize(1300, 0x5A);
    buf.extend_from_slice(&word.to_be_bytes());
    buf.resize(2600, 0x5A);
    buf.extend_from_slice(&word.to_be_bytes());

    match decode_free_format_stream(&buf) {
        Err(oxideav_mp2::FrameError::FreeFormat(FreeFormatError::UnsupportedBitrate {
            base_slots,
            sample_rate,
        })) => {
            assert_eq!(base_slots, 1300);
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
    let (_standard, mut free) = build_streams(192_000, 44_100, Mode::Stereo, 2);
    // The base frames here are unpadded (626 each, Table B.2b — the
    // free-format table at 44.1 kHz). Inject a padding slot into the
    // first frame: set its padding_bit and insert one byte.
    // padding_bit is bit 9 of the 32-bit header = bit 1 of byte 2.
    free[2] |= 0x02; // set padding_bit on frame 0's header
    free.insert(626, 0x00); // grow frame 0 by one slot
                            // Frame 0 is now 627 bytes (padded), frame 1 is 626 (unpadded).
    let base = measure_base_slots(&free).expect("measure padded first frame");
    assert_eq!(base, 626, "padding removed from base recovery");
    // And the whole stream still decodes without panicking.
    let pcm = decode_free_format_stream(&free).expect("decode padded free-format stream");
    assert_eq!(pcm.len(), 2);
    // Two frames → 2 × 1152 samples per channel.
    assert_eq!(pcm[0].len(), 2 * PCM_SAMPLES_PER_CHANNEL);
}

#[test]
fn free_format_encode_path_round_trips_via_to_free_format() {
    // §2.4.2.3 free-format ENCODE: take the standard encoder's output and
    // convert it to a free-format stream with `to_free_format`, then decode
    // it back with `decode_free_format_stream`. The result must match the
    // standard decode bit-for-bit (the payload is untouched; only the
    // bitrate_index nibble is cleared).
    let (standard, _free_via_manual) = build_streams(192_000, 44_100, Mode::Stereo, 5);
    // 192 kbit/s stereo at 44.1 kHz → floor(144*192000/44100) = 626 base.
    let frame_size = 626usize;
    let free = to_free_format(&standard, frame_size);

    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = decode_free_format_stream(&free).expect("free-format decode");

    assert_eq!(ff_pcm.len(), std_pcm.len());
    for ch in 0..std_pcm.len() {
        assert_eq!(ff_pcm[ch], std_pcm[ch], "ch {ch} encode-path round-trip");
    }
    // Confirm the produced stream is genuinely free format.
    let h = FrameHeader::parse_allow_free_format(&free).unwrap();
    assert!(
        h.is_free_format(),
        "to_free_format produced a free-format frame"
    );
}

#[test]
fn free_format_rewrite_of_real_48k_fixture_decodes_identically() {
    // The committed `stereo_48k_192` conformance fixture (192 kbit/s
    // stereo at 48 kHz = 96 kbit/s per channel → Table B.2a, exactly the
    // free-format table at 48 kHz) rewritten to free format must decode
    // bit-identically to the signalled stream. An independent black-box
    // reference decoder agrees byte-for-byte on this equivalence (its
    // decode of the rewritten stream is identical to its decode of the
    // signalled one — see `fixtures/GENERATION.md`), so this test ties
    // the crate's free-format read to independently-validated behaviour
    // on a real encoded stream, not just to our own encoder's output.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/stereo_48k_192.mp2"
    );
    let Ok(standard) = std::fs::read(path) else {
        eprintln!("skip: fixture absent at {path}");
        return;
    };
    let header = FrameHeader::parse(&standard).expect("fixture header");
    assert_eq!(header.sample_rate, 48_000);
    assert_eq!(header.bit_rate, 192_000);
    let frame_size = header.frame_size_bytes(); // 576, unpadded at 48 kHz
    assert_eq!(standard.len() % frame_size, 0, "constant-size frames");

    let free = to_free_format(&standard, frame_size);
    let std_pcm = decode_all_frames(&standard).expect("signalled decode");
    let ff_pcm = decode_free_format_stream(&free).expect("free-format decode");
    assert_eq!(ff_pcm.len(), std_pcm.len());
    for ch in 0..std_pcm.len() {
        assert_eq!(ff_pcm[ch], std_pcm[ch], "ch {ch}: table B.2a coincidence");
    }
}

#[test]
fn free_format_table_is_fixed_by_sampling_frequency() {
    // Table 3-B.2a header (PDF p. 46): free format listed at 48 kHz;
    // Table 3-B.2b header (PDF p. 47): free format listed at 44.1 and
    // 32 kHz; B.2c/B.2d carry no free-format row.
    use oxideav_mp2::{select_table, BitAllocTable};
    let mk = |sample_rate: u32| FrameHeader {
        lsf: false,
        protection_bit: true,
        bit_rate: 0, // free-format sentinel
        sample_rate,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    };
    assert_eq!(select_table(&mk(48_000)), Some(BitAllocTable::B2a));
    assert_eq!(select_table(&mk(44_100)), Some(BitAllocTable::B2b));
    assert_eq!(select_table(&mk(32_000)), Some(BitAllocTable::B2b));
    let mut lsf = mk(24_000);
    lsf.lsf = true;
    assert_eq!(select_table(&lsf), Some(BitAllocTable::B1Lsf));
}
