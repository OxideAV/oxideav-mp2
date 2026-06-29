//! §2.4.2.3 free-format (`bitrate_index == '0000'`) frame-size
//! determination for MPEG-1 / MPEG-2 LSF Layer II.
//!
//! # What "free format" is
//!
//! ISO/IEC 11172-3 §2.4.2.3 (PDF page 21): "The all zero value indicates
//! the 'free format' condition, in which a fixed bitrate which does not
//! need to be in the list can be used. Fixed means that a frame contains
//! either N or N+1 slots, depending on the value of the padding bit."
//!
//! So a free-format stream:
//!
//! * carries a **constant** (but unsignalled) bitrate for the whole
//!   stream — the §2.4.2.3 note "in free format, fixed bitrate is
//!   required" makes the constancy mandatory;
//! * sizes every frame at `N` slots, or `N + 1` when the frame's
//!   `padding_bit` is set, where `N` is a constant determined by the
//!   bitrate and sampling frequency.
//!
//! Because the header does not name the bitrate, the decoder recovers the
//! constant base slot count `N` by **measuring** the distance between the
//! frame's syncword and the *next* syncword in the stream, then removing
//! the contribution of the current frame's `padding_bit`. One byte == one
//! Layer II slot (§2.4.2.1), so the measured byte distance is the slot
//! count directly.
//!
//! # Table selection
//!
//! The §2.4.3.1 bit-allocation table is keyed on `(sampling frequency,
//! per-channel bitrate)`, and the spec tabulates only the standard ladder
//! bitrates (Table 3-B.2 headers). A free-format stream may run at any
//! constant bitrate; the standard does **not** define which Annex B table
//! to use for an off-ladder bitrate. This module therefore recovers the
//! free-format bitrate from the measured base slot count `N` by inverting
//! the §2.4.3.1 size formula and matching it against the standard ladder
//! (the common in-the-wild case: a stream that uses free-format framing
//! but a bitrate that *happens* to coincide with a ladder value, which
//! then selects a well-defined table). A measured size that does not map
//! to a ladder bitrate is reported as
//! [`FreeFormatError::UnsupportedBitrate`] rather than guessed — the
//! Annex B table for a genuinely off-ladder free-format bitrate is a
//! documented spec gap, not something this clean-room implementation
//! invents.

use crate::header::{
    decode_bitrate, decode_bitrate_lsf, find_sync, FrameHeader, HeaderError, Mode,
};

/// Errors raised while determining a free-format frame's size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeFormatError {
    /// The header at the front of the buffer could not be parsed.
    Header(HeaderError),
    /// The header was a normal (non-free-format) frame; the caller should
    /// use [`FrameHeader::frame_size_bytes`] directly.
    NotFreeFormat,
    /// No following syncword was found, so the sync-to-sync distance
    /// cannot be measured. (The very last frame of a stream needs the
    /// `N` recovered from an earlier frame instead.)
    NoFollowingSync,
    /// The measured base slot count does not correspond to any §2.4.2.3
    /// Layer II ladder bitrate at this sampling frequency, so no Annex B
    /// bit-allocation table is defined for it.
    UnsupportedBitrate {
        /// The measured base slot count (`N`, padding removed).
        base_slots: usize,
        /// The frame's sampling frequency in Hz.
        sample_rate: u32,
    },
    /// The measured distance was smaller than a minimal Layer II frame, so
    /// the "next sync" was almost certainly a false positive inside the
    /// audio payload rather than a real frame boundary.
    ImplausibleDistance {
        /// The measured byte distance between the two syncwords.
        distance: usize,
    },
}

impl core::fmt::Display for FreeFormatError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FreeFormatError::Header(e) => write!(f, "free-format: header error: {e}"),
            FreeFormatError::NotFreeFormat => {
                write!(f, "free-format: header is not a free-format frame")
            }
            FreeFormatError::NoFollowingSync => {
                write!(f, "free-format: no following syncword to measure against")
            }
            FreeFormatError::UnsupportedBitrate {
                base_slots,
                sample_rate,
            } => write!(
                f,
                "free-format: measured base size {base_slots} slots at {sample_rate} Hz \
                 maps to no Layer II ladder bitrate (Annex B table undefined)"
            ),
            FreeFormatError::ImplausibleDistance { distance } => write!(
                f,
                "free-format: sync-to-sync distance {distance} too small for a Layer II frame"
            ),
        }
    }
}

impl std::error::Error for FreeFormatError {}

impl From<HeaderError> for FreeFormatError {
    fn from(value: HeaderError) -> Self {
        FreeFormatError::Header(value)
    }
}

