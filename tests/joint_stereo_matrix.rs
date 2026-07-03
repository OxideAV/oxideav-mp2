//! Round 360 — joint-stereo (intensity) + dual-channel mode×rate
//! robustness matrix for the Layer II encode → decode pipeline.
//!
//! The existing `tests/roundtrip_multirate.rs` exercises only
//! `Mode::Stereo` with `ModeExtension::Bound4`. This test broadens the
//! channel-mode axis so the §2.4.1.6 **intensity-stereo region**
//! (`bound ≤ sb < sblimit`) and the `dual_channel` two-independent-mono
//! path get equal coverage across the whole sampling-rate ladder:
//!
//! * **Joint-stereo, every `mode_extension` bound (4 / 8 / 12 / 16).**
//!   The §2.4.2.3 bound sets where intensity coding starts; above it
//!   the bitstream carries ONE shared sample codeword per subband per
//!   §2.4.2.6, with each channel rescaling it by its own §2.4.3.3.3
//!   scalefactor. A decoder that mis-counts the codewords in the
//!   intensity region desyncs the whole frame, so a successful decode
//!   that lands exactly `frame_size_bytes()` long and reproduces the
//!   right tone is direct evidence the intensity loop stays
//!   bit-aligned.
//!
//! * **Bound clamping at low-`sblimit` tables.** §2.4.2.3:
//!   `bound = min(mode_extension_bound, sblimit)`. At 64 kbit/s the
//!   per-channel rate selects B.2c (`sblimit = 8`) at 48 kHz or B.2d
//!   (`sblimit = 12`) at 32 kHz, so `Bound12` / `Bound16` collapse the
//!   intensity region to empty — the joint-stereo frame degenerates to
//!   a flat per-channel read. The decoder must handle that degenerate
//!   case without reading phantom intensity codewords.
//!
//! * **Dual-channel mode.** Two independent mono programmes carried in
//!   one stream (`bound == sblimit`, no shared region). The two
//!   channels are decoded with fully independent allocation,
//!   scalefactors and codewords; feeding two *different* tones must
//!   reconstruct each channel's own tone.
//!
//! # Conformance basis
//!
//! The §C.1.3 analysis and §2.4.3.2 synthesis filterbanks are
//! floating-point with no prescribed accumulation order (ISO/IEC
//! 11172-4 defines conformance as a *bounded* difference signal), so —
//! exactly as in `roundtrip_multirate.rs` — the assertions are envelope
//! properties (sample count, reconstruction-energy ratio, spectral
//! localisation), not byte equalities.
//!
//! Clean-room basis: rate ladders, the `(bitrate, mode)` matrix, the
//! `bound = (mode_extension + 1) · 4` mapping and the
//! `bound = min(bound, sblimit)` clamp are all read from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` (§2.4.1.6 / §2.4.2.3 /
//! §2.4.2.6) and `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`
//! (§2.4.2.3 LSF Table). No third-party MP2 implementation source was
//! consulted.

use oxideav_mp2::audio_data::parse_audio_data_with_section_bits;
use oxideav_mp2::frame::{decode_frame, FrameError};
use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames, encode_all_frames_js, FrameHeader, PaddingScheduler,
    PCM_SAMPLES_PER_CHANNEL,
};

use oxideav_core::bits::BitReader;

/// Total byte length of an `n_frames` stream under the §2.4.2.3 padding
/// schedule the batch encoder drives (per-frame `N` / `N+1` slots at
/// the fractional 44,1 / 22,05 kHz rates; constant `N` elsewhere).
fn scheduled_stream_len(header: &FrameHeader, n_frames: usize) -> usize {
    let mut s = PaddingScheduler::new();
    (0..n_frames)
        .map(|_| s.next_header(header).frame_size_bytes())
        .sum()
}

/// Combined §C.1.3 analysis + §2.4.3.2 synthesis filterbank group
/// delay for Layer II, in samples (matches `roundtrip_multirate.rs`).
const FILTERBANK_DELAY: usize = 480;

fn header(
    lsf: bool,
    sample_rate: u32,
    bit_rate: u32,
    mode: Mode,
    mode_extension: ModeExtension,
) -> FrameHeader {
    FrameHeader {
        lsf,
        protection_bit: true, // true == "no CRC" per the §2.4.2.3 inverted convention
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode,
        mode_extension,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    }
}

/// `n_frames` of a continuous per-channel sine at `freq_hz`, sampled at
/// `sample_rate`. Amplitude stays inside the §2.4.3.4.7.1 `[-1, +1]`
/// range. Each channel may carry its own frequency.
fn tone_stream(freqs: &[f64], amp: f64, sample_rate: u32, n_frames: usize) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    freqs
        .iter()
        .map(|&f| {
            let omega = 2.0 * std::f64::consts::PI * f / sample_rate as f64;
            (0..total).map(|i| amp * (omega * i as f64).sin()).collect()
        })
        .collect()
}

