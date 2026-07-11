//! §2.4.2.3 padding-bit rate control — integration coverage.
//!
//! "If this bit equals '1', the frame contains an additional slot to
//! adjust the mean bitrate to the sampling frequency […]. Padding is
//! necessary with a sampling frequency of 44,1 kHz." (ISO/IEC
//! 11172-3:1993 §2.4.2.3; ISO/IEC 13818-3 keeps the same Layer II
//! syntax at the LSF rates, where 22,05 kHz is the fractional one.)
//!
//! The spec pins the target with an accumulated-length invariant — the
//! total coded length must never deviate more than (+0, −1 slot) from
//! `Σ 1152·bitrate/(8·Fs)` bytes — and supplies the `rest`/`dif`
//! decision procedure the crate implements as
//! [`PaddingScheduler`]. These tests drive the *public* encode
//! surfaces (batch [`encode_all_frames`] and the registry
//! [`make_encoder`]) and verify the emitted byte streams:
//!
//! 1. **Frame-walk** — every emitted frame's header carries exactly the
//!    padding bit the §2.4.2.3 algorithm prescribes, and its byte size
//!    is `N + padding`.
//! 2. **Mean-bitrate invariant** — the accumulated stream length stays
//!    strictly within one slot of the exact `Σ 144·bitrate/Fs` value at
//!    every frame boundary (the verbatim procedure's envelope; it
//!    forces the first frame unpadded, then corrects).
//! 3. **Decode symmetry** — the padded stream round-trips through
//!    [`decode_all_frames`] with exact sample count (the decoder sizes
//!    each frame from its own header).
//! 4. **Free format** — clearing the `bitrate_index` nibble of a padded
//!    stream still resolves through the §2.4.2.3 free-format
//!    size-measurement path ("Padding may also be required in free
//!    format"), decoding bit-identically to the standard stream.
//!
//! Clean-room basis: §2.4.2.3 padding_bit semantics + decision
//! procedure and the §2.4.3.1 frame-size formula, read from the staged
//! `docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf` and
//! `docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`. No
//! third-party MP2 implementation source was consulted.

use oxideav_mp2::header::{Emphasis, Mode, ModeExtension};
use oxideav_mp2::{
    decode_all_frames, encode_all_frames, make_encoder, FrameHeader, PaddingScheduler,
    PCM_SAMPLES_PER_CHANNEL,
};

use oxideav_core::{AudioFrame, CodecId, CodecParameters, Error, Frame};

fn stereo_header(lsf: bool, sample_rate: u32, bit_rate: u32) -> FrameHeader {
    FrameHeader {
        lsf,
        protection_bit: true, // no CRC (inverted §2.4.2.3 convention)
        bit_rate,
        sample_rate,
        padding: false,
        private_bit: false,
        mode: Mode::Stereo,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    }
}

/// `n_frames` of a continuous stereo sine.
fn tone_stream(freq_hz: f64, amp: f64, sample_rate: u32, n_frames: usize) -> Vec<Vec<f64>> {
    let omega = 2.0 * std::f64::consts::PI * freq_hz / sample_rate as f64;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    (0..2)
        .map(|_| (0..total).map(|i| amp * (omega * i as f64).sin()).collect())
        .collect()
}

/// Walk `stream` frame by frame (each frame sized by its own header)
/// and return the per-frame `(padding, size)` pairs.
fn walk_frames(stream: &[u8]) -> Vec<(bool, usize)> {
    let mut out = Vec::new();
    let mut off = 0;
    while off < stream.len() {
        let h = FrameHeader::parse(&stream[off..]).expect("frame header at boundary");
        let size = h.frame_size_bytes();
        assert!(off + size <= stream.len(), "frame overruns stream");
        out.push((h.padding, size));
        off += size;
    }
    assert_eq!(off, stream.len(), "stream is whole frames");
    out
}

/// The fractional Layer II rates: (lsf, sample_rate, total bitrate).
const FRACTIONAL_RATES: &[(bool, u32, u32)] = &[
    (false, 44_100, 128_000),
    (false, 44_100, 192_000),
    (true, 22_050, 64_000),
];

/// The evenly-dividing rates never pad.
const EVEN_RATES: &[(bool, u32, u32)] = &[
    (false, 32_000, 128_000),
    (false, 48_000, 192_000),
    (true, 16_000, 64_000),
    (true, 24_000, 64_000),
];

#[test]
fn batch_encode_emits_the_spec_padding_schedule_at_fractional_rates() {
    let n_frames = 48;
    for &(lsf, sample_rate, bit_rate) in FRACTIONAL_RATES {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(700.0, 0.4, sample_rate, n_frames);
        let bytes = encode_all_frames(&header, &stream, 0).expect("encode");

        let frames = walk_frames(&bytes);
        assert_eq!(frames.len(), n_frames, "{sample_rate} Hz: frame count");

        // 1. Frame-walk: emitted padding bits equal the §2.4.2.3
        //    algorithm's prescription, and sizes are N + padding.
        let mut sched = PaddingScheduler::new();
        let base = header.frame_size_bytes();
        let mut n_padded = 0usize;
        for (f, &(padding, size)) in frames.iter().enumerate() {
            let want = sched.next(bit_rate, sample_rate);
            assert_eq!(
                padding, want,
                "{sample_rate} Hz frame {f}: padding bit vs §2.4.2.3 algorithm"
            );
            assert_eq!(
                size,
                base + usize::from(padding),
                "{sample_rate} Hz frame {f}: frame size"
            );
            n_padded += usize::from(padding);
        }
        assert!(!frames[0].0, "{sample_rate} Hz: first frame unpadded");
        assert!(n_padded > 0, "{sample_rate} Hz: schedule genuinely pads");

        // 2. Mean-bitrate invariant: the verbatim §2.4.2.3 procedure
        //    (first frame forced unpadded) keeps the accumulated length
        //    strictly within one slot of the exact value at every
        //    frame boundary.
        let exact_per_frame = 144.0 * f64::from(bit_rate) / f64::from(sample_rate);
        let mut actual = 0.0f64;
        for (f, &(_, size)) in frames.iter().enumerate() {
            actual += size as f64;
            let dev = actual - exact_per_frame * (f + 1) as f64;
            assert!(
                dev.abs() < 1.0,
                "{sample_rate} Hz frame {f}: accumulated deviation {dev} \
                 reaches a whole slot"
            );
        }

        // 3. Decode symmetry.
        let planes = decode_all_frames(&bytes).expect("decode padded stream");
        assert_eq!(planes.len(), 2);
        for plane in &planes {
            assert_eq!(plane.len(), n_frames * PCM_SAMPLES_PER_CHANNEL);
        }
    }
}