/// The smallest plausible Layer II frame: a 4-byte header plus the
/// §2.4.1.6 bit-allocation header for the narrowest table. Used to reject
/// a "next sync" hit that is really a false positive inside the payload.
const MIN_PLAUSIBLE_FRAME_BYTES: usize = 24;

/// Measure a free-format frame's base slot count `N` from the distance to
/// the next syncword.
///
/// `buf` must start at the current frame's syncword. The returned `N` is
/// the §2.4.2.3 base slot count with the current frame's `padding_bit`
/// removed, so the actual current-frame size is `N + padding_bit` and any
/// later frame's size is `N + that frame's padding_bit`.
///
/// # False-positive resistance
///
/// Layer II audio payload can legitimately contain the 12-bit sync
/// pattern (`0xFF 0xFx`), so the *first* sync candidate after the header
/// is not necessarily the next frame boundary. The measurement therefore
/// confirms a candidate base size `N` by requiring that a syncword also
/// appears at the position predicted for the *following* frame
/// (`N` or `N + 1` bytes onward, the §2.4.2.3 "N or N+1 slots" rule). A
/// candidate that fails this two-frame lock is rejected and the next
/// candidate is tried. This is the standard sync-lock criterion for
/// free-format streams, expressed directly from the §2.4.2.3 fixed-size
/// invariant rather than borrowed from any implementation.
pub fn measure_base_slots(buf: &[u8]) -> Result<usize, FreeFormatError> {
    let header = FrameHeader::parse_allow_free_format(buf)?;
    if !header.is_free_format() {
        return Err(FreeFormatError::NotFreeFormat);
    }
    let pad0 = if header.padding { 1usize } else { 0 };

    // Scan candidate sync positions, starting past this frame's own
    // header, and accept the first that satisfies the two-frame lock.
    let mut search_from = MIN_PLAUSIBLE_FRAME_BYTES.min(buf.len());
    loop {
        let Some(rel) = find_sync(&buf[search_from..]) else {
            // No (further) candidate passed the lock.
            return Err(FreeFormatError::NoFollowingSync);
        };
        let distance = search_from + rel; // size of THIS frame, sync-to-sync
        if distance < MIN_PLAUSIBLE_FRAME_BYTES {
            search_from = distance + 2;
            continue;
        }
        // Candidate base `N` after removing THIS frame's padding slot.
        let base = distance - pad0;
        if confirm_lock(buf, distance, base) {
            return Ok(base);
        }
        // Reject this candidate; resume the search just past it.
        search_from = distance + 2;
    }
}

/// Confirm a candidate frame size by checking the predicted *next* frame
/// boundary also carries a syncword.
///
/// The current frame occupies `[0, distance)`; the next frame starts at
/// `distance` and is sized `base + next_padding` for `next_padding ∈
/// {0, 1}` (§2.4.2.3 "N or N+1 slots"). The frame after that must begin
/// with a syncword. We accept the candidate if either next-padding choice
/// lands a syncword at the predicted position, or if the stream ends
/// (single trailing frame) with the current frame being the last.
fn confirm_lock(buf: &[u8], distance: usize, base: usize) -> bool {
    // The next frame must itself start with a syncword.
    if distance + 2 > buf.len() {
        // Stream ends right at this boundary — a lone two-frame stream.
        return true;
    }
    if !(buf[distance] == 0xFF && (buf[distance + 1] & 0xF0) == 0xF0) {
        return false;
    }
    // Predict where the frame AFTER the next one starts and require a
    // syncword there (or the stream to end), for either padding choice.
    for next_pad in [0usize, 1] {
        let after_next = distance + base + next_pad;
        if after_next + 2 > buf.len() {
            // The next frame is the last; one confirmed boundary suffices.
            return true;
        }
        if buf[after_next] == 0xFF && (buf[after_next + 1] & 0xF0) == 0xF0 {
            return true;
        }
    }
    false
}

