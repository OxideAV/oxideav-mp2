# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate adheres
to [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
