//! Round 162 — malformed-input property tests for the §2.4.1.3 / §2.4.2.3
//! Layer II frame header parser, the §2.4.3.1 frame-size / truncation
//! checks, and the §2.4.1.6 frame-level decode loop.
//!
//! Clean-room basis: every assertion here cross-checks behaviour against
//! the §2.4.2.3 validation contract already documented in
//! `crate::header::HeaderError` and the §2.4.3.1 truncation contract
//! already documented in `crate::frame::FrameError::Truncated`. No
//! third-party MP2 implementation source was consulted; the tests
//! exercise the in-crate public API only.
//!
//! ## Suites
//!
//! 1. **Bit-flip exhaustion (32 bits).** Walk every bit position of the
//!    canonical 192 kbit/s / 44.1 kHz / Stereo / no-CRC header and
//!    confirm that `FrameHeader::parse` either accepts the mutated
//!    word with a structurally-valid result (semantic-only bits like
//!    `private_bit`, `copyright`, `original`, and the unconstrained
//!    `mode_extension` / `padding`) or rejects it with one of the
//!    §2.4.2.3 / §2.4.1.3 documented `HeaderError` variants. No
//!    parse path may panic or produce a header whose `channels()` is
//!    zero or whose `frame_size_bytes()` is below the 4-byte header.
//!
//! 2. **Prefix-truncation exhaustion.** For every prefix length
//!    `0..frame_size_bytes()` of a constructed Layer II frame, both
//!    `decode_frame` and `FrameHeader::parse` must return a documented
//!    error rather than panicking. The decoder must report a
//!    `Truncated { have, need }` whose `need >= 4` and whose
//!    `have == prefix_len`.
//!
//! 3. **Sync-search robustness.** `find_sync` over an arbitrary
//!    non-syncword buffer must return `None`; over a buffer with a
//!    sync at a known offset it must return that offset; and
//!    `FrameHeader::parse` of a non-syncword head must yield
//!    `HeaderError::BadSync` without panicking — exhaustively
//!    across the 256 possible second-byte values whose top nibble
//!    differs from `0xF`.

use oxideav_mp2::{
    decode_frame, find_sync, AudioDataError, Emphasis, FrameError, FrameHeader, HeaderError, Mode,
    ModeExtension,
};

/// Build a 4-byte Layer II header from explicit field values.
/// Mirrors the test helper in `src/header.rs` but in the integration
/// test world (which only sees the public API).
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
    // §2.4.1.3 layout: sync(12) | id(1)=1 | layer(2)='10' |
    // protection(1) | bitrate(4) | sf(2) | pad(1) | priv(1) |
    // mode(2) | mode_ext(2) | cr(1) | orig(1) | emph(2)
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

/// Canonical reference header: 192 kbit/s / 44.1 kHz / Stereo / no-CRC.
/// `FrameHeader::parse` returns the same struct on this input that the
/// `src/header.rs` `parses_a_canonical_192kbps_44100_stereo_no_crc_header`
/// test pins; we reuse it as the "anchor" for the bit-flip suite below.
fn canonical_192k_stereo_header() -> [u8; 4] {
    build_header(0b1010, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1)
}

/// Build a complete Layer II frame whose entire payload after the
/// header is `payload`. The header is the canonical 192k/44.1k/Stereo
/// /no-CRC header (frame_size_bytes = 626 — §2.4.3.1
/// `floor(144 * 192000 / 44100) = 626`).
///
/// `payload` must be `frame_size_bytes - 4` bytes long. When the
/// `protection_bit` is `'1'` (no CRC) the §2.4.1.6 audio-data section
/// starts immediately after byte 3.
fn synthesize_complete_frame(payload_pattern: u8) -> Vec<u8> {
    let header = canonical_192k_stereo_header();
    let parsed = FrameHeader::parse(&header).expect("canonical header parses");
    let fs = parsed.frame_size_bytes();
    let mut frame = vec![payload_pattern; fs];
    frame[..4].copy_from_slice(&header);
    frame
}

