# oxideav-mp2

A pure-Rust **MPEG-1 Audio Layer II** (MP2 / MUSICAM) codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Clean-room rebuild in progress (round 126, 2026-05-25).** The prior
implementation was retired under the workspace
[clean-room policy](https://github.com/OxideAV/oxideav-workspace/blob/master/docs/IMPLEMENTOR_ROUND.md):
the provenance of its bit-allocation and synthesis-window data tables
could not be defended as clean-room — module doc-comments recorded that
those tables had been transcribed from external library source rather
than derived solely from the ISO/IEC specification, which violates the
clean-room provenance requirement. Master history was fully erased per
the Hat-3 cold-enforcement procedure on 2026-05-24.

The rebuild reads numeric tables **only** from ISO/IEC 11172-3 (1993),
the staged 157-page edition with Annex B
(`docs/audio/mp3/ISO_IEC_11172-3-MP3-1993.pdf`, SHA-256
`ef67bbc34eaab825e804bb87835c0cc0cd9ae6c7f77d3cec64d779726ffe322d`),
and the accompanying clean-room markdown extracts under
`docs/audio/mp3/`.

## What works today

The crate parses and validates the MPEG-1 Layer II **frame header** and
exposes the Annex B Table 3-B.1 scalefactor multipliers, all derived
solely from the ISO/IEC 11172-3 PDF:

- **32-bit frame header** (§2.4.1.3 / §2.4.2.3): all thirteen fields
  decoded into the typed [`FrameHeader`] struct. §2.4.2.3 validation is
  enforced up-front: the 12-bit syncword (`0xFFF`), `ID == 1` (MPEG-1),
  and `layer == '10'` (Layer II) are checked first, then the Layer II
  `bitrate_index` ladder (32 / 48 / 56 / 64 / 80 / 96 / 112 / 128 / 160
  / 192 / 224 / 256 / 320 / 384 kbit/s) and the `sampling_frequency`
  table (44.1 / 48 / 32 kHz) are decoded with the forbidden (`'1111'`),
  free-format (`'0000'`), and reserved (`'11'`) codes rejected
  explicitly. The §2.4.2.3 "For Layer II, not all combinations of total
  bitrate and mode are allowed" matrix is enforced (32/48/56/80 kbit/s
  = single_channel only; 224/256/320/384 kbit/s = no single_channel),
  and the `emphasis` `'10'` reserved code is rejected. The mode
  extension's `bound` mapping `bound = (mode_extension + 1) × 4` is
  exposed via `ModeExtension::bound()`.
- **Frame sizing** (§2.4.3.1 + §2.4.2.1): `frame_size_bytes()` returns
  `floor(144 · bitrate / Fs) + padding_bit` directly in bytes (Layer II
  uses one byte per slot, so the slot count equals the byte count).
- **Sync search** (§2.4.3.1): `find_sync` locates the byte-aligned
  12-bit syncword in a buffer for cold synchronisation.
- **Annex B Table 3-B.1** "Layer I, II scalefactors": the 63
  multipliers used by §2.4.3.3.3 to rescale requantized samples are
  tabulated in `tables::SCALEFACTORS` and self-checked against the
  closed-form `scalefactor[i] = 2^((3 − i) / 3)`.

21 unit tests cover the bitrate / sampling-frequency ladders end-to-end,
sync detection (positive + negative paths), the §2.4.2.3 disallowed
bitrate/mode combinations (rejection + the matching allow paths), every
emphasis value (including the reserved-`'10'` rejection), every mode
and mode-extension code, the LSF-rejection path, short-buffer rejection,
the reserved-`'00'` layer rejection, frame-size calculation at three
canonical (bitrate, sample-rate) points (with padding on / off), and
the Table 3-B.1 scalefactor closed-form / endpoint / monotonicity /
exact-power-of-two cross-checks.

## What does not work yet

`register()` is a no-op until the §2.4.1.6 / §2.4.3.3 audio-data decode
path lands: bit-allocation tables 3-B.2a..d "Possible quantization per
subband" (§2.4.3.3.1), per-subband `scfsi` decoding (§2.4.3.3.2),
1..3 scalefactor indices per subband per `scfsi` schedule (§2.4.3.3.3),
sample requantization per Table 3-B.4 "Layer II classes of quantization"
(§2.4.3.3.4) including the `samplecode` triplet de-grouping path, and
the §2.4.3.2 polyphase synthesis filterbank driven by Table 3-B.3
"Coefficients D[i] of the synthesis window" (rendered as PNG pages
56-58 under `docs/audio/mp3/annex-b-renders/`). The §2.4.1.4 / §2.4.3.1
CRC-16 over the Table 3-B.5 protected fields (header bits 16…31 + bit
allocation + scfsi, per `docs/audio/mp3/mp1-crc-iso-extracts.md`) is
likewise a followup. Encoder + the ISO/IEC 13818-3 §2.4.2.3 LSF Layer
II ladder (16 / 22.05 / 24 kHz) are subsequent followups.

## Spec note recorded

The markdown extract `docs/audio/mp3/mp1-annex-b-iso-extracts.md`
documents the Table 3-B.1 closed-form as `2^((1 − i) / 3)`. That
formula does not reproduce the tabulated values (it gives
`scalefactor[0] ≈ 1.260` instead of the tabulated `2.000`). The PDF
table itself is correct and is the authoritative reading; the
verifier formula in the markdown is off by 2 in the exponent
numerator and should be `2^((3 − i) / 3)`. The crate's
`SCALEFACTORS` array is keyed on the PDF; only the doc's verifier
formula is in error.

## License

MIT — see [LICENSE](./LICENSE).
