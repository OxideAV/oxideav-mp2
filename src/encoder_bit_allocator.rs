//! §C.1.5.2.7 encoder-side bit allocator (informative Annex C of
//! ISO/IEC 11172-3 (1993)).
//!
//! Annex C §C.1.5.2.7 describes the Layer II encoder's iterative
//! procedure for distributing the frame's available bits across the
//! per-(channel, sub-band) sample slots. The procedure runs after the
//! analysis filterbank ([`crate::AnalysisFilterbank`]) has produced
//! sub-band samples and after the psychoacoustic model has emitted a
//! signal-to-mask ratio (SMR, in dB) for each (channel, sub-band)
//! slot. It decides how many quantization steps `nb_steps[ch][sb]`
//! to spend on each slot, subject to the constraint that the bits
//! actually written do not exceed the frame's fixed length.
//!
//! The §C.1.5.2.7 algorithm is:
//!
//! 1. Compute `adb`, the available number of "data bits" for samples
//!    plus scalefactors:
//!
//!    ```text
//!    adb = cb − (bhdr + bcrc + bbal + bsel + bscf + bspl + banc)
//!    ```
//!
//!    where `cb` is the frame's total bit count (header + payload),
//!    `bhdr = 32` is the §2.4.1.3 header, `bcrc = 16` when
//!    `protection_bit == 0` and `0` otherwise, `bbal` is the §2.4.1.6
//!    bit-allocation section, `bsel` is the §2.4.1.6 scfsi section,
//!    `bscf` is the §2.4.1.6 scalefactor section, `bspl` is the
//!    §2.4.1.6 sample-codeword section, and `banc` is caller-supplied
//!    ancillary data.
//!
//! 2. Initialize every `nb_steps[ch][sb] = 0`. Compute the
//!    mask-to-noise ratio `MNR[ch][sb] = SNR[ch][sb] − SMR[ch][sb]`
//!    where `SNR[ch][sb]` is read from Table C.5 ("Layer II Signal-to-
//!    Noise Ratios") indexed by `nb_steps[ch][sb]` (and is `0.0 dB` at
//!    `nb_steps = 0`).
//!
//! 3. Iteratively step up the (ch, sb) slot with the lowest MNR by
//!    advancing its row in Table B.2 to the next higher `nb_steps`
//!    column. Recompute the marginal bit cost (Δbspl, Δbsel, Δbscf)
//!    and back the step out if it would push `adb` negative. Stop
//!    when no further step fits.
//!
//! ## Joint-stereo handling above `bound`
//!
//! §2.4.1.6 forces `allocation[1][sb] = allocation[0][sb]` for
//! sub-bands `sb ≥ bound` in `joint_stereo` mode. The allocator
//! enforces this by treating an above-`bound` sub-band as a single
//! "merged" (ch, sb) slot: only one MNR is tracked (the worst of the
//! two channels'), only one step is taken (both channels' `nb_steps`
//! move together), and the marginal sample-bit cost counts the
//! **single shared codeword** the §2.4.1.6 syntax puts on the wire
//! (§2.4.2.6 "the coded representation of the sample is valid for
//! both channels" — the frame writer emits one Annex G.1 sum-signal
//! triplet per granule above `bound`). Both channels still record an
//! independent scalefactor (and may select independent scfsi), so the
//! marginal scalefactor + scfsi cost counts both channels.
//!
//! ## Bit-cost-during-allocation policy
//!
//! Each (ch, sb) slot's scalefactor + scfsi cost depends on its scfsi
//! schedule (§2.4.3.3.2): 2 bits of scfsi + `{1, 2, 3} × 6` bits of
//! scalefactor, depending on the schedule. The scfsi schedule is
//! selected by [`crate::encoder_scfsi::select_scfsi`] *after* the
//! allocator has decided which slots carry a non-zero allocation, so
//! the allocator does not know the exact cost at the moment it
//! decides whether to spend a bit. The §C.1.5.2.7 prose
//! ("`bscf` has to be updated according to the number of scalefactors
//! required for this subband") is ambiguous on what to use as the
//! per-slot scalefactor cost at allocation time.
//!
//! This implementation budgets the **worst case** at allocation time:
//! 2 bits of scfsi + 3 × 6 = 18 bits of scalefactor = **20 bits** per
//! first-time non-zero (ch, sb) slot. The §C.1.5.2.5 selector later
//! reduces this to between 8 bits (`scfsi = '10'`) and 20 bits
//! (`scfsi = '00'`) per slot. The allocator's worst-case budget is
//! always at least as large as the actual cost, so the actual frame
//! never overruns `adb`; if anything the frame may have a few unused
//! bits at the end (which the §2.4.1.6 writer handles by writing the
//! `banc` ancillary section into the slack).
//!
//! ## Bit accounting (constant terms)
//!
//! For a given header:
//!
//! * `cb = 8 × frame_size_bytes`. The §2.4.3.1 formula
//!   `N = floor(144 × bitrate / Fs) + padding_bit` gives the byte
//!   count, multiplied by 8 for the bit count.
//! * `bhdr = 32` (§2.4.1.3).
//! * `bcrc ∈ {0, 16}` per `header.protection_bit`.
//! * `bbal = Σ_{sb<bound} channels × nbal(sb) + Σ_{sb in [bound, sblimit)} nbal(sb)`.
//!   The `bound .. sblimit` range collapses for non-joint-stereo
//!   (`bound == sblimit`) so the formula reduces to
//!   `channels × Σ_{sb<sblimit} nbal(sb)` there.
//!
//! The iterative section tracks the four mutable terms:
//!
//! * `bspl` — sample bits.
//! * `bsel` — scfsi bits (2 per non-zero (ch, sb), or 2 per merged slot
//!   doubled because both channels still carry an independent scfsi
//!   for the joint-stereo above-bound region).
//! * `bscf` — scalefactor bits (18 per non-zero (ch, sb)).
//! * `bspl` per-slot increment when `nb_steps` advances from `prev` to
//!   `next` along the row: `Δbspl = (36 / s_next) × bw_next −
//!   (36 / s_prev) × bw_prev` where `s_x` is `samples_per_codeword`
//!   (3 for grouped, 1 for ungrouped) and `bw_x` is `bits_per_codeword`
//!   per Table 3-B.4. The merged joint-stereo slot pays this once —
//!   one shared codeword is on the wire per §2.4.1.6 / §2.4.2.6.
//!
//! No external encoder or decoder source was consulted. The §C.1.5.2.7
//! procedure is the informative Annex C algorithm; the worst-case
//! scalefactor cost during allocation is a documented algorithmic
//! choice that satisfies the §C.1.5.2.7 "adb is not less than any
//! possible increase" termination rule.

