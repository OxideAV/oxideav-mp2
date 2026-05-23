//! Encoder option schema for the MP2 encoder.
//!
//! The following switches are exposed via [`oxideav_core::options`]:
//!
//! - `vbr_quality` (`u32`, 0..=9): switch the encoder to **VBR**. The
//!   value selects an SNR-per-bit allocation threshold (0 = best
//!   quality / largest files, 9 = lowest quality / smallest files).
//!   In VBR mode, each frame's `bitrate_index` is chosen
//!   independently from the standard ladder to be the smallest slot
//!   that fits the encoded payload.
//! - `joint_stereo` (`bool`): enable Layer II **intensity stereo** in
//!   stereo inputs. The encoder picks the smallest bound from
//!   `{4, 8, 12, 16}` (per ISO/IEC 11172-3 §2.4.2.3 Table 3-B.3) at
//!   which the upper-band L/R correlation is high enough that a
//!   shared spectral coefficient + per-channel scalefactor is a good
//!   approximation. Subbands below the bound stay independent.
//! - `psy_model` (`string`): psychoacoustic model selector — `"ath"`
//!   (default, Terhardt ATH bias), `"none"` (strict-energy allocator
//!   preserved for byte-exact reproducibility of pre-0.0.9 output),
//!   or `""` (alias for default).
//! - `dual_channel` (`bool`): emit `mode = 0b10` (dual_channel) on
//!   stereo inputs instead of the default `mode = 0b00` (stereo). Per
//!   ISO/IEC 11172-3 §2.4.2.3 the two carry an identical Layer II
//!   bitstream layout — both channels independent, no shared
//!   subbands — but the header bit signals to the decoder that the
//!   two channels are unrelated audio streams (e.g. dual-language
//!   broadcast), as opposed to a stereo pair of one signal. Ignored
//!   for mono inputs and silently overridden by `joint_stereo = true`
//!   (joint stereo and dual channel are mutually exclusive header
//!   modes).
//! - `copyright` (`bool`): set the header `copyright` bit (ISO/IEC
//!   11172-3 §2.4.2.3). `false` (default) → `0` = no copyright on the
//!   bitstream; `true` → `1` = copyright protected.
//! - `original` (`bool`): set the header `original/copy` bit
//!   (§2.4.2.3). `false` (default) → `0` = the bitstream is a copy;
//!   `true` → `1` = the bitstream is an original.
//! - `emphasis` (`string`): set the header 2-bit `emphasis` field
//!   (§2.4.2.3). `"none"` (default, `00`), `"50/15"` (`01`,
//!   50/15 µs), or `"ccitt"` (`11`, CCITT J.17). The reserved value
//!   `10` is not exposed.
//! - `private_bit` (`bool`): set the header `private_bit` (§2.4.2.3),
//!   a single bit reserved for private use that is never assigned by
//!   ISO. `false` (default) → `0`.
//!
//! All options compose freely (CBR + joint stereo + ATH, VBR +
//! dual_channel, etc.).

use oxideav_core::options::{CodecOptionsStruct, OptionField, OptionKind, OptionValue};
use oxideav_core::{Error, Result};

/// De-emphasis selector exposed through the `emphasis` encoder option,
/// matching the 2-bit header `emphasis` field (ISO/IEC 11172-3
/// §2.4.2.3). The reserved code `0b10` is intentionally not
/// representable.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    /// `0b00` — no de-emphasis.
    #[default]
    None,
    /// `0b01` — 50/15 microsecond de-emphasis.
    FiftyFifteen,
    /// `0b11` — CCITT J.17 de-emphasis.
    CcittJ17,
}

impl Emphasis {
    /// The 2-bit on-wire `emphasis` code for this selection.
    pub fn code(self) -> u32 {
        match self {
            Emphasis::None => 0b00,
            Emphasis::FiftyFifteen => 0b01,
            Emphasis::CcittJ17 => 0b11,
        }
    }
}

/// Psychoacoustic model selector exposed through the `psy_model`
/// encoder option.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsyModel {
    /// No psychoacoustic bias — bit allocator scores by raw
    /// signal energy / cost. This is what the encoder did up to and
    /// including v0.0.8.
    None,
    /// **A**bsolute **T**hreshold of **H**earing bias. Per-subband
    /// ATH energy floor (Terhardt closed-form) is subtracted from the
    /// per-subband signal energy before scoring, so subbands whose
    /// signal sits below the audibility threshold don't compete for
    /// bits. Cheap (one extra subtraction per allocator step) and
    /// produces audibly cleaner low-bitrate output.
    #[default]
    Ath,
}

