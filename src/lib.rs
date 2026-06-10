//! # oxideav-mp2
//!
//! Pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
//! codec, clean-room rebuilt against ISO/IEC 11172-3 (1993) and the
//! §2.4 / Annex B / Annex C low-sampling-rate extension of ISO/IEC
//! 13818-3 (1997).
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
//!   against the closed-form `scalefactor[i] = 2^((3 − i) / 3)`.
//!
//! * **§2.4.1.6 / §2.4.3.3.1..3 audio-data side info**
//!   ([`audio_data`] module): the per-frame bit-allocation +
//!   scalefactor-selection-information + scalefactor loops are parsed
//!   into a typed [`audio_data::AudioData`] struct. Bit allocation
//!   indexes into Tables 3-B.2a..d via the
//!   [`bitalloc::BitAllocTable`] selected by the
//!   `(sample_rate, per-channel bitrate)` rule from §2.4.2.3; scfsi
//!   schedules are typed by [`audio_data::Scfsi`]; on-wire scalefactor
//!   indices (1, 2, or 3 per subband per channel depending on scfsi)
//!   are expanded across the three granules per the §2.4.2.3 schedule.
//!
//! * **§2.4.3.3.4 sample requantization** ([`requant`] module): turns a
//!   bitstream sample code into a normalized fractional value `s''`.
//!   [`requant::degroup`] runs the §2.4.3.3.4 radix-`nlevels`
//!   degrouping for grouped classes (`nb_steps ∈ {3, 5, 9}`);
//!   [`requant::requantize_code`] performs the MSB inversion + two's
//!   complement fractional interpretation + the `s'' = C * (s''' + D)`
//!   linear formula with the Table 3-B.4 `C` / `D` constants;
//!   [`requant::read_triplet`] reads one (subband, granule) triplet
//!   from the bitstream; [`requant::requantize_scaled`] layers the
//!   §2.4.3.3.3 Table 3-B.1 rescaling (`s' = factor * s''`) on top.
//!
//! * **§2.4.1.4 / §2.4.3.1 CRC-16** ([`crc`] module): the
//!   `G(X) = X^16 + X^15 + X^2 + 1` shift register with initial state
//!   `0xFFFF` is implemented as a primitive that both the decoder
//!   (verify) and the encoder (emit) can drive. The Layer II
//!   protected-field set per Annex B Table B.5 (header bits 16…31 +
//!   bit allocation + scfsi) is wrapped by [`crc16_layer2`] and
//!   [`verify_layer2_crc`]; the granular per-bit / per-field
//!   primitives [`crc16_step`], [`crc16_update_bits`], and
//!   [`crc16_update_packed`] are exposed for the encoder's streaming
//!   accumulation.
//!
//! * **§2.4.3.2 / §2.4.3.3.5 polyphase synthesis filterbank**
//!   ([`synthesis`] module): one Annex A Figure A.2 invocation
//!   (`shift V` → `matrix V = N * S` → `build U` → `window W = U * D`
//!   → `S_j = sum_{i = 0..16} W[j + 32 * i]`) consumes one
//!   32-vector of subband samples and produces one 32-vector of
//!   reconstructed PCM samples in the §2.4.3.4.7.1 nominal `[-1, +1]`
//!   range. The N_ik matrix is precomputed at construction from the
//!   §2.4.3.3.5 closed form
//!   `N_ik = cos[(16 + i)(2k + 1) * pi / 64]`; the 512 D[i]
//!   synthesis-window coefficients are read verbatim from Annex B
//!   Table 3-B.3 (PDF pages 50-52) into
//!   [`tables_synthesis::D`]. Caller-side resets are exposed via
//!   [`SynthesisFilterbank::reset`].
//!
//! * **§2.4.1.6 / §2.4.3.1 / §2.4.3.2 frame-level decode loop**
//!   ([`frame`] module): [`frame::decode_frame`] consumes one
//!   complete Layer II frame from a buffer, drives the
//!   `for (gr, sb, ch)` triplet loop through
//!   [`requant::read_triplet`], applies the §2.4.3.3.3 scalefactor
//!   rescaling per the
//!   `scalefactor_granule = sample_granule / 4` partition (§2.4.2.3),
//!   pushes the 36 successive 32-vectors of subband samples per
//!   channel through a [`SynthesisFilterbank`], and emits 1152 PCM
//!   samples per channel ([`frame::PCM_SAMPLES_PER_CHANNEL`]). When
//!   `protection_bit == 0` the §2.4.3.1 CRC-16 over the protected
//!   region (Annex B Table B.5) is verified via [`crc16_layer2`];
//!   mismatches raise [`frame::FrameError::CrcMismatch`]. A
//!   per-stream [`frame::FrameDecodeState`] threads the polyphase
//!   filterbank's V ring buffer across successive frames per Annex A
//!   Figure A.2 footnote 1. [`frame::decode_all_frames`] chains
//!   successive frames until the input buffer is exhausted.
//!
//! * **§C.1.3 Annex C polyphase analysis filterbank** ([`analysis`]
//!   module): the encoder's time-reversed dual of the §2.4.3.3.5
//!   decoder filterbank. One [`AnalysisFilterbank::push_audio`] call
//!   consumes one 32-vector of PCM input samples and produces one
//!   32-vector of subband samples through the documented
//!   `shift X` → `insert 32 most-recent samples` → `window Z = X * C`
//!   → `Y_i = sum_{j = 0..8} Z[i + 64 * j]` →
//!   `S_i = sum_{k = 0..64} M_ik * Y_k` pipeline. The 32x64 `M_ik`
//!   matrix is precomputed at construction from the §C.1.3 closed
//!   form `M_ik = cos[(2i + 1)(k - 16) * pi / 64]`; the 512 C[i]
//!   analysis-window coefficients are read verbatim from Annex C
//!   Table C.1 (PDF pages 67-69) into [`tables_analysis::C`]. The
//!   spec-paired filterbank-window identity `D[i] == 32 * C[i]` is
//!   honoured to within 1 ULP at the 9-decimal-digit grid (cross-
//!   checked by [`tables_analysis::tests::c_matches_d_over_32_within_rounding`]).
//!
//! * **§2.4.3.3.4 encoder sub-band sample quantizer**
//!   ([`encoder_samples`] module). [`quantize_sample`] inverts the
//!   normative decode mapping: divide out the Table 3-B.4 linear
//!   formula (`s''' = s''/C − D`), clamp into the legal `k` range
//!   for the active class (ungrouped: `[−2^(n−1), 2^(n−1) − 1]`;
//!   grouped: `[−2^(n−1), nb_steps − 1 − 2^(n−1)]`), encode as
//!   `n`-bit two's complement, then re-invert the MSB to match
//!   [`crate::requant::requantize_code`]'s consumption. For grouped
//!   classes the three per-triplet digits are packed via the radix-
//!   `nlevels` rule (the inverse of [`crate::requant::degroup`]).
//!   [`write_triplet`] / [`write_triplet_scaled`] drive a
//!   `oxideav_core::bits::BitWriter` through one (subband, granule)
//!   triplet, advancing it by the same
//!   bit count [`crate::requant::read_triplet`] would consume.
//!
//! * **§C.1.5.2.7 encoder iterative bit-allocator**
//!   ([`encoder_bit_allocator`] module). [`allocate_bits`] runs the
//!   informative Annex C iterative loop end-to-end against a
//!   per-(channel, sub-band) signal-to-mask ratio table
//!   ([`encoder_bit_allocator::SmrTable`]) supplied by the caller
//!   (typically the §D.1 / §D.2 psychoacoustic model). It returns an
//!   [`AudioData`] whose `nb_steps` field is populated and ready to be
//!   fed to [`compute_scalefactors`] → [`select_scfsi`] →
//!   [`write_audio_data`]. [`fixed_bit_budget`] surfaces the constant
//!   bit-budget terms (`bhdr`, `bcrc`, `bbal`) the iteration starts
//!   from; [`snr_db`] reads the Annex C Table C.5
//!   "Layer II Signal-to-Noise Ratios" entry for a given `nb_steps`;
//!   [`sample_bits_for`] returns the per-frame sample-codeword bit
//!   cost the iteration charges when an `nb_steps` decision is taken.
//!
//! * **§2.4 / Annex C frame-level encode loop** ([`encoder_frame`]
//!   module). [`encode_frame(header, pcm, smr_db, banc)`] pulls the
//!   prior encoder stages together: analysis filterbank →
//!   per-(channel, sub-band, granule) scalefactor extraction →
//!   §C.1.5.2.7 bit allocation against the supplied SMR table →
//!   §C.1.5.2.5 / Table C.4 scfsi selection → §2.4.1.6 audio-data
//!   section + §2.4.3.3.4 sample codewords → §2.4.3.1 CRC patch (when
//!   `protection_bit == 0`), returning a `Vec<u8>` of exactly
//!   `header.frame_size_bytes()` bytes. [`encode_frame_with`] takes
//!   a persistent [`EncodeFrameState`] so the §C.1.3 X ring buffer
//!   persists across successive frames (the encoder dual of
//!   [`frame::FrameDecodeState`]). The emitted frame round-trips
//!   through [`frame::decode_frame`] for stereo, single-channel, and
//!   joint-stereo headers, with and without the optional CRC slot;
//!   flipping any payload bit afterwards makes the decoder reject
//!   the frame via [`frame::FrameError::CrcMismatch`]. Varying the
//!   per-(channel, sub-band) SMR table redirects the §C.1.5.2.7
//!   allocator's spend toward the boosted sub-bands.
//!
//! * **§2.4.1.8 `ancillary_data()` emission** on the encoder
//!   ([`encoder_frame`] module). [`encode_frame_with_ancillary`] and
//!   [`encode_frame_with_state_and_ancillary`] accept an `ancillary:
//!   &[u8]` payload and copy it into the §2.4.2.1 frame tail that
//!   begins immediately after the §2.4.1.6 audio-data + §2.4.3.3.4
//!   sample-codeword region (the copy starts on the next byte
//!   boundary so the tail is byte-granular). The §C.1.5.2.7 `banc`
//!   reservation still steers the allocator — pass
//!   `banc >= ancillary.len() * 8` to guarantee the tail is wide
//!   enough. Over-long payloads surface
//!   [`EncodeError::AncillaryTooLarge`] carrying both the actual
//!   tail capacity (`space`) and the rejected length (`got`). The
//!   §2.4.3.1 CRC patch is byte-identical between the empty-
//!   ancillary and non-empty-ancillary frames — Annex B Table B.5
//!   does not protect the §2.4.1.8 tail.
//!
//! ## What does not work yet
//!
//! The §D.1 / §D.2 psychoacoustic model is not yet built — callers
//! must supply their own [`encoder_bit_allocator::SmrTable`]. A
//! constant 0 dB table produces a syntactically-valid frame whose
//! allocation is rate-driven only.