/// Goertzel single-bin power estimate of `signal` at `freq_hz`
/// (sampled at `sample_rate`).
fn goertzel_power(signal: &[f64], freq_hz: f64, sample_rate: u32) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq_hz / sample_rate as f64;
    let coeff = 2.0 * w.cos();
    let mut s_prev = 0.0;
    let mut s_prev2 = 0.0;
    for &x in signal {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    s_prev * s_prev + s_prev2 * s_prev2 - coeff * s_prev * s_prev2
}

/// Assert the decoded `out` plane reproduces `want_freq` and not
/// `probe_freq`, and that the residual against the delayed original
/// holds only a fraction of the signal energy.
fn assert_tone_reconstructed(
    out: &[f64],
    original: &[f64],
    want_freq: f64,
    probe_freq: f64,
    sample_rate: u32,
    n_frames: usize,
    label: &str,
) {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL; // skip ramp-in
    let hi = total - PCM_SAMPLES_PER_CHANNEL; // skip trailing partial
    assert!(hi > lo, "{label}: stream long enough for a steady middle");

    let mut sig_energy = 0.0_f64;
    let mut err_energy = 0.0_f64;
    for i in lo..hi {
        let w = original[i - FILTERBANK_DELAY];
        let g = out[i];
        sig_energy += w * w;
        let e = g - w;
        err_energy += e * e;
    }
    assert!(sig_energy > 0.0, "{label}: non-trivial signal");
    let ratio = err_energy / sig_energy;
    assert!(
        ratio < 0.5,
        "{label}: reconstruction error/signal energy {ratio:.4} too high"
    );

    let steady = &out[lo..hi];
    let tone_power = goertzel_power(steady, want_freq, sample_rate);
    let probe_power = goertzel_power(steady, probe_freq, sample_rate);
    assert!(
        tone_power > 100.0 * probe_power.max(f64::MIN_POSITIVE),
        "{label}: tone power {tone_power:.3e} does not dominate probe power {probe_power:.3e}"
    );
}

/// (is_lsf, sample_rate, total_bitrate) tuples whose joint-stereo
/// per-channel rate selects B.2b (sblimit=30) at 44.1/32 kHz or B.2a
/// (sblimit=27) at 48 kHz — i.e. tables wide enough that all four
/// `mode_extension` bounds leave a non-empty intensity region.
const WIDE_RATE_MATRIX: &[(bool, u32, u32)] = &[
    (false, 32_000, 192_000), // per_ch 96 → B.2b (sblimit 30)
    (false, 44_100, 192_000), // per_ch 96 → B.2b (sblimit 30)
    (false, 48_000, 192_000), // per_ch 96 → B.2a (sblimit 27)
    (true, 16_000, 128_000),  // LSF → B.1 (sblimit 30)
    (true, 22_050, 128_000),  // LSF → B.1 (sblimit 30)
    (true, 24_000, 128_000),  // LSF → B.1 (sblimit 30)
];

const ALL_BOUNDS: &[ModeExtension] = &[
    ModeExtension::Bound4,
    ModeExtension::Bound8,
    ModeExtension::Bound12,
    ModeExtension::Bound16,
];

#[test]
fn joint_stereo_round_trips_at_every_bound_and_rate() {
    let n_frames = 8;
    let amp = 0.5;
    let tone_hz = 1_000.0;
    let probe_hz = 7_000.0;

    for &(lsf, sample_rate, bit_rate) in WIDE_RATE_MATRIX {
        for &ext in ALL_BOUNDS {
            let h = header(lsf, sample_rate, bit_rate, Mode::JointStereo, ext);
            let label = format!("JS {sample_rate}Hz {}kbps {ext:?}", bit_rate / 1000);

            // Identical input on both channels so the shared above-bound
            // codeword and per-channel scalefactors coincide.
            let stream = tone_stream(&[tone_hz, tone_hz], amp, sample_rate, n_frames);

            let bytes = encode_all_frames(&h, &stream, 0)
                .unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));
            // A desync in the intensity region would change the byte
            // count: the frame must be exactly n_frames whole frames
            // (§2.4.2.3 padded frames one slot larger).
            assert_eq!(
                bytes.len(),
                scheduled_stream_len(&h, n_frames),
                "{label}: encoded byte length"
            );

            let planes =
                decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
            assert_eq!(planes.len(), 2, "{label}: stereo");
            let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
            for (ch, plane) in planes.iter().enumerate() {
                assert_eq!(plane.len(), total, "{label}: ch {ch} sample count");
            }

            // Both channels carried the same tone; each must reproduce it.
            for (ch, plane) in planes.iter().enumerate() {
                assert_tone_reconstructed(
                    plane,
                    &stream[ch],
                    tone_hz,
                    probe_hz,
                    sample_rate,
                    n_frames,
                    &format!("{label} ch{ch}"),
                );
            }
        }
    }
}

