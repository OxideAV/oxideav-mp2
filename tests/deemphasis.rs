//! End-to-end §2.4.2.4 de-emphasis: a stream whose header signals the
//! 50/15 µs curve decodes to PCM with the de-emphasis IIR applied,
//! whereas an otherwise-identical `emphasis == '00'` stream does not.
//!
//! The `emphasis` field is a header flag that does not influence bit
//! allocation or sample coding, so two streams encoded from the same
//! PCM — one flagged `None`, one flagged `FiftyFifteen` — carry
//! byte-identical audio-data sections. Their decodes therefore differ
//! *only* by the decoder-applied de-emphasis, which this test pins
//! exactly by running the reference [`DeEmphasis`] filter over the
//! `None` decode and comparing.

use oxideav_mp2::deemphasis::DeEmphasis;
use oxideav_mp2::encoder_frame::encode_all_frames;
use oxideav_mp2::frame::decode_all_frames;
use oxideav_mp2::header::{Emphasis, FrameHeader, Mode, ModeExtension};

fn header(emphasis: Emphasis) -> FrameHeader {
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
        emphasis,
        // protection_bit == true → no CRC slot; keeps the two streams
        // differing only in the header emphasis field.
        protection_bit: true,
    }
}

/// Three frames of a two-tone signal (a low tone plus strong
/// high-frequency content the de-emphasis shelf will attenuate).
fn source_pcm() -> Vec<Vec<f64>> {
    let n = 3 * 1152;
    let fs = 48_000.0_f64;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fs;
        let lo = 0.3 * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin();
        let hi = 0.3 * (2.0 * std::f64::consts::PI * 15_000.0 * t).sin();
        left.push(lo + hi);
        right.push(lo - hi);
    }
    vec![left, right]
}

#[test]
fn fifty_fifteen_stream_decodes_with_deemphasis_applied() {
    let pcm = source_pcm();

    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    let emph = encode_all_frames(&header(Emphasis::FiftyFifteen), &pcm, 0).expect("encode emph");

    // The audio-data payload must be identical (only the header byte
    // carrying the emphasis field differs).
    assert_eq!(plain.len(), emph.len(), "frame sizing must match");

    let plain_pcm = decode_all_frames(&plain).expect("decode plain");
    let emph_pcm = decode_all_frames(&emph).expect("decode emph");

    assert_eq!(plain_pcm.len(), 2);
    assert_eq!(emph_pcm.len(), 2);

    for ch in 0..2 {
        assert_eq!(plain_pcm[ch].len(), emph_pcm[ch].len());
        // The emphasis decode must equal the plain decode passed
        // through the reference de-emphasis filter, sample for sample.
        let mut filt = DeEmphasis::fifty_fifteen(48_000);
        let mut differed = false;
        for (i, &p) in plain_pcm[ch].iter().enumerate() {
            let expected = filt.process_sample(p);
            let got = emph_pcm[ch][i];
            assert!(
                (expected - got).abs() < 1e-9,
                "ch{ch}[{i}]: deemphasis mismatch expected={expected} got={got}"
            );
            if (p - got).abs() > 1e-3 {
                differed = true;
            }
        }
        // Sanity: the de-emphasis actually changed the signal (the
        // 15 kHz component is strongly attenuated), so the two decodes
        // are not trivially equal.
        assert!(differed, "ch{ch}: de-emphasis produced no audible change");
    }
}

#[test]
fn ccitt_j17_stream_decodes_unfiltered() {
    // J.17 is a documented docs gap: the decoder delivers PCM
    // unfiltered, identical to `Emphasis::None`.
    let pcm = source_pcm();
    let none = decode_all_frames(&encode_all_frames(&header(Emphasis::None), &pcm, 0).unwrap())
        .expect("decode none");
    let j17 = decode_all_frames(&encode_all_frames(&header(Emphasis::CcittJ17), &pcm, 0).unwrap())
        .expect("decode j17");
    for ch in 0..2 {
        for (a, b) in none[ch].iter().zip(&j17[ch]) {
            assert!((a - b).abs() < 1e-12, "j17 must be unfiltered like none");
        }
    }
}