#![warn(missing_debug_implementations)]

use oxideav_core::RuntimeContext;

pub mod analysis;
pub mod audio_data;
pub mod bitalloc;
pub mod codec_decoder;
pub mod crc;
pub mod encoder_bit_allocator;
pub mod encoder_frame;
pub mod encoder_samples;
pub mod encoder_scalefactors;
pub mod encoder_scfsi;
pub mod frame;
pub mod header;
pub mod psy;
pub mod requant;
pub mod synthesis;
pub mod tables;
pub mod tables_analysis;
pub mod tables_d2;
pub mod tables_synthesis;

pub use analysis::{AnalysisFilterbank, NUM_SUBBANDS as ANALYSIS_NUM_SUBBANDS, X_BUF_LEN};
pub use audio_data::{
    parse_audio_data, parse_audio_data_with_section_bits, write_audio_data,
    write_audio_data_with_section_bits, AudioData, AudioDataError, AudioDataWriteError, Scfsi,
    MAX_CHANNELS,
};
pub use bitalloc::{
    bitrate_per_channel_kbps, class_of_quantization, is_grouped, select_table, BitAllocTable,
    QuantClass, NUM_SUBBANDS,
};
pub use codec_decoder::{
    make_decoder, register_codecs, Mp2CoreDecoder, CODEC_ID_STR, WAVE_FORMAT_MPEG,
};
pub use crc::{
    crc16_layer2, crc16_step, crc16_update_bits, crc16_update_packed, verify_layer2_crc,
    INIT_STATE as CRC_INIT_STATE,
};
pub use encoder_bit_allocator::{
    allocate_bits, fixed_bit_budget, sample_bits_for, snr_db, BitAllocError, FixedBitBudget,
    SmrTable, CRC_BITS, HEADER_BITS, SCFSI_BITS_PER_SLOT, WORST_CASE_SCALEFACTOR_BITS_PER_SLOT,
};
pub use encoder_frame::{
    encode_frame, encode_frame_with, encode_frame_with_ancillary,
    encode_frame_with_state_and_ancillary, EncodeError, EncodeFrameState,
};
pub use encoder_samples::{
    quantize_sample, quantize_scaled, write_triplet, write_triplet_scaled, SampleWriteError,
};
pub use encoder_scalefactors::{
    compute_scalefactors, extract_scalefactor_index, pick_scalefactor_index,
};
pub use encoder_scfsi::{
    classify_difference, select_scfsi, DifferenceClass, ScfsiSelection, TransmissionPattern,
};
pub use frame::{
    decode_all_frames, decode_frame, decode_frame_with, layer2_crc, DecodedFrame, FrameDecodeState,
    FrameError, PCM_SAMPLES_PER_CHANNEL, SAMPLES_PER_TRIPLET, SAMPLE_GRANULES_PER_FRAME,
};
pub use header::{
    decode_bitrate, decode_bitrate_lsf, decode_sampling_frequency, decode_sampling_frequency_lsf,
    encode_bitrate, encode_bitrate_lsf, encode_sampling_frequency, encode_sampling_frequency_lsf,
    find_sync, is_layer2_bitrate_mode_allowed, Emphasis, FrameHeader, HeaderError, Mode,
    ModeExtension, SYNCWORD,
};
pub use psy::{
    decimate_tonal_maskers, fft_line_to_subband_layer2, global_masking_threshold_db,
    hann_window_layer2, individual_masking_threshold_db, is_local_maximum, is_tonal_layer2,
    list_non_tonal_layer2, masker_in_target_window, masking_function_vf, masking_index_non_tonal,
    masking_index_tonal, minimum_masking_threshold_subband, non_tonal_band_index, non_tonal_spl_db,
    relevant_maskers_for_target_line, scalefactor_spl_term_db, signal_to_mask_ratio_subband,
    sound_pressure_level_subband, tonal_neighbourhood_layer2, tonal_spl_db,
    zero_tonal_neighbourhood_layer2, Masker, MaskerKind, SubbandSplMethod, LAYER2_FFT_BINS,
    LAYER2_FFT_LEN, MASKING_FUNCTION_DZ_HI, MASKING_FUNCTION_DZ_LO, NUM_SUBBANDS_LAYER2,
    SPL_FULL_SCALE, SPL_PEAK_RMS_CORRECTION_DB, TONALITY_THRESHOLD_DB,
    TONAL_DECIMATION_WINDOW_BARK,
};
pub use requant::{degroup, read_triplet, requantize_code, requantize_scaled, RequantError};
pub use synthesis::{SynthesisFilterbank, NUM_SUBBANDS as SYNTH_NUM_SUBBANDS, V_BUF_LEN};
pub use tables::{SCALEFACTORS, SCALEFACTOR_COUNT};
pub use tables_analysis::{C as ANALYSIS_WINDOW, C_LEN as ANALYSIS_WINDOW_LEN};
pub use tables_d2::{
    CriticalBandBoundary, SamplingRate as PsyAnnexDSamplingRate, TABLE_D_2D_LAYER_II_32KHZ,
    TABLE_D_2E_LAYER_II_44K1HZ, TABLE_D_2F_LAYER_II_48KHZ,
};
pub use tables_synthesis::{D as SYNTHESIS_WINDOW, D_LEN as SYNTHESIS_WINDOW_LEN};