#[test]
fn joint_stereo_intensity_region_is_actually_exercised() {
    // The robustness test above proves the decoder *survives* the
    // intensity region, but only if the region is non-empty. Pin that
    // the encoder produces an allocated above-bound subband for a
    // wide-table joint-stereo frame at each bound, and that the
    // decoder's parsed bound matches the clamped expectation — so the
    // round-trip is genuinely covering the shared-codeword path and not
    // silently degenerating to `bound == sblimit`.
    let n_frames = 2;
    let amp = 0.6;
    // A tone high enough to push energy into upper subbands so the
    // allocator funds the intensity region. 1 kHz at 44.1 kHz sits in a
    // low subband; pick a spread of energy via a richer two-tone input.
    let sample_rate = 44_100;
    let stream: Vec<Vec<f64>> = {
        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let mk = |f1: f64, f2: f64| -> Vec<f64> {
            let w1 = 2.0 * std::f64::consts::PI * f1 / sample_rate as f64;
            let w2 = 2.0 * std::f64::consts::PI * f2 / sample_rate as f64;
            (0..total)
                .map(|i| amp * 0.5 * ((w1 * i as f64).sin() + (w2 * i as f64).sin()))
                .collect()
        };
        vec![mk(1_000.0, 12_000.0), mk(1_000.0, 12_000.0)]
    };

    for &ext in ALL_BOUNDS {
        let h = header(false, sample_rate, 192_000, Mode::JointStereo, ext);
        let label = format!("JS-intensity {ext:?}");
        let bytes =
            encode_all_frames(&h, &stream, 0).unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));

        // Parse frame 0's audio-data side info to inspect the bound and
        // the above-bound allocation.
        let mut reader = BitReader::with_position(&bytes, 4);
        let (audio, _, _) =
            parse_audio_data_with_section_bits(&h, &mut reader).expect("parse audio-data");

        let expected_bound = ext.bound().min(audio.sblimit);
        assert_eq!(audio.bound, expected_bound, "{label}: clamped bound");
        assert!(
            audio.bound < audio.sblimit,
            "{label}: wide table keeps a non-empty intensity region (bound {} < sblimit {})",
            audio.bound,
            audio.sblimit
        );
        // At least one above-bound subband should carry a shared
        // allocation — otherwise the round-trip never reads a shared
        // codeword and the intensity path is untested.
        let any_allocated = (audio.bound..audio.sblimit).any(|sb| audio.nb_steps[0][sb] != 0);
        assert!(
            any_allocated,
            "{label}: at least one above-bound subband must be allocated for the test to bite"
        );
        // §2.4.1.6 forces allocation[1][sb] == allocation[0][sb] above
        // bound; the decoder copies the single on-wire field to both
        // channels.
        for sb in audio.bound..audio.sblimit {
            assert_eq!(
                audio.nb_steps[0][sb], audio.nb_steps[1][sb],
                "{label}: above-bound sb={sb} must share allocation across channels"
            );
        }
    }
}

#[test]
fn joint_stereo_bound_clamps_to_sblimit_at_low_rate_tables() {
    // §2.4.2.3 `bound = min(mode_extension_bound, sblimit)`. At 64
    // kbit/s the joint-stereo per-channel rate is 32 kbit/s, selecting
    // B.2c (sblimit=8) at 48 kHz and B.2d (sblimit=12) at 32 kHz. With
    // `Bound16` the intensity region collapses to empty; the
    // joint-stereo frame degenerates to a flat per-channel read. The
    // decoder must round-trip the degenerate case without reading any
    // phantom intensity codewords.
    let n_frames = 4;
    let amp = 0.4;
    let tone_hz = 900.0;
    let probe_hz = 5_000.0;

    // (sample_rate, expected_sblimit) for the B.2c / B.2d clamp cases.
    let clamp_cases: &[(u32, usize)] = &[(48_000, 8), (32_000, 12)];

    for &(sample_rate, expected_sblimit) in clamp_cases {
        for &ext in &[ModeExtension::Bound12, ModeExtension::Bound16] {
            let h = header(false, sample_rate, 64_000, Mode::JointStereo, ext);
            let label = format!("JS-clamp {sample_rate}Hz {ext:?}");
            let stream = tone_stream(&[tone_hz, tone_hz], amp, sample_rate, n_frames);
            let bytes = encode_all_frames(&h, &stream, 0)
                .unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));

            let mut reader = BitReader::with_position(&bytes, 4);
            let (audio, _, _) =
                parse_audio_data_with_section_bits(&h, &mut reader).expect("parse audio-data");
            assert_eq!(
                audio.sblimit, expected_sblimit,
                "{label}: low-rate table sblimit"
            );
            // Bound clamps down to sblimit → intensity region is empty.
            assert_eq!(
                audio.bound, expected_sblimit,
                "{label}: bound clamps to sblimit so intensity region is empty"
            );

            let planes =
                decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
            assert_eq!(planes.len(), 2, "{label}: stereo");
            let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
            for (ch, plane) in planes.iter().enumerate() {
                assert_eq!(plane.len(), total, "{label}: ch {ch} sample count");
                assert_tone_reconstructed(
                    plane,
                    &stream[ch],
                    tone_hz,
                    probe_hz,
                    sample_rate,
                    n_frames,
                    &format!("{label} ch{ch}"),
                );
            }
        }
    }
}

