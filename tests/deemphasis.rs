//! End-to-end §2.4.2.4 emphasis coverage:
//!
//! * `fifty_fifteen_stream_decodes_with_deemphasis_applied` and
//!   `ccitt_j17_stream_decodes_with_deemphasis_applied` isolate the
//!   *decoder's* de-emphasis by hand-patching the header emphasis field
//!   of an unaltered (`None`) stream, so the two decodes differ only by
//!   the decoder-applied filter — pinned exactly against the reference
//!   [`DeEmphasis`].
//! * `preemphasis_encode_deemphasis_decode_recovers_tone` and
//!   `j17_preemphasis_encode_deemphasis_decode_recovers_tone` exercise
//!   the full acoustic loop: the encoder pre-emphasises when the header
//!   signals the curve, the decoder de-emphasises, and the original
//!   spectrum is recovered.

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
    // Isolate the *decoder's* de-emphasis: encode with `None`, then
    // hand-patch every frame header's 2-bit emphasis field to `'01'`
    // (50/15 µs). At 48 kHz / 192 kbit/s each frame is a constant
    // 144·192000/48000 = 576 bytes with no padding, and with the CRC
    // suppressed (`protection_bit == true`) the emphasis bits — the two
    // LSBs of header byte 3 — can be flipped without touching the
    // audio-data payload. The two decodes then differ *only* by the
    // decoder-applied de-emphasis.
    let pcm = source_pcm();

    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    const FRAME_SIZE: usize = 576;
    assert_eq!(
        plain.len() % FRAME_SIZE,
        0,
        "expected constant 576-byte frames"
    );
    let mut emph = plain.clone();
    for f in (0..emph.len()).step_by(FRAME_SIZE) {
        // emphasis == word & 0x3 → the two LSBs of header byte 3.
        emph[f + 3] = (emph[f + 3] & 0xFC) | 0x01;
    }

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
fn ccitt_j17_stream_decodes_with_deemphasis_applied() {
    // Same isolation as the 50/15 µs test above, but hand-patching the
    // emphasis field to `'11'` (CCITT J.17): the decode must equal the
    // plain decode passed through the reference J.17 de-emphasis
    // cascade, sample for sample.
    let pcm = source_pcm();

    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    const FRAME_SIZE: usize = 576;
    assert_eq!(plain.len() % FRAME_SIZE, 0);
    let mut emph = plain.clone();
    for f in (0..emph.len()).step_by(FRAME_SIZE) {
        emph[f + 3] = (emph[f + 3] & 0xFC) | 0x03;
    }

    let plain_pcm = decode_all_frames(&plain).expect("decode plain");
    let emph_pcm = decode_all_frames(&emph).expect("decode emph");

    for ch in 0..2 {
        let mut filt = DeEmphasis::ccitt_j17(48_000);
        let mut differed = false;
        for (i, &p) in plain_pcm[ch].iter().enumerate() {
            let expected = filt.process_sample(p);
            let got = emph_pcm[ch][i];
            assert!(
                (expected - got).abs() < 1e-9,
                "ch{ch}[{i}]: J.17 deemphasis mismatch expected={expected} got={got}"
            );
            if (p - got).abs() > 1e-3 {
                differed = true;
            }
        }
        // Sanity: the 15 kHz component sits deep in the J.17 shelf
        // (≈ −18 dB), so the two decodes are not trivially equal.
        assert!(differed, "ch{ch}: J.17 de-emphasis produced no change");
    }
}

/// Naive single-bin energy at frequency `f` (Hz) over `x` sampled at
/// `fs`, skipping the filterbank group-delay preamble.
fn bin_energy(x: &[f64], fs: f64, f: f64, skip: usize) -> f64 {
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &s) in x.iter().enumerate().skip(skip) {
        let w = 2.0 * std::f64::consts::PI * f * (n as f64) / fs;
        re += s * w.cos();
        im += s * w.sin();
    }
    re * re + im * im
}

