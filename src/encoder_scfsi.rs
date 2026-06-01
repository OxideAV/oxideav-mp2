//! §C.1.5.2.5 / §C.1.5.2.6 encoder-side scalefactor-selection-information
//! (SCFSI) selection — Annex C "Coding of scalefactors" + "Coding of
//! scalefactor selection information".
//!
//! ISO/IEC 11172-3 (1993) §C.1.5.2.5 describes the Layer II encoder's
//! procedure for *deciding which of the three per-frame scalefactors to
//! transmit*. Each subband produces three Table 3-B.1 indices per frame
//! (one per scalefactor-granule of 12 sub-band samples — see
//! [`crate::encoder_scalefactors`]); the §C.1.5.2.5 procedure classifies
//! the two successive differences `dscf1 = scf1 - scf2` and
//! `dscf2 = scf2 - scf3` into one of five classes and indexes Table C.4
//! ("Layer II scalefactor transmission patterns", PDF page 76). Table
//! C.4 then prescribes (a) which actual scalefactor values to use after
//! the encoder's "adjusting" step, (b) which of those values are
//! transmitted (the *transmission pattern* — 1, 2, or 3 of the three
//! granule slots), and (c) the 2-bit `scfsi[ch][sb]` code that the
//! decoder reads back per §2.4.3.3.2.
//!
//! The class-of-differences mapping (PDF page 73, §C.1.5.2.5
//! "The class of each of the differences is determined as follows:"):
//!
//! | class | dscf range          |
//! |-------|---------------------|
//! |   1   | `dscf <= -3`        |
//! |   2   | `-3 <  dscf <  0`   |
//! |   3   | `dscf == 0`         |
//! |   4   | `0   <  dscf <  3`  |
//! |   5   | `dscf >= 3`         |
//!
//! Table C.4 (PDF page 76):
//!
//! | (cls1,cls2) | used    | pattern | scfsi |
//! |-------------|---------|---------|-------|
//! | (1,1)       | 1 2 3   | 1 2 3   |  00   |
//! | (1,2)       | 1 2 2   | 1 2 .   |  11   |
//! | (1,3)       | 1 2 2   | 1 2 .   |  11   |
//! | (1,4)       | 1 3 3   | 1 . 3   |  11   |
//! | (1,5)       | 1 2 3   | 1 2 3   |  00   |
//! | (2,1)       | 1 1 3   | 1 . 3   |  01   |
//! | (2,2)       | 1 1 1   | . . 1   |  10   |
//! | (2,3)       | 1 1 1   | . . 1   |  10   |
//! | (2,4)       | 4 4 4   | . . 4   |  10   |
//! | (2,5)       | 1 1 3   | 1 . 3   |  01   |
//! | (3,1)       | 1 1 1   | . . 1   |  10   |
//! | (3,2)       | 1 1 1   | . . 1   |  10   |
//! | (3,3)       | 1 1 1   | . . 1   |  10   |
//! | (3,4)       | 3 3 3   | . . 3   |  10   |
//! | (3,5)       | 1 1 3   | 1 . 3   |  01   |
//! | (4,1)       | 2 2 2   | . . 2   |  10   |
//! | (4,2)       | 2 2 2   | . . 2   |  10   |
//! | (4,3)       | 2 2 2   | . . 2   |  10   |
//! | (4,4)       | 3 3 3   | . . 3   |  10   |
//! | (4,5)       | 1 2 3   | 1 2 3   |  00   |
//! | (5,1)       | 1 2 3   | 1 2 3   |  00   |
//! | (5,2)       | 1 2 2   | 1 2 .   |  11   |
//! | (5,3)       | 1 2 2   | 1 2 .   |  11   |
//! | (5,4)       | 1 3 3   | 1 . 3   |  11   |
//! | (5,5)       | 1 2 3   | 1 2 3   |  00   |
//!
//! Per §C.1.5.2.5: column "scalefactor used in encoder" labels of `1`,
//! `2`, `3` mean the first, second and third granule's scalefactor; `4`
//! means *the maximum of the three scalefactors*. In Table 3-B.1, larger
//! multipliers correspond to *smaller indices* (entry 0 is the largest,
//! entry 62 the smallest), so "maximum scalefactor" maps to *minimum
//! index*. The resulting `used` triple is then narrowed to the
//! `transmission pattern` set of slots whose 6-bit Table 3-B.1 indices
//! the encoder physically writes to the bitstream (the decoder
//! re-expands across granules per the 2-bit `scfsi` code per
//! §2.4.3.3.3).
//!
//! Clean-room: this module reads only ISO/IEC 11172-3 (1993) §C.1.5.2.5,
//! §C.1.5.2.6, and Table C.4 (PDF pages 73 + 76). No third-party MP2
//! encoder source was consulted; the procedure is the documented
//! Annex C "possible Layer II encoding method" (§C.1.5.2.1
//! "This clause describes a possible Layer II encoding method").
//!
//! ## How this fits into the encoder
//!
//! Per frame per (channel, subband) with a non-zero allocation:
//!
//! 1. [`crate::encoder_scalefactors::compute_scalefactors`] computes
//!    three Table 3-B.1 indices `[scf1, scf2, scf3]` (one per
//!    scalefactor-granule).
//! 2. [`select_scfsi`] is called with the triple and returns a
//!    [`ScfsiSelection`] carrying: the adjusted `used` triple the
//!    decoder will reconstruct via the §2.4.3.3.3 schedule, the
//!    transmission pattern (which slots of `used` to write to the
//!    bitstream), and the 2-bit `scfsi` code that selects the
//!    matching [`crate::audio_data::Scfsi`] decode schedule.
//! 3. The yet-to-be-built §2.4.1.6 audio-data writer writes the 2-bit
//!    `scfsi.code()` and then exactly `pattern.transmitted_count()`
//!    6-bit scalefactor indices read off `used[pattern.transmitted_*]`.
//!
//! The end-to-end self-consistency check is the *wire round-trip*: the
//! `used` triple the encoder claims the decoder will reconstruct equals
//! what the §2.4.3.3.2 / §2.4.3.3.3 [`Scfsi`] schedule actually
//! reconstructs from the bits the encoder physically writes. Checked
//! by [`tests::scfsi_pattern_round_trips_to_used_triple`].