/// Typed options consumed by the MP2 encoder.
#[derive(Default, Debug, Clone)]
pub struct Mp2EncoderOptions {
    /// `Some(0..=9)` → switch to VBR with the given quality target.
    pub vbr_quality: Option<u8>,
    /// Enable joint-stereo (intensity stereo) emission for two-channel
    /// inputs. Ignored for mono. Mutually exclusive with `dual_channel`
    /// — when both are set, `joint_stereo` wins.
    pub joint_stereo: bool,
    /// Psychoacoustic model. Defaults to [`PsyModel::Ath`] (ATH bias);
    /// set to `"none"` for the strict-energy allocator preserved for
    /// reproducibility of pre-0.0.9 output.
    pub psy_model: PsyModel,
    /// Emit `mode = 0b10` (dual_channel) for two-channel inputs. Per
    /// ISO/IEC 11172-3 §2.4.2.3 the dual_channel bitstream layout is
    /// the same as plain stereo (both channels independent, no shared
    /// subbands) — only the header mode bits differ to signal that
    /// the two channels are unrelated audio streams. Ignored for
    /// mono. Overridden by `joint_stereo = true`.
    pub dual_channel: bool,
    /// Header `copyright` bit (ISO/IEC 11172-3 §2.4.2.3). `false`
    /// (default) → `0` = no copyright; `true` → `1` = copyright
    /// protected. Carries no effect on the audio payload.
    pub copyright: bool,
    /// Header `original/copy` bit (§2.4.2.3). `false` (default) →
    /// `0` = copy; `true` → `1` = original.
    pub original: bool,
    /// Header 2-bit `emphasis` field (§2.4.2.3). Defaults to
    /// [`Emphasis::None`].
    pub emphasis: Emphasis,
    /// Header `private_bit` (§2.4.2.3) — one bit reserved for private
    /// use that ISO will never assign. `false` (default) → `0`.
    pub private_bit: bool,
}

impl CodecOptionsStruct for Mp2EncoderOptions {
    const SCHEMA: &'static [OptionField] = &[
        OptionField {
            name: "vbr_quality",
            kind: OptionKind::U32,
            default: OptionValue::U32(u32::MAX),
            help: "VBR quality 0..=9 (0=best, 9=smallest). Switches to VBR mode.",
        },
        OptionField {
            name: "joint_stereo",
            kind: OptionKind::Bool,
            default: OptionValue::Bool(false),
            help: "Enable joint-stereo (intensity-stereo) coding for stereo inputs.",
        },
        OptionField {
            name: "psy_model",
            kind: OptionKind::String,
            default: OptionValue::String(String::new()),
            help: "Psychoacoustic model: \"\" (default ATH), \"ath\", or \"none\".",
        },
        OptionField {
            name: "dual_channel",
            kind: OptionKind::Bool,
            default: OptionValue::Bool(false),
            help: "Emit dual_channel mode (0b10) for two-channel inputs instead of stereo (0b00).",
        },
        OptionField {
            name: "copyright",
            kind: OptionKind::Bool,
            default: OptionValue::Bool(false),
            help: "Set the header copyright bit (1 = copyright protected).",
        },
        OptionField {
            name: "original",
            kind: OptionKind::Bool,
            default: OptionValue::Bool(false),
            help: "Set the header original/copy bit (1 = original, 0 = copy).",
        },
        OptionField {
            name: "emphasis",
            kind: OptionKind::String,
            default: OptionValue::String(String::new()),
            help: "De-emphasis: \"\"/\"none\" (00), \"50/15\" (01), or \"ccitt\" (11).",
        },
        OptionField {
            name: "private_bit",
            kind: OptionKind::Bool,
            default: OptionValue::Bool(false),
            help: "Set the header private_bit (reserved for private use).",
        },
    ];

    fn apply(&mut self, key: &str, v: &OptionValue) -> Result<()> {
        match key {
            "vbr_quality" => {
                let n = v.as_u32()?;
                if n == u32::MAX {
                    self.vbr_quality = None;
                } else if n > 9 {
                    return Err(Error::invalid(format!(
                        "MP2 encoder: vbr_quality must be 0..=9, got {n}"
                    )));
                } else {
                    self.vbr_quality = Some(n as u8);
                }
            }
            "joint_stereo" => {
                self.joint_stereo = v.as_bool()?;
            }
            "psy_model" => {
                let s = v.as_str()?;
                self.psy_model = match s {
                    "" => PsyModel::default(),
                    "ath" => PsyModel::Ath,
                    "none" => PsyModel::None,
                    other => {
                        return Err(Error::invalid(format!(
                            "MP2 encoder: psy_model must be one of \"ath\" / \"none\", got {other:?}"
                        )));
                    }
                };
            }
            "dual_channel" => {
                self.dual_channel = v.as_bool()?;
            }
            "copyright" => {
                self.copyright = v.as_bool()?;
            }
            "original" => {
                self.original = v.as_bool()?;
            }
            "emphasis" => {
                let s = v.as_str()?;
                self.emphasis = match s {
                    "" | "none" => Emphasis::None,
                    "50/15" => Emphasis::FiftyFifteen,
                    "ccitt" => Emphasis::CcittJ17,
                    other => {
                        return Err(Error::invalid(format!(
                            "MP2 encoder: emphasis must be one of \"none\" / \"50/15\" / \"ccitt\", got {other:?}"
                        )));
                    }
                };
            }
            "private_bit" => {
                self.private_bit = v.as_bool()?;
            }
            _ => unreachable!("guarded by SCHEMA"),
        }
        Ok(())
    }
}
