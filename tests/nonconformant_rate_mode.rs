//! Decoding of MPEG-1 Layer II frames whose `(bitrate, mode)` pair lies
//! outside the ISO/IEC 11172-3 §2.4.2.3 "allowed modes" matrix.
//!
//! The matrix (PDF page 21) constrains encoders: 32 / 48 / 56 / 80
//! kbit/s are single_channel only, 224 / 256 / 320 / 384 kbit/s are
//! two-channel only. Streams that ignore it exist — a widely used
//! black-box encoder writes mono at 384 kbit/s by default — and they
//! are fully decodable: the frame length follows from §2.4.3.1 and the
//! allocation table from the per-channel rate (§2.4.3.3.1), which
//! `oxideav_mp2::select_table` extends beyond the Table 3-B.2 header
//! lists (≤ 48 kbit/s per channel → the low-rate table, ≥ 96 → the
//! high-rate table).
//!
//! Two layers of evidence:
//!
//! 1. **Committed pins** (always run). Non-conformant frames are
//!    derived from this crate's own encoder output: a conformant frame
//!    whose table is the one the extension predicts is re-labelled to
//!    the non-conformant rate by rewriting the `bitrate_index` nibble
//!    and resizing the §2.4.1.8 ancillary tail (zero-padded when the
//!    target frame is longer; trimmed when shorter, the source having
//!    been encoded with that much ancillary space reserved). The audio
//!    data is untouched, so the decode must be **bit-identical** to
//!    the source's — which holds only if the same table is selected.
//! 2. **Black-box matrix** (skips at runtime without the tools). Every
//!    MPEG-1 and MPEG-2 LSF (rate, channels, bitrate) cell a black-box
//!    encoder will write is decoded here and by up to two black-box
//!    decoders; the PCM must agree within the ISO/IEC 11172-4 Layer II
//!    accuracy criterion (see `assert_close`).

use std::path::{Path, PathBuf};
use std::process::Command;

use oxideav_mp2::header::{Emphasis, HeaderError, Mode, ModeExtension};
use oxideav_mp2::{
    BitAllocTable, EncodeFrameState, FrameDecodeState, FrameError, FrameHeader,
    PCM_SAMPLES_PER_CHANNEL, decode_all_frames, decode_frame_with,
    encode_frame_auto_with_ancillary, select_table,
};

const MPEG1_RATES: [u32; 3] = [48_000, 44_100, 32_000];
const LADDER_KBPS: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];

fn header(sample_rate: u32, mode: Mode, kbps: u32) -> FrameHeader {
    FrameHeader {
        lsf: false,
        protection_bit: true, // '1' == no CRC word
        bit_rate: kbps * 1000,
        sample_rate,
        padding: false,
        private_bit: false,
        mode,
        mode_extension: ModeExtension::Bound4,
        copyright: false,
        original: true,
        emphasis: Emphasis::None,
    }
}

/// §2.4.2.3 `bitrate_index` of an MPEG-1 Layer II ladder rate.
fn bitrate_index(kbps: u32) -> u8 {
    1 + LADDER_KBPS
        .iter()
        .position(|&k| k == kbps)
        .expect("ladder rate") as u8
}

/// A busy deterministic test signal: three incommensurate tones per
/// channel (different per channel) so several subbands carry samples.
fn signal(channels: usize, sample_rate: u32, n_frames: usize) -> Vec<Vec<f64>> {
    let total = n_frames * PCM_SAMPLES_PER_CHANNEL;
    (0..channels)
        .map(|ch| {
            (0..total)
                .map(|i| {
                    let t = i as f64 / f64::from(sample_rate);
                    let w = 2.0 * std::f64::consts::PI * t;
                    0.30 * (w * (440.0 + 110.0 * ch as f64)).sin()
                        + 0.20 * (w * (1_730.0 + 300.0 * ch as f64)).sin()
                        + 0.10 * (w * 3_911.0).sin()
                })
                .collect()
        })
        .collect()
}