use crate::audio_data::Scfsi;

/// One of the five classes the §C.1.5.2.5 "class of differences"
/// procedure assigns to `dscf1` and `dscf2`.
///
/// The classes are 1-based in the spec; we expose them as 1..=5 to keep
/// the table cross-references obvious. [`classify_difference`] is the
/// canonical entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DifferenceClass {
    /// `dscf <= -3` (the next granule's index drops by 3+, i.e. its
    /// multiplier *grows* substantially since Table 3-B.1 is decreasing).
    Class1 = 1,
    /// `-3 < dscf < 0` — a smaller decrease in index (= modest growth
    /// in multiplier).
    Class2 = 2,
    /// `dscf == 0` — identical successive scalefactor.
    Class3 = 3,
    /// `0 < dscf < 3` — a smaller increase in index (= modest decline
    /// in multiplier).
    Class4 = 4,
    /// `dscf >= 3` (the next granule's index rises by 3+, i.e. its
    /// multiplier drops substantially).
    Class5 = 5,
}

impl DifferenceClass {
    /// The 1-based class number used by Table C.4.
    pub fn as_index(self) -> u8 {
        self as u8
    }
}

/// Classify one §C.1.5.2.5 difference `dscf = scf_n - scf_{n+1}`.
///
/// The inputs are i16 because the spec's classification uses signed
/// arithmetic. Conformant inputs come from u8 scalefactor indices in
/// `[0, 62]`, so the difference lies in `[-62, 62]` and fits in i16
/// with room to spare; we accept any i16 for completeness.
pub fn classify_difference(dscf: i16) -> DifferenceClass {
    if dscf <= -3 {
        DifferenceClass::Class1
    } else if dscf < 0 {
        DifferenceClass::Class2
    } else if dscf == 0 {
        DifferenceClass::Class3
    } else if dscf < 3 {
        DifferenceClass::Class4
    } else {
        DifferenceClass::Class5
    }
}

/// Which of the three granule slots a Table C.4 row prescribes to
/// physically write to the bitstream.
///
/// The "transmission pattern" column of Table C.4 lists the slots that
/// remain after the encoder's "adjusting" step has identified the
/// equal-value runs. Per §C.1.5.2.5: "If, after this adjusting of
/// scalefactors two or three are the same, not all scalefactors need to
/// be transmitted." A slot's truth value here mirrors the column: `true`
/// means the corresponding `used` slot is written; `false` means it is
/// reconstructed by the decoder per the §2.4.3.3.3 [`Scfsi`] schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransmissionPattern {
    /// Whether `used[0]` is physically written to the bitstream.
    pub write0: bool,
    /// Whether `used[1]` is physically written.
    pub write1: bool,
    /// Whether `used[2]` is physically written.
    pub write2: bool,
}

impl TransmissionPattern {
    /// How many of the three slots are physically written
    /// (the wire-side count, 1..=3).
    pub fn transmitted_count(self) -> usize {
        usize::from(self.write0) + usize::from(self.write1) + usize::from(self.write2)
    }
}

/// A complete §C.1.5.2.5 / §C.1.5.2.6 SCFSI selection for one
/// `(channel, subband)` slot of a Layer II frame.
///
/// Returned by [`select_scfsi`]; consumed by the §2.4.1.6 audio-data
/// writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScfsiSelection {
    /// The §C.1.5.2.5 "scalefactor used in encoder" triple: the three
    /// Table 3-B.1 indices the *decoder* will reconstruct after applying
    /// the §2.4.3.3.3 schedule. May differ from the input scalefactors
    /// when Table C.4 prescribes adjustment (`4 4 4` row, repetitions).
    pub used: [u8; 3],
    /// Which of the three `used` slots are physically written to the
    /// bitstream (per the Table C.4 "transmission pattern" column).
    pub pattern: TransmissionPattern,
    /// The 2-bit §2.4.3.3.2 `scfsi[ch][sb]` schedule the decoder reads
    /// back. The decoder uses this to re-expand the transmitted
    /// scalefactor indices into the three-granule reconstruction.
    pub scfsi: Scfsi,
    /// The two class assignments `(class(dscf1), class(dscf2))` — for
    /// diagnostics and exhaustive testing only; the audio-data writer
    /// never consumes them.
    pub classes: (DifferenceClass, DifferenceClass),
}

