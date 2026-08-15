//! ISO/IEC 13818-3 §2.5 **multichannel extension** decode for Layer II.
//!
//! A 13818-3 multichannel (MC) stream is an ISO/IEC 11172-3 Layer II
//! stream whose §2.4.1.8 ancillary-data field carries the
//! `mc_extension()` payload (§2.5.1.12.1):
//!
//! ```text
//!   mc_extension() {
//!     mc_header()
//!     mc_error_check()
//!     mc_composite_status_info()
//!     mc_audio_data()
//!     ml_audio_data()
//!   }
//! ```
//!
//! For Layer II the whole extension rides `mc_extension_data_part1()`
//! directly after the base frame's sample loop (§2.5.1.3), optionally
//! continued in a separate **extension bit stream** of `ext_frame()`s
//! (§2.5.1.5 / Annex A Figure A.2) when `ext_bit_stream_present`; the
//! last `n_ad_bytes` bytes of the base frame then remain MPEG-1
//! ancillary data (§2.5.2.13).
//!
//! The decode chain implemented here follows §2.5.3.2:
//!
//! 1. base-frame decode to *subband* samples (requantised, and scaled
//!    per §2.4.3.3.3) — the compatible pair `Lo` / `Ro` (`T0` / `T1`);
//! 2. `mc_header` + CRC-detected extension parse (§2.5.3.1: "The
//!    MPEG-1 ancillary data field is initially assumed to contain the
//!    coded multichannel extension. If the mandatory CRC-check yields
//!    a valid result, then multichannel decoding will be started.");
//! 3. transmission-channel decode of `T2..T4`: bit allocation per
//!    Table B.2a (48 kHz) / B.2b (44,1 / 32 kHz) with
//!    `msblimit = sblimit` (§2.5.2.17), scfsi / scalefactors /
//!    requantisation exactly as §2.4.3;
//! 4. **dynamic crosstalk** (§2.5.3.2.1.2): missing (allocation,
//!    samples) copied from the indicated transmission channel or from
//!    `Lo` / `Ro`, re-scaled by the *destination* channel's
//!    transmitted scalefactors;
//! 5. **multichannel prediction** (§2.5.3.2.1.3): up-to-second-order
//!    predictors from `T0` / `T1` (after requantisation and
//!    scalefactor application) with per-predictor delay compensation,
//!    added to the transmitted prediction-error signal in subband
//!    groups 0..7;
//! 6. **dematrixing** (§2.5.3.2.1.1): the per-subband-group
//!    `tc_allocation` decoding matrix recovers the weighted
//!    presentation channels from `Lo`, `Ro`, `T2..T4`;
//! 7. **de-normalisation** (§2.5.3.2.5): inverse weighting (√2 or 2
//!    on centre/surround) and the overall de-normalisation factor
//!    (1 + √2, or 1,5 + 0,5·√2 for `dematrix_procedure` `'01'`);
//! 8. synthesis filterbank per presentation channel (§2.5.3.2.6), and
//!    the **LFE** channel decoded as block-companded PCM at `Fs / 96`
//!    per §2.5.3.2.4 (Layer I requantisation, no grouping, no
//!    filterbank);
//! 9. **multilingual** channels (`ml_audio_data()`, §2.5.2.18):
//!    independent Layer II channels at the full or half sampling
//!    frequency (half-frequency allocation per 13818-3 Table B.1).
//!
//! `multi_lingual_layer == '1'` (Layer III ml) is rejected — this
//! crate is the Layer II codec.
//!
//! Everything here is decode-only: the encoder-side MC matrixing is a
//! separate concern the standard leaves to §2.5's informative annexes.

// The §2.5.1 syntax loops below are deliberately written in the
// spec's own index-based `for (sb…) for (mch…)` notation so the wire
// order stays visually checkable against the printed syntax tables;
// iterator rewrites obscure that correspondence across the several
// parallel per-[mch][sb] arrays.
#![allow(clippy::needless_range_loop)]

use crate::audio_data::Scfsi;
use crate::bitalloc::{class_of_quantization, BitAllocTable, NUM_SUBBANDS};
use crate::crc::{crc16_step, INIT_STATE};
use crate::frame::PCM_SAMPLES_PER_CHANNEL;
use crate::header::{FrameHeader, HeaderError, Mode};
use crate::requant::{degroup, requantize_code};
use crate::synthesis::SynthesisFilterbank;
use crate::tables::SCALEFACTORS;
use oxideav_core::bits::BitReader;

/// Subband samples per subband per Layer II frame (12 granules × 3).
const SLOTS: usize = 36;
/// Samples per scalefactor part (§2.5.2.17: "three equal parts of 12
/// subband samples each per subband").
const SLOTS_PER_PART: usize = 12;
/// LFE samples per frame (§2.5.3.2.4: sampling frequency `Fs / 96`,
/// 1152 / 96 = 12 — one `lf_sample` per granule, §2.5.1.17).
pub const LFE_SAMPLES_PER_FRAME: usize = 12;
/// Deepest reach of the §2.5.3.2.1.3 predictor into the past:
/// `delay_comp` ≤ 7 plus predictor order `pci` ≤ 2.
const PRED_HISTORY: usize = 9;

/// √2, spelled once.
const SQRT2: f64 = std::f64::consts::SQRT_2;

/// Errors raised by the §2.5 multichannel decode.
#[derive(Debug, Clone, PartialEq)]
pub enum McError {
    /// The base frame failed to parse.
    Header(HeaderError),
    /// The base frame buffer is shorter than its signalled size.
    Truncated { have: usize, need: usize },
    /// The base frame's sample data was malformed (unknown
    /// quantization class or premature end).
    BaseFrame(String),
    /// The ancillary field does not carry a valid multichannel
    /// extension: the §2.5.2.14 `mc_crc_check` failed. Per §2.5.3.1
    /// the CRC doubles as the multichannel-presence detector, so this
    /// is also the "plain 11172-3 stream" outcome.
    McCrcMismatch { computed: u16, expected: u16 },
    /// The multichannel extension field ended before the syntax did.
    UnexpectedEnd,
    /// The base header is LSF (13818-3 §2.4). The §2.5 multichannel
    /// extension is defined on the MPEG-1-compatible (full-rate) base.
    LsfBase,
    /// `dyn_cross_mode` decoded to a value the §2.5.2.15 tables mark
    /// forbidden for the active channel configuration.
    ForbiddenDynCrossMode { sbgr: usize, mode: u8 },
    /// `multi_lingual_layer == '1'` (Layer III multilingual) — not a
    /// Layer II payload; out of scope for this crate.
    MlLayer3Unsupported,
    /// An extension frame was required (`ext_bit_stream_present`) but
    /// the extension bit stream is exhausted or absent.
    MissingExtFrame,
    /// An extension frame did not start with the §2.5.2.10
    /// `ext_syncword` `'0111 1111 1111'`.
    ExtSyncword,
    /// The §2.5.2.10 `ext_crc_check` failed.
    ExtCrcMismatch { computed: u16, expected: u16 },
    /// A scalefactor index ≥ 63 (reserved) appeared on the wire.
    ReservedScalefactor,
    /// The multichannel configuration changed mid-stream.
    ConfigChanged,
}

impl core::fmt::Display for McError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            McError::Header(e) => write!(f, "mc: base header: {e}"),
            McError::Truncated { have, need } => {
                write!(f, "mc: base frame truncated ({have} < {need} bytes)")
            }
            McError::BaseFrame(s) => write!(f, "mc: base frame: {s}"),
            McError::McCrcMismatch { computed, expected } => write!(
                f,
                "mc: mc_crc_check mismatch (computed {computed:#06x}, wire {expected:#06x}) — \
                 no multichannel extension detected"
            ),
            McError::UnexpectedEnd => write!(f, "mc: extension field ended mid-syntax"),
            McError::LsfBase => write!(f, "mc: LSF base frames carry no §2.5 extension"),
            McError::ForbiddenDynCrossMode { sbgr, mode } => {
                write!(f, "mc: forbidden dyn_cross_mode {mode} in sbgr {sbgr}")
            }
            McError::MlLayer3Unsupported => {
                write!(
                    f,
                    "mc: Layer III multilingual is out of scope for a Layer II decoder"
                )
            }
            McError::MissingExtFrame => {
                write!(
                    f,
                    "mc: ext_bit_stream_present but no extension frame available"
                )
            }
            McError::ExtSyncword => write!(f, "mc: extension frame lacks ext_syncword"),
            McError::ExtCrcMismatch { computed, expected } => write!(
                f,
                "mc: ext_crc_check mismatch (computed {computed:#06x}, wire {expected:#06x})"
            ),
            McError::ReservedScalefactor => write!(f, "mc: reserved scalefactor index 63"),
            McError::ConfigChanged => write!(f, "mc: channel configuration changed mid-stream"),
        }
    }
}

impl std::error::Error for McError {}

impl From<HeaderError> for McError {
    fn from(e: HeaderError) -> Self {
        McError::Header(e)
    }
}

// ---------------------------------------------------------------------------
// MC header (§2.5.1.13 / §2.5.2.13)
// ---------------------------------------------------------------------------

/// The §2.5.2.13 `centre` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Centre {
    /// `'00'` — no centre channel present.
    None,
    /// `'01'` — centre channel present.
    Present,
    /// `'11'` — centre bandwidth limited (Phantom coding): subbands
    /// above subband 11 are not transmitted for the centre channel.
    Phantom,
}

/// The §2.5.2.13 `surround` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surround {
    /// `'00'` — no surround.
    None,
    /// `'01'` — mono surround.
    Mono,
    /// `'10'` — stereo surround.
    Stereo,
    /// `'11'` — no surround, but second stereo programme present.
    SecondStereo,
}

/// Parsed §2.5.1.13 `mc_header()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McHeader {
    /// `ext_bit_stream_present` — an extension bit stream carries the
    /// remainder of the multichannel/multilingual information.
    pub ext_bit_stream_present: bool,
    /// `n_ad_bytes` — MPEG-1 ancillary bytes at the end of the base
    /// frame (transmitted only when an extension bit stream exists for
    /// Layer I/II; 0 otherwise).
    pub n_ad_bytes: u8,
    /// `centre`.
    pub centre: Centre,
    /// `surround`.
    pub surround: Surround,
    /// `lfe` — low frequency enhancement channel present.
    pub lfe: bool,
    /// `audio_mix` — large (`false`) / small (`true`) listening room;
    /// to be ignored by the decoder (§2.5.2.13).
    pub audio_mix: bool,
    /// `dematrix_procedure` (`0..=3`).
    pub dematrix_procedure: u8,
    /// `no_of_multi_lingual_ch` (`0..=7`).
    pub no_of_multi_lingual_ch: u8,
    /// `multi_lingual_fs` — `true` when the multilingual channels run
    /// at half the main sampling frequency.
    pub multi_lingual_fs_half: bool,
    /// `multi_lingual_layer` — `true` selects Layer III ml (rejected
    /// by this decoder), `false` Layer II ml.
    pub multi_lingual_layer3: bool,
    /// `copyright_identification_bit`.
    pub copyright_identification_bit: bool,
    /// `copyright_identification_start`.
    pub copyright_identification_start: bool,
}

// ---------------------------------------------------------------------------
// Channel configuration (§2.5.2.15)
// ---------------------------------------------------------------------------

/// The audio-channel role a transmission channel or presentation slot
/// carries (§2.5.2.15 / §2.5.3.2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McChannel {
    /// Presentation left.
    Left,
    /// Presentation right.
    Right,
    /// Centre.
    Centre,
    /// Left surround.
    LeftSurround,
    /// Right surround.
    RightSurround,
    /// Mono surround (`S`).
    MonoSurround,
    /// Second stereo programme left (`L2`).
    SecondLeft,
    /// Second stereo programme right (`R2`).
    SecondRight,
}

/// The §2.5.2.15 multichannel configuration, derived from the
/// `centre` / `surround` header fields and the base frame's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McConfig {
    /// Front channels in the *main* programme (3 or 2; 1 for a mono
    /// base).
    pub front: u8,
    /// Surround channels in the main programme (0, 1 or 2).
    pub surround: u8,
    /// Second stereo programme present (`surround == '11'`).
    pub second_stereo: bool,
    /// Number of channels in the multichannel extension part
    /// (`nmch`).
    pub nmch: usize,
    /// Width of each `tc_allocation` field in bits.
    pub tc_allocation_bits: u32,
    /// Width of each `dyn_cross_mode` field in bits.
    pub dyn_cross_bits: u32,
    /// Phantom-coded centre (`centre == '11'`).
    pub phantom_centre: bool,
}