/// Encode `n_frames` conformant frames at `src_kbps`, each with at
/// least `reserve_bytes` of ancillary tail left unused, and re-label
/// every frame to `dst_kbps`. Returns `(source stream, relabelled
/// stream)`.
fn relabelled_stream(
    sample_rate: u32,
    mode: Mode,
    src_kbps: u32,
    dst_kbps: u32,
    n_frames: usize,
) -> (Vec<u8>, Vec<u8>) {
    let src_header = header(sample_rate, mode, src_kbps);
    let dst_header = header(sample_rate, mode, dst_kbps);
    let src_size = src_header.frame_size_bytes();
    let dst_size = dst_header.frame_size_bytes();
    let reserve_bytes = src_size.saturating_sub(dst_size);
    // One extra byte of reservation absorbs the allocator's sub-byte
    // rounding.
    let banc = if reserve_bytes == 0 {
        0
    } else {
        8 * (reserve_bytes as u32 + 1)
    };

    let pcm = signal(mode.channels(), sample_rate, n_frames);
    let mut state = EncodeFrameState::new();
    let mut src = Vec::new();
    let mut dst = Vec::new();
    for f in 0..n_frames {
        let lo = f * PCM_SAMPLES_PER_CHANNEL;
        let frame_pcm: Vec<Vec<f64>> = pcm
            .iter()
            .map(|c| c[lo..lo + PCM_SAMPLES_PER_CHANNEL].to_vec())
            .collect();
        let frame =
            encode_frame_auto_with_ancillary(&src_header, &frame_pcm, banc, &[], &mut state)
                .expect("conformant source frame encodes");
        assert_eq!(frame.len(), src_size);

        // The trimmed tail must be pure ancillary space.
        let decoded = decode_frame_with(&frame, &mut FrameDecodeState::new()).expect("source");
        assert!(
            decoded.ancillary.bits >= 8 * reserve_bytes,
            "source frame keeps only {} ancillary bits, need {}",
            decoded.ancillary.bits,
            8 * reserve_bytes
        );

        let mut relabelled = frame.clone();
        relabelled[2] = (relabelled[2] & 0x0F) | (bitrate_index(dst_kbps) << 4);
        relabelled.resize(dst_size, 0);
        src.extend_from_slice(&frame);
        dst.extend_from_slice(&relabelled);
    }
    (src, dst)
}

/// Every (rate, mode, bitrate) cell outside the matrix, with the
/// conformant source rate whose table the §2.4.3.3.1 extension
/// predicts.
fn nonconformant_cells() -> Vec<(u32, Mode, u32, u32)> {
    let mut cells = Vec::new();
    for sr in MPEG1_RATES {
        for mode in [Mode::Stereo, Mode::JointStereo, Mode::DualChannel] {
            // per-channel 16 / 24 / 28 / 40 kbit/s -> the table of
            // per-channel 32 (64 kbit/s total).
            for dst in [32, 48, 56, 80] {
                cells.push((sr, mode, 64, dst));
            }
        }
        // per-channel 224 … 384 kbit/s -> the table of per-channel 192.
        for dst in [224, 256, 320, 384] {
            cells.push((sr, Mode::SingleChannel, 192, dst));
        }
    }
    cells
}

#[test]
fn table_selection_outside_the_header_lists_is_pinned() {
    use BitAllocTable::{B2a, B2b, B2c, B2d};
    for (sr, low, high) in [(48_000, B2c, B2a), (44_100, B2c, B2b), (32_000, B2d, B2b)] {
        for kbps in [32, 48, 56, 80] {
            for mode in [Mode::Stereo, Mode::JointStereo, Mode::DualChannel] {
                assert_eq!(
                    select_table(&header(sr, mode, kbps)),
                    Some(low),
                    "{sr} Hz {mode:?} {kbps}k"
                );
            }
        }
        for kbps in [224, 256, 320, 384] {
            assert_eq!(
                select_table(&header(sr, Mode::SingleChannel, kbps)),
                Some(high),
                "{sr} Hz mono {kbps}k"
            );
        }
    }
    // No ladder rate is left without a table in any mode.
    for sr in MPEG1_RATES {
        for kbps in LADDER_KBPS {
            for mode in [
                Mode::Stereo,
                Mode::JointStereo,
                Mode::DualChannel,
                Mode::SingleChannel,
            ] {
                assert!(select_table(&header(sr, mode, kbps)).is_some());
            }
        }
    }
}

