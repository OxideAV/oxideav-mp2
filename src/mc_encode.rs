//! ISO/IEC 13818-3 §2.5 **multichannel extension** encode for Layer II
//! — the encode-side dual of [`crate::mc`].
//!
//! A multichannel encode produces an ordinary ISO/IEC 11172-3 Layer II
//! frame whose §2.4.1.8 ancillary field carries the `mc_extension()`
//! payload (§2.5.1.3), optionally continued in a §2.5.1.5 extension
//! frame. The pipeline per frame:
//!
//! 1. **Matrixing** (§C.2.1.5): the presentation channels are combined
//!    into the MPEG-1-compatible pair `Lo` / `Ro` and the *weighted*
//!    audio signals `Lw, Rw, Cw, LSw, RSw` (`Sw`). For
//!    `dematrix_procedure` `'00'` the compatible pair is
//!    `Lo = α(L + βC + γLS)`, `Ro = α(R + βC + γRS)` with
//!    `α = 1/(1+√2)`, `β = γ = 1/√2`; for `'01'`
//!    `α = 1/(1,5 + 0,5·√2)`, `γ = 0,5`; for `'10'` (phase-mixed
//!    surround) `Lo = α(L + βC − γ·jS)`, `Ro = α(R + βC + γ·jS)` with
//!    the monophonic surround `jS = (LS + RS)/2` (or `S`); `'11'`
//!    transmits every signal unmatrixed. The α normalisation is
//!    exactly the attenuation §2.5.3.2.5's de-normalisation undoes,
//!    and bounds `|Lo|, |Ro| ≤ 1` for presentation channels inside the
//!    §2.4.3.4.7.1 nominal range.
//! 2. **Analysis** (§C.1.3): every weighted signal runs its own
//!    filterbank; scalefactors and a §D.1 Model-1 SMR are derived per
//!    signal.
//! 3. **Transmission-channel switching** (§2.5.2.15 / §C.2.1.6): per
//!    subband group the `tc_allocation` row is either the configured
//!    global value (`tc_sbgr_select = '1'`) or elected per group by
//!    the §C.2.1.6 rule (the signals with the lowest maximum
//!    scalefactor ride `T2..T4`), and the transmission channels are
//!    assembled in the subband domain.
//! 4. **Phantom-centre coding** (§2.5.2.13 / §C.2.1.9): the centre's
//!    subbands 12 and above are folded at −3 dB into `Lw` / `Rw` and
//!    signalled `centre = '11'` (`centre_limited` ⇒ no allocation).
//! 5. **Base encode**: `Lo` / `Ro` run the ordinary §C.1.5.2.7 Layer II
//!    encode with a `banc` ancillary reservation equal to the
//!    extension budget, so a §2.5-unaware decoder plays the compatible
//!    stereo downmix. The encoded base is re-read (the decoder's own
//!    §2.5.3.1 base pass) so the dynamic-crosstalk and prediction
//!    decisions below see exactly the `T0` / `T1` samples the decoder
//!    will.
//! 6. **Dynamic crosstalk** (§2.5.2.15 / §C.2.1.7): per subband group
//!    every legal `dyn_cross_mode` is scored against the decoder's
//!    reconstruction (source samples re-scaled by the substituted
//!    channel's own scalefactors) and the mode dropping the most
//!    transmission channels within a bounded substitution error wins.
//! 7. **Prediction** (§2.5.3.2.1.3 / §C.2.1.8): first-order zero-delay
//!    predictors of the transmitted channels from the decoded `T0` /
//!    `T1`, transmitted where a group measurably wins.
//! 8. **MC allocation**: the §C.1.5.2.7 minimum-MNR greedy procedure
//!    against Table B.2a / B.2b with `msblimit = sblimit` (§2.5.2.17),
//!    then serialisation in the §2.5.1.12.1 / §2.5.1.17 / §2.5.1.18
//!    wire order with the §2.5.2.14 `mc_crc_check`.
//! 9. **LFE** (§2.5.3.2.4), **second stereo programme** (`surround =
//!    '11'`, `L2` / `R2` transmitted unmatrixed) and **multilingual
//!    channels** (§2.5.2.18 `ml_audio_data()`, full or half sampling
//!    frequency).
//! 10. **Splice / spill**: the serialised extension is written into the
//!     encoded base frame starting at the first ancillary bit; what does
//!     not fit is carried by a §2.5.1.5 extension frame
//!     (`ext_header()` + `ext_data()`) when the configuration allows an
//!     extension bit stream, or refused otherwise.
//!
//! Clean-room: the syntax, wire order and matrixing equations are read
//! from ISO/IEC 13818-3 (1997) §2.5.1 / §2.5.2 / §2.5.3 / Annex C.2
//! only, mirrored against this crate's own §2.5 decoder.

// The §2.5.1 syntax loops are written in the spec's index-based
// `for (sb…) for (mch…)` notation so the wire order stays visually
// checkable against the printed syntax tables (same convention as
// `crate::mc`).
#![allow(clippy::needless_range_loop)]

use crate::analysis::AnalysisFilterbank;
use crate::audio_data::Scfsi;
use crate::bitalloc::{class_of_quantization, BitAllocTable, NUM_SUBBANDS};
use crate::crc::{crc16_step, INIT_STATE};
use crate::encoder_bit_allocator::{snr_db, SCFSI_BITS_PER_SLOT};
use crate::encoder_frame::{
    encode_frame_auto_with, EncodeError, EncodeFrameState, MODEL1_WINDOW_DELAY_SAMPLES,
};
use crate::encoder_samples::write_triplet_scaled;
use crate::encoder_scalefactors::{pick_scalefactor_index, SUBBAND_SAMPLES_PER_FRAME};
use crate::encoder_scfsi::{select_scfsi, ScfsiSelection};
use crate::frame::PCM_SAMPLES_PER_CHANNEL;
use crate::header::{FrameHeader, Mode, PaddingScheduler};
use crate::mc::{
    decode_base_subbands, dyn_cross_sources, fallback_base_channel, npred_for,
    predictable_channels, sbgr_of_subband, tc_roles, BaseSubbands, Centre, McChannel, McConfig,
    McHeader, Surround, TcSource, SBGR_BOUNDS,
};
use crate::psy::{annex_d_sampling_rate, compute_smr_model1_frame, LAYER2_FFT_LEN};
use crate::tables::SCALEFACTORS;
use oxideav_core::bits::BitWriter;

/// Subband samples per subband per Layer II frame (12 granules × 3).
const SLOTS: usize = SUBBAND_SAMPLES_PER_FRAME;
/// Subband samples per scalefactor part (§2.4.3.3.3: three parts).
const SLOTS_PER_PART: usize = SLOTS / 3;
/// LFE samples per frame (`Fs / 96` ⇒ 1152 / 96 = 12, §2.5.3.2.4).
pub const LFE_SAMPLES_PER_FRAME: usize = crate::mc::LFE_SAMPLES_PER_FRAME;
/// PCM samples per frame of a half-sampling-frequency multilingual
/// channel (§2.5.2.18: 6 granules of 3 × 32 samples).
pub const ML_HALF_RATE_SAMPLES_PER_FRAME: usize = PCM_SAMPLES_PER_CHANNEL / 2;
/// The §2.5.2.10 `ext_syncword`.
const EXT_SYNCWORD: u32 = 0b0111_1111_1111;
/// `ext_header()` size in bytes (12 + 16 + 11 + 1 bits).
const EXT_HEADER_BYTES: usize = 5;
/// Largest `ext_length` the 11-bit field can carry (bytes).
const EXT_MAX_BYTES: usize = 2047;
/// Dynamic-crosstalk substitution bound: a transmission channel is
/// dropped in a subband group only when the decoder's substitute
/// (§2.5.2.15 copy rule, re-scaled by the channel's own scalefactors)
/// leaves at most this fraction of the channel's energy as error
/// (10 dB substitution SNR).
const DYN_CROSS_MAX_ERROR_RATIO: f64 = 0.1;

/// √2, spelled once.
const SQRT2: f64 = std::f64::consts::SQRT_2;