use crate::audio_data::{AudioData, Scfsi, MAX_CHANNELS};
use crate::bitalloc::{
    bitrate_per_channel_kbps, class_of_quantization, select_table, BitAllocTable, NUM_SUBBANDS,
};
use crate::encoder_scalefactors::SUBBAND_SAMPLES_PER_FRAME;
use crate::header::{FrameHeader, Mode};

/// `bhdr` (§2.4.1.3 header bit count).
pub const HEADER_BITS: u32 = 32;

/// `bcrc` when `protection_bit == 0` (§2.4.1.4 CRC-16 slot).
pub const CRC_BITS: u32 = 16;

/// Per-slot scfsi bit cost (§2.4.3.3.2, fixed 2-bit field).
pub const SCFSI_BITS_PER_SLOT: u32 = 2;

/// Per-slot worst-case scalefactor bit cost (§2.4.1.7, 3 × 6 bits).
pub const WORST_CASE_SCALEFACTOR_BITS_PER_SLOT: u32 = 18;

/// Errors the encoder-side bit allocator can surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitAllocError {
    /// The `(Fs, bitrate, mode)` triple does not select any Layer II
    /// bit-allocation sub-table (i.e. the header is internally
    /// inconsistent).
    NoBitallocTable,
    /// The frame's constant bit budget — `bhdr + bcrc + bbal` — already
    /// exceeds `cb`. No data bits remain even before the iterative
    /// section runs. The triple `(cb, fixed, banc)` is returned for
    /// caller diagnostics.
    InsufficientFrameSize {
        /// Total frame size in bits (`8 × frame_size_bytes`).
        cb: u32,
        /// Sum `bhdr + bcrc + bbal` (the constant-budget portion).
        fixed: u32,
        /// Caller-supplied ancillary bit reservation (`banc`).
        banc: u32,
    },
}

impl core::fmt::Display for BitAllocError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BitAllocError::NoBitallocTable => {
                write!(
                    f,
                    "bit allocator: (Fs, bitrate, mode) does not select any Layer II sub-table"
                )
            }
            BitAllocError::InsufficientFrameSize { cb, fixed, banc } => {
                write!(
                    f,
                    "bit allocator: frame's constant bit budget {fixed} + ancillary {banc} > cb={cb}"
                )
            }
        }
    }
}

impl std::error::Error for BitAllocError {}

/// Table C.5 ("Layer II Signal-to-Noise Ratios", PDF page 76) — the
/// signal-to-noise ratio in dB for each Table 3-B.4 `nb_steps` value.
///
/// `nb_steps == 0` (the §2.4.2.3 "no bits allocated" sentinel) is
/// included with `0.0 dB`: with no bits, the quantizer adds no signal
/// content, so the SNR is conventionally taken as the no-encoding
/// reference of `0.0 dB`. This makes the §C.1.5.2.7 starting MNR
/// equal to `−SMR`, the bare signal-to-mask ratio.
///
/// Ordered by ascending `nb_steps`.
const SNR_TABLE: [(u32, f64); 18] = [
    (0, 0.00),
    (3, 7.00),
    (5, 11.00),
    (7, 16.00),
    (9, 20.84),
    (15, 25.28),
    (31, 31.59),
    (63, 37.75),
    (127, 43.84),
    (255, 49.89),
    (511, 55.93),
    (1023, 61.96),
    (2047, 67.98),
    (4095, 74.01),
    (8191, 80.03),
    (16383, 86.05),
    (32767, 92.01),
    (65535, 98.01),
];

/// Signal-to-noise ratio in dB for an `nb_steps` value, per Table C.5.
///
/// Returns `None` for any value not in the table (the §2.4.2.3 sentinel
/// `0` is in the table at `0.0 dB`).
pub fn snr_db(nb_steps: u32) -> Option<f64> {
    SNR_TABLE
        .iter()
        .find_map(|&(n, snr)| if n == nb_steps { Some(snr) } else { None })
    // PDF Table C.5 also tabulates the conventionally-redundant `0` step
    // at the top of the table; both decoder and allocator treat it as a
    // valid (zero-bit) slot.
}

/// Per-slot constant bit reservation breakdown. Returned by
/// [`fixed_bit_budget`] for callers that want to render an explicit
/// "available bits" line in their progress trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedBitBudget {
    /// `cb` — total frame size in bits (`8 × frame_size_bytes`).
    pub cb: u32,
    /// `bhdr` — header bits (32, §2.4.1.3).
    pub bhdr: u32,
    /// `bcrc` — CRC-16 slot (0 or 16, §2.4.1.4).
    pub bcrc: u32,
    /// `bbal` — total bit-allocation section across all (ch, sb) slots
    /// (§2.4.1.6, sum of `nbal[sb]` widths).
    pub bbal: u32,
    /// `sblimit` from the active sub-table.
    pub sblimit: usize,
    /// `bound` — `[0, bound)` get an `allocation[ch][sb]` per channel;
    /// `[bound, sblimit)` get a single shared `allocation[0][sb]` field.
    pub bound: usize,
    /// Number of channels (1 or 2, §2.2.6).
    pub channels: usize,
    /// The B.2 sub-table the (Fs, bitrate, mode) triple selected.
    pub table: BitAllocTable,
}