#[test]
fn relabelled_frames_decode_bit_identically_to_their_conformant_source() {
    for (sr, mode, src_kbps, dst_kbps) in nonconformant_cells() {
        let (src, dst) = relabelled_stream(sr, mode, src_kbps, dst_kbps, 3);
        let tag = format!("{sr} Hz {mode:?} {src_kbps}k -> {dst_kbps}k");

        // The strict parse still refuses; the tolerant one accepts and
        // flags the header.
        assert_eq!(
            FrameHeader::parse(&dst),
            Err(HeaderError::DisallowedBitrateModeCombination {
                bit_rate: dst_kbps * 1000,
                mode
            }),
            "{tag}"
        );
        let h = FrameHeader::parse_tolerant(&dst).expect("tolerant parse");
        assert_eq!(h.bit_rate, dst_kbps * 1000, "{tag}");
        assert!(!h.is_bitrate_mode_conformant(), "{tag}");
        assert_eq!(
            FrameHeader::parse_tolerant_allow_free_format(&dst),
            Ok(h),
            "{tag}"
        );

        let want = decode_all_frames(&src).expect("source decodes");
        let got = decode_all_frames(&dst).unwrap_or_else(|e| panic!("{tag}: {e}"));
        assert_eq!(got.len(), mode.channels(), "{tag}");
        assert_eq!(got, want, "{tag}: PCM differs from the conformant source");
        assert!(
            got[0].iter().any(|&s| s.abs() > 0.05),
            "{tag}: decoded silence"
        );
    }
}

#[test]
fn strict_state_refuses_what_the_default_state_decodes() {
    let (src, dst) = relabelled_stream(44_100, Mode::SingleChannel, 192, 384, 1);

    let mut tolerant = FrameDecodeState::new();
    assert!(!tolerant.strict_bitrate_mode());
    decode_frame_with(&dst, &mut tolerant).expect("default state decodes");

    let mut strict = FrameDecodeState::new().with_strict_bitrate_mode(true);
    assert!(strict.strict_bitrate_mode());
    match decode_frame_with(&dst, &mut strict) {
        Err(FrameError::Header(HeaderError::DisallowedBitrateModeCombination {
            bit_rate: 384_000,
            mode: Mode::SingleChannel,
        })) => {}
        other => panic!("strict state must refuse, got {other:?}"),
    }
    // Conformant frames are unaffected by the strict switch.
    decode_frame_with(&src, &mut strict).expect("strict state decodes conformant frames");
}

mod registry {
    use super::*;
    use oxideav_core::{CodecId, CodecParameters, Frame, Packet, SampleFormat, TimeBase};
    use oxideav_mp2::{CODEC_ID_STR, make_decoder, make_encoder};

    #[test]
    fn registry_decoder_plays_mono_384k_and_stereo_32k() {
        for (sr, mode, src_kbps, dst_kbps) in [
            (44_100, Mode::SingleChannel, 192, 384),
            (48_000, Mode::Stereo, 64, 32),
            (32_000, Mode::JointStereo, 64, 80),
        ] {
            let (_, dst) = relabelled_stream(sr, mode, src_kbps, dst_kbps, 2);
            let size = header(sr, mode, dst_kbps).frame_size_bytes();
            let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
            p.sample_rate = Some(sr);
            p.channels = Some(mode.channels() as u16);
            p.sample_format = Some(SampleFormat::S16);
            let mut dec = make_decoder(&p).expect("decoder");
            for frame in dst.chunks(size) {
                let pkt = Packet::new(0, TimeBase::new(1, sr as i64), frame.to_vec());
                dec.send_packet(&pkt).expect("send_packet");
                let Frame::Audio(audio) = dec.receive_frame().expect("receive_frame") else {
                    panic!("expected audio");
                };
                assert_eq!(audio.samples as usize, PCM_SAMPLES_PER_CHANNEL);
                assert_eq!(audio.data.len(), mode.channels());
            }
        }
    }