#[test]
fn dual_channel_reconstructs_two_independent_tones() {
    // §2.4.2.3 `dual_channel`: two independent mono programmes carried
    // in one stream. `bound == sblimit` (no intensity region), and the
    // two channels are decoded with fully independent allocation,
    // scalefactors and codewords. Feeding each channel a *different*
    // tone must reconstruct that channel's own tone — a cross-channel
    // leak (e.g. accidental codeword sharing) would put channel 1's
    // tone into channel 0.
    let n_frames = 8;
    let amp = 0.5;
    // Two distinct tones, both low enough to localise cleanly at every
    // rate in the matrix (including the LSF 16 kHz / flat-SMR path,
    // whose allocator starves higher subbands more than the
    // perceptually-shaped MPEG-1 rates).
    let l_hz = 800.0;
    let r_hz = 1_600.0;

    // dual_channel is allowed at 64..=192 kbit/s and 224..=384 kbit/s.
    for &(lsf, sample_rate, bit_rate) in WIDE_RATE_MATRIX {
        let h = header(
            lsf,
            sample_rate,
            bit_rate,
            Mode::DualChannel,
            ModeExtension::Bound4, // ignored for dual_channel
        );
        let label = format!("dual {sample_rate}Hz {}kbps", bit_rate / 1000);
        let stream = tone_stream(&[l_hz, r_hz], amp, sample_rate, n_frames);

        let bytes =
            encode_all_frames(&h, &stream, 0).unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));
        assert_eq!(
            bytes.len(),
            scheduled_stream_len(&h, n_frames),
            "{label}: encoded byte length"
        );

        // dual_channel has bound == sblimit (no shared region).
        let mut reader = BitReader::with_position(&bytes, 4);
        let (audio, _, _) =
            parse_audio_data_with_section_bits(&h, &mut reader).expect("parse audio-data");
        assert_eq!(
            audio.bound, audio.sblimit,
            "{label}: dual_channel has no intensity region"
        );

        let planes = decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        assert_eq!(planes.len(), 2, "{label}: two channels");

        // Channel 0 reproduces the left tone (and not the right one);
        // channel 1 reproduces the right tone (and not the left one).
        assert_tone_reconstructed(
            &planes[0],
            &stream[0],
            l_hz,
            r_hz, // the *other* channel's tone is the probe
            sample_rate,
            n_frames,
            &format!("{label} ch0"),
        );
        assert_tone_reconstructed(
            &planes[1],
            &stream[1],
            r_hz,
            l_hz,
            sample_rate,
            n_frames,
            &format!("{label} ch1"),
        );
    }
}

// ---------------------------------------------------------------------
// Decode-robustness fuzz: adversarial joint-stereo / dual-channel
// payloads built DIRECTLY (not via our own encoder).
//
// The round-trip tests above pair our encoder with our decoder, so a
// shared bug in the intensity-region loop (e.g. both reading two
// codewords above bound) could cancel out and pass. These tests bypass
// the encoder entirely: they synthesise raw frames with a joint-stereo
// or dual-channel header and an arbitrary payload, then assert
// `decode_frame` never panics, never overruns the buffer, and either
// succeeds with the correct PCM shape or returns a documented
// `FrameError`. A spec-conformant decoder consumes a bounded, exact
// number of payload bits for any allocation, so even a byte-pattern our
// encoder would never emit must be handled gracefully.
// ---------------------------------------------------------------------

/// Build a 4-byte Layer II header word from explicit field values
/// (§2.4.1.3 layout), MPEG-1 (`id == 1`, layer `'10'`).
fn build_header_bytes(
    bitrate_index: u32,
    sf_index: u32,
    mode_bits: u32,
    mode_ext_bits: u32,
    protection_bit: u32,
) -> [u8; 4] {
    let word: u32 = (0xFFF << 20)
        | (1 << 19)
        | (0b10 << 17)
        | (protection_bit << 16)
        | (bitrate_index << 12)
        | (sf_index << 10)
        | (mode_bits << 6)
        | (mode_ext_bits << 4)
        | (1 << 2); // original = 1, everything else 0
    word.to_be_bytes()
}