// ---------------------------------------------------------------------
// Suite 1: 32-bit bit-flip exhaustion of the canonical header.
// ---------------------------------------------------------------------

/// Every single-bit flip of the canonical 4-byte header must produce
/// either (a) `Ok(_)` with a structurally-valid `FrameHeader` whose
/// `channels() ∈ {1, 2}` and whose `frame_size_bytes() >= 4`, or (b)
/// one of the documented `HeaderError` variants. The parser is not
/// permitted to panic, to overrun the 4-byte input, or to produce a
/// header that the rest of the decoder cannot consume.
#[test]
fn header_bit_flips_never_panic_or_violate_postconditions() {
    let baseline = canonical_192k_stereo_header();
    assert!(FrameHeader::parse(&baseline).is_ok(), "baseline must parse");

    for bit in 0..32u8 {
        let byte_idx = (bit / 8) as usize;
        let mask = 1u8 << (7 - (bit % 8));
        let mut mutated = baseline;
        mutated[byte_idx] ^= mask;

        match FrameHeader::parse(&mutated) {
            Ok(h) => {
                assert!(
                    h.channels() == 1 || h.channels() == 2,
                    "bit={bit}: header parsed but channels()={} outside {{1,2}}",
                    h.channels()
                );
                assert!(
                    h.frame_size_bytes() >= 4,
                    "bit={bit}: frame_size_bytes()={} < 4",
                    h.frame_size_bytes()
                );
                assert_eq!(h.samples_per_channel(), 1152);
            }
            Err(err) => {
                // Every error must be one of the documented variants;
                // the match below is exhaustive (no wildcard) so a new
                // variant added without updating this test fails to
                // compile.
                match err {
                    HeaderError::BufferTooShort
                    | HeaderError::BadSync
                    | HeaderError::UnsupportedLayer(_)
                    | HeaderError::ForbiddenBitrate
                    | HeaderError::FreeFormat
                    | HeaderError::ReservedSamplingFrequency
                    | HeaderError::ReservedEmphasis
                    | HeaderError::DisallowedBitrateModeCombination { .. }
                    | HeaderError::UnsupportedBitrate(_)
                    | HeaderError::UnsupportedSamplingFrequency(_) => {}
                }
            }
        }
    }
}