#[test]
fn preemphasis_encode_deemphasis_decode_recovers_tone() {
    // Encoding with the 50/15 µs curve pre-emphasises the PCM before
    // quantization; the decoder de-emphasises it. The round-trip must
    // reproduce the *original* spectral content (both the 1 kHz and the
    // 15 kHz component), not the pre-emphasised one.
    let pcm = source_pcm();
    let fs = 48_000.0;

    let stream = encode_all_frames(&header(Emphasis::FiftyFifteen), &pcm, 0).expect("encode");
    let out = decode_all_frames(&stream).expect("decode");
    assert_eq!(out.len(), 2);

    // Skip the combined analysis+synthesis filterbank group delay.
    let skip = 512;
    for (ch, chan) in out.iter().enumerate() {
        let e_lo = bin_energy(chan, fs, 1_000.0, skip);
        let e_hi = bin_energy(chan, fs, 15_000.0, skip);
        let e_probe = bin_energy(chan, fs, 7_000.0, skip);
        // Both source tones dominate an unrelated probe bin, proving the
        // right spectrum is reproduced rather than broadband noise.
        assert!(
            e_lo > 50.0 * e_probe,
            "ch{ch}: 1 kHz tone not recovered (lo={e_lo}, probe={e_probe})"
        );
        assert!(
            e_hi > 50.0 * e_probe,
            "ch{ch}: 15 kHz tone not recovered after de-emphasis (hi={e_hi}, probe={e_probe})"
        );
    }

    // The pre-emphasis genuinely altered the encoded audio data (the two
    // streams differ beyond the single header emphasis byte per frame).
    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    assert_eq!(plain.len(), stream.len());
    let differing_bytes = plain.iter().zip(&stream).filter(|(a, b)| a != b).count();
    assert!(
        differing_bytes > 3,
        "pre-emphasis changed only {differing_bytes} bytes — audio data not re-encoded"
    );
}

#[test]
fn j17_preemphasis_encode_deemphasis_decode_recovers_tone() {
    // Same acoustic loop with the CCITT J.17 curve: the encoder
    // pre-emphasises (boosting the 15 kHz tone by ≈ 18.4 dB — an 8.4×
    // amplitude factor, much stronger than the 50/15 µs shelf), the
    // decoder's J.17 de-emphasis undoes it, and the original two-tone
    // spectrum comes back. The high tone uses a smaller amplitude than
    // `source_pcm()` so the pre-emphasised peak stays inside the
    // Layer II scalefactor range.
    let n = 3 * 1152;
    let fs = 48_000.0_f64;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fs;
        let lo = 0.3 * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin();
        let hi = 0.1 * (2.0 * std::f64::consts::PI * 15_000.0 * t).sin();
        left.push(lo + hi);
        right.push(lo - hi);
    }
    let pcm = vec![left, right];

    let stream = encode_all_frames(&header(Emphasis::CcittJ17), &pcm, 0).expect("encode");
    let out = decode_all_frames(&stream).expect("decode");
    assert_eq!(out.len(), 2);

    let skip = 512;
    for (ch, chan) in out.iter().enumerate() {
        let e_lo = bin_energy(chan, fs, 1_000.0, skip);
        let e_hi = bin_energy(chan, fs, 15_000.0, skip);
        let e_probe = bin_energy(chan, fs, 7_000.0, skip);
        assert!(
            e_lo > 50.0 * e_probe,
            "ch{ch}: 1 kHz tone not recovered (lo={e_lo}, probe={e_probe})"
        );
        assert!(
            e_hi > 50.0 * e_probe,
            "ch{ch}: 15 kHz tone not recovered after J.17 de-emphasis (hi={e_hi}, probe={e_probe})"
        );
    }

    // The J.17 pre-emphasis genuinely altered the encoded audio data.
    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    assert_eq!(plain.len(), stream.len());
    let differing_bytes = plain.iter().zip(&stream).filter(|(a, b)| a != b).count();
    assert!(
        differing_bytes > 3,
        "J.17 pre-emphasis changed only {differing_bytes} bytes"
    );
}