/// Synthesise a complete frame: a parseable header followed by a payload
/// filled with `pattern`. The frame is exactly `frame_size_bytes()`
/// long. Returns `None` if the header doesn't parse (caller skips).
fn synth_frame(header4: [u8; 4], pattern: u8) -> Option<Vec<u8>> {
    let parsed = FrameHeader::parse(&header4).ok()?;
    let fs = parsed.frame_size_bytes();
    let mut frame = vec![pattern; fs];
    frame[..4].copy_from_slice(&header4);
    Some(frame)
}

/// Assert a decode result is either a correctly-shaped success or a
/// documented error — never a panic or malformed output.
fn assert_decode_graceful(frame: &[u8], channels: usize, label: &str) {
    match decode_frame(frame) {
        Ok(decoded) => {
            assert_eq!(decoded.pcm.len(), channels, "{label}: channel count");
            for (ch, plane) in decoded.pcm.iter().enumerate() {
                assert_eq!(
                    plane.len(),
                    PCM_SAMPLES_PER_CHANNEL,
                    "{label}: ch {ch} sample count"
                );
                for (n, &v) in plane.iter().enumerate() {
                    assert!(v.is_finite(), "{label}: ch {ch} n {n} non-finite: {v}");
                }
            }
        }
        // Exhaustive match (no wildcard): a new FrameError variant added
        // without updating this test fails to compile, keeping the
        // documented-error contract honest.
        Err(err) => match err {
            FrameError::Header(_)
            | FrameError::AudioData(_)
            | FrameError::Requant(_)
            | FrameError::Truncated { .. }
            | FrameError::CrcMismatch { .. }
            | FrameError::UnknownQuantClass { .. }
            | FrameError::FreeFormat(_) => {}
        },
    }
}

#[test]
fn joint_stereo_adversarial_payloads_never_panic() {
    // 192 kbit/s (index 0b1010) joint-stereo (mode '01') at 44.1 kHz
    // (sf '00'), no CRC (protection '1'), across all four mode_extension
    // bounds, with a spread of payload byte patterns. The all-ones
    // pattern maximally allocates every subband (largest nb_steps),
    // stressing the deepest sample-codeword reads through the intensity
    // region; the alternating patterns walk the bit-alignment.
    for mode_ext in 0..4u32 {
        let header4 = build_header_bytes(0b1010, 0b00, 0b01, mode_ext, 1);
        for &pattern in &[0x00u8, 0xFF, 0xAA, 0x55, 0x0F, 0xF0, 0x3C, 0xC3] {
            let Some(frame) = synth_frame(header4, pattern) else {
                continue;
            };
            assert_decode_graceful(
                &frame,
                2,
                &format!("JS-fuzz ext={mode_ext} pattern={pattern:#04x}"),
            );
        }
    }
}

#[test]
fn dual_channel_adversarial_payloads_never_panic() {
    // dual_channel (mode '10') at 192 kbit/s / 44.1 kHz: bound ==
    // sblimit, two fully-independent channels. The same payload spread
    // exercises the per-channel allocation reads with no shared region.
    let header4 = build_header_bytes(0b1010, 0b00, 0b10, 0b00, 1);
    for &pattern in &[0x00u8, 0xFF, 0xAA, 0x55, 0x0F, 0xF0, 0x3C, 0xC3, 0x99, 0x66] {
        let Some(frame) = synth_frame(header4, pattern) else {
            continue;
        };
        assert_decode_graceful(&frame, 2, &format!("dual-fuzz pattern={pattern:#04x}"));
    }
}

#[test]
fn joint_stereo_adversarial_payloads_at_low_rate_tables_never_panic() {
    // 64 kbit/s (index 0b0110) joint-stereo at 48 kHz (sf '01') and
    // 32 kHz (sf '10') select the narrow B.2c / B.2d tables where the
    // bound clamps to sblimit. Fuzz the payload at every bound so the
    // degenerate (empty intensity region) and non-degenerate cases both
    // get adversarial coverage.
    for (sf_index, rate_label) in [(0b01u32, "48kHz"), (0b10u32, "32kHz")] {
        for mode_ext in 0..4u32 {
            let header4 = build_header_bytes(0b0110, sf_index, 0b01, mode_ext, 1);
            for &pattern in &[0x00u8, 0xFF, 0xAA, 0x55, 0x3C] {
                let Some(frame) = synth_frame(header4, pattern) else {
                    continue;
                };
                assert_decode_graceful(
                    &frame,
                    2,
                    &format!("JS-lowrate-fuzz {rate_label} ext={mode_ext} pattern={pattern:#04x}"),
                );
            }
        }
    }
}