/// Recover the constant free-format bitrate (in bit/s) from a base slot
/// count `N`, for the given header's sampling frequency / LSF flag.
///
/// Inverts the §2.4.3.1 size formula `N = floor(144 · bitrate / Fs)` by
/// scanning the §2.4.2.3 Layer II ladder for the bitrate whose computed
/// base size equals `N`. Returns the ladder `bit_rate` (bit/s) on a
/// match, or [`FreeFormatError::UnsupportedBitrate`] otherwise.
pub fn bitrate_from_base_slots(
    header: &FrameHeader,
    base_slots: usize,
) -> Result<u32, FreeFormatError> {
    let fs = header.sample_rate;
    // The 14 ladder bitrate_index values (1..=14); index 0 is free
    // format, 15 is forbidden — both excluded here by construction.
    for index in 1u8..=14 {
        let candidate = if header.lsf {
            decode_bitrate_lsf(index)
        } else {
            decode_bitrate(index)
        };
        let Ok(bit_rate) = candidate else { continue };
        let n = (144u64 * bit_rate as u64) / fs as u64;
        if n as usize == base_slots {
            return Ok(bit_rate);
        }
    }
    Err(FreeFormatError::UnsupportedBitrate {
        base_slots,
        sample_rate: fs,
    })
}

/// A free-format frame's resolved size + the recovered ladder bitrate.
///
/// The `bit_rate` is the constant free-format rate recovered from the
/// measured base slot count; it lets the caller build a
/// [`FrameHeader`] with a concrete `bit_rate` so the existing
/// bit-allocation-table selection (`crate::bitalloc::select_table`) and
/// the standard frame-decode path apply unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreeFormatLayout {
    /// Constant base slot count `N` (this frame's padding removed).
    pub base_slots: usize,
    /// The §2.4.3.1 size of THIS frame (`N + this padding_bit`).
    pub frame_size: usize,
    /// The recovered constant bitrate in bit/s.
    pub bit_rate: u32,
}

/// Fully resolve the free-format frame at the front of `buf`: measure its
/// base slot count, recover the constant bitrate, and validate the
/// recovered (bitrate, mode) pair against the §2.4.2.3 matrix exactly as
/// a normal frame would be.
pub fn resolve(buf: &[u8]) -> Result<FreeFormatLayout, FreeFormatError> {
    let header = FrameHeader::parse_allow_free_format(buf)?;
    if !header.is_free_format() {
        return Err(FreeFormatError::NotFreeFormat);
    }
    let base_slots = measure_base_slots(buf)?;
    let bit_rate = bitrate_from_base_slots(&header, base_slots)?;
    // Re-apply the §2.4.2.3 (bitrate, mode) matrix on the *recovered*
    // bitrate (MPEG-1 only); the LSF ladder does not restate it.
    if !header.lsf && !crate::header::is_layer2_bitrate_mode_allowed(bit_rate, header.mode) {
        return Err(FreeFormatError::Header(
            HeaderError::DisallowedBitrateModeCombination {
                bit_rate,
                mode: header.mode,
            },
        ));
    }
    let frame_size = base_slots + if header.padding { 1 } else { 0 };
    Ok(FreeFormatLayout {
        base_slots,
        frame_size,
        bit_rate,
    })
}

/// Build a [`FrameHeader`] with the free-format `bit_rate` filled in from
/// a recovered ladder bitrate, so the standard decode path can size and
/// allocate the frame as if the header had named the bitrate directly.
///
/// `mode` is unchanged; only `bit_rate` is rewritten from `0` to the
/// recovered constant rate.
pub fn header_with_recovered_bitrate(header: &FrameHeader, bit_rate: u32) -> FrameHeader {
    let mut h = *header;
    h.bit_rate = bit_rate;
    h
}