/// Pick the §C.1.5.2.5 / §C.1.5.2.6 SCFSI selection for one
/// `(channel, subband)` slot.
///
/// `scalefactors` is the per-granule Table 3-B.1 index triple
/// `[scf1, scf2, scf3]` produced by
/// [`crate::encoder_scalefactors::compute_scalefactors`] (each entry in
/// `0..=62`). Returns the full [`ScfsiSelection`] — the adjusted `used`
/// triple, the transmission pattern, and the 2-bit `scfsi` code.
///
/// # Mapping
///
/// * Compute `dscf1 = scf1 - scf2` and `dscf2 = scf2 - scf3`
///   (§C.1.5.2.5).
/// * Classify each via [`classify_difference`] (§C.1.5.2.5 class table,
///   PDF page 73).
/// * Index Table C.4 (PDF page 76) by the resulting `(class1, class2)`
///   pair to obtain the `used` recipe, the transmission pattern, and
///   the 2-bit `scfsi` code.
///
/// # Notes
///
/// * Per §C.1.5.2.5 "4 means the maximum of the three scalefactors":
///   Table 3-B.1 is monotonically *decreasing* (entry 0 = 2.0; entry 62
///   ≈ 1.2e-6), so the *maximum multiplier* corresponds to the
///   *minimum index*. The (2, 4) row's `used = [4, 4, 4]` recipe
///   therefore writes `min(scf1, scf2, scf3)` into all three slots.
/// * Inputs are not range-checked: any `u8` is accepted (the
///   §C.1.5.2.6 procedure operates on the index arithmetic, not on the
///   multiplier values), but conformant input lies in `[0, 62]`.
pub fn select_scfsi(scalefactors: [u8; 3]) -> ScfsiSelection {
    let [scf1, scf2, scf3] = scalefactors;
    let dscf1 = i16::from(scf1) - i16::from(scf2);
    let dscf2 = i16::from(scf2) - i16::from(scf3);
    let class1 = classify_difference(dscf1);
    let class2 = classify_difference(dscf2);
    apply_table_c4(class1, class2, scalefactors)
}

/// The Table C.4 lookup, factored out so the tests can exercise the
/// row mapping directly.
fn apply_table_c4(
    class1: DifferenceClass,
    class2: DifferenceClass,
    scalefactors: [u8; 3],
) -> ScfsiSelection {
    use DifferenceClass::*;

    // The "max scalefactor" per §C.1.5.2.5 — since Table 3-B.1 is
    // monotonically decreasing, the largest multiplier is the smallest
    // index.
    let max_scf = scalefactors.iter().copied().min().unwrap_or(0);

    // Encode the table as a single match; each arm names the (used,
    // pattern, scfsi) triple per Table C.4 row.
    let (used, pattern, scfsi) = match (class1, class2) {
        // Row block (class1 = 1): 1,1 -> all three; 1,5 -> all three;
        // 1,2 / 1,3 -> first two; 1,4 -> first + third.
        (Class1, Class1) => (
            [scalefactors[0], scalefactors[1], scalefactors[2]],
            TX_ALL,
            Scfsi::ThreePerGranule,
        ),
        (Class1, Class2) | (Class1, Class3) => (
            [scalefactors[0], scalefactors[1], scalefactors[1]],
            TX_FIRST_SECOND,
            Scfsi::Share0Then12,
        ),
        (Class1, Class4) => (
            [scalefactors[0], scalefactors[2], scalefactors[2]],
            TX_FIRST_THIRD,
            Scfsi::Share0Then12,
        ),
        (Class1, Class5) => (
            [scalefactors[0], scalefactors[1], scalefactors[2]],
            TX_ALL,
            Scfsi::ThreePerGranule,
        ),

        // Row block (class1 = 2): 2,1 / 2,5 -> first + third;
        // 2,2 / 2,3 -> third only (scf1 reused via scfsi=10); 2,4 ->
        // third only (max).
        (Class2, Class1) | (Class2, Class5) => (
            [scalefactors[0], scalefactors[0], scalefactors[2]],
            TX_FIRST_THIRD,
            Scfsi::Share01Then2,
        ),
        (Class2, Class2) | (Class2, Class3) => (
            [scalefactors[0], scalefactors[0], scalefactors[0]],
            TX_THIRD_ONLY,
            Scfsi::ShareAll,
        ),
        (Class2, Class4) => ([max_scf, max_scf, max_scf], TX_THIRD_ONLY, Scfsi::ShareAll),

        // Row block (class1 = 3): 3,1 / 3,2 / 3,3 -> all three the
        // same as scf1; 3,4 -> all three == scf3; 3,5 -> first +
        // third.
        (Class3, Class1) | (Class3, Class2) | (Class3, Class3) => (
            [scalefactors[0], scalefactors[0], scalefactors[0]],
            TX_THIRD_ONLY,
            Scfsi::ShareAll,
        ),
        (Class3, Class4) => (
            [scalefactors[2], scalefactors[2], scalefactors[2]],
            TX_THIRD_ONLY,
            Scfsi::ShareAll,
        ),
        (Class3, Class5) => (
            [scalefactors[0], scalefactors[0], scalefactors[2]],
            TX_FIRST_THIRD,
            Scfsi::Share01Then2,
        ),

        // Row block (class1 = 4): 4,1 / 4,2 / 4,3 -> all three ==
        // scf2; 4,4 -> all three == scf3; 4,5 -> all three.
        (Class4, Class1) | (Class4, Class2) | (Class4, Class3) => (
            [scalefactors[1], scalefactors[1], scalefactors[1]],
            TX_THIRD_ONLY,
            Scfsi::ShareAll,
        ),
        (Class4, Class4) => (
            [scalefactors[2], scalefactors[2], scalefactors[2]],
            TX_THIRD_ONLY,
            Scfsi::ShareAll,
        ),
        (Class4, Class5) => (
            [scalefactors[0], scalefactors[1], scalefactors[2]],
            TX_ALL,
            Scfsi::ThreePerGranule,
        ),

        // Row block (class1 = 5): 5,1 / 5,5 -> all three; 5,2 / 5,3 ->
        // first two; 5,4 -> first + third.
        (Class5, Class1) | (Class5, Class5) => (
            [scalefactors[0], scalefactors[1], scalefactors[2]],
            TX_ALL,
            Scfsi::ThreePerGranule,
        ),
        (Class5, Class2) | (Class5, Class3) => (
            [scalefactors[0], scalefactors[1], scalefactors[1]],
            TX_FIRST_SECOND,
            Scfsi::Share0Then12,
        ),
        (Class5, Class4) => (
            [scalefactors[0], scalefactors[2], scalefactors[2]],
            TX_FIRST_THIRD,
            Scfsi::Share0Then12,
        ),
    };

    ScfsiSelection {
        used,
        pattern,
        scfsi,
        classes: (class1, class2),
    }
}

