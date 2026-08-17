//! ISO/IEC 13818-3 §2.5 **multichannel extension** encode for Layer II
//! — the encode-side dual of [`crate::mc`].
//!
//! A multichannel encode produces an ordinary ISO/IEC 11172-3 Layer II
//! frame whose §2.4.1.8 ancillary field carries the `mc_extension()`
//! payload (§2.5.1.3). The pipeline per frame:
//!
//! 1. **Matrixing** (§2.5.3.3): the presentation channels are combined
//!    into the MPEG-1-compatible pair `Lo` / `Ro` and the weighted
//!    transmission channels `T2..T4`. For `dematrix_procedure` `'00'`
//!    the compatible pair is `Lo = α(L + βC + γLS)`,
//!    `Ro = α(R + βC + γRS)` with `α = 1/(1+√2)`, `β = γ = 1/√2`; for
//!    `'01'` `α = 1/(1,5 + 0,5·√2)`, `γ = 0,5`; `'11'` transmits every
//!    signal unmatrixed. The α normalisation is exactly the attenuation
//!    §2.5.3.2.5's de-normalisation undoes ("to avoid overload when
//!    calculating the compatible signals"), and bounds `|Lo|, |Ro| ≤ 1`
//!    for presentation channels inside the §2.4.3.4.7.1 nominal range.
//!    The transmitted channels carry the *weighted* signals (`Cw =
//!    αβ·C`, `LSw = αγ·LS`, …) so the §2.5.3.2.1.1 decoding matrix
//!    recovers the originals.
//! 2. **Base encode**: `Lo` / `Ro` run the ordinary §C.1.5.2.7 Layer II
//!    encode (§D.1 Model-1 auto-SMR) with a `banc` ancillary
//!    reservation equal to the assembled extension's bit length, so
//!    a §2.5-unaware decoder plays the compatible stereo downmix.
//! 3. **MC extension**: the transmission channels are analysed
//!    (§C.1.3 filterbank), scalefactor-extracted, scfsi-selected and
//!    greedily bit-allocated (the §C.1.5.2.7 minimum-MNR procedure
//!    against a §D.1 Model-1 SMR per transmission channel) against the
//!    Table B.2a (48 kHz) / B.2b (44,1 / 32 kHz) ladder with
//!    `msblimit = sblimit` (§2.5.2.17), then serialised in the
//!    §2.5.1.12.1 / §2.5.1.17 wire order with the §2.5.2.14
//!    `mc_crc_check` over mc_header + composite status + allocation +
//!    scfsi. The composite status signals `tc_sbgr_select = '1'` with
//!    the global `tc_allocation = 0` (centre / surround on `T2..T4`),
//!    `dyn_cross_on = '0'` and `mc_prediction_on = '0'` — all
//!    encoder-side *options* the syntax lets an encoder decline
//!    (§2.5.2.15 marks their use free to the encoder; the decode side
//!    of all three is fully implemented in [`crate::mc`]).
//! 4. **LFE** (§2.5.2.17 / §2.5.3.2.4): the low-frequency-enhancement
//!    channel is quantized as block-companded PCM — 12 samples per
//!    frame at `Fs / 96`, one Table B.1 scalefactor, Layer-I-style
//!    `2^nb − 1`-level requantisation (no grouping).
//! 5. **Splice**: the serialised extension is written into the encoded
//!    base frame starting at the exact first ancillary bit — where the
//!    §2.5.3.1 CRC-detection rule expects `mc_header()`.
//!
//! Everything is emitted with `ext_bit_stream_present = '0'`: the whole
//! extension must fit the base frame's ancillary capacity (a stream
//! needing a §2.5.1.5 extension bit stream is refused with
//! [`McEncodeError::BudgetTooSmall`] rather than silently truncated).
//!
//! Clean-room: the syntax, wire order and matrixing equations are read
//! from ISO/IEC 13818-3 (1997) §2.5.1 / §2.5.2 / §2.5.3 only, mirrored
//! against this crate's own §2.5 decoder. No external encoder source
//! was consulted.

// The §2.5.1 syntax loops are written in the spec's index-based
// `for (sb…) for (mch…)` notation so the wire order stays visually
// checkable against the printed syntax tables (same convention as
// `crate::mc`).
#![allow(clippy::needless_range_loop)]

use crate::analysis::AnalysisFilterbank;
use crate::audio_data::Scfsi;
use crate::bitalloc::{class_of_quantization, BitAllocTable, NUM_SUBBANDS};
use crate::crc::{crc16_step, INIT_STATE};
use crate::encoder_bit_allocator::{sample_bits_for, snr_db, SCFSI_BITS_PER_SLOT};
use crate::encoder_frame::{
    encode_frame_auto_with, EncodeError, EncodeFrameState, MODEL1_WINDOW_DELAY_SAMPLES,
};
use crate::encoder_samples::write_triplet_scaled;
use crate::encoder_scalefactors::{
    compute_scalefactors, pick_scalefactor_index, SUBBAND_SAMPLES_PER_FRAME,
};
use crate::encoder_scfsi::{select_scfsi, ScfsiSelection};
use crate::frame::PCM_SAMPLES_PER_CHANNEL;
use crate::header::{FrameHeader, Mode, PaddingScheduler};
use crate::mc::{base_ancillary_start_bit, Centre, McConfig, McHeader, Surround};
use crate::psy::{annex_d_sampling_rate, compute_smr_model1_frame, LAYER2_FFT_LEN};
use oxideav_core::bits::BitWriter;

/// Subband samples per subband per Layer II frame (12 granules × 3).
const SLOTS: usize = SUBBAND_SAMPLES_PER_FRAME;
/// LFE samples per frame (`Fs / 96` ⇒ 1152 / 96 = 12, §2.5.3.2.4).
pub const LFE_SAMPLES_PER_FRAME: usize = crate::mc::LFE_SAMPLES_PER_FRAME;

/// √2, spelled once.
const SQRT2: f64 = std::f64::consts::SQRT_2;