/// Some bit positions are §2.4.1.3 hard-coded fields whose flip MUST
/// always produce a specific error or a specific successful re-decode.
/// Pin them by position:
///
/// * bits 31..20 — syncword (12 bits): any single flip breaks the
///   §2.4.2.3 `'1111 1111 1111'` requirement → `BadSync`.
/// * bit 19 — `ID`: flipping `'1'` → `'0'` switches the header from
///   ISO/IEC 11172-3 MPEG-1 to the ISO/IEC 13818-3 §2.4.2.3 LSF
///   extension. The flipped baseline still parses successfully; the
///   `lsf` field flips and the bitrate / sample-rate fields decode
///   against the LSF ladder rather than the MPEG-1 ladder.
/// * bits 18..17 — `layer`: flipping either bit moves us off `'10'`
///   → `UnsupportedLayer(_)`.
#[test]
fn header_bit_flips_in_fixed_fields_produce_specific_errors() {
    let baseline = canonical_192k_stereo_header();

    // Bits 31..20: syncword. Word is big-endian; bit n counted from
    // the MSB of byte 0.
    for bit in 0..12u8 {
        let mut mutated = baseline;
        let byte_idx = (bit / 8) as usize;
        let mask = 1u8 << (7 - (bit % 8));
        mutated[byte_idx] ^= mask;
        assert_eq!(
            FrameHeader::parse(&mutated),
            Err(HeaderError::BadSync),
            "syncword bit {bit} flip did not yield BadSync"
        );
    }

    // Bit 19: ID. Word position bit 19 = (32-1)-19 = 12 bits from MSB
    // → byte 1 bit (7 - (12 % 8)) = byte 1, mask 1<<3 = 0x08. The
    // canonical baseline has bitrate_index='1010' (= 192 kbit/s in
    // MPEG-1, = 96 kbit/s in LSF) and sf_index='00' (= 44.1 kHz in
    // MPEG-1, = 22.05 kHz in LSF); flipping the ID bit re-decodes
    // both fields against the LSF ladder per ISO/IEC 13818-3 §2.4.2.3.
    let baseline_parsed = FrameHeader::parse(&baseline).unwrap();
    assert!(!baseline_parsed.lsf, "baseline is MPEG-1");
    let mut mutated = baseline;
    mutated[1] ^= 0x08;
    let lsf_parsed = FrameHeader::parse(&mutated).unwrap();
    assert!(lsf_parsed.lsf, "ID bit flip should select LSF");
    assert_eq!(lsf_parsed.bit_rate, 96_000, "LSF bitrate_index='1010' = 96");
    assert_eq!(lsf_parsed.sample_rate, 22_050, "LSF sf_index='00' = 22.05");

    // Bits 18..17: layer.
    //   bit 18 → byte 1 bit (7 - (13 % 8)) = bit 2 → mask 0x04
    //   bit 17 → byte 1 bit (7 - (14 % 8)) = bit 1 → mask 0x02
    for mask in [0x04u8, 0x02u8] {
        let mut mutated = baseline;
        mutated[1] ^= mask;
        match FrameHeader::parse(&mutated) {
            Err(HeaderError::UnsupportedLayer(_)) => {}
            other => panic!("layer bit mask 0x{mask:02X} flip → {other:?}, want UnsupportedLayer"),
        }
    }
}

/// `protection_bit` is bit 16 of the header word. Flipping it does
/// not invalidate the §2.4.2.3 header — it just toggles whether the
/// decoder expects a CRC slot. The flipped header must still parse
/// successfully, and the `protection_bit` field must flip relative
/// to the baseline.
#[test]
fn protection_bit_flip_just_toggles_the_field() {
    let baseline = canonical_192k_stereo_header();
    let parsed_baseline = FrameHeader::parse(&baseline).unwrap();
    assert!(
        parsed_baseline.protection_bit,
        "baseline is the unprotected (wire '1') form"
    );

    let mut mutated = baseline;
    mutated[1] ^= 0x01; // bit 16 = bit 0 of byte 1
    let parsed_mutated = FrameHeader::parse(&mutated).unwrap();
    assert!(
        !parsed_mutated.protection_bit,
        "after flip, wire bit should be '0' (CRC present)"
    );
    // Every other field is unchanged.
    assert_eq!(parsed_baseline.bit_rate, parsed_mutated.bit_rate);
    assert_eq!(parsed_baseline.sample_rate, parsed_mutated.sample_rate);
    assert_eq!(parsed_baseline.mode, parsed_mutated.mode);
    assert_eq!(parsed_baseline.emphasis, parsed_mutated.emphasis);
    assert_eq!(
        parsed_baseline.mode_extension,
        parsed_mutated.mode_extension
    );
}

// ---------------------------------------------------------------------
// Suite 2: prefix-truncation exhaustion.
// ---------------------------------------------------------------------

/// `FrameHeader::parse` on every prefix `0..4` must return
/// `BufferTooShort` rather than panicking. Above 4 bytes the
/// canonical header always parses, so 0..4 is the meaningful range
/// for the header path.
#[test]
fn header_parse_returns_buffer_too_short_for_every_short_prefix() {
    let header = canonical_192k_stereo_header();
    for prefix_len in 0..4 {
        let prefix = &header[..prefix_len];
        assert_eq!(
            FrameHeader::parse(prefix),
            Err(HeaderError::BufferTooShort),
            "prefix length {prefix_len} should be BufferTooShort"
        );
    }
    // The full 4 bytes parse.
    assert!(FrameHeader::parse(&header).is_ok());
}