#[test]
fn batch_encode_never_pads_at_evenly_dividing_rates() {
    let n_frames = 12;
    for &(lsf, sample_rate, bit_rate) in EVEN_RATES {
        let header = stereo_header(lsf, sample_rate, bit_rate);
        let stream = tone_stream(700.0, 0.4, sample_rate, n_frames);
        let bytes = encode_all_frames(&header, &stream, 0).expect("encode");
        assert_eq!(
            bytes.len(),
            n_frames * header.frame_size_bytes(),
            "{sample_rate} Hz: dif == 0 → constant frame size"
        );
        for (f, &(padding, _)) in walk_frames(&bytes).iter().enumerate() {
            assert!(!padding, "{sample_rate} Hz frame {f}: no padding");
        }
    }
}

#[test]
fn registry_encoder_packets_follow_the_padding_schedule() {
    let n_frames = 24;
    let sample_rate = 44_100u32;
    let bit_rate = 192_000u32;
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;

    let mut p = CodecParameters::audio(CodecId::new("mp2"));
    p.sample_rate = Some(sample_rate);
    p.channels = Some(2);
    p.bit_rate = Some(u64::from(bit_rate));
    let mut enc = make_encoder(&p).expect("make_encoder");

    // Planar S16 tone.
    let omega = 2.0 * std::f64::consts::PI * 800.0 / f64::from(sample_rate);
    let plane: Vec<u8> = (0..total)
        .flat_map(|i| {
            let s = (0.4 * (omega * i as f64).sin() * 32768.0)
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
            s.to_le_bytes()
        })
        .collect();
    enc.send_frame(&Frame::Audio(AudioFrame {
        samples: total as u32,
        pts: Some(0),
        data: vec![plane.clone(), plane],
    }))
    .expect("send_frame");
    enc.flush().expect("flush");

    let mut sched = PaddingScheduler::new();
    let base = stereo_header(false, sample_rate, bit_rate).frame_size_bytes();
    let mut npkts = 0usize;
    let mut n_padded = 0usize;
    loop {
        match enc.receive_packet() {
            Ok(pkt) => {
                let want_pad = sched.next(bit_rate, sample_rate);
                assert_eq!(
                    pkt.data.len(),
                    base + usize::from(want_pad),
                    "packet {npkts}: size vs §2.4.2.3 schedule"
                );
                let h = FrameHeader::parse(&pkt.data).expect("packet header");
                assert_eq!(h.padding, want_pad, "packet {npkts}: padding bit");
                n_padded += usize::from(want_pad);
                npkts += 1;
            }
            Err(Error::Eof) => break,
            Err(e) => panic!("receive_packet: {e:?}"),
        }
    }
    assert_eq!(npkts, n_frames);
    assert!(n_padded > 0, "44,1 kHz registry stream genuinely pads");
}

#[test]
fn padded_free_format_stream_resolves_and_decodes_bit_identically() {
    // "Padding may also be required in free format." A padded stream
    // whose bitrate_index nibbles are cleared must still resolve its
    // constant base size through the §2.4.2.3 sync-to-sync measurement
    // (frames are N or N+1 slots) and decode to bit-identical PCM.
    // 192 kbit/s stereo at 44.1 kHz keeps the signalled table (96 kbit/s
    // per channel → B.2b) equal to the free-format table at 44.1 kHz, so
    // the decode-identity premise holds (Table 3-B.2b header lists free
    // format).
    let n_frames = 16;
    let header = stereo_header(false, 44_100, 192_000);
    let stream = tone_stream(600.0, 0.4, 44_100, n_frames);
    let standard = encode_all_frames(&header, &stream, 0).expect("encode");

    // Padding-aware free-format rewrite: walk frames by their own
    // header size, clearing each frame's bitrate_index nibble.
    let mut free = standard.clone();
    let mut off = 0usize;
    while off < free.len() {
        let h = FrameHeader::parse(&free[off..]).expect("header");
        free[off + 2] &= 0x0F; // clear bitrate_index nibble (§2.4.2.3)
        off += h.frame_size_bytes();
    }

    let std_pcm = decode_all_frames(&standard).expect("standard decode");
    let ff_pcm = oxideav_mp2::decode_free_format_stream(&free).expect("padded free-format decode");
    assert_eq!(ff_pcm.len(), std_pcm.len());
    for ch in 0..std_pcm.len() {
        assert_eq!(ff_pcm[ch], std_pcm[ch], "ch {ch} bit-identical PCM");
    }
}
