//! Round 185 — ISO/IEC 13818-3 §2.4.2.3 / §2.4.3.1 LSF Layer II
//! integration coverage.
//!
//! The §2.4.2.3 LSF (low sampling-rate) extension reuses the
//! 11172-3 Layer II frame syntax but swaps the bit-allocation table
//! and the bitrate / sampling-frequency mappings. This file pins
//! the end-to-end behaviour from `FrameHeader::parse` through
//! `select_table` and `decode_frame` for an LSF frame.
//!
//! Clean-room basis: every assertion derives from the staged
//! `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf` (§2.4.2.3
//! PDF page 21 for the bitrate/sample-rate tables, §2.4.3.1 PDF
//! page 22 for the table-B.1-replaces-B.2 rule, Annex B Table B.1
//! PDF page 71 for the per-subband nb_steps table, Annex C.1.2 PDF
//! page 75 for the LSF frame-size formula `N = bitrate × 144 / Fs`).
//! No third-party MP2 implementation source was consulted.

use oxideav_mp2::{
    bitalloc::BitAllocTable, decode_frame, select_table, FrameError, FrameHeader, NUM_SUBBANDS,
};

/// Build a 4-byte LSF Layer II header with the ID bit cleared per
/// ISO/IEC 13818-3 §2.4.2.3.
#[allow(clippy::too_many_arguments)]
fn build_lsf_header(
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
    // §2.4.1.3 layout shared with 11172-3, with ID = '0' for LSF:
    //   sync(12) | id(1)=0 | layer(2)='10' | protection(1) |
    //   bitrate(4) | sf(2) | pad(1) | priv(1) | mode(2) | mode_ext(2)
    //   | cr(1) | orig(1) | emph(2)
    // ID bit (bit 19) is left implicitly zero — the LSF (`ID == 0`)
    // selector per ISO/IEC 13818-3 §2.4.2.3 — by not OR-ing anything
    // into bit 19 of the word.
    let word: u32 = (0xFFF << 20)
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

#[test]
fn lsf_header_parse_decodes_every_combination_of_bitrate_and_sampling_rate() {
    // ISO/IEC 13818-3 PDF page 21, Layer II / Layer III LSF column.
    let lsf_bitrates_kbps = [
        (0b0001u32, 8u32),
        (0b0010, 16),
        (0b0011, 24),
        (0b0100, 32),
        (0b0101, 40),
        (0b0110, 48),
        (0b0111, 56),
        (0b1000, 64),
        (0b1001, 80),
        (0b1010, 96),
        (0b1011, 112),
        (0b1100, 128),
        (0b1101, 144),
        (0b1110, 160),
    ];
    let lsf_sample_rates = [(0b00u32, 22_050u32), (0b01, 24_000), (0b10, 16_000)];

    for &(br_code, kbps) in &lsf_bitrates_kbps {
        for &(sf_code, sr) in &lsf_sample_rates {
            // Stereo, mode_ext=0, no padding, no private, no copyright,
            // original=1, emph=0, protection=1 (no CRC).
            let bytes = build_lsf_header(br_code, sf_code, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
            let h = FrameHeader::parse(&bytes).expect("LSF header should parse");
            assert!(h.lsf, "LSF bit not set for br={br_code} sf={sf_code}");
            assert_eq!(h.bit_rate, kbps * 1000);
            assert_eq!(h.sample_rate, sr);
            assert_eq!(h.samples_per_channel(), 1152, "LSF Layer II is 1152 spf");
            // ISO/IEC 13818-3 Annex C.1.2 PDF page 75:
            //   N = floor(bitrate * 144 / Fs)
            // ("In Layer II, a slot consists of 8 bits.") No padding
            // bit on this build_lsf_header invocation.
            let expected_n = ((kbps * 1000) as u64 * 144 / sr as u64) as usize;
            assert_eq!(
                h.frame_size_bytes(),
                expected_n,
                "frame_size at br={kbps} kbps Fs={sr} Hz"
            );
            // Every LSF header routes to Table B.1 of 13818-3
            // per §2.4.3.1.
            assert_eq!(select_table(&h), Some(BitAllocTable::B1Lsf));
        }
    }
}

#[test]
fn lsf_header_emit_round_trips_through_parse_for_every_cell() {
    // 14 LSF bitrates × 3 LSF sample rates × 4 modes = 168
    // (bitrate, mode) pairs; ISO/IEC 13818-3 §2.4.2.3 does not
    // restate the 11172-3 "not all combinations are allowed"
    // matrix, so every cell must round-trip.
    let kbps_codes = [
        (0b0001u32, 8u32),
        (0b0010, 16),
        (0b0011, 24),
        (0b0100, 32),
        (0b0101, 40),
        (0b0110, 48),
        (0b0111, 56),
        (0b1000, 64),
        (0b1001, 80),
        (0b1010, 96),
        (0b1011, 112),
        (0b1100, 128),
        (0b1101, 144),
        (0b1110, 160),
    ];
    let sf_codes = [(0b00u32, 22_050u32), (0b01, 24_000), (0b10, 16_000)];
    // mode bits: 00=stereo, 01=joint stereo, 10=dual channel,
    // 11=single channel.
    let mode_codes = [0b00u32, 0b01, 0b10, 0b11];

    let mut covered = 0usize;
    for &(br, _) in &kbps_codes {
        for &(sf, _) in &sf_codes {
            for &mode_code in &mode_codes {
                let bytes = build_lsf_header(br, sf, 1, 1, mode_code, 0b01, 1, 0, 0b01, 0);
                let h = FrameHeader::parse(&bytes).expect("LSF parse");
                let emitted = h.emit_bytes().expect("LSF emit");
                assert_eq!(emitted, bytes);
                let h2 = FrameHeader::parse(&emitted).unwrap();
                assert_eq!(h, h2);
                covered += 1;
            }
        }
    }
    assert_eq!(covered, 14 * 3 * 4);
}

#[test]
fn lsf_header_padding_bit_adds_exactly_one_byte_to_the_frame_size() {
    // Padding semantics are inherited from 11172-3 §2.4.2.3 per
    // 13818-3 §2.4.2.3 "padding_bit - See ISO/IEC 11172-3, 2.4.2.3.
    // Padding is necessary with a sampling frequency of 22,05 kHz".
    for sf in [(0b00u32, 22_050u32), (0b01, 24_000), (0b10, 16_000)] {
        let (sf_code, _) = sf;
        let without = build_lsf_header(0b1010, sf_code, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        let with = build_lsf_header(0b1010, sf_code, 1, 0, 0b00, 0b00, 0, 1, 0b00, 1);
        let h_without = FrameHeader::parse(&without).unwrap();
        let h_with = FrameHeader::parse(&with).unwrap();
        assert!(h_without.lsf);
        assert!(!h_without.padding);
        assert!(h_with.padding);
        assert_eq!(
            h_with.frame_size_bytes(),
            h_without.frame_size_bytes() + 1,
            "padding should add exactly one byte at Fs={}",
            sf.1
        );
    }
}

#[test]
fn lsf_b1_sblimit_drives_audio_data_subband_iteration() {
    // The Table B.1 sblimit = 30 means subbands [0, 30) carry
    // allocation fields; subbands 30 and 31 carry none. Pin this
    // via select_table + sblimit().
    let h = FrameHeader::parse(&build_lsf_header(
        0b1000, 0b00, 0, 0, 0b00, 0b00, 0, 1, 0b00, 1,
    ))
    .unwrap();
    let table = select_table(&h).expect("LSF header must route to B1Lsf");
    assert_eq!(table, BitAllocTable::B1Lsf);
    assert_eq!(table.sblimit(), 30);
    // Subbands 30..32 are silent in Table B.1 — the LSF nyquist
    // limit at 12 kHz (Fs=24, half band) maps the last two of the
    // 32 polyphase subbands to zero coverage.
    assert_eq!(table.nbal(30), 0);
    assert_eq!(table.nbal(31), 0);
    // Every sb < sblimit must have a non-zero nbal width.
    for sb in 0..table.sblimit() {
        assert!(table.nbal(sb) > 0, "LSF sb={sb} should have nbal > 0");
    }
    // NUM_SUBBANDS is 32 — Table B.1 doesn't extend the subband
    // count, it just covers fewer of them.
    assert_eq!(NUM_SUBBANDS, 32);
}

#[test]
fn lsf_decode_frame_rejects_all_zero_payload_with_truncation_rather_than_panic() {
    // A bare 4-byte LSF header followed by 0 bytes of audio data is
    // truncated — decode_frame must return FrameError::Truncated,
    // not panic. The header itself is well-formed.
    let header_bytes = build_lsf_header(0b1000, 0b00, 0, 0, 0b11, 0b00, 0, 1, 0b00, 1);
    // 64 kbit/s LSF at 22.05 kHz: N = floor(64000 * 144 / 22050) = 417 bytes.
    let h = FrameHeader::parse(&header_bytes).unwrap();
    assert_eq!(h.frame_size_bytes(), 417);
    // Provide only the 4-byte header → 413-byte payload missing.
    match decode_frame(&header_bytes) {
        Err(FrameError::Truncated { have, need }) => {
            assert_eq!(have, 4);
            assert_eq!(need, 417);
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
}

#[test]
fn lsf_decode_frame_with_all_zero_allocation_succeeds_and_yields_silence() {
    // Construct an LSF Layer II frame whose payload is all zeros.
    // The §2.4.1.6 / §2.4.3.3 bit-allocation field is read first
    // (sum_of_nbal = 75 bits per channel for Table B.1 LSF); with
    // all bits zero, every subband reads the `-` sentinel
    // (nb_steps = 0), no scfsi / scalefactors / samples are
    // transmitted (§2.4.2.3 "no bits allocated for this subband"),
    // and the output is bit-exact silence (zeros).
    //
    // 64 kbit/s LSF + 22.05 kHz: N = 417 bytes. Single_channel mode
    // (mode_bits = 0b11) keeps the allocation field at 75 bits
    // (one channel × 75 bits = 75 bits → 10 bytes once the
    // zero-allocation rolls all subbands).
    let header_bytes = build_lsf_header(0b1000, 0b00, 0, 0, 0b11, 0b00, 0, 1, 0b00, 1);
    let h = FrameHeader::parse(&header_bytes).unwrap();
    let frame_size = h.frame_size_bytes();
    assert_eq!(frame_size, 417);

    let mut frame = vec![0u8; frame_size];
    frame[..4].copy_from_slice(&header_bytes);
    // Payload is already all-zero (the allocation field reads as
    // the `-` sentinel for every subband per Table B.1).

    let decoded = decode_frame(&frame).expect("zero-allocation LSF frame should decode");
    assert_eq!(decoded.pcm.len(), 1, "single_channel = 1 channel");
    assert_eq!(decoded.pcm[0].len(), 1152, "LSF Layer II = 1152 spf");
    // Every sample must be exactly zero — no scalefactors, no
    // sample bits transmitted → the requantizer never runs.
    for (i, &s) in decoded.pcm[0].iter().enumerate() {
        assert_eq!(
            s, 0.0,
            "LSF zero-allocation sample[{i}] should be exact zero, got {s}"
        );
    }
}