impl FixedBitBudget {
    /// Constant bits already spent before iterative allocation begins.
    /// (`bhdr + bcrc + bbal`.)
    pub fn fixed(&self) -> u32 {
        self.bhdr + self.bcrc + self.bbal
    }
}

/// Compute the §C.1.5.2.7 constant-budget terms (`bhdr`, `bcrc`,
/// `bbal`) and surface the (Fs, bitrate, mode) sub-table choice
/// alongside them.
///
/// Returns [`BitAllocError::NoBitallocTable`] if no Layer II sub-table
/// is selected (header internal inconsistency).
pub fn fixed_bit_budget(header: &FrameHeader) -> Result<FixedBitBudget, BitAllocError> {
    let table = select_table(header).ok_or(BitAllocError::NoBitallocTable)?;
    let channels = header.channels();
    let sblimit = table.sblimit();
    let bound = match header.mode {
        Mode::JointStereo => header.mode_extension.bound().min(sblimit),
        _ => sblimit,
    };

    let cb = (header.frame_size_bytes() as u32) * 8;
    let bhdr = HEADER_BITS;
    // §2.4.2.3: `protection_bit == false` means CRC-16 is present;
    // `true` means no CRC. The header type uses a bool for this.
    let bcrc = if !header.protection_bit { CRC_BITS } else { 0 };

    // §2.4.1.6: per-channel allocation in [0, bound); single shared
    // allocation in [bound, sblimit). Sub-bands at or above sblimit do
    // not carry an allocation field.
    let mut bbal: u32 = 0;
    for sb in 0..bound.min(sblimit) {
        bbal += (channels as u32) * table.nbal(sb);
    }
    for sb in bound..sblimit {
        bbal += table.nbal(sb);
    }

    // For `single_channel` (mode-resolved channels == 1) the bound is
    // forced to sblimit by `parse_audio_data_with_section_bits`, so the
    // joint-stereo branch never fires when channels == 1. The matching
    // header.channels() == 1 path through this function follows the
    // same shape: `bound == sblimit` and the `[bound, sblimit)` second
    // loop is empty.

    let _ = bitrate_per_channel_kbps(header); // sanity (unused, but
                                              // verifies the per-channel arithmetic select_table
                                              // already exercised).

    Ok(FixedBitBudget {
        cb,
        bhdr,
        bcrc,
        bbal,
        sblimit,
        bound,
        channels,
        table,
    })
}

/// Per-(channel, sub-band) signal-to-mask ratio in dB, the output of
/// the psychoacoustic model. Slots above `sblimit` are ignored by the
/// allocator (they carry no allocation field).
pub type SmrTable = [[f64; NUM_SUBBANDS]; MAX_CHANNELS];