/// Errors raised by the §2.5 multichannel encode.
#[derive(Debug, Clone, PartialEq)]
pub enum McEncodeError {
    /// The [`McEncodeConfig`] is not one this encoder can emit:
    /// `front` ∉ {2, 3}, `surround` ∉ {0, 1, 2}, an unsupported
    /// `dematrix_procedure` (only `'00'` / `'01'` / `'11'` are encoded
    /// — the `'10'` phase-mixed-surround *encode* is not implemented),
    /// or an out-of-range `lfe_allocation`.
    BadConfig(String),
    /// The base header is unsuitable: the §2.5 extension is defined on
    /// an MPEG-1-compatible Layer II base (no LSF), and this encoder
    /// requires a two-channel `Stereo` base mode.
    BadBaseHeader(String),
    /// `pcm.len()` does not match the configuration's presentation
    /// channel count, or a channel buffer is not exactly
    /// [`PCM_SAMPLES_PER_CHANNEL`] samples (per frame) / a whole
    /// multiple of it (batch).
    BadPcmShape { have: usize, need: usize },
    /// The LFE input is missing / present contrary to `cfg.lfe`, or
    /// its length is not [`LFE_SAMPLES_PER_FRAME`] per frame.
    BadLfeShape { have: usize, need: usize },
    /// The multichannel extension's fixed cost (mc_header, CRC,
    /// composite status, allocation fields, LFE) already exceeds the
    /// extension bit budget — the frame bitrate is too low for this
    /// configuration (an extension bit stream would be required;
    /// this encoder emits single-frame extensions only).
    BudgetTooSmall { fixed: u32, budget: u32 },
    /// The base Layer II encode failed (most commonly the §C.1.5.2.7
    /// allocator's `InsufficientFrameSize` when the extension
    /// reservation leaves the base pair no data bits).
    Base(EncodeError),
    /// Internal consistency failure (a table lookup the allocator
    /// never steps outside of failed) — a bug signal.
    Internal(String),
}

impl core::fmt::Display for McEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            McEncodeError::BadConfig(s) => write!(f, "mc_encode: bad config: {s}"),
            McEncodeError::BadBaseHeader(s) => write!(f, "mc_encode: bad base header: {s}"),
            McEncodeError::BadPcmShape { have, need } => {
                write!(f, "mc_encode: pcm shape {have}, expected {need}")
            }
            McEncodeError::BadLfeShape { have, need } => {
                write!(f, "mc_encode: lfe shape {have}, expected {need}")
            }
            McEncodeError::BudgetTooSmall { fixed, budget } => write!(
                f,
                "mc_encode: extension fixed cost {fixed} bits exceeds budget {budget}"
            ),
            McEncodeError::Base(e) => write!(f, "mc_encode: base encode: {e}"),
            McEncodeError::Internal(s) => write!(f, "mc_encode: internal: {s}"),
        }
    }
}

impl std::error::Error for McEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            McEncodeError::Base(e) => Some(e),
            _ => None,
        }
    }
}

impl From<EncodeError> for McEncodeError {
    fn from(e: EncodeError) -> Self {
        McEncodeError::Base(e)
    }
}

/// Multichannel encode configuration: which §2.5.2.15 channel
/// configuration to emit and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McEncodeConfig {
    /// Front channels: `2` (L, R) or `3` (L, R, C).
    pub front: u8,
    /// Surround channels: `0`, `1` (mono surround S) or `2` (LS, RS).
    pub surround: u8,
    /// Emit a §2.5.3.2.4 low-frequency-enhancement channel.
    pub lfe: bool,
    /// §2.5.2.13 `dematrix_procedure`: `0` (`'00'`), `1` (`'01'`) or
    /// `3` (`'11'`, no matrixing). The `'10'` phase-mixed-surround
    /// encode is not offered.
    pub dematrix_procedure: u8,
    /// §2.5.2.17 `lfe_allocation` (quantizer index; `nb = value + 1`
    /// bits per LFE sample). Range `2..=15` — index 1 selects the
    /// 3-level quantizer whose Layer II class is *grouped*, but the
    /// §2.5.1.17 LFE field is a single ungrouped `nb`-bit code per
    /// granule, so this encoder starts at the 7-level class.
    pub lfe_allocation: u8,
    /// Extension bit budget override. `None` splits the frame's data
    /// bits between the base pair and the extension in proportion to
    /// their channel counts (`nmch / (2 + nmch)` to the extension,
    /// plus the fixed LFE cost).
    pub mc_bits: Option<u32>,
    /// §2.5.3.2.1.3 **multichannel prediction**: when enabled the
    /// encoder fits one first-order, zero-delay predictor per
    /// (subband group 0..7, transmission channel, compatible source
    /// `T0`/`T1`) by least squares, quantizes the coefficients to the
    /// wire grid `(v − 127)/32`, and transmits the prediction *error*
    /// in the subbands whose group the fit measurably wins
    /// (≥ 10 % residual-energy reduction). The predictors are fitted
    /// against the encoder's unquantized `Lo`/`Ro` subband signals;
    /// the decoder predicts from its requantised pair, so the residual
    /// mismatch is bounded by the base pair's quantization noise —
    /// the §2.5.3.2.1.3 decode arithmetic is what is normative, the
    /// election is the encoder's (§2.5.2.15 leaves it free).
    pub prediction: bool,
}

impl Default for McEncodeConfig {
    /// 3/2 (five presentation channels), no LFE, procedure `'00'`.
    fn default() -> Self {
        McEncodeConfig {
            front: 3,
            surround: 2,
            lfe: false,
            dematrix_procedure: 0,
            lfe_allocation: 7,
            mc_bits: None,
            prediction: false,
        }
    }
}

impl McEncodeConfig {
    /// Validate the configuration.
    fn validate(&self) -> Result<(), McEncodeError> {
        if !matches!(self.front, 2 | 3) {
            return Err(McEncodeError::BadConfig(format!(
                "front={} (2 or 3)",
                self.front
            )));
        }
        if self.surround > 2 {
            return Err(McEncodeError::BadConfig(format!(
                "surround={} (0, 1 or 2)",
                self.surround
            )));
        }
        if !matches!(self.dematrix_procedure, 0 | 1 | 3) {
            return Err(McEncodeError::BadConfig(format!(
                "dematrix_procedure={} ('00'/'01'/'11' only)",
                self.dematrix_procedure
            )));
        }
        if self.lfe && !(2..=15).contains(&self.lfe_allocation) {
            return Err(McEncodeError::BadConfig(format!(
                "lfe_allocation={} (2..=15)",
                self.lfe_allocation
            )));
        }
        Ok(())
    }

    /// Number of full-bandwidth presentation channels
    /// (`front + surround`) the encode consumes, in
    /// [`McConfig::layout`] order: L, R, [C], [LS, RS | S].
    pub fn presentation_channels(&self) -> usize {
        usize::from(self.front) + usize::from(self.surround)
    }

    /// The §2.5.1.13 `mc_header()` this configuration emits.
    pub fn mc_header(&self) -> McHeader {
        McHeader {
            ext_bit_stream_present: false,
            n_ad_bytes: 0,
            centre: if self.front == 3 {
                Centre::Present
            } else {
                Centre::None
            },
            surround: match self.surround {
                0 => Surround::None,
                1 => Surround::Mono,
                _ => Surround::Stereo,
            },
            lfe: self.lfe,
            audio_mix: false,
            dematrix_procedure: self.dematrix_procedure,
            no_of_multi_lingual_ch: 0,
            multi_lingual_fs_half: false,
            multi_lingual_layer3: false,
            copyright_identification_bit: false,
            copyright_identification_start: false,
        }
    }
}