/// Errors raised by the §2.5 multichannel encode.
#[derive(Debug, Clone, PartialEq)]
pub enum McEncodeError {
    /// The [`McEncodeConfig`] is not one this encoder can emit (see the
    /// field docs for the legal combinations).
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
    /// The multilingual input does not match `cfg.multilingual`
    /// channels of [`PCM_SAMPLES_PER_CHANNEL`] (full rate) /
    /// [`ML_HALF_RATE_SAMPLES_PER_FRAME`] (half rate) samples per frame.
    BadMlShape { have: usize, need: usize },
    /// The multichannel extension's fixed cost (mc_header, CRC,
    /// composite status, allocation fields, LFE) already exceeds the
    /// extension bit budget — the frame bitrate is too low for this
    /// configuration, or the budget exceeds the base frame's ancillary
    /// capacity and no extension bit stream was allowed.
    BudgetTooSmall { fixed: u32, budget: u32 },
    /// The extension spill does not fit the 11-bit `ext_length` field
    /// (§2.5.2.10: at most 2047 bytes per extension frame).
    ExtFrameTooLarge { need: usize },
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
            McEncodeError::BadMlShape { have, need } => {
                write!(f, "mc_encode: multilingual shape {have}, expected {need}")
            }
            McEncodeError::BudgetTooSmall { fixed, budget } => write!(
                f,
                "mc_encode: extension fixed cost {fixed} bits exceeds budget {budget}"
            ),
            McEncodeError::ExtFrameTooLarge { need } => write!(
                f,
                "mc_encode: extension frame needs {need} bytes, ext_length carries at most {EXT_MAX_BYTES}"
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
    /// §2.5.2.13 `dematrix_procedure`: `0` (`'00'`), `1` (`'01'`),
    /// `2` (`'10'`, phase-mixed surround — 3/1 and 3/2 only) or `3`
    /// (`'11'`, no matrixing).
    pub dematrix_procedure: u8,
    /// §2.5.2.17 `lfe_allocation` (quantizer index; `nb = value + 1`
    /// bits per LFE sample). Range `2..=15` — index 1 selects the
    /// 3-level quantizer whose Layer II class is *grouped*, but the
    /// §2.5.1.17 LFE field is a single ungrouped `nb`-bit code per
    /// granule, so this encoder starts at the 7-level class.
    pub lfe_allocation: u8,
    /// Extension bit budget override (mc_extension bits per frame,
    /// multilingual data included). `None` splits the frame's data
    /// bits between the base pair and the extension in proportion to
    /// their channel counts (a half-rate multilingual channel counts
    /// one half), plus the fixed LFE cost. With `ext_bit_stream` the
    /// budget may exceed the base frame's ancillary capacity — the
    /// remainder spills into the extension frame.
    pub mc_bits: Option<u32>,
    /// §2.5.2.15 global `tc_allocation` (used when `adaptive_tc` is
    /// off): which audio channels ride the transmission channels
    /// `T2..T4` (the rest are recovered by the decoding matrix from
    /// the compatible pair). Transmitted with `tc_sbgr_select = '1'`.
    /// Legal ranges: 3/2 → `0..=7`, 3/1 → `0..=4` (`5` only with
    /// procedure `'10'`), 3/0 / 2/1 → `0..=2`, 2/2 → `0..=3`;
    /// procedure `'11'` requires `0`; Phantom coding restricts the
    /// value to rows carrying the centre (`0, 3, 4, 5` in 3/2,
    /// `0, 3, 4` in 3/1, `0` in 3/0). Default `0`.
    pub tc_allocation: u8,
    /// §C.2.1.6 **dynamic transmission channel switching**: elect the
    /// `tc_allocation` per subband group from the signals' maximum
    /// scalefactors (the quietest signals ride `T2..T4`), emitting
    /// `tc_sbgr_select = '0'` with twelve values when they differ.
    /// Ignored (forced `0`) under procedure `'11'`.
    pub adaptive_tc: bool,
    /// §C.2.1.7 **dynamic crosstalk**: per subband group, drop
    /// transmission channels whose decoder-side substitute (§2.5.2.15
    /// copy from a sibling `Txy` carrier or from `Lo` / `Ro`, re-scaled
    /// by the channel's own scalefactors) stays within a 10 dB
    /// substitution SNR, signalling the `dyn_cross_mode` that saves
    /// the most.
    pub dyn_cross: bool,
    /// §C.2.1.9 **Phantom coding of the centre channel** (`centre =
    /// '11'`): the centre's subbands 12 and above are not transmitted;
    /// their content is folded at −3 dB into `Lw` / `Rw` so the
    /// decoding matrix reproduces it as a phantom source. Requires
    /// `front == 3` and a matrixing procedure (`'00'` / `'01'` /
    /// `'10'`).
    pub phantom_centre: bool,
    /// Second stereo programme (`surround = '11'`): two extra
    /// presentation channels `L2`, `R2` follow the main programme in
    /// the input and are transmitted unmatrixed on the last two
    /// transmission channels. Requires `surround == 0`.
    pub second_stereo: bool,
    /// Number of §2.5.2.18 multilingual / commentary channels
    /// (`0..=7`), each encoded as an independent Layer II channel in
    /// `ml_audio_data()`.
    pub multilingual: u8,
    /// Multilingual channels at half the main sampling frequency
    /// (`multi_lingual_fs = '1'`: Table B.1 of ISO/IEC 13818-3, six
    /// granules per frame).
    pub multilingual_fs_half: bool,
    /// Allow a §2.5.1.5 **extension bit stream**: every frame sets
    /// `ext_bit_stream_present = '1'` and is paired with an
    /// `ext_frame()` carrying whatever part of the extension the base
    /// frame's ancillary field could not hold (a header-only frame
    /// when everything fit). Only [`encode_mc_frame_ext_with`] /
    /// [`encode_mc_all_frames_ext`] can return the extension frames.
    pub ext_bit_stream: bool,
    /// §2.5.3.2.1.3 **multichannel prediction**: when enabled the
    /// encoder fits one first-order, zero-delay predictor per
    /// (subband group 0..7, predictable transmission channel,
    /// compatible source `T0`/`T1`) by least squares against the
    /// *decoded* base pair (exactly what the decoder predicts from),
    /// quantizes the coefficients to the wire grid `(v − 127)/32`, and
    /// transmits the prediction *error* in the subbands whose group
    /// the fit measurably wins (≥ 10 % residual-energy reduction).
    pub prediction: bool,
}

impl Default for McEncodeConfig {
    /// 3/2 (five presentation channels), no LFE, procedure `'00'`,
    /// global `tc_allocation` 0, no crosstalk / prediction / phantom /
    /// second stereo / multilingual / extension bit stream.
    fn default() -> Self {
        McEncodeConfig {
            front: 3,
            surround: 2,
            lfe: false,
            dematrix_procedure: 0,
            lfe_allocation: 7,
            mc_bits: None,
            tc_allocation: 0,
            adaptive_tc: false,
            dyn_cross: false,
            phantom_centre: false,
            second_stereo: false,
            multilingual: 0,
            multilingual_fs_half: false,
            ext_bit_stream: false,
            prediction: false,
        }
    }
}

/// The `tc_allocation` values §2.5.2.15 allows for a configuration
/// (Phantom coding and the `'10'`-only 3/1 row 5 included).
fn legal_tc_values(cfg: &McEncodeConfig) -> Vec<u8> {
    if cfg.dematrix_procedure == 3 {
        return vec![0];
    }
    let all: Vec<u8> = match (cfg.front, cfg.surround) {
        (3, 2) => (0..=7).collect(),
        (3, 1) if cfg.dematrix_procedure == 2 => (0..=5).collect(),
        (3, 1) => (0..=4).collect(),
        (2, 2) => (0..=3).collect(),
        (3, 0) | (2, 1) => (0..=2).collect(),
        _ => vec![0],
    };
    if cfg.phantom_centre {
        let keep: &[u8] = match (cfg.front, cfg.surround) {
            (3, 2) => &[0, 3, 4, 5],
            (3, 1) => &[0, 3, 4],
            _ => &[0],
        };
        all.into_iter().filter(|v| keep.contains(v)).collect()
    } else {
        all
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
        if self.dematrix_procedure > 3 {
            return Err(McEncodeError::BadConfig(format!(
                "dematrix_procedure={} ('00'/'01'/'10'/'11')",
                self.dematrix_procedure
            )));
        }
        if self.dematrix_procedure == 2 && !(self.front == 3 && self.surround >= 1) {
            return Err(McEncodeError::BadConfig(
                "dematrix_procedure '10' can only occur with a 3/1 or 3/2 configuration (§2.5.2.13)"
                    .into(),
            ));
        }
        if self.lfe && !(2..=15).contains(&self.lfe_allocation) {
            return Err(McEncodeError::BadConfig(format!(
                "lfe_allocation={} (2..=15)",
                self.lfe_allocation
            )));
        }
        if self.second_stereo && self.surround != 0 {
            return Err(McEncodeError::BadConfig(
                "a second stereo programme (surround '11') excludes surround channels".into(),
            ));
        }
        if self.phantom_centre {
            if self.front != 3 {
                return Err(McEncodeError::BadConfig(
                    "phantom_centre needs a centre channel (front == 3)".into(),
                ));
            }
            if self.dematrix_procedure == 3 {
                return Err(McEncodeError::BadConfig(
                    "phantom_centre folds the centre's upper subbands through the decoding \
                     matrix; procedure '11' has none"
                        .into(),
                ));
            }
        }
        if self.multilingual > 7 {
            return Err(McEncodeError::BadConfig(format!(
                "multilingual={} (0..=7)",
                self.multilingual
            )));
        }
        let legal = legal_tc_values(self);
        if !self.adaptive_tc && !legal.contains(&self.tc_allocation) {
            return Err(McEncodeError::BadConfig(format!(
                "tc_allocation={} not in {legal:?} for this configuration (§2.5.2.15)",
                self.tc_allocation
            )));
        }
        Ok(())
    }

    /// Number of full-bandwidth presentation channels
    /// (`front + surround [+ 2]`) the encode consumes, in
    /// [`McConfig::layout`] order: L, R, [C], [LS, RS | S], [L2, R2].
    pub fn presentation_channels(&self) -> usize {
        usize::from(self.front)
            + usize::from(self.surround)
            + if self.second_stereo { 2 } else { 0 }
    }

    /// PCM samples per frame each multilingual channel supplies.
    pub fn multilingual_samples_per_frame(&self) -> usize {
        if self.multilingual_fs_half {
            ML_HALF_RATE_SAMPLES_PER_FRAME
        } else {
            PCM_SAMPLES_PER_CHANNEL
        }
    }

    /// The §2.5.1.13 `mc_header()` this configuration emits.
    pub fn mc_header(&self) -> McHeader {
        McHeader {
            ext_bit_stream_present: self.ext_bit_stream,
            n_ad_bytes: 0,
            centre: match (self.front, self.phantom_centre) {
                (3, true) => Centre::Phantom,
                (3, false) => Centre::Present,
                _ => Centre::None,
            },
            surround: match (self.surround, self.second_stereo) {
                (0, true) => Surround::SecondStereo,
                (0, false) => Surround::None,
                (1, _) => Surround::Mono,
                _ => Surround::Stereo,
            },
            lfe: self.lfe,
            audio_mix: false,
            dematrix_procedure: self.dematrix_procedure,
            no_of_multi_lingual_ch: self.multilingual,
            multi_lingual_fs_half: self.multilingual_fs_half,
            multi_lingual_layer3: false,
            copyright_identification_bit: false,
            copyright_identification_start: false,
        }
    }
}

/// One encoded multichannel frame: the base frame (extension spliced
/// into its ancillary field) and, when the configuration uses an
/// extension bit stream, the paired §2.5.1.5 `ext_frame()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McEncodedFrame {
    /// The MPEG-1-compatible base frame.
    pub base: Vec<u8>,
    /// The extension frame (`Some` iff `cfg.ext_bit_stream`).
    pub ext: Option<Vec<u8>>,
}

/// Cross-frame encode state: the base pair's [`EncodeFrameState`]
/// (§C.1.3 X ring buffers + §D.1 window history), one analysis
/// filterbank + §D.1 Step-1 window history per weighted audio signal
/// and per multilingual channel.
#[derive(Debug, Default)]
pub struct McEncodeState {
    base: EncodeFrameState,
    role_fb: Vec<AnalysisFilterbank>,
    role_hist: Vec<Vec<f64>>,
    ml_fb: Vec<AnalysisFilterbank>,
    ml_hist: Vec<Vec<f64>>,
}

impl McEncodeState {
    /// Fresh state (zeroed filterbanks and histories).
    pub fn new() -> Self {
        Self::default()
    }

    /// Re-zero everything (seek / discontinuity).
    pub fn reset(&mut self) {
        self.base.reset();
        for fb in self.role_fb.iter_mut().chain(self.ml_fb.iter_mut()) {
            fb.reset();
        }
        for h in self.role_hist.iter_mut().chain(self.ml_hist.iter_mut()) {
            h.iter_mut().for_each(|s| *s = 0.0);
        }
    }

    fn ensure_channels(
        fb: &mut Vec<AnalysisFilterbank>,
        hist: &mut Vec<Vec<f64>>,
        n: usize,
        hist_len: usize,
    ) {
        while fb.len() < n {
            fb.push(AnalysisFilterbank::new());
        }
        while hist.len() < n {
            hist.push(vec![0.0; hist_len]);
        }
        for h in hist.iter_mut() {
            if h.len() != hist_len {
                h.resize(hist_len, 0.0);
            }
        }
    }
}

/// One channel's frame of subband samples, `[sb][slot]`.
type SubbandFrame = Box<[[f64; SLOTS]; NUM_SUBBANDS]>;

/// Run one channel of PCM through its analysis filterbank
/// (`nslots` slots of 32 samples).
fn analyse(fb: &mut AnalysisFilterbank, pcm: &[f64], nslots: usize) -> SubbandFrame {
    let mut sub: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
    let mut block = [0.0f64; NUM_SUBBANDS];
    let mut out_block = [0.0f64; NUM_SUBBANDS];
    for t in 0..nslots {
        block.copy_from_slice(&pcm[t * NUM_SUBBANDS..(t + 1) * NUM_SUBBANDS]);
        fb.push_audio(&block, &mut out_block);
        for sb in 0..NUM_SUBBANDS {
            sub[sb][t] = out_block[sb];
        }
    }
    sub
}

