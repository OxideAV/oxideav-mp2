//! §2.4.1.8 `ancillary_data()` — decode-side surface.
//!
//! The audio_data() syntax closes with the §2.4.1.8 ancillary bit-loop
//! whose length §2.4.2.8 fixes as the frame-byte budget minus the
//! header / error-check / audio-data spend. The encoder has carried a
//! payload path (`encode_frame_with_ancillary`) since the frame
//! assembler landed; these tests cover the matching decoder surface:
//! `DecodedFrame::ancillary` captures the raw tail (sub-byte residue +
//! whole bytes) so the §2.4.1.8 round trip closes, and Annex B Table
//! B.5's protected-field boundary (the tail is *not* CRC-covered) is
//! pinned.

use oxideav_mp2::encoder_frame::{
    encode_all_frames_with_ancillary, encode_frame, encode_frame_auto_with_ancillary,
    encode_frame_with_ancillary, EncodeError, EncodeFrameState,
};
use oxideav_mp2::frame::{decode_frame, decode_frame_with, FrameDecodeState};
use oxideav_mp2::header::{Emphasis, FrameHeader, Mode, ModeExtension};
use oxideav_mp2::{PaddingScheduler, SmrTable, NUM_SUBBANDS, PCM_SAMPLES_PER_CHANNEL};

fn header(protection_bit: bool) -> FrameHeader {
    FrameHeader {
        lsf: false,
        bit_rate: 192_000,
        sample_rate: 48_000,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
        protection_bit,
    }
}

/// One frame of a stereo two-tone signal.
fn source_pcm() -> Vec<Vec<f64>> {
    let fs = 48_000.0_f64;
    let mut left = Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL);
    let mut right = Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL);
    for i in 0..PCM_SAMPLES_PER_CHANNEL {
        let t = i as f64 / fs;
        left.push(0.4 * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin());
        right.push(0.4 * (2.0 * std::f64::consts::PI * 3_000.0 * t).sin());
    }
    vec![left, right]
}

fn flat_smr() -> SmrTable {
    [[10.0; NUM_SUBBANDS]; 2]
}

/// The §2.4.2.8 length identity every decoded tail must satisfy: the
/// frame end is byte-aligned, so
/// `bits == residue_bits + 8 · bytes.len()`.
fn assert_tail_invariant(anc: &oxideav_mp2::frame::Ancillary) {
    assert_eq!(
        anc.bits,
        usize::from(anc.residue_bits) + 8 * anc.bytes.len(),
        "§2.4.2.8 tail bit-count identity violated"
    );
    assert!(anc.residue_bits < 8, "residue must be sub-byte");
    if anc.residue_bits > 0 {
        assert_eq!(
            anc.residue >> anc.residue_bits,
            0,
            "residue value must fit its bit count"
        );
    } else {
        assert_eq!(anc.residue, 0);
    }
}

#[test]
fn encoder_payload_round_trips_byte_aligned() {
    // The encoder byte-aligns the §2.4.3.3.4 sample region with zero
    // bits and copies the payload at the first whole tail byte
    // (§2.4.1.8 syntax is a bit-loop, but the §2.4.2.1 frame is
    // byte-granular). On decode the tail therefore opens with an
    // all-zero residue and the payload sits at `bytes[0..len]`,
    // zero-padded to the frame end.
    let payload: &[u8] = b"\x4F\x78\x41\x56\x00\xFF\x5A\xA5 ancillary";
    let banc = (payload.len() * 8) as u32 + 64;
    let frame =
        encode_frame_with_ancillary(&header(true), &source_pcm(), &flat_smr(), banc, payload)
            .expect("encode with ancillary payload");

    let decoded = decode_frame(&frame).expect("decode");
    let anc = &decoded.ancillary;
    assert_tail_invariant(anc);
    assert_eq!(anc.residue, 0, "alignment residue bits must be zero");
    assert!(
        anc.bytes.starts_with(payload),
        "decoded tail must open with the encoder's payload"
    );
    assert!(
        anc.bytes[payload.len()..].iter().all(|&b| b == 0),
        "tail beyond the payload is the §2.4.2.1 zero-fill"
    );
    assert!(anc.bits >= payload.len() * 8);
    assert!(!anc.is_all_zero());
}

#[test]
fn empty_ancillary_decodes_as_all_zero_tail() {
    let frame = encode_frame(&header(true), &source_pcm(), &flat_smr(), 0).expect("encode");
    let decoded = decode_frame(&frame).expect("decode");
    assert_tail_invariant(&decoded.ancillary);
    assert!(
        decoded.ancillary.is_all_zero(),
        "an empty-ancillary encode zero-fills the whole §2.4.1.8 tail"
    );
}

#[test]
fn ancillary_tail_is_not_crc_protected() {
    // Annex B Table B.5 protects the header second half plus the
    // §2.4.1.6 allocation + scfsi section — NOT the §2.4.1.8 tail.
    // Rewriting tail bytes of a CRC-protected frame must therefore
    // decode cleanly, with bit-identical PCM and the rewrite visible
    // in the surfaced ancillary. (The dual of
    // `emphasis_bits_are_inside_the_layer2_crc_protected_field` in
    // `tests/deemphasis.rs`.)
    let payload: &[u8] = b"\x11\x22\x33\x44";
    let banc = (payload.len() * 8) as u32 + 64;
    let frame = encode_frame_with_ancillary(
        &header(false), // CRC word present
        &source_pcm(),
        &flat_smr(),
        banc,
        payload,
    )
    .expect("encode CRC frame with ancillary");

    let reference = decode_frame(&frame).expect("decode untampered");
    assert!(reference.ancillary.bytes.starts_with(payload));

    let mut tampered = frame.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF; // deep inside the zero-fill tail
    let decoded = decode_frame(&tampered).expect("tail rewrite must not trip the CRC");
    assert_eq!(decoded.pcm, reference.pcm, "PCM unaffected by tail bytes");
    assert_tail_invariant(&decoded.ancillary);
    assert_eq!(
        decoded.ancillary.bytes.last().copied(),
        Some(0xFF),
        "the rewrite is visible in the surfaced tail"
    );
    assert!(!decoded.ancillary.is_all_zero());
}