/// Cross-frame encode state: the base pair's [`EncodeFrameState`]
/// (§C.1.3 X ring buffers + §D.1 window history), one analysis
/// filterbank per transmission channel, and the transmission channels'
/// §D.1 Step-1 window-delay history.
#[derive(Debug, Default)]
pub struct McEncodeState {
    base: EncodeFrameState,
    tx_fb: Vec<AnalysisFilterbank>,
    tx_hist: Vec<Vec<f64>>,
    /// Mirror filterbanks producing the encoder-side `T0`/`T1`
    /// subband signals the §2.5.3.2.1.3 predictors are fitted
    /// against (fed the same `Lo`/`Ro` PCM as the base encode, so
    /// their X ring buffers stay in lockstep with it).
    t01_fb: Vec<AnalysisFilterbank>,
}

impl McEncodeState {
    /// Fresh state (zeroed filterbanks and histories).
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-zero everything (seek / discontinuity).
    pub fn reset(&mut self) {
        self.base.reset();
        for fb in self.tx_fb.iter_mut().chain(self.t01_fb.iter_mut()) {
            fb.reset();
        }
        for h in &mut self.tx_hist {
            h.iter_mut().for_each(|s| *s = 0.0);
        }
    }

    fn ensure_tx_channels(&mut self, n: usize) {
        while self.tx_fb.len() < n {
            self.tx_fb.push(AnalysisFilterbank::new());
        }
        while self.tx_hist.len() < n {
            self.tx_hist.push(vec![0.0; MODEL1_WINDOW_DELAY_SAMPLES]);
        }
    }

    fn ensure_t01_channels(&mut self) {
        while self.t01_fb.len() < 2 {
            self.t01_fb.push(AnalysisFilterbank::new());
        }
    }
}

/// One frame's §2.5.3.2.1.3 prediction election: per subband group
/// 0..7, whether prediction is signalled, and the quantized wire
/// coefficient `v` of predictor `px = 2·mch + src` (`127` = zero =
/// `predsi 0`). All predictors are first-order with zero delay
/// compensation.
struct PredPlan {
    on: [bool; 8],
    coef_v: [[u8; 6]; 8],
    npred: usize,
}

impl PredPlan {
    fn any(&self) -> bool {
        self.on.iter().any(|&b| b)
    }

    fn predsi(&self, sbgr: usize, px: usize) -> u8 {
        u8::from(self.coef_v[sbgr][px] != 127)
    }

    /// Extra extension bits this election costs: the 8 per-sbgr flags
    /// plus, per enabled group, `2·npred` predsi bits and
    /// `3 + 8` bits per transmitted coefficient.
    fn extra_bits(&self) -> u32 {
        let mut bits = 8u32;
        for sbgr in 0..8 {
            if !self.on[sbgr] {
                continue;
            }
            bits += 2 * self.npred as u32;
            for px in 0..self.npred {
                if self.predsi(sbgr, px) != 0 {
                    bits += 3 + 8;
                }
            }
        }
        bits
    }
}

/// Quantize a predictor coefficient to the §2.5.3.2.1.3 wire grid
/// `c = (v − 127)/32`, `v ∈ 0..=255`.
fn quantize_pred_coef(c: f64) -> u8 {
    (c * 32.0 + 127.0).round().clamp(0.0, 255.0) as u8
}

/// Fit the §2.5.3.2.1.3 predictors for subband groups 0..7 and, where
/// a group measurably wins (≥ 10 % residual-energy reduction across
/// its channels), replace the transmission channels' subband samples
/// with the prediction *error*. Returns the election.
fn fit_and_apply_prediction(
    tx_sub: &mut [Box<[[f64; SLOTS]; NUM_SUBBANDS]>],
    t01: &[Box<[[f64; SLOTS]; NUM_SUBBANDS]>],
) -> PredPlan {
    let nmch = tx_sub.len();
    let mut plan = PredPlan {
        on: [false; 8],
        coef_v: [[127u8; 6]; 8],
        npred: 2 * nmch,
    };
    for sbgr in 0..8usize {
        let sb = sbgr; // groups 0..7 are single subbands (§2.5.2.15)
        let a = &t01[0][sb];
        let b = &t01[1][sb];
        let (aa, bb, ab) = {
            let (mut aa, mut bb, mut ab) = (0.0f64, 0.0f64, 0.0f64);
            for t in 0..SLOTS {
                aa += a[t] * a[t];
                bb += b[t] * b[t];
                ab += a[t] * b[t];
            }
            (aa, bb, ab)
        };
        let det = aa * bb - ab * ab;
        let mut orig_total = 0.0f64;
        let mut resid_total = 0.0f64;
        let mut coefs = [[0.0f64; 2]; 5];
        for (mch, tx_ch) in tx_sub.iter().enumerate() {
            let x = &tx_ch[sb];
            let (mut ax, mut bx, mut xx) = (0.0f64, 0.0f64, 0.0f64);
            for t in 0..SLOTS {
                ax += a[t] * x[t];
                bx += b[t] * x[t];
                xx += x[t] * x[t];
            }
            // Solve the 2×2 normal equations; a near-singular system
            // (silent or mono-collapsed base pair) keeps zero
            // coefficients.
            let (mut c0, mut c1) = if det.abs() > 1e-18 {
                ((bb * ax - ab * bx) / det, (aa * bx - ab * ax) / det)
            } else {
                (0.0, 0.0)
            };
            // Quantize to the wire grid, then measure the *quantized*
            // predictor's residual — that is what the decoder adds.
            c0 = f64::from(i32::from(quantize_pred_coef(c0)) - 127) / 32.0;
            c1 = f64::from(i32::from(quantize_pred_coef(c1)) - 127) / 32.0;
            let mut resid = 0.0f64;
            for t in 0..SLOTS {
                let e = x[t] - c0 * a[t] - c1 * b[t];
                resid += e * e;
            }
            // Per-channel guard: keep the predictor only when it
            // helps this channel.
            if resid <= xx {
                coefs[mch] = [c0, c1];
                orig_total += xx;
                resid_total += resid;
            } else {
                coefs[mch] = [0.0, 0.0];
                orig_total += xx;
                resid_total += xx;
            }
        }
        // Group election: ≥ 10 % energy win across the channels.
        if orig_total > 0.0 && resid_total <= 0.9 * orig_total {
            plan.on[sbgr] = true;
            for (mch, tx_ch) in tx_sub.iter_mut().enumerate() {
                let [c0, c1] = coefs[mch];
                plan.coef_v[sbgr][2 * mch] = quantize_pred_coef(c0);
                plan.coef_v[sbgr][2 * mch + 1] = quantize_pred_coef(c1);
                if c0 == 0.0 && c1 == 0.0 {
                    continue;
                }
                for t in 0..SLOTS {
                    tx_ch[sb][t] -= c0 * a[t] + c1 * b[t];
                }
            }
        }
    }
    plan
}