#[test]
fn joint_stereo_truncated_frames_are_truncated_not_panic() {
    // Every prefix of a joint-stereo frame that is too short to hold the
    // declared payload must be rejected gracefully (Truncated, or a
    // payload-underflow AudioData/Requant error) — never a panic or an
    // out-of-bounds read in the intensity region.
    let header4 = build_header_bytes(0b1010, 0b00, 0b01, 0b01, 1); // Bound8
    let Some(frame) = synth_frame(header4, 0xC9) else {
        panic!("header must parse");
    };
    // Walk a representative set of prefixes (every byte boundary would
    // be ~626 iterations; sample densely near the start where the
    // header/audio-data boundary lives and sparsely after).
    let fs = frame.len();
    let mut len = 4;
    while len < fs {
        assert_decode_graceful(&frame[..len], 2, &format!("JS-trunc len={len}"));
        len += if len < 40 { 1 } else { 17 };
    }
}

#[test]
fn joint_stereo_silence_round_trips_to_exact_zero_at_every_bound() {
    // Silence funds no scalefactors or sample codewords, so the
    // requantiser never runs and the intensity region carries nothing —
    // the decode must be exact zero regardless of the declared bound.
    let n_frames = 3;
    let sample_rate = 44_100;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let stream: Vec<Vec<f64>> = vec![vec![0.0; total]; 2];

    for &ext in ALL_BOUNDS {
        let h = header(false, sample_rate, 192_000, Mode::JointStereo, ext);
        let label = format!("JS-silence {ext:?}");
        let bytes =
            encode_all_frames(&h, &stream, 0).unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));
        let planes = decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        assert_eq!(planes.len(), 2, "{label}");
        for (ch, plane) in planes.iter().enumerate() {
            assert_eq!(plane.len(), total, "{label}: ch {ch} len");
            for (i, &s) in plane.iter().enumerate() {
                assert_eq!(
                    s, 0.0,
                    "{label}: silence sample[{i}] on ch {ch} must be exact zero, got {s}"
                );
            }
        }
    }
}

/// RMS of a plane over its steady middle (skipping the ramp-in / trailing
/// partial frame).
fn steady_rms(plane: &[f64], n_frames: usize) -> f64 {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
    let hi = total - PCM_SAMPLES_PER_CHANNEL;
    let mut sum = 0.0_f64;
    for &s in &plane[lo..hi] {
        sum += s * s;
    }
    (sum / (hi - lo) as f64).sqrt()
}