    #[test]
    fn encoder_still_refuses_the_disallowed_combinations() {
        for (sr, channels, kbps) in [
            (44_100, 1u16, 384u64),
            (48_000, 1, 224),
            (32_000, 2, 32),
            (44_100, 2, 80),
        ] {
            let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
            p.sample_rate = Some(sr);
            p.channels = Some(channels);
            p.sample_format = Some(SampleFormat::S16);
            p.bit_rate = Some(kbps * 1000);
            assert!(
                make_encoder(&p).is_err(),
                "{sr} Hz {channels} ch {kbps}k must be refused by the encoder"
            );
        }
    }
}

// ---------------------------------------------------------------------
// Black-box matrix
// ---------------------------------------------------------------------

fn have(tool: &str, flag: &str) -> bool {
    Command::new(tool)
        .arg(flag)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("oxideav-mp2-ratemode-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Interleaved s16 as the `Decoder`-trait wrapper maps it.
fn to_s16_interleaved(pcm: &[Vec<f64>]) -> Vec<i16> {
    let n = pcm.iter().map(Vec::len).min().unwrap_or(0);
    let mut out = Vec::with_capacity(n * pcm.len());
    for i in 0..n {
        for ch in pcm {
            out.push(
                (ch[i] * 32768.0)
                    .round()
                    .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16,
            );
        }
    }
    out
}

fn s16le(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect()
}

/// Encode a noise + tone test clip with the black-box encoder. Returns
/// `None` when the tool declines the combination.
fn blackbox_encode(dir: &Path, sr: u32, channels: usize, kbps: u32) -> Option<Vec<u8>> {
    let path = dir.join(format!("{sr}_{channels}_{kbps}.mp2"));
    let src = format!(
        "anoisesrc=r={sr}:d=0.6:c=pink:a=0.4:seed=7[n];sine=f=997:r={sr}:d=0.6[s];\
         [n][s]amix=inputs=2:normalize=0,pan={}",
        if channels == 2 {
            "stereo|c0=c0|c1=0.6*c0"
        } else {
            "mono|c0=c0"
        }
    );
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &src])
        .args(["-c:a", "mp2", "-b:a", &format!("{kbps}k")])
        .args(["-ar", &sr.to_string(), "-ac", &channels.to_string()])
        .args(["-f", "mp2"])
        .arg(&path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    // Only keep the cell when the tool really wrote what was asked.
    let h = FrameHeader::parse_tolerant(&bytes).ok()?;
    (h.bit_rate == kbps * 1000 && h.sample_rate == sr && h.channels() == channels).then_some(bytes)
}

/// Black-box decoder A, asked for its floating-point output so the
/// comparison is not dominated by its 16-bit rounding.
fn blackbox_decode_ffmpeg(dir: &Path, sr: u32, channels: usize, kbps: u32) -> Option<Vec<f64>> {
    let path = dir.join(format!("{sr}_{channels}_{kbps}.mp2"));
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-c:a", "mp2float", "-f", "mp3", "-i"])
        .arg(&path)
        .args(["-f", "f32le", "-c:a", "pcm_f32le", "-"])
        .output()
        .ok()?;
    out.status.success().then(|| {
        out.stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&b| f64::from(f32::from_le_bytes(b)))
            .collect()
    })
}

fn blackbox_decode_mpg123(dir: &Path, sr: u32, channels: usize, kbps: u32) -> Option<Vec<i16>> {
    let path = dir.join(format!("{sr}_{channels}_{kbps}.mp2"));
    let out = Command::new("mpg123")
        .args(["-q", "--no-gapless", "-e", "s16", "-s"])
        .arg(&path)
        .output()
        .ok()?;
    out.status.success().then(|| s16le(&out.stdout))
}

/// Layer II accuracy check, in units of one 16-bit LSB (`2^-15` of
/// full scale).
///
/// ISO/IEC 11172-4 defines audio decoder accuracy as a bound on the
/// difference signal rather than bit identity: rms below
/// `2^-15 / sqrt(12)` (0.289 LSB) with no sample further apart than
/// `2^-14` (2 LSB). Against a floating-point reference that criterion
/// is applied as written (`max_lsb = 2.0`, `rms_lsb = 0.289`). Against
/// a reference that is already rounded to 16 bits, both sides carry up
/// to half an LSB of rounding, so the caller passes 1 LSB / 0.5 LSB.
fn assert_close(tag: &str, ours: &[f64], theirs: &[f64], max_lsb: f64, rms_lsb: f64) {
    assert_eq!(ours.len(), theirs.len(), "{tag}: sample count");
    let mut max = 0f64;
    let mut sq = 0f64;
    for (&a, &b) in ours.iter().zip(theirs) {
        let d = (a - b).abs() * 32768.0;
        max = max.max(d);
        sq += d * d;
    }
    let rms = (sq / ours.len().max(1) as f64).sqrt();
    if std::env::var_os("OXIDEAV_MP2_MATRIX_VERBOSE").is_some() {
        eprintln!("{tag}: max {max:.4} LSB, rms {rms:.4} LSB");
    }
    assert!(max <= max_lsb, "{tag}: max difference {max} LSB");
    assert!(rms < rms_lsb, "{tag}: rms difference {rms} LSB");
}