/// Crate-local error type. The frame-level decode path is wired up via
/// [`frame::decode_frame`]; [`Error::NotImplemented`] is reserved for
/// the §2.4.1.4 encoder path that has not yet been built.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// A 4-byte Layer II header could not be parsed from the input.
    Header(HeaderError),
    /// The §2.4.1.6 / §2.4.3.3 audio-data side info could not be parsed.
    AudioData(AudioDataError),
    /// The §2.4.1.6 / §2.4.3.2 frame-level decode loop failed.
    Frame(FrameError),
    /// A reachable Layer II decode/encode path that is not yet wired up
    /// (currently: encoder).
    NotImplemented,
}

impl From<HeaderError> for Error {
    fn from(value: HeaderError) -> Self {
        Error::Header(value)
    }
}

impl From<AudioDataError> for Error {
    fn from(value: AudioDataError) -> Self {
        Error::AudioData(value)
    }
}

impl From<FrameError> for Error {
    fn from(value: FrameError) -> Self {
        Error::Frame(value)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Header(err) => write!(f, "oxideav-mp2 header error: {err}"),
            Error::AudioData(err) => write!(f, "oxideav-mp2 audio-data error: {err}"),
            Error::Frame(err) => write!(f, "oxideav-mp2 frame error: {err}"),
            Error::NotImplemented => {
                write!(
                    f,
                    "oxideav-mp2: codec path not yet wired up (encoder pending)"
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Header(err) => Some(err),
            Error::AudioData(err) => Some(err),
            Error::Frame(err) => Some(err),
            Error::NotImplemented => None,
        }
    }
}

/// Codec registration: install the MPEG-1 Audio Layer II decoder into
/// `ctx`'s codec registry. The factory accepts a [`CodecParameters`]
/// hint and returns a [`Mp2CoreDecoder`] that adapts the existing
/// [`frame::decode_frame_with`] primitive to the framework's
/// packet-in / frame-out [`oxideav_core::Decoder`] trait. See
/// [`codec_decoder::register_codecs`] for the claimed container tags
/// (`WAVE_FORMAT_MPEG = 0x0050`, Matroska `A_MPEG/L2`) and the §2.4.1.3
/// layer-field probe used to disambiguate the `0x0050` collision with
/// `oxideav-mp1`.
pub fn register(ctx: &mut RuntimeContext) {
    codec_decoder::register_codecs(&mut ctx.codecs);
}

oxideav_core::register!("mp2", register);
