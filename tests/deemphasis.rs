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
use oxideav_mp2::frame::{decode_all_frames, decode_frame, FrameError};
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

#[test]
fn emphasis_switching_mid_stream_rebuilds_the_filter_per_curve() {
    // A stream whose frames signal different emphasis values is legal —
    // every frame is decoded from its own header. The decoder carries
    // the per-channel IIR state across frames *while the curve is
    // unchanged*, rebuilds a fresh filter when the curve switches, and
    // drops the filter on `'00'`. Pin that exact semantics: patch a
    // 4-frame `None` stream to '01', '01', '11', '00' and reproduce the
    // decode with reference filters applied frame by frame.
    let n = 4 * 1152;
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
    let pcm = vec![left, right];

    let plain = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode plain");
    const FRAME_SIZE: usize = 576;
    assert_eq!(plain.len(), 4 * FRAME_SIZE);
    let per_frame_emphasis: [u8; 4] = [0b01, 0b01, 0b11, 0b00];
    let mut emph = plain.clone();
    for (frame, &bits) in per_frame_emphasis.iter().enumerate() {
        let f = frame * FRAME_SIZE;
        emph[f + 3] = (emph[f + 3] & 0xFC) | bits;
    }

    let plain_pcm = decode_all_frames(&plain).expect("decode plain");
    let emph_pcm = decode_all_frames(&emph).expect("decode emph");

    for ch in 0..2 {
        // Frames 0–1: one 50/15 µs filter with state carried across the
        // frame boundary (NOT re-created per frame).
        let mut fifty = DeEmphasis::fifty_fifteen(48_000);
        for i in 0..2 * 1152 {
            let expected = fifty.process_sample(plain_pcm[ch][i]);
            assert!(
                (expected - emph_pcm[ch][i]).abs() < 1e-9,
                "ch{ch}[{i}]: 50/15 span mismatch"
            );
        }
        // Frame 2: a FRESH J.17 filter (curve switch discards the 50/15
        // state).
        let mut j17 = DeEmphasis::ccitt_j17(48_000);
        for i in 2 * 1152..3 * 1152 {
            let expected = j17.process_sample(plain_pcm[ch][i]);
            assert!(
                (expected - emph_pcm[ch][i]).abs() < 1e-9,
                "ch{ch}[{i}]: J.17 span mismatch"
            );
        }
        // Frame 3: '00' — delivered unfiltered.
        for i in 3 * 1152..4 * 1152 {
            assert!(
                (plain_pcm[ch][i] - emph_pcm[ch][i]).abs() < 1e-12,
                "ch{ch}[{i}]: none span must be unfiltered"
            );
        }
    }
}

/// Rewrite every frame header's 2-bit emphasis field in a Layer II
/// stream, walking real frame boundaries (the per-frame padding bit
/// makes 44,1 kHz frame sizes vary, so a fixed stride is wrong).
fn patch_emphasis(stream: &[u8], bits: u8) -> Vec<u8> {
    let mut out = stream.to_vec();
    let mut pos = 0usize;
    while pos + 4 <= out.len() {
        let header = FrameHeader::parse(&out[pos..]).expect("frame header while patching");
        out[pos + 3] = (out[pos + 3] & 0xFC) | bits;
        pos += header.frame_size_bytes();
    }
    out
}

#[test]
fn staged_fixture_emphasis_rewrite_decodes_with_the_reference_deemphasis() {
    // The staged J.17 note (§5) documents the header-rewrite probe:
    // take the `layer2-stereo-44100-192kbps` fixture (31 frames,
    // CRC absent) and flip `emphasis` from '00' to a filtered code in
    // every frame header. The three surveyed third-party decoders all
    // parse-and-discard the field (byte-identical PCM), so the staged
    // fixture can pin nothing about de-emphasis. *This* decoder
    // honours §2.4.2.4: run the same rewrite here and require the
    // patched decode to equal the plain decode passed through the
    // reference filter — sample for sample, on real broadcast-chain
    // content, exercising the 44,1 kHz J.17 fit inside the full
    // pipeline. Skips cleanly when `docs/` isn't checked out.
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/input.mp3"
    );
    if !std::path::Path::new(fixture_path).exists() {
        eprintln!("skip: staged Layer II fixture absent at {fixture_path}");
        return;
    }
    let stream = std::fs::read(fixture_path).expect("read staged fixture");
    let plain_pcm = decode_all_frames(&stream).expect("decode plain fixture");
    assert_eq!(plain_pcm.len(), 2, "fixture is stereo");

    for (bits, make_filter) in [
        (0b01u8, DeEmphasis::fifty_fifteen as fn(u32) -> DeEmphasis),
        (0b11u8, DeEmphasis::ccitt_j17 as fn(u32) -> DeEmphasis),
    ] {
        let patched = patch_emphasis(&stream, bits);
        let emph_pcm = decode_all_frames(&patched).expect("decode patched fixture");
        for ch in 0..2 {
            assert_eq!(plain_pcm[ch].len(), emph_pcm[ch].len());
            let mut filt = make_filter(44_100);
            let mut differed = false;
            for (i, &p) in plain_pcm[ch].iter().enumerate() {
                let expected = filt.process_sample(p);
                let got = emph_pcm[ch][i];
                assert!(
                    (expected - got).abs() < 1e-9,
                    "emphasis {bits:#04b} ch{ch}[{i}]: expected {expected}, got {got}"
                );
                if (p - got).abs() > 1e-3 {
                    differed = true;
                }
            }
            assert!(
                differed,
                "emphasis {bits:#04b} ch{ch}: de-emphasis produced no change on the fixture"
            );
        }
    }
}