/// Sweep (rate × channels × bitrate), decode every cell the black-box
/// encoder writes, and compare with the black-box decoders. Returns
/// `(cells written, cells outside the §2.4.2.3 matrix)`.
fn run_matrix(tag: &str, rates: &[u32], ladder_kbps: &[u32]) -> Option<(usize, usize)> {
    if !have("ffmpeg", "-version") {
        eprintln!("skip: ffmpeg not available");
        return None;
    }
    let mpg123 = have("mpg123", "--version");
    if !mpg123 {
        eprintln!("note: mpg123 not available, comparing with one black-box decoder only");
    }
    let dir = scratch_dir(tag);
    let mut written = 0;
    let mut nonconformant = 0;
    for &sr in rates {
        for channels in [1usize, 2] {
            for &kbps in ladder_kbps {
                let Some(bytes) = blackbox_encode(&dir, sr, channels, kbps) else {
                    continue;
                };
                written += 1;
                let tag = format!("{sr} Hz {channels} ch {kbps}k");
                if FrameHeader::parse(&bytes).is_err() {
                    nonconformant += 1;
                }
                let pcm = decode_all_frames(&bytes).unwrap_or_else(|e| panic!("{tag}: {e}"));
                assert_eq!(pcm.len(), channels, "{tag}");
                let n = pcm.iter().map(Vec::len).min().unwrap_or(0);
                let ours: Vec<f64> = (0..n).flat_map(|i| pcm.iter().map(move |c| c[i])).collect();
                assert!(ours.iter().any(|&s| s.abs() > 0.03), "{tag}: silence");
                let theirs = blackbox_decode_ffmpeg(&dir, sr, channels, kbps)
                    .unwrap_or_else(|| panic!("{tag}: black-box decoder A failed"));
                assert_close(
                    &format!("{tag} vs A"),
                    &ours,
                    &theirs,
                    2.0,
                    1.0 / 12f64.sqrt(),
                );
                if mpg123 {
                    let ours16: Vec<f64> = to_s16_interleaved(&pcm)
                        .iter()
                        .map(|&s| f64::from(s) / 32768.0)
                        .collect();
                    let theirs: Vec<f64> = blackbox_decode_mpg123(&dir, sr, channels, kbps)
                        .unwrap_or_else(|| panic!("{tag}: black-box decoder B failed"))
                        .iter()
                        .map(|&s| f64::from(s) / 32768.0)
                        .collect();
                    assert_close(&format!("{tag} vs B"), &ours16, &theirs, 1.0, 0.5);
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("matrix: {written} cells written, {nonconformant} outside the mode matrix");
    Some((written, nonconformant))
}

#[test]
fn blackbox_matrix_mpeg1_every_rate_and_mode() {
    let Some((written, nonconformant)) = run_matrix("mpeg1", &MPEG1_RATES, &LADDER_KBPS) else {
        return;
    };
    assert!(written > 0, "the black-box encoder wrote nothing");
    // The reported stream (mono, default bitrate) is one of these.
    assert!(
        nonconformant > 0,
        "expected the black-box encoder to write cells outside the matrix"
    );
}

#[test]
fn blackbox_matrix_mpeg2_lsf_every_rate_and_mode() {
    const LSF_KBPS: [u32; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let Some((written, _)) = run_matrix("lsf", &[24_000, 22_050, 16_000], &LSF_KBPS) else {
        return;
    };
    assert!(written > 0, "the black-box encoder wrote nothing");
}