/// §2.5.3.3 matrixing constants `(α, β, γ)` for a dematrix procedure.
/// Procedure `'11'` performs no matrixing (all ones, applied nowhere).
fn matrix_coeffs(proc_: u8) -> (f64, f64, f64) {
    match proc_ {
        1 => (1.0 / (1.5 + 0.5 * SQRT2), 1.0 / SQRT2, 0.5),
        3 => (1.0, 1.0, 1.0),
        _ => (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2),
    }
}

/// One frame's matrixing: presentation channels (in
/// [`McConfig::layout`] order) → the compatible pair `[Lo, Ro]` and
/// the weighted transmission channels `T2..` (in the `tc_allocation
/// = 0` role order: centre first, then surround).
fn matrix_downmix(cfg: &McEncodeConfig, pcm: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let n = pcm[0].len();
    let front = usize::from(cfg.front);
    let (alpha, beta, gamma) = matrix_coeffs(cfg.dematrix_procedure);
    let no_matrix = cfg.dematrix_procedure == 3;
    // Weight of the centre / surround signals on the wire.
    let w_c = if no_matrix { 1.0 } else { alpha * beta };
    let w_s = if no_matrix { 1.0 } else { alpha * gamma };

    let nmch = match (cfg.front, cfg.surround) {
        (3, 2) => 3,
        (3, 1) | (2, 2) => 2,
        (3, 0) | (2, 1) => 1,
        _ => 0,
    };
    let mut base = vec![vec![0.0f64; n]; 2];
    let mut tx = vec![vec![0.0f64; n]; nmch];
    for i in 0..n {
        let l = pcm[0][i];
        let r = pcm[1][i];
        let c = if cfg.front == 3 { pcm[2][i] } else { 0.0 };
        let (lsur, rsur) = match cfg.surround {
            2 => (pcm[front][i], pcm[front + 1][i]),
            1 => (pcm[front][i], pcm[front][i]),
            _ => (0.0, 0.0),
        };
        if no_matrix {
            base[0][i] = l;
            base[1][i] = r;
        } else {
            base[0][i] = alpha * (l + beta * c + gamma * lsur);
            base[1][i] = alpha * (r + beta * c + gamma * rsur);
        }
        let mut t = 0usize;
        if cfg.front == 3 {
            tx[t][i] = w_c * c;
            t += 1;
        }
        match cfg.surround {
            2 => {
                tx[t][i] = w_s * lsur;
                tx[t + 1][i] = w_s * rsur;
            }
            1 => {
                tx[t][i] = w_s * lsur;
            }
            _ => {}
        }
    }
    (base, tx)
}

/// Feed bits `[from, to)` of a packed buffer into the §2.4.3.1 CRC-16
/// shift register (the same register the §2.5.2.14 `mc_crc_check`
/// uses).
fn crc_feed(mut reg: u16, bytes: &[u8], from: u64, to: u64) -> u16 {
    for i in from..to {
        let bit = (bytes[(i / 8) as usize] >> (7 - (i % 8))) & 1;
        reg = crc16_step(reg, bit != 0);
    }
    reg
}

/// Overwrite `nbits` bits of `frame` starting at absolute bit
/// `start_bit` with the leading bits of `blob`.
fn splice_bits(frame: &mut [u8], start_bit: u64, blob: &[u8], nbits: usize) {
    for i in 0..nbits {
        let bit = (blob[i / 8] >> (7 - (i % 8))) & 1;
        let pos = start_bit + i as u64;
        let byte = (pos / 8) as usize;
        let sh = 7 - (pos % 8) as u32;
        frame[byte] = (frame[byte] & !(1 << sh)) | (bit << sh);
    }
}

/// Per-transmission-channel prepared coding decisions.
struct TxPlan {
    /// `nb_steps` per subband (0 = no allocation).
    nb_steps: Vec<u32>,
    /// Wire allocation index per subband.
    alloc_idx: Vec<u32>,
    /// §C.1.5.2.5 scfsi selection per subband (valid where allocated).
    scfsi: Vec<ScfsiSelection>,
}

/// The §C.1.5.2.7 minimum-MNR greedy allocation for the transmission
/// channels, against the Table B.2a / B.2b ladder with
/// `msblimit = sblimit` (§2.5.2.17) and an explicit variable-bit
/// budget. Activation of a slot pays its scfsi (2 bits) plus the
/// *exact* transmitted-scalefactor cost of its Table C.4 selection;
/// each quantizer step pays the exact §2.4.3.3.4 sample-bit delta.
fn allocate_mc_bits(
    table: BitAllocTable,
    msblimit: usize,
    smr_db: &[Vec<f64>],
    scfsi: &[Vec<ScfsiSelection>],
    mut budget: i64,
) -> Result<Vec<TxPlan>, McEncodeError> {
    let nmch = smr_db.len();
    let mut nb_steps = vec![vec![0u32; NUM_SUBBANDS]; nmch];
    let mut row_idx = vec![vec![0u32; NUM_SUBBANDS]; nmch];
    let mut eligible = vec![vec![true; NUM_SUBBANDS]; nmch];
    let mut mnr = vec![vec![0.0f64; NUM_SUBBANDS]; nmch];
    for m in 0..nmch {
        for sb in 0..msblimit {
            mnr[m][sb] = -smr_db[m][sb];
            if table.nbal(sb) == 0 {
                eligible[m][sb] = false;
            }
        }
    }
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for m in 0..nmch {
            for sb in 0..msblimit {
                if !eligible[m][sb] {
                    continue;
                }
                let width = 1u32 << table.nbal(sb);
                if row_idx[m][sb] + 1 >= width {
                    eligible[m][sb] = false;
                    continue;
                }
                match best {
                    Some((_, _, bm)) if bm <= mnr[m][sb] => {}
                    _ => best = Some((m, sb, mnr[m][sb])),
                }
            }
        }
        let Some((m, sb, _)) = best else { break };
        let next_row = row_idx[m][sb] + 1;
        let cur_nb = nb_steps[m][sb];
        let next_nb = table
            .nb_steps(sb, next_row)
            .ok_or_else(|| McEncodeError::Internal(format!("nb_steps({sb}, {next_row})")))?;
        let mut delta = i64::from(sample_bits_for(next_nb)) - i64::from(sample_bits_for(cur_nb));
        if cur_nb == 0 && next_nb != 0 {
            delta += i64::from(SCFSI_BITS_PER_SLOT)
                + 6 * scfsi[m][sb].pattern.transmitted_count() as i64;
        }
        if delta > budget {
            eligible[m][sb] = false;
            continue;
        }
        budget -= delta;
        row_idx[m][sb] = next_row;
        nb_steps[m][sb] = next_nb;
        mnr[m][sb] = snr_db(next_nb).unwrap_or(0.0) - smr_db[m][sb];
    }

    let mut out = Vec::with_capacity(nmch);
    for m in 0..nmch {
        let mut alloc_idx = vec![0u32; NUM_SUBBANDS];
        for sb in 0..msblimit {
            alloc_idx[sb] = table
                .allocation_index(sb, nb_steps[m][sb])
                .ok_or_else(|| McEncodeError::Internal(format!("allocation_index({sb})")))?;
        }
        out.push(TxPlan {
            nb_steps: nb_steps[m].clone(),
            alloc_idx,
            scfsi: scfsi[m].clone(),
        });
    }
    Ok(out)
}