#[test]
fn emphasis_bits_are_inside_the_layer2_crc_protected_field() {
    // §2.4.1.4 / Annex B Table B.5: the Layer II CRC protects the
    // second half of the header — the wire fields from
    // `bitrate_index` through `emphasis` inclusive (this crate's
    // Table B.5 reading is validated bit-exactly against a
    // reference-encoded CRC stream, `tests/fixtures/crc_48k_192.mp2`).
    // Flipping the emphasis bits of a CRC-protected frame without
    // recomputing the CRC must therefore be *detected*:
    let pcm = source_pcm();
    let mut h = header(Emphasis::None);
    h.protection_bit = false; // CRC word present
    let stream = encode_all_frames(&h, &pcm, 0).expect("encode CRC stream");

    let parsed = FrameHeader::parse(&stream).expect("parse header");
    let frame_len = parsed.frame_size_bytes();
    let mut tampered = stream[..frame_len].to_vec();
    tampered[3] = (tampered[3] & 0xFC) | 0x03; // emphasis '00' → '11'
    match decode_frame(&tampered) {
        Err(FrameError::CrcMismatch { .. }) => {}
        other => panic!("tampered emphasis must fail CRC, got {other:?}"),
    }
    // The untampered frame passes its CRC check.
    decode_frame(&stream[..frame_len]).expect("untampered CRC frame decodes");

    // On a CRC-absent stream (`protection_bit == 1`) the same edit is
    // legal — that is what the header-rewrite tests above rely on.
    let free = encode_all_frames(&header(Emphasis::None), &pcm, 0).expect("encode no-CRC");
    let parsed = FrameHeader::parse(&free).expect("parse no-CRC header");
    let mut patched = free[..parsed.frame_size_bytes()].to_vec();
    patched[3] = (patched[3] & 0xFC) | 0x03;
    decode_frame(&patched).expect("emphasis patch on a CRC-absent frame decodes");
}

#[test]
fn j17_round_trips_at_an_lsf_rate() {
    // The MPEG-2 LSF header carries the same §2.4.2.3 emphasis field;
    // exercise the J.17 pre→de acoustic loop at 24 kHz so an LSF-rate
    // filter fit runs inside the real pipeline (not just the unit
    // tests).
    let fs_hz = 24_000_u32;
    let fs = f64::from(fs_hz);
    let n = 3 * 1152;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / fs;
        let lo = 0.3 * (2.0 * std::f64::consts::PI * 500.0 * t).sin();
        let hi = 0.1 * (2.0 * std::f64::consts::PI * 9_000.0 * t).sin();
        left.push(lo + hi);
        right.push(lo - hi);
    }
    let pcm = vec![left, right];

    let lsf_header = FrameHeader {
        lsf: true,
        bit_rate: 160_000,
        sample_rate: fs_hz,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::CcittJ17,
        protection_bit: true,
    };
    let stream = encode_all_frames(&lsf_header, &pcm, 0).expect("encode LSF J.17");
    let out = decode_all_frames(&stream).expect("decode LSF J.17");
    assert_eq!(out.len(), 2);

    let skip = 512;
    for (ch, chan) in out.iter().enumerate() {
        let e_lo = bin_energy(chan, fs, 500.0, skip);
        let e_hi = bin_energy(chan, fs, 9_000.0, skip);
        let e_probe = bin_energy(chan, fs, 4_000.0, skip);
        assert!(
            e_lo > 50.0 * e_probe,
            "ch{ch}: 500 Hz tone not recovered (lo={e_lo}, probe={e_probe})"
        );
        assert!(
            e_hi > 50.0 * e_probe,
            "ch{ch}: 9 kHz tone not recovered after LSF J.17 de-emphasis (hi={e_hi}, probe={e_probe})"
        );
    }
}