#[test]
fn batch_encode_carries_per_frame_payloads_through_padding() {
    // The batch §2.4.1.8 path at a fractional rate (44,1 kHz — the
    // §2.4.2.3 padding schedule interleaves 627-slot frames): each
    // frame carries its own payload, the output is byte-identical to a
    // hand-rolled `encode_frame_auto_with_ancillary` loop threading
    // the same `PaddingScheduler` + `EncodeFrameState`, and every
    // payload comes back from the frame-level decode.
    let hdr = FrameHeader {
        sample_rate: 44_100,
        ..header(true)
    };
    let n_frames = 4usize;
    let fs = 44_100.0_f64;
    let mut left = Vec::with_capacity(n_frames * PCM_SAMPLES_PER_CHANNEL);
    let mut right = Vec::with_capacity(n_frames * PCM_SAMPLES_PER_CHANNEL);
    for i in 0..n_frames * PCM_SAMPLES_PER_CHANNEL {
        let t = i as f64 / fs;
        left.push(0.4 * (2.0 * std::f64::consts::PI * 900.0 * t).sin());
        right.push(0.4 * (2.0 * std::f64::consts::PI * 2_500.0 * t).sin());
    }
    let pcm = vec![left, right];

    let payloads: [&[u8]; 4] = [b"frame-0", b"", b"\x00\xFF\x7E", b"the fourth payload"];
    let banc = 8 * payloads.iter().map(|p| p.len()).max().unwrap() as u32 + 64;

    let batch =
        encode_all_frames_with_ancillary(&hdr, &pcm, banc, &payloads).expect("batch encode");

    // Hand-rolled equivalence.
    let mut sched = PaddingScheduler::new();
    let mut state = EncodeFrameState::new();
    let mut rolled = Vec::new();
    for (f, payload) in payloads.iter().enumerate() {
        let fh = sched.next_header(&hdr);
        let frame_pcm: Vec<Vec<f64>> = pcm
            .iter()
            .map(|p| p[f * PCM_SAMPLES_PER_CHANNEL..(f + 1) * PCM_SAMPLES_PER_CHANNEL].to_vec())
            .collect();
        rolled.extend_from_slice(
            &encode_frame_auto_with_ancillary(&fh, &frame_pcm, banc, payload, &mut state)
                .expect("hand-rolled frame"),
        );
    }
    assert_eq!(batch, rolled, "batch != hand-rolled loop");

    // Walk the frames and recover each payload from the surfaced tail.
    let mut dec_state = FrameDecodeState::new();
    let mut pos = 0usize;
    let mut padded_seen = false;
    for payload in payloads {
        let fh = FrameHeader::parse(&batch[pos..]).expect("batch frame header");
        padded_seen |= fh.padding;
        let size = fh.frame_size_bytes();
        let decoded = decode_frame_with(&batch[pos..pos + size], &mut dec_state).expect("decode");
        assert_tail_invariant(&decoded.ancillary);
        assert_eq!(decoded.ancillary.residue, 0);
        assert!(
            decoded.ancillary.bytes.starts_with(payload),
            "payload not recovered from frame at {pos}"
        );
        assert!(decoded.ancillary.bytes[payload.len()..]
            .iter()
            .all(|&b| b == 0));
        pos += size;
    }
    assert_eq!(pos, batch.len(), "walked the whole stream");
    assert!(
        padded_seen,
        "44,1 kHz schedule should pad at least one of 4 frames"
    );

    // A mis-sized payload list is refused, not realigned.
    let short: [&[u8]; 2] = [b"a", b"b"];
    match encode_all_frames_with_ancillary(&hdr, &pcm, banc, &short) {
        Err(EncodeError::AncillaryFrameCountMismatch { have: 2, need: 4 }) => {}
        other => panic!("expected AncillaryFrameCountMismatch, got {other:?}"),
    }
}

#[test]
fn staged_fixture_tails_satisfy_the_length_identity() {
    // Walk every frame of the staged black-box-encoded fixture and
    // check the §2.4.2.8 identity on real third-party frames (whose
    // encoders spend the budget differently from ours). Skips when
    // `docs/` isn't checked out.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/input.mp3"
    );
    if !std::path::Path::new(path).exists() {
        eprintln!("skip: staged Layer II fixture absent at {path}");
        return;
    }
    let stream = std::fs::read(path).expect("read fixture");
    let mut state = FrameDecodeState::new();
    let mut pos = 0usize;
    let mut frames = 0usize;
    while pos + 4 <= stream.len() {
        let h = FrameHeader::parse(&stream[pos..]).expect("fixture frame header");
        let size = h.frame_size_bytes();
        if pos + size > stream.len() {
            break;
        }
        let decoded =
            decode_frame_with(&stream[pos..pos + size], &mut state).expect("fixture frame");
        assert_tail_invariant(&decoded.ancillary);
        pos += size;
        frames += 1;
    }
    assert_eq!(frames, 31, "fixture is 31 frames");
}