// The four Table C.4 transmission patterns named for readability.
//
// `TX_ALL`           — pattern column reads "1 2 3" (all three slots).
// `TX_FIRST_SECOND`  — pattern column reads "1 2 ."  (first two slots).
// `TX_FIRST_THIRD`   — pattern column reads "1 . 3"  (slots 0 and 2).
// `TX_THIRD_ONLY`    — pattern column reads ". . 1"  (slot 2 only,
//                       carrying the single shared value).
const TX_ALL: TransmissionPattern = TransmissionPattern {
    write0: true,
    write1: true,
    write2: true,
};
const TX_FIRST_SECOND: TransmissionPattern = TransmissionPattern {
    write0: true,
    write1: true,
    write2: false,
};
const TX_FIRST_THIRD: TransmissionPattern = TransmissionPattern {
    write0: true,
    write1: false,
    write2: true,
};
const TX_THIRD_ONLY: TransmissionPattern = TransmissionPattern {
    write0: false,
    write1: false,
    write2: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity-check the classifier across the spec's class boundaries.
    #[test]
    fn classify_difference_pins_every_class_boundary() {
        // Class 1: dscf <= -3.
        assert_eq!(classify_difference(-100), DifferenceClass::Class1);
        assert_eq!(classify_difference(-4), DifferenceClass::Class1);
        assert_eq!(classify_difference(-3), DifferenceClass::Class1);
        // Class 2: -3 < dscf < 0.
        assert_eq!(classify_difference(-2), DifferenceClass::Class2);
        assert_eq!(classify_difference(-1), DifferenceClass::Class2);
        // Class 3: dscf == 0.
        assert_eq!(classify_difference(0), DifferenceClass::Class3);
        // Class 4: 0 < dscf < 3.
        assert_eq!(classify_difference(1), DifferenceClass::Class4);
        assert_eq!(classify_difference(2), DifferenceClass::Class4);
        // Class 5: dscf >= 3.
        assert_eq!(classify_difference(3), DifferenceClass::Class5);
        assert_eq!(classify_difference(4), DifferenceClass::Class5);
        assert_eq!(classify_difference(100), DifferenceClass::Class5);
    }

    /// PDF page 76 row-by-row pin: every one of the 25 Table C.4
    /// entries maps to the documented `(used, pattern, scfsi)` triple.
    ///
    /// The test fixture chooses scalefactor inputs whose differences
    /// land in the exact target class, then verifies the lookup output.
    /// Where the "used" column references "4 = max of three", the input
    /// triple is chosen so the max is unambiguous.
    #[test]
    fn table_c4_full_25_row_pin() {
        // (class1, class2, sample input that lands in those classes,
        //   expected `used`, expected pattern flags, expected scfsi).
        // Sample inputs are picked so dscf1/dscf2 land in the target
        // class. With Table 3-B.1 indices in 0..=62, dscf in [-62, 62];
        // we use modest values 5..25 to stay well inside the legal
        // range.
        //
        // To target (Class_X, Class_Y) we need scf1-scf2 in class X
        // and scf2-scf3 in class Y. We pick scf2 = 10 as the anchor;
        // then scf1 = 10 + step_X (for the dscf1 sign of class X) and
        // scf3 = 10 - step_Y (so dscf2 = step_Y, sign of class Y).
        //
        // step values per class (size to land in the class interior):
        //   Class1 (dscf <= -3): pick dscf = -5 -> step = -5 (scf1 < scf2)
        //   Class2 (-3 < dscf < 0): pick dscf = -2 -> step = -2
        //   Class3 (dscf == 0): step = 0
        //   Class4 (0 < dscf < 3): pick dscf = +2 -> step = +2
        //   Class5 (dscf >= 3): pick dscf = +5 -> step = +5

        #[allow(clippy::type_complexity)]
        let rows: [(
            DifferenceClass,
            DifferenceClass,
            [u8; 3],
            [u8; 3],
            TransmissionPattern,
            Scfsi,
        ); 25] = [
            // (1,1): scf1=5, scf2=10, scf3=15 -> dscf1=-5, dscf2=-5 -> (1,1)
            // used=[scf1,scf2,scf3]=[5,10,15], pattern=1 2 3, scfsi=00
            (
                DifferenceClass::Class1,
                DifferenceClass::Class1,
                [5, 10, 15],
                [5, 10, 15],
                TX_ALL,
                Scfsi::ThreePerGranule,
            ),
            // (1,2): scf1=5, scf2=10, scf3=12 -> dscf1=-5, dscf2=-2 -> (1,2)
            // used=[5,10,10], pattern=1 2 ., scfsi=11
            (
                DifferenceClass::Class1,
                DifferenceClass::Class2,
                [5, 10, 12],
                [5, 10, 10],
                TX_FIRST_SECOND,
                Scfsi::Share0Then12,
            ),
            // (1,3): scf1=5, scf2=10, scf3=10 -> dscf1=-5, dscf2=0 -> (1,3)
            (
                DifferenceClass::Class1,
                DifferenceClass::Class3,
                [5, 10, 10],
                [5, 10, 10],
                TX_FIRST_SECOND,
                Scfsi::Share0Then12,
            ),
            // (1,4): scf1=5, scf2=10, scf3=8 -> dscf1=-5, dscf2=+2 -> (1,4)
            // used=[scf1,scf3,scf3]=[5,8,8], pattern=1 . 3, scfsi=11
            (
                DifferenceClass::Class1,
                DifferenceClass::Class4,
                [5, 10, 8],
                [5, 8, 8],
                TX_FIRST_THIRD,
                Scfsi::Share0Then12,
            ),
            // (1,5): scf1=5, scf2=10, scf3=5 -> dscf1=-5, dscf2=+5 -> (1,5)
            (
                DifferenceClass::Class1,
                DifferenceClass::Class5,
                [5, 10, 5],
                [5, 10, 5],
                TX_ALL,
                Scfsi::ThreePerGranule,
            ),
            // (2,1): scf1=8, scf2=10, scf3=15 -> dscf1=-2, dscf2=-5 -> (2,1)
            // used=[scf1,scf1,scf3]=[8,8,15], pattern=1 . 3, scfsi=01
            (
                DifferenceClass::Class2,
                DifferenceClass::Class1,
                [8, 10, 15],
                [8, 8, 15],
                TX_FIRST_THIRD,
                Scfsi::Share01Then2,
            ),
            // (2,2): scf1=8, scf2=10, scf3=12 -> dscf1=-2, dscf2=-2 -> (2,2)
            // used=[scf1,scf1,scf1]=[8,8,8], pattern=. . 1, scfsi=10
            (
                DifferenceClass::Class2,
                DifferenceClass::Class2,
                [8, 10, 12],
                [8, 8, 8],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (2,3): scf1=8, scf2=10, scf3=10 -> dscf1=-2, dscf2=0 -> (2,3)
            (
                DifferenceClass::Class2,
                DifferenceClass::Class3,
                [8, 10, 10],
                [8, 8, 8],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (2,4): scf1=8, scf2=10, scf3=8 -> dscf1=-2, dscf2=+2 -> (2,4)
            // used=[max=8,8,8] (smallest index = largest multiplier);
            // pattern=. . 1, scfsi=10. min(8,10,8)=8.
            (
                DifferenceClass::Class2,
                DifferenceClass::Class4,
                [8, 10, 8],
                [8, 8, 8],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (2,5): scf1=8, scf2=10, scf3=5 -> dscf1=-2, dscf2=+5 -> (2,5)
            (
                DifferenceClass::Class2,
                DifferenceClass::Class5,
                [8, 10, 5],
                [8, 8, 5],
                TX_FIRST_THIRD,
                Scfsi::Share01Then2,
            ),
            // (3,1): scf1=10, scf2=10, scf3=15 -> dscf1=0, dscf2=-5 -> (3,1)
            // used=[scf1,scf1,scf1]=[10,10,10], pattern=. . 1, scfsi=10
            (
                DifferenceClass::Class3,
                DifferenceClass::Class1,
                [10, 10, 15],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (3,2): scf1=10, scf2=10, scf3=12 -> dscf1=0, dscf2=-2 -> (3,2)
            (
                DifferenceClass::Class3,
                DifferenceClass::Class2,
                [10, 10, 12],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (3,3): scf1=10, scf2=10, scf3=10 -> dscf1=0, dscf2=0 -> (3,3)
            (
                DifferenceClass::Class3,
                DifferenceClass::Class3,
                [10, 10, 10],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (3,4): scf1=10, scf2=10, scf3=8 -> dscf1=0, dscf2=+2 -> (3,4)
            // used=[scf3,scf3,scf3]=[8,8,8], pattern=. . 1, scfsi=10
            (
                DifferenceClass::Class3,
                DifferenceClass::Class4,
                [10, 10, 8],
                [8, 8, 8],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (3,5): scf1=10, scf2=10, scf3=5 -> dscf1=0, dscf2=+5 -> (3,5)
            // used=[scf1,scf1,scf3]=[10,10,5], pattern=1 . 3, scfsi=01
            (
                DifferenceClass::Class3,
                DifferenceClass::Class5,
                [10, 10, 5],
                [10, 10, 5],
                TX_FIRST_THIRD,
                Scfsi::Share01Then2,
            ),
            // (4,1): scf1=12, scf2=10, scf3=15 -> dscf1=+2, dscf2=-5 -> (4,1)
            // used=[scf2,scf2,scf2]=[10,10,10], pattern=. . 1, scfsi=10
            (
                DifferenceClass::Class4,
                DifferenceClass::Class1,
                [12, 10, 15],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (4,2): scf1=12, scf2=10, scf3=12 -> dscf1=+2, dscf2=-2 -> (4,2)
            (
                DifferenceClass::Class4,
                DifferenceClass::Class2,
                [12, 10, 12],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (4,3): scf1=12, scf2=10, scf3=10 -> dscf1=+2, dscf2=0 -> (4,3)
            (
                DifferenceClass::Class4,
                DifferenceClass::Class3,
                [12, 10, 10],
                [10, 10, 10],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (4,4): scf1=12, scf2=10, scf3=8 -> dscf1=+2, dscf2=+2 -> (4,4)
            // used=[scf3,scf3,scf3]=[8,8,8], pattern=. . 1, scfsi=10
            (
                DifferenceClass::Class4,
                DifferenceClass::Class4,
                [12, 10, 8],
                [8, 8, 8],
                TX_THIRD_ONLY,
                Scfsi::ShareAll,
            ),
            // (4,5): scf1=12, scf2=10, scf3=5 -> dscf1=+2, dscf2=+5 -> (4,5)
            // used=[scf1,scf2,scf3]=[12,10,5], pattern=1 2 3, scfsi=00
            (
                DifferenceClass::Class4,
                DifferenceClass::Class5,
                [12, 10, 5],
                [12, 10, 5],
                TX_ALL,
                Scfsi::ThreePerGranule,
            ),
            // (5,1): scf1=15, scf2=10, scf3=15 -> dscf1=+5, dscf2=-5 -> (5,1)
            (
                DifferenceClass::Class5,
                DifferenceClass::Class1,
                [15, 10, 15],
                [15, 10, 15],
                TX_ALL,
                Scfsi::ThreePerGranule,
            ),
            // (5,2): scf1=15, scf2=10, scf3=12 -> dscf1=+5, dscf2=-2 -> (5,2)
            // used=[scf1,scf2,scf2]=[15,10,10], pattern=1 2 ., scfsi=11
            (
                DifferenceClass::Class5,
                DifferenceClass::Class2,
                [15, 10, 12],
                [15, 10, 10],
                TX_FIRST_SECOND,
                Scfsi::Share0Then12,
            ),
            // (5,3): scf1=15, scf2=10, scf3=10 -> dscf1=+5, dscf2=0 -> (5,3)
            (
                DifferenceClass::Class5,
                DifferenceClass::Class3,
                [15, 10, 10],
                [15, 10, 10],
                TX_FIRST_SECOND,
                Scfsi::Share0Then12,
            ),
            // (5,4): scf1=15, scf2=10, scf3=8 -> dscf1=+5, dscf2=+2 -> (5,4)
            // used=[scf1,scf3,scf3]=[15,8,8], pattern=1 . 3, scfsi=11
            (
                DifferenceClass::Class5,
                DifferenceClass::Class4,
                [15, 10, 8],
                [15, 8, 8],
                TX_FIRST_THIRD,
                Scfsi::Share0Then12,
            ),
            // (5,5): scf1=15, scf2=10, scf3=5 -> dscf1=+5, dscf2=+5 -> (5,5)
            (
                DifferenceClass::Class5,
                DifferenceClass::Class5,
                [15, 10, 5],
                [15, 10, 5],
                TX_ALL,
                Scfsi::ThreePerGranule,
            ),
        ];

        for (expected_c1, expected_c2, input, expected_used, expected_pattern, expected_scfsi) in
            rows
        {
            let sel = select_scfsi(input);
            assert_eq!(
                sel.classes,
                (expected_c1, expected_c2),
                "row ({expected_c1:?}, {expected_c2:?}) -- input {input:?} -- classes"
            );
            assert_eq!(
                sel.used, expected_used,
                "row ({expected_c1:?}, {expected_c2:?}) -- input {input:?} -- used"
            );
            assert_eq!(
                sel.pattern, expected_pattern,
                "row ({expected_c1:?}, {expected_c2:?}) -- input {input:?} -- pattern"
            );
            assert_eq!(
                sel.scfsi, expected_scfsi,
                "row ({expected_c1:?}, {expected_c2:?}) -- input {input:?} -- scfsi"
            );
        }
    }

    /// Identical-triplet input always picks the "1 1 1 / scfsi=10"
    /// shape: both differences are zero so we land at (Class3, Class3),
    /// the table's most-compact entry.
    #[test]
    fn identical_triplet_picks_share_all() {
        for v in 0u8..=62 {
            let sel = select_scfsi([v, v, v]);
            assert_eq!(
                sel.classes,
                (DifferenceClass::Class3, DifferenceClass::Class3)
            );
            assert_eq!(sel.used, [v, v, v]);
            assert_eq!(sel.pattern, TX_THIRD_ONLY);
            assert_eq!(sel.scfsi, Scfsi::ShareAll);
            assert_eq!(sel.pattern.transmitted_count(), 1);
        }
    }

    /// Scalefactor indices stepping by 25 in either direction land in
    /// class 1 or class 5 (both `|dscf| >= 3`), and rows (5,5) and
    /// (1,1) both transmit all three slots with scfsi=00.
    ///
    /// Note: Table 3-B.1 is monotonically *decreasing* (entry 0 is
    /// the largest multiplier, entry 62 the smallest), so a sequence
    /// of indices `[5, 30, 55]` represents a *decreasing* multiplier
    /// across the three granules; `dscf1 = scf1 - scf2 = -25` is in
    /// Class1, not Class5.
    #[test]
    fn large_strictly_changing_indices_pick_three_per_granule() {
        // Indices strictly *increasing* -> dscf strongly negative -> (1,1).
        let asc = select_scfsi([5, 30, 55]);
        assert_eq!(
            asc.classes,
            (DifferenceClass::Class1, DifferenceClass::Class1)
        );
        assert_eq!(asc.used, [5, 30, 55]);
        assert_eq!(asc.pattern, TX_ALL);
        assert_eq!(asc.scfsi, Scfsi::ThreePerGranule);
        assert_eq!(asc.pattern.transmitted_count(), 3);

        // Indices strictly *decreasing* -> dscf strongly positive -> (5,5).
        let desc = select_scfsi([55, 30, 5]);
        assert_eq!(
            desc.classes,
            (DifferenceClass::Class5, DifferenceClass::Class5)
        );
        assert_eq!(desc.used, [55, 30, 5]);
        assert_eq!(desc.pattern, TX_ALL);
        assert_eq!(desc.scfsi, Scfsi::ThreePerGranule);
        assert_eq!(desc.pattern.transmitted_count(), 3);
    }

    /// Class (2,4) is the only row that invokes the "4 = max of three"
    /// recipe: confirm the encoder rewrites *all* three slots to the
    /// minimum index (= largest Table 3-B.1 multiplier) regardless of
    /// the input ordering.
    #[test]
    fn class_2_4_uses_max_multiplier_aka_min_index() {
        // scf1=20, scf2=22, scf3=20 -> dscf1=-2 (Class2), dscf2=+2 (Class4)
        // -> row (2,4); used = [min(20,22,20), min, min] = [20,20,20].
        let sel = select_scfsi([20, 22, 20]);
        assert_eq!(
            sel.classes,
            (DifferenceClass::Class2, DifferenceClass::Class4)
        );
        assert_eq!(sel.used, [20, 20, 20]);

        // A case where the minimum is not at position 0 or 2.
        // scf1=11, scf2=10, scf3=11 -> dscf1=+1 (Class4), dscf2=-1 (Class2)
        // -> row (4,2); the "used" column there is "2 2 2" (= scf2), not
        // the "4 4 4" max-recipe row; min would have been at scf2 anyway.
        let sel2 = select_scfsi([11, 10, 11]);
        assert_eq!(
            sel2.classes,
            (DifferenceClass::Class4, DifferenceClass::Class2)
        );
        assert_eq!(sel2.used, [10, 10, 10]);
    }

    /// The transmission pattern's `transmitted_count` must match the
    /// Table C.4 column ("1 2 3" -> 3, "1 2 ." -> 2, "1 . 3" -> 2,
    /// ". . 1" -> 1) for every one of the 25 rows.
    #[test]
    fn transmitted_count_matches_table_c4_column_for_every_row() {
        use DifferenceClass::*;
        // (class1, class2, expected_count)
        let rows: [(DifferenceClass, DifferenceClass, usize); 25] = [
            (Class1, Class1, 3),
            (Class1, Class2, 2),
            (Class1, Class3, 2),
            (Class1, Class4, 2),
            (Class1, Class5, 3),
            (Class2, Class1, 2),
            (Class2, Class2, 1),
            (Class2, Class3, 1),
            (Class2, Class4, 1),
            (Class2, Class5, 2),
            (Class3, Class1, 1),
            (Class3, Class2, 1),
            (Class3, Class3, 1),
            (Class3, Class4, 1),
            (Class3, Class5, 2),
            (Class4, Class1, 1),
            (Class4, Class2, 1),
            (Class4, Class3, 1),
            (Class4, Class4, 1),
            (Class4, Class5, 3),
            (Class5, Class1, 3),
            (Class5, Class2, 2),
            (Class5, Class3, 2),
            (Class5, Class4, 2),
            (Class5, Class5, 3),
        ];

        for (c1, c2, expected) in rows {
            // Use a small synthetic triple to land in the requested row.
            let input = synth_for_classes(c1, c2);
            let sel = select_scfsi(input);
            assert_eq!(sel.classes, (c1, c2), "synth_for_classes wrong");
            assert_eq!(
                sel.pattern.transmitted_count(),
                expected,
                "row ({c1:?}, {c2:?})"
            );
        }
    }

    /// Per §C.1.5.2.6: "Only the scfsi for the subbands which will get
    /// a nonzero bit allocation are transmitted." This is a property of
    /// the caller, not the lookup; still, the 2-bit scfsi code we emit
    /// must round-trip through [`Scfsi`]'s schedule to recover the
    /// `used` triple from just the transmitted slots — i.e. the
    /// decoder's reconstruction of `used` from the wire matches what we
    /// claimed.
    ///
    /// The §2.4.3.3.2 / §2.4.3.3.3 schedule (per [`Scfsi`]):
    ///   00 = [a, b, c]
    ///   01 = [a, a, c]
    ///   10 = [a, a, a]
    ///   11 = [a, c, c]
    ///
    /// So given the scfsi code and the slots written via `pattern`,
    /// the decoder reads back the same `used` triple.
    #[test]
    fn scfsi_pattern_round_trips_to_used_triple() {
        // Exhaustive over a coarse grid of scalefactor inputs.
        for scf1 in (0u8..=62).step_by(7) {
            for scf2 in (0u8..=62).step_by(5) {
                for scf3 in (0u8..=62).step_by(11) {
                    let sel = select_scfsi([scf1, scf2, scf3]);
                    let reconstructed = decoder_reconstruct(&sel);
                    assert_eq!(
                        reconstructed, sel.used,
                        "round-trip failed for input [{scf1}, {scf2}, {scf3}]: \
                         pattern={:?} scfsi={:?} used={:?} reconstructed={reconstructed:?}",
                        sel.pattern, sel.scfsi, sel.used
                    );
                }
            }
        }
    }

    /// [`select_scfsi`] is a pure deterministic function of the input
    /// triple — same input always produces same output. This guards
    /// against an accidental dependency on hidden state (e.g. caching
    /// the previous frame's classes), which would silently desync the
    /// encoder against the decoder.
    ///
    /// NOTE: `select_scfsi` is *not* idempotent under chained
    /// application — `select_scfsi(select_scfsi(x).used)` may pick a
    /// different Table C.4 row because the `used` triple's intra-frame
    /// differences differ from the original triple's (e.g. row (2,1)
    /// emits `used=[a,a,c]` whose `dscf1=0` lands in row (3,1) on the
    /// second pass). The spec's "adjusted" step is a *one-shot*
    /// transformation defined against the *original* classes, not a
    /// fixed point of repeated application.
    #[test]
    fn select_scfsi_is_a_pure_deterministic_function() {
        for scf1 in (0u8..=62).step_by(3) {
            for scf2 in (0u8..=62).step_by(5) {
                for scf3 in (0u8..=62).step_by(7) {
                    let a = select_scfsi([scf1, scf2, scf3]);
                    let b = select_scfsi([scf1, scf2, scf3]);
                    assert_eq!(a, b, "non-deterministic for [{scf1},{scf2},{scf3}]");
                }
            }
        }
    }

    /// The §C.1.5.2.5 "used" triple's compaction must always be
    /// reversible by the corresponding scfsi schedule; an empty or
    /// no-write pattern would lose information and is forbidden by the
    /// Table C.4 column ordering. Every row has at least slot 2 set or
    /// the full triple.
    #[test]
    fn every_pattern_writes_at_least_one_slot() {
        for scf1 in 0u8..=10 {
            for scf2 in 0u8..=10 {
                for scf3 in 0u8..=10 {
                    let sel = select_scfsi([scf1, scf2, scf3]);
                    assert!(
                        sel.pattern.transmitted_count() >= 1,
                        "pattern with zero transmitted slots for input [{scf1}, {scf2}, {scf3}]"
                    );
                    assert!(sel.pattern.transmitted_count() <= 3);
                }
            }
        }
    }

    /// Synthesize a (scf1, scf2, scf3) triple whose `(class(dscf1),
    /// class(dscf2))` lands at `(c1, c2)`. Used by the
    /// transmitted-count + arbitrary-row tests above.
    fn synth_for_classes(c1: DifferenceClass, c2: DifferenceClass) -> [u8; 3] {
        // Pick scf2 as the anchor; choose scf1 / scf3 so that
        // scf1 - scf2 lands in c1's interior and scf2 - scf3 lands in
        // c2's interior. Class interior values:
        //   Class1 (dscf <= -3): -5
        //   Class2 (-3 < dscf < 0): -2
        //   Class3 (dscf == 0): 0
        //   Class4 (0 < dscf < 3): +2
        //   Class5 (dscf >= 3): +5
        let step1: i16 = match c1 {
            DifferenceClass::Class1 => -5,
            DifferenceClass::Class2 => -2,
            DifferenceClass::Class3 => 0,
            DifferenceClass::Class4 => 2,
            DifferenceClass::Class5 => 5,
        };
        let step2: i16 = match c2 {
            DifferenceClass::Class1 => -5,
            DifferenceClass::Class2 => -2,
            DifferenceClass::Class3 => 0,
            DifferenceClass::Class4 => 2,
            DifferenceClass::Class5 => 5,
        };
        let scf2 = 20i16;
        let scf1 = scf2 + step1;
        let scf3 = scf2 - step2;
        [scf1 as u8, scf2 as u8, scf3 as u8]
    }

    /// Reconstruct the decoder-side `used` triple from the
    /// `(pattern, scfsi)` pair the encoder emits — implements the
    /// §2.4.3.3.2 / §2.4.3.3.3 schedule literally per [`Scfsi`]'s
    /// documented table.
    fn decoder_reconstruct(sel: &ScfsiSelection) -> [u8; 3] {
        // The on-wire 6-bit indices the decoder will physically read:
        //   slot 0 if pattern.write0
        //   slot 1 if pattern.write1
        //   slot 2 if pattern.write2
        // Where they come from in the encoder's `used` triple matches
        // the source-slot at the same index.
        let wire0 = sel.pattern.write0.then_some(sel.used[0]);
        let wire1 = sel.pattern.write1.then_some(sel.used[1]);
        let wire2 = sel.pattern.write2.then_some(sel.used[2]);

        // Per [`Scfsi`] documentation in the audio_data module:
        //   00 = [a, b, c] -- parts 0, 1, 2 each on the wire
        //   01 = [a, a, c] -- part 0 covers granules 0+1, part 2 on wire
        //   10 = [a, a, a] -- part 0 only (reused everywhere)
        //   11 = [a, c, c] -- part 0 covers granule 0, part 2 covers 1+2
        //
        // Map the on-wire parts to granule positions per the schedule.
        match sel.scfsi {
            Scfsi::ThreePerGranule => [
                wire0.expect("scfsi=00 requires wire0"),
                wire1.expect("scfsi=00 requires wire1"),
                wire2.expect("scfsi=00 requires wire2"),
            ],
            Scfsi::Share01Then2 => {
                // Two wire parts: first covers granules 0 & 1 (a),
                // second covers granule 2 (c). Per Table C.4 rows
                // (2,1) / (2,5) / (3,5), wire layout is pattern
                // `1 . 3` -- part 0 (`a`) at slot 0 and part 2 (`c`)
                // at slot 2.
                let a = wire0.expect("scfsi=01 expects wire0");
                let c = wire2.expect("scfsi=01 expects wire2");
                [a, a, c]
            }
            Scfsi::ShareAll => {
                // One wire part: in Table C.4 it lives at slot 2 (the
                // ". . 1" pattern); the spec calls it "the single
                // scalefactor". Reuse across all three granules.
                let a = wire2.expect("scfsi=10 expects wire2");
                [a, a, a]
            }
            Scfsi::Share0Then12 => {
                // Two wire parts: first covers granule 0 (a), second
                // covers granules 1 & 2 (c). Per Table C.4 rows
                // (1,2) / (1,3) / (5,2) / (5,3), pattern is `1 2 .` --
                // wire0 = a, wire1 = c. Per row (1,4) / (5,4) the
                // pattern is `1 . 3` -- wire0 = a, wire2 = c.
                let a = wire0.expect("scfsi=11 expects wire0");
                let c = wire1.or(wire2).expect("scfsi=11 expects wire1 or wire2");
                [a, c, c]
            }
        }
    }
}
