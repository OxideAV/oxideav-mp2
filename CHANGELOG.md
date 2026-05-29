# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate adheres
to [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (round 175 — encoder, step 2: §2.4.2.3 bit-allocation inverse mapping)

- **`BitAllocTable::allocation_index(sb, nb_steps) -> Option<u32>`**
  (`bitalloc` module): encoder-side inverse of the existing
  `BitAllocTable::nb_steps(sb, index)`. Given the `nb_steps` value
  the encoder wants to record for subband `sb`, the function returns
  the `nbal`-bit `allocation[ch][sb]` field code that the §2.4.1.6
  decoder will read back into that same `nb_steps`. The §2.4.2.3
  "-" sentinel `nb_steps == 0` always maps to the index-0 code
  (irrespective of subband); subbands at or past the active
  sub-table's `sblimit` return `None` (no allocation field exists);
  and off-row `nb_steps` values — those that do not appear in the
  PDF row for the targeted subband — return `None` (the §2.4.2.3
  prose constrains the encoder to one of the tabulated column
  values, so an off-row value is not representable on the wire).
  The mapping is well-defined: each B.2 row carries each `nb_steps`
  value at most once because the columns are strictly monotonically
  increasing in the PDF, so the inverse is a total function on the
  row's range and the empty function elsewhere. This is the first
  encoder-side primitive required to build the §2.4.1.6 audio-data
  writer (next encoder step after the existing §2.4.1.3 / §2.4.2.3
  header writer and the §2.4.1.4 / §2.4.3.1 CRC-16 write primitives).
- **Five new tests** under `bitalloc::tests` (121 → 126, all green):
  `allocation_index_is_total_inverse_of_nb_steps_for_every_cell`
  walks every `(table, sb, index)` triple of all four B.2 sub-tables
  (B.2a / B.2b / B.2c / B.2d — 27 + 30 + 8 + 12 sblimit subbands ×
  every `nbal`-bit column) and confirms
  `allocation_index(sb, nb_steps(sb, idx)) == idx` byte-for-byte;
  `allocation_index_zero_sentinel_returns_zero_for_every_in_range_subband`
  pins the §2.4.2.3 zero-sentinel rule across every in-range
  `(table, sb)` pair;
  `allocation_index_rejects_subbands_at_or_past_sblimit` confirms
  `None` for every `sb ∈ [sblimit, 32)` against each of the four
  sub-tables, including the zero sentinel and arbitrary
  in-the-row-elsewhere `nb_steps` values;
  `allocation_index_rejects_off_row_nb_steps` exercises specific
  off-row probes (`5` and `9` against B.2a's wide row sb=0..=2 vs.
  presence in the short row sb=3..=10; `63` against the nbal=3 row
  vs. presence of `31`; `7` against the nbal=2 row vs. presence of
  `65535`; plus a battery of arbitrary non-tabulated values
  including `1, 2, 4, 6, 8, 10, 11, 16, 99`);
  `allocation_index_matches_pdf_rows_for_b2c_and_b2d` cross-checks
  the encoder mapping against the literal PDF page-48/49 column
  ordering for both the nbal=4 wide row (sb=0..=1) and the nbal=3
  short row (B.2c sb=2..=7 and B.2d sb=2..=11).

The new function does not change any decode behaviour or the
existing public surface; it is purely additive (one new method on
`BitAllocTable`, already re-exported at the crate root) and
strictly clean-room — every numeric value the tests assert is
already in `src/bitalloc.rs` (transcribed directly from the
staged ISO/IEC 11172-3 PDF pages 46-49 in earlier rounds).

### Added (round 162 — malformed-input property tests)

- **`tests/malformed_input.rs`** integration suite (+14 tests; total
  107 → 121, all green) property-testing the §2.4.1.3 / §2.4.2.3
  `FrameHeader::parse` parser and the §2.4.3.1 / §2.4.1.6
  `decode_frame` loop against malformed inputs without any new
  src/ surface change:
  - **32-bit header bit-flip exhaustion**
    (`header_bit_flips_never_panic_or_violate_postconditions`):
    every single-bit flip of the canonical 192k/44.1k/Stereo/no-CRC
    header must either succeed with `channels() ∈ {1, 2}` and
    `frame_size_bytes() ≥ 4`, or return one of the 11 documented
    `HeaderError` variants — the test's error match arm is
    wildcard-free so a future `HeaderError` addition forces this
    test to be updated. Three further tests pin §2.4.1.3 fixed-field
    flips to specific errors: syncword bits 31..20 →
    `BadSync` (×12), `ID` bit 19 → `LsfNotSupported`, layer bits
    18..17 → `UnsupportedLayer(_)`. The `protection_bit` flip is
    pinned to a successful parse with the field toggled.
  - **Derived-field bit-flip oracles**
    (`sampling_frequency_to_reserved_value_is_rejected`,
    `emphasis_to_reserved_value_is_rejected`,
    `bitrate_high_bit_flip_triggers_layer2_mode_matrix_rejection`,
    `semantic_only_bit_flips_round_trip_through_parse`): five tests
    pin specific bit-flip → error mappings derived from the
    §2.4.2.3 ladders / matrix (e.g. flipping the high `bitrate_index`
    bit takes the canonical 192k stereo header to 48k stereo, which
    is in the disallowed matrix).
  - **Prefix-truncation exhaustion**
    (`decode_frame_truncation_is_exhaustive_and_never_panics`,
    `header_parse_returns_buffer_too_short_for_every_short_prefix`,
    `decode_frame_truncation_at_crc_slot_boundary`,
    `payload_one_byte_short_is_truncated_not_audio_data_underflow`):
    for every prefix length `0..626` of a synthesized
    canonical 192k/44.1k frame, `decode_frame` returns either
    `Header(BufferTooShort)` (prefix < 4) or
    `Truncated { have, need }` (4 ≤ prefix < 626) with `have ==
    prefix_len`, `need == 626`, never panicking; a CRC-protected
    header with 5 bytes (one CRC slot byte missing) is pinned to
    `Truncated { have: 5, need >= 6 }`; and a one-byte-short
    payload is pinned to `Truncated` (NOT
    `AudioData(UnexpectedEnd)`) so a future reorder that runs the
    §2.4.1.6 bit reader before the §2.4.3.1 frame-size check is
    caught immediately.
  - **Sync-search robustness**
    (`find_sync_is_none_when_no_syncword_present`,
    `find_sync_reports_leftmost_match`,
    `parse_reports_bad_sync_exhaustively_for_non_f_second_byte`):
    `find_sync` is exhaustively confirmed to return `None` across
    the 240 second-byte values with top nibble ≠ `0xF` paired with
    `0xFF`, and across the 255 first-byte values ≠ `0xFF` paired
    with `0xFF`; `find_sync` of a planted sync at offset 13 returns
    `Some(13)` and a planted earlier sync wins over the later one;
    `FrameHeader::parse` of every `[0xFF, b1, 0xA0, 0x04]` for
    `b1 ∈ 0x00..=0xEF` returns `BadSync` without panicking.

The new tests do not touch `src/` — they harden the existing
public surface (`FrameHeader::parse`, `decode_frame`, `find_sync`,
`HeaderError`, `FrameError`) against malformed inputs without
adding behaviour. The §2.4.1.3 header byte/bit map and the
§2.4.3.1 / §2.4.2.1 `floor(144·br/Fs)+padding` frame-size formula
are the only specification material the tests rely on, and both
are already documented at the top of `src/header.rs`.

### Added (round 157 — encoder, step 1: §2.4.2.3 frame-header writer)

- **§2.4.1.3 / §2.4.2.3 frame-header writer** (encoder side, `header`
  module): the inverse of `FrameHeader::parse` is
  `FrameHeader::emit_bytes(&self) -> Result<[u8; 4], HeaderError>`,
  which packs the 32-bit big-endian §2.4.1.3 word — syncword
  (`0xFFF`) + `ID = '1'` (MPEG-1) + `layer = '10'` (Layer II) +
  `protection_bit` + bitrate code + sampling-frequency code +
  padding + private + mode + mode_extension + copyright + original
  + emphasis — directly from the typed `FrameHeader` struct. The
  output is bit-exact identical to what `parse` would have read.
  Two new helper functions `encode_bitrate(bit_rate) -> Result<u8,
  HeaderError>` and `encode_sampling_frequency(sample_rate) ->
  Result<u8, HeaderError>` invert `decode_bitrate` /
  `decode_sampling_frequency`. Encoder-side validation mirrors the
  decoder's §2.4.2.3 contract: off-ladder `bit_rate` is rejected as
  the new `HeaderError::UnsupportedBitrate(u32)`, `sample_rate`
  outside `{32000, 44100, 48000}` Hz as
  `HeaderError::UnsupportedSamplingFrequency(u32)`, and the
  §2.4.2.3 disallowed-(bitrate, mode) matrix as
  `HeaderError::DisallowedBitrateModeCombination`. Lookup ordering
  reports the most-specific error first — off-ladder bitrate /
  sample-rate are flagged before the matrix check, so the caller
  always learns the true cause of rejection. The two §2.4.2.3
  reserved codes (`sampling_frequency = '11'`, `emphasis = '10'`)
  cannot be produced because the type system has no corresponding
  values.
- Nine new tests under `header::tests`:
  `encode_bitrate_inverts_decode_bitrate` (round-trips all 14
  ladder codes + rejects 0 / off-ladder / non-kbps inputs),
  `encode_sampling_frequency_inverts_decode` (round-trips all 3
  sf codes + rejects 11_025 / LSF 16/22.05/24 kHz),
  `emit_bytes_round_trips_a_canonical_header` (192 kbit/s /
  44.1 kHz / Stereo / no-CRC byte-for-byte),
  `emit_bytes_round_trips_every_bitrate_sample_rate_mode_combo`
  (walks the full 14 × 3 × 4 matrix: 120 allowed cells round-trip
  byte-for-byte; 48 disallowed cells reject with the expected
  variant), `emit_bytes_walks_all_mode_extensions_and_emphases`
  (all four `mode_extension` codes + all three `emphasis` values),
  `emit_bytes_rejects_unsupported_bitrate_and_sample_rate`
  (200 kbit/s + 22 050 Hz LSF),
  `emit_bytes_rejects_disallowed_bitrate_mode_pair`
  (32 kbit/s + Stereo),
  `emit_bytes_sets_syncword_id_and_layer_bits_correctly` (the three
  fixed §2.4.2.3 bits in their right positions), and
  `emit_bytes_padding_and_protection_bit_flip_correctly` (per-bit
  + frame-size delta when padding flips). Test count moves from 98
  to 107 (+9, all green).

### Added (round 150 — clean-room rebuild, step 6)

- **§2.4.1.6 / §2.4.3.1 / §2.4.3.2 frame-level decode loop**
  (`frame` module): `decode_frame(buf) -> DecodedFrame` parses one
  complete Layer II frame from the front of a buffer, drives the
  §2.4.1.6 `for (gr=0..12, sb=0..sblimit, ch=0..nch)` triplet loop
  through `requant::read_triplet`, applies the §2.4.3.3.3 rescaling
  with the §2.4.2.3 `scalefactor_granule = sample_granule / 4`
  partition (3 scalefactor-granules of 4 sample-granules each), and
  pushes the resulting 36 successive 32-vectors of subband samples
  per channel through a per-channel `SynthesisFilterbank` to emit
  `12 × 3 × 32 = 1152` PCM samples per channel (§2.4.2.1 "1 152 for
  Layer II"). When `protection_bit == 0` the §2.4.3.1 CRC-16 over
  Annex B Table B.5's protected region (header bits 16…31 + alloc +
  scfsi) is verified via `crc16_layer2`; mismatches raise
  `FrameError::CrcMismatch`. A per-stream `FrameDecodeState` threads
  the polyphase filterbank's V ring buffer across successive frames
  per Annex A Figure A.2 footnote 1 (`FrameDecodeState::reset()`
  re-zeroes V for seek / discontinuity). The convenience
  `decode_all_frames(buf)` chains frames until the buffer is
  exhausted. The staged 31-frame stereo fixture at
  `docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/input.mp3`
  (192 kbit/s, 44.1 kHz stereo, B.2a sub-table, mode_extension=0)
  decodes cleanly end-to-end — every one of the
  `2 × 31 × 1152 = 71 424` PCM samples is finite + bounded in
  `[-4, +4]` (the §2.4.3.4.7.1 nominal range is `[-1, +1]`).
- **Supporting API** in `audio_data`:
  `parse_audio_data_with_section_bits(header, reader)` returns
  `(AudioData, alloc_bits, scfsi_bits)` so the frame-level decode
  loop can compute the §2.4.3.1 CRC over exactly the bits the
  §2.4.1.6 syntax just consumed without re-parsing.
- **Crate `Error::Frame(FrameError)`** wrapper exposed at the top
  level alongside the existing `Error::Header` / `Error::AudioData`
  variants; the historical `Error::NotImplemented` is now reserved
  for the encoder path.
- **10 new unit tests** (98 total): first-frame decode of the
  staged 192 kbit/s stereo fixture has the expected `(sample_rate,
  bit_rate, mode, protection_bit)` and 2 × 1152 finite PCM samples;
  second-frame chaining survives the padded-vs-unpadded `frame_size`
  difference (626 vs 627 bytes); `decode_all_frames` produces
  `31 × 1152` finite, non-trivially-loud samples per channel;
  CRC-mismatch detection on a synthetic protected frame; truncated-
  buffer rejection via `FrameError::Truncated`; `reset()` survives
  a subsequent decode; channel filterbanks evolve independently
  across successive `decode_frame_with` calls (same-state ≠
  fresh-state); `PCM_SAMPLES_PER_CHANNEL = 12 × 3 × 32 = 1152`
  identity; `compute_layer2_crc` byte-aligned-extraction helper
  agrees with the public `crc16_layer2`; `layer2_crc` re-export
  matches `crc::crc16_layer2`.

### Added (round 147 — clean-room rebuild, step 5)

- **§2.4.3.2 / §2.4.3.3.5 polyphase synthesis subband filter**
  (`synthesis` module) — one Annex A Figure A.2 invocation consumes
  one 32-vector of subband samples and produces 32 reconstructed PCM
  samples in the §2.4.3.4.7.1 nominal `[-1, +1]` range, while the
  1024-entry V ring buffer evolves through the `shift V` →
  `matrix V` → `build U` → `window W = U * D` →
  `S_j = Σ_{i = 0..16} W[j + 32 * i]` pipeline.
  - `SynthesisFilterbank::new()` precomputes the 64 × 32 `N_ik`
    matrix from the §2.4.3.3.5 closed form
    `N_ik = cos[(16 + i)(2k + 1) π / 64]` and seeds V with zeros per
    Annex A Figure A.2 footnote 1.
  - `SynthesisFilterbank::push_subbands(&[f64; 32], &mut [f64; 32])`
    is the per-channel decode primitive — one call per slot of 32
    subband samples.
  - `SynthesisFilterbank::reset()` re-zeroes V for cold restarts.
- **Annex B Table 3-B.3 "Coefficients D[i] of the synthesis window"**
  (`tables_synthesis::D`) — all 512 signed coefficients transcribed
  verbatim from the staged PDF (full-PDF pages 50-52, rendered as
  `docs/audio/mp3/annex-b-renders/Table-B.3-coefficients-Di-p56..58.png`).
  Extraction used `pdftotext -layout` with residual OCR artefacts
  resolved by direct visual cross-check against the PNG renders; the
  table satisfies the documented anti-mirror magnitude identity
  `|D[256 + k]| == |D[256 - k]|` (for `k != 0`) and exhibits the
  seven §2.4.3.3.5 sign-block boundaries at indices 64, 128, 192,
  256, 320, 384, 448 (asserted by unit tests).
- **16 new unit tests** (88 total): the §2.4.3.3.5 N_ik matrix
  matches the closed form for every `(i, k)`, hits the algebraic
  landmarks `N[0, 0] = N[0, 4] = √2/2` and `N[16, 0] = 0`, is
  bounded by 1 in magnitude; the V buffer starts at zero, stays at
  zero across a full 16-frame cycle of zero input, is correctly
  re-zeroed by `reset()`; a single-subband impulse propagates for at
  most 16 frames and then drops exactly to zero (the
  Annex A Figure A.2 V-buffer depth); the filterbank's output stays
  finite + bounded under a realistic single-subband excitation; two
  independent instances given different inputs produce different
  outputs; the Annex B Table 3-B.3 `D` array has size 512, hits the
  PDF-page-50/51/52 endpoint readings (`D[0] = 0`,
  `D[69] = D[70] = 0.003479004`, `D[256] = 1.144989014`,
  `D[442] = D[443] = -0.003479004`), peaks at index 256, has the
  magnitude anti-mirror identity around 256, and shows the seven
  documented sign-block boundaries.

The `synthesis` module closes the last §2.4.3 primitive blocking a
Layer II decode path. With `audio_data` (§2.4.1.6 side-info parser),
`requant` (§2.4.3.3.4 sample requantizer), `synthesis` (§2.4.3.2 /
§2.4.3.3.5 filterbank), and `crc` (§2.4.3.1 protected-field CRC-16)
all in place, the next step is the §2.4.1.6 / §2.4.3.1 frame-level
decode loop that wires the existing primitives into the registry
contract.

### Added (round 142 — clean-room rebuild, step 4)

- **§2.4.1.4 / §2.4.3.1 CRC-16** (`crc` module): the Layer II
  protected-field CRC-16 from ISO/IEC 11172-3 (1993) §2.4.3.1 is
  implemented as a primitive shared by encoder and decoder.
  - `G(X) = X^16 + X^15 + X^2 + 1` (PDF page 36, rendered as
    `docs/audio/mp3/mp1-crc-polynomial-iso11172-3-eq.png`) with the
    §2.4.3.1 `'1111 1111 1111 1111'` (`0xFFFF`) initial shift-register
    state. The tap mask is `0x8005` (the polynomial minus its X^16
    term, with bits at positions 15, 2, 0).
  - `crc16_step(reg, bit) -> u16` runs one feedback-shift step.
  - `crc16_update_bits(reg, value, nbits) -> u16` feeds the low
    `nbits` of `value` MSB-first — useful for variable-width fields
    (one `allocation[ch][sb]` of width `nbal ∈ {2, 3, 4}` etc.).
  - `crc16_update_packed(reg, &[u8], nbits) -> u16` consumes a
    left-aligned packed bitstream of arbitrary bit length.
  - `crc16_layer2(header_high, header_low, &allocation_and_scfsi,
    bits) -> u16` is the Layer II entry point: it feeds the two
    header bytes (bits 16…31, per Annex B Table B.5 transcribed in
    `docs/audio/mp3/mp1-crc-iso-extracts.md`) followed by the
    variable-length bit-allocation + scfsi payload.
  - `verify_layer2_crc(expected, ..) -> bool` is the decoder-side
    accept/reject helper.
- **13 new unit tests** (72 total): the production step matches an
  independently-derived long-form GF(2) reference across a spread of
  register values × both input bits; the per-bit / bit-width /
  packed-buffer APIs agree on the same single-byte input; partial-byte
  feeds; mid-byte stream-splitting equivalence; the Layer II entry
  point equals manual streaming; verify accepts the round-trip and
  rejects every single-bit-flipped expected value; every single bit
  in the protected region flips the CRC; every contiguous burst of
  length 1..=16 (= the polynomial degree) is detected; the
  empty-payload header-only degenerate case; the polynomial constants
  (`INIT_STATE`, `TAPS`) are pinned per spec; zero-width updates are
  no-ops; `crc16_update_packed` panics on a short buffer.

### Added (round 133 — clean-room rebuild, step 3)

- **§2.4.3.3.4 sample requantizer** (`requant` module): turns a Layer II
  bitstream sample code into a normalized fractional value `s''`.
  - `requantize_code(&QuantClass, code) -> f64` performs the §2.4.3.3.4
    steps for one sample: invert the first (MSB) bit, read the result as
    an `n`-bit two's complement fractional number (MSB weight −1), then
    apply the linear formula `s'' = C * (s''' + D)` with the Table 3-B.4
    `C` / `D` constants. `n = QuantClass::bits_per_sample()`.
  - `degroup(&QuantClass, combined) -> [u32; 3]` runs the §2.4.3.3.4
    radix-`nlevels` degrouping (`s[i] = c % nlevels; c = c DIV nlevels;`)
    for the grouped classes (`nb_steps ∈ {3, 5, 9}`).
  - `read_triplet(&QuantClass, &mut BitReader) -> [f64; 3]` reads one
    (subband, granule) triplet — one combined codeword for a grouped
    class, three separable codes otherwise — and requantizes all three.
  - `requantize_scaled(&QuantClass, scalefactor_index, &mut BitReader)`
    layers the §2.4.3.3.3 Table 3-B.1 rescaling (`s' = factor * s''`)
    on top; reserved scalefactor index 63 is rejected via the new
    `RequantError::ReservedScalefactorIndex`.
- **`QuantClass::bits_per_sample()`**: `ceil(log2(nb_steps))` — the
  width of a single degrouped sample code (equals `bits_per_codeword`
  for ungrouped classes; strictly narrower for the three grouped ones).
- **13 new unit tests** (59 total): the 3-level class produces the
  expected symmetric `{−C/2, 0, +C/2}` levels; a from-prose reference
  requantizer cross-checks `requantize_code` for every Table 3-B.4
  class (exhaustive for the narrow classes, sampled for the wide ones);
  requantized magnitudes stay within `C*(1+D)`; degrouping matches the
  radix-`nlevels` decomposition (exhaustive for `nb_steps` 3 and 5);
  `read_triplet` reads the right bit count for grouped vs separable
  classes and round-trips through `requantize_code`; `requantize_scaled`
  applies the Table 3-B.1 unity / doubling multipliers and rejects index
  63; the `UnexpectedEnd` short-buffer path; and the `C` / `D` constants
  are pinned independently of the `bitalloc` table test.

### Added (round 129 — clean-room rebuild, step 2)

- **§2.4.1.6 audio-data side info** (`audio_data` module): the
  bit-allocation loop (§2.4.3.3.1), scalefactor-selection-information
  loop (§2.4.3.3.2), and scalefactor decode loop (§2.4.3.3.3) are
  parsed end-to-end into a typed `AudioData` struct
  (`parse_audio_data(&FrameHeader, &mut BitReader) ->
  Result<AudioData, AudioDataError>`). The joint-stereo
  `[bound, sblimit)` "intensity-stereo subbands share one allocation
  across both channels" branch is honoured; the four §2.4.2.3 scfsi
  schedules (`'00'/'01'/'10'/'11'`) are typed by `Scfsi` and expand
  the 1, 2, or 3 on-wire 6-bit `scalefactor[ch][sb][part]` indices
  across the three granules per the spec. Reserved scalefactor index
  63 is rejected with a distinct `AudioDataError::ReservedScalefactorIndex`
  variant.
- **Annex B Tables 3-B.2a..d + 3-B.4** (`bitalloc` module):
  the four B.2 sub-tables ("Possible quantization per subband",
  PDF pages 46-49) are transcribed verbatim with their `sblimit`
  (27 / 30 / 8 / 12), per-subband `nbal` widths, and per-subband
  index-to-`nb_steps` rows. `BitAllocTable::nb_steps(sb, index)`
  performs the lookup; `select_table(header)` picks the active
  sub-table per the §2.4.2.3 `(sample_rate, per-channel bitrate)`
  rule. Table 3-B.4 (PDF page 50) is exposed via
  `class_of_quantization(nb_steps) -> Option<QuantClass>` carrying
  the `C` / `D` requantization constants, the `grouping` flag (with
  the §2.4.2.3 "value is 3, 5, or 9" rule cross-checked against
  `is_grouped`), and the codeword shape (`samples_per_codeword`,
  `bits_per_codeword`).
- **25 new unit tests** (46 total) cover the per-`sblimit` /
  per-`nbal` layout of every B.2 sub-table (cross-checked against
  the PDF footer sum-of-nbal totals: 88 / 94 / 26), per-row
  index→`nb_steps` round-trips against the literal PDF rows, the
  four `(sample_rate, bitrate)` table-selection branches, Table
  3-B.4 spot lookups against PDF page 50, every B.2 cell resolving
  to a known B.4 class, the four scfsi expansion schedules, joint-
  stereo allocation sharing above `bound`, the zero-allocation skip
  path, reserved scalefactor index 63 rejection, and the bit-budget
  identity `allocation_bits == 2 * Σ nbal` for stereo.
- **`oxideav_core::bits::BitReader`** is now a dependency surface
  (re-used as the §2.4.1.6 reader).

### Fixed

- The lib.rs doc-comment for `tables::SCALEFACTORS` listed the
  closed-form as `2^((1 − i) / 3)`; it is `2^((3 − i) / 3)` (the
  table itself was correct — only the docstring was off).

### Added (round 126 — clean-room rebuild, step 1)

- **MPEG-1 Layer II frame header** (`header` module): the §2.4.1.3
  32-bit Layer II header is parsed into a typed `FrameHeader` struct.
  The §2.4.2.3 validation is enforced up-front — bad sync, LSF
  (`ID == 0`), non-Layer-II frames, the forbidden / free-format /
  reserved table codes, the reserved emphasis `'10'`, and the
  §2.4.2.3 disallowed `(bitrate, mode)` combinations are all rejected
  with distinct `HeaderError` variants. The Layer II `bitrate_index`
  ladder (32 … 384 kbit/s) and the `sampling_frequency` table (44.1 /
  48 / 32 kHz) are decoded directly from the ISO/IEC 11172-3 PDF page
  21 transcription embedded in the module docstring. The
  `mode_extension` `bound` mapping `bound = (mode_extension + 1) × 4`
  is exposed via `ModeExtension::bound()`.
- **`FrameHeader::frame_size_bytes()`** computes
  `floor(144 · bit_rate / sample_rate) + padding_bit` per §2.4.3.1,
  with the §2.4.2.1 "one byte per Layer II slot" identity. The
  spec-known three-way (44.1 kHz / 48 kHz / 32 kHz) cases are
  unit-tested.
- **`find_sync`** locates the byte-aligned 12-bit §2.4.3.1 syncword in
  a buffer.
- **Annex B Table 3-B.1** "Layer I, II scalefactors" (`tables` module):
  the 63 multipliers used by §2.4.3.3.3 to rescale requantized samples
  are transcribed verbatim from PDF page 51 (`docs/audio/mp3/mp1-
  annex-b-iso-extracts.md`) and self-checked against the closed-form
  `scalefactor[i] = 2^((3 − i) / 3)`. Note: the markdown extract
  records this relation as `2^((1 − i) / 3)`, which does not reproduce
  the tabulated values; the PDF table is the authoritative reading
  (see README "Spec note recorded").
- **21 unit tests** cover the bitrate / sampling-frequency ladders,
  the §2.4.2.3 disallowed bitrate/mode matrix (rejection + matching
  allow paths), every emphasis / mode / mode-extension code, sync
  search (positive + negative), short-buffer rejection, LSF rejection,
  the reserved-`'00'` layer rejection, the protection-bit zero path,
  frame sizing at three canonical (bitrate, Fs) points (padding on /
  off), and the Table 3-B.1 closed-form / endpoint / monotonicity /
  exact-power-of-two cross-checks.

### Unchanged

- `register()` remains a no-op; the §2.4.1.6 / §2.4.3.3 audio-data
  decode path (bit allocation per Tables 3-B.2a..d, scfsi,
  scalefactor triples, requantization per Table 3-B.4, polyphase
  synthesis per Table 3-B.3, plus the §2.4.1.4 / §2.4.3.1 CRC-16) is
  the next coherent step and lands in a subsequent round.

### Erased (2026-05-24)

- Prior master history was force-erased on **2026-05-24** under
  Hat-3 cold enforcement of the workspace clean-room policy
  (`docs/IMPLEMENTOR_ROUND.md`). The retired implementation's
  bit-allocation and synthesis-window data tables carried module
  doc-comments stating the tables were transcribed from external
  library source rather than derived solely from the ISO/IEC
  specification.