/// Table B.1 scalefactor indices per `[part][sb]` for a frame of
/// `nslots` slots (three equal parts, §2.4.3.3.3 / §2.5.2.18).
fn scalefactors_of(sub: &SubbandFrame, nslots: usize) -> [[u8; NUM_SUBBANDS]; 3] {
    let per_part = nslots / 3;
    let mut out = [[62u8; NUM_SUBBANDS]; 3];
    for sb in 0..NUM_SUBBANDS {
        for p in 0..3 {
            let max_abs = sub[sb][p * per_part..(p + 1) * per_part]
                .iter()
                .fold(0.0f64, |m, &v| m.max(v.abs()));
            out[p][sb] = pick_scalefactor_index(max_abs);
        }
    }
    out
}

/// §D.1 Model-1 SMR for one channel: the 1024-sample analysis window
/// is the channel's history tail plus the head of this frame's PCM
/// (192-sample look-back for 1152-sample frames, per
/// [`MODEL1_WINDOW_DELAY_SAMPLES`]; a shorter half-rate multilingual
/// frame keeps a longer tail so the window is always full).
fn model1_smr(
    hist: &mut [f64],
    pcm: &[f64],
    sf: &[[u8; NUM_SUBBANDS]; 3],
    fs: crate::tables_d2::SamplingRate,
    per_ch_kbps: f64,
) -> Vec<f64> {
    let hist_len = hist.len();
    let head = LAYER2_FFT_LEN - hist_len;
    let mut scf_max = [0.0f64; NUM_SUBBANDS];
    for sb in 0..NUM_SUBBANDS {
        let min_idx = sf[0][sb].min(sf[1][sb]).min(sf[2][sb]);
        scf_max[sb] = SCALEFACTORS[min_idx as usize];
    }
    let mut window = Vec::with_capacity(LAYER2_FFT_LEN);
    window.extend_from_slice(hist);
    window.extend_from_slice(&pcm[..head.min(pcm.len())]);
    let smr = compute_smr_model1_frame(&window, &scf_max, fs, per_ch_kbps);
    let tail_at = pcm.len() - hist_len;
    hist.copy_from_slice(&pcm[tail_at..]);
    smr.to_vec()
}

/// History length keeping a full 1024-sample Model-1 window for
/// frames of `n` samples.
fn model1_hist_len(n: usize) -> usize {
    MODEL1_WINDOW_DELAY_SAMPLES.max(LAYER2_FFT_LEN.saturating_sub(n))
}

