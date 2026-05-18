//! Encoder option schema for the MP2 encoder.
//!
//! Two switches are exposed via [`oxideav_core::options`]:
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
//!
//! Both options can be combined.

use oxideav_core::options::{CodecOptionsStruct, OptionField, OptionKind, OptionValue};
use oxideav_core::{Error, Result};

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
    /// inputs. Ignored for mono.
    pub joint_stereo: bool,
    /// Psychoacoustic model. Defaults to [`PsyModel::Ath`] (ATH bias);
    /// set to `"none"` for the strict-energy allocator preserved for
    /// reproducibility of pre-0.0.9 output.
    pub psy_model: PsyModel,
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
            _ => unreachable!("guarded by SCHEMA"),
        }
        Ok(())
    }
}