/// Run the §C.1.5.2.7 iterative bit-allocator against `smr_db` and
/// emit an [`AudioData`] whose `nb_steps` field is populated.
///
/// `banc` is the caller's ancillary-data reservation (bits) — leaves
/// space for the §2.4.1.10 ancillary section; pass `0` if none is
/// required.
///
/// The returned `AudioData` has its `nb_steps` decided; the
/// `scfsi` field is left at [`Scfsi::ThreePerGranule`] (the default
/// value used as a placeholder by [`AudioData::new`]) and the
/// `scalefactor` field is left at zero. The caller fills both in
/// using [`crate::compute_scalefactors`] +
/// [`crate::select_scfsi`] before driving
/// [`crate::write_audio_data`].
#[allow(clippy::needless_range_loop)]
pub fn allocate_bits(
    header: &FrameHeader,
    smr_db: &SmrTable,
    banc: u32,
) -> Result<AudioData, BitAllocError> {
    let budget = fixed_bit_budget(header)?;
    let table = budget.table;
    let sblimit = budget.sblimit;
    let bound = budget.bound;
    let channels = budget.channels;

    let fixed = budget.fixed();
    if fixed + banc > budget.cb {
        return Err(BitAllocError::InsufficientFrameSize {
            cb: budget.cb,
            fixed,
            banc,
        });
    }
    let mut adb: i64 = budget.cb as i64 - (fixed + banc) as i64;

    // §C.1.5.2.7 mutable accumulators. We keep them in i64 for the
    // subtraction-against-adb safety.
    let mut bspl: i64 = 0;
    let mut bsel: i64 = 0;
    let mut bscf: i64 = 0;

    // Per-slot row position (index into `BitAllocTable::row(sb)`) and
    // the running `nb_steps` value. A `row_idx` of 0 corresponds to
    // the §2.4.2.3 "no allocation" sentinel; any positive index
    // corresponds to a real B.2 column.
    let mut row_idx = [[0u32; NUM_SUBBANDS]; MAX_CHANNELS];
    let mut nb_steps = [[0u32; NUM_SUBBANDS]; MAX_CHANNELS];

    // Per-slot MNR. Slots above sblimit are unused and never advance.
    let mut mnr = [[0.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
    for ch in 0..channels {
        for sb in 0..sblimit {
            mnr[ch][sb] = -smr_db[ch][sb];
        }
    }

    // §C.1.5.2.7: each iteration step picks the (ch, sb) with the
    // smallest MNR, advances its `nb_steps`, and re-prices it. Above
    // `bound` in joint-stereo mode both channels move together (they
    // share one allocation field); below `bound` (or in any
    // non-joint-stereo mode) each (ch, sb) moves independently.
    loop {
        // Pick the eligible slot with the smallest MNR.
        let mut best: Option<(usize, usize, f64)> = None;
        let mut consider = |ch: usize, sb: usize, m: f64| {
            // Eligibility: the slot must still have a higher Table B.2
            // column to step into.
            if row_idx[ch][sb] + 1 >= row_width(table, sb) {
                return;
            }
            match best {
                Some((_, _, bm)) if bm <= m => {}
                _ => best = Some((ch, sb, m)),
            }
        };
        for sb in 0..bound.min(sblimit) {
            for ch in 0..channels {
                consider(ch, sb, mnr[ch][sb]);
            }
        }
        for sb in bound..sblimit {
            // Above bound: treat the (ch=0, sb) slot as the
            // "representative", but compare against the *worse* of the
            // two channels' MNR so the joint allocation chases the
            // noisier channel.
            let m = if channels == 2 {
                mnr[0][sb].min(mnr[1][sb])
            } else {
                mnr[0][sb]
            };
            // Eligibility still tracks ch=0's row index; both channels
            // step together so ch=0 and ch=1 row indices stay in sync
            // for sb ≥ bound.
            if row_idx[0][sb] + 1 >= row_width(table, sb) {
                continue;
            }
            match best {
                Some((_, _, bm)) if bm <= m => {}
                _ => best = Some((0, sb, m)),
            }
        }

        let Some((ch, sb, _)) = best else { break };
        let merged = sb >= bound && channels == 2;

        let cur_row = row_idx[ch][sb];
        let next_row = cur_row + 1;
        let cur_nb = nb_steps[ch][sb];
        let next_nb = nb_steps_at(table, sb, next_row);

        // §C.1.5.2.7 marginal-cost calculation. For a merged
        // joint-stereo slot the sample cost is counted ONCE: §2.4.1.6
        // puts a single shared triplet on the wire for `sb >= bound`
        // (§2.4.2.6 "the coded representation of the sample is valid
        // for both channels"), which is exactly what the frame writer
        // emits (the Annex G.1 sum signal).
        let d_bspl = sample_bits_for(next_nb) as i64 - sample_bits_for(cur_nb) as i64;
        let mut d_bsel: i64 = 0;
        let mut d_bscf: i64 = 0;
        if cur_nb == 0 && next_nb != 0 {
            // First-time non-zero: budget worst-case scfsi + scalefactors.
            d_bsel += SCFSI_BITS_PER_SLOT as i64;
            d_bscf += WORST_CASE_SCALEFACTOR_BITS_PER_SLOT as i64;
            if merged {
                // Both channels record an independent scfsi + scalefactor
                // in the [bound, sblimit) intensity-stereo region per
                // §2.4.1.6 even though the allocation field is shared.
                d_bsel += SCFSI_BITS_PER_SLOT as i64;
                d_bscf += WORST_CASE_SCALEFACTOR_BITS_PER_SLOT as i64;
            }
        }
        let delta_total = d_bspl + d_bsel + d_bscf;

        if adb < delta_total {
            // No more bits — drop the slot from the eligibility pool by
            // forcing its eligibility check to fail next iteration. We
            // do that by marking the slot as "stuck": move its row_idx
            // to the top so the eligibility test `row_idx + 1 >= width`
            // fails. The MNR value remains as-is for traceability.
            let top = row_width(table, sb).saturating_sub(1);
            if merged {
                row_idx[0][sb] = top;
                row_idx[1][sb] = top;
            } else {
                row_idx[ch][sb] = top;
            }
            continue;
        }

        // Commit the step.
        bspl += d_bspl;
        bsel += d_bsel;
        bscf += d_bscf;
        adb -= delta_total;

        if merged {
            row_idx[0][sb] = next_row;
            row_idx[1][sb] = next_row;
            nb_steps[0][sb] = next_nb;
            nb_steps[1][sb] = next_nb;
            let new_snr = snr_db(next_nb).unwrap_or(0.0);
            // For the merged slot the per-channel MNR is updated against
            // each channel's own SMR — they share `next_nb` but their
            // (per-channel) MNR is what feeds the next worst-of-two
            // comparison.
            mnr[0][sb] = new_snr - smr_db[0][sb];
            mnr[1][sb] = new_snr - smr_db[1][sb];
        } else {
            row_idx[ch][sb] = next_row;
            nb_steps[ch][sb] = next_nb;
            let new_snr = snr_db(next_nb).unwrap_or(0.0);
            mnr[ch][sb] = new_snr - smr_db[ch][sb];
        }
    }

    let _ = (bspl, bsel, bscf); // accumulated for future tracing.

    // Materialize the result.
    let mut out = AudioData {
        table,
        sblimit,
        bound,
        channels,
        nb_steps: [[0; NUM_SUBBANDS]; MAX_CHANNELS],
        scfsi: [[Scfsi::ThreePerGranule; NUM_SUBBANDS]; MAX_CHANNELS],
        scalefactor: [[[0u8; 3]; NUM_SUBBANDS]; MAX_CHANNELS],
    };
    for ch in 0..channels {
        for sb in 0..sblimit {
            out.nb_steps[ch][sb] = nb_steps[ch][sb];
        }
    }
    Ok(out)
}

/// Per-(channel, sub-band) sample-bit cost for one frame, given the
/// `nb_steps` decision.
///
/// Returns `0` for the §2.4.2.3 sentinel `nb_steps == 0` and any
/// out-of-Table-B.4 value (which the allocator never advances into,
/// but defensive callers may invoke this directly).
///
/// For an ungrouped class (`grouping == false`): every sample carries
/// `bits_per_codeword` bits, so the per-frame cost is
/// `SUBBAND_SAMPLES_PER_FRAME × bits_per_codeword`.
///
/// For a grouped class (`grouping == true`, `nb_steps ∈ {3, 5, 9}`):
/// three samples pack into one `bits_per_codeword`-wide codeword, so
/// the per-frame cost is
/// `(SUBBAND_SAMPLES_PER_FRAME / 3) × bits_per_codeword`.
pub fn sample_bits_for(nb_steps: u32) -> u32 {
    let Some(class) = class_of_quantization(nb_steps) else {
        return 0;
    };
    let codewords = (SUBBAND_SAMPLES_PER_FRAME as u32) / class.samples_per_codeword;
    codewords * class.bits_per_codeword
}

/// Width of the `BitAllocTable::row(sb)` for a `(table, sb)` pair —
/// `1 << nbal(sb)`. Used to decide eligibility for further iteration.
fn row_width(table: BitAllocTable, sb: usize) -> u32 {
    let nbal = table.nbal(sb);
    if nbal == 0 {
        0
    } else {
        1u32 << nbal
    }
}

/// `nb_steps` at row position `row_idx` along `(table, sb)`. The
/// row-position-0 sentinel returns 0 (the §2.4.2.3 "no allocation"
/// value); positions `1..row_width` map to the tabulated `nb_steps`
/// values via [`BitAllocTable::nb_steps`].
fn nb_steps_at(table: BitAllocTable, sb: usize, row_idx: u32) -> u32 {
    if row_idx == 0 {
        0
    } else {
        table.nb_steps(sb, row_idx).unwrap_or(0)
    }
}

/// Cost in bits of quieting one slot to `MNR >= 0` against `smr`:
/// the cheapest Table B.2 row whose Table C.5 SNR reaches `smr`
/// (the top row when even that falls short), plus the worst-case
/// 20-bit scfsi + scalefactor overhead when a non-zero allocation is
/// required. `smr <= 0` costs nothing (`MNR = SNR − SMR >= 0` already
/// holds at `nb_steps = 0`).
fn slot_demand_bits(table: BitAllocTable, sb: usize, smr: f64) -> u64 {
    if smr <= 0.0 {
        return 0;
    }
    let width = row_width(table, sb);
    if width == 0 {
        return 0;
    }
    // Walk the row ladder to the first nb_steps whose SNR covers smr;
    // hold the top row if none does.
    let mut chosen = nb_steps_at(table, sb, width - 1);
    for row in 1..width {
        let nb = nb_steps_at(table, sb, row);
        if snr_db(nb).unwrap_or(0.0) >= smr {
            chosen = nb;
            break;
        }
    }
    if chosen == 0 {
        return 0;
    }
    u64::from(sample_bits_for(chosen))
        + u64::from(SCFSI_BITS_PER_SLOT + WORST_CASE_SCALEFACTOR_BITS_PER_SLOT)
}

/// Annex G.1 required-bitrate estimate: the number of §C.1.5.2.7 data
/// bits needed to bring **every** (channel, sub-band) slot to
/// `MNR >= 0` under `header`'s mode / `mode_extension` (i.e. its
/// `bound`), using the same cost model as [`allocate_bits`]:
///
/// * below `bound` each channel pays its own sample codewords and
///   scfsi/scalefactor overhead;
/// * in the `bound..sblimit` intensity region the merged slot pays ONE
///   shared codeword (§2.4.1.6 / §2.4.2.6) sized against the **more
///   demanding** of the two channels' SMRs ("the higher of the bit
///   allocations for left and right channel is used", Annex G.1), plus
///   both channels' scfsi/scalefactor overhead.
///
/// Compare against [`available_data_bits`] to drive the Annex G.1
/// decision "if the required bitrate exceeds the available bitrate,
/// […] set a number of subbands to intensity stereo mode".
#[allow(clippy::needless_range_loop)] // sb/ch index two parallel tables
pub fn demand_bits(header: &FrameHeader, smr_db: &SmrTable) -> Result<u64, BitAllocError> {
    let budget = fixed_bit_budget(header)?;
    let table = budget.table;
    let mut demand = 0u64;
    for sb in 0..budget.bound.min(budget.sblimit) {
        for ch in 0..budget.channels {
            demand += slot_demand_bits(table, sb, smr_db[ch][sb]);
        }
    }
    for sb in budget.bound..budget.sblimit {
        let smr = if budget.channels == 2 {
            smr_db[0][sb].max(smr_db[1][sb])
        } else {
            smr_db[0][sb]
        };
        let d = slot_demand_bits(table, sb, smr);
        if d > 0 && budget.channels == 2 {
            // One shared codeword + the SECOND channel's scfsi +
            // scalefactor overhead (the first channel's is already in
            // `d`).
            demand += d + u64::from(SCFSI_BITS_PER_SLOT + WORST_CASE_SCALEFACTOR_BITS_PER_SLOT);
        } else {
            demand += d;
        }
    }
    Ok(demand)
}

/// The §C.1.5.2.7 `adb` for `header`: data bits available for samples
/// and scalefactors after the constant terms (`bhdr + bcrc + bbal`)
/// and the caller's `banc` ancillary reservation.
///
/// Negative when the constant terms alone exceed the frame (the same
/// condition [`allocate_bits`] rejects as
/// [`BitAllocError::InsufficientFrameSize`]).
pub fn available_data_bits(header: &FrameHeader, banc: u32) -> Result<i64, BitAllocError> {
    let budget = fixed_bit_budget(header)?;
    Ok(i64::from(budget.cb) - i64::from(budget.fixed()) - i64::from(banc))
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::header::{Emphasis, FrameHeader, Mode, ModeExtension};

    fn canonical_header() -> FrameHeader {
        // 192 kbit/s / 44.1 kHz / Stereo / no-CRC, the staged fixture's
        // header. The bit-exact 4-byte big-endian rendering of these
        // fields would round-trip through `FrameHeader::parse`.
        FrameHeader {
            lsf: false,
            protection_bit: true, // true == "no CRC" per §2.4.2.3 inverted convention
            bit_rate: 192_000,
            sample_rate: 44_100,
            padding: false,
            private_bit: false,
            mode: Mode::Stereo,
            mode_extension: ModeExtension::Bound4,
            copyright: false,
            original: true,
            emphasis: Emphasis::None,
        }
    }

    #[test]
    fn snr_table_matches_pdf_landmarks() {
        // Spot-check PDF page 76 entries.
        assert_eq!(snr_db(0), Some(0.00));
        assert_eq!(snr_db(3), Some(7.00));
        assert_eq!(snr_db(9), Some(20.84));
        assert_eq!(snr_db(63), Some(37.75));
        assert_eq!(snr_db(1023), Some(61.96));
        assert_eq!(snr_db(65535), Some(98.01));
    }

    #[test]
    fn snr_table_is_strictly_increasing() {
        // Adding more steps cannot *decrease* SNR — the §C.1.5.2.7
        // monotonicity invariant the iterative loop relies on.
        for w in SNR_TABLE.windows(2) {
            assert!(w[0].1 < w[1].1, "{:?} -> {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn snr_table_includes_only_table_c5_steps() {
        // Every Table 3-B.4 class must appear in Table C.5 (the
        // allocator otherwise can't compute a marginal SNR).
        for &n in &[
            3u32, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1023, 2047, 4095, 8191, 16383, 32767, 65535,
        ] {
            assert!(
                snr_db(n).is_some(),
                "missing Table C.5 entry for nb_steps={n}"
            );
        }
        // The sentinel 0 is also tabulated.
        assert_eq!(snr_db(0), Some(0.0));
        // A non-tabulated value returns None.
        assert_eq!(snr_db(4), None);
    }

    #[test]
    fn sample_bits_for_grouped_and_ungrouped() {
        // Grouped (3 samples per codeword, nb_steps ∈ {3, 5, 9}): 36
        // samples / 3 = 12 codewords.
        assert_eq!(sample_bits_for(3), 12 * 5);
        assert_eq!(sample_bits_for(5), 12 * 7);
        assert_eq!(sample_bits_for(9), 12 * 10);
        // Ungrouped: 36 codewords (one per sample).
        assert_eq!(sample_bits_for(7), 36 * 3);
        assert_eq!(sample_bits_for(15), 36 * 4);
        assert_eq!(sample_bits_for(65535), 36 * 16);
        // Sentinel and off-table values return 0.
        assert_eq!(sample_bits_for(0), 0);
        assert_eq!(sample_bits_for(4), 0);
    }

    #[test]
    fn fixed_budget_canonical_192k_stereo() {
        // 192 kbit/s / 44.1 kHz / no-pad → 626 bytes → 5008 bits;
        // protection_bit=true means no CRC.
        // 192k/44.1k stereo → 96 kbit/s per channel → table B.2b
        // (sblimit=30, sum-of-nbal=94). channels=2, bound=sblimit=30
        // (non-joint), so bbal = 2 × 94 = 188.
        let h = canonical_header();
        let b = fixed_bit_budget(&h).unwrap();
        assert_eq!(b.cb, 626 * 8);
        assert_eq!(b.bhdr, 32);
        assert_eq!(b.bcrc, 0);
        assert_eq!(b.table, BitAllocTable::B2b);
        assert_eq!(b.sblimit, 30);
        assert_eq!(b.bound, 30);
        assert_eq!(b.channels, 2);
        assert_eq!(b.bbal, 188);
        assert_eq!(b.fixed(), 32 + 188);
    }

    #[test]
    fn fixed_budget_joint_stereo_shares_above_bound() {
        // 192k stereo header, override mode to JointStereo with
        // bound=4 (mode_extension = '00').
        let mut h = canonical_header();
        h.mode = Mode::JointStereo;
        h.mode_extension = ModeExtension::Bound4;
        let b = fixed_bit_budget(&h).unwrap();
        assert_eq!(b.table, BitAllocTable::B2b);
        assert_eq!(b.sblimit, 30);
        assert_eq!(b.bound, 4);
        assert_eq!(b.channels, 2);
        // bbal = 2 × Σ_{sb<4} nbal(sb) + Σ_{sb in [4, 30)} nbal(sb)
        // B.2b: sb 0..=2 nbal=4, sb 3..=10 nbal=4, sb 11..=22 nbal=3,
        //       sb 23..=29 nbal=2.
        // [0..4) widths: 4,4,4,4 = 16; doubled = 32.
        // [4..30) widths: 7×4 + 12×3 + 7×2 = 28 + 36 + 14 = 78.
        assert_eq!(b.bbal, 32 + 78);
    }

    #[test]
    fn fixed_budget_single_channel_bound_is_sblimit() {
        let mut h = canonical_header();
        // single_channel at 192 kbit/s is disallowed by the matrix; use
        // 80 kbit/s (allowed for single_channel) to stay legal.
        h.bit_rate = 80_000;
        h.mode = Mode::SingleChannel;
        h.mode_extension = ModeExtension::Bound4;
        let b = fixed_bit_budget(&h).unwrap();
        // 80 kbit/s single = 80 kbit/s per ch → B.2a (sblimit=27,
        // sum-of-nbal=88).
        assert_eq!(b.table, BitAllocTable::B2a);
        assert_eq!(b.channels, 1);
        assert_eq!(b.bound, 27);
        assert_eq!(b.bbal, 88); // 1 × 88
    }

    #[test]
    fn allocator_respects_budget_under_uniform_smr() {
        // §C.1.5.2.7 terminates on bit budget, not on a perceptual
        // threshold — so even with uniformly negative SMR (the mask is
        // well above the noise floor everywhere) the loop continues
        // until adb falls below the smallest possible marginal cost.
        // The invariant under test is the budget: the allocator never
        // overspends, regardless of SMR.
        let h = canonical_header();
        let smr = [[-1000.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        let data = allocate_bits(&h, &smr, 0).unwrap();
        let budget = fixed_bit_budget(&h).unwrap();
        let mut spent: u32 = 0;
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                let nb = data.nb_steps[ch][sb];
                spent += sample_bits_for(nb);
                if nb != 0 {
                    spent += SCFSI_BITS_PER_SLOT + WORST_CASE_SCALEFACTOR_BITS_PER_SLOT;
                }
            }
        }
        let available = budget.cb - budget.fixed();
        assert!(
            spent <= available,
            "allocator overspent budget: spent={spent} > available={available}"
        );
    }

    #[test]
    fn allocator_with_uniform_high_smr_respects_adb() {
        let h = canonical_header();
        // Very high SMR: every slot wants as many bits as possible.
        let smr = [[100.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        let data = allocate_bits(&h, &smr, 0).unwrap();
        let budget = fixed_bit_budget(&h).unwrap();
        let mut spent: u32 = 0;
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                let nb = data.nb_steps[ch][sb];
                spent += sample_bits_for(nb);
                if nb != 0 {
                    spent += SCFSI_BITS_PER_SLOT + WORST_CASE_SCALEFACTOR_BITS_PER_SLOT;
                }
            }
        }
        let available = budget.cb - budget.fixed();
        assert!(
            spent <= available,
            "allocator overspent budget: spent={spent} > available={available}"
        );
        // And at least one slot got bits (otherwise the allocator is
        // broken).
        let mut any_nonzero = false;
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                if data.nb_steps[ch][sb] != 0 {
                    any_nonzero = true;
                }
            }
        }
        assert!(
            any_nonzero,
            "allocator failed to spend any bits at high SMR"
        );
    }

    #[test]
    fn allocator_prioritises_high_smr_slots() {
        let h = canonical_header();
        // One sub-band has a much higher SMR than every other slot.
        // §C.1.5.2.7 picks the lowest-MNR slot each iteration; the
        // high-SMR slot starts with the lowest MNR (−SMR is most
        // negative) and will be stepped first. Every step lifts its
        // MNR by the SNR delta; once it overtakes the next-lowest
        // slot, that one starts taking steps too. So the property is
        // not "only the high slot gets bits" but rather "the high
        // slot's nb_steps is the largest in the frame."
        let mut smr = [[0.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        smr[0][5] = 100.0;
        let data = allocate_bits(&h, &smr, 0).unwrap();
        let target = data.nb_steps[0][5];
        assert!(target != 0, "high-SMR slot (ch=0, sb=5) must receive bits");
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                if (ch, sb) != (0, 5) {
                    assert!(
                        data.nb_steps[ch][sb] <= target,
                        "high-SMR slot ch=0 sb=5 nb_steps={target} \
                         must dominate ch={ch} sb={sb} nb_steps={}",
                        data.nb_steps[ch][sb]
                    );
                }
            }
        }
    }

    #[test]
    fn allocator_emits_valid_b2_column_values() {
        let h = canonical_header();
        let mut smr = [[0.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        for ch in 0..2 {
            for sb in 0..NUM_SUBBANDS {
                // Mild positive SMR — drives partial allocation.
                smr[ch][sb] = 20.0;
            }
        }
        let data = allocate_bits(&h, &smr, 0).unwrap();
        let table = data.table;
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                let nb = data.nb_steps[ch][sb];
                // The chosen nb_steps must be reachable by a B.2 row
                // index (the encoder writer requires this).
                if nb == 0 {
                    // Sentinel — always reachable as row index 0.
                } else {
                    assert!(
                        table.allocation_index(sb, nb).is_some(),
                        "ch={ch} sb={sb}: nb_steps={nb} is not in table {:?}'s row",
                        table
                    );
                }
            }
        }
    }

    #[test]
    fn allocator_joint_stereo_shares_nb_steps_above_bound() {
        // JointStereo with bound=4 (B.2b, sblimit=30 → [4, 30) shared).
        let mut h = canonical_header();
        h.mode = Mode::JointStereo;
        h.mode_extension = ModeExtension::Bound4;
        // Asymmetric SMR: ch=0 high, ch=1 low. Above bound the
        // allocation must be shared, so both channels' nb_steps[sb]
        // must match for sb in [bound, sblimit).
        let mut smr = [[-100.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        for sb in 0..NUM_SUBBANDS {
            smr[0][sb] = 30.0;
            smr[1][sb] = -10.0;
        }
        let data = allocate_bits(&h, &smr, 0).unwrap();
        for sb in data.bound..data.sblimit {
            assert_eq!(
                data.nb_steps[0][sb], data.nb_steps[1][sb],
                "joint-stereo above-bound sb={sb} must share nb_steps: ch0={} vs ch1={}",
                data.nb_steps[0][sb], data.nb_steps[1][sb]
            );
        }
    }

    #[test]
    fn allocator_insufficient_frame_size_reports_error() {
        // No real Layer II header is short enough to trigger this in
        // practice, but the `banc` parameter lets us simulate an
        // ancillary reservation larger than the available bits.
        let h = canonical_header();
        let budget = fixed_bit_budget(&h).unwrap();
        let banc = budget.cb; // reserve the entire frame for ancillary.
        let smr = [[0.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        let err = allocate_bits(&h, &smr, banc).unwrap_err();
        match err {
            BitAllocError::InsufficientFrameSize { cb, fixed, banc: b } => {
                assert_eq!(cb, budget.cb);
                assert_eq!(fixed, budget.fixed());
                assert_eq!(b, banc);
            }
            other => panic!("expected InsufficientFrameSize, got {other:?}"),
        }
    }

    #[test]
    fn allocator_output_writes_through_audio_data_writer() {
        // End-to-end: allocate, fill in scfsi + scalefactors, run
        // write_audio_data, then read it back via parse_audio_data
        // and verify the nb_steps match.
        use crate::audio_data::{parse_audio_data, write_audio_data};
        use oxideav_core::bits::{BitReader, BitWriter};

        let h = canonical_header();
        let mut smr = [[0.0f64; NUM_SUBBANDS]; MAX_CHANNELS];
        for ch in 0..2 {
            for sb in 0..NUM_SUBBANDS {
                smr[ch][sb] = 30.0;
            }
        }
        let mut data = allocate_bits(&h, &smr, 0).unwrap();
        // Fill in scalefactor + scfsi defaults so the writer accepts.
        // Index 0 is a valid scalefactor; ThreePerGranule means all
        // three on-wire 6-bit slots are written.
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                if data.nb_steps[ch][sb] != 0 {
                    data.scfsi[ch][sb] = Scfsi::ThreePerGranule;
                    data.scalefactor[ch][sb] = [0, 0, 0];
                }
            }
        }

        let mut bw = BitWriter::new();
        write_audio_data(&h, &data, &mut bw).unwrap();
        bw.align_to_byte_zero();
        let buf = bw.finish();

        let mut br = BitReader::new(&buf);
        let parsed = parse_audio_data(&h, &mut br).unwrap();
        for ch in 0..data.channels {
            for sb in 0..data.sblimit {
                assert_eq!(
                    parsed.nb_steps[ch][sb], data.nb_steps[ch][sb],
                    "round-trip mismatch at ch={ch} sb={sb}"
                );
            }
        }
    }
    // ---- Annex G.1 demand estimation ----

    #[test]
    fn demand_bits_is_zero_for_non_positive_smr() {
        // MNR = SNR − SMR >= 0 already holds at nb_steps = 0 when
        // SMR <= 0, so no slot demands anything.
        let h = canonical_header();
        let smr: SmrTable = [[0.0; NUM_SUBBANDS]; MAX_CHANNELS];
        assert_eq!(demand_bits(&h, &smr).unwrap(), 0);
        let smr_neg: SmrTable = [[-25.0; NUM_SUBBANDS]; MAX_CHANNELS];
        assert_eq!(demand_bits(&h, &smr_neg).unwrap(), 0);
    }

    #[test]
    fn demand_bits_single_slot_exact() {
        // One below-bound slot demanding SNR >= 15 dB: the cheapest
        // Table B.2 row whose Table C.5 SNR reaches 15 dB, plus the
        // 20-bit worst-case scfsi + scalefactor overhead.
        let h = canonical_header();
        let mut smr: SmrTable = [[0.0; NUM_SUBBANDS]; MAX_CHANNELS];
        smr[0][3] = 15.0;
        let budget = fixed_bit_budget(&h).unwrap();
        // Independently find the cheapest covering row for sb = 3.
        let mut expect_nb = 0;
        for row in 1..row_width(budget.table, 3) {
            let nb = nb_steps_at(budget.table, 3, row);
            if snr_db(nb).unwrap_or(0.0) >= 15.0 {
                expect_nb = nb;
                break;
            }
        }
        assert!(expect_nb > 0, "test premise: some row covers 15 dB");
        let expect = u64::from(sample_bits_for(expect_nb))
            + u64::from(SCFSI_BITS_PER_SLOT + WORST_CASE_SCALEFACTOR_BITS_PER_SLOT);
        assert_eq!(demand_bits(&h, &smr).unwrap(), expect);
    }

    #[test]
    fn demand_bits_decreases_as_the_intensity_bound_narrows() {
        // For a symmetric positive SMR, every subband moved from the
        // per-channel region into the intensity region saves one
        // channel's sample codewords (the shared codeword is on the
        // wire once), so demand is monotonically non-increasing along
        // Stereo -> Bound16 -> Bound12 -> Bound8 -> Bound4.
        let smr: SmrTable = [[30.0; NUM_SUBBANDS]; MAX_CHANNELS];
        let mut h = canonical_header();
        let mut prev = None;
        let candidates = [
            (Mode::Stereo, ModeExtension::Bound4),
            (Mode::JointStereo, ModeExtension::Bound16),
            (Mode::JointStereo, ModeExtension::Bound12),
            (Mode::JointStereo, ModeExtension::Bound8),
            (Mode::JointStereo, ModeExtension::Bound4),
        ];
        for (mode, ext) in candidates {
            h.mode = mode;
            h.mode_extension = ext;
            let d = demand_bits(&h, &smr).unwrap();
            assert!(d > 0, "flat 30 dB SMR demands bits ({mode:?} {ext:?})");
            if let Some(p) = prev {
                assert!(
                    d <= p,
                    "demand must not grow as the bound narrows ({mode:?} {ext:?}: {d} > {p})"
                );
            }
            prev = Some(d);
        }
    }

    #[test]
    fn available_data_bits_matches_the_fixed_budget_identity() {
        let h = canonical_header();
        let budget = fixed_bit_budget(&h).unwrap();
        assert_eq!(
            available_data_bits(&h, 0).unwrap(),
            i64::from(budget.cb) - i64::from(budget.fixed())
        );
        assert_eq!(
            available_data_bits(&h, 100).unwrap(),
            i64::from(budget.cb) - i64::from(budget.fixed()) - 100
        );
    }
}