#[test]
fn joint_stereo_reconstructs_per_channel_levels_in_the_intensity_region() {
    // The decisive intensity-stereo property: above `bound` the two
    // channels share ONE sample codeword but each carries its OWN
    // §2.4.3.3.3 scalefactor, so the decoder must reconstruct each
    // channel at its own level by rescaling the shared codeword by that
    // channel's scalefactor (frame.rs Region-2 loop).
    //
    // The round-trip tests above feed both channels identical input, so
    // their above-bound scalefactors coincide and a bug that ignored
    // channel 1's scalefactor (e.g. reused channel 0's) would still
    // pass. Here channel 1 is a deliberately QUIETER copy of channel 0
    // (same tone, half the amplitude). With correct per-channel
    // rescaling the decoded channel-1 level must be markedly below
    // channel 0; a decoder that applied channel 0's scalefactor to both
    // would reconstruct them at the SAME level and fail this test.
    let n_frames = 10;
    let amp0 = 0.6;
    let amp1 = 0.3; // half-level — a clear 6 dB pan toward channel 0

    for &(lsf, sample_rate, bit_rate) in WIDE_RATE_MATRIX {
        // Pick a tone whose subband index clears Bound4 so the tone's
        // subband lands in the intensity region (bound <= sb < sblimit).
        // A subband is `Fs/64` Hz wide; the ~5.5th subband centre lands
        // at subband index ≈ 5 (>= bound 4) for every rate in the matrix.
        let sb_width = sample_rate as f64 / 64.0;
        let intensity_tone = 5.5 * sb_width; // lands in subband ~5 (>= bound 4)

        let ext = ModeExtension::Bound4;
        let h = header(lsf, sample_rate, bit_rate, Mode::JointStereo, ext);
        let label = format!("JS-levels {sample_rate}Hz");

        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let omega = 2.0 * std::f64::consts::PI * intensity_tone / sample_rate as f64;
        let ch0: Vec<f64> = (0..total)
            .map(|i| amp0 * (omega * i as f64).sin())
            .collect();
        let ch1: Vec<f64> = (0..total)
            .map(|i| amp1 * (omega * i as f64).sin())
            .collect();
        let stream = vec![ch0, ch1];

        let bytes =
            encode_all_frames(&h, &stream, 0).unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));

        // Confirm the tone's subband is actually in the intensity region
        // (bound <= sb < sblimit) for this rate — otherwise the test
        // would be exercising the per-channel region instead.
        let mut reader = BitReader::with_position(&bytes, 4);
        let (audio, _, _) =
            parse_audio_data_with_section_bits(&h, &mut reader).expect("parse audio-data");
        assert!(
            audio.bound < audio.sblimit,
            "{label}: non-empty intensity region (bound {} < sblimit {})",
            audio.bound,
            audio.sblimit
        );

        let planes = decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        assert_eq!(planes.len(), 2, "{label}: stereo");

        let rms0 = steady_rms(&planes[0], n_frames);
        let rms1 = steady_rms(&planes[1], n_frames);
        assert!(rms0 > 1e-3, "{label}: channel 0 must carry the tone");
        assert!(
            rms1 > 1e-4,
            "{label}: channel 1 must carry the (quieter) tone"
        );

        // Channel 1 is half the amplitude of channel 0. The §2.4.3.3.3
        // scalefactor ladder is coarse (Table 3-B.1 steps of 2^(1/3)),
        // so the decoded ratio won't be exactly 0.5, but channel 1 must
        // clearly be QUIETER than channel 0 — proving its own
        // scalefactor was applied to the shared codeword. A decoder that
        // reused channel 0's scalefactor would give ratio ≈ 1.0.
        let ratio = rms1 / rms0;
        assert!(
            ratio < 0.75,
            "{label}: channel-1/channel-0 RMS ratio {ratio:.3} must reflect the \
             6 dB intensity pan (expected well below 1.0; ≈1.0 would mean \
             channel 1's own scalefactor was ignored)"
        );
        // And not absurdly low (the codeword is shared, so channel 1 is
        // a rescaled copy, not silence).
        assert!(
            ratio > 0.2,
            "{label}: channel-1/channel-0 RMS ratio {ratio:.3} unexpectedly low"
        );
    }
}

#[test]
fn intensity_sum_signal_preserves_right_only_content_above_bound() {
    // Annex G.1: "for some subbands, instead of transmitting separate
    // left and right subband samples, only the SUM-signal is
    // transmitted, but with scalefactors for both the left and right
    // channels". The decisive consequence: content present ONLY in
    // channel 1 above the bound must survive the encode — the shared
    // codeword is `L + R`, not channel 0's samples. An encoder that
    // wrote channel 0's (near-silent) samples as the shared codeword
    // would silence channel 1's tone entirely; this test fails against
    // that implementation and passes against the Annex G.1 sum signal.
    let n_frames = 10;
    let amp = 0.5;

    for &(lsf, sample_rate, bit_rate) in WIDE_RATE_MATRIX {
        // A tone whose subband clears Bound4 (subband ≈ 5), present in
        // channel 1 ONLY. Channel 0 is silent.
        let sb_width = sample_rate as f64 / 64.0;
        let intensity_tone = 5.5 * sb_width;

        let h = header(
            lsf,
            sample_rate,
            bit_rate,
            Mode::JointStereo,
            ModeExtension::Bound4,
        );
        let label = format!("JS-right-only {sample_rate}Hz");

        let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let omega = 2.0 * std::f64::consts::PI * intensity_tone / sample_rate as f64;
        let ch0: Vec<f64> = vec![0.0; total];
        let ch1: Vec<f64> = (0..total).map(|i| amp * (omega * i as f64).sin()).collect();
        let stream = vec![ch0, ch1];

        let bytes =
            encode_all_frames(&h, &stream, 0).unwrap_or_else(|e| panic!("{label}: encode: {e:?}"));
        let planes = decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{label}: decode: {e:?}"));
        assert_eq!(planes.len(), 2, "{label}: stereo");

        // Channel 1's tone must survive at close to its original level:
        // the sum signal is `0 + R = R`, quantized by the sum's own
        // scalefactor and rescaled on decode by channel 1's transmitted
        // scalefactor. Allow generous headroom for the Table 3-B.1
        // ladder granularity + quantization noise.
        let rms1 = steady_rms(&planes[1], n_frames);
        let input_rms = amp / 2.0_f64.sqrt();
        assert!(
            rms1 > 0.4 * input_rms,
            "{label}: right-only tone must survive the intensity encode \
             (decoded RMS {rms1:.4} vs input RMS {input_rms:.4}); a \
             channel-0-codeword encoder silences it"
        );

        // Spectral identity: it is the *tone* that survives, not noise.
        let total_len = n_frames * PCM_SAMPLES_PER_CHANNEL;
        let lo = FILTERBANK_DELAY + PCM_SAMPLES_PER_CHANNEL;
        let hi = total_len - PCM_SAMPLES_PER_CHANNEL;
        let steady = &planes[1][lo..hi];
        let tone_p = goertzel_power(steady, intensity_tone, sample_rate);
        let probe_p = goertzel_power(steady, 0.3 * intensity_tone, sample_rate);
        assert!(
            tone_p > 100.0 * probe_p.max(f64::MIN_POSITIVE),
            "{label}: decoded channel 1 must localise at the input tone \
             (tone {tone_p:.3e} vs probe {probe_p:.3e})"
        );

        // Channel 0 rescales the same codeword by its OWN (near-silent)
        // scalefactor, so it must stay far quieter than channel 1.
        let rms0 = steady_rms(&planes[0], n_frames);
        assert!(
            rms0 < 0.2 * rms1,
            "{label}: silent channel 0 must stay quiet (rms0 {rms0:.5} vs \
             rms1 {rms1:.5})"
        );
    }
}

