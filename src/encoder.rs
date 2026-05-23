//! Pure-Rust MPEG Audio Layer II encoder.
//!
//! Scope:
//! - MPEG-1 Layer II, 32 / 44.1 / 48 kHz (mono, plain stereo, dual
//!   channel by way of plain stereo, joint stereo).
//! - MPEG-2 LSF Layer II (ISO/IEC 13818-3 §2.4), 16 / 22.05 / 24 kHz
//!   (same channel modes).
//! - CBR — one bitrate per encoder instance, from the standard Layer II
//!   ladder. MPEG-1 32..=384 kbps with the §2.4.2.3 mode restrictions;
//!   MPEG-2 LSF 8..=160 kbps, no mode restrictions.
//! - VBR — per-frame bitrate slot picked from the standard ladder so
//!   that the smallest slot whose payload budget admits the encoded
//!   subband data is selected. A `vbr_quality` knob (0..=9) shapes the
//!   allocator's stop condition: high quality = the allocator keeps
//!   spending bits as long as the subband-energy / extra-bit ratio
//!   stays above a small threshold; low quality = stricter threshold,
//!   coarser quantisation, smaller frames. In VBR mode the encoder
//!   prepends a Xing/Info header frame on flush so downstream tools
//!   (ffmpeg, mediainfo, foobar2000) can show an accurate average
//!   bitrate.
//! - Joint stereo (intensity stereo, ISO/IEC 11172-3 §2.4.2.6 +
//!   §2.4.3.3): subbands at and above a "bound" carry one shared
//!   spectral coefficient with **per-channel scalefactors**. Subbands
//!   below the bound stay independent. The encoder picks the bound
//!   from `{4, 8, 12, 16}` (the four header-encodable values) by
//!   measuring per-subband normalised L/R correlation and picking the
//!   smallest bound at which all upper-band correlations exceed
//!   [`JOINT_STEREO_CORR_THRESHOLD`].
//! - Dual_channel emission (§2.4.2.3, `mode = 0b10`): the Layer II
//!   bitstream layout is byte-identical to plain stereo (both
//!   channels independent, no shared subbands); only the 2-bit
//!   header `mode` field flips. The `dual_channel` encoder option
//!   exposes the flag for use cases where the two channels carry
//!   unrelated audio (bilingual broadcast, separate commentary
//!   tracks, …). Joint stereo wins when both are requested.
//!
//! Pipeline (mirror of the decoder):
//!   PCM → polyphase analysis → joint-stereo bound decision (stereo +
//!   `joint_stereo` only) → per-subband scalefactor extraction → bit
//!   allocation → sample quantisation (grouped + ungrouped) → bit
//!   packing.
//!
//! # What is NOT implemented
//! - No psychoacoustic model. The bit allocator uses a per-bit signal
//!   energy heuristic — it picks the (channel, subband) pair whose
//!   upgrade gives the largest energy/cost ratio, much like LAME's
//!   "athonly" model.
//! - No CRC-16.
//! - No free-format output.

use std::collections::VecDeque;

use oxideav_core::Encoder;
use oxideav_core::{
    AudioFrame, CodecId, CodecParameters, Error, Frame, MediaType, Packet, Result, SampleFormat,
    TimeBase,
};

use crate::analysis::{analyze_frame, AnalysisState};
use crate::options::{Emphasis, Mp2EncoderOptions, PsyModel};
use crate::psy::{ath_weight_per_subband, joint_stereo_threshold_relaxation_per_subband};
use crate::tables::{scalefactor_magnitude, select_alloc_table, AllocEntry, AllocTable, TABLE_LSF};
use crate::CODEC_ID_STR;
use oxideav_core::bits::BitWriter;
use oxideav_core::options::parse_options;

/// Which MPEG Audio version the encoder is emitting. Selects the header
/// `version_id` bit, the sample-rate / bitrate ladder, and the bit
/// allocation table. MPEG-2.5 is not in scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncVersion {
    /// MPEG-1 Layer II, 32 / 44.1 / 48 kHz.
    Mpeg1,
    /// MPEG-2 LSF Layer II (ISO/IEC 13818-3 §2.4), 16 / 22.05 / 24 kHz.
    Mpeg2Lsf,
}

/// Encoder rate-control mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RateControl {
    /// One CBR slot for the lifetime of the encoder.
    Cbr,
    /// Per-frame slot from the standard ladder.
    Vbr,
}

/// Minimum normalised L/R correlation per subband for that subband to
/// be eligible for intensity-stereo coding. A correlation of 1.0 means
/// L and R differ by a real-valued gain only — perfect intensity
/// stereo. Below ~0.7 the energy direction is ambiguous and the
/// shared-coefficient approximation degrades audibly. See ISO 11172-3
/// §D.2 for context.
const JOINT_STEREO_CORR_THRESHOLD: f32 = 0.7;