/// True if the recovered (bitrate, mode) pair is a single-channel-only
/// ladder row but the frame declared two channels — a malformed
/// free-format stream. Exposed for callers that want to validate without
/// triggering the full `resolve` path.
pub fn recovered_pair_is_valid(header: &FrameHeader, bit_rate: u32) -> bool {
    if header.lsf {
        return true;
    }
    let two_channel = !matches!(header.mode, Mode::SingleChannel);
    let _ = two_channel;
    crate::header::is_layer2_bitrate_mode_allowed(bit_rate, header.mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::{Emphasis, Mode, ModeExtension};

    /// Build a free-format header (bitrate_index = '0000') at a given
    /// sampling frequency / mode / padding, as raw 4 bytes.
    fn free_format_header_bytes(sf_index: u32, mode_bits: u32, padding: u32) -> [u8; 4] {
        // sync(12)=0xFFF | id(1)=1 | layer(2)='10' | protection(1)=1 |
        // bitrate(4)='0000' (free format — omitted as it is all-zero) |
        // sf(2) | pad(1) | priv(1)=0 | mode(2) | mode_ext(2)=0 | cr(1)=0
        // | orig(1)=0 | emph(2)='00'
        let word: u32 = (0xFFF << 20)
            | (1 << 19)
            | (0b10 << 17)
            | (1 << 16)
            | (sf_index << 10)
            | (padding << 9)
            | (mode_bits << 6);
        word.to_be_bytes()
    }

    #[test]
    fn parse_allow_free_format_records_zero_bitrate_sentinel() {
        let bytes = free_format_header_bytes(0b00, 0b00, 0); // 44.1k, stereo
        let h = FrameHeader::parse_allow_free_format(&bytes).expect("free-format parse");
        assert!(h.is_free_format());
        assert_eq!(h.bit_rate, 0);
        assert_eq!(h.sample_rate, 44_100);
        assert_eq!(h.mode, Mode::Stereo);
    }

    #[test]
    fn strict_parse_still_rejects_free_format() {
        let bytes = free_format_header_bytes(0b00, 0b00, 0);
        assert_eq!(FrameHeader::parse(&bytes), Err(HeaderError::FreeFormat));
    }

    #[test]
    fn measure_base_slots_from_two_syncwords() {
        // Build a free-format frame of 417 bytes (a 128 kbit/s-equivalent
        // base at 44.1 kHz: floor(144*128000/44100) = 417), unpadded,
        // followed by a second syncword.
        let mut buf = free_format_header_bytes(0b00, 0b00, 0).to_vec();
        buf.resize(417, 0xAB); // fill payload (avoid 0xFF runs that look like sync)
                               // append a second valid free-format header so find_sync hits it
        buf.extend_from_slice(&free_format_header_bytes(0b00, 0b00, 0));
        let base = measure_base_slots(&buf).expect("measure");
        assert_eq!(base, 417);
    }

    #[test]
    fn measure_base_slots_removes_current_padding() {
        // A padded current frame of 418 bytes → base N should be 417.
        let mut buf = free_format_header_bytes(0b00, 0b00, 1).to_vec();
        buf.resize(418, 0xAB);
        buf.extend_from_slice(&free_format_header_bytes(0b00, 0b00, 0));
        let base = measure_base_slots(&buf).expect("measure");
        assert_eq!(base, 417, "padding slot removed from base");
    }

    #[test]
    fn bitrate_recovery_maps_base_size_to_ladder() {
        let bytes = free_format_header_bytes(0b00, 0b00, 0); // 44.1k stereo
        let h = FrameHeader::parse_allow_free_format(&bytes).unwrap();
        // 417 base slots at 44.1 kHz == 128 kbit/s ladder rate.
        let br = bitrate_from_base_slots(&h, 417).expect("recover");
        assert_eq!(br, 128_000);
    }

    #[test]
    fn off_ladder_base_size_is_reported_not_guessed() {
        let bytes = free_format_header_bytes(0b00, 0b00, 0);
        let h = FrameHeader::parse_allow_free_format(&bytes).unwrap();
        // 500 base slots at 44.1 kHz matches no ladder bitrate.
        match bitrate_from_base_slots(&h, 500) {
            Err(FreeFormatError::UnsupportedBitrate {
                base_slots,
                sample_rate,
            }) => {
                assert_eq!(base_slots, 500);
                assert_eq!(sample_rate, 44_100);
            }
            other => panic!("expected UnsupportedBitrate, got {other:?}"),
        }
    }

    #[test]
    fn resolve_recovers_full_layout() {
        let mut buf = free_format_header_bytes(0b00, 0b00, 0).to_vec();
        buf.resize(417, 0xAB);
        buf.extend_from_slice(&free_format_header_bytes(0b00, 0b00, 0));
        let layout = resolve(&buf).expect("resolve");
        assert_eq!(layout.base_slots, 417);
        assert_eq!(layout.frame_size, 417);
        assert_eq!(layout.bit_rate, 128_000);
    }

    #[test]
    fn no_following_sync_is_reported() {
        let mut buf = free_format_header_bytes(0b00, 0b00, 0).to_vec();
        buf.resize(417, 0xAB); // no second syncword
        assert_eq!(
            measure_base_slots(&buf),
            Err(FreeFormatError::NoFollowingSync)
        );
    }

    #[test]
    fn header_with_recovered_bitrate_only_rewrites_bitrate() {
        let bytes = free_format_header_bytes(0b00, 0b00, 0);
        let h = FrameHeader::parse_allow_free_format(&bytes).unwrap();
        let filled = header_with_recovered_bitrate(&h, 128_000);
        assert_eq!(filled.bit_rate, 128_000);
        assert!(!filled.is_free_format());
        assert_eq!(filled.sample_rate, h.sample_rate);
        assert_eq!(filled.mode, h.mode);
        assert_eq!(filled.padding, h.padding);
        let _ = (Emphasis::None, ModeExtension::Bound4);
    }
}