/// One frame's §2.5.3.2.1.3 prediction election: per subband group
/// 0..7, whether prediction is signalled, and the quantized wire
/// coefficient `v` of predictor `px = 2·k + src` for the `k`-th
/// predictable transmission channel (`127` = zero = `predsi 0`). All
/// predictors are first-order with zero delay compensation.
struct PredPlan {
    on: [bool; 8],
    coef_v: [[u8; 6]; 8],
    npred: [usize; 8],
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
            bits += 2 * self.npred[sbgr] as u32;
            for px in 0..self.npred[sbgr] {
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
/// its predictable channels), replace those channels' subband samples
/// with the prediction *error*. `targets[sbgr]` lists the predictable
/// transmission channels of the group (§2.5.2.15 `npred` = twice that
/// count); `t01[src][sb][t]` are the decoded compatible-pair samples.
fn fit_and_apply_prediction(
    tx_sub: &mut [SubbandFrame],
    t01: &[SubbandFrame],
    targets: &[Vec<usize>; 8],
) -> PredPlan {
    let mut plan = PredPlan {
        on: [false; 8],
        coef_v: [[127u8; 6]; 8],
        npred: [0; 8],
    };
    for sbgr in 0..8usize {
        let sb = sbgr; // groups 0..7 are single subbands (§2.5.2.15)
        plan.npred[sbgr] = 2 * targets[sbgr].len();
        if targets[sbgr].is_empty() {
            continue;
        }
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
        for (k, &mch) in targets[sbgr].iter().enumerate() {
            let x = &tx_sub[mch][sb];
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
            orig_total += xx;
            if resid <= xx {
                coefs[k] = [c0, c1];
                resid_total += resid;
            } else {
                coefs[k] = [0.0, 0.0];
                resid_total += xx;
            }
        }
        // Group election: ≥ 10 % energy win across the channels.
        if orig_total > 0.0 && resid_total <= 0.9 * orig_total {
            plan.on[sbgr] = true;
            for (k, &mch) in targets[sbgr].iter().enumerate() {
                let [c0, c1] = coefs[k];
                plan.coef_v[sbgr][2 * k] = quantize_pred_coef(c0);
                plan.coef_v[sbgr][2 * k + 1] = quantize_pred_coef(c1);
                if c0 == 0.0 && c1 == 0.0 {
                    continue;
                }
                let x = &mut tx_sub[mch][sb];
                for t in 0..SLOTS {
                    x[t] -= c0 * a[t] + c1 * b[t];
                }
            }
        }
    }
    plan
}

/// §C.2.1.5 matrixing constants `(α, β, γ)` for a dematrix procedure.
/// Procedure `'11'` performs no matrixing (all ones, applied nowhere).
fn matrix_coeffs(proc_: u8) -> (f64, f64, f64) {
    match proc_ {
        1 => (1.0 / (1.5 + 0.5 * SQRT2), 1.0 / SQRT2, 0.5),
        3 => (1.0, 1.0, 1.0),
        _ => (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2),
    }
}

/// One frame's matrixing: presentation channels (in
/// [`McConfig::layout`] order) → the compatible pair `[Lo, Ro]` and the
/// weighted audio signals, one per layout slot (`Lw, Rw, Cw, LSw, RSw`
/// / `Sw`, then an unweighted `L2, R2`). Per-role wire weights: front
/// channels ride at α, centre at αβ, surround at αγ (§2.5.3.2.5's
/// inverses); procedure `'11'` carries everything unweighted.
fn matrix_downmix(cfg: &McEncodeConfig, pcm: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let n = pcm[0].len();
    let front = usize::from(cfg.front);
    let (alpha, beta, gamma) = matrix_coeffs(cfg.dematrix_procedure);
    let no_matrix = cfg.dematrix_procedure == 3;
    let phase_mixed = cfg.dematrix_procedure == 2;
    let w_f = if no_matrix { 1.0 } else { alpha };
    let w_c = if no_matrix { 1.0 } else { alpha * beta };
    let w_s = if no_matrix { 1.0 } else { alpha * gamma };

    let n_roles = pcm.len();
    let mut base = vec![vec![0.0f64; n]; 2];
    let mut roles = vec![vec![0.0f64; n]; n_roles];
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
        } else if phase_mixed {
            // §C.2.1.5 procedure 2: the monophonic surround jS enters
            // the compatible pair in antiphase.
            let js = 0.5 * (lsur + rsur);
            base[0][i] = alpha * (l + beta * c - gamma * js);
            base[1][i] = alpha * (r + beta * c + gamma * js);
        } else {
            base[0][i] = alpha * (l + beta * c + gamma * lsur);
            base[1][i] = alpha * (r + beta * c + gamma * rsur);
        }
        roles[0][i] = w_f * l;
        roles[1][i] = w_f * r;
        let mut k = 2;
        if cfg.front == 3 {
            roles[k][i] = w_c * c;
            k += 1;
        }
        match cfg.surround {
            2 => {
                roles[k][i] = w_s * lsur;
                roles[k + 1][i] = w_s * rsur;
                k += 2;
            }
            1 => {
                roles[k][i] = w_s * lsur;
                k += 1;
            }
            _ => {}
        }
        if cfg.second_stereo {
            roles[k][i] = pcm[k][i];
            roles[k + 1][i] = pcm[k + 1][i];
        }
    }
    (base, roles)
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
/// `start_bit` with bits `[skip, skip + nbits)` of `blob`.
fn splice_bits(frame: &mut [u8], start_bit: u64, blob: &[u8], skip: usize, nbits: usize) {
    for i in 0..nbits {
        let src = skip + i;
        let bit = (blob[src / 8] >> (7 - (src % 8))) & 1;
        let pos = start_bit + i as u64;
        let byte = (pos / 8) as usize;
        let sh = 7 - (pos % 8) as u32;
        frame[byte] = (frame[byte] & !(1 << sh)) | (bit << sh);
    }
}

/// Bits one allocated slot's samples cost per frame for `ngr`
/// granules (§2.4.3.3.4 codeword sizes).
fn sample_bits(nb_steps: u32, ngr: usize) -> u32 {
    let Some(class) = class_of_quantization(nb_steps) else {
        return 0;
    };
    let codewords = (ngr as u32 * 3) / class.samples_per_codeword;
    codewords * class.bits_per_codeword
}

/// Per-channel prepared coding decisions.
struct TxPlan {
    /// `nb_steps` per subband (0 = no allocation).
    nb_steps: Vec<u32>,
    /// Wire allocation index per subband.
    alloc_idx: Vec<u32>,
    /// §C.1.5.2.5 scfsi selection per subband (valid where allocated).
    scfsi: Vec<ScfsiSelection>,
}

/// The §C.1.5.2.7 minimum-MNR greedy allocation for a set of
/// channels against one Table B.2 / B.1 ladder and an explicit
/// variable-bit budget. Activation of a slot pays its scfsi (2 bits)
/// plus the *exact* transmitted-scalefactor cost of its Table C.4
/// selection, plus `extra_activation[m][sb]` (the side information of
/// channels that will be copied from it under dynamic crosstalk);
/// each quantizer step pays the exact §2.4.3.3.4 sample-bit delta.
/// Slots with `eligible[m][sb] == false` are never allocated.
#[allow(clippy::too_many_arguments)] // one call site per ladder; a struct would only rename the fields
fn allocate_bits(
    table: BitAllocTable,
    sblimit: usize,
    ngr: usize,
    smr_db: &[Vec<f64>],
    scfsi: &[Vec<ScfsiSelection>],
    eligible: &[Vec<bool>],
    extra_activation: &[Vec<u32>],
    mut budget: i64,
) -> Result<Vec<TxPlan>, McEncodeError> {
    let nch = smr_db.len();
    let mut nb_steps = vec![vec![0u32; NUM_SUBBANDS]; nch];
    let mut row_idx = vec![vec![0u32; NUM_SUBBANDS]; nch];
    let mut open = vec![vec![false; NUM_SUBBANDS]; nch];
    let mut mnr = vec![vec![0.0f64; NUM_SUBBANDS]; nch];
    for m in 0..nch {
        for sb in 0..sblimit {
            mnr[m][sb] = -smr_db[m][sb];
            open[m][sb] = eligible[m][sb] && table.nbal(sb) != 0;
        }
    }
    loop {
        let mut best: Option<(usize, usize, f64)> = None;
        for m in 0..nch {
            for sb in 0..sblimit {
                if !open[m][sb] {
                    continue;
                }
                let width = 1u32 << table.nbal(sb);
                if row_idx[m][sb] + 1 >= width {
                    open[m][sb] = false;
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
        let mut delta = i64::from(sample_bits(next_nb, ngr)) - i64::from(sample_bits(cur_nb, ngr));
        if cur_nb == 0 && next_nb != 0 {
            delta += i64::from(SCFSI_BITS_PER_SLOT)
                + 6 * scfsi[m][sb].pattern.transmitted_count() as i64
                + i64::from(extra_activation[m][sb]);
        }
        if delta > budget {
            open[m][sb] = false;
            continue;
        }
        budget -= delta;
        row_idx[m][sb] = next_row;
        nb_steps[m][sb] = next_nb;
        mnr[m][sb] = snr_db(next_nb).unwrap_or(0.0) - smr_db[m][sb];
    }

    let mut out = Vec::with_capacity(nch);
    for m in 0..nch {
        let mut alloc_idx = vec![0u32; NUM_SUBBANDS];
        for sb in 0..sblimit {
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

/// Write the transmitted scalefactors of one slot per its Table C.4
/// pattern.
fn write_scalefactors(w: &mut BitWriter, sel: &ScfsiSelection) {
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

/// Validate the per-frame input shapes.
fn validate_inputs(
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
    ml: &[Vec<f64>],
    frames: usize,
) -> Result<(), McEncodeError> {
    let n_present = cfg.presentation_channels();
    if pcm.len() != n_present {
        return Err(McEncodeError::BadPcmShape {
            have: pcm.len(),
            need: n_present,
        });
    }
    let need = frames * PCM_SAMPLES_PER_CHANNEL;
    for ch in pcm {
        if ch.len() != need {
            return Err(McEncodeError::BadPcmShape {
                have: ch.len(),
                need,
            });
        }
    }
    let lfe_need = if cfg.lfe {
        frames * LFE_SAMPLES_PER_FRAME
    } else {
        0
    };
    let lfe_have = lfe.map_or(0, <[f64]>::len);
    if lfe_have != lfe_need || (cfg.lfe && lfe.is_none()) {
        return Err(McEncodeError::BadLfeShape {
            have: lfe_have,
            need: lfe_need,
        });
    }
    let nml = usize::from(cfg.multilingual);
    if ml.len() != nml {
        return Err(McEncodeError::BadMlShape {
            have: ml.len(),
            need: nml,
        });
    }
    let ml_need = frames * cfg.multilingual_samples_per_frame();
    for ch in ml {
        if ch.len() != ml_need {
            return Err(McEncodeError::BadMlShape {
                have: ch.len(),
                need: ml_need,
            });
        }
    }
    Ok(())
}

/// Per-frame dynamic-crosstalk election result.
struct DynPlan {
    on: bool,
    lr: bool,
    mode: [u8; 12],
    second: [bool; 12],
    /// How each `(mch, sb)` slot reaches the decoder.
    source: Vec<[TcSource; NUM_SUBBANDS]>,
}

impl DynPlan {
    fn none(nmch: usize) -> Self {
        DynPlan {
            on: false,
            lr: false,
            mode: [0; 12],
            second: [false; 12],
            source: vec![[TcSource::Transmitted; NUM_SUBBANDS]; nmch],
        }
    }

    fn crossed(&self, mch: usize, sb: usize) -> bool {
        self.source[mch][sb] != TcSource::Transmitted
    }

    /// Whether `(mch, sb)` is a `Txy` carrier: some other channel is
    /// copied from it in this subband, so its transmitted samples are
    /// the §C.2.1.7 sum re-scaled per channel by the decoder.
    fn is_carrier(&self, mch: usize, sb: usize) -> bool {
        self.source
            .iter()
            .any(|col| col[sb] == TcSource::FromTc(mch))
    }
}

/// Energy of the difference between `x` and the decoder's
/// dynamic-crosstalk substitute: `src_raw` re-scaled by `x`'s own
/// per-part scalefactors (§2.5.2.15 copies the requantised, not yet
/// re-scaled samples).
fn substitution_error(x: &[f64; SLOTS], src_raw: &[f64; SLOTS], sf_x: &[u8; 3]) -> (f64, f64) {
    let (mut err, mut energy) = (0.0f64, 0.0f64);
    for t in 0..SLOTS {
        let f = SCALEFACTORS[sf_x[t / SLOTS_PER_PART] as usize];
        let e = x[t] - src_raw[t] * f;
        err += e * e;
        energy += x[t] * x[t];
    }
    (err, energy)
}

/// Normalised ("requantised but not re-scaled") view of a carrier
/// signal: each part divided by its own scalefactor, which is what a
/// decoder copying the carrier's raw samples sees before applying the
/// target channel's scalefactors.
fn normalised(carrier: &[f64; SLOTS]) -> [f64; SLOTS] {
    let mut out = [0.0f64; SLOTS];
    for p in 0..3 {
        let part = &carrier[p * SLOTS_PER_PART..(p + 1) * SLOTS_PER_PART];
        let max_abs = part.iter().fold(0.0f64, |m, &v| m.max(v.abs()));
        let f = SCALEFACTORS[pick_scalefactor_index(max_abs) as usize];
        for t in 0..SLOTS_PER_PART {
            out[p * SLOTS_PER_PART + t] = part[t] / f;
        }
    }
    out
}

/// §C.2.1.7 dynamic-crosstalk election. For every subband group and
/// both `dyn_cross_LR` polarities, every legal `dyn_cross_mode` is
/// tried: a mode is admissible when each transmission channel it drops
/// is reproduced by the decoder's copy rule within
/// [`DYN_CROSS_MAX_ERROR_RATIO`]; the admissible mode dropping the most
/// channels wins (ties → lowest code). `Txy` carriers are installed as
/// the sum of the channels they stand for (§C.2.1.7: "the subband
/// samples of the representation channels … are added"). Returns the
/// plan and leaves `tx_sub` holding the carriers.
#[allow(clippy::too_many_arguments)] // single call site; the args are the frame's working set
fn elect_dyn_cross(
    cfg: &McEncodeConfig,
    mc_cfg: &McConfig,
    msblimit: usize,
    tc_alloc: &[u8; 12],
    tx_sub: &mut [SubbandFrame],
    tx_sf: &[[[u8; NUM_SUBBANDS]; 3]],
    base: &BaseSubbands,
    centre_limited: &[[bool; NUM_SUBBANDS]],
) -> DynPlan {
    let nmch = mc_cfg.nmch;
    let main_nmch = mc_cfg.main_nmch();
    let n_modes: u8 = 1 << mc_cfg.dyn_cross_bits;
    let sf_of =
        |m: usize, sb: usize| -> [u8; 3] { [tx_sf[m][0][sb], tx_sf[m][1][sb], tx_sf[m][2][sb]] };

    let evaluate = |lr: bool| -> (DynPlan, usize) {
        let mut plan = DynPlan::none(nmch);
        plan.lr = lr;
        let mut dropped_total = 0usize;
        for sbgr in 0..12 {
            let (lo, hi) = SBGR_BOUNDS[sbgr];
            if lo >= msblimit {
                continue;
            }
            let hi = hi.min(msblimit - 1);
            // Main-programme modes.
            let mut best: Option<(u8, usize, Vec<TcSource>)> = None;
            for mode in 1..n_modes {
                let Some(sources) = dyn_cross_sources(mc_cfg, mode) else {
                    continue;
                };
                let mut ok = true;
                let mut dropped = 0usize;
                'sb: for sb in lo..=hi {
                    let roles = tc_roles(mc_cfg, tc_alloc[sbgr]);
                    // Carriers of this mode: each transmitted channel
                    // plus everything copied from it.
                    let mut carrier: Vec<[f64; SLOTS]> =
                        (0..main_nmch).map(|i| tx_sub[i][sb]).collect();
                    for (j, src) in sources.iter().enumerate() {
                        if let TcSource::FromTc(i) = src {
                            for t in 0..SLOTS {
                                carrier[*i][t] += tx_sub[j][sb][t];
                            }
                        }
                    }
                    for (j, src) in sources.iter().enumerate() {
                        let src_raw: [f64; SLOTS] = match src {
                            TcSource::Transmitted => continue,
                            TcSource::FromTc(i) => normalised(&carrier[*i]),
                            TcSource::FromBase => {
                                let role = roles.get(j).copied().unwrap_or(McChannel::Left);
                                let bch = fallback_base_channel(role, lr);
                                let mut raw = [0.0f64; SLOTS];
                                if base.nb_steps[bch][sb] != 0 {
                                    for t in 0..SLOTS {
                                        raw[t] = base.raw[bch][t][sb];
                                    }
                                }
                                raw
                            }
                        };
                        if centre_limited[j][sb] {
                            continue;
                        }
                        let (err, energy) =
                            substitution_error(&tx_sub[j][sb], &src_raw, &sf_of(j, sb));
                        if err > DYN_CROSS_MAX_ERROR_RATIO * energy {
                            ok = false;
                            break 'sb;
                        }
                    }
                    dropped = sources
                        .iter()
                        .filter(|s| **s != TcSource::Transmitted)
                        .count();
                }
                if ok && dropped > 0 && !best.as_ref().is_some_and(|(_, d, _)| *d >= dropped) {
                    best = Some((mode, dropped, sources));
                }
            }
            if let Some((mode, dropped, sources)) = best {
                plan.mode[sbgr] = mode;
                dropped_total += dropped;
                for sb in lo..=hi {
                    for (j, src) in sources.iter().enumerate() {
                        plan.source[j][sb] = *src;
                    }
                }
            }
            // Second stereo programme: R2 copied from the L2 carrier.
            if mc_cfg.second_stereo {
                let (l2, r2) = (main_nmch, main_nmch + 1);
                let mut ok = true;
                for sb in lo..=hi {
                    let mut carrier = tx_sub[l2][sb];
                    for t in 0..SLOTS {
                        carrier[t] += tx_sub[r2][sb][t];
                    }
                    let (err, energy) =
                        substitution_error(&tx_sub[r2][sb], &normalised(&carrier), &sf_of(r2, sb));
                    if err > DYN_CROSS_MAX_ERROR_RATIO * energy {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    plan.second[sbgr] = true;
                    dropped_total += 1;
                    for sb in lo..=hi {
                        plan.source[r2][sb] = TcSource::FromTc(l2);
                    }
                }
            }
        }
        plan.on = dropped_total > 0;
        (plan, dropped_total)
    };

    let (plan_l, n_l) = evaluate(false);
    let plan = if cfg.dyn_cross && n_l > 0 {
        // Try the other polarity only when a centre / mono-surround
        // fallback exists to be steered by dyn_cross_LR.
        let (plan_r, n_r) = evaluate(true);
        if n_r > n_l {
            plan_r
        } else {
            plan_l
        }
    } else {
        plan_l
    };
    if !plan.on {
        return DynPlan::none(nmch);
    }
    // Install the carriers: every transmitted channel absorbs the
    // channels copied from it.
    let snapshot: Vec<SubbandFrame> = tx_sub.to_vec();
    for j in 0..nmch {
        for sb in 0..msblimit {
            if let TcSource::FromTc(i) = plan.source[j][sb] {
                for t in 0..SLOTS {
                    tx_sub[i][sb][t] += snapshot[j][sb][t];
                }
            }
        }
    }
    plan
}

/// §C.2.1.6 per-subband-group `tc_allocation` election: among the
/// legal rows, the one whose transmitted signals have the lowest
/// maximum scalefactor multipliers over the group (ties → lowest
/// code).
fn elect_tc_allocation(
    cfg: &McEncodeConfig,
    mc_cfg: &McConfig,
    msblimit: usize,
    layout: &[McChannel],
    role_sf: &[[[u8; NUM_SUBBANDS]; 3]],
) -> [u8; 12] {
    let legal = legal_tc_values(cfg);
    let mut out = [0u8; 12];
    for sbgr in 0..12 {
        let (lo, hi) = SBGR_BOUNDS[sbgr];
        let hi = hi.min(msblimit.saturating_sub(1));
        let mut best: Option<(f64, u8)> = None;
        for &v in &legal {
            let roles = tc_roles(mc_cfg, v);
            let mut score = 0.0f64;
            for role in roles {
                let Some(ri) = layout.iter().position(|r| *r == role) else {
                    continue;
                };
                let mut max_mult = 0.0f64;
                for sb in lo..=hi {
                    for p in 0..3 {
                        max_mult = max_mult.max(SCALEFACTORS[role_sf[ri][p][sb] as usize]);
                    }
                }
                score += max_mult;
            }
            if !best.is_some_and(|(s, _)| score >= s) {
                best = Some((score, v));
            }
        }
        out[sbgr] = best.map_or(0, |(_, v)| v);
    }
    out
}

/// Serialise and emit one §2.5.1.5 `ext_frame()` carrying `bits`
/// bits of extension data starting at bit `skip` of `blob`.
fn build_ext_frame(blob: &[u8], skip: usize, bits: usize) -> Result<Vec<u8>, McEncodeError> {
    let data_bytes = bits.div_ceil(8);
    let ext_length = EXT_HEADER_BYTES + data_bytes;
    if ext_length > EXT_MAX_BYTES {
        return Err(McEncodeError::ExtFrameTooLarge { need: ext_length });
    }
    let mut w = BitWriter::with_capacity(ext_length);
    w.write_u32(EXT_SYNCWORD, 12);
    w.write_u32(0, 16); // ext_crc_check, patched below
    w.write_u32(ext_length as u32, 11);
    w.write_u32(0, 1); // ext_ID_bit
    for i in 0..bits {
        let src = skip + i;
        w.write_u32(u32::from((blob[src / 8] >> (7 - (src % 8))) & 1), 1);
    }
    while (w.bit_position() as usize) < ext_length * 8 {
        w.write_u32(0, 1);
    }
    let mut ext = w.finish();
    ext.truncate(ext_length);
    // §2.5.2.10: CRC over 128 bits from the first bit of ext_length, or
    // fewer if the frame ends earlier.
    let crc_bits = ((ext_length as u64) * 8 - 28).min(128);
    let reg = crc_feed(INIT_STATE, &ext, 28, 28 + crc_bits);
    ext[1] = (ext[1] & 0xF0) | ((reg >> 12) as u8 & 0x0F);
    ext[2] = (reg >> 4) as u8;
    ext[3] = (ext[3] & 0x0F) | (((reg & 0x0F) as u8) << 4);
    Ok(ext)
}

/// Encode one Layer II multichannel frame.
///
/// * `header` — the base frame header (MPEG-1 rate, `Stereo` mode).
/// * `pcm` — `cfg.presentation_channels()` channels of
///   [`PCM_SAMPLES_PER_CHANNEL`] samples each, in [`McConfig::layout`]
///   order (L, R, [C], [LS, RS | S], [L2, R2]), nominal `[-1, +1]`
///   range.
/// * `lfe` — exactly [`LFE_SAMPLES_PER_FRAME`] samples at `Fs / 96`
///   when `cfg.lfe`, `None` otherwise.
///
/// Returns the complete base frame with the `mc_extension()` spliced
/// into its §2.4.1.8 ancillary field; the result decodes through
/// [`crate::mc::decode_mc_frame_with`] to the presentation channels
/// and through the plain [`crate::frame::decode_frame`] to the
/// compatible stereo downmix. Configurations with multilingual
/// channels or an extension bit stream need
/// [`encode_mc_frame_ext_with`].
pub fn encode_mc_frame_with(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
    state: &mut McEncodeState,
) -> Result<Vec<u8>, McEncodeError> {
    if cfg.ext_bit_stream {
        return Err(McEncodeError::BadConfig(
            "ext_bit_stream output needs encode_mc_frame_ext_with".into(),
        ));
    }
    encode_mc_frame_ext_with(header, cfg, pcm, lfe, &[], state).map(|f| f.base)
}

/// Encode one Layer II multichannel frame with multilingual input and
/// extension-bit-stream output.
///
/// `ml` holds `cfg.multilingual` channels of
/// [`McEncodeConfig::multilingual_samples_per_frame`] samples each;
/// everything else is as for [`encode_mc_frame_with`]. The returned
/// [`McEncodedFrame::ext`] is `Some` exactly when `cfg.ext_bit_stream`.
pub fn encode_mc_frame_ext_with(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
    ml: &[Vec<f64>],
    state: &mut McEncodeState,
) -> Result<McEncodedFrame, McEncodeError> {
    cfg.validate()?;
    validate_base_header(header)?;
    validate_inputs(cfg, pcm, lfe, ml, 1)?;

    let mc_header = cfg.mc_header();
    let mc_cfg = McConfig::from_header(&mc_header, header.mode);
    let nmch = mc_cfg.nmch;
    let main_nmch = mc_cfg.main_nmch();
    let layout = mc_cfg.layout();
    debug_assert_eq!(layout.len(), pcm.len());
    let nml = usize::from(cfg.multilingual);
    let ml_n = cfg.multilingual_samples_per_frame();
    let ml_slots = ml_n / NUM_SUBBANDS;
    let ml_ngr = ml_slots / 3;

    // Table B.2a at 48 kHz, B.2b at 44,1 / 32 kHz regardless of bitrate,
    // msblimit = sblimit (§2.5.2.17).
    let mc_table = match header.sample_rate {
        48_000 => BitAllocTable::B2a,
        _ => BitAllocTable::B2b,
    };
    let msblimit = mc_table.sblimit();
    let ml_table = if cfg.multilingual_fs_half {
        BitAllocTable::B1Lsf
    } else {
        mc_table
    };
    let mlsblimit = ml_table.sblimit();

    // ---- matrixing (§C.2.1.5) -------------------------------------------
    let (base_pcm, role_pcm) = matrix_downmix(cfg, pcm);
    let n_roles = role_pcm.len();

    // ---- per-signal analysis, scalefactors, SMR --------------------------
    McEncodeState::ensure_channels(
        &mut state.role_fb,
        &mut state.role_hist,
        n_roles,
        model1_hist_len(PCM_SAMPLES_PER_CHANNEL),
    );
    let mut role_sub: Vec<SubbandFrame> = Vec::with_capacity(n_roles);
    for (i, ch) in role_pcm.iter().enumerate() {
        role_sub.push(analyse(&mut state.role_fb[i], ch, SLOTS));
    }
    // Phantom coding (§C.2.1.9): the centre's subbands ≥ 12 ride the
    // compatible pair only; in the subband domain that is `Lw += Cw`,
    // `Rw += Cw`, `Cw = 0` there (the α·β weight is the −3 dB the
    // procedure prescribes).
    let ml_weight = if cfg.multilingual_fs_half { 0.5 } else { 1.0 };
    let per_ch_kbps =
        f64::from(header.bit_rate) / 1000.0 / (2.0 + nmch as f64 + ml_weight * nml as f64);
    let fs = annex_d_sampling_rate(header.sample_rate);
    let mut role_sf: Vec<[[u8; NUM_SUBBANDS]; 3]> =
        role_sub.iter().map(|s| scalefactors_of(s, SLOTS)).collect();
    let mut role_smr: Vec<Vec<f64>> = vec![vec![0.0f64; NUM_SUBBANDS]; n_roles];
    if let Some(fs) = fs {
        for i in 0..n_roles {
            role_smr[i] = model1_smr(
                &mut state.role_hist[i],
                &role_pcm[i],
                &role_sf[i],
                fs,
                per_ch_kbps,
            );
        }
    }
    if cfg.phantom_centre {
        let ci = 2; // layout: L, R, C, …
        for sb in 12..NUM_SUBBANDS {
            for t in 0..SLOTS {
                let c = role_sub[ci][sb][t];
                role_sub[0][sb][t] += c;
                role_sub[1][sb][t] += c;
                role_sub[ci][sb][t] = 0.0;
            }
            // The folded content is masked by whichever of the two
            // signals demands more: take the larger SMR.
            role_smr[0][sb] = role_smr[0][sb].max(role_smr[ci][sb]);
            role_smr[1][sb] = role_smr[1][sb].max(role_smr[ci][sb]);
        }
        for i in [0, 1, ci] {
            role_sf[i] = scalefactors_of(&role_sub[i], SLOTS);
        }
    }

    // ---- transmission channel switching (§2.5.2.15 / §C.2.1.6) -----------
    let tc_alloc: [u8; 12] = if cfg.adaptive_tc && cfg.dematrix_procedure != 3 {
        elect_tc_allocation(cfg, &mc_cfg, msblimit, &layout, &role_sf)
    } else {
        [cfg.tc_allocation; 12]
    };
    let tc_sbgr_select = tc_alloc.iter().all(|&v| v == tc_alloc[0]);
    let role_index = |role: McChannel| layout.iter().position(|r| *r == role);
    let mut tx_sub: Vec<SubbandFrame> = (0..nmch)
        .map(|_| Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]))
        .collect();
    let mut tx_smr: Vec<Vec<f64>> = vec![vec![0.0f64; NUM_SUBBANDS]; nmch];
    for sb in 0..NUM_SUBBANDS {
        let roles = tc_roles(&mc_cfg, tc_alloc[sbgr_of_subband(sb)]);
        for m in 0..main_nmch {
            let ri = roles.get(m).and_then(|r| role_index(*r)).ok_or_else(|| {
                McEncodeError::Internal(format!("no role for T{} sb {sb}", m + 2))
            })?;
            tx_sub[m][sb] = role_sub[ri][sb];
            tx_smr[m][sb] = role_smr[ri][sb];
        }
        if mc_cfg.second_stereo {
            for (k, role) in [McChannel::SecondLeft, McChannel::SecondRight]
                .into_iter()
                .enumerate()
            {
                let ri = role_index(role)
                    .ok_or_else(|| McEncodeError::Internal("second stereo role missing".into()))?;
                tx_sub[main_nmch + k][sb] = role_sub[ri][sb];
                tx_smr[main_nmch + k][sb] = role_smr[ri][sb];
            }
        }
    }
    // §2.5.2.13 centre_limited: the centre tc (T2 for every tc row
    // Phantom coding allows) carries no allocation above subband 11.
    let mut centre_limited = vec![[false; NUM_SUBBANDS]; nmch.max(1)];
    if cfg.phantom_centre {
        for sb in 12..msblimit {
            centre_limited[0][sb] = true;
        }
    }

    // ---- multilingual analysis (§2.5.2.18) -------------------------------
    McEncodeState::ensure_channels(
        &mut state.ml_fb,
        &mut state.ml_hist,
        nml,
        model1_hist_len(ml_n),
    );
    let mut ml_sub: Vec<SubbandFrame> = Vec::with_capacity(nml);
    let mut ml_sf: Vec<[[u8; NUM_SUBBANDS]; 3]> = Vec::with_capacity(nml);
    let mut ml_smr: Vec<Vec<f64>> = vec![vec![0.0f64; NUM_SUBBANDS]; nml];
    let ml_fs = if cfg.multilingual_fs_half {
        annex_d_sampling_rate(header.sample_rate / 2)
    } else {
        fs
    };
    for i in 0..nml {
        let sub = analyse(&mut state.ml_fb[i], &ml[i], ml_slots);
        let sf = scalefactors_of(&sub, ml_slots);
        if let Some(fs) = ml_fs {
            ml_smr[i] = model1_smr(&mut state.ml_hist[i], &ml[i], &sf, fs, per_ch_kbps);
        }
        ml_sub.push(sub);
        ml_sf.push(sf);
    }

    // ---- extension bit budget ------------------------------------------
    let cb = (header.frame_size_bytes() as u32) * 8;
    let bcrc: u32 = if header.protection_bit { 0 } else { 16 };
    let lfe_nb = u32::from(cfg.lfe_allocation) + 1;
    let hdr_bits: u32 = if cfg.ext_bit_stream { 24 } else { 16 };
    // Provisional fixed cost (no crosstalk / prediction yet): mc_header
    // + mc_crc + composite status + allocation fields + LFE + ML
    // allocation fields.
    let tc_field_bits = if tc_sbgr_select {
        mc_cfg.tc_allocation_bits
    } else {
        12 * mc_cfg.tc_allocation_bits
    };
    let mut fixed0: u32 = hdr_bits + 16 + 3 + tc_field_bits;
    if cfg.lfe {
        fixed0 += 4 + 6 + 12 * lfe_nb;
    }
    for sb in 0..msblimit {
        for m in 0..nmch {
            if !centre_limited[m][sb] {
                fixed0 += mc_table.nbal(sb);
            }
        }
    }
    for sb in 0..mlsblimit {
        fixed0 += ml_table.nbal(sb) * nml as u32;
    }
    let avail = cb.saturating_sub(32 + bcrc);
    // Proportional split of the post-fixed-cost data bits: the
    // extension's channel-count share (a half-rate multilingual
    // channel counts one half).
    let ext_w = nmch as f64 + ml_weight * nml as f64;
    let proportional =
        fixed0 + (avail.saturating_sub(fixed0) as f64 * ext_w / (2.0 + ext_w)) as u32;
    let budget = cfg.mc_bits.unwrap_or(proportional);
    if budget < fixed0 || (!cfg.ext_bit_stream && budget > avail) {
        return Err(McEncodeError::BudgetTooSmall {
            fixed: fixed0,
            budget: budget.min(avail),
        });
    }
    // In-frame reservation. With an extension bit stream the base
    // frame keeps at least its proportional share of the data bits —
    // only the extension's own share rides the base ancillary field,
    // the excess spills into the ext_frame() (§2.5.1.12.3).
    let reserve = if cfg.ext_bit_stream {
        budget.min(avail).min(proportional)
    } else {
        budget
    };

    // ---- base encode with the extension reservation ----------------------
    let mut frame = encode_frame_auto_with(header, &base_pcm, reserve, &mut state.base)?;
    let base = decode_base_subbands(&frame)
        .map_err(|e| McEncodeError::Internal(format!("re-parse: {e}")))?;
    let anc_start = base.anc_start_bit;
    let total_bits = frame.len() as u64 * 8;
    let base_capacity = (total_bits - anc_start) as usize;
    if (base_capacity as u32) < reserve {
        return Err(McEncodeError::Internal(format!(
            "ancillary tail {base_capacity} bits < reservation {reserve} bits"
        )));
    }

    // ---- dynamic crosstalk (§2.5.2.15 / §C.2.1.7) -----------------------
    // Scalefactors of the original (pre-carrier, pre-prediction)
    // transmission signals: copied channels keep their own envelope.
    let tx_sf_orig: Vec<[[u8; NUM_SUBBANDS]; 3]> =
        tx_sub.iter().map(|s| scalefactors_of(s, SLOTS)).collect();
    let tx_snapshot: Option<Vec<SubbandFrame>> =
        (cfg.dyn_cross && nmch > 0).then(|| tx_sub.clone());
    let mut dyn_plan = if cfg.dyn_cross && nmch > 0 {
        elect_dyn_cross(
            cfg,
            &mc_cfg,
            msblimit,
            &tc_alloc,
            &mut tx_sub,
            &tx_sf_orig,
            &base,
            &centre_limited,
        )
    } else {
        DynPlan::none(nmch)
    };

    // ---- transmission-channel scalefactors + scfsi ----------------------
    // §C.2.1.7 intensity semantics: a crossed channel AND a `Txy`
    // carrier both transmit their own pre-carrier envelope on the wire
    // (the decoder re-scales the shared raw samples per channel); the
    // carrier's *samples* are instead normalised against the summed
    // signal's envelope below, so the codes stay in range.
    let make_scfsi = |tx_sub: &[SubbandFrame], dyn_plan: &DynPlan| -> Vec<Vec<ScfsiSelection>> {
        let mut out = Vec::with_capacity(nmch);
        for m in 0..nmch {
            let sf = scalefactors_of(&tx_sub[m], SLOTS);
            let mut sels = Vec::with_capacity(NUM_SUBBANDS);
            for sb in 0..NUM_SUBBANDS {
                let use_sf = if dyn_plan.crossed(m, sb) || dyn_plan.is_carrier(m, sb) {
                    &tx_sf_orig[m]
                } else {
                    &sf
                };
                sels.push(select_scfsi([use_sf[0][sb], use_sf[1][sb], use_sf[2][sb]]));
            }
            out.push(sels);
        }
        out
    };
    let mut tx_scfsi = make_scfsi(&tx_sub, &dyn_plan);

    // ---- exact fixed cost (sans prediction) -----------------------------
    let fixed_cost = |dyn_plan: &DynPlan,
                      tx_scfsi: &[Vec<ScfsiSelection>]|
     -> (u32, Vec<Vec<bool>>, Vec<Vec<u32>>) {
        let mut fixed: u32 = hdr_bits + 16 + 3 + tc_field_bits;
        if dyn_plan.on {
            fixed += 1 + 12 * (mc_cfg.dyn_cross_bits + u32::from(mc_cfg.second_stereo));
        }
        if cfg.lfe {
            fixed += 4 + 6 + 12 * lfe_nb;
        }
        let mut eligible = vec![vec![true; NUM_SUBBANDS]; nmch];
        let mut extra_activation = vec![vec![0u32; NUM_SUBBANDS]; nmch];
        for sb in 0..msblimit {
            for m in 0..nmch {
                if centre_limited[m][sb] {
                    eligible[m][sb] = false;
                    continue;
                }
                match dyn_plan.source[m][sb] {
                    TcSource::Transmitted => fixed += mc_table.nbal(sb),
                    TcSource::FromTc(i) => {
                        eligible[m][sb] = false;
                        extra_activation[i][sb] += SCFSI_BITS_PER_SLOT
                            + 6 * tx_scfsi[m][sb].pattern.transmitted_count() as u32;
                    }
                    TcSource::FromBase => {
                        eligible[m][sb] = false;
                        let roles = tc_roles(&mc_cfg, tc_alloc[sbgr_of_subband(sb)]);
                        let role = roles.get(m).copied().unwrap_or(McChannel::Left);
                        let bch = fallback_base_channel(role, dyn_plan.lr);
                        if base.nb_steps[bch][sb] != 0 {
                            fixed += SCFSI_BITS_PER_SLOT
                                + 6 * tx_scfsi[m][sb].pattern.transmitted_count() as u32;
                        }
                    }
                }
            }
        }
        fixed += (0..mlsblimit)
            .map(|sb| ml_table.nbal(sb) * nml as u32)
            .sum::<u32>();
        (fixed, eligible, extra_activation)
    };
    let (mut fixed, mut eligible, mut extra_activation) = fixed_cost(&dyn_plan, &tx_scfsi);
    if budget < fixed && dyn_plan.on {
        // The election's side-information cost outgrew the budget at
        // this bitrate — restore the pre-carrier transmission signals
        // and fall back to plain transmission.
        tx_sub = tx_snapshot.expect("snapshot exists when crosstalk was elected");
        dyn_plan = DynPlan::none(nmch);
        tx_scfsi = make_scfsi(&tx_sub, &dyn_plan);
        (fixed, eligible, extra_activation) = fixed_cost(&dyn_plan, &tx_scfsi);
    }
    if budget < fixed {
        return Err(McEncodeError::BudgetTooSmall { fixed, budget });
    }
    let dyn_plan = dyn_plan;

    // ---- §2.5.3.2.1.3 multichannel prediction election ------------------
    // Gate on worst-case side-information headroom so the fit never has
    // to be undone: 8 flag bits + per group `2·npred` predsi bits +
    // `3 + 8` bits per coefficient.
    let pred_worst: u32 = 8
        + (0..8)
            .map(|sbgr| {
                let mode = if dyn_plan.on { dyn_plan.mode[sbgr] } else { 0 };
                let npred = npred_for(&mc_cfg, mode) as u32;
                2 * npred + npred * 11
            })
            .sum::<u32>();
    let pred_plan = if cfg.prediction && main_nmch > 0 && budget - fixed >= pred_worst {
        let mut t01: Vec<SubbandFrame> = Vec::with_capacity(2);
        for bch in 0..2 {
            let mut sub: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
            for sb in 0..NUM_SUBBANDS {
                for t in 0..SLOTS {
                    sub[sb][t] = base.scaled[bch][t][sb];
                }
            }
            t01.push(sub);
        }
        // The decoder applies predictor `px = 2·k + src` to the k-th
        // entry of ITS `predictable_channels` list — the encoder's
        // target list must be that list verbatim (the tables already
        // exclude every copied / combined channel, and subband groups
        // 0..7 sit below any Phantom-coding limit).
        let mut targets: [Vec<usize>; 8] = Default::default();
        for sbgr in 0..8 {
            let mode = if dyn_plan.on { dyn_plan.mode[sbgr] } else { 0 };
            targets[sbgr] = predictable_channels(&mc_cfg, mode);
        }
        let mut plan = fit_and_apply_prediction(&mut tx_sub, &t01, &targets);
        for sbgr in 0..8 {
            // npred is a property of the dyn_cross_mode, not of the
            // surviving targets (§2.5.2.15) — the wire predsi count
            // must match the decoder's `npred_for`.
            let mode = if dyn_plan.on { dyn_plan.mode[sbgr] } else { 0 };
            plan.npred[sbgr] = npred_for(&mc_cfg, mode);
        }
        plan.any().then_some(plan)
    } else {
        None
    };
    if let Some(plan) = &pred_plan {
        fixed += plan.extra_bits();
    }
    // Re-derive the wire scalefactors now the predicted slots hold
    // residuals (crossed channels and carriers keep their pre-carrier
    // envelope — untouched by the fit).
    tx_scfsi = make_scfsi(&tx_sub, &dyn_plan);
    // Sample-quantisation envelope: the post-carrier / post-residual
    // signal itself (differs from the wire scalefactors exactly on the
    // §C.2.1.7 carriers).
    let tx_sf_quant: Vec<[[u8; NUM_SUBBANDS]; 3]> =
        tx_sub.iter().map(|s| scalefactors_of(s, SLOTS)).collect();

    // ---- greedy allocations ----------------------------------------------
    let var_total = i64::from(budget - fixed);
    let ext_w = nmch as f64 + ml_weight * nml as f64;
    let ml_var = if nml == 0 || ext_w <= 0.0 {
        0
    } else {
        (var_total as f64 * ml_weight * nml as f64 / ext_w) as i64
    };
    let plans = allocate_bits(
        mc_table,
        msblimit,
        12,
        &tx_smr,
        &tx_scfsi,
        &eligible,
        &extra_activation,
        var_total - ml_var,
    )?;
    let ml_scfsi: Vec<Vec<ScfsiSelection>> = ml_sf
        .iter()
        .map(|sf| {
            (0..NUM_SUBBANDS)
                .map(|sb| select_scfsi([sf[0][sb], sf[1][sb], sf[2][sb]]))
                .collect()
        })
        .collect();
    let ml_eligible = vec![vec![true; NUM_SUBBANDS]; nml];
    let ml_extra = vec![vec![0u32; NUM_SUBBANDS]; nml];
    let ml_plans = allocate_bits(
        ml_table,
        mlsblimit,
        ml_ngr,
        &ml_smr,
        &ml_scfsi,
        &ml_eligible,
        &ml_extra,
        ml_var,
    )?;

    // Effective (transmitted or copied) allocation per slot, the
    // decoder's §2.5.2.15 view.
    let mut eff_alloc = vec![[0u32; NUM_SUBBANDS]; nmch];
    for sb in 0..msblimit {
        for m in 0..nmch {
            eff_alloc[m][sb] = match dyn_plan.source[m][sb] {
                _ if centre_limited[m][sb] => 0,
                TcSource::Transmitted => plans[m].nb_steps[sb],
                TcSource::FromTc(i) => plans[i].nb_steps[sb],
                TcSource::FromBase => {
                    let roles = tc_roles(&mc_cfg, tc_alloc[sbgr_of_subband(sb)]);
                    let role = roles.get(m).copied().unwrap_or(McChannel::Left);
                    base.nb_steps[fallback_base_channel(role, dyn_plan.lr)][sb]
                }
            };
        }
    }

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

    // ---- serialise the extension (§2.5.1.12.1 wire order) ---------------
    let mut w = BitWriter::with_capacity((budget as usize).div_ceil(8) + 4);
    // mc_header (§2.5.1.13).
    w.write_u32(u32::from(cfg.ext_bit_stream), 1);
    if cfg.ext_bit_stream {
        w.write_u32(0, 8); // n_ad_bytes: no MPEG-1 ancillary tail
    }
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
    w.write_u32(u32::from(cfg.multilingual), 3);
    w.write_u32(u32::from(cfg.multilingual_fs_half), 1);
    w.write_u32(0, 1); // multi_lingual_layer: Layer II ml
    w.write_u32(0, 1); // copyright_identification_bit
    w.write_u32(0, 1); // copyright_identification_start
    debug_assert_eq!(w.bit_position(), u64::from(hdr_bits));
    w.write_u32(0, 16); // mc_crc_check placeholder, patched below
    let status_start = w.bit_position();
    // mc_composite_status_info (§2.5.1.15).
    w.write_u32(u32::from(tc_sbgr_select), 1);
    w.write_u32(u32::from(dyn_plan.on), 1);
    w.write_u32(u32::from(pred_plan.is_some()), 1);
    if mc_cfg.tc_allocation_bits > 0 {
        if tc_sbgr_select {
            w.write_u32(u32::from(tc_alloc[0]), mc_cfg.tc_allocation_bits);
        } else {
            for sbgr in 0..12 {
                w.write_u32(u32::from(tc_alloc[sbgr]), mc_cfg.tc_allocation_bits);
            }
        }
    }
    if dyn_plan.on {
        w.write_u32(u32::from(dyn_plan.lr), 1);
        for sbgr in 0..12 {
            if mc_cfg.dyn_cross_bits > 0 {
                w.write_u32(u32::from(dyn_plan.mode[sbgr]), mc_cfg.dyn_cross_bits);
            }
            if mc_cfg.second_stereo {
                w.write_u32(u32::from(dyn_plan.second[sbgr]), 1);
            }
        }
    }
    if let Some(plan) = &pred_plan {
        for sbgr in 0..8 {
            w.write_u32(u32::from(plan.on[sbgr]), 1);
            if plan.on[sbgr] {
                for px in 0..plan.npred[sbgr] {
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
            if !centre_limited[mch][sb] && !dyn_plan.crossed(mch, sb) {
                w.write_u32(plans[mch].alloc_idx[sb], mc_table.nbal(sb));
            }
        }
    }
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if eff_alloc[mch][sb] != 0 {
                w.write_u32(scfsi_code(tx_scfsi[mch][sb].scfsi), 2);
            }
        }
    }
    let scfsi_end = w.bit_position();
    if let Some(plan) = &pred_plan {
        // Delay compensation + coefficients for every transmitted
        // predictor (first-order, zero delay).
        for sbgr in 0..8 {
            if !plan.on[sbgr] {
                continue;
            }
            for px in 0..plan.npred[sbgr] {
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
            if eff_alloc[mch][sb] != 0 {
                write_scalefactors(&mut w, &tx_scfsi[mch][sb]);
            }
        }
    }
    // Granule loop: LFE sample first, then one triplet per transmitted
    // allocated (sb, mch) slot.
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
                if nb == 0 || dyn_plan.crossed(mch, sb) || centre_limited[mch][sb] {
                    continue;
                }
                let class = class_of_quantization(nb)
                    .ok_or_else(|| McEncodeError::Internal(format!("class {nb}")))?;
                let triplet = [
                    tx_sub[mch][sb][base_slot],
                    tx_sub[mch][sb][base_slot + 1],
                    tx_sub[mch][sb][base_slot + 2],
                ];
                // §C.2.1.7 carriers: quantise against the summed
                // signal's envelope; the wire scalefactors stay the
                // channel's own, so the decoder's per-channel re-scale
                // lands each copy at its intended level.
                let quant_sf = if dyn_plan.is_carrier(mch, sb) {
                    tx_sf_quant[mch][part][sb]
                } else {
                    tx_scfsi[mch][sb].used[part]
                };
                write_triplet_scaled(&class, quant_sf, &triplet, &mut w)
                    .map_err(|e| McEncodeError::Internal(format!("sample write: {e}")))?;
            }
        }
    }
    // ml_audio_data (§2.5.1.18).
    for sb in 0..mlsblimit {
        for mlch in 0..nml {
            w.write_u32(ml_plans[mlch].alloc_idx[sb], ml_table.nbal(sb));
        }
    }
    for sb in 0..mlsblimit {
        for mlch in 0..nml {
            if ml_plans[mlch].nb_steps[sb] != 0 {
                w.write_u32(scfsi_code(ml_plans[mlch].scfsi[sb].scfsi), 2);
            }
        }
    }
    for sb in 0..mlsblimit {
        for mlch in 0..nml {
            if ml_plans[mlch].nb_steps[sb] != 0 {
                write_scalefactors(&mut w, &ml_plans[mlch].scfsi[sb]);
            }
        }
    }
    for gr in 0..ml_ngr {
        let part = gr / (ml_ngr / 3);
        let base_slot = gr * 3;
        for sb in 0..mlsblimit {
            for mlch in 0..nml {
                let nb = ml_plans[mlch].nb_steps[sb];
                if nb == 0 {
                    continue;
                }
                let class = class_of_quantization(nb)
                    .ok_or_else(|| McEncodeError::Internal(format!("ml class {nb}")))?;
                let triplet = [
                    ml_sub[mlch][sb][base_slot],
                    ml_sub[mlch][sb][base_slot + 1],
                    ml_sub[mlch][sb][base_slot + 2],
                ];
                write_triplet_scaled(
                    &class,
                    ml_plans[mlch].scfsi[sb].used[part],
                    &triplet,
                    &mut w,
                )
                .map_err(|e| McEncodeError::Internal(format!("ml sample write: {e}")))?;
            }
        }
    }
    let blob_bits = w.bit_position() as usize;
    debug_assert!(blob_bits as u32 <= budget, "{blob_bits} > {budget}");
    let mut blob = w.finish();

    // §2.5.2.14 mc_crc_check: "begins with the first bit of the
    // multichannel header and ends with the last bit of the scfsi
    // field, but excluding the mc_crc_check field itself."
    let mut reg = crc_feed(INIT_STATE, &blob, 0, u64::from(hdr_bits));
    reg = crc_feed(reg, &blob, status_start, scfsi_end);
    let crc_byte = (hdr_bits / 8) as usize;
    blob[crc_byte] = (reg >> 8) as u8;
    blob[crc_byte + 1] = (reg & 0xFF) as u8;

    // ---- splice at the first §2.4.1.8 ancillary bit, spill the rest -------
    let in_base = blob_bits.min(base_capacity);
    splice_bits(&mut frame, anc_start, &blob, 0, in_base);
    let ext = if cfg.ext_bit_stream {
        Some(build_ext_frame(&blob, in_base, blob_bits - in_base)?)
    } else if blob_bits > in_base {
        return Err(McEncodeError::Internal(format!(
            "extension {blob_bits} bits exceeds ancillary tail {base_capacity} bits"
        )));
    } else {
        None
    };
    Ok(McEncodedFrame { base: frame, ext })
}

/// Batch multichannel encode: one continuous per-channel PCM buffer
/// (whole multiple of [`PCM_SAMPLES_PER_CHANNEL`] samples per channel,
/// [`McConfig::layout`] order) plus an optional LFE buffer at `Fs / 96`
/// ([`LFE_SAMPLES_PER_FRAME`] samples per frame) → the concatenated
/// Layer II multichannel byte stream, threading one [`McEncodeState`]
/// and the §2.4.2.3 [`PaddingScheduler`] across frames. Configurations
/// with multilingual channels or an extension bit stream need
/// [`encode_mc_all_frames_ext`].
pub fn encode_mc_all_frames(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
) -> Result<Vec<u8>, McEncodeError> {
    if cfg.ext_bit_stream {
        return Err(McEncodeError::BadConfig(
            "ext_bit_stream output needs encode_mc_all_frames_ext".into(),
        ));
    }
    encode_mc_all_frames_ext(header, cfg, pcm, lfe, &[]).map(|s| s.base)
}

/// A batch-encoded multichannel stream: the base bit stream and, when
/// the configuration uses one, the concatenated §2.5.1.1.2 extension
/// bit stream (one `ext_frame()` per base frame).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McEncodedStream {
    /// The MPEG-1-compatible base bit stream.
    pub base: Vec<u8>,
    /// The extension bit stream (`Some` iff `cfg.ext_bit_stream`).
    pub ext: Option<Vec<u8>>,
}

/// Batch multichannel encode with multilingual input and
/// extension-bit-stream output — see [`encode_mc_all_frames`] and
/// [`encode_mc_frame_ext_with`]. `ml` holds `cfg.multilingual` channels
/// of `frames × `[`McEncodeConfig::multilingual_samples_per_frame`]
/// samples each.
pub fn encode_mc_all_frames_ext(
    header: &FrameHeader,
    cfg: &McEncodeConfig,
    pcm: &[Vec<f64>],
    lfe: Option<&[f64]>,
    ml: &[Vec<f64>],
) -> Result<McEncodedStream, McEncodeError> {
    cfg.validate()?;
    validate_base_header(header)?;
    if pcm.len() != cfg.presentation_channels() {
        return Err(McEncodeError::BadPcmShape {
            have: pcm.len(),
            need: cfg.presentation_channels(),
        });
    }
    let n = pcm[0].len();
    if n % PCM_SAMPLES_PER_CHANNEL != 0 {
        return Err(McEncodeError::BadPcmShape {
            have: n,
            need: n.next_multiple_of(PCM_SAMPLES_PER_CHANNEL),
        });
    }
    let n_frames = n / PCM_SAMPLES_PER_CHANNEL;
    validate_inputs(cfg, pcm, lfe, ml, n_frames)?;
    let ml_n = cfg.multilingual_samples_per_frame();

    let mut state = McEncodeState::new();
    let mut padding = PaddingScheduler::new();
    let mut base_out = Vec::with_capacity(n_frames * (header.frame_size_bytes() + 1));
    let mut ext_out: Option<Vec<u8>> = cfg.ext_bit_stream.then(Vec::new);
    let mut frame_pcm: Vec<Vec<f64>> = vec![Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL); pcm.len()];
    let mut frame_ml: Vec<Vec<f64>> = vec![Vec::with_capacity(ml_n); ml.len()];
    for f in 0..n_frames {
        let frame_header = padding.next_header(header);
        let at = f * PCM_SAMPLES_PER_CHANNEL;
        for (ch, plane) in pcm.iter().enumerate() {
            frame_pcm[ch].clear();
            frame_pcm[ch].extend_from_slice(&plane[at..at + PCM_SAMPLES_PER_CHANNEL]);
        }
        for (ch, plane) in ml.iter().enumerate() {
            frame_ml[ch].clear();
            frame_ml[ch].extend_from_slice(&plane[f * ml_n..(f + 1) * ml_n]);
        }
        let frame_lfe = lfe.map(|s| &s[f * LFE_SAMPLES_PER_FRAME..(f + 1) * LFE_SAMPLES_PER_FRAME]);
        let encoded = encode_mc_frame_ext_with(
            &frame_header,
            cfg,
            &frame_pcm,
            frame_lfe,
            &frame_ml,
            &mut state,
        )?;
        base_out.extend_from_slice(&encoded.base);
        if let (Some(out), Some(ext)) = (&mut ext_out, encoded.ext) {
            out.extend_from_slice(&ext);
        }
    }
    Ok(McEncodedStream {
        base: base_out,
        ext: ext_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrixing_normalisation_bounds_the_compatible_pair() {
        // Full-scale presentation channels must produce |Lo|, |Ro| <= 1
        // for procedures '00', '01' and '10' — the α attenuation exists
        // exactly for this (§2.5.3.2.5).
        for proc_ in [0u8, 1, 2] {
            let cfg = McEncodeConfig {
                dematrix_procedure: proc_,
                ..McEncodeConfig::default()
            };
            let pcm: Vec<Vec<f64>> = vec![vec![1.0; 4]; 5];
            let (base, roles) = matrix_downmix(&cfg, &pcm);
            assert_eq!(roles.len(), 5);
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
        for proc_ in [0u8, 1, 2] {
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
    fn phase_mixed_downmix_carries_the_mono_surround_in_antiphase() {
        // §C.2.1.5 procedure 2: Lo = Lw + Cw − jSw, Ro = Rw + Cw + jSw
        // with jS the mono component of LS / RS.
        let cfg = McEncodeConfig {
            dematrix_procedure: 2,
            ..McEncodeConfig::default()
        };
        let pcm: Vec<Vec<f64>> = vec![vec![0.0], vec![0.0], vec![0.0], vec![0.4], vec![0.2]];
        let (base, roles) = matrix_downmix(&cfg, &pcm);
        let (alpha, _, gamma) = matrix_coeffs(2);
        let js = 0.3;
        assert!((base[0][0] + alpha * gamma * js).abs() < 1e-12);
        assert!((base[1][0] - alpha * gamma * js).abs() < 1e-12);
        assert!((roles[3][0] - alpha * gamma * 0.4).abs() < 1e-12);
        assert!((roles[4][0] - alpha * gamma * 0.2).abs() < 1e-12);
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
                dematrix_procedure: 4,
                ..McEncodeConfig::default()
            },
            // '10' needs a centre and surround.
            McEncodeConfig {
                front: 2,
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
            McEncodeConfig {
                second_stereo: true,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                front: 2,
                phantom_centre: true,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                dematrix_procedure: 3,
                phantom_centre: true,
                ..McEncodeConfig::default()
            },
            // Phantom coding restricts tc_allocation to centre rows.
            McEncodeConfig {
                phantom_centre: true,
                tc_allocation: 1,
                ..McEncodeConfig::default()
            },
            McEncodeConfig {
                multilingual: 8,
                ..McEncodeConfig::default()
            },
            // 3/1 row 5 exists only under procedure '10'.
            McEncodeConfig {
                surround: 1,
                tc_allocation: 5,
                ..McEncodeConfig::default()
            },
        ] {
            assert!(
                matches!(bad.validate(), Err(McEncodeError::BadConfig(_))),
                "{bad:?}"
            );
        }
        // …and the 3/1 row 5 IS legal under '10'.
        let ok = McEncodeConfig {
            surround: 1,
            dematrix_procedure: 2,
            tc_allocation: 5,
            ..McEncodeConfig::default()
        };
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn legal_tc_values_follow_the_2_5_2_15_tables() {
        let base = McEncodeConfig::default();
        assert_eq!(legal_tc_values(&base), (0..=7).collect::<Vec<_>>());
        let phantom = McEncodeConfig {
            phantom_centre: true,
            ..base
        };
        assert_eq!(legal_tc_values(&phantom), vec![0, 3, 4, 5]);
        let p31 = McEncodeConfig {
            surround: 1,
            phantom_centre: true,
            ..base
        };
        assert_eq!(legal_tc_values(&p31), vec![0, 3, 4]);
        let p30 = McEncodeConfig {
            surround: 0,
            phantom_centre: true,
            ..base
        };
        assert_eq!(legal_tc_values(&p30), vec![0]);
        let unmatrixed = McEncodeConfig {
            dematrix_procedure: 3,
            ..base
        };
        assert_eq!(legal_tc_values(&unmatrixed), vec![0]);
    }

    #[test]
    fn splice_bits_overwrites_the_exact_range() {
        let mut frame = vec![0xFFu8; 4];
        let blob = [0b1010_1010u8, 0b1100_0000];
        splice_bits(&mut frame, 5, &blob, 0, 10);
        // Bits 5..15 replaced with 1010101011; bits 0..5 and 15.. stay 1.
        assert_eq!(frame[0], 0b1111_1101);
        assert_eq!(frame[1], 0b0101_0111);
        assert_eq!(frame[2], 0xFF);
        // A skip offset reads from inside the blob.
        let mut frame = vec![0u8; 2];
        splice_bits(&mut frame, 0, &blob, 4, 4);
        assert_eq!(frame[0], 0b1010_0000);
    }

    #[test]
    fn ext_frame_header_and_crc_follow_2_5_2_10() {
        // Header-only frame: ext_length 5, CRC over the 12 bits of
        // ext_length + ext_ID_bit.
        let ext = build_ext_frame(&[], 0, 0).unwrap();
        assert_eq!(ext.len(), 5);
        assert_eq!(ext[0], 0x7F);
        assert_eq!(ext[1] & 0xF0, 0xF0);
        let length = (u32::from(ext[3] & 0x0F) << 7) | (u32::from(ext[4]) >> 1);
        assert_eq!(length, 5);
        let wire_crc =
            (u16::from(ext[1] & 0x0F) << 12) | (u16::from(ext[2]) << 4) | (u16::from(ext[3]) >> 4);
        assert_eq!(wire_crc, crc_feed(INIT_STATE, &ext, 28, 40));
        // Data-bearing frame: bits land after the 5-byte header, CRC
        // covers 128 bits.
        let blob = [0xA5u8; 40];
        let ext = build_ext_frame(&blob, 3, 300).unwrap();
        assert_eq!(ext.len(), 5 + 38);
        let wire_crc =
            (u16::from(ext[1] & 0x0F) << 12) | (u16::from(ext[2]) << 4) | (u16::from(ext[3]) >> 4);
        assert_eq!(wire_crc, crc_feed(INIT_STATE, &ext, 28, 28 + 128));
        for i in 0..300usize {
            let src = 3 + i;
            let want = (blob[src / 8] >> (7 - src % 8)) & 1;
            let pos = 40 + i;
            let have = (ext[pos / 8] >> (7 - pos % 8)) & 1;
            assert_eq!(have, want, "bit {i}");
        }
        assert!(matches!(
            build_ext_frame(&vec![0u8; 3000], 0, 2043 * 8),
            Err(McEncodeError::ExtFrameTooLarge { .. })
        ));
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

    fn xorshift(seed: &mut u64) -> f64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        (*seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }

    #[test]
    fn prediction_fit_recovers_a_planted_linear_relation() {
        // Plant x = 0,5·a + 0,25·b (grid-exact coefficients) in every
        // predictable subband: the fit must enable all eight groups,
        // recover the coefficients, and leave a near-zero residual.
        let mut a: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut b: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut x: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for sb in 0..8 {
            for t in 0..SLOTS {
                a[sb][t] = xorshift(&mut seed);
                b[sb][t] = xorshift(&mut seed);
                x[sb][t] = 0.5 * a[sb][t] + 0.25 * b[sb][t];
            }
        }
        let mut tx = vec![x];
        let t01 = vec![a, b];
        let targets: [Vec<usize>; 8] = std::array::from_fn(|_| vec![0]);
        let plan = fit_and_apply_prediction(&mut tx, &t01, &targets);
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
        // win is *real* for those very samples. The invariant: an OFF
        // group leaves its samples untouched, and an ON group's
        // transmitted residual genuinely carries ≤ 90 % of the
        // original energy.
        let mut a: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut b: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut x: SubbandFrame = Box::new([[0.0f64; SLOTS]; NUM_SUBBANDS]);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for sb in 0..8 {
            for t in 0..SLOTS {
                a[sb][t] = xorshift(&mut seed);
                b[sb][t] = xorshift(&mut seed);
                x[sb][t] = xorshift(&mut seed);
            }
        }
        let before = x.clone();
        let mut tx = vec![x];
        let t01 = vec![a, b];
        let targets: [Vec<usize>; 8] = std::array::from_fn(|_| vec![0]);
        let plan = fit_and_apply_prediction(&mut tx, &t01, &targets);
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
    fn substitution_error_is_zero_for_a_self_copy_and_full_for_silence_source() {
        let mut seed = 7u64;
        let mut x = [0.0f64; SLOTS];
        for v in x.iter_mut() {
            *v = 0.3 * xorshift(&mut seed);
        }
        let sf = {
            let s = scalefactors_of(
                &Box::new(std::array::from_fn(
                    |sb| if sb == 0 { x } else { [0.0; SLOTS] },
                )),
                SLOTS,
            );
            [s[0][0], s[1][0], s[2][0]]
        };
        let (err, energy) = substitution_error(&x, &normalised(&x), &sf);
        assert!(energy > 0.0);
        assert!(err < 1e-24, "self copy error {err}");
        let (err, energy) = substitution_error(&x, &[0.0; SLOTS], &sf);
        assert!((err - energy).abs() < 1e-12);
    }

    #[test]
    fn sample_bits_halve_with_the_granule_count() {
        for nb in [3u32, 5, 7, 9, 15, 63, 65535] {
            assert_eq!(
                sample_bits(nb, 12),
                crate::encoder_bit_allocator::sample_bits_for(nb)
            );
            assert_eq!(sample_bits(nb, 6) * 2, sample_bits(nb, 12));
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