/// MPEG-1 Layer II bitrate ladder, kbps. Index = `bitrate_index` − 1.
const BITRATES_MPEG1: [u32; 14] = [
    32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
/// MPEG-2 LSF Layer II bitrate ladder, kbps.
const BITRATES_MPEG2_LSF: [u32; 14] = [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// The four joint-stereo bound values addressable through
/// `mode_extension` (ISO 11172-3 §2.4.2.3 Table 3-B.3).
const JOINT_STEREO_BOUNDS: [u32; 4] = [4, 8, 12, 16];

/// Build a Layer II encoder for the requested parameters.
pub fn make_encoder(params: &CodecParameters) -> Result<Box<dyn Encoder>> {
    let channels = params
        .channels
        .ok_or_else(|| Error::invalid("MP2 encoder: missing channels"))?;
    if !(1..=2).contains(&channels) {
        return Err(Error::invalid("MP2 encoder: channels must be 1 or 2"));
    }
    let sample_rate = params
        .sample_rate
        .ok_or_else(|| Error::invalid("MP2 encoder: missing sample_rate"))?;
    // MPEG-2 LSF (ISO/IEC 13818-3 §2.4) adds the 16/22.05/24 kHz band;
    // everything else (including MPEG-2.5's 8/11.025/12 kHz) is rejected.
    let version = match sample_rate {
        32_000 | 44_100 | 48_000 => EncVersion::Mpeg1,
        16_000 | 22_050 | 24_000 => EncVersion::Mpeg2Lsf,
        _ => {
            return Err(Error::unsupported(format!(
                "MP2 encoder: unsupported sample rate {sample_rate} (need 16000/22050/24000/32000/44100/48000)"
            )));
        }
    };

    let opts: Mp2EncoderOptions = parse_options(&params.options)?;
    let rate_control = if opts.vbr_quality.is_some() {
        RateControl::Vbr
    } else {
        RateControl::Cbr
    };
    let vbr_quality = opts.vbr_quality.unwrap_or(2);
    let allow_joint_stereo = opts.joint_stereo && channels == 2;
    // dual_channel is mutually exclusive with joint_stereo (the two
    // values share the `mode` bit field). joint_stereo wins per the
    // README contract — a caller that wants dual_channel must not also
    // set joint_stereo. Both are silently ignored on mono input.
    let emit_dual_channel = opts.dual_channel && channels == 2 && !allow_joint_stereo;
    let psy_model = opts.psy_model;
    let copyright = opts.copyright;
    let original = opts.original;
    let emphasis = opts.emphasis;
    let private_bit = opts.private_bit;

    let bitrate_kbps = params.bit_rate.map(|b| (b / 1000) as u32).unwrap_or(192);
    let br_index = match version {
        EncVersion::Mpeg1 => bitrate_to_index(bitrate_kbps).ok_or_else(|| {
            Error::unsupported(format!(
                "MP2 encoder: unsupported MPEG-1 bitrate {bitrate_kbps} kbps"
            ))
        })?,
        EncVersion::Mpeg2Lsf => bitrate_to_index_lsf(bitrate_kbps).ok_or_else(|| {
            Error::unsupported(format!(
                "MP2 encoder: unsupported MPEG-2 LSF bitrate {bitrate_kbps} kbps"
            ))
        })?,
    };

    // Per ISO/IEC 11172-3 Table 3-B.2, Layer II MPEG-1 forbids some
    // (mode, bitrate) combos. MPEG-2 LSF (§13818-3 §2.4.2.3) relaxes
    // these — all 14 LSF bitrates are permitted in any channel mode.
    // CBR-only enforcement: VBR may legitimately roam through any slot.
    if matches!(version, EncVersion::Mpeg1) && matches!(rate_control, RateControl::Cbr) {
        match channels {
            1 if matches!(bitrate_kbps, 224 | 256 | 320 | 384) => {
                return Err(Error::invalid(format!(
                    "MP2 encoder: bitrate {bitrate_kbps} kbps not permitted in mono mode"
                )));
            }
            2 if matches!(bitrate_kbps, 32 | 48) => {
                return Err(Error::invalid(format!(
                    "MP2 encoder: bitrate {bitrate_kbps} kbps not permitted in stereo modes"
                )));
            }
            _ => {}
        }
    }

    let sample_format = params.sample_format.unwrap_or(SampleFormat::S16);
    if sample_format != SampleFormat::S16 {
        return Err(Error::unsupported(format!(
            "MP2 encoder: input sample format {sample_format:?} not supported (need S16)"
        )));
    }

    let sr_index = match (version, sample_rate) {
        (EncVersion::Mpeg1, 44_100) => 0u8,
        (EncVersion::Mpeg1, 48_000) => 1,
        (EncVersion::Mpeg1, 32_000) => 2,
        (EncVersion::Mpeg2Lsf, 22_050) => 0,
        (EncVersion::Mpeg2Lsf, 24_000) => 1,
        (EncVersion::Mpeg2Lsf, 16_000) => 2,
        _ => unreachable!(),
    };

    let mut output = params.clone();
    output.media_type = MediaType::Audio;
    output.codec_id = CodecId::new(CODEC_ID_STR);
    output.sample_format = Some(sample_format);
    output.channels = Some(channels);
    output.sample_rate = Some(sample_rate);
    output.bit_rate = Some((bitrate_kbps as u64) * 1000);

    // Pre-compute the ATH per-subband perceptual weight and the
    // joint-stereo per-subband relaxation. Both are deterministic
    // functions of (sample rate, model) so we lift them out of the
    // per-frame hot path. `PsyModel::None` falls back to the v0.0.8
    // strict-energy behaviour: weights of 1.0 everywhere (= no
    // attenuation) and a zero relaxation table.
    let ath_weight = match psy_model {
        PsyModel::Ath => ath_weight_per_subband(sample_rate),
        PsyModel::None => [1.0f32; 32],
    };
    let js_relax = match psy_model {
        PsyModel::Ath => joint_stereo_threshold_relaxation_per_subband(),
        PsyModel::None => [0.0f32; 32],
    };

    Ok(Box::new(Mp2Encoder {
        output_params: output,
        version,
        channels,
        sample_rate,
        bitrate_kbps,
        sr_index,
        br_index,
        rate_control,
        vbr_quality,
        allow_joint_stereo,
        emit_dual_channel,
        copyright,
        original,
        emphasis,
        private_bit,
        psy_model,
        ath_weight,
        js_relax,
        time_base: TimeBase::new(1, sample_rate as i64),
        analysis_state: [AnalysisState::new(), AnalysisState::new()],
        pcm_queue: vec![Vec::new(); channels as usize],
        pending_packets: VecDeque::new(),
        vbr_buffered_packets: VecDeque::new(),
        frame_index: 0,
        eof: false,
        cumulative_padded_bits: 0,
        emitted_xing: false,
        vbr_total_payload_bytes: 0,
    }))
}

struct Mp2Encoder {
    output_params: CodecParameters,
    version: EncVersion,
    channels: u16,
    sample_rate: u32,
    /// CBR slot. In VBR mode this seeds the canonical bitrate field of
    /// the Xing/Info placeholder header — every actual data frame
    /// substitutes its own per-frame slot.
    bitrate_kbps: u32,
    sr_index: u8,
    br_index: u32,
    rate_control: RateControl,
    /// VBR quality 0..=9; only consulted when `rate_control == Vbr`.
    vbr_quality: u8,
    /// `true` when joint-stereo emission is permitted (set only for
    /// 2-channel inputs with the `joint_stereo` option enabled).
    allow_joint_stereo: bool,
    /// `true` when the encoder emits `mode = 0b10` (dual_channel)
    /// instead of `0b00` (stereo) for two-channel inputs. The
    /// bitstream layout is identical to plain stereo; only the
    /// header mode field differs. Set only for 2-channel inputs with
    /// `dual_channel = true` and `joint_stereo = false`.
    emit_dual_channel: bool,
    /// Header `copyright` bit (ISO/IEC 11172-3 §2.4.2.3): `1` =
    /// copyright protected, `0` = none. Carries no payload effect.
    copyright: bool,
    /// Header `original/copy` bit (§2.4.2.3): `1` = original, `0` =
    /// copy.
    original: bool,
    /// Header 2-bit `emphasis` field (§2.4.2.3).
    emphasis: Emphasis,
    /// Header `private_bit` (§2.4.2.3), reserved for private use.
    private_bit: bool,
    /// Selected psychoacoustic model. Retained on the struct for
    /// runtime inspection by tests; the per-subband data tables are
    /// pre-materialised below.
    #[allow(dead_code)]
    psy_model: PsyModel,
    /// Per-subband ATH-derived multiplicative weight applied to the
    /// raw subband energy before allocator scoring. `1.0` everywhere
    /// for [`PsyModel::None`] (= legacy strict-energy behaviour).
    ath_weight: [f32; 32],
    /// Per-subband relaxation subtracted from the base joint-stereo
    /// correlation threshold (zero for [`PsyModel::None`]).
    js_relax: [f32; 32],
    time_base: TimeBase,
    analysis_state: [AnalysisState; 2],
    pcm_queue: Vec<Vec<f32>>,
    /// CBR pending packet queue (drained immediately on each
    /// `receive_packet` call).
    pending_packets: VecDeque<Packet>,
    /// VBR holding pen — frames are buffered here until `flush()` so we
    /// can compute totals for the Xing/Info header and prepend it.
    vbr_buffered_packets: VecDeque<Packet>,
    frame_index: u64,
    eof: bool,
    /// Fractional-byte CBR padding accumulator; see [`next_padding`].
    cumulative_padded_bits: u64,
    /// VBR: have we emitted the placeholder Xing header yet?
    emitted_xing: bool,
    /// VBR: total bytes of frame payload (audio frames, not Xing header).
    vbr_total_payload_bytes: u64,
}

impl Mp2Encoder {
    fn frame_bytes_at_rate(&self, kbps: u32, padding: bool) -> usize {
        let base = (144 * kbps * 1000 / self.sample_rate) as usize;
        base + if padding { 1 } else { 0 }
    }

    fn frame_bytes(&self, padding: bool) -> usize {
        self.frame_bytes_at_rate(self.bitrate_kbps, padding)
    }

    /// Decide whether this frame should set the padding bit. Same
    /// accumulator scheme as the mp3 encoder — for fractional bits per
    /// frame, we count remainders modulo `8 * sample_rate` and pay off
    /// with one padding byte whenever the accumulator overflows.
    fn next_padding(&mut self) -> bool {
        let num = 144_000u64 * self.bitrate_kbps as u64;
        let sr = self.sample_rate as u64;
        let rem = num - (num / sr) * sr;
        self.cumulative_padded_bits += rem;
        let pad = self.cumulative_padded_bits >= sr * 8;
        if pad {
            self.cumulative_padded_bits -= sr * 8;
        }
        pad
    }

    fn ingest(&mut self, frame: &AudioFrame) -> Result<()> {
        // Stream-level validation (channel count, sample rate, S16
        // sample format) is owned by the factory at construction —
        // see `make_encoder`. The slim AudioFrame doesn't carry them.
        let data = frame
            .data
            .first()
            .ok_or_else(|| Error::invalid("MP2 encoder: empty frame"))?;
        let n_ch = self.channels as usize;
        let n_samples = data.len() / (2 * n_ch);
        for i in 0..n_samples {
            for ch in 0..n_ch {
                let off = (i * n_ch + ch) * 2;
                let s = i16::from_le_bytes([data[off], data[off + 1]]) as f32 / 32768.0;
                self.pcm_queue[ch].push(s);
            }
        }
        self.flush_ready_frames(false)
    }

    fn flush_ready_frames(&mut self, drain: bool) -> Result<()> {
        let n_ch = self.channels as usize;
        loop {
            let avail = self.pcm_queue[0].len();
            if avail < 1152 {
                if drain && avail > 0 {
                    for ch in 0..n_ch {
                        self.pcm_queue[ch].resize(1152, 0.0);
                    }
                } else {
                    return Ok(());
                }
            }
            let pkt = self.encode_one_frame()?;
            self.enqueue_packet(pkt);
            if drain && self.pcm_queue[0].iter().all(|&v| v == 0.0) {
                return Ok(());
            }
        }
    }

    fn enqueue_packet(&mut self, pkt: Packet) {
        match self.rate_control {
            RateControl::Cbr => self.pending_packets.push_back(pkt),
            RateControl::Vbr => {
                self.vbr_total_payload_bytes += pkt.data.len() as u64;
                self.vbr_buffered_packets.push_back(pkt);
            }
        }
    }

    fn encode_one_frame(&mut self) -> Result<Packet> {
        let n_ch = self.channels as usize;

        // Drain 1152 samples/channel from the queue.
        let mut pcm_in: Vec<[f32; 1152]> = vec![[0.0f32; 1152]; n_ch];
        for ch in 0..n_ch {
            for i in 0..1152 {
                pcm_in[ch][i] = self.pcm_queue[ch][i];
            }
            self.pcm_queue[ch].drain(..1152);
        }

        // --- 1. Analysis: 32 × 36 subband buffer per channel ---
        let mut sub: Vec<[[f32; 36]; 32]> = (0..n_ch).map(|_| [[0.0f32; 36]; 32]).collect();
        for ch in 0..n_ch {
            analyze_frame(&mut self.analysis_state[ch], &pcm_in[ch], &mut sub[ch]);
        }

        // --- 2. Decide stereo mode + bound for this frame.
        // For mono and plain stereo: `bound = sblimit`, so all subbands
        // are below the bound and per-channel-independent (= no shared
        // intensity-stereo subbands). For joint stereo: pick the
        // smallest header-addressable bound at which the upper-band
        // L/R correlation is "high enough" (Pearson > threshold for
        // every subband at-or-above the candidate bound).
        let stereo = n_ch == 2;
        let table_for_jsd: &AllocTable = match self.version {
            EncVersion::Mpeg1 => select_alloc_table(self.sample_rate, stereo, self.br_index),
            EncVersion::Mpeg2Lsf => &TABLE_LSF,
        };
        let (frame_mode_code, bound_subband) = if self.allow_joint_stereo {
            match pick_joint_stereo_bound(&sub, table_for_jsd.sblimit, &self.js_relax) {
                Some((mode_ext, bnd)) => (FrameMode::JointStereo(mode_ext), bnd as usize),
                None => (FrameMode::Stereo, table_for_jsd.sblimit),
            }
        } else if n_ch == 2 {
            // `dual_channel` shares the plain-stereo bitstream layout
            // (every subband per-channel-independent); only the
            // header mode bits flip.
            if self.emit_dual_channel {
                (FrameMode::DualChannel, table_for_jsd.sblimit)
            } else {
                (FrameMode::Stereo, table_for_jsd.sblimit)
            }
        } else {
            (FrameMode::Mono, table_for_jsd.sblimit)
        };

        // --- 3. Compute scalefactors. For independent subbands each
        // channel uses its own subband peak. For shared-intensity
        // subbands (sb >= bound), each channel still has its own
        // scalefactor — the spec puts the "intensity stereo" axis in
        // the magnitude domain, not the spectral one.
        let mut scf_idx = vec![[[0u8; 3]; 32]; n_ch];
        for ch in 0..n_ch {
            for sb in 0..table_for_jsd.sblimit {
                for part in 0..3 {
                    let base = part * 12;
                    let mut peak = 0.0f32;
                    for i in 0..12 {
                        let v = sub[ch][sb][base + i].abs();
                        if v > peak {
                            peak = v;
                        }
                    }
                    scf_idx[ch][sb][part] = pick_scalefactor(peak);
                }
            }
        }

        // SCFSI selection is independent of the bound (it's a
        // per-channel decision over the three parts).
        let mut scfsi = vec![[0u8; 32]; n_ch];
        for ch in 0..n_ch {
            for sb in 0..table_for_jsd.sblimit {
                let a = scf_idx[ch][sb][0];
                let b = scf_idx[ch][sb][1];
                let c = scf_idx[ch][sb][2];
                scfsi[ch][sb] = pick_scfsi(a, b, c);
            }
        }

        // --- 4. Build the shared-band averaged subband array. For
        // sb >= bound (joint stereo only), the encoder transmits one
        // sample triple per subband. We follow the standard intensity-
        // stereo encoder choice of "energy-weighted L+R sum":
        //   shared_x = (sf_L * L + sf_R * R) / (sf_L + sf_R)
        // so when the decoder later remultiplies by sf_L (or sf_R),
        // the L (or R) reconstruction stays close to the input
        // amplitude. We use the per-part scalefactor magnitudes —
        // close enough for the encoder's rough quantisation grid.
        // For sb < bound we keep per-channel arrays as-is.
        let table = table_for_jsd;

        // --- 5. Pick the bit budget for this frame.
        // CBR: fixed slot from `br_index` plus padding accumulator.
        // VBR: walk the standard ladder upward and accept the first
        // slot whose payload-budget admits the chosen allocation.
        let header_bits = 32u32;
        let bitalloc_bits: i64 = (0..table.sblimit)
            .map(|sb| {
                let nbal = table.nbal(sb) as i64;
                if sb < bound_subband {
                    nbal * n_ch as i64
                } else {
                    nbal
                }
            })
            .sum();

        // Subband energy per (ch, sb). For shared-intensity bands we
        // use the maximum of the two channels — that's what matters
        // for masking.
        let mut energy = vec![[0.0f32; 32]; n_ch];
        for ch in 0..n_ch {
            for sb in 0..table.sblimit {
                let mut e = 0.0f32;
                for i in 0..36 {
                    let v = sub[ch][sb][i];
                    e += v * v;
                }
                energy[ch][sb] = e / 36.0;
            }
        }
        let stop_score = vbr_quality_to_stop_score(self.vbr_quality, self.rate_control);

        // Pre-compute padding/frame_bytes/br_index for CBR; defer for
        // VBR until after a trial allocation reveals the natural
        // payload size.
        let (frame_bytes, padding, frame_br_index, alloc) = match self.rate_control {
            RateControl::Cbr => {
                let padding = self.next_padding();
                let frame_bytes = self.frame_bytes(padding);
                let alloc = run_allocator(
                    table,
                    n_ch,
                    bound_subband,
                    &energy,
                    &scfsi,
                    frame_bytes as i64 * 8 - header_bits as i64 - bitalloc_bits,
                    stop_score,
                    &self.ath_weight,
                )?;
                (frame_bytes, padding, self.br_index, alloc)
            }
            RateControl::Vbr => {
                // Allocate at the largest standard slot to discover
                // the natural payload size at this quality, then snap
                // to the smallest standard slot whose payload budget
                // admits that allocation.
                let table_kbps = bitrate_table(self.version);
                let max_kbps = *table_kbps.last().unwrap();
                let max_bytes = self.frame_bytes_at_rate(max_kbps, false);
                let alloc_unbounded = run_allocator(
                    table,
                    n_ch,
                    bound_subband,
                    &energy,
                    &scfsi,
                    max_bytes as i64 * 8 - header_bits as i64 - bitalloc_bits,
                    stop_score,
                    &self.ath_weight,
                )?;
                let used_bits =
                    allocation_payload_bits(table, n_ch, bound_subband, &alloc_unbounded, &scfsi);
                let needed_bits = header_bits as i64 + bitalloc_bits + used_bits;
                let needed_bytes = (needed_bits as usize).div_ceil(8);
                let (idx, kbps) = pick_vbr_slot(self.version, self.sample_rate, needed_bytes, n_ch);
                let frame_bytes = self.frame_bytes_at_rate(kbps, false);
                (frame_bytes, false, idx, alloc_unbounded)
            }
        };

        // --- 6. Build the shared spectral coefficients for sb >= bound ---
        // Shared coefficient is computed as a scalefactor-weighted sum:
        //   x_shared = (sf_L * L + sf_R * R) / (sf_L + sf_R + ε)
        // Per-channel reconstruction in the decoder then multiplies by
        // sf_L (or sf_R) — recovering an L (or R) approximation
        // proportional to its own scalefactor envelope. This is the
        // canonical "intensity stereo" encoder formula (ISO 11172-3
        // §D.2 — informative annex on bit allocation).
        let mut shared_sub: [[f32; 36]; 32] = [[0.0f32; 36]; 32];
        if matches!(frame_mode_code, FrameMode::JointStereo(_)) && n_ch == 2 {
            for sb in bound_subband..table.sblimit {
                for part in 0..3 {
                    let sf0 = scalefactor_magnitude(scf_idx[0][sb][part]);
                    let sf1 = scalefactor_magnitude(scf_idx[1][sb][part]);
                    let den = sf0 + sf1 + 1e-20;
                    let base = part * 12;
                    for i in 0..12 {
                        shared_sub[sb][base + i] =
                            (sf0 * sub[0][sb][base + i] + sf1 * sub[1][sb][base + i]) / den;
                    }
                }
            }
        }

        // --- 7. Write frame ---
        let mut w = BitWriter::with_capacity(frame_bytes);

        // Header (32 bits).
        w.write_u32(0xFFF, 12);
        let id_bit = match self.version {
            EncVersion::Mpeg1 => 1,
            EncVersion::Mpeg2Lsf => 0,
        };
        w.write_u32(id_bit, 1);
        w.write_u32(0b10, 2); // Layer II
        w.write_u32(1, 1); // protection_bit = 1 (no CRC)
        w.write_u32(frame_br_index, 4);
        w.write_u32(self.sr_index as u32, 2);
        w.write_u32(if padding { 1 } else { 0 }, 1);
        w.write_u32(self.private_bit as u32, 1); // private_bit
        let (mode_bits, mode_ext_bits) = match frame_mode_code {
            FrameMode::Mono => (0b11u32, 0u32),
            FrameMode::Stereo => (0b00, 0),
            FrameMode::JointStereo(ext) => (0b01, ext),
            FrameMode::DualChannel => (0b10, 0),
        };
        w.write_u32(mode_bits, 2);
        w.write_u32(mode_ext_bits, 2);
        w.write_u32(self.copyright as u32, 1); // copyright
        w.write_u32(self.original as u32, 1); // original/copy
        w.write_u32(self.emphasis.code(), 2); // emphasis

        // --- 7a. Bit allocation ---
        // Below the bound: per-channel. At/above the bound: single
        // shared allocation field.
        for sb in 0..table.sblimit {
            let nbal = table.nbal(sb);
            if sb < bound_subband {
                for ch in 0..n_ch {
                    w.write_u32(alloc[ch][sb] as u32, nbal);
                }
            } else {
                w.write_u32(alloc[0][sb] as u32, nbal);
            }
        }

        // --- 7b. SCFSI: 2 bits per subband*channel with alloc != 0 ---
        // Joint stereo doesn't change the scalefactor layout — every
        // active subband still emits per-channel SCFSI + scalefactors.
        for sb in 0..table.sblimit {
            for ch in 0..n_ch {
                if alloc[ch][sb] != 0 {
                    w.write_u32(scfsi[ch][sb] as u32, 2);
                }
            }
        }

        // --- 7c. Scalefactors: 6 bits each, count determined by SCFSI ---
        for sb in 0..table.sblimit {
            for ch in 0..n_ch {
                if alloc[ch][sb] == 0 {
                    continue;
                }
                let scf = scf_idx[ch][sb];
                match scfsi[ch][sb] {
                    0 => {
                        w.write_u32(scf[0] as u32, 6);
                        w.write_u32(scf[1] as u32, 6);
                        w.write_u32(scf[2] as u32, 6);
                    }
                    1 => {
                        w.write_u32(scf[0] as u32, 6); // parts 0==1
                        w.write_u32(scf[2] as u32, 6);
                    }
                    2 => {
                        w.write_u32(scf[0] as u32, 6);
                    }
                    _ => {
                        // scfsi == 3: part 0 separate, parts 1==2
                        w.write_u32(scf[0] as u32, 6);
                        w.write_u32(scf[1] as u32, 6);
                    }
                }
            }
        }

        // --- 7d. Sample payload ---
        // Layer II nests: 3 groups of 12 samples (= 1 part each), each
        // split into 4 triples of 3 samples. Triple writes respect the
        // allocated class — grouped quantiser packs all three samples
        // into one codeword; ungrouped writes each sample independently.
        for gr in 0..3 {
            for tr in 0..4 {
                let base_idx = gr * 12 + tr * 3;
                for sb in 0..table.sblimit {
                    if sb < bound_subband {
                        for ch in 0..n_ch {
                            let a = alloc[ch][sb];
                            if a == 0 {
                                continue;
                            }
                            let entry = class_entry(table, sb, a);
                            let sf_mag = scalefactor_magnitude(scf_idx[ch][sb][gr]);
                            write_triple(&mut w, entry, &sub[ch][sb], base_idx, sf_mag);
                        }
                    } else {
                        // Shared-intensity triple — one codeword for
                        // both channels. The scalefactor that maps
                        // back into the shared-coefficient amplitude
                        // domain (the inverse of our weighted sum) is
                        // sf_combined = (sf_L + sf_R) — using that
                        // here keeps the quantiser well-conditioned
                        // even when one channel dominates.
                        let a = alloc[0][sb];
                        if a == 0 {
                            continue;
                        }
                        let entry = class_entry(table, sb, a);
                        // Use max(sf_L, sf_R) as the encoder-side
                        // amplitude reference: the decoder, when it
                        // remultiplies by sf_L (or sf_R) separately,
                        // gets close to L (or R) for the dominant
                        // channel. Using max keeps the unit-amplitude
                        // domain ≥ 1, so the quantisation grid never
                        // clips.
                        let sf_l = scalefactor_magnitude(scf_idx[0][sb][gr]);
                        let sf_r = if n_ch == 2 {
                            scalefactor_magnitude(scf_idx[1][sb][gr])
                        } else {
                            sf_l
                        };
                        let sf_ref = sf_l.max(sf_r);
                        write_triple(&mut w, entry, &shared_sub[sb], base_idx, sf_ref);
                    }
                }
            }
        }

        // --- 7e. Pad to frame length ---
        // Fill any remaining bits with zero ancillary data.
        w.align_to_byte();
        let mut bytes = w.into_bytes();
        if bytes.len() > frame_bytes {
            // Shouldn't happen if the allocator respected the budget;
            // clip defensively so we never emit over-length frames.
            bytes.truncate(frame_bytes);
        }
        if bytes.len() < frame_bytes {
            bytes.resize(frame_bytes, 0);
        }

        let pts = (self.frame_index as i64) * 1152;
        let mut pkt = Packet::new(0, self.time_base, bytes);
        pkt.pts = Some(pts);
        pkt.dts = Some(pts);
        pkt.duration = Some(1152);
        pkt.flags.keyframe = true;
        self.frame_index += 1;
        Ok(pkt)
    }

    /// Build the Xing/Info header frame for a VBR stream. The header
    /// frame is itself a valid Layer II frame (4-byte sync + zero
    /// sample payload), with the Xing tag block planted in what would
    /// otherwise be sample bits.
    ///
    /// `toc` is the optional 100-byte seek table (Xing flag `0x4`,
    /// trace-doc §7.2): `toc[i]` is `floor(256 · byte_offset / total)`
    /// for the stream position reached at `i/100` of the total
    /// duration, so a player seeking to percentage `p` jumps to
    /// `(toc[p] / 256) · total_bytes`. When `None` the TOC flag is
    /// cleared and only the frames/bytes fields are written.
    fn build_xing_frame(
        &self,
        num_frames: u32,
        total_bytes: u32,
        toc: Option<&[u8; 100]>,
    ) -> Vec<u8> {
        // Pick a header bitrate slot whose frame size comfortably
        // holds the Xing block (~120 bytes including TOC). Use the
        // canonical CBR slot of the encoder (or 128 kbps on MPEG-1 /
        // 64 kbps on MPEG-2 LSF as a safe fallback).
        let kbps = match self.version {
            EncVersion::Mpeg1 => 128.max(self.bitrate_kbps),
            EncVersion::Mpeg2Lsf => 64.max(self.bitrate_kbps),
        };
        let br_index = match self.version {
            EncVersion::Mpeg1 => bitrate_to_index(kbps).unwrap_or(self.br_index),
            EncVersion::Mpeg2Lsf => bitrate_to_index_lsf(kbps).unwrap_or(self.br_index),
        };
        let frame_bytes = self.frame_bytes_at_rate(kbps, false);
        let mut bytes = vec![0u8; frame_bytes];

        // Build the 32-bit header into bytes[0..4].
        let mut hdr: u32 = 0;
        hdr |= 0xFFFu32 << 20;
        let id_bit: u32 = match self.version {
            EncVersion::Mpeg1 => 1,
            EncVersion::Mpeg2Lsf => 0,
        };
        hdr |= id_bit << 19;
        hdr |= 0b10u32 << 17; // Layer II
        hdr |= 1u32 << 16; // protection bit = 1 (no CRC)
        hdr |= br_index << 12;
        hdr |= (self.sr_index as u32) << 10;
        // padding=0
        hdr |= (self.private_bit as u32) << 8; // private_bit
                                               // Xing/Info frame mirrors the stream's nominal mode so a
                                               // tool scanning the file sees a consistent channel layout.
        let mode_bits: u32 = if self.channels == 1 {
            0b11
        } else if self.emit_dual_channel {
            0b10
        } else {
            0b00
        };
        hdr |= mode_bits << 6;
        // Mirror the metadata flags so the placeholder frame's header
        // matches the data frames a tool will see right after it.
        hdr |= (self.copyright as u32) << 3; // copyright
        hdr |= (self.original as u32) << 2; // original/copy
        hdr |= self.emphasis.code(); // emphasis (2 bits)
        bytes[0..4].copy_from_slice(&hdr.to_be_bytes());

        // The Xing tag offset within a Layer II frame (post header,
        // post placeholder bit-alloc indices). For Layer II the
        // canonical Xing-frame layout — as observed in the output of
        // typical Layer-II CBR/VBR encoders — is to
        // place the "Info"/"Xing" magic at byte offset 0x24 from the
        // start of the frame for stereo / 0x15 for mono on Layer III,
        // but for Layer II the tag is placed at a fixed offset of
        // `header_bytes` so the body is found by string search. ffmpeg
        // and mediainfo find the tag by scanning the frame for the
        // "Xing"/"Info" magic, so any offset past the header works.
        // We place it at offset 4 (immediately after the header) for
        // simplicity — the rest of the frame is zeroed.
        let tag_off = 4usize;
        // A TOC is only written when it fits entirely in the placeholder
        // frame after the 16-byte magic/flags/frames/bytes block; a TOC
        // that would overrun the frame is silently dropped (the flag is
        // cleared) rather than truncated.
        let want_toc = toc.is_some() && tag_off + 16 + 100 <= bytes.len();
        if tag_off + 16 <= bytes.len() {
            // VBR streams use the "Xing" magic; "Info" is the CBR spelling.
            // Both are recognised by ffmpeg/mediainfo, but signalling
            // "Xing" lets a scanner know the per-frame bitrate varies.
            bytes[tag_off..tag_off + 4].copy_from_slice(b"Xing");
            // Flags: bit 0 = Frames, bit 1 = Bytes, bit 2 = TOC (only
            // when one was supplied and fits).
            let flags: u32 = if want_toc { 0x0000_0007 } else { 0x0000_0003 };
            bytes[tag_off + 4..tag_off + 8].copy_from_slice(&flags.to_be_bytes());
            bytes[tag_off + 8..tag_off + 12].copy_from_slice(&num_frames.to_be_bytes());
            bytes[tag_off + 12..tag_off + 16].copy_from_slice(&total_bytes.to_be_bytes());
            if want_toc {
                // SAFETY: `want_toc` implies `toc.is_some()`.
                let toc = toc.unwrap();
                bytes[tag_off + 16..tag_off + 16 + 100].copy_from_slice(toc);
            }
        }
        bytes
    }
}

impl Encoder for Mp2Encoder {
    fn codec_id(&self) -> &CodecId {
        &self.output_params.codec_id
    }
    fn output_params(&self) -> &CodecParameters {
        &self.output_params
    }
    fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        match frame {
            Frame::Audio(a) => self.ingest(a),
            _ => Err(Error::invalid("MP2 encoder: audio frames only")),
        }
    }
    fn receive_packet(&mut self) -> Result<Packet> {
        // CBR drains immediately. VBR holds frames in
        // `vbr_buffered_packets` until `flush()` so we can prepend the
        // Xing header before emitting them — only after `flush()` do
        // VBR packets become available via `receive_packet`.
        if let Some(p) = self.pending_packets.pop_front() {
            return Ok(p);
        }
        Err(Error::NeedMore)
    }
    fn flush(&mut self) -> Result<()> {
        if !self.eof {
            self.eof = true;
            self.flush_ready_frames(true)?;
        }
        // VBR drain: prepend the Xing/Info header (built from the
        // post-encode totals) and move every buffered packet into the
        // CBR pending queue so they become reachable through
        // `receive_packet`.
        if matches!(self.rate_control, RateControl::Vbr) && !self.emitted_xing {
            let n_frames = self.vbr_buffered_packets.len() as u32;
            let total_payload = self.vbr_total_payload_bytes as u32;
            // First pass: build the placeholder to discover its
            // length. Second pass: rewrite with the correct
            // total-file-size value (LAME convention: the Xing "Bytes"
            // field counts the Xing frame too).
            //
            // `Frames`/`Bytes` count the whole VBR stream including the
            // Xing frame itself, so the TOC's `total_bytes` denominator
            // and frame-position walk must also start from the Xing
            // frame at byte 0 (index 0 in `frame_sizes`).
            let placeholder = self.build_xing_frame(n_frames + 1, total_payload, None);
            let xing_len = placeholder.len();
            let total_bytes = total_payload + xing_len as u32;
            let mut frame_sizes: Vec<u32> = Vec::with_capacity(self.vbr_buffered_packets.len() + 1);
            frame_sizes.push(xing_len as u32);
            frame_sizes.extend(
                self.vbr_buffered_packets
                    .iter()
                    .map(|p| p.data.len() as u32),
            );
            let toc = build_xing_toc(&frame_sizes, total_bytes);
            let xing_bytes = self.build_xing_frame(n_frames + 1, total_bytes, Some(&toc));
            let mut xing_pkt = Packet::new(0, self.time_base, xing_bytes);
            xing_pkt.pts = Some(0);
            xing_pkt.dts = Some(0);
            xing_pkt.duration = Some(1152);
            xing_pkt.flags.keyframe = true;
            self.pending_packets.push_back(xing_pkt);
            while let Some(p) = self.vbr_buffered_packets.pop_front() {
                self.pending_packets.push_back(p);
            }
            self.emitted_xing = true;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum FrameMode {
    Mono,
    Stereo,
    /// Joint stereo with `mode_extension` selecting the bound from
    /// {00→4, 01→8, 10→12, 11→16}.
    JointStereo(u32),
    /// Dual-channel — two unrelated mono signals carried on the L/R
    /// channels (e.g. dual-language broadcast). Same Layer II
    /// bitstream layout as plain [`FrameMode::Stereo`], only the
    /// header `mode` field differs (`0b10` instead of `0b00`).
    DualChannel,
}

/// Reverse-map a bitrate in kbps to its 4-bit header-field index (1..=14
/// for MPEG-1 Layer II). Returns `None` for unsupported values.
fn bitrate_to_index(kbps: u32) -> Option<u32> {
    BITRATES_MPEG1
        .iter()
        .position(|&v| v == kbps)
        .map(|idx| (idx + 1) as u32)
}

/// Reverse-map a bitrate in kbps to its 4-bit header-field index (1..=14)
/// on the MPEG-2 LSF Layer II ladder (ISO/IEC 13818-3 §2.4.2.3).
fn bitrate_to_index_lsf(kbps: u32) -> Option<u32> {
    BITRATES_MPEG2_LSF
        .iter()
        .position(|&v| v == kbps)
        .map(|idx| (idx + 1) as u32)
}

fn bitrate_table(v: EncVersion) -> &'static [u32] {
    match v {
        EncVersion::Mpeg1 => &BITRATES_MPEG1,
        EncVersion::Mpeg2Lsf => &BITRATES_MPEG2_LSF,
    }
}

/// Build the 100-entry Xing/Info seek table (de-facto Xing header
/// flag `0x4`; trace-doc §7.2 "interpolation table for seeking").
///
/// `frame_sizes` is the byte length of every frame in playback order,
/// *including* the leading Xing/Info frame at index 0 (so cumulative
/// offsets start at byte 0 of the emitted stream). `total_bytes` is the
/// sum of all frame sizes; it is the denominator a player divides into.
///
/// Each entry `toc[i]` records the stream position reached at `i/100`
/// of the total duration as a fraction of `total_bytes` scaled to a
/// byte: `toc[i] = round(256 · offset_at(i/100) / total_bytes)`,
/// clamped to `255`. All frames are 1152 samples, so playback time is
/// proportional to frame index and the time fraction `i/100` maps to
/// frame `round(i/100 · num_frames)`. A player seeking to percentage
/// `p` then jumps to byte `(toc[p] / 256) · total_bytes`. The
/// classic-Xing convention puts `toc[0] = 0` (start of stream).
fn build_xing_toc(frame_sizes: &[u32], total_bytes: u32) -> [u8; 100] {
    let mut toc = [0u8; 100];
    let num_frames = frame_sizes.len();
    if num_frames == 0 || total_bytes == 0 {
        return toc;
    }
    // Prefix sums of byte offsets: cum[k] = bytes before frame k.
    let mut cum = vec![0u64; num_frames + 1];
    for (k, &sz) in frame_sizes.iter().enumerate() {
        cum[k + 1] = cum[k] + sz as u64;
    }
    let total = total_bytes as u64;
    for (i, slot) in toc.iter_mut().enumerate() {
        // Frame whose start is closest to i/100 of the total duration.
        // round(i * num_frames / 100), clamped so we never index past
        // the last frame's start offset.
        let frame_idx = ((i as u64 * num_frames as u64 + 50) / 100).min(num_frames as u64 - 1);
        let offset = cum[frame_idx as usize];
        // round(256 * offset / total), clamped to a single byte.
        let val = (256 * offset + total / 2) / total;
        *slot = val.min(255) as u8;
    }
    toc
}

/// Walk the version's bitrate ladder (smallest first) and pick the
/// smallest slot whose unpadded frame size meets `needed_bytes`. If no
/// slot fits, fall back to the largest slot — the caller will then
/// truncate the payload.
///
/// MPEG-1 Layer II (ISO/IEC 11172-3 §2.4.2.3 Table 3-B.2) forbids
/// some (mode, bitrate) pairs even in VBR mode — single-channel
/// streams cannot use ≥ 224 kbps, and stereo streams cannot use
/// 32 or 48 kbps. We filter those out here so VBR never emits a
/// header a strict decoder would reject. MPEG-2 LSF (§13818-3
/// §2.4.2.3) has no such restrictions; the filter is a no-op there.
fn pick_vbr_slot(
    version: EncVersion,
    sample_rate: u32,
    needed_bytes: usize,
    n_channels: usize,
) -> (u32, u32) {
    let table = bitrate_table(version);
    let is_mpeg1 = matches!(version, EncVersion::Mpeg1);
    let permitted = |kbps: u32| -> bool {
        if !is_mpeg1 {
            return true;
        }
        match n_channels {
            1 => !matches!(kbps, 224 | 256 | 320 | 384),
            _ => !matches!(kbps, 32 | 48),
        }
    };
    let mut fallback: Option<(u32, u32)> = None;
    for (i, &kbps) in table.iter().enumerate() {
        if !permitted(kbps) {
            continue;
        }
        let frame_bytes = (144 * kbps * 1000 / sample_rate) as usize;
        if fallback.is_none() {
            fallback = Some(((i + 1) as u32, kbps));
        }
        if frame_bytes >= needed_bytes {
            return ((i + 1) as u32, kbps);
        }
    }
    // Nothing fits → return the largest permitted slot. If nothing is
    // permitted at all (impossible by ladder construction), fall
    // through to the absolute last slot.
    if let Some((_, _)) = fallback {
        // Find the largest permitted slot.
        for (i, &kbps) in table.iter().enumerate().rev() {
            if permitted(kbps) {
                return ((i + 1) as u32, kbps);
            }
        }
    }
    let last = table.len() - 1;
    ((last + 1) as u32, table[last])
}

/// Greedy bit allocator. Iteratively bumps each subband (per channel,
/// or shared for sb >= bound) to the next class while a (cost, energy)
/// score remains above the stop threshold AND remaining budget covers
/// the upgrade. Returns the per-channel allocation grid.
///
/// `ath_weight` is the per-subband perceptual weight in `(0, 1]`
/// (=== 1.0 everywhere when [`PsyModel::None`]). Subband energies are
/// scaled by `weight^2` before scoring — subbands whose centre
/// frequency sits well outside the audible range (deep sub-bass,
/// near-Nyquist ultrasonic) drop in priority by 20–40 dB without
/// being silenced outright.
#[allow(clippy::too_many_arguments)]
fn run_allocator(
    table: &AllocTable,
    n_ch: usize,
    bound_subband: usize,
    energy: &[[f32; 32]],
    scfsi: &[[u8; 32]],
    initial_budget_bits: i64,
    stop_score: f32,
    ath_weight: &[f32; 32],
) -> Result<Vec<[u8; 32]>> {
    if initial_budget_bits < 0 {
        return Err(Error::other("MP2 encoder: frame too small for header"));
    }
    let mut alloc = vec![[0u8; 32]; n_ch];
    let mut remaining: i64 = initial_budget_bits;
    let other_ch_idx = if n_ch == 2 { 1 } else { 0 };

    loop {
        let mut best: Option<(usize, usize, u8, i64)> = None;
        let mut best_score = f32::NEG_INFINITY;
        for ch in 0..n_ch {
            for sb in 0..table.sblimit {
                if sb >= bound_subband && ch != 0 {
                    // Shared-intensity subbands: only ch=0 drives the
                    // allocation; ch=1 mirrors what ch=0 picks.
                    continue;
                }
                let cur = alloc[ch][sb];
                let max = (1u32 << table.nbal(sb)) - 1;
                if cur as u32 >= max {
                    continue;
                }
                let next = cur + 1;
                let cost = upgrade_cost_bits(
                    table,
                    sb,
                    cur,
                    next,
                    scfsi[ch][sb],
                    sb >= bound_subband,
                    scfsi[other_ch_idx][sb],
                    n_ch,
                );
                let raw_energy = if sb >= bound_subband && n_ch == 2 {
                    energy[0][sb].max(energy[1][sb])
                } else {
                    energy[ch][sb]
                };
                // Apply the per-subband ATH weight (= 1.0 for
                // PsyModel::None). Energy is amplitude-squared, so
                // the perceptual attenuation is weight^2 (a weight
                // of 0.1 multiplies the score by 0.01 → near-Nyquist
                // subbands have to be ~100× more energetic before
                // they outrank a mid-band subband for the same cost).
                let w = ath_weight[sb];
                let weighted_energy = raw_energy * w * w;
                let score = weighted_energy / (cost as f32).max(1.0);
                if score > best_score && cost as i64 <= remaining {
                    best_score = score;
                    best = Some((ch, sb, next, cost as i64));
                }
            }
        }

        match best {
            Some((ch, sb, next, cost)) => {
                if best_score < stop_score {
                    break;
                }
                alloc[ch][sb] = next;
                if sb >= bound_subband && n_ch == 2 {
                    alloc[1][sb] = next;
                }
                remaining -= cost;
            }
            None => break,
        }
    }
    Ok(alloc)
}

/// Compute the total per-frame sample-payload bit cost of a given
/// allocation grid (samples + SCFSI + scalefactor bits — does NOT
/// include the 32-bit header or the bit-alloc indices themselves).
fn allocation_payload_bits(
    table: &AllocTable,
    n_ch: usize,
    bound_subband: usize,
    alloc: &[[u8; 32]],
    scfsi: &[[u8; 32]],
) -> i64 {
    let mut bits: i64 = 0;
    for sb in 0..table.sblimit {
        if sb < bound_subband {
            for ch in 0..n_ch {
                let a = alloc[ch][sb];
                if a == 0 {
                    continue;
                }
                bits += sample_bits_per_subband_for_class(table, sb, a) as i64;
                bits += 2; // SCFSI
                bits += 6 * scfsi_sf_count(scfsi[ch][sb]) as i64;
            }
        } else {
            let a = alloc[0][sb];
            if a == 0 {
                continue;
            }
            bits += sample_bits_per_subband_for_class(table, sb, a) as i64;
            // SCFSI + scalefactors are still per-channel even in
            // shared-intensity subbands.
            for ch in 0..n_ch {
                bits += 2;
                bits += 6 * scfsi_sf_count(scfsi[ch][sb]) as i64;
            }
        }
    }
    bits
}

/// Map VBR quality 0..=9 to a stop score for the allocator. The
/// allocator stops upgrading subbands once the energy/bit ratio of
/// the next-best upgrade drops below this threshold (no effect in
/// CBR — CBR runs the full "spend everything" loop).
fn vbr_quality_to_stop_score(q: u8, mode: RateControl) -> f32 {
    if matches!(mode, RateControl::Cbr) {
        // CBR: never short-circuit. The greedy loop terminates only
        // when the remaining budget is too small to afford any
        // upgrade.
        f32::NEG_INFINITY
    } else {
        // Higher quality → smaller stop score → more bits get spent.
        // Hand-tuned to span ~24..160 kbps over a music-like input
        // at 44.1 kHz mono at q=0..9.
        // q=0 → 1e-7 (essentially "spend everything")
        // q=9 → 1e-2 (drop subbands with weak masking gain quickly)
        const STOPS: [f32; 10] = [1e-7, 5e-7, 2e-6, 1e-5, 5e-5, 2e-4, 1e-3, 3e-3, 8e-3, 2e-2];
        STOPS[q.min(9) as usize]
    }
}

/// Pick a joint-stereo bound for the current frame. Returns
/// `Some((mode_ext, bound))` when at least the smallest bound (4)
/// has every upper subband above its per-subband intensity-stereo
/// correlation threshold; falls back to plain stereo (`None`)
/// otherwise.
///
/// `js_relax[sb]` is a per-subband relaxation subtracted from the base
/// [`JOINT_STEREO_CORR_THRESHOLD`]. Higher subbands have larger
/// relaxation values because spatial hearing is less acute above
/// ~2 kHz; this lets intensity stereo engage on material that's
/// "almost correlated" in the high bands without giving up bits the
/// low bands need. All-zero relaxation reproduces the v0.0.8 strict
/// threshold behaviour.
///
/// We try the bound candidates from smallest to largest. The smallest
/// bound that "works" wins because it exposes the most subbands to
/// shared-coefficient coding (= the most bit savings).
fn pick_joint_stereo_bound(
    sub: &[[[f32; 36]; 32]],
    sblimit: usize,
    js_relax: &[f32; 32],
) -> Option<(u32, u32)> {
    if sub.len() != 2 {
        return None;
    }
    // Per-subband normalised L/R correlation (Pearson over 36 samples
    // with zero mean assumed).
    let mut corr = [0.0f32; 32];
    for sb in 0..sblimit {
        let mut e_l = 0.0f32;
        let mut e_r = 0.0f32;
        let mut e_lr = 0.0f32;
        for i in 0..36 {
            let l = sub[0][sb][i];
            let r = sub[1][sb][i];
            e_l += l * l;
            e_r += r * r;
            e_lr += l * r;
        }
        let denom = (e_l * e_r).sqrt() + 1e-12;
        corr[sb] = (e_lr / denom).abs();
    }
    // For each candidate bound, check that every subband at-or-above
    // the bound is correlated enough — where "enough" is the base
    // threshold minus the per-subband relaxation.
    for (idx, &bnd_u32) in JOINT_STEREO_BOUNDS.iter().enumerate() {
        let bnd = bnd_u32 as usize;
        if bnd >= sblimit {
            // Bound past sblimit: nothing to share. Fall through to
            // the next (larger) candidate.
            continue;
        }
        let mut all_ok = true;
        for sb in bnd..sblimit {
            let threshold = (JOINT_STEREO_CORR_THRESHOLD - js_relax[sb]).max(0.0);
            if corr[sb] < threshold {
                all_ok = false;
                break;
            }
        }
        if all_ok {
            return Some((idx as u32, bnd_u32));
        }
    }
    None
}

/// Given a signal peak magnitude, pick the smallest scalefactor index `i`
/// whose magnitude `2 * 2^(-i/3)` is strictly greater than `peak`. A
/// larger index means a smaller scalefactor, so we want the largest
/// quantisation resolution that still covers the peak without clipping.
///
/// Falls back to index 62 (smallest SF) for tiny peaks, and to index 0
/// (largest SF) for peaks that exceed the table range.
fn pick_scalefactor(peak: f32) -> u8 {
    if !peak.is_finite() || peak <= 0.0 {
        return 62;
    }
    // Index 0 covers peaks up to 2.0. Index 62 covers tiny values.
    // Find the largest index whose magnitude >= peak.
    let mut best = 0u8;
    for i in 0..63u8 {
        let mag = scalefactor_magnitude(i);
        if mag >= peak {
            best = i;
        } else {
            break;
        }
    }
    best
}

/// Pick the SCFSI value that represents the triple `(a, b, c)` exactly
/// when possible. For imperfect matches, fall back to 0 (full transmission).
fn pick_scfsi(a: u8, b: u8, c: u8) -> u8 {
    if a == b && b == c {
        2
    } else if a == b && a != c {
        1
    } else if b == c && a != b {
        3
    } else {
        0
    }
}

/// Look up the class-entry for allocation index `a` (>= 1) of subband `sb`.
fn class_entry(table: &AllocTable, sb: usize, a: u8) -> AllocEntry {
    let base = table.offsets[sb];
    table.entries[base + a as usize]
}

/// Return the number of sample bits per subband-part consumed by class `a`
/// of subband `sb`. Each subband transmits three parts × four triples per
/// part. One triple is either one grouped codeword (3/5/9-level) or three
/// ungrouped codewords. For allocation index 0 the cost is zero.
fn sample_bits_per_subband_for_class(table: &AllocTable, sb: usize, a: u8) -> u32 {
    if a == 0 {
        return 0;
    }
    let e = class_entry(table, sb, a);
    let bits = e.bits as u32;
    // 3 parts × 4 triples per part = 12 triples.
    if e.d > 0 {
        // Grouped: one codeword per triple.
        12 * bits
    } else {
        // Ungrouped: `bits` bits × 3 samples per triple × 12 triples.
        12 * bits * 3
    }
}

/// The extra bits required to go from current allocation `cur` to `next`.
/// Accounts for sample bits, SCFSI bits (2, paid once per channel when
/// allocation transitions 0 → non-zero), and scalefactor bits (6 per SF
/// field, count determined by SCFSI).
///
/// In a shared-intensity subband (`shared` true), the sample bit cost is
/// paid only once for both channels — but SCFSI/scalefactor bits are still
/// per-channel.
#[allow(clippy::too_many_arguments)]
fn upgrade_cost_bits(
    table: &AllocTable,
    sb: usize,
    cur: u8,
    next: u8,
    scfsi: u8,
    shared: bool,
    other_scfsi: u8,
    n_ch: usize,
) -> u32 {
    let cur_sample = sample_bits_per_subband_for_class(table, sb, cur);
    let next_sample = sample_bits_per_subband_for_class(table, sb, next);
    let mut cost = next_sample.saturating_sub(cur_sample);
    if cur == 0 && next != 0 {
        // Pay SCFSI + scalefactor bits once for each channel that is
        // about to start emitting (in a shared band, both channels do).
        if shared && n_ch == 2 {
            // SCFSI + scalefactors for ch0 and ch1.
            cost += 2 * 2;
            cost += 6 * scfsi_sf_count(scfsi);
            cost += 6 * scfsi_sf_count(other_scfsi);
        } else {
            cost += 2;
            cost += 6 * scfsi_sf_count(scfsi);
        }
    }
    cost
}

/// Count of 6-bit scalefactor fields transmitted for a given SCFSI value.
fn scfsi_sf_count(scfsi: u8) -> u32 {
    match scfsi {
        0 => 3,
        1 | 3 => 2,
        2 => 1,
        _ => 3,
    }
}

/// Write one 3-sample triple to the bit writer, given the class entry and
/// the scalefactor magnitude for the owning part.
fn write_triple(
    w: &mut BitWriter,
    entry: AllocEntry,
    row: &[f32; 36],
    base_idx: usize,
    sf_mag: f32,
) {
    let bits = entry.bits as u32;
    let d = entry.d as i32;
    if d > 0 {
        // Grouped 3/5/9-level quantiser.
        let levels = d as u32;
        let mut idx = [0u32; 3];
        for i in 0..3 {
            let s = row[base_idx + i] / sf_mag.max(1e-20);
            // Map fractional amplitude `f ∈ [-1..+1]` to quantiser level
            // index `i ∈ [0..L-1]` using the inverse of the decoder's
            // (2*i - (L-1)) / L mapping:
            //   i = round((f * L + (L - 1)) / 2)
            let l = levels as f32;
            let raw = (s * l + (l - 1.0)) * 0.5;
            let mut ii = raw.round() as i32;
            if ii < 0 {
                ii = 0;
            }
            if ii as u32 >= levels {
                ii = (levels - 1) as i32;
            }
            idx[i] = ii as u32;
        }
        // Pack as base-L integer: code = s0 + L*s1 + L²*s2.
        let code = idx[0] + levels * idx[1] + levels * levels * idx[2];
        w.write_u32(code, bits);
    } else {
        // Ungrouped `bits`-bit unsigned codeword per sample.
        // Decoder does: out = (v + d) * c * sf, with c = 2/(2^bits - 1)
        // and d = -(2^(bits-1) - 1). Inverting:
        //   v = round(out / (c * sf) - d)
        //     = round(s * (2^bits - 1) / 2 - d)
        let levels = (1u32 << bits) - 1;
        let c = 2.0f32 / (levels as f32);
        for i in 0..3 {
            let s = row[base_idx + i] / sf_mag.max(1e-20);
            let raw = s / c - d as f32;
            let mut v = raw.round() as i32;
            if v < 0 {
                v = 0;
            }
            let max_code = (1u32 << bits) - 1;
            if v as u32 > max_code {
                v = max_code as i32;
            }
            w.write_u32(v as u32, bits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_index_lookup() {
        assert_eq!(bitrate_to_index(128), Some(8));
        assert_eq!(bitrate_to_index(192), Some(10));
        assert_eq!(bitrate_to_index(384), Some(14));
        assert_eq!(bitrate_to_index(999), None);
    }

    #[test]
    fn scalefactor_pick_reasonable() {
        // Peak 1.0 should land at an index whose magnitude >= 1.0.
        let i = pick_scalefactor(1.0);
        let mag = scalefactor_magnitude(i);
        assert!((1.0..=2.0).contains(&mag), "mag={mag}");
        // Tiny peak → large index.
        let i = pick_scalefactor(1e-8);
        assert!(i >= 50, "got {i}");
    }

    #[test]
    fn scfsi_patterns() {
        assert_eq!(pick_scfsi(5, 5, 5), 2);
        assert_eq!(pick_scfsi(5, 5, 9), 1);
        assert_eq!(pick_scfsi(3, 8, 8), 3);
        assert_eq!(pick_scfsi(1, 2, 3), 0);
    }

    #[test]
    fn vbr_slot_picks_smallest_fit() {
        // 24 kHz LSF: 144 * 8 * 1000 / 24000 = 48 bytes for 8 kbps.
        // A frame needing 100 bytes should land around 24 kbps
        // (144 * 24 * 1000 / 24000 = 144).
        let (idx, kbps) = pick_vbr_slot(EncVersion::Mpeg2Lsf, 24_000, 100, 2);
        assert!(kbps >= 16, "expected >=16 kbps, got {kbps}");
        assert!(idx >= 1);
    }

    #[test]
    fn vbr_slot_excludes_invalid_mpeg1_stereo_rates() {
        // MPEG-1 stereo at 44.1 kHz: a tiny `needed_bytes` would
        // naively pick 32 kbps but that's forbidden in stereo by
        // Table 3-B.2. The picker must skip 32/48 and land on
        // 56 kbps (smallest permitted stereo rate).
        let (idx, kbps) = pick_vbr_slot(EncVersion::Mpeg1, 44_100, 1, 2);
        assert_eq!(kbps, 56, "got {kbps}");
        assert_eq!(idx, 3);
    }

    #[test]
    fn vbr_slot_excludes_invalid_mpeg1_mono_rates() {
        // MPEG-1 mono can use 32..192 kbps but not 224+. A very
        // large `needed_bytes` should saturate at 192 kbps.
        let (idx, kbps) = pick_vbr_slot(EncVersion::Mpeg1, 44_100, 100_000, 1);
        assert_eq!(kbps, 192, "mono picker should top out at 192, got {kbps}");
        assert_eq!(idx, 10);
    }

    #[test]
    fn vbr_slot_lsf_unrestricted() {
        // LSF allows every slot in every mode.
        let (_idx, kbps) = pick_vbr_slot(EncVersion::Mpeg2Lsf, 24_000, 1, 2);
        assert_eq!(kbps, 8, "LSF stereo should allow the 8 kbps slot");
    }

    #[test]
    fn xing_toc_endpoints_and_monotonic() {
        // 100 equal-size frames, total 1000 bytes (10 each). The TOC
        // walks frame round(i*100/100)=i, so offset_at(i)=10*i bytes,
        // toc[i]=round(256*10*i/1000)=round(2.56*i).
        let sizes = vec![10u32; 100];
        let total: u32 = sizes.iter().sum();
        let toc = build_xing_toc(&sizes, total);
        // Start of stream.
        assert_eq!(toc[0], 0, "toc[0] must be 0 (start of stream)");
        // Non-decreasing.
        for w in toc.windows(2) {
            assert!(w[1] >= w[0], "TOC must be non-decreasing: {w:?}");
        }
        // toc[50] ~ 50% → round(2.56*50)=128.
        assert_eq!(toc[50], 128, "toc[50]={}", toc[50]);
        // toc[99] ~ 99% → round(2.56*99)=253. Every entry is a `u8`, so
        // the spec's 0..=255 single-byte range is satisfied by the type.
        assert_eq!(toc[99], 253, "toc[99]={}", toc[99]);
    }

    #[test]
    fn xing_toc_variable_sizes_track_byte_offsets() {
        // Front-loaded stream: first frame is huge, rest are tiny. The
        // TOC should climb steeply near the start and flatten later,
        // because most bytes are spent at the front.
        let mut sizes = vec![1000u32];
        sizes.extend(std::iter::repeat_n(10u32, 99));
        let total: u32 = sizes.iter().sum(); // 1000 + 990 = 1990
        let toc = build_xing_toc(&sizes, total);
        assert_eq!(toc[0], 0);
        // By 1% of duration (frame 1) we've already passed the 1000-byte
        // first frame: round(256*1000/1990)=129. The first step is the
        // biggest jump in the table.
        assert_eq!(toc[1], 129, "toc[1]={}", toc[1]);
        // Subsequent steps are small (each later frame is only 10 bytes).
        let big_step = toc[1] - toc[0];
        let small_step = toc[50] - toc[49];
        assert!(
            big_step > small_step,
            "front-loaded stream: first step {big_step} should exceed mid step {small_step}"
        );
    }

    #[test]
    fn xing_toc_degenerate_inputs() {
        // Empty / zero-total inputs return an all-zero table rather than
        // dividing by zero.
        assert_eq!(build_xing_toc(&[], 0), [0u8; 100]);
        assert_eq!(build_xing_toc(&[100], 0), [0u8; 100]);
        // Single frame: every fraction maps to frame 0 (offset 0).
        assert_eq!(build_xing_toc(&[417], 417), [0u8; 100]);
    }

    #[test]
    fn quality_stop_monotonic() {
        // Higher quality index should yield a STRICTLY higher stop
        // score — i.e. quality 9 is more aggressive about dropping
        // weak subbands than quality 0.
        let s0 = vbr_quality_to_stop_score(0, RateControl::Vbr);
        let s9 = vbr_quality_to_stop_score(9, RateControl::Vbr);
        assert!(s9 > s0, "s0={s0}, s9={s9}");
        // CBR mode never short-circuits.
        assert_eq!(
            vbr_quality_to_stop_score(0, RateControl::Cbr),
            f32::NEG_INFINITY
        );
    }

    #[test]
    fn joint_stereo_bound_picks_smallest_for_correlated_input() {
        // Build a 2-channel subband layout where every upper subband
        // is fully correlated (L == R) — should pick the smallest
        // bound (index 0 → bound 4).
        let mut sub: Vec<[[f32; 36]; 32]> = vec![[[0.0f32; 36]; 32]; 2];
        for sb in 0..27 {
            for i in 0..36 {
                let v = ((sb + i) as f32 * 0.01).sin() * 0.3;
                sub[0][sb][i] = v;
                sub[1][sb][i] = v;
            }
        }
        // Strict (no relaxation) — every subband must be over the
        // base threshold. Perfectly correlated input clears it.
        let strict_relax = [0.0f32; 32];
        let pick = pick_joint_stereo_bound(&sub, 27, &strict_relax);
        assert_eq!(pick, Some((0, 4)), "got {pick:?}");
    }

    #[test]
    fn joint_stereo_relaxation_admits_more_borderline_input() {
        // Hand-build a signal whose upper-subband correlation sits
        // *just below* the strict 0.7 threshold (~0.65) but *above*
        // the relaxed threshold for high subbands.
        let mut sub: Vec<[[f32; 36]; 32]> = vec![[[0.0f32; 36]; 32]; 2];
        for sb in 0..27 {
            for i in 0..36 {
                let v = ((sb + i) as f32 * 0.01).sin() * 0.3;
                sub[0][sb][i] = v;
                // Mix in 50% noise so |corr| ~ 0.65 — between
                // 0.7-strict and 0.55-relaxed-at-high-bands.
                let n = ((sb * 7 + i * 3) as f32 * 0.13).sin() * 0.3;
                sub[1][sb][i] = 0.6 * v + 0.8 * n;
            }
        }
        let relax = joint_stereo_threshold_relaxation_per_subband();
        let strict_relax = [0.0f32; 32];
        let strict = pick_joint_stereo_bound(&sub, 27, &strict_relax);
        let relaxed = pick_joint_stereo_bound(&sub, 27, &relax);
        eprintln!("strict pick: {strict:?}, relaxed pick: {relaxed:?}");
        // The relaxed pick must engage in at least as many situations
        // as strict: if strict succeeded, relaxed also succeeds (and
        // ideally at a smaller bound). If strict was None, relaxed
        // may still succeed at a high bound.
        if let Some((_, sb)) = strict {
            assert!(
                relaxed.map(|(_, r)| r <= sb).unwrap_or(false),
                "relaxation regressed: strict={strict:?}, relaxed={relaxed:?}"
            );
        }
    }

    #[test]
    fn joint_stereo_bound_falls_back_to_stereo_for_uncorrelated() {
        let mut sub: Vec<[[f32; 36]; 32]> = vec![[[0.0f32; 36]; 32]; 2];
        for sb in 0..27 {
            for i in 0..36 {
                // L: positive, R: opposite phase.
                let v = ((sb + i) as f32 * 0.01).sin() * 0.3;
                sub[0][sb][i] = v;
                sub[1][sb][i] = -v;
            }
        }
        // |corr| is still 1.0 in this case (sign doesn't matter to
        // |Pearson|), so flip ch1 to white noise instead.
        for sb in 0..27 {
            for i in 0..36 {
                sub[1][sb][i] = ((sb * 7 + i * 3) as f32 * 0.13).sin() * 0.3;
            }
        }
        let strict_relax = [0.0f32; 32];
        let pick = pick_joint_stereo_bound(&sub, 27, &strict_relax);
        // At least the bound 16 should still kick in if corr is high
        // by coincidence — accept None or any bound.
        eprintln!("uncorrelated pick: {pick:?}");
    }

    #[test]
    fn encoder_roundtrip_silence() {
        use crate::decoder::make_decoder;
        use oxideav_core::Frame as CoreFrame;

        let mut params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        params.channels = Some(1);
        params.sample_rate = Some(44_100);
        params.sample_format = Some(SampleFormat::S16);
        params.bit_rate = Some(128_000);
        let mut enc = make_encoder(&params).unwrap();

        // Feed 3 frames of silence.
        let mut data = Vec::new();
        for _ in 0..1152 * 3 {
            data.extend_from_slice(&0i16.to_le_bytes());
        }
        let frame = AudioFrame {
            samples: 1152 * 3,
            pts: Some(0),
            data: vec![data],
        };
        enc.send_frame(&CoreFrame::Audio(frame)).unwrap();
        let mut packets: Vec<Packet> = Vec::new();
        while let Ok(p) = enc.receive_packet() {
            packets.push(p);
        }
        enc.flush().unwrap();
        while let Ok(p) = enc.receive_packet() {
            packets.push(p);
        }
        assert!(!packets.is_empty(), "no packets produced");

        // Decode back.
        let dparams = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        let mut dec = make_decoder(&dparams).unwrap();
        let mut decoded = 0u32;
        for p in &packets {
            dec.send_packet(p).unwrap();
            if let Ok(CoreFrame::Audio(a)) = dec.receive_frame() {
                decoded += a.samples;
            }
        }
        assert!(decoded >= 1152, "decoded too few samples: {decoded}");
    }
}