/// The 2-bit §2.4.3.3.2 wire code of a [`Scfsi`] schedule.
fn scfsi_code(s: Scfsi) -> u32 {
    match s {
        Scfsi::ThreePerGranule => 0,
        Scfsi::Share01Then2 => 1,
        Scfsi::ShareAll => 2,
        Scfsi::Share0Then12 => 3,
    }
}

/// Validate the base header for a §2.5 multichannel encode.
fn validate_base_header(header: &FrameHeader) -> Result<(), McEncodeError> {
    if header.lsf {
        return Err(McEncodeError::BadBaseHeader(
            "the §2.5 extension is defined on an MPEG-1-compatible (full-rate) base".into(),
        ));
    }
    if header.mode != Mode::Stereo {
        return Err(McEncodeError::BadBaseHeader(format!(
            "base mode {:?} unsupported (Stereo compatible pair required)",
            header.mode
        )));
    }
    Ok(())
}

/// Encode one Layer II multichannel frame.
///
/// * `header` — the base frame header (MPEG-1 rate, `Stereo` mode).
/// * `pcm` — `cfg.presentation_channels()` channels of
///   [`PCM_SAMPLES_PER_CHANNEL`] samples each, in [`McConfig::layout`]
///   order (L, R, [C], [LS, RS | S]), nominal `[-1, +1]` range.
/// * `lfe` — exactly [`LFE_SAMPLES_PER_FRAME`] samples at `Fs / 96`
///   when `cfg.lfe`, `None` otherwise.
///
/// Returns the complete base frame with the `mc_extension()` spliced
/// into its §2.4.1.8 ancillary field; the result decodes through
/// [`crate::mc::decode_mc_frame_with`] to the presentation channels
/// and through the plain [`crate::frame::decode_frame`] to the
/// compatible stereo downmix.
pub fn encode_mc_frame_with(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
    state: &mut McEncodeState,
) -> Result<Vec<u8>, McEncodeError> {
    cfg.validate()?;
    validate_base_header(header)?;
    let n_present = cfg.presentation_channels();
    if pcm.len() != n_present {
        return Err(McEncodeError::BadPcmShape {
            have: pcm.len(),
            need: n_present,
        });
    }
    for ch in pcm {
        if ch.len() != PCM_SAMPLES_PER_CHANNEL {
            return Err(McEncodeError::BadPcmShape {
                have: ch.len(),
                need: PCM_SAMPLES_PER_CHANNEL,
            });
        }
    }
    match (cfg.lfe, lfe) {
        (true, Some(s)) if s.len() == LFE_SAMPLES_PER_FRAME => {}
        (true, Some(s)) => {
            return Err(McEncodeError::BadLfeShape {
                have: s.len(),
                need: LFE_SAMPLES_PER_FRAME,
            })
        }
        (true, None) => {
            return Err(McEncodeError::BadLfeShape {
                have: 0,
                need: LFE_SAMPLES_PER_FRAME,
            })
        }
        (false, Some(_)) => {
            return Err(McEncodeError::BadLfeShape {
                have: LFE_SAMPLES_PER_FRAME,
                need: 0,
            })
        }
        (false, None) => {}
    }

    let mc_header = cfg.mc_header();
    let mc_cfg = McConfig::from_header(&mc_header, header.mode);
    let nmch = mc_cfg.nmch;

    // ---- matrixing (§2.5.3.3) -----------------------------------------
    let (base_pcm, tx_pcm) = matrix_downmix(cfg, pcm);
    debug_assert_eq!(tx_pcm.len(), nmch);

    // ---- transmission-channel analysis + scalefactors ------------------
    // Table B.2a at 48 kHz, B.2b at 44,1 / 32 kHz regardless of bitrate,
    // msblimit = sblimit (§2.5.2.17).
    let mc_table = match header.sample_rate {
        48_000 => BitAllocTable::B2a,
        _ => BitAllocTable::B2b,
    };
    let msblimit = mc_table.sblimit();

    state.ensure_tx_channels(nmch);
    #[allow(unused_mut)]
    let mut tx_sub: Vec<Box<[[f64; SLOTS]; NUM_SUBBANDS]>> = Vec::with_capacity(nmch);
    for (m, ch) in tx_pcm.iter().enumerate() {
        let fb = &mut state.tx_fb[m];
        let mut sub = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut block = [0.0f64; NUM_SUBBANDS];
        let mut out_block = [0.0f64; NUM_SUBBANDS];
        for t in 0..SLOTS {
            block.copy_from_slice(&ch[t * NUM_SUBBANDS..(t + 1) * NUM_SUBBANDS]);
            fb.push_audio(&block, &mut out_block);
            for sb in 0..NUM_SUBBANDS {
                sub[sb][t] = out_block[sb];
            }
        }
        tx_sub.push(sub);
    }

    // ---- §2.5.3.2.1.3 multichannel prediction election ------------------
    // Fit first-order zero-delay predictors of the weighted
    // transmission channels from the encoder-side T0/T1 subband
    // signals, and transmit the prediction error where a subband
    // group measurably wins. Runs before scalefactor extraction so
    // the wire scalefactors describe the transmitted (error) signal.
    let pred_plan = if cfg.prediction && nmch > 0 {
        state.ensure_t01_channels();
        let mut t01_sub: Vec<Box<[[f64; SLOTS]; NUM_SUBBANDS]>> = Vec::with_capacity(2);
        for (bch, ch) in base_pcm.iter().enumerate() {
            let fb = &mut state.t01_fb[bch];
            let mut sub = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
            let mut block = [0.0f64; NUM_SUBBANDS];
            let mut out_block = [0.0f64; NUM_SUBBANDS];
            for t in 0..SLOTS {
                block.copy_from_slice(&ch[t * NUM_SUBBANDS..(t + 1) * NUM_SUBBANDS]);
                fb.push_audio(&block, &mut out_block);
                for sb in 0..NUM_SUBBANDS {
                    sub[sb][t] = out_block[sb];
                }
            }
            t01_sub.push(sub);
        }
        let plan = fit_and_apply_prediction(&mut tx_sub, &t01_sub);
        plan.any().then_some(plan)
    } else {
        None
    };

    // Per-channel scalefactors (also feeding the §D.1 Step-2 scf_max)
    // and scfsi selections.
    let mut tx_sf: Vec<[[u8; NUM_SUBBANDS]; 3]> = Vec::with_capacity(nmch);
    let mut tx_scfsi: Vec<Vec<ScfsiSelection>> = Vec::with_capacity(nmch);
    for m in 0..nmch {
        let sf = compute_scalefactors(&tx_sub[m], NUM_SUBBANDS);
        let mut sels = Vec::with_capacity(NUM_SUBBANDS);
        for sb in 0..NUM_SUBBANDS {
            sels.push(select_scfsi([sf[0][sb], sf[1][sb], sf[2][sb]]));
        }
        tx_sf.push(sf);
        tx_scfsi.push(sels);
    }

    // ---- §D.1 Model-1 SMR per transmission channel ----------------------
    let mut tx_smr: Vec<Vec<f64>> = vec![vec![0.0f64; NUM_SUBBANDS]; nmch];
    if let Some(fs) = annex_d_sampling_rate(header.sample_rate) {
        let per_ch_kbps = f64::from(header.bit_rate) / 1000.0 / (2 + nmch).max(1) as f64;
        for m in 0..nmch {
            let mut scf_max = [0.0f64; NUM_SUBBANDS];
            for sb in 0..NUM_SUBBANDS {
                let min_idx = tx_sf[m][0][sb].min(tx_sf[m][1][sb]).min(tx_sf[m][2][sb]);
                scf_max[sb] = crate::tables::SCALEFACTORS[min_idx as usize];
            }
            let head = LAYER2_FFT_LEN - MODEL1_WINDOW_DELAY_SAMPLES;
            let mut window = Vec::with_capacity(LAYER2_FFT_LEN);
            window.extend_from_slice(&state.tx_hist[m]);
            window.extend_from_slice(&tx_pcm[m][..head]);
            let smr = compute_smr_model1_frame(&window, &scf_max, fs, per_ch_kbps);
            tx_smr[m][..NUM_SUBBANDS].copy_from_slice(&smr[..NUM_SUBBANDS]);
            let tail_at = tx_pcm[m].len() - MODEL1_WINDOW_DELAY_SAMPLES;
            let tail = tx_pcm[m][tail_at..].to_vec();
            state.tx_hist[m].copy_from_slice(&tail);
        }
    }

    // ---- extension bit budget ------------------------------------------
    let cb = (header.frame_size_bytes() as u32) * 8;
    let bcrc: u32 = if header.protection_bit { 0 } else { 16 };
    let lfe_nb = u32::from(cfg.lfe_allocation) + 1;
    // Fixed extension cost: mc_header(16) + mc_crc(16) + composite
    // status (tc_sbgr_select + dyn_cross_on + mc_prediction_on + the
    // single global tc_allocation) + allocation fields + the LFE block.
    let mut fixed: u32 = 16 + 16 + 3 + mc_cfg.tc_allocation_bits;
    if let Some(plan) = &pred_plan {
        fixed += plan.extra_bits();
    }
    if cfg.lfe {
        fixed += 4 + 6 + 12 * lfe_nb;
    }
    for sb in 0..msblimit {
        fixed += mc_table.nbal(sb) * nmch as u32;
    }
    let avail = cb.saturating_sub(32 + bcrc);
    let budget = match cfg.mc_bits {
        Some(b) => b,
        None => {
            // Proportional split of the post-fixed-cost data bits:
            // nmch / (2 + nmch) of what remains goes to the extension.
            let var = avail.saturating_sub(fixed) as f64 * nmch as f64 / (2 + nmch) as f64;
            fixed + var as u32
        }
    };
    if budget < fixed || budget > avail {
        return Err(McEncodeError::BudgetTooSmall {
            fixed,
            budget: budget.min(avail),
        });
    }

    // ---- greedy MC allocation -------------------------------------------
    let plans = allocate_mc_bits(
        mc_table,
        msblimit,
        &tx_smr,
        &tx_scfsi,
        i64::from(budget - fixed),
    )?;

    // ---- LFE quantization (§2.5.3.2.4) -----------------------------------
    let (lfe_alloc_field, lf_scf, lfe_class) = if cfg.lfe {
        let s = lfe.expect("validated above");
        let max_abs = s.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
        let class = class_of_quantization((1u32 << lfe_nb) - 1)
            .ok_or_else(|| McEncodeError::Internal(format!("no class for nb={lfe_nb}")))?;
        debug_assert!(!class.grouping, "LFE codes are ungrouped (§2.5.1.17)");
        (
            u32::from(cfg.lfe_allocation),
            pick_scalefactor_index(max_abs),
            Some(class),
        )
    } else {
        (0, 0, None)
    };

    // ---- serialise the extension (§2.5.1.12.1 / §2.5.1.17 wire order) ----
    let mut w = BitWriter::with_capacity((budget as usize).div_ceil(8) + 4);
    // mc_header (16 bits, no extension bit stream).
    w.write_u32(0, 1); // ext_bit_stream_present
    w.write_u32(
        match mc_header.centre {
            Centre::None => 0,
            Centre::Present => 1,
            Centre::Phantom => 3,
        },
        2,
    );
    w.write_u32(
        match mc_header.surround {
            Surround::None => 0,
            Surround::Mono => 1,
            Surround::Stereo => 2,
            Surround::SecondStereo => 3,
        },
        2,
    );
    w.write_u32(u32::from(mc_header.lfe), 1);
    w.write_u32(0, 1); // audio_mix
    w.write_u32(u32::from(mc_header.dematrix_procedure), 2);
    w.write_u32(0, 3); // no_of_multi_lingual_ch
    w.write_u32(0, 1); // multi_lingual_fs
    w.write_u32(0, 1); // multi_lingual_layer
    w.write_u32(0, 1); // copyright_identification_bit
    w.write_u32(0, 1); // copyright_identification_start
    debug_assert_eq!(w.bit_position(), 16);
    w.write_u32(0, 16); // mc_crc_check placeholder, patched below
    let status_start = w.bit_position();
    debug_assert_eq!(status_start, 32);
    // mc_composite_status_info (§2.5.1.15): global tc_allocation 0, no
    // dynamic crosstalk, no prediction.
    w.write_u32(1, 1); // tc_sbgr_select
    w.write_u32(0, 1); // dyn_cross_on
    w.write_u32(u32::from(pred_plan.is_some()), 1); // mc_prediction_on
    if mc_cfg.tc_allocation_bits > 0 {
        w.write_u32(0, mc_cfg.tc_allocation_bits);
    }
    if let Some(plan) = &pred_plan {
        // §2.5.1.15: per-sbgr mc_prediction flag + predsi fields.
        for sbgr in 0..8 {
            w.write_u32(u32::from(plan.on[sbgr]), 1);
            if plan.on[sbgr] {
                for px in 0..plan.npred {
                    w.write_u32(u32::from(plan.predsi(sbgr, px)), 2);
                }
            }
        }
    }
    // mc_audio_data (§2.5.1.17).
    if cfg.lfe {
        w.write_u32(lfe_alloc_field, 4);
    }
    for sb in 0..msblimit {
        for mch in 0..nmch {
            w.write_u32(plans[mch].alloc_idx[sb], mc_table.nbal(sb));
        }
    }
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if plans[mch].nb_steps[sb] != 0 {
                w.write_u32(scfsi_code(plans[mch].scfsi[sb].scfsi), 2);
            }
        }
    }
    let scfsi_end = w.bit_position();
    if let Some(plan) = &pred_plan {
        // §2.5.1.17: delay compensation + coefficients for every
        // transmitted predictor (first-order, zero delay).
        for sbgr in 0..8 {
            if !plan.on[sbgr] {
                continue;
            }
            for px in 0..plan.npred {
                if plan.predsi(sbgr, px) != 0 {
                    w.write_u32(0, 3); // delay_comp
                    w.write_u32(u32::from(plan.coef_v[sbgr][px]), 8);
                }
            }
        }
    }
    if cfg.lfe {
        w.write_u32(u32::from(lf_scf), 6);
    }
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if plans[mch].nb_steps[sb] == 0 {
                continue;
            }
            let sel = &plans[mch].scfsi[sb];
            match sel.scfsi {
                Scfsi::ThreePerGranule => {
                    for p in 0..3 {
                        w.write_u32(u32::from(sel.used[p]), 6);
                    }
                }
                Scfsi::Share01Then2 => {
                    w.write_u32(u32::from(sel.used[0]), 6);
                    w.write_u32(u32::from(sel.used[2]), 6);
                }
                Scfsi::ShareAll => {
                    w.write_u32(u32::from(sel.used[0]), 6);
                }
                Scfsi::Share0Then12 => {
                    w.write_u32(u32::from(sel.used[0]), 6);
                    w.write_u32(u32::from(sel.used[1]), 6);
                }
            }
        }
    }
    // Granule loop: LFE sample first, then one triplet per allocated
    // (sb, mch) slot.
    for gr in 0..12 {
        if let Some(class) = &lfe_class {
            let s = lfe.expect("validated above")[gr];
            let code = crate::encoder_samples::quantize_scaled(class, lf_scf, s)
                .map_err(|e| McEncodeError::Internal(format!("lfe quantize: {e}")))?;
            w.write_u32(code, lfe_nb);
        }
        let part = gr / 4;
        let base_slot = gr * 3;
        for sb in 0..msblimit {
            for mch in 0..nmch {
                let nb = plans[mch].nb_steps[sb];
                if nb == 0 {
                    continue;
                }
                let class = class_of_quantization(nb)
                    .ok_or_else(|| McEncodeError::Internal(format!("class {nb}")))?;
                let triplet = [
                    tx_sub[mch][sb][base_slot],
                    tx_sub[mch][sb][base_slot + 1],
                    tx_sub[mch][sb][base_slot + 2],
                ];
                write_triplet_scaled(&class, plans[mch].scfsi[sb].used[part], &triplet, &mut w)
                    .map_err(|e| McEncodeError::Internal(format!("sample write: {e}")))?;
            }
        }
    }
    let blob_bits = w.bit_position() as usize;
    debug_assert!(blob_bits as u32 <= budget, "{blob_bits} > {budget}");
    let mut blob = w.finish();

    // §2.5.2.14 mc_crc_check: "begins with the first bit of the
    // multichannel header and ends with the last bit of the scfsi
    // field, but excluding the mc_crc_check field itself."
    let mut reg = crc_feed(INIT_STATE, &blob, 0, 16);
    reg = crc_feed(reg, &blob, status_start, scfsi_end);
    blob[2] = (reg >> 8) as u8;
    blob[3] = (reg & 0xFF) as u8;

    // ---- base encode with the extension reservation ----------------------
    let mut frame = encode_frame_auto_with(header, &base_pcm, blob_bits as u32, &mut state.base)?;

    // ---- splice at the first §2.4.1.8 ancillary bit -----------------------
    let anc_start = base_ancillary_start_bit(&frame)
        .map_err(|e| McEncodeError::Internal(format!("re-parse: {e}")))?;
    let total_bits = frame.len() as u64 * 8;
    if total_bits - anc_start < blob_bits as u64 {
        return Err(McEncodeError::Internal(format!(
            "ancillary tail {} bits < extension {} bits despite banc reservation",
            total_bits - anc_start,
            blob_bits
        )));
    }
    splice_bits(&mut frame, anc_start, &blob, blob_bits);
    Ok(frame)
}