impl McConfig {
    /// Derive the configuration per §2.5.2.15 A)…G).
    pub fn from_header(mc: &McHeader, base_mode: Mode) -> McConfig {
        let base_mono = base_mode == Mode::SingleChannel;
        let front: u8 = if base_mono {
            1
        } else if mc.centre != Centre::None {
            3
        } else {
            2
        };
        let (surround, second) = match mc.surround {
            Surround::None => (0u8, false),
            Surround::Mono => (1, false),
            Surround::Stereo => (2, false),
            Surround::SecondStereo => (0, true),
        };
        let (main_nmch, tc_bits, dc_bits) = match (front, surround) {
            (3, 2) => (3usize, 3u32, 4u32),
            (3, 1) => (2, 3, 3),
            (3, 0) => (1, 2, 1),
            (2, 2) => (2, 2, 3),
            (2, 1) => (1, 2, 1),
            // 2/0 and 1/0 — no main extension channels, zero-width
            // tc_allocation / dyn_cross_mode fields (§2.5.2.15 F, G).
            _ => (0, 0, 0),
        };
        let nmch = main_nmch + if second { 2 } else { 0 };
        McConfig {
            front,
            surround,
            second_stereo: second,
            nmch,
            tc_allocation_bits: tc_bits,
            dyn_cross_bits: dc_bits,
            phantom_centre: mc.centre == Centre::Phantom,
        }
    }

    /// Number of *main-programme* extension channels (T2…, excluding
    /// a second stereo programme).
    pub fn main_nmch(&self) -> usize {
        self.nmch - if self.second_stereo { 2 } else { 0 }
    }

    /// Presentation-channel layout of the decoded output (order of
    /// [`McDecodedFrame::channels`]).
    pub fn layout(&self) -> Vec<McChannel> {
        let mut out = Vec::new();
        match self.front {
            1 => {
                // Mono base: the compatible mono programme.
                out.push(McChannel::Left);
            }
            _ => {
                out.push(McChannel::Left);
                out.push(McChannel::Right);
                if self.front == 3 {
                    out.push(McChannel::Centre);
                }
            }
        }
        match self.surround {
            2 => {
                out.push(McChannel::LeftSurround);
                out.push(McChannel::RightSurround);
            }
            1 => out.push(McChannel::MonoSurround),
            _ => {}
        }
        if self.second_stereo {
            out.push(McChannel::SecondLeft);
            out.push(McChannel::SecondRight);
        }
        out
    }
}

/// §2.5.2.15 subband-group table: subbands spanned by each of the 12
/// subband groups.
pub const SBGR_BOUNDS: [(usize, usize); 12] = [
    (0, 0),
    (1, 1),
    (2, 2),
    (3, 3),
    (4, 4),
    (5, 5),
    (6, 6),
    (7, 7),
    (8, 9),
    (10, 11),
    (12, 15),
    (16, 31),
];

/// Map a subband to its §2.5.2.15 subband group.
pub fn sbgr_of_subband(sb: usize) -> usize {
    match sb {
        0..=7 => sb,
        8..=9 => 8,
        10..=11 => 9,
        12..=15 => 10,
        _ => 11,
    }
}

// ---------------------------------------------------------------------------
// Dynamic crosstalk (§2.5.2.15 / §2.5.3.2.1.2)
// ---------------------------------------------------------------------------

/// How one transmission channel's (allocation, samples) are obtained
/// in a subband group under dynamic crosstalk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TcSource {
    /// Transmitted normally.
    Transmitted,
    /// Copied from another extension transmission channel (0-based
    /// `mch` index within `T2..`).
    FromTc(usize),
    /// Missing with no `Tij` term — §2.5.2.15 fallback: copied from
    /// `Lo` / `Ro` / per `dyn_cross_LR` depending on the audio
    /// channel the tc carries.
    FromBase,
}

/// Per-mode dynamic-crosstalk decode: which of the (up to three) main
/// extension channels are transmitted, copied from a sibling tc, or
/// copied from the base pair. Returns `None` for forbidden codes.
///
/// Transcribed from the §2.5.2.15 `dyn_cross_mode` tables A)…E).
fn dyn_cross_sources(config: &McConfig, mode: u8) -> Option<Vec<TcSource>> {
    use TcSource::{FromBase, FromTc, Transmitted};
    let main = config.main_nmch();
    let table: Option<Vec<TcSource>> = match (config.front, config.surround) {
        (3, 2) => match mode {
            0 => Some(vec![Transmitted, Transmitted, Transmitted]),
            1 => Some(vec![Transmitted, Transmitted, FromBase]),
            2 => Some(vec![Transmitted, FromBase, Transmitted]),
            3 => Some(vec![FromBase, Transmitted, Transmitted]),
            4 => Some(vec![Transmitted, FromBase, FromBase]),
            5 => Some(vec![FromBase, Transmitted, FromBase]),
            6 => Some(vec![FromBase, FromBase, Transmitted]),
            7 => Some(vec![FromBase, FromBase, FromBase]),
            // '1000' T2 | T34 | −  : T4 copied from T3.
            8 => Some(vec![Transmitted, Transmitted, FromTc(1)]),
            // '1001' T23 | − | T4 : T3 copied from T2.
            9 => Some(vec![Transmitted, FromTc(0), Transmitted]),
            // '1010' T24 | T3 | − : T4 copied from T2.
            10 => Some(vec![Transmitted, Transmitted, FromTc(0)]),
            // '1011' T23 | − | −  : T3 from T2, T4 fallback.
            11 => Some(vec![Transmitted, FromTc(0), FromBase]),
            // '1100' T24 | − | −  : T4 from T2, T3 fallback.
            12 => Some(vec![Transmitted, FromBase, FromTc(0)]),
            // '1101' − | T34 | −  : T4 from T3, T2 fallback.
            13 => Some(vec![FromBase, Transmitted, FromTc(1)]),
            // '1110' T234 | − | − : T3 and T4 from T2.
            14 => Some(vec![Transmitted, FromTc(0), FromTc(0)]),
            _ => None,
        },
        (3, 1) | (2, 2) => match mode {
            0 => Some(vec![Transmitted, Transmitted]),
            1 => Some(vec![Transmitted, FromBase]),
            2 => Some(vec![FromBase, Transmitted]),
            3 => Some(vec![FromBase, FromBase]),
            // '100' T23 | − : second tc copied from the first.
            4 => Some(vec![Transmitted, FromTc(0)]),
            _ => None,
        },
        (3, 0) | (2, 1) if main == 1 => match mode {
            0 => Some(vec![Transmitted]),
            1 => Some(vec![FromBase]),
            _ => None,
        },
        // 2/0, 1/0 — zero-width mode field, nothing to decode.
        _ => Some(vec![]),
    };
    table.filter(|t| t.len() == main)
}

/// §2.5.2.15 `npred` table: number of predictors as a function of the
/// configuration and the active `dyn_cross_mode` (mode 0 when dynamic
/// crosstalk is off).
fn npred_for(config: &McConfig, dyn_mode: u8) -> usize {
    match (config.front, config.surround) {
        (3, 2) => [6, 4, 4, 4, 2, 2, 2, 0, 2, 2, 2, 0, 0, 0, 0]
            .get(dyn_mode as usize)
            .copied()
            .unwrap_or(0),
        (3, 1) | (2, 2) => [4, 2, 2, 0, 0].get(dyn_mode as usize).copied().unwrap_or(0),
        (3, 0) | (2, 1) if config.main_nmch() == 1 => {
            [2, 0].get(dyn_mode as usize).copied().unwrap_or(0)
        }
        _ => 0,
    }
}