#[test]
fn demand_driven_auto_bound_switches_with_bitrate_and_decodes() {
    // Annex G.1 flow: "First, an estimation is made of the required
    // bitrate […]. If the required bitrate exceeds the available
    // bitrate, the required bitrate can be decreased by setting a
    // number of subbands to intensity stereo mode." The batch
    // `encode_all_frames_js` applies that decision per frame: the SAME
    // stereo tone must come out as full `Stereo` frames when the
    // bitrate covers its §D.1 demand (384 kbit/s) and as `JointStereo`
    // frames when it does not (96 kbit/s).
    let n_frames = 6;
    let sample_rate = 44_100;
    let stream = tone_stream(&[1_000.0, 1_000.0], 0.3, sample_rate, n_frames);

    // Walk a stream frame by frame collecting each frame's mode.
    let modes_of = |bytes: &[u8]| -> Vec<Mode> {
        let mut out = Vec::new();
        let mut off = 0;
        while off < bytes.len() {
            let h = FrameHeader::parse(&bytes[off..]).expect("frame header");
            out.push(h.mode);
            off += h.frame_size_bytes();
        }
        out
    };

    // Rich budget: full Stereo everywhere.
    let rich = header(
        false,
        sample_rate,
        384_000,
        Mode::JointStereo,
        ModeExtension::Bound4,
    );
    let bytes = encode_all_frames_js(&rich, &stream, 0).expect("js 384k");
    let modes = modes_of(&bytes);
    assert_eq!(modes.len(), n_frames);
    assert!(
        modes.iter().all(|&m| m == Mode::Stereo),
        "384 kbit/s: demand fits, every frame full Stereo (got {modes:?})"
    );
    let planes = decode_all_frames(&bytes).expect("decode 384k");
    assert_eq!(planes[0].len(), n_frames * PCM_SAMPLES_PER_CHANNEL);

    // Tight budget + a signal whose per-frame §D.1 demand stays far
    // above the 96 kbit/s budget: a comb of well-separated tones, one
    // per low subband. Tonal maskers leave a large in-band SMR in each
    // carrying subband, so every frame demands deep quantization
    // across many slots and intensity coding kicks in on every frame.
    // (A single steady tone is too easy — after the frame-0 filterbank
    // ramp-in its masked demand fits full Stereo even at 96 kbit/s —
    // and broadband noise masks itself into near-zero demand; both
    // outcomes are the per-frame adaptivity the policy is meant to
    // show.)
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    let sb_width = sample_rate as f64 / 64.0;
    let comb: Vec<f64> = (0..total)
        .map(|i| {
            let t = i as f64;
            (0..12)
                .map(|k| {
                    let f = (k as f64 + 0.5) * sb_width;
                    0.07 * (2.0 * std::f64::consts::PI * f * t / sample_rate as f64).sin()
                })
                .sum()
        })
        .collect();
    let stream_comb = vec![comb.clone(), comb];
    let tight = header(
        false,
        sample_rate,
        96_000,
        Mode::JointStereo,
        ModeExtension::Bound4,
    );
    let bytes = encode_all_frames_js(&tight, &stream_comb, 0).expect("js 96k");
    let modes = modes_of(&bytes);
    assert_eq!(modes.len(), n_frames);
    assert!(
        modes.iter().all(|&m| m == Mode::JointStereo),
        "96 kbit/s tone comb: demand overshoots, every frame JointStereo (got {modes:?})"
    );
    let planes = decode_all_frames(&bytes).expect("decode 96k");
    assert_eq!(planes.len(), 2, "intensity-coded stream decodes as stereo");
    assert_eq!(planes[0].len(), n_frames * PCM_SAMPLES_PER_CHANNEL);
}