/// Batch multichannel encode: one continuous per-channel PCM buffer
/// (whole multiple of [`PCM_SAMPLES_PER_CHANNEL`] samples per channel,
/// [`McConfig::layout`] order) plus an optional LFE buffer at `Fs / 96`
/// ([`LFE_SAMPLES_PER_FRAME`] samples per frame) → the concatenated
/// Layer II multichannel byte stream, threading one [`McEncodeState`]
/// and the §2.4.2.3 [`PaddingScheduler`] across frames.
pub fn encode_mc_all_frames(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
) -> Result<Vec<u8>, McEncodeError> {
    cfg.validate()?;
    validate_base_header(header)?;
    let n_present = cfg.presentation_channels();
    if pcm.len() != n_present {
        return Err(McEncodeError::BadPcmShape {
            have: pcm.len(),
            need: n_present,
        });
    }
    let n = pcm[0].len();
    if n % PCM_SAMPLES_PER_CHANNEL != 0 {
        return Err(McEncodeError::BadPcmShape {
            have: n,
            need: n.next_multiple_of(PCM_SAMPLES_PER_CHANNEL),
        });
    }
    for ch in pcm {
        if ch.len() != n {
            return Err(McEncodeError::BadPcmShape {
                have: ch.len(),
                need: n,
            });
        }
    }
    let n_frames = n / PCM_SAMPLES_PER_CHANNEL;
    let lfe_need = if cfg.lfe {
        n_frames * LFE_SAMPLES_PER_FRAME
    } else {
        0
    };
    let lfe_have = lfe.map_or(0, <[f64]>::len);
    if lfe_have != lfe_need {
        return Err(McEncodeError::BadLfeShape {
            have: lfe_have,
            need: lfe_need,
        });
    }

    let mut state = McEncodeState::new();
    let mut padding = PaddingScheduler::new();
    let mut out = Vec::with_capacity(n_frames * (header.frame_size_bytes() + 1));
    let mut frame_pcm: Vec<Vec<f64>> = vec![Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL); pcm.len()];
    for f in 0..n_frames {
        let frame_header = padding.next_header(header);
        let base = f * PCM_SAMPLES_PER_CHANNEL;
        for (ch, plane) in pcm.iter().enumerate() {
            frame_pcm[ch].clear();
            frame_pcm[ch].extend_from_slice(&plane[base..base + PCM_SAMPLES_PER_CHANNEL]);
        }
        let frame_lfe = lfe.map(|s| &s[f * LFE_SAMPLES_PER_FRAME..(f + 1) * LFE_SAMPLES_PER_FRAME]);
        let bytes = encode_mc_frame_with(&frame_header, cfg, &frame_pcm, frame_lfe, &mut state)?;
        out.extend_from_slice(&bytes);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrixing_normalisation_bounds_the_compatible_pair() {
        // Full-scale presentation channels must produce |Lo|, |Ro| <= 1
        // for procedures '00' and '01' — the α attenuation exists
        // exactly for this (§2.5.3.2.5).
        for proc_ in [0u8, 1] {
            let cfg = McEncodeConfig {
                dematrix_procedure: proc_,
                ..McEncodeConfig::default()
            };
            let pcm: Vec<Vec<f64>> = vec![vec![1.0; 4]; 5];
            let (base, tx) = matrix_downmix(&cfg, &pcm);
            assert_eq!(tx.len(), 3);
            for ch in &base {
                for &s in ch {
                    assert!(
                        s.abs() <= 1.0 + 1e-12,
                        "proc {proc_}: compatible sample {s} out of range"
                    );
                }
            }
        }
    }

    #[test]
    fn matrix_weights_invert_through_the_dematrix_constants() {
        // α · denorm == 1, and the weighted signals recover through the
        // §2.5.3.2.5 inverse weighting: w_enc · w_dec · denorm == 1.
        for proc_ in [0u8, 1] {
            let (alpha, beta, gamma) = matrix_coeffs(proc_);
            let denorm = if proc_ == 1 {
                1.5 + 0.5 * SQRT2
            } else {
                1.0 + SQRT2
            };
            let w_c_dec = SQRT2;
            let w_s_dec = if proc_ == 1 { 2.0 } else { SQRT2 };
            assert!((alpha * denorm - 1.0).abs() < 1e-12);
            assert!((alpha * beta * w_c_dec * denorm - 1.0).abs() < 1e-12);
            assert!((alpha * gamma * w_s_dec * denorm - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn config_validation_rejects_out_of_range_fields() {
        let ok = McEncodeConfig::default();
        assert!(ok.validate().is_ok());
        for bad in [
            McEncodeConfig {
                front: 1,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                surround: 3,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                dematrix_procedure: 2,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                lfe: true,
                lfe_allocation: 1,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                lfe: true,
                lfe_allocation: 16,
                ..McEncodeConfig::default()
            },
        ] {
            assert!(matches!(bad.validate(), Err(McEncodeError::BadConfig(_))));
        }
    }

    #[test]
    fn splice_bits_overwrites_the_exact_range() {
        let mut frame = vec![0xFFu8; 4];
        let blob = [0b1010_1010u8, 0b1100_0000];
        splice_bits(&mut frame, 5, &blob, 10);
        // Bits 5..15 replaced with 1010101011; bits 0..5 and 15.. stay 1.
        assert_eq!(frame[0], 0b1111_1101);
        assert_eq!(frame[1], 0b0101_0111);
        assert_eq!(frame[2], 0xFF);
    }

    #[test]
    fn pred_coef_quantizer_matches_the_wire_grid() {
        // c = (v − 127)/32 round-trips for every representable value,
        // clamps outside the grid, and 0 ↦ 127 (the predsi-0 sentinel).
        assert_eq!(quantize_pred_coef(0.0), 127);
        assert_eq!(quantize_pred_coef(1.0), 127 + 32);
        assert_eq!(quantize_pred_coef(-1.0), 127 - 32);
        assert_eq!(quantize_pred_coef(100.0), 255);
        assert_eq!(quantize_pred_coef(-100.0), 0);
        for v in 0u8..=255 {
            let c = f64::from(i32::from(v) - 127) / 32.0;
            assert_eq!(quantize_pred_coef(c), v, "v={v}");
        }
    }

    #[test]
    fn prediction_fit_recovers_a_planted_linear_relation() {
        // Plant x = 0,5·a + 0,25·b (grid-exact coefficients) in every
        // predictable subband: the fit must enable all eight groups,
        // recover the coefficients, and leave a near-zero residual.
        let mut a = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut b = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut x = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        for sb in 0..8 {
            for t in 0..SLOTS {
                a[sb][t] = rand();
                b[sb][t] = rand();
                x[sb][t] = 0.5 * a[sb][t] + 0.25 * b[sb][t];
            }
        }
        let mut tx = vec![x];
        let t01 = vec![a, b];
        let plan = fit_and_apply_prediction(&mut tx, &t01);
        assert!(plan.any());
        for sbgr in 0..8 {
            assert!(plan.on[sbgr], "sbgr {sbgr}");
            assert_eq!(plan.coef_v[sbgr][0], 127 + 16, "c0 = 0,5");
            assert_eq!(plan.coef_v[sbgr][1], 127 + 8, "c1 = 0,25");
            assert_eq!(plan.predsi(sbgr, 0), 1);
            assert_eq!(plan.predsi(sbgr, 1), 1);
            // The transmitted signal is the (here: zero) error.
            for t in 0..SLOTS {
                assert!(tx[0][sbgr][t].abs() < 1e-12, "sbgr {sbgr} slot {t}");
            }
        }
        // predsi 1 per transmitted coefficient → 3 + 8 bits each.
        assert_eq!(plan.extra_bits(), 8 + 8 * (2 * 2 + 2 * 11));
    }

    #[test]
    fn prediction_election_only_fires_on_a_measured_energy_win() {
        // Independent x/a/b: over 36 samples a least-squares fit can
        // still cross the 10 % bar by chance — and when it does, the
        // win is *real* for those very samples (the decoder adds back
        // exactly what was subtracted; only the coefficient bits are
        // spent). The invariant to pin is therefore not "off on random
        // data" but: an OFF group leaves its samples untouched, and an
        // ON group's transmitted residual genuinely carries ≤ 90 % of
        // the original energy.
        let mut a = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut b = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut x = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        for sb in 0..8 {
            for t in 0..SLOTS {
                a[sb][t] = rand();
                b[sb][t] = rand();
                x[sb][t] = rand();
            }
        }
        let before = x.clone();
        let mut tx = vec![x];
        let t01 = vec![a, b];
        let plan = fit_and_apply_prediction(&mut tx, &t01);
        let energy = |v: &[f64; SLOTS]| v.iter().map(|s| s * s).sum::<f64>();
        for sb in 0..8 {
            if plan.on[sb] {
                assert!(
                    energy(&tx[0][sb]) <= 0.9 * energy(&before[sb]) + 1e-12,
                    "sb {sb}: enabled without the 10 % win"
                );
            } else {
                assert_eq!(tx[0][sb], before[sb], "sb {sb} untouched");
                assert_eq!(plan.coef_v[sb], [127u8; 6], "sb {sb} zero coefs");
            }
        }
    }

    #[test]
    fn lfe_class_exists_and_is_ungrouped_for_every_offered_allocation() {
        for alloc in 2u8..=15 {
            let nb = u32::from(alloc) + 1;
            let class = class_of_quantization((1u32 << nb) - 1)
                .unwrap_or_else(|| panic!("no class for lfe_allocation {alloc}"));
            assert!(!class.grouping, "lfe_allocation {alloc}");
            assert_eq!(class.bits_per_codeword, nb, "lfe_allocation {alloc}");
        }
    }
}