/// The main extension channels (0-based `mch` indices) that carry
/// predictors under the active `dyn_cross_mode`, in transmission
/// order — each owns two predictors (`px` even from `T0`, odd from
/// `T1`), per the §2.5.3.2.1.3 correspondence and its dynamic-
/// crosstalk adaptation ("For other configurations and the different
/// dynamic crosstalk modes, the correspondence … has to be adapted").
/// Channels carrying combined (`Txy`) signals or copied channels get
/// none ("no prediction").
fn predictable_channels(config: &McConfig, dyn_mode: u8) -> Vec<usize> {
    match (config.front, config.surround) {
        (3, 2) => match dyn_mode {
            0 => vec![0, 1, 2],
            1 => vec![0, 1],
            2 => vec![0, 2],
            3 => vec![1, 2],
            4 => vec![0],
            5 => vec![1],
            6 => vec![2],
            8 => vec![0],  // T34 combined → only T2
            9 => vec![2],  // T23 combined → only T4
            10 => vec![1], // T24 combined → only T3
            _ => vec![],
        },
        (3, 1) | (2, 2) => match dyn_mode {
            0 => vec![0, 1],
            1 => vec![0],
            2 => vec![1],
            _ => vec![],
        },
        (3, 0) | (2, 1) if config.main_nmch() == 1 => match dyn_mode {
            0 => vec![0],
            _ => vec![],
        },
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// tc_allocation → audio-channel roles (§2.5.2.15 A…G)
// ---------------------------------------------------------------------------

/// The audio channels carried by the main extension transmission
/// channels for a given `tc_allocation` value (§2.5.2.15 tables).
/// Used for the §2.5.3.2.1.2 fallback-copy rule (which base channel a
/// missing tc borrows from).
fn tc_roles(config: &McConfig, tc_allocation: u8) -> Vec<McChannel> {
    use McChannel::{Centre, Left, LeftSurround, MonoSurround, Right, RightSurround};
    match (config.front, config.surround) {
        (3, 2) => match tc_allocation {
            0 => vec![Centre, LeftSurround, RightSurround],
            1 => vec![Left, LeftSurround, RightSurround],
            2 => vec![Right, LeftSurround, RightSurround],
            3 => vec![Centre, Left, RightSurround],
            4 => vec![Centre, LeftSurround, Right],
            5 => vec![Centre, Left, Right],
            6 => vec![Right, Left, RightSurround],
            _ => vec![Left, LeftSurround, Right],
        },
        (3, 1) => match tc_allocation {
            0 => vec![Centre, MonoSurround],
            1 => vec![Left, MonoSurround],
            2 => vec![Right, MonoSurround],
            3 => vec![Centre, Left],
            4 => vec![Centre, Right],
            _ => vec![Left, Right], // 5 — only with dematrix_procedure '10'
        },
        (3, 0) => match tc_allocation {
            0 => vec![Centre],
            1 => vec![Left],
            _ => vec![Right],
        },
        (2, 2) => match tc_allocation {
            0 => vec![LeftSurround, RightSurround],
            1 => vec![Left, RightSurround],
            2 => vec![LeftSurround, Right],
            _ => vec![Left, Right],
        },
        (2, 1) => match tc_allocation {
            0 => vec![MonoSurround],
            1 => vec![Left],
            _ => vec![Right],
        },
        _ => vec![],
    }
}

/// §2.5.2.15 / §2.5.3.2.1.2 fallback-copy source (base channel index
/// 0 = `Lo`, 1 = `Ro`) for a missing transmission channel carrying
/// `role`: "Lw and LSw shall be copied from Lo, Rw and RSw shall be
/// copied from Ro, Cw and Sw shall be copied from Lo if
/// dyn_cross_LR == '0', or from Ro if dyn_cross_LR == '1'."
fn fallback_base_channel(role: McChannel, dyn_cross_lr: bool) -> usize {
    match role {
        McChannel::Left | McChannel::LeftSurround | McChannel::SecondLeft => 0,
        McChannel::Right | McChannel::RightSurround | McChannel::SecondRight => 1,
        _ => usize::from(dyn_cross_lr),
    }
}

// ---------------------------------------------------------------------------
// Base-frame subband extraction
// ---------------------------------------------------------------------------

/// Requantised base-frame subband data: the compatible pair before the
/// synthesis filterbank, with the §2.4.3.3.3 scalefactor scaling kept
/// separable (dynamic crosstalk copies the *requantised but not yet
/// re-scaled* samples, §2.5.2.15).
struct BaseSubbands {
    header: FrameHeader,
    /// Requantised (unscaled) samples `s''`, `[ch][slot][sb]`.
    raw: Vec<Vec<[f64; NUM_SUBBANDS]>>,
    /// Scaled samples `s' = factor · s''`, `[ch][slot][sb]`.
    scaled: Vec<Vec<[f64; NUM_SUBBANDS]>>,
    /// Base `nb_steps` per `[ch][sb]` (0 = no allocation).
    nb_steps: [[u32; NUM_SUBBANDS]; 2],
    /// Absolute bit position of the §2.4.1.8 ancillary field start.
    anc_start_bit: u64,
    /// Frame size in bytes.
    frame_size: usize,
}

/// Decode one base Layer II frame to requantised + scaled subband
/// samples (mirrors `frame::decode_frame_with`'s sample loop, but
/// keeps the scalefactor application separable and stops before the
/// synthesis filterbank).
fn decode_base_subbands(buf: &[u8]) -> Result<BaseSubbands, McError> {
    let header = FrameHeader::parse(buf)?;
    if header.lsf {
        return Err(McError::LsfBase);
    }
    let frame_size = header.frame_size_bytes();
    if buf.len() < frame_size {
        return Err(McError::Truncated {
            have: buf.len(),
            need: frame_size,
        });
    }
    let frame = &buf[..frame_size];
    let channels = header.channels();
    let after_header_byte = if header.protection_bit { 4 } else { 6 };
    if frame_size < after_header_byte {
        return Err(McError::Truncated {
            have: frame_size,
            need: after_header_byte,
        });
    }
    let mut reader = BitReader::with_position(frame, after_header_byte);
    let audio = crate::audio_data::parse_audio_data(&header, &mut reader)
        .map_err(|e| McError::BaseFrame(e.to_string()))?;

    let mut raw = vec![vec![[0.0_f64; NUM_SUBBANDS]; SLOTS]; channels];
    let mut scaled = vec![vec![[0.0_f64; NUM_SUBBANDS]; SLOTS]; channels];

    for sample_gr in 0..12 {
        let sf_gr = sample_gr / 4;
        let base = sample_gr * 3;
        // Region 1: one triplet per channel.
        for sb in 0..audio.bound {
            for ch in 0..channels {
                let nb = audio.nb_steps[ch][sb];
                if nb == 0 {
                    continue;
                }
                let class = class_of_quantization(nb)
                    .ok_or_else(|| McError::BaseFrame(format!("unknown class {nb}")))?;
                let trip = crate::requant::read_triplet(&class, &mut reader)
                    .map_err(|e| McError::BaseFrame(e.to_string()))?;
                let sf = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = *SCALEFACTORS.get(sf).ok_or(McError::ReservedScalefactor)?;
                for (k, &s) in trip.iter().enumerate() {
                    raw[ch][base + k][sb] = s;
                    scaled[ch][base + k][sb] = s * factor;
                }
            }
        }
        // Region 2: intensity — one shared triplet.
        for sb in audio.bound..audio.sblimit {
            let nb = audio.nb_steps[0][sb];
            if nb == 0 {
                continue;
            }
            let class = class_of_quantization(nb)
                .ok_or_else(|| McError::BaseFrame(format!("unknown class {nb}")))?;
            let trip = crate::requant::read_triplet(&class, &mut reader)
                .map_err(|e| McError::BaseFrame(e.to_string()))?;
            for ch in 0..channels {
                let sf = audio.scalefactor[ch][sb][sf_gr] as usize;
                let factor = *SCALEFACTORS.get(sf).ok_or(McError::ReservedScalefactor)?;
                for (k, &s) in trip.iter().enumerate() {
                    raw[ch][base + k][sb] = s;
                    scaled[ch][base + k][sb] = s * factor;
                }
            }
        }
    }

    let mut nb_steps = [[0u32; NUM_SUBBANDS]; 2];
    nb_steps[..channels].copy_from_slice(&audio.nb_steps[..channels]);
    // A mono base presents its single channel as both `T0` and `T1`
    // sources' fallback never fires for it in practice; keep channel 1
    // zeroed.
    Ok(BaseSubbands {
        header,
        raw,
        scaled,
        nb_steps,
        anc_start_bit: reader.bit_position(),
        frame_size,
    })
}

// ---------------------------------------------------------------------------
// Bit packing (part1 ‖ ext_data concatenation)
// ---------------------------------------------------------------------------

/// Copy the bit range `[start_bit, end_bit)` of `bytes` into a fresh
/// left-aligned packed buffer.
fn pack_bit_range(bytes: &[u8], start_bit: u64, end_bit: u64) -> Vec<u8> {
    debug_assert!(start_bit <= end_bit);
    let n = (end_bit - start_bit) as usize;
    let mut out = vec![0u8; n.div_ceil(8)];
    for i in 0..n {
        let src = start_bit + i as u64;
        let bit = (bytes[(src / 8) as usize] >> (7 - (src % 8))) & 1;
        if bit != 0 {
            out[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    out
}

/// Append `extra` whole bytes after the first `bit_len` bits of
/// `packed` (shifting into the sub-byte residue if needed).
fn append_bits(packed: &mut Vec<u8>, bit_len: &mut usize, extra: &[u8]) {
    let shift = *bit_len % 8;
    if shift == 0 {
        packed.truncate(bit_len.div_ceil(8));
        packed.extend_from_slice(extra);
    } else {
        packed.truncate(bit_len.div_ceil(8));
        let keep = 8 - shift; // free bits in the last byte
        for &b in extra {
            let last = packed.len() - 1;
            packed[last] |= b >> shift;
            packed.push(b << keep);
        }
    }
    *bit_len += extra.len() * 8;
}

/// Feed bits `[from, to)` of a packed buffer into the §2.4.3.1 CRC-16
/// shift register.
fn crc_feed_bits(mut reg: u16, packed: &[u8], from: u64, to: u64) -> u16 {
    for i in from..to {
        let bit = (packed[(i / 8) as usize] >> (7 - (i % 8))) & 1;
        reg = crc16_step(reg, bit != 0);
    }
    reg
}

// ---------------------------------------------------------------------------
// Decoded output
// ---------------------------------------------------------------------------

/// One decoded multichannel frame.
#[derive(Debug, Clone, PartialEq)]
pub struct McDecodedFrame {
    /// The base frame header.
    pub base_header: FrameHeader,
    /// The parsed `mc_header()`.
    pub mc_header: McHeader,
    /// The derived channel configuration.
    pub config: McConfig,
    /// Presentation-channel labels for `channels` (same order).
    pub layout: Vec<McChannel>,
    /// Reconstructed full-bandwidth presentation channels, 1152
    /// samples each, in `layout` order.
    pub channels: Vec<Vec<f64>>,
    /// LFE channel — 12 samples at `Fs / 96` (§2.5.3.2.4) — when
    /// `mc_header.lfe`.
    pub lfe: Option<Vec<f64>>,
    /// Multilingual channels (1152 samples at `Fs`, or 576 at
    /// `Fs / 2` when `multi_lingual_fs_half`).
    pub multilingual: Vec<Vec<f64>>,
    /// This frame's §2.5.1.15 `dyn_cross_on` flag.
    pub dyn_cross_on: bool,
    /// This frame's §2.5.1.15 `mc_prediction_on` flag.
    pub mc_prediction_on: bool,
}

/// Cross-frame decode state: per-presentation-channel synthesis
/// filterbanks, multilingual filterbanks, and the §2.5.3.2.1.3
/// predictor history of the scaled `T0` / `T1` subband samples.
#[derive(Debug, Default)]
pub struct McDecodeState {
    fb: Vec<SynthesisFilterbank>,
    ml_fb: Vec<SynthesisFilterbank>,
    /// `[base_ch][sb][k]` — last [`PRED_HISTORY`] scaled samples of the
    /// previous frame, oldest first.
    pred_hist: [Vec<[f64; PRED_HISTORY]>; 2],
    layout_len: Option<usize>,
}

impl McDecodeState {
    /// Fresh state (zeroed filterbanks and predictor history).
    pub fn new() -> Self {
        McDecodeState {
            fb: Vec::new(),
            ml_fb: Vec::new(),
            pred_hist: [
                vec![[0.0; PRED_HISTORY]; NUM_SUBBANDS],
                vec![[0.0; PRED_HISTORY]; NUM_SUBBANDS],
            ],
            layout_len: None,
        }
    }

    /// Re-zero all filterbanks and the predictor history (seek /
    /// discontinuity).
    pub fn reset(&mut self) {
        for fb in self.fb.iter_mut().chain(self.ml_fb.iter_mut()) {
            fb.reset();
        }
        for hist in &mut self.pred_hist {
            for h in hist.iter_mut() {
                *h = [0.0; PRED_HISTORY];
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The per-frame decoder
// ---------------------------------------------------------------------------

/// Internal parse of `mc_composite_status_info()` (§2.5.1.15).
struct McComposite {
    tc_allocation: [u8; 12],
    dyn_cross_lr: bool,
    dyn_cross_mode: [u8; 12],
    dyn_second_stereo: [bool; 12],
    mc_prediction: [bool; 8],
    /// `predsi[sbgr][px]` — number of transmitted coefficients (0..3).
    predsi: [[u8; 6]; 8],
}

fn read_u32(r: &mut BitReader<'_>, n: u32, limit: u64) -> Result<u32, McError> {
    if r.bit_position() + u64::from(n) > limit {
        return Err(McError::UnexpectedEnd);
    }
    r.read_u32(n).map_err(|_| McError::UnexpectedEnd)
}

/// Decode one Layer II multichannel frame.
///
/// `buf` must start at the base frame's syncword; `ext_frame` is the
/// matching extension frame (starting at `ext_syncword`) when the
/// stream uses an extension bit stream. Returns the decoded frame and
/// the number of extension-frame bytes consumed (0 when none).
pub fn decode_mc_frame_with(
    buf: &[u8],
    ext_frame: Option<&[u8]>,
    state: &mut McDecodeState,
) -> Result<(McDecodedFrame, usize), McError> {
    let base = decode_base_subbands(buf)?;
    let frame = &buf[..base.frame_size];
    let total_bits = base.frame_size as u64 * 8;

    // ---- mc_header (§2.5.1.13) ---------------------------------------
    // The extension field starts at the first ancillary bit.
    if total_bits.saturating_sub(base.anc_start_bit) < 16 {
        return Err(McError::UnexpectedEnd);
    }
    let mut hdr_reader = BitReader::with_position(frame, (base.anc_start_bit / 8) as usize);
    // Skip to the exact bit.
    let skip = (base.anc_start_bit % 8) as u32;
    if skip > 0 {
        hdr_reader.skip(skip).map_err(|_| McError::UnexpectedEnd)?;
    }
    let ext_present = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let n_ad_bytes = if ext_present {
        read_u32(&mut hdr_reader, 8, total_bits)? as u8
    } else {
        0
    };
    let centre = match read_u32(&mut hdr_reader, 2, total_bits)? {
        0 => Centre::None,
        1 => Centre::Present,
        3 => Centre::Phantom,
        // '10' is "not defined" (§2.5.2.13); surface as a CRC-style
        // rejection rather than a panic — a garbage ancillary field
        // will normally also fail the CRC below, but the header parse
        // must stay total.
        _ => Centre::None,
    };
    let surround = match read_u32(&mut hdr_reader, 2, total_bits)? {
        0 => Surround::None,
        1 => Surround::Mono,
        2 => Surround::Stereo,
        _ => Surround::SecondStereo,
    };
    let lfe = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let audio_mix = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let dematrix_procedure = read_u32(&mut hdr_reader, 2, total_bits)? as u8;
    let no_of_multi_lingual_ch = read_u32(&mut hdr_reader, 3, total_bits)? as u8;
    let multi_lingual_fs_half = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let multi_lingual_layer3 = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let copyright_identification_bit = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let copyright_identification_start = read_u32(&mut hdr_reader, 1, total_bits)? == 1;
    let mc_header = McHeader {
        ext_bit_stream_present: ext_present,
        n_ad_bytes,
        centre,
        surround,
        lfe,
        audio_mix,
        dematrix_procedure,
        no_of_multi_lingual_ch,
        multi_lingual_fs_half,
        multi_lingual_layer3,
        copyright_identification_bit,
        copyright_identification_start,
    };

    // ---- assemble the extension bit field ----------------------------
    // Layer II: part1 = ancillary region minus the trailing n_ad_bytes
    // (§2.5.2.13), then optional ext_data from the extension frame
    // (§2.5.1.12.3).
    // An `n_ad_bytes` claiming more trailing ancillary bytes than the
    // frame (or its ancillary region) holds is not a valid extension.
    let part1_end_bit = total_bits
        .checked_sub(u64::from(n_ad_bytes) * 8)
        .ok_or(McError::UnexpectedEnd)?;
    if part1_end_bit < base.anc_start_bit {
        return Err(McError::UnexpectedEnd);
    }
    let mut packed = pack_bit_range(frame, base.anc_start_bit, part1_end_bit);
    let mut mc_bits = (part1_end_bit - base.anc_start_bit) as usize;

    let mut ext_consumed = 0usize;
    if ext_present {
        let ext = ext_frame.ok_or(McError::MissingExtFrame)?;
        if ext.len() < 5 {
            return Err(McError::MissingExtFrame);
        }
        // ext_header (§2.5.1.10): syncword(12) crc(16) length(11) ID(1).
        let mut er = BitReader::new(ext);
        let sync = er.read_u32(12).map_err(|_| McError::UnexpectedEnd)?;
        if sync != 0b0111_1111_1111 {
            return Err(McError::ExtSyncword);
        }
        let wire_crc = er.read_u32(16).map_err(|_| McError::UnexpectedEnd)? as u16;
        let ext_length = er.read_u32(11).map_err(|_| McError::UnexpectedEnd)? as usize;
        let _ext_id = er.read_u32(1).map_err(|_| McError::UnexpectedEnd)?;
        if ext_length < 5 || ext.len() < ext_length {
            return Err(McError::MissingExtFrame);
        }
        // §2.5.2.10: CRC over 128 bits starting at the first bit of
        // ext_length (or fewer if the frame ends earlier).
        let crc_bits = (u64::from(ext_length as u32) * 8 - 28).min(128);
        let computed = crc_feed_bits(INIT_STATE, ext, 28, 28 + crc_bits);
        if computed != wire_crc {
            return Err(McError::ExtCrcMismatch {
                computed,
                expected: wire_crc,
            });
        }
        append_bits(&mut packed, &mut mc_bits, &ext[5..ext_length]);
        ext_consumed = ext_length;
    }
    let limit = mc_bits as u64;
    let mut r = BitReader::new(&packed);

    // Header layout inside `packed`: mc_header bits then the 16-bit
    // mc_crc_check (§2.5.1.12.1 order).
    let hdr_bits: u64 = if ext_present { 24 } else { 16 };
    r.skip(hdr_bits as u32)
        .map_err(|_| McError::UnexpectedEnd)?;
    let wire_mc_crc = read_u32(&mut r, 16, limit)? as u16;

    let config = McConfig::from_header(&mc_header, base.header.mode);
    let nmch = config.nmch;
    let main_nmch = config.main_nmch();

    // ---- mc_composite_status_info (§2.5.1.15) ------------------------
    let status_start = r.bit_position();
    let tc_sbgr_select = read_u32(&mut r, 1, limit)? == 1;
    let dyn_cross_on = read_u32(&mut r, 1, limit)? == 1;
    let mc_prediction_on = read_u32(&mut r, 1, limit)? == 1;
    let mut comp = McComposite {
        tc_allocation: [0; 12],
        dyn_cross_lr: false,
        dyn_cross_mode: [0; 12],
        dyn_second_stereo: [false; 12],
        mc_prediction: [false; 8],
        predsi: [[0; 6]; 8],
    };
    let tc_bits = config.tc_allocation_bits;
    // dematrix_procedure '11' implies tc_allocation == 0 (§2.5.2.15);
    // the field is still on the wire per the syntax.
    if tc_sbgr_select {
        let v = if tc_bits > 0 {
            read_u32(&mut r, tc_bits, limit)? as u8
        } else {
            0
        };
        comp.tc_allocation = [v; 12];
    } else {
        for sbgr in 0..12 {
            comp.tc_allocation[sbgr] = if tc_bits > 0 {
                read_u32(&mut r, tc_bits, limit)? as u8
            } else {
                0
            };
        }
    }
    if dyn_cross_on {
        comp.dyn_cross_lr = read_u32(&mut r, 1, limit)? == 1;
        for sbgr in 0..12 {
            if config.dyn_cross_bits > 0 {
                comp.dyn_cross_mode[sbgr] = read_u32(&mut r, config.dyn_cross_bits, limit)? as u8;
            }
            if mc_header.surround == Surround::SecondStereo {
                comp.dyn_second_stereo[sbgr] = read_u32(&mut r, 1, limit)? == 1;
            }
        }
    }
    if mc_prediction_on {
        for sbgr in 0..8 {
            comp.mc_prediction[sbgr] = read_u32(&mut r, 1, limit)? == 1;
            if comp.mc_prediction[sbgr] {
                let dyn_mode = if dyn_cross_on {
                    comp.dyn_cross_mode[sbgr]
                } else {
                    0
                };
                let npred = npred_for(&config, dyn_mode);
                for px in 0..npred {
                    comp.predsi[sbgr][px] = read_u32(&mut r, 2, limit)? as u8;
                }
            }
        }
    }

    // Per-sbgr dynamic-crosstalk source map for the main channels.
    let mut sources: Vec<Vec<TcSource>> = Vec::with_capacity(12);
    for sbgr in 0..12 {
        let mode = if dyn_cross_on {
            comp.dyn_cross_mode[sbgr]
        } else {
            0
        };
        let s = dyn_cross_sources(&config, mode)
            .ok_or(McError::ForbiddenDynCrossMode { sbgr, mode })?;
        sources.push(s);
    }

    // `dyn_cross[mch][sb]` — true when (allocation, samples) are not
    // transmitted for that slot.
    let mut dyn_cross = [[false; NUM_SUBBANDS]; 5];
    let mc_table = match base.header.sample_rate {
        48_000 => BitAllocTable::B2a,
        _ => BitAllocTable::B2b, // 44,1 / 32 kHz (§2.5.2.17)
    };
    let msblimit = mc_table.sblimit();
    for sb in 0..msblimit {
        let sbgr = sbgr_of_subband(sb);
        for (m, src) in sources[sbgr].iter().enumerate() {
            if *src != TcSource::Transmitted {
                dyn_cross[m][sb] = true;
            }
        }
        // Second stereo: dyn_second_stereo copies R2 from L2.
        if config.second_stereo && comp.dyn_second_stereo[sbgr] {
            dyn_cross[main_nmch + 1][sb] = true;
        }
    }

    // `centre_limited` (§2.5.2.13): Phantom coding zeroes the centre
    // tc's subbands above 11. Under Phantom coding the tc_allocation
    // values are restricted so the centre rides the first extension
    // channel.
    let mut centre_limited = [[false; NUM_SUBBANDS]; 5];
    if config.phantom_centre {
        for sb in 12..msblimit {
            centre_limited[0][sb] = true;
        }
    }

    // ---- mc_audio_data (§2.5.1.17) -----------------------------------
    let audio_start = r.bit_position();
    let lfe_allocation = if mc_header.lfe {
        read_u32(&mut r, 4, limit)? as usize
    } else {
        0
    };

    // allocation[mch][sb]
    let mut alloc = [[0u32; NUM_SUBBANDS]; 5]; // nb_steps domain
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if !centre_limited[mch][sb] && !dyn_cross[mch][sb] {
                let nbal = mc_table.nbal(sb);
                let idx = read_u32(&mut r, nbal, limit)?;
                alloc[mch][sb] = mc_table.nb_steps(sb, idx).ok_or(McError::UnexpectedEnd)?;
            }
            // centre_limited ⇒ allocation stays 0 (§2.5.2.13); a
            // dyn-crossed slot is resolved by copy below.
        }
    }
    // Resolve copied allocations (§2.5.2.15: "The bit allocation of
    // subbands for which dyn_cross[Tx][sb] is true, has to be copied
    // from the corresponding transmission channel").
    let base_channels = base.header.channels();
    for sb in 0..msblimit {
        let sbgr = sbgr_of_subband(sb);
        for mch in 0..nmch {
            if !dyn_cross[mch][sb] || centre_limited[mch][sb] {
                continue;
            }
            let src = if mch < main_nmch {
                sources[sbgr][mch]
            } else {
                // dyn_second_stereo: R2 from L2.
                TcSource::FromTc(main_nmch)
            };
            alloc[mch][sb] = match src {
                TcSource::FromTc(i) => alloc[i][sb],
                TcSource::FromBase | TcSource::Transmitted => {
                    let roles = tc_roles(&config, comp.tc_allocation[sbgr]);
                    let role = roles.get(mch).copied().unwrap_or(McChannel::Left);
                    let bch = fallback_base_channel(role, comp.dyn_cross_lr).min(base_channels - 1);
                    base.nb_steps[bch][sb]
                }
            };
        }
    }

    // scfsi[mch][sb]
    let mut scfsi = [[Scfsi::ThreePerGranule; NUM_SUBBANDS]; 5];
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if alloc[mch][sb] != 0 {
                scfsi[mch][sb] = match read_u32(&mut r, 2, limit)? {
                    0 => Scfsi::ThreePerGranule,
                    1 => Scfsi::Share01Then2,
                    2 => Scfsi::ShareAll,
                    _ => Scfsi::Share0Then12,
                };
            }
        }
    }
    let scfsi_end = r.bit_position();

    // ---- mc_error_check (§2.5.2.14) ----------------------------------
    // "the calculation begins with the first bit of the multichannel
    // header and ends with the last bit of the scfsi field, but
    // excluding the mc_crc_check field itself."
    let mut reg = crc_feed_bits(INIT_STATE, &packed, 0, hdr_bits);
    reg = crc_feed_bits(reg, &packed, status_start, scfsi_end);
    if reg != wire_mc_crc {
        return Err(McError::McCrcMismatch {
            computed: reg,
            expected: wire_mc_crc,
        });
    }
    let _ = audio_start;

    if multi_lingual_layer3 {
        return Err(McError::MlLayer3Unsupported);
    }

    // ---- prediction coefficients (§2.5.1.17 / §2.5.3.2.1.3) ----------
    // delay_comp[sbgr][px], pred_coef[sbgr][px][pci]
    let mut delay_comp = [[0u32; 6]; 8];
    let mut pred_coef = [[[0.0f64; 3]; 6]; 8];
    if mc_prediction_on {
        for sbgr in 0..8 {
            if !comp.mc_prediction[sbgr] {
                continue;
            }
            let dyn_mode = if dyn_cross_on {
                comp.dyn_cross_mode[sbgr]
            } else {
                0
            };
            let npred = npred_for(&config, dyn_mode);
            for px in 0..npred {
                let n_coef = comp.predsi[sbgr][px];
                if n_coef != 0 {
                    delay_comp[sbgr][px] = read_u32(&mut r, 3, limit)?;
                    for pci in 0..n_coef as usize {
                        let v = read_u32(&mut r, 8, limit)? as f64;
                        // §2.5.3.2.1.3 dequantisation.
                        pred_coef[sbgr][px][pci] = (v - 127.0) / 32.0;
                    }
                }
            }
        }
    }

    // ---- lf_scalefactor + scalefactors (§2.5.1.17) -------------------
    let lf_scalefactor = if lfe_allocation != 0 {
        let v = read_u32(&mut r, 6, limit)? as usize;
        if v >= SCALEFACTORS.len() {
            return Err(McError::ReservedScalefactor);
        }
        v
    } else {
        0
    };
    let mut scalefactor = [[[0u8; 3]; NUM_SUBBANDS]; 5];
    for sb in 0..msblimit {
        for mch in 0..nmch {
            if alloc[mch][sb] == 0 {
                continue;
            }
            let read6 = |r: &mut BitReader<'_>| -> Result<u8, McError> {
                let v = read_u32(r, 6, limit)? as u8;
                if v >= 63 {
                    return Err(McError::ReservedScalefactor);
                }
                Ok(v)
            };
            match scfsi[mch][sb] {
                Scfsi::ThreePerGranule => {
                    for p in 0..3 {
                        scalefactor[mch][sb][p] = read6(&mut r)?;
                    }
                }
                Scfsi::Share01Then2 => {
                    let a = read6(&mut r)?;
                    let b = read6(&mut r)?;
                    scalefactor[mch][sb] = [a, a, b];
                }
                Scfsi::ShareAll => {
                    let a = read6(&mut r)?;
                    scalefactor[mch][sb] = [a, a, a];
                }
                Scfsi::Share0Then12 => {
                    let a = read6(&mut r)?;
                    let b = read6(&mut r)?;
                    scalefactor[mch][sb] = [a, b, b];
                }
            }
        }
    }

    // ---- granule loop (§2.5.1.17) ------------------------------------
    let mut lfe_samples = vec![0.0f64; LFE_SAMPLES_PER_FRAME];
    let mut raw_mc = vec![vec![[0.0f64; NUM_SUBBANDS]; SLOTS]; nmch];
    let lfe_bits = (lfe_allocation as u32) + 1; // table: index 1 → 2 bits … 15 → 16 bits
    for gr in 0..12 {
        if lfe_allocation != 0 {
            let code = read_u32(&mut r, lfe_bits, limit)?;
            lfe_samples[gr] = requantize_lfe(code, lfe_bits) * SCALEFACTORS[lf_scalefactor];
        }
        let base_slot = gr * 3;
        for sb in 0..msblimit {
            for (mch, raw_ch) in raw_mc.iter_mut().enumerate() {
                if alloc[mch][sb] == 0 || dyn_cross[mch][sb] {
                    continue;
                }
                let class = class_of_quantization(alloc[mch][sb]).ok_or(McError::UnexpectedEnd)?;
                if class.grouping {
                    let combined = read_u32(&mut r, class.bits_per_codeword, limit)?;
                    let codes = degroup(&class, combined).map_err(|_| McError::UnexpectedEnd)?;
                    for (k, &c) in codes.iter().enumerate() {
                        raw_ch[base_slot + k][sb] = requantize_code(&class, c);
                    }
                } else {
                    for k in 0..3 {
                        let code = read_u32(&mut r, class.bits_per_codeword, limit)?;
                        raw_ch[base_slot + k][sb] = requantize_code(&class, code);
                    }
                }
            }
        }
    }

    // ---- dynamic-crosstalk sample copy (§2.5.3.2.1.2) ----------------
    // Copy the *requantised but not yet re-scaled* subband samples.
    for sb in 0..msblimit {
        let sbgr = sbgr_of_subband(sb);
        for mch in 0..nmch {
            if !dyn_cross[mch][sb] || alloc[mch][sb] == 0 {
                continue;
            }
            let src = if mch < main_nmch {
                sources[sbgr][mch]
            } else {
                TcSource::FromTc(main_nmch)
            };
            match src {
                TcSource::FromTc(i) => {
                    for slot in 0..SLOTS {
                        raw_mc[mch][slot][sb] = raw_mc[i][slot][sb];
                    }
                }
                TcSource::FromBase | TcSource::Transmitted => {
                    let roles = tc_roles(&config, comp.tc_allocation[sbgr]);
                    let role = roles.get(mch).copied().unwrap_or(McChannel::Left);
                    let bch = fallback_base_channel(role, comp.dyn_cross_lr).min(base_channels - 1);
                    for slot in 0..SLOTS {
                        raw_mc[mch][slot][sb] = base.raw[bch][slot][sb];
                    }
                }
            }
        }
    }

    // ---- scalefactor application (§2.5.3.2.3) ------------------------
    let mut scaled_mc = vec![vec![[0.0f64; NUM_SUBBANDS]; SLOTS]; nmch];
    for (mch, (raw_ch, scaled_ch)) in raw_mc.iter().zip(scaled_mc.iter_mut()).enumerate() {
        for sb in 0..msblimit {
            if alloc[mch][sb] == 0 {
                continue;
            }
            for slot in 0..SLOTS {
                let part = slot / SLOTS_PER_PART;
                let f = SCALEFACTORS[scalefactor[mch][sb][part] as usize];
                scaled_ch[slot][sb] = raw_ch[slot][sb] * f;
            }
        }
    }

    // ---- multichannel prediction (§2.5.3.2.1.3) ----------------------
    // T0/T1 with the previous frame's history prepended.
    if mc_prediction_on {
        let mut t01 = [
            vec![[0.0f64; PRED_HISTORY + SLOTS]; NUM_SUBBANDS],
            vec![[0.0f64; PRED_HISTORY + SLOTS]; NUM_SUBBANDS],
        ];
        for bch in 0..2 {
            let src_ch = bch.min(base_channels - 1);
            for (sb, ext) in t01[bch].iter_mut().enumerate() {
                ext[..PRED_HISTORY].copy_from_slice(&state.pred_hist[bch][sb]);
                for slot in 0..SLOTS {
                    ext[PRED_HISTORY + slot] = base.scaled[src_ch][slot][sb];
                }
            }
        }
        for sbgr in 0..8usize {
            if !comp.mc_prediction[sbgr] {
                continue;
            }
            let sb = sbgr; // groups 0..7 are single subbands
            if sb >= msblimit {
                continue;
            }
            let dyn_mode = if dyn_cross_on {
                comp.dyn_cross_mode[sbgr]
            } else {
                0
            };
            let targets = predictable_channels(&config, dyn_mode);
            for (k, &mch) in targets.iter().enumerate() {
                for slot in 0..SLOTS {
                    let mut pred = 0.0f64;
                    for src in 0..2usize {
                        let px = 2 * k + src;
                        let d = delay_comp[sbgr][px] as usize;
                        for pci in 0..3usize {
                            let c = pred_coef[sbgr][px][pci];
                            if c != 0.0 {
                                let idx = PRED_HISTORY + slot - d - pci;
                                pred += c * t01[src][sb][idx];
                            }
                        }
                    }
                    scaled_mc[mch][slot][sb] += pred;
                }
            }
        }
    }
    // Advance the predictor history with this frame's tail.
    for bch in 0..2 {
        let src_ch = bch.min(base_channels - 1);
        for sb in 0..NUM_SUBBANDS {
            for k in 0..PRED_HISTORY {
                state.pred_hist[bch][sb][k] = base.scaled[src_ch][SLOTS - PRED_HISTORY + k][sb];
            }
        }
    }

    // ---- dematrixing + de-normalisation (§2.5.3.2.1.1 / §2.5.3.2.5) --
    let layout = config.layout();
    if let Some(n) = state.layout_len {
        if n != layout.len() {
            return Err(McError::ConfigChanged);
        }
    } else {
        state.layout_len = Some(layout.len());
    }
    let nch_out = layout.len();
    let mut out_sub = vec![vec![[0.0f64; NUM_SUBBANDS]; SLOTS]; nch_out];
    let proc_ = dematrix_procedure;
    for slot in 0..SLOTS {
        for sb in 0..NUM_SUBBANDS {
            let sbgr = sbgr_of_subband(sb);
            let tc = comp.tc_allocation[sbgr];
            let lo = base.scaled[0][slot][sb];
            let ro = base.scaled[base_channels - 1][slot][sb];
            let t = |m: usize| -> f64 {
                if m < main_nmch && sb < msblimit {
                    scaled_mc[m][slot][sb]
                } else {
                    0.0
                }
            };
            let mixed = dematrix(&config, proc_, tc, lo, ro, t(0), t(1), t(2));
            for (ch, v) in mixed.into_iter().take(nch_out).enumerate() {
                out_sub[ch][slot][sb] = v;
            }
            if config.second_stereo && sb < msblimit {
                // Second stereo programme: transmitted directly, not
                // part of any dematrixing (§2.5.3.2.1.1).
                out_sub[nch_out - 2][slot][sb] = scaled_mc[main_nmch][slot][sb];
                out_sub[nch_out - 1][slot][sb] = scaled_mc[main_nmch + 1][slot][sb];
            }
        }
    }

    // ---- synthesis (§2.5.3.2.6) --------------------------------------
    while state.fb.len() < nch_out {
        state.fb.push(SynthesisFilterbank::new());
    }
    let mut channels: Vec<Vec<f64>> = Vec::with_capacity(nch_out);
    let mut out_block = [0.0f64; NUM_SUBBANDS];
    for (ch, sub) in out_sub.iter().enumerate() {
        let fb = &mut state.fb[ch];
        let mut pcm = Vec::with_capacity(PCM_SAMPLES_PER_CHANNEL);
        for slot in sub.iter() {
            fb.push_subbands(slot, &mut out_block);
            pcm.extend_from_slice(&out_block);
        }
        channels.push(pcm);
    }

    // ---- multilingual channels (§2.5.1.18) ---------------------------
    let nmlch = no_of_multi_lingual_ch as usize;
    let mut multilingual: Vec<Vec<f64>> = Vec::new();
    if nmlch > 0 {
        let ml_table = if multi_lingual_fs_half {
            BitAllocTable::B1Lsf
        } else {
            mc_table
        };
        let mlsblimit = ml_table.sblimit();
        let ngr: usize = if multi_lingual_fs_half { 6 } else { 12 };
        let ml_slots = ngr * 3;
        let mut ml_alloc = vec![[0u32; NUM_SUBBANDS]; nmlch];
        for sb in 0..mlsblimit {
            for ch_alloc in ml_alloc.iter_mut() {
                let idx = read_u32(&mut r, ml_table.nbal(sb), limit)?;
                ch_alloc[sb] = ml_table.nb_steps(sb, idx).ok_or(McError::UnexpectedEnd)?;
            }
        }
        let mut ml_scfsi = vec![[Scfsi::ThreePerGranule; NUM_SUBBANDS]; nmlch];
        for sb in 0..mlsblimit {
            for (ch_alloc, ch_scfsi) in ml_alloc.iter().zip(ml_scfsi.iter_mut()) {
                if ch_alloc[sb] != 0 {
                    ch_scfsi[sb] = match read_u32(&mut r, 2, limit)? {
                        0 => Scfsi::ThreePerGranule,
                        1 => Scfsi::Share01Then2,
                        2 => Scfsi::ShareAll,
                        _ => Scfsi::Share0Then12,
                    };
                }
            }
        }
        let mut ml_scf = vec![[[0u8; 3]; NUM_SUBBANDS]; nmlch];
        for sb in 0..mlsblimit {
            for mlch in 0..nmlch {
                if ml_alloc[mlch][sb] == 0 {
                    continue;
                }
                let read6 = |r: &mut BitReader<'_>| -> Result<u8, McError> {
                    let v = read_u32(r, 6, limit)? as u8;
                    if v >= 63 {
                        return Err(McError::ReservedScalefactor);
                    }
                    Ok(v)
                };
                ml_scf[mlch][sb] = match ml_scfsi[mlch][sb] {
                    Scfsi::ThreePerGranule => [read6(&mut r)?, read6(&mut r)?, read6(&mut r)?],
                    Scfsi::Share01Then2 => {
                        let a = read6(&mut r)?;
                        let b = read6(&mut r)?;
                        [a, a, b]
                    }
                    Scfsi::ShareAll => {
                        let a = read6(&mut r)?;
                        [a, a, a]
                    }
                    Scfsi::Share0Then12 => {
                        let a = read6(&mut r)?;
                        let b = read6(&mut r)?;
                        [a, b, b]
                    }
                };
            }
        }
        let mut ml_sub = vec![vec![[0.0f64; NUM_SUBBANDS]; ml_slots]; nmlch];
        for gr in 0..ngr {
            let base_slot = gr * 3;
            // §2.5.2.18: the frame divides into three parts of ngr/3
            // granules each for the scalefactor schedule.
            let part = gr / (ngr / 3);
            for sb in 0..mlsblimit {
                for mlch in 0..nmlch {
                    if ml_alloc[mlch][sb] == 0 {
                        continue;
                    }
                    let class =
                        class_of_quantization(ml_alloc[mlch][sb]).ok_or(McError::UnexpectedEnd)?;
                    let f = SCALEFACTORS[ml_scf[mlch][sb][part] as usize];
                    if class.grouping {
                        let combined = read_u32(&mut r, class.bits_per_codeword, limit)?;
                        let codes =
                            degroup(&class, combined).map_err(|_| McError::UnexpectedEnd)?;
                        for (k, &c) in codes.iter().enumerate() {
                            ml_sub[mlch][base_slot + k][sb] = requantize_code(&class, c) * f;
                        }
                    } else {
                        for k in 0..3 {
                            let code = read_u32(&mut r, class.bits_per_codeword, limit)?;
                            ml_sub[mlch][base_slot + k][sb] = requantize_code(&class, code) * f;
                        }
                    }
                }
            }
        }
        while state.ml_fb.len() < nmlch {
            state.ml_fb.push(SynthesisFilterbank::new());
        }
        for (mlch, sub) in ml_sub.iter().enumerate() {
            let fb = &mut state.ml_fb[mlch];
            let mut pcm = Vec::with_capacity(ml_slots * NUM_SUBBANDS);
            for slot in sub.iter() {
                fb.push_subbands(slot, &mut out_block);
                pcm.extend_from_slice(&out_block);
            }
            multilingual.push(pcm);
        }
    }

    Ok((
        McDecodedFrame {
            base_header: base.header,
            mc_header,
            config,
            layout,
            channels,
            lfe: if lfe { Some(lfe_samples) } else { None },
            multilingual,
            dyn_cross_on,
            mc_prediction_on,
        },
        ext_consumed,
    ))
}

/// §2.5.3.2.4 LFE requantisation: block-companded PCM, Layer I style
/// (no grouping) — `s'' = (2^nb / (2^nb − 1)) · (s''' + 2^(1−nb))`
/// with the §2.4.3.2.1 MSB-inversion / two's-complement-fraction
/// interpretation, `nb` bits per sample.
fn requantize_lfe(code: u32, nb: u32) -> f64 {
    debug_assert!((2..=16).contains(&nb));
    let msb = 1u32 << (nb - 1);
    let inverted = code ^ msb;
    let v = if inverted & msb != 0 {
        inverted as i32 - (1i32 << nb)
    } else {
        inverted as i32
    };
    let fraction = v as f64 / msb as f64;
    let two_nb = (1u64 << nb) as f64;
    (two_nb / (two_nb - 1.0)) * (fraction + 2.0f64.powi(1 - nb as i32))
}

// ---------------------------------------------------------------------------
// Dematrixing (§2.5.3.2.1.1) + de-normalisation (§2.5.3.2.5)
// ---------------------------------------------------------------------------

/// Apply the §2.5.3.2.1.1 decoding matrix for one subband sample and
/// the §2.5.3.2.5 inverse weighting + de-normalisation, returning the
/// presentation channels in [`McConfig::layout`] order (second-stereo
/// slots are produced by the caller).
#[allow(clippy::too_many_arguments)]
fn dematrix(
    config: &McConfig,
    proc_: u8,
    tc: u8,
    lo: f64,
    ro: f64,
    t2: f64,
    t3: f64,
    t4: f64,
) -> [f64; 5] {
    // dematrix_procedure '11': no matrixing — all signals directly in
    // the transmission channels, weights and de-normalisation 1.
    if proc_ == 3 {
        return match (config.front, config.surround) {
            (3, 2) => [lo, ro, t2, t3, t4],
            (3, 1) => [lo, ro, t2, t3, 0.0],
            (3, 0) => [lo, ro, t2, 0.0, 0.0],
            (2, 2) => [lo, ro, t2, t3, 0.0],
            (2, 1) => [lo, ro, t2, 0.0, 0.0],
            _ => [lo, ro, 0.0, 0.0, 0.0],
        };
    }
    // §2.5.3.2.5 constants.
    let denorm = if proc_ == 1 {
        1.5 + 0.5 * SQRT2
    } else {
        1.0 + SQRT2
    };
    let w_c = SQRT2;
    let w_s = if proc_ == 1 { 2.0 } else { SQRT2 };

    // Weighted signals per the §2.5.3.2.1.1 tables.
    match (config.front, config.surround) {
        (3, 2) => {
            let (lw, rw, cw, lsw, rsw) = if proc_ == 2 {
                // dematrixing procedure '10' — phase-mixed surround:
                // jSw = 0,5·(jLSw + jRSw) participates in the front
                // dematrix; T3/T4 carry jLSw/jRSw where transmitted.
                match tc {
                    0 => {
                        let jsw = 0.5 * (t3 + t4);
                        (lo - t2 + jsw, ro - t2 - jsw, t2, t3, t4)
                    }
                    1 => {
                        let jsw = 0.5 * (t3 + t4);
                        let cw = lo - t2 + jsw;
                        (t2, ro - cw - jsw, cw, t3, t4)
                    }
                    2 => {
                        let jsw = 0.5 * (t3 + t4);
                        let cw = ro - t2 - jsw;
                        (lo - cw + jsw, t2, cw, t3, t4)
                    }
                    3 => {
                        let rw = lo + ro - 2.0 * t2 - t3;
                        let jlsw = -2.0 * (lo - t2 - t3) - t4;
                        (t3, rw, t2, jlsw, t4)
                    }
                    4 => {
                        let lw = lo + ro - 2.0 * t2 - t4;
                        let jrsw = 2.0 * ro - 2.0 * (t2 + t4) - t3;
                        (lw, t4, t2, t3, jrsw)
                    }
                    5 => {
                        let jlsw = 0.5 * (ro - lo + t3 - t4);
                        (t3, t4, t2, jlsw, jlsw)
                    }
                    6 => {
                        let cw = 0.5 * (ro + lo - t2 - t3);
                        let jlsw = ro - lo - t2 + t3 - t4;
                        (t3, t2, cw, jlsw, t4)
                    }
                    _ => {
                        let cw = 0.5 * (lo + ro - t2 - t4);
                        let jrsw = ro - lo + t2 - t3 - t4;
                        (t2, t4, cw, t3, jrsw)
                    }
                }
            } else {
                // procedures '00' / '01'.
                match tc {
                    0 => (lo - t2 - t3, ro - t2 - t4, t2, t3, t4),
                    1 => {
                        let cw = lo - t2 - t3;
                        (t2, ro - cw - t4, cw, t3, t4)
                    }
                    2 => {
                        let cw = ro - t2 - t4;
                        (lo - cw - t3, t2, cw, t3, t4)
                    }
                    3 => {
                        let lsw = lo - t3 - t2;
                        (t3, ro - t2 - t4, t2, lsw, t4)
                    }
                    4 => (lo - t2 - t3, t4, t2, t3, ro - t4 - t2),
                    5 => {
                        let lsw = lo - t3 - t2;
                        let rsw = ro - t4 - t2;
                        (t3, t4, t2, lsw, rsw)
                    }
                    6 => {
                        let cw = ro - t2 - t4;
                        let lsw = lo - t3 - cw;
                        (t3, t2, cw, lsw, t4)
                    }
                    _ => {
                        let cw = lo - t2 - t3;
                        let rsw = ro - t4 - cw;
                        (t2, t4, cw, t3, rsw)
                    }
                }
            };
            [
                lw * denorm,
                rw * denorm,
                cw * w_c * denorm,
                lsw * w_s * denorm,
                rsw * w_s * denorm,
            ]
        }
        (3, 1) => {
            let (lw, rw, cw, sw) = if proc_ == 2 {
                match tc {
                    0 => (lo - t2 + t3, ro - t2 - t3, t2, t3),
                    1 => {
                        let cw = lo - t2 + t3;
                        (t2, ro - cw - t3, cw, t3)
                    }
                    2 => {
                        let cw = ro - t2 - t3;
                        (lo - cw + t3, t2, cw, t3)
                    }
                    3 => {
                        let jsw = -lo + t2 + t3;
                        (t3, ro - t2 - jsw, t2, jsw)
                    }
                    4 => {
                        let jsw = ro - t2 - t3;
                        (lo - t2 + jsw, t3, t2, jsw)
                    }
                    _ => {
                        let cw = 0.5 * (ro + lo - t2 - t3);
                        let jsw = 0.5 * (ro - lo + t2 - t3);
                        (t2, t3, cw, jsw)
                    }
                }
            } else {
                match tc {
                    0 => (lo - t2 - t3, ro - t2 - t3, t2, t3),
                    1 => {
                        let cw = lo - t2 - t3;
                        (t2, ro - cw - t3, cw, t3)
                    }
                    2 => {
                        let cw = ro - t2 - t3;
                        (lo - cw - t3, t2, cw, t3)
                    }
                    3 => {
                        let sw = lo - t2 - t3;
                        (t3, ro - t2 - sw, t2, sw)
                    }
                    _ => {
                        let sw = ro - t2 - t3;
                        (lo - t2 - sw, t3, t2, sw)
                    }
                }
            };
            [
                lw * denorm,
                rw * denorm,
                cw * w_c * denorm,
                sw * w_s * denorm,
                0.0,
            ]
        }
        (3, 0) => {
            let (lw, rw, cw) = match tc {
                0 => (lo - t2, ro - t2, t2),
                1 => {
                    let cw = lo - t2;
                    (t2, ro - cw, cw)
                }
                _ => {
                    let cw = ro - t2;
                    (lo - cw, t2, cw)
                }
            };
            [lw * denorm, rw * denorm, cw * w_c * denorm, 0.0, 0.0]
        }
        (2, 2) => {
            let (lw, rw, lsw, rsw) = match tc {
                0 => (lo - t2, ro - t3, t2, t3),
                1 => (t2, ro - t3, lo - t2, t3),
                2 => (lo - t2, t3, t2, ro - t3),
                _ => (t2, t3, lo - t2, ro - t3),
            };
            [
                lw * denorm,
                rw * denorm,
                lsw * w_s * denorm,
                rsw * w_s * denorm,
                0.0,
            ]
        }
        (2, 1) => {
            let (lw, rw, sw) = match tc {
                0 => (lo - t2, ro - t2, t2),
                1 => {
                    let sw = lo - t2;
                    (t2, ro - sw, sw)
                }
                _ => {
                    let sw = ro - t2;
                    (lo - sw, t2, sw)
                }
            };
            [lw * denorm, rw * denorm, sw * w_s * denorm, 0.0, 0.0]
        }
        // 2/0 and 1/0: no main extension channels were matrixed into
        // the compatible pair — the base channels ARE the programme
        // (nothing was added, so there is no encoder attenuation to
        // undo; §2.5.3.2.5's factor exists to "undo the attenuation,
        // done at the encoder side to avoid overload when calculating
        // the compatible signals").
        _ => [lo, ro, 0.0, 0.0, 0.0],
    }
}

// ---------------------------------------------------------------------------
// Stream-level driver
// ---------------------------------------------------------------------------

/// Decoded multichannel stream: concatenated per-channel PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct McDecodedStream {
    /// The first frame's `mc_header()`.
    pub mc_header: McHeader,
    /// The derived channel configuration.
    pub config: McConfig,
    /// Presentation-channel labels for `channels`.
    pub layout: Vec<McChannel>,
    /// Per-channel concatenated PCM (1152 samples per frame each).
    pub channels: Vec<Vec<f64>>,
    /// LFE PCM at `Fs / 96` (12 samples per frame), when present.
    pub lfe: Option<Vec<f64>>,
    /// Multilingual PCM (1152 or 576 samples per frame each).
    pub multilingual: Vec<Vec<f64>>,
    /// Number of frames decoded.
    pub frames: usize,
    /// Frames whose §2.5.1.15 `dyn_cross_on` flag was set.
    pub dyn_cross_frames: usize,
    /// Frames whose §2.5.1.15 `mc_prediction_on` flag was set.
    pub prediction_frames: usize,
}

/// Decode a whole Layer II multichannel stream: `base` is the
/// MPEG-1-compatible base bit stream, `ext` the §2.5.1.1.2 extension
/// bit stream when the stream uses one.
///
/// Frames are chained by `frame_size_bytes()`; each base frame whose
/// `ext_bit_stream_present` is set consumes the next extension frame
/// from `ext` (§2.5.2.13).
pub fn decode_mc_stream(base: &[u8], ext: Option<&[u8]>) -> Result<McDecodedStream, McError> {
    let mut state = McDecodeState::new();
    let mut offset = 0usize;
    let mut ext_offset = 0usize;
    let mut out: Option<McDecodedStream> = None;
    while base.len() - offset >= 4 {
        let remaining = &base[offset..];
        // Stop at a trailing partial frame (mirror of
        // `decode_all_frames`' chaining).
        let header = FrameHeader::parse(remaining)?;
        if remaining.len() < header.frame_size_bytes() {
            break;
        }
        let ext_slice = ext.map(|e| &e[ext_offset.min(e.len())..]);
        let (frame, ext_used) = decode_mc_frame_with(remaining, ext_slice, &mut state)?;
        ext_offset += ext_used;
        offset += header.frame_size_bytes();
        let frame_dyn = usize::from(frame.dyn_cross_on);
        let frame_pred = usize::from(frame.mc_prediction_on);
        match &mut out {
            None => {
                out = Some(McDecodedStream {
                    mc_header: frame.mc_header,
                    config: frame.config,
                    layout: frame.layout,
                    channels: frame.channels,
                    lfe: frame.lfe,
                    multilingual: frame.multilingual,
                    frames: 1,
                    dyn_cross_frames: frame_dyn,
                    prediction_frames: frame_pred,
                });
            }
            Some(acc) => {
                if frame.layout.len() != acc.layout.len() {
                    return Err(McError::ConfigChanged);
                }
                for (dst, src) in acc.channels.iter_mut().zip(frame.channels) {
                    dst.extend(src);
                }
                match (&mut acc.lfe, frame.lfe) {
                    (Some(dst), Some(src)) => dst.extend(src),
                    (None, None) => {}
                    _ => return Err(McError::ConfigChanged),
                }
                if acc.multilingual.len() != frame.multilingual.len() {
                    return Err(McError::ConfigChanged);
                }
                for (dst, src) in acc.multilingual.iter_mut().zip(frame.multilingual) {
                    dst.extend(src);
                }
                acc.frames += 1;
                acc.dyn_cross_frames += frame_dyn;
                acc.prediction_frames += frame_pred;
            }
        }
    }
    out.ok_or(McError::UnexpectedEnd)
}

/// Probe whether the first frame of `buf` carries a §2.5 multichannel
/// extension (per the §2.5.3.1 CRC-detection rule). Frames whose
/// extension spills into an extension bit stream are still detected —
/// the mc CRC region is required to sit in the base part only when no
/// extension frame is supplied, so this probe hands the parser an
/// empty extension and treats "missing ext frame" as *present*.
pub fn has_mc_extension(buf: &[u8]) -> bool {
    let mut state = McDecodeState::new();
    match decode_mc_frame_with(buf, None, &mut state) {
        Ok(_) => true,
        Err(McError::MissingExtFrame) => true,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- in-tree end-to-end: hand-built 2/0 + LFE extension ---------

    /// MSB-first bit writer into a byte buffer at an arbitrary bit
    /// offset (test-only helper for splicing an `mc_extension()` into
    /// a real frame's ancillary field).
    struct BitPoker<'a> {
        buf: &'a mut [u8],
        pos: u64,
    }

    impl BitPoker<'_> {
        fn put(&mut self, value: u32, n: u32) {
            for i in (0..n).rev() {
                let bit = (value >> i) & 1;
                let byte = (self.pos / 8) as usize;
                let shift = 7 - (self.pos % 8) as u32;
                self.buf[byte] = (self.buf[byte] & !(1 << shift)) | ((bit as u8) << shift);
                self.pos += 1;
            }
        }
    }

    /// Build a real Layer II base frame (this crate's encoder, silence,
    /// 384 kbit/s stereo @ 48 kHz → a large §2.4.1.8 tail), splice a
    /// hand-assembled 2/0-configuration `mc_extension()` carrying only
    /// an LFE channel into the ancillary field (correct §2.5.2.14 CRC),
    /// and decode it: the presentation pair must be the compatible pair
    /// untouched, and the LFE samples must requantise per §2.5.3.2.4.
    #[test]
    fn hand_built_lfe_extension_round_trips_through_the_mc_decoder() {
        let header = FrameHeader {
            lsf: false,
            bit_rate: 384_000,
            sample_rate: 48_000,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: crate::header::ModeExtension::Bound4,
            copyright: false,
            original: false,
            emphasis: crate::header::Emphasis::None,
            protection_bit: true,
        };
        let silence = vec![vec![0.0f64; PCM_SAMPLES_PER_CHANNEL]; 2];
        let smr: crate::encoder_bit_allocator::SmrTable = [[0.0; NUM_SUBBANDS]; 2];
        let mut frame =
            crate::encoder_frame::encode_frame(&header, &silence, &smr, 0).expect("encode");

        // Locate the ancillary field via the plain decoder.
        let plain = crate::frame::decode_frame(&frame).expect("decode base");
        let total_bits = frame.len() as u64 * 8;
        let anc_start = total_bits - plain.ancillary.bits as u64;
        assert!(
            plain.ancillary.bits > 200,
            "need tail room for the extension"
        );

        // mc_header (16 bits, no ext stream): ext=0, centre='00',
        // surround='00', lfe=1, audio_mix=0, dematrix='00', ml='000',
        // ml_fs=0, ml_layer=0, copyright bits 0.
        let lfe_alloc: u32 = 3; // 4 bits/sample, 15 levels
        let lf_scf: u32 = 3; // Table B.1 index 3 → factor 1,0
        let lfe_codes: [u32; 12] = [8, 9, 10, 11, 12, 13, 14, 15, 0, 3, 5, 7];

        // Assemble the extension twice: once to compute the CRC over
        // (header ‖ composite ‖ lfe_allocation) — the whole
        // through-scfsi protected region for a config with nmch == 0 —
        // then splice with the CRC in place.
        let mut reg = INIT_STATE;
        let feed = |reg: &mut u16, v: u32, n: u32| {
            for i in (0..n).rev() {
                *reg = crc16_step(*reg, (v >> i) & 1 != 0);
            }
        };
        // mc_header: all fields zero except lfe (bit 10 of 16).
        let mc_header_bits: u32 = 1 << 10;
        feed(&mut reg, mc_header_bits, 16);
        feed(&mut reg, 0b1_0_0, 3); // tc_sbgr_select=1, dyn_cross_on=0, mc_prediction_on=0
        feed(&mut reg, lfe_alloc, 4);

        let mut poker = BitPoker {
            buf: &mut frame,
            pos: anc_start,
        };
        poker.put(mc_header_bits, 16);
        poker.put(u32::from(reg), 16); // mc_crc_check
        poker.put(0b1_0_0, 3);
        poker.put(lfe_alloc, 4);
        poker.put(lf_scf, 6);
        for gr in 0..12 {
            poker.put(lfe_codes[gr], lfe_alloc + 1);
        }

        let mut state = McDecodeState::new();
        let (decoded, ext_used) =
            decode_mc_frame_with(&frame, None, &mut state).expect("mc decode");
        assert_eq!(ext_used, 0);
        assert_eq!(decoded.layout, vec![McChannel::Left, McChannel::Right]);
        assert_eq!(decoded.config.nmch, 0);
        assert!(decoded.mc_header.lfe);
        // 2/0: the compatible pair passes through untouched.
        assert_eq!(decoded.channels[0], plain.pcm[0]);
        assert_eq!(decoded.channels[1], plain.pcm[1]);
        // LFE requantisation per §2.5.3.2.4 with scalefactor 1,0.
        let lfe = decoded.lfe.expect("lfe present");
        assert_eq!(lfe.len(), LFE_SAMPLES_PER_FRAME);
        for (gr, &code) in lfe_codes.iter().enumerate() {
            let want = requantize_lfe(code, lfe_alloc + 1) * SCALEFACTORS[lf_scf as usize];
            assert!(
                (lfe[gr] - want).abs() < 1e-15,
                "gr {gr}: {} vs {want}",
                lfe[gr]
            );
        }

        // Tampering with a protected bit must be detected as a CRC
        // mismatch — the §2.5.3.1 multichannel-presence detector.
        let mut tampered = frame.clone();
        let flip = anc_start + 5; // inside the mc_header
        tampered[(flip / 8) as usize] ^= 1 << (7 - (flip % 8) as u32);
        let mut state = McDecodeState::new();
        match decode_mc_frame_with(&tampered, None, &mut state) {
            Err(McError::McCrcMismatch { .. }) => {}
            other => panic!("expected McCrcMismatch, got {other:?}"),
        }
    }

    /// Adversarial ancillary tails: whatever bytes sit in the tail,
    /// the multichannel parse must return a `Result`, never panic.
    #[test]
    fn adversarial_ancillary_tails_never_panic() {
        let header = FrameHeader {
            lsf: false,
            bit_rate: 384_000,
            sample_rate: 48_000,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: crate::header::ModeExtension::Bound4,
            copyright: false,
            original: false,
            emphasis: crate::header::Emphasis::None,
            protection_bit: true,
        };
        let silence = vec![vec![0.0f64; PCM_SAMPLES_PER_CHANNEL]; 2];
        let smr: crate::encoder_bit_allocator::SmrTable = [[0.0; NUM_SUBBANDS]; 2];
        let frame = crate::encoder_frame::encode_frame(&header, &silence, &smr, 0).expect("encode");
        let plain = crate::frame::decode_frame(&frame).expect("decode base");
        let tail_bytes = plain.ancillary.bytes.len();
        let tail_start = frame.len() - tail_bytes;

        // Deterministic xorshift pseudo-random tails.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rand8 = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as u8
        };
        for case in 0..200 {
            let mut buf = frame.clone();
            match case % 4 {
                0 => buf[tail_start..].fill(0x00),
                1 => buf[tail_start..].fill(0xFF),
                2 => buf[tail_start..].fill(0xAA),
                _ => {
                    for b in &mut buf[tail_start..] {
                        *b = rand8();
                    }
                }
            }
            let mut state = McDecodeState::new();
            // Any Result is acceptable; a panic is the only failure.
            let _ = decode_mc_frame_with(&buf, None, &mut state);
            let _ = has_mc_extension(&buf);
        }
    }

    /// Fuzz-found regression: on a frame *shorter* than the maximal
    /// `n_ad_bytes` claim (255 bytes — possible only at the small
    /// frame sizes, e.g. 32 kbit/s single-channel = 144 bytes), an
    /// `mc_header` asserting `ext_bit_stream_present` with an
    /// overclaimed `n_ad_bytes` used to underflow the part1-end
    /// computation. It must be rejected as a malformed extension.
    #[test]
    fn overclaimed_n_ad_bytes_on_a_small_frame_is_rejected_not_panicking() {
        let header = FrameHeader {
            lsf: false,
            bit_rate: 32_000,
            sample_rate: 48_000,
            padding: false,
            private_bit: false,
            mode: Mode::SingleChannel,
            mode_extension: crate::header::ModeExtension::Bound4,
            copyright: false,
            original: false,
            emphasis: crate::header::Emphasis::None,
            protection_bit: true,
        };
        let silence = vec![vec![0.0f64; PCM_SAMPLES_PER_CHANNEL]; 1];
        let smr: crate::encoder_bit_allocator::SmrTable = [[0.0; NUM_SUBBANDS]; 2];
        let mut frame =
            crate::encoder_frame::encode_frame(&header, &silence, &smr, 0).expect("encode");
        assert!(
            frame.len() * 8 < 16 + 255 * 8,
            "premise: frame smaller than the claim"
        );
        let plain = crate::frame::decode_frame(&frame).expect("decode base");
        let anc_start = frame.len() as u64 * 8 - plain.ancillary.bits as u64;
        // ext_bit_stream_present = 1, n_ad_bytes = 255.
        let mut poker = BitPoker {
            buf: &mut frame,
            pos: anc_start,
        };
        poker.put(1, 1);
        poker.put(255, 8);
        let mut state = McDecodeState::new();
        match decode_mc_frame_with(&frame, Some(&[0u8; 64]), &mut state) {
            Err(McError::UnexpectedEnd) => {}
            other => panic!("expected UnexpectedEnd, got {other:?}"),
        }
    }

    // ---- sbgr table -------------------------------------------------

    #[test]
    fn sbgr_mapping_matches_the_table() {
        for (sbgr, &(lo, hi)) in SBGR_BOUNDS.iter().enumerate() {
            for sb in lo..=hi {
                assert_eq!(sbgr_of_subband(sb), sbgr, "sb {sb}");
            }
        }
        // Exhaustive tiling of 0..32.
        let mut covered = [false; 32];
        for &(lo, hi) in &SBGR_BOUNDS {
            for c in covered.iter_mut().take(hi + 1).skip(lo) {
                assert!(!*c);
                *c = true;
            }
        }
        assert!(covered.iter().all(|&c| c));
    }

    // ---- npred table (§2.5.2.15) ------------------------------------

    #[test]
    fn npred_matches_predictable_channel_pairs() {
        // npred is always exactly two predictors per predictable
        // channel — the table and the channel list must agree.
        let cases: [(u8, u8, usize); 5] = [(3, 2, 15), (3, 1, 5), (2, 2, 5), (3, 0, 2), (2, 1, 2)];
        for (front, surround, n_modes) in cases {
            let cfg = McConfig {
                front,
                surround,
                second_stereo: false,
                nmch: match (front, surround) {
                    (3, 2) => 3,
                    (3, 1) | (2, 2) => 2,
                    _ => 1,
                },
                tc_allocation_bits: 0,
                dyn_cross_bits: 0,
                phantom_centre: false,
            };
            for mode in 0..n_modes {
                let npred = npred_for(&cfg, mode as u8);
                let chans = predictable_channels(&cfg, mode as u8);
                assert_eq!(npred, 2 * chans.len(), "{front}/{surround} mode {mode}");
            }
        }
    }

    #[test]
    fn dyn_cross_source_tables_cover_all_legal_modes() {
        let c32 = McConfig {
            front: 3,
            surround: 2,
            second_stereo: false,
            nmch: 3,
            tc_allocation_bits: 3,
            dyn_cross_bits: 4,
            phantom_centre: false,
        };
        for mode in 0..15u8 {
            assert!(dyn_cross_sources(&c32, mode).is_some(), "3/2 mode {mode}");
        }
        assert!(
            dyn_cross_sources(&c32, 15).is_none(),
            "3/2 '1111' forbidden"
        );
        let c31 = McConfig {
            front: 3,
            surround: 1,
            nmch: 2,
            ..c32
        };
        for mode in 0..5u8 {
            assert!(dyn_cross_sources(&c31, mode).is_some(), "3/1 mode {mode}");
        }
        for mode in 5..8u8 {
            assert!(dyn_cross_sources(&c31, mode).is_none(), "3/1 mode {mode}");
        }
    }

    // ---- LFE requantisation -----------------------------------------

    #[test]
    fn lfe_requant_matches_the_table_b4_closed_form() {
        // The §2.4.3.2.1 Layer I formula's C and D equal the Table
        // 3-B.4 constants for the 2^nb − 1 classes.
        for nb in 2u32..=16 {
            let nlevels = (1u32 << nb) - 1;
            if let Some(class) = class_of_quantization(nlevels) {
                if !class.grouping {
                    // Spot-check a few codes against requantize_code.
                    for code in [0u32, 1, (1 << nb) - 1, 1 << (nb - 1)] {
                        let ours = requantize_lfe(code, nb);
                        let theirs = requantize_code(&class, code);
                        // Table 3-B.4 prints C/D to 11 decimals; the
                        // closed form is exact — compare at print
                        // precision.
                        assert!(
                            (ours - theirs).abs() < 1e-9,
                            "nb {nb} code {code}: {ours} vs {theirs}"
                        );
                    }
                }
            }
        }
        // Full-scale positive code for nb=2: code '11' → inverted '01'
        // → +0,5 → 4/3·(0,5+0,5) = 4/3·1 … bounded by C·(1−2^−1+D).
        let v = requantize_lfe(0b11, 2);
        assert!((v - (4.0 / 3.0)).abs() < 1e-12);
    }

    // ---- dematrixing round-trips ------------------------------------

    /// Build the §2.5.3.3.2 downmix `Lo = α(L + βC + γLS)`,
    /// `Ro = α(R + βC + γRS)` and the weighted transmission channels
    /// for a given tc_allocation, then verify `dematrix` recovers the
    /// original presentation channels (procedures '00' and '01').
    #[test]
    fn dematrix_32_recovers_all_channels_for_all_tc_allocations() {
        for proc_ in [0u8, 1] {
            let (alpha, beta, gamma) = if proc_ == 1 {
                (1.0 / (1.5 + 0.5 * SQRT2), 1.0 / SQRT2, 0.5)
            } else {
                (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2)
            };
            let (l, r, c, ls, rs) = (0.31, -0.72, 0.15, 0.44, -0.27);
            let lo = alpha * (l + beta * c + gamma * ls);
            let ro = alpha * (r + beta * c + gamma * rs);
            let lw = alpha * l;
            let rw = alpha * r;
            let cw = alpha * beta * c;
            let lsw = alpha * gamma * ls;
            let rsw = alpha * gamma * rs;
            let cfg = McConfig {
                front: 3,
                surround: 2,
                second_stereo: false,
                nmch: 3,
                tc_allocation_bits: 3,
                dyn_cross_bits: 4,
                phantom_centre: false,
            };
            for tc in 0..8u8 {
                // §2.5.2.15 table A: which weighted signals ride T2..T4.
                let pick = |ch: McChannel| match ch {
                    McChannel::Left => lw,
                    McChannel::Right => rw,
                    McChannel::Centre => cw,
                    McChannel::LeftSurround => lsw,
                    _ => rsw,
                };
                let roles = tc_roles(&cfg, tc);
                let t2 = pick(roles[0]);
                let t3 = pick(roles[1]);
                let t4 = pick(roles[2]);
                let out = dematrix(&cfg, proc_, tc, lo, ro, t2, t3, t4);
                for (i, (got, want)) in out.iter().zip([l, r, c, ls, rs]).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-12,
                        "proc {proc_} tc {tc} ch {i}: {got} vs {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn dematrix_31_and_21_recover_all_channels() {
        for proc_ in [0u8, 1] {
            let (alpha, beta, gamma) = if proc_ == 1 {
                (1.0 / (1.5 + 0.5 * SQRT2), 1.0 / SQRT2, 0.5)
            } else {
                (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2)
            };
            // 3/1: Lo = α(L + βC + γS), Ro = α(R + βC + γS).
            let (l, r, c, s) = (0.21, -0.65, 0.4, -0.12);
            let lo = alpha * (l + beta * c + gamma * s);
            let ro = alpha * (r + beta * c + gamma * s);
            let (lw, rw, cw, sw) = (alpha * l, alpha * r, alpha * beta * c, alpha * gamma * s);
            let cfg = McConfig {
                front: 3,
                surround: 1,
                second_stereo: false,
                nmch: 2,
                tc_allocation_bits: 3,
                dyn_cross_bits: 3,
                phantom_centre: false,
            };
            for tc in 0..5u8 {
                let pick = |ch: McChannel| match ch {
                    McChannel::Left => lw,
                    McChannel::Right => rw,
                    McChannel::Centre => cw,
                    _ => sw,
                };
                let roles = tc_roles(&cfg, tc);
                let out = dematrix(&cfg, proc_, tc, lo, ro, pick(roles[0]), pick(roles[1]), 0.0);
                for (i, (got, want)) in out.iter().zip([l, r, c, s]).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-12,
                        "3/1 proc {proc_} tc {tc} ch {i}: {got} vs {want}"
                    );
                }
            }
            // 2/1: Lo = α(L + γS), Ro = α(R + γS).
            let lo = alpha * (l + gamma * s);
            let ro = alpha * (r + gamma * s);
            let cfg21 = McConfig {
                front: 2,
                surround: 1,
                second_stereo: false,
                nmch: 1,
                tc_allocation_bits: 2,
                dyn_cross_bits: 1,
                phantom_centre: false,
            };
            for tc in 0..3u8 {
                let pick = |ch: McChannel| match ch {
                    McChannel::Left => alpha * l,
                    McChannel::Right => alpha * r,
                    _ => alpha * gamma * s,
                };
                let roles = tc_roles(&cfg21, tc);
                let out = dematrix(&cfg21, proc_, tc, lo, ro, pick(roles[0]), 0.0, 0.0);
                for (i, (got, want)) in out.iter().zip([l, r, s]).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-12,
                        "2/1 proc {proc_} tc {tc} ch {i}: {got} vs {want}"
                    );
                }
            }
        }
    }

    #[test]
    fn dematrix_30_and_22_recover_all_channels() {
        for proc_ in [0u8, 1] {
            let (alpha, beta, gamma) = if proc_ == 1 {
                (1.0 / (1.5 + 0.5 * SQRT2), 1.0 / SQRT2, 0.5)
            } else {
                (1.0 / (1.0 + SQRT2), 1.0 / SQRT2, 1.0 / SQRT2)
            };
            // 3/0: Lo = α(L + βC), Ro = α(R + βC).
            let (l, r, c) = (0.5, -0.3, 0.2);
            let lo = alpha * (l + beta * c);
            let ro = alpha * (r + beta * c);
            let cfg = McConfig {
                front: 3,
                surround: 0,
                second_stereo: false,
                nmch: 1,
                tc_allocation_bits: 2,
                dyn_cross_bits: 1,
                phantom_centre: false,
            };
            for tc in 0..3u8 {
                let t2 = match tc {
                    0 => alpha * beta * c,
                    1 => alpha * l,
                    _ => alpha * r,
                };
                let out = dematrix(&cfg, proc_, tc, lo, ro, t2, 0.0, 0.0);
                for (i, (got, want)) in out.iter().zip([l, r, c]).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-12,
                        "3/0 proc {proc_} tc {tc} ch {i}"
                    );
                }
            }
            // 2/2: Lo = α(L + γLS), Ro = α(R + γRS).
            let (ls, rs) = (0.11, -0.4);
            let lo = alpha * (l + gamma * ls);
            let ro = alpha * (r + gamma * rs);
            let cfg22 = McConfig {
                front: 2,
                surround: 2,
                second_stereo: false,
                nmch: 2,
                tc_allocation_bits: 2,
                dyn_cross_bits: 3,
                phantom_centre: false,
            };
            for tc in 0..4u8 {
                let pick = |ch: McChannel| match ch {
                    McChannel::Left => alpha * l,
                    McChannel::Right => alpha * r,
                    McChannel::LeftSurround => alpha * gamma * ls,
                    _ => alpha * gamma * rs,
                };
                let roles = tc_roles(&cfg22, tc);
                let out = dematrix(
                    &cfg22,
                    proc_,
                    tc,
                    lo,
                    ro,
                    pick(roles[0]),
                    pick(roles[1]),
                    0.0,
                );
                for (i, (got, want)) in out.iter().zip([l, r, ls, rs]).enumerate() {
                    assert!(
                        (got - want).abs() < 1e-12,
                        "2/2 proc {proc_} tc {tc} ch {i}"
                    );
                }
            }
        }
    }

    /// Procedure '10' round-trip: construct `Lo` / `Ro` from the
    /// tc-0 inverse relations and verify every tc_allocation recovers
    /// the same weighted signal set.
    #[test]
    fn dematrix_32_procedure_2_is_consistent_across_tc_allocations() {
        let (lw, rw, cw, jlsw, jrsw) = (0.32, -0.51, 0.18, 0.27, -0.09);
        let jsw = 0.5 * (jlsw + jrsw);
        // From tc 0: Lw = Lo − Cw + jSw and Rw = Ro − Cw − jSw.
        let lo = lw + cw - jsw;
        let ro = rw + cw + jsw;
        let cfg = McConfig {
            front: 3,
            surround: 2,
            second_stereo: false,
            nmch: 3,
            tc_allocation_bits: 3,
            dyn_cross_bits: 4,
            phantom_centre: false,
        };
        let denorm = 1.0 + SQRT2;
        let want = [
            lw * denorm,
            rw * denorm,
            cw * SQRT2 * denorm,
            jlsw * SQRT2 * denorm,
            jrsw * SQRT2 * denorm,
        ];
        for tc in [0u8, 1, 2, 6, 7] {
            let pick = |ch: McChannel| match ch {
                McChannel::Left => lw,
                McChannel::Right => rw,
                McChannel::Centre => cw,
                McChannel::LeftSurround => jlsw,
                _ => jrsw,
            };
            let roles = tc_roles(&cfg, tc);
            let out = dematrix(
                &cfg,
                2,
                tc,
                lo,
                ro,
                pick(roles[0]),
                pick(roles[1]),
                pick(roles[2]),
            );
            for (i, (got, want)) in out.iter().zip(want).enumerate() {
                assert!(
                    (got - want).abs() < 1e-12,
                    "proc 10 tc {tc} ch {i}: {got} vs {want}"
                );
            }
        }
    }

    // ---- bit packing -------------------------------------------------

    #[test]
    fn pack_and_append_preserve_bit_sequences() {
        let bytes = [0b1011_0010u8, 0b0111_1101, 0b1100_0001];
        let packed = pack_bit_range(&bytes, 3, 19);
        // Bits 3..19 = 1 0010 0111 1101 11 → packed MSB-first.
        let mut r = BitReader::new(&packed);
        assert_eq!(r.read_u32(16).unwrap(), 0b1001_0011_1110_1110);
        let mut packed2 = pack_bit_range(&bytes, 3, 8); // 5 bits: 10010
        let mut len = 5usize;
        append_bits(&mut packed2, &mut len, &[0xAB, 0xCD]);
        assert_eq!(len, 21);
        let mut r = BitReader::new(&packed2);
        assert_eq!(r.read_u32(5).unwrap(), 0b10010);
        assert_eq!(r.read_u32(8).unwrap(), 0xAB);
        assert_eq!(r.read_u32(8).unwrap(), 0xCD);
    }

    // ---- config derivation ------------------------------------------

    #[test]
    fn config_derivation_covers_the_seven_layouts() {
        let hdr = |centre: Centre, surround: Surround| McHeader {
            ext_bit_stream_present: false,
            n_ad_bytes: 0,
            centre,
            surround,
            lfe: false,
            audio_mix: false,
            dematrix_procedure: 0,
            no_of_multi_lingual_ch: 0,
            multi_lingual_fs_half: false,
            multi_lingual_layer3: false,
            copyright_identification_bit: false,
            copyright_identification_start: false,
        };
        let cases = [
            (
                Centre::Present,
                Surround::Stereo,
                3usize,
                3u32,
                4u32,
                5usize,
            ),
            (Centre::Present, Surround::Mono, 2, 3, 3, 4),
            (Centre::Present, Surround::None, 1, 2, 1, 3),
            (Centre::None, Surround::Stereo, 2, 2, 3, 4),
            (Centre::None, Surround::Mono, 1, 2, 1, 3),
            (Centre::None, Surround::None, 0, 0, 0, 2),
            (Centre::Present, Surround::SecondStereo, 3, 2, 1, 5),
            (Centre::None, Surround::SecondStereo, 2, 0, 0, 4),
        ];
        for (centre, surround, nmch, tc_bits, dc_bits, n_out) in cases {
            let cfg = McConfig::from_header(&hdr(centre, surround), Mode::Stereo);
            assert_eq!(cfg.nmch, nmch, "{centre:?}/{surround:?}");
            assert_eq!(cfg.tc_allocation_bits, tc_bits, "{centre:?}/{surround:?}");
            assert_eq!(cfg.dyn_cross_bits, dc_bits, "{centre:?}/{surround:?}");
            assert_eq!(cfg.layout().len(), n_out, "{centre:?}/{surround:?}");
        }
        // Phantom coding is a 3-front config with the phantom flag.
        let cfg = McConfig::from_header(&hdr(Centre::Phantom, Surround::Stereo), Mode::Stereo);
        assert!(cfg.phantom_centre);
        assert_eq!(cfg.nmch, 3);
        // Mono base + second stereo = 1/0 + 2/0.
        let cfg = McConfig::from_header(
            &hdr(Centre::None, Surround::SecondStereo),
            Mode::SingleChannel,
        );
        assert_eq!(cfg.nmch, 2);
        assert_eq!(cfg.layout().len(), 3);
    }
}