/// `decode_frame` on every prefix `0..frame_size_bytes` of a complete
/// frame must return either `FrameError::Header(BufferTooShort)`
/// (prefix < 4) or `FrameError::Truncated { have: prefix_len, need:
/// frame_size_bytes }` (4 <= prefix < frame_size_bytes), and must
/// never panic. The frame body is filled with `0xFF` so that, were
/// the truncation guard accidentally skipped, the §2.4.1.6 bit
/// reader would still find data to consume — i.e. the truncation
/// check is exercised against a buffer that "looks like" it could
/// continue past the end.
#[test]
fn decode_frame_truncation_is_exhaustive_and_never_panics() {
    let frame = synthesize_complete_frame(0xFF);
    let fs = frame.len();
    assert_eq!(fs, 626, "canonical 192k/44.1k frame is 626 bytes");

    // 4..fs-1 should report Truncated with the right shape.
    for prefix_len in 4..fs {
        let prefix = &frame[..prefix_len];
        match decode_frame(prefix) {
            Err(FrameError::Truncated { have, need }) => {
                assert_eq!(have, prefix_len, "prefix={prefix_len}");
                assert!(
                    need >= 4,
                    "prefix={prefix_len}: need={need} is below minimum 4-byte header"
                );
                // `need` should equal `fs` (or, when protection_bit
                // turns out to be 0 and the 6-byte minimum trips
                // first, exactly 6). Our canonical baseline has
                // protection_bit == '1' so the only `need` is `fs`.
                assert_eq!(
                    need, fs,
                    "prefix={prefix_len}: need {need} != frame_size {fs}"
                );
            }
            other => panic!("prefix={prefix_len}: expected Truncated, got {other:?}"),
        }
    }

    // 0..3 should bubble up the BufferTooShort header error.
    for prefix_len in 0..4 {
        let prefix = &frame[..prefix_len];
        match decode_frame(prefix) {
            Err(FrameError::Header(HeaderError::BufferTooShort)) => {}
            other => panic!("prefix={prefix_len}: expected Header(BufferTooShort), got {other:?}"),
        }
    }
}

