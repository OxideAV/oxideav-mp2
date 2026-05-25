//! # oxideav-mp2
//!
//! Pure-Rust **MPEG-1 Audio Layer II** (MP2 / MUSICAM) codec, clean-room
//! rebuilt against ISO/IEC 11172-3 (1993) and ISO/IEC 13818-3 (1997).
//!
//! ## Status
//!
//! Clean-room rebuild **in progress** (round 126, started 2026-05-25).
//! The prior implementation was retired under the workspace clean-room
//! policy because its bit-allocation and synthesis-window data tables
//! had been transcribed from external library source rather than read
//! from the ISO/IEC specification; master history was force-erased on
//! 2026-05-24 per the Hat-3 cold-enforcement procedure.
//!
//! ## What works today
//!
//! * **Frame header** (§2.4.1.3 / §2.4.2.3): the 32-bit Layer II header
//!   is parsed into the typed [`FrameHeader`] struct. All
//!   §2.4.2.3 validation is enforced up-front:
//!   - syncword (`'1111 1111 1111'`), `ID == 1` (MPEG-1), and
//!     `layer == '10'` are checked before any other field is read;
//!   - the Layer II `bitrate_index` ladder (32 / 48 / 56 / 64 / 80 /
//!     96 / 112 / 128 / 160 / 192 / 224 / 256 / 320 / 384 kbit/s,
//!     PDF page 21) and the `sampling_frequency` table (44.1 / 48 /
//!     32 kHz, PDF page 21) are decoded — forbidden (`'1111'`),
//!     free-format (`'0000'`), and reserved (`'11'`) codes are
//!     rejected explicitly;
//!   - the §2.4.2.3 "For Layer II, not all combinations of total
//!     bitrate and mode are allowed" matrix is enforced (32/48/56/80
//!     kbit/s = single_channel only; 224/256/320/384 kbit/s = no
//!     single_channel);
//!   - the `emphasis` `'10'` reserved code is rejected;
//!   - [`FrameHeader::frame_size_bytes`] returns the §2.4.3.1
//!     `N = floor(144 · bitrate / Fs) + padding_bit` byte count
//!     (Layer II uses one byte per slot per §2.4.2.1);
//!   - [`find_sync`] locates the byte-aligned 12-bit syncword in a
//!     buffer for cold synchronisation per §2.4.3.1.
//! * **Annex B Table 3-B.1** "Layer I, II scalefactors": the 63
//!   multipliers used by §2.4.3.3.3 to rescale requantized samples
//!   are tabulated in [`tables::SCALEFACTORS`] and self-checked
//!   against the closed-form `scalefactor[i] = 2^((1 − i) / 3)`.
//!
//! ## What does not work yet
//!
//! [`register`] is a no-op until the §2.4.1.6 / §2.4.3.3 audio-data
//! decode path lands (bit-allocation tables B.2a..d, scfsi, scalefactor
//! triples, requantization per Table B.4, and the §2.4.3.2 polyphase
//! synthesis filter driven by Table B.3). The Annex B Table B.3 page
//! renders (`docs/audio/mp3/annex-b-renders/Table-B.3-coefficients-Di-p56..58.png`)
//! and the Layer II Tables B.2 / B.4 (§2.4.3.3.1 / §2.4.3.3.4) are
//! staged in the PDF; the rebuild lands these in subsequent rounds
//! alongside the corresponding decode step.

#![warn(missing_debug_implementations)]

use oxideav_core::RuntimeContext;

pub mod header;
pub mod tables;

pub use header::{
    decode_bitrate, decode_sampling_frequency, find_sync, is_layer2_bitrate_mode_allowed, Emphasis,
    FrameHeader, HeaderError, Mode, ModeExtension, SYNCWORD,
};
pub use tables::{SCALEFACTORS, SCALEFACTOR_COUNT};

/// Crate-local error type. Decode paths beyond the frame header are
/// not yet wired up; [`Error::NotImplemented`] continues to gate them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A 4-byte Layer II header could not be parsed from the input.
    Header(HeaderError),
    /// A reachable Layer II decode/encode path that is not yet wired up.
    NotImplemented,
}

impl From<HeaderError> for Error {
    fn from(value: HeaderError) -> Self {
        Error::Header(value)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Header(err) => write!(f, "oxideav-mp2 header error: {err}"),
            Error::NotImplemented => {
                write!(
                    f,
                    "oxideav-mp2: codec path not yet wired up (clean-room rebuild in progress)"
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Header(err) => Some(err),
            Error::NotImplemented => None,
        }
    }
}

/// Codec registration — currently a no-op; the audio-data decode path
/// is not yet wired into the runtime codec registry.
pub fn register(_ctx: &mut RuntimeContext) {}

oxideav_core::register!("mp2", register);