/// A `protection_bit == 0` frame whose tail is truncated at byte 5
/// (i.e. only one of the two CRC slot bytes is present) must report
/// the §2.4.3.1 6-byte minimum rather than overflowing the slice.
#[test]
fn decode_frame_truncation_at_crc_slot_boundary() {
    // Build a CRC-protected variant of the canonical header by
    // flipping bit 16 (protection_bit) to 0. The first 4 bytes are
    // a valid header; we then append exactly one CRC byte so the
    // total length is 5 — short of the 6 required to read the full
    // 16-bit CRC slot.
    let mut header = canonical_192k_stereo_header();
    header[1] &= !0x01;
    let buf = [header[0], header[1], header[2], header[3], 0xAA];
    match decode_frame(&buf) {
        Err(FrameError::Truncated { have, need }) => {
            assert_eq!(have, 5);
            assert!(need >= 6, "need={need} must be >= 6 for CRC slot");
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Suite 3: sync-search and BadSync robustness.
// ---------------------------------------------------------------------

/// `find_sync` over a buffer with no 12-bit syncword must return
/// `None` and not panic. The 0..256 second-byte sweep skips the
/// `0xF_` range (where the top nibble would qualify as part of a
/// syncword); for every other value the function must report no
/// sync.
#[test]
fn find_sync_is_none_when_no_syncword_present() {
    // Empty / single-byte / pair-of-non-sync bytes.
    assert_eq!(find_sync(&[]), None);
    assert_eq!(find_sync(&[0xFF]), None);
    assert_eq!(find_sync(&[0xFE, 0xFF]), None);

    // Sweep second-byte values 0x00..=0xEF (no `0xF_` top nibble).
    for b1 in 0u8..=0xEF {
        let buf = [0xFF, b1];
        assert!(
            find_sync(&buf).is_none(),
            "find_sync wrongly matched [0xFF, 0x{b1:02X}]"
        );
    }
    // Sweep first-byte values that are not 0xFF; the second-byte top
    // nibble being 0xF is irrelevant when the first byte isn't 0xFF.
    for b0 in 0u8..=0xFE {
        let buf = [b0, 0xFF];
        assert!(
            find_sync(&buf).is_none(),
            "find_sync wrongly matched [0x{b0:02X}, 0xFF]"
        );
    }
}

/// `find_sync` reports the leftmost byte-aligned 12-bit syncword in
/// a buffer; this nails down the contract by planting a syncword at
/// a known offset and asserting the returned position.
#[test]
fn find_sync_reports_leftmost_match() {
    let mut buf = vec![0u8; 17];
    buf[13] = 0xFF;
    buf[14] = 0xF0; // exactly the 12-bit sync; low nibble of byte 14 is don't-care
    assert_eq!(find_sync(&buf), Some(13));

    // Plant a second sync earlier; the function must prefer it.
    buf[5] = 0xFF;
    buf[6] = 0xFC;
    assert_eq!(find_sync(&buf), Some(5));
}

/// `FrameHeader::parse` on a non-syncword head must report
/// `BadSync` for every 0..256 second-byte value whose top nibble
/// isn't `0xF`. None of those iterations may panic.
#[test]
fn parse_reports_bad_sync_exhaustively_for_non_f_second_byte() {
    for b1 in 0u8..=0xEF {
        let buf = [0xFF, b1, 0xA0, 0x04];
        assert_eq!(
            FrameHeader::parse(&buf),
            Err(HeaderError::BadSync),
            "non-sync second-byte 0x{b1:02X} did not report BadSync"
        );
    }
}

// ---------------------------------------------------------------------
// Suite 4: derived-field bit-flip oracles.
// ---------------------------------------------------------------------

/// Flipping the high-order bit of the `bitrate_index` (bit 15) on
/// the canonical 192 kbit/s baseline (`0b1010`) drops the field to
/// `0b0010` = 48 kbit/s. 48 kbit/s with stereo mode is in the
/// §2.4.2.3 "not all combinations are allowed" matrix → expected
/// rejection.
#[test]
fn bitrate_high_bit_flip_triggers_layer2_mode_matrix_rejection() {
    let mut header = canonical_192k_stereo_header();
    // bit 15 → byte 1 LSB → mask 0x80 on byte 1. Wait: bit 15 from
    // the MSB of byte 0 → byte 1 bit (7 - (15 % 8)) = byte 1, mask
    // 1<<0 = 0x01. That's the protection_bit. Let's recompute.
    //
    // The bitrate_index occupies bits 15..12 in §2.4.1.3 indexing
    // ("bit 31 is MSB"). Big-endian byte 2 bits 7..4 are word bits
    // 15..12 respectively. The high-order bit of the bitrate field
    // is therefore byte 2 bit 7 → mask 0x80.
    header[2] ^= 0x80;
    let result = FrameHeader::parse(&header);
    match result {
        Err(HeaderError::DisallowedBitrateModeCombination { bit_rate, mode }) => {
            assert_eq!(bit_rate, 48_000);
            assert_eq!(mode, Mode::Stereo);
        }
        other => panic!("expected disallowed (48k, Stereo), got {other:?}"),
    }
}

/// Flipping bits 11..10 of the canonical header (sampling_frequency)
/// from `0b00` to `0b11` must report `ReservedSamplingFrequency`.
/// Bits 11 and 10 of the word correspond to byte 2 bits 3 and 2
/// respectively (mask 0x08 + 0x04 = 0x0C).
#[test]
fn sampling_frequency_to_reserved_value_is_rejected() {
    let mut header = canonical_192k_stereo_header();
    header[2] ^= 0x0C; // sf 00 → sf 11
    assert_eq!(
        FrameHeader::parse(&header),
        Err(HeaderError::ReservedSamplingFrequency),
    );
}

/// Flipping bit 1 of the emphasis field (word bit 1 = byte 3 bit 1)
/// on the canonical `'00'` emphasis flips it to `'10'`, the reserved
/// value.
#[test]
fn emphasis_to_reserved_value_is_rejected() {
    let mut header = canonical_192k_stereo_header();
    header[3] ^= 0x02; // emphasis 00 → 10
    assert_eq!(
        FrameHeader::parse(&header),
        Err(HeaderError::ReservedEmphasis),
    );
}

/// The `private_bit`, `copyright`, `original`, `padding`, and
/// `mode_extension` fields have no §2.4.2.3 reserved values for
/// Layer II Stereo at 192 kbit/s. Flipping any of those bits on the
/// canonical header must still produce a successful parse (with the
/// affected field reflected in the output).
#[test]
fn semantic_only_bit_flips_round_trip_through_parse() {
    let baseline = canonical_192k_stereo_header();
    let bp = FrameHeader::parse(&baseline).unwrap();

    // padding (word bit 9 → byte 2 bit 1 → mask 0x02)
    let mut h = baseline;
    h[2] ^= 0x02;
    let p = FrameHeader::parse(&h).unwrap();
    assert_eq!(p.padding, !bp.padding);
    // padding flip changes frame_size_bytes by exactly 1.
    let diff = (p.frame_size_bytes() as i64) - (bp.frame_size_bytes() as i64);
    assert_eq!(diff.abs(), 1, "padding flip should shift frame size by 1");

    // private_bit (word bit 8 → byte 2 bit 0 → mask 0x01)
    let mut h = baseline;
    h[2] ^= 0x01;
    let p = FrameHeader::parse(&h).unwrap();
    assert_eq!(p.private_bit, !bp.private_bit);

    // copyright (word bit 3 → byte 3 bit 3 → mask 0x08)
    let mut h = baseline;
    h[3] ^= 0x08;
    let p = FrameHeader::parse(&h).unwrap();
    assert_eq!(p.copyright, !bp.copyright);

    // original (word bit 2 → byte 3 bit 2 → mask 0x04)
    let mut h = baseline;
    h[3] ^= 0x04;
    let p = FrameHeader::parse(&h).unwrap();
    assert_eq!(p.original, !bp.original);

    // mode_extension low bit (word bit 4 → byte 3 bit 4 → mask 0x10).
    // The canonical header has mode_ext = '00' = Bound4; flipping
    // word bit 4 gives '01' = Bound8.
    let mut h = baseline;
    h[3] ^= 0x10;
    let p = FrameHeader::parse(&h).unwrap();
    assert_eq!(p.mode_extension, ModeExtension::Bound8);

    // Sanity: emphasis must remain `None` across all of the above.
    assert_eq!(bp.emphasis, Emphasis::None);
}

// ---------------------------------------------------------------------
// Suite 5: payload-truncation past the header is reported as
// `Truncated` (not `AudioData(UnexpectedEnd)`) because the §2.4.3.1
// frame-size check fires first.
// ---------------------------------------------------------------------

/// When the body is one byte short, the §2.4.3.1 frame-size check
/// trips before the §2.4.1.6 reader is invoked — i.e. the caller
/// gets `FrameError::Truncated`, NEVER `FrameError::AudioData(
/// UnexpectedEnd)`. This guards against a future regression where
/// the frame-size check is reordered after the bit reader.
#[test]
fn payload_one_byte_short_is_truncated_not_audio_data_underflow() {
    let frame = synthesize_complete_frame(0x00);
    let short = &frame[..frame.len() - 1];
    match decode_frame(short) {
        Err(FrameError::Truncated { have, need }) => {
            assert_eq!(have, frame.len() - 1);
            assert_eq!(need, frame.len());
        }
        Err(FrameError::AudioData(AudioDataError::UnexpectedEnd)) => {
            panic!(
                "regression: one-byte-short frame surfaced as AudioData(UnexpectedEnd); \
                 §2.4.3.1 frame-size check should fire first"
            );
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}
