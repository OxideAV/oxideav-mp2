# oxideav-mp2

A pure-Rust **MPEG-1 Audio Layer II** (MP2 / MUSICAM) codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

**Clean-room rebuild in progress (round 150, 2026-05-26).** The prior
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
- **§2.4.1.3 / §2.4.2.3 header writer** (encoder side): the inverse
  of `FrameHeader::parse` is `FrameHeader::emit_bytes(&self) ->
  Result<[u8; 4], HeaderError>`. The 4-byte big-endian word it
  produces is bit-exact identical to what `parse` would read back —
  syncword (bits 31..20 = `0xFFF`), `ID = '1'` (MPEG-1), `layer =
  '10'` (Layer II), `protection_bit`, the §2.4.2.3 ladder code from
  `encode_bitrate`, the §2.4.2.3 table code from
  `encode_sampling_frequency`, then padding / private / mode /
  mode_extension / copyright / original / emphasis. Encoder-side
  validation mirrors the decoder: `bit_rate` outside the 14-row Layer
  II ladder is rejected as `HeaderError::UnsupportedBitrate(rate)`,
  `sample_rate ∉ {32000, 44100, 48000}` Hz as
  `HeaderError::UnsupportedSamplingFrequency(rate)`, and the
  §2.4.2.3 disallowed (bitrate, mode) matrix as
  `HeaderError::DisallowedBitrateModeCombination`. Lookup order
  reports the most-specific error first (off-ladder bitrate is
  flagged before the matrix). The two §2.4.2.3 reserved codes —
  `sampling_frequency = '11'` and `emphasis = '10'` — cannot be
  produced because the type system has no corresponding values.
- **Annex B Table 3-B.1** "Layer I, II scalefactors": the 63
  multipliers used by §2.4.3.3.3 to rescale requantized samples are
  tabulated in `tables::SCALEFACTORS` and self-checked against the
  closed-form `scalefactor[i] = 2^((3 − i) / 3)`.
- **Annex B Tables 3-B.2a..d** "Possible quantization per subband" and
  **Table 3-B.4** "Layer II classes of quantization"
  (`bitalloc` module): the four B.2 sub-tables are transcribed from
  PDF pages 46-49 with their `sblimit` (27 / 30 / 8 / 12), per-subband
  `nbal` widths (4 / 3 / 2 bits), and per-subband index→`nb_steps`
  mappings. `select_table` picks the active sub-table from the
  §2.4.2.3 `(sample_rate, per-channel bitrate)` rule. Table 3-B.4
  rows are exposed as `QuantClass` (the `C` / `D` requantization
  constants plus `grouping`, `samples_per_codeword`, and
  `bits_per_codeword` columns); `is_grouped` materialises the
  §2.4.2.3 "value is 3, 5, or 9" grouping check.
- **§2.4.1.6 audio-data side info** (`audio_data` module): the
  bit-allocation loop (with per-subband `nbal` widths and the
  joint-stereo "intensity-stereo subbands share one allocation"
  branch above `bound`), the §2.4.3.3.2 scfsi loop (only over
  allocation-non-zero (ch, sb) pairs; 2-bit field decoded into the
  `Scfsi` enum), and the §2.4.3.3.3 scalefactor loop (1, 2, or 3
  on-wire 6-bit indices expanded across the three granules per the
  §2.4.2.3 scfsi schedule) are parsed end-to-end into a typed
  `AudioData` struct using `oxideav_core::bits::BitReader`.
- **§2.4.3.3.4 sample requantization** (`requant` module): a bitstream
  sample code is turned into a normalized fractional value `s''`.
  `requantize_code` inverts the first (MSB) bit, reads the result as an
  `n`-bit two's complement fractional number (MSB weight −1), and
  applies `s'' = C * (s''' + D)` with the Table 3-B.4 constants;
  `degroup` runs the radix-`nlevels` splitting for the grouped classes
  (`nb_steps ∈ {3, 5, 9}`); `read_triplet` reads one (subband, granule)
  triplet (one combined codeword when grouped, three separable codes
  otherwise); and `requantize_scaled` layers the §2.4.3.3.3 Table 3-B.1
  rescaling (`s' = factor * s''`) on top. `QuantClass::bits_per_sample`
  exposes the single-sample width `ceil(log2(nb_steps))`.
- **§2.4.1.4 / §2.4.3.1 CRC-16** (`crc` module): the
  `G(X) = X^16 + X^15 + X^2 + 1` feedback shift register with the
  §2.4.3.1 `'1111 1111 1111 1111'` (`0xFFFF`) initial state is
  implemented as a primitive shared by encoder and decoder. The Annex B
  Table B.5 Layer II protected-field set (header bits 16…31 + bit
  allocation + scfsi, per `docs/audio/mp3/mp1-crc-iso-extracts.md`) is
  wrapped by `crc16_layer2(header_high, header_low, allocation_and_scfsi,
  bit_count) -> u16` and `verify_layer2_crc(expected, ..) -> bool`. The
  per-bit / per-field / per-packed-buffer primitives `crc16_step`,
  `crc16_update_bits`, and `crc16_update_packed` are exposed so the
  encoder can stream the variable-length §2.4.1.6 allocation + scfsi
  payload into the running CRC without a temporary materialised buffer.
- **§2.4.3.2 / §2.4.3.3.5 polyphase synthesis filterbank**
  (`synthesis` + `tables_synthesis` modules): one Annex A Figure A.2
  invocation consumes one 32-vector of subband samples (output of
  `requant::read_triplet`) and produces 32 reconstructed PCM samples
  in the §2.4.3.4.7.1 nominal `[-1, +1]` range, driving the 1024-entry
  V ring buffer through the documented `shift V` → `matrix V` →
  `build U` → `window W = U * D` → `S_j = Σ_{i=0..16} W[j + 32i]`
  pipeline. The 64×32 `N_ik` matrix is precomputed at construction
  from the §2.4.3.3.5 closed form `N_ik = cos[(16 + i)(2k + 1) π/64]`;
  the 512 D[i] window coefficients are transcribed verbatim from
  Annex B Table 3-B.3 (PDF pages 50-52, rendered as
  `docs/audio/mp3/annex-b-renders/Table-B.3-coefficients-Di-p56..58.png`)
  with the global peak `D[256] = 1.144989014` and the
  anti-mirror identity `|D[256 + k]| = |D[256 - k]|` checked by
  unit tests. `SynthesisFilterbank::push_subbands(&[f64; 32], &mut
  [f64; 32])` is the per-channel decode primitive; `reset()` re-seeds
  V with zeros for cold restarts per Figure A.2 footnote 1.
- **§2.4.1.6 / §2.4.3.1 / §2.4.3.2 frame-level decode loop**
  (`frame` module): `decode_frame(buf) -> DecodedFrame` parses one
  Layer II frame end-to-end. The §2.4.3.1 CRC slot is verified via
  `crc16_layer2` when `protection_bit == 0` (mismatch raises
  `FrameError::CrcMismatch`). The §2.4.1.6 `for (gr=0..12, sb=0..sblimit,
  ch=0..nch)` triplet loop drives `requant::read_triplet`, applies the
  §2.4.3.3.3 rescaling with the scfsi-expanded scalefactor selected by
  `scalefactor_granule = sample_granule / 4` (§2.4.2.3 "scalefactors
  are transmitted for groups of 12 subband samples"), buffers
  `12 × 3 × 32 = 1152` subband samples per channel, and pushes 36
  successive 32-vectors through a per-channel `SynthesisFilterbank`
  to produce 1152 PCM samples per channel (§2.4.2.1 "1 152 for Layer
  II"). A per-stream `FrameDecodeState` threads the polyphase
  filterbank's V ring buffer across successive frames per Annex A
  Figure A.2 footnote 1; `decode_all_frames(buf)` chains frames
  until the buffer is exhausted. The staged 31-frame stereo fixture
  `docs/audio/mp3/fixtures/layer2-stereo-44100-192kbps/input.mp3`
  (192 kbit/s, 44.1 kHz, B.2a) decodes cleanly end-to-end —
  `2 × 31 × 1152 = 71 424` finite PCM samples in
  `[-4, +4]` (the §2.4.3.4.7.1 nominal range is `[-1, +1]`).

107 unit tests cover the bitrate / sampling-frequency ladders
end-to-end, sync detection (positive + negative paths), the §2.4.2.3
disallowed bitrate/mode combinations (rejection + the matching allow
paths), every emphasis value (including the reserved-`'10'`
rejection), every mode and mode-extension code, the LSF-rejection
path, short-buffer rejection, the reserved-`'00'` layer rejection,
frame-size calculation at three canonical (bitrate, sample-rate)
points (with padding on / off), the Table 3-B.1 scalefactor
closed-form / endpoint / monotonicity / exact-power-of-two
cross-checks, the per-`sblimit` / per-`nbal` layout of every B.2
sub-table (with sum-of-nbal cross-checks against the PDF footer
totals: 88, 94, 26), per-row index→`nb_steps` round-trips against
the literal PDF rows, the four `(sample_rate, bitrate)`
table-selection branches, Table 3-B.4 spot lookups against PDF
page 50, every B.2 cell resolving to a known B.4 class, the four
scfsi expansion schedules (`'00'/'01'/'10'/'11'` → `[a,b,c]`,
`[a,a,c]`, `[a,a,a]`, `[a,c,c]`), joint-stereo allocation sharing
above `bound`, the zero-allocation skip path, the reserved
scalefactor-index-63 rejection, the bit-budget identity
(allocation bits = `2 × Σ nbal` for stereo), the 3-level symmetric
requantizer levels, a from-prose reference requantizer cross-checked
against `requantize_code` for every Table 3-B.4 class, the radix-
`nlevels` degrouping (exhaustive for `nb_steps` 3 and 5), the
grouped-vs-separable `read_triplet` bit counts, the Table 3-B.1
unity / doubling rescaling, the §2.4.3.1 CRC-16 polynomial pinned
against an independently-derived long-form GF(2) reference (spread of
register values × both input bits), the three CRC update APIs
(per-bit / bit-width / packed-buffer) agreeing on the same input,
mid-byte stream-splitting equivalence, the empty-payload
header-only degenerate exercise, single-bit error detection across
every bit of the §2.4.3.1 protected region, contiguous burst
detection up to the polynomial degree (16 bits), and the
verify-accepts-roundtrip / verify-rejects-mismatch property; plus the
Annex B Table 3-B.3 `D` array size (512), the PDF-page-50/51/52
endpoint readings (`D[0] = 0`, `D[69] = D[70] = 0.003479004`,
`D[256] = 1.144989014`, `D[442] = D[443] = -0.003479004`), the
peak-at-256 property, the magnitude anti-mirror identity around 256,
and the seven §2.4.3.3.5 sign-block flip boundaries (64, 128, 192,
256, 320, 384, 448); the precomputed `N_ik` matrix matches the
`cos[(16 + i)(2k + 1) π/64]` closed form for every `(i, k) ∈
[0, 64) × [0, 32)`, hits the algebraic landmarks `N[0, 0] = N[0, 4] =
√2/2` and `N[16, 0] = 0`, and is bounded by 1 in magnitude; the
filterbank's V buffer starts at zero, stays at zero under zero input
across at least one full V cycle (16 frames of 64), is correctly
re-zeroed by `reset()`, propagates a single-subband impulse for at
most 16 frames then drops exactly to zero, stays finite + bounded
under a realistic single-subband excitation, and two independent
instances given different inputs produce different outputs; plus
encoder-side: `encode_bitrate` round-trips every Layer II ladder
code 1..14 against `decode_bitrate` (and rejects off-ladder /
non-kbps inputs), `encode_sampling_frequency` round-trips all three
sf codes (and rejects LSF 16/22.05/24 kHz and reserved
sample-rates), `FrameHeader::emit_bytes` round-trips the canonical
192 kbit/s / 44.1 kHz / Stereo header byte-for-byte, walks every
allowed cell of the (14 bitrate × 3 sf × 4 mode) §2.4.2.3 matrix
(120 allowed / 48 rejected per the matrix), exercises all four
`mode_extension` codes and all three `emphasis` values, rejects
unsupported bitrate / sample-rate / disallowed (bitrate, mode), and
emits the spec-mandated syncword `0xFFF` + ID `'1'` + layer `'10'`
with the padding and protection bits in the correct positions.

## What does not work yet

`register()` remains a no-op until the codec is wired through
`oxideav_core`'s `Decoder` trait surface (registry-level integration
contract). The bottom-up decoder primitive `decode_frame(buf)` is
already callable; the remaining wiring is the framework's
`Decoder for Mp2Decoder` adapter plus a tag declaration.

Bit-exact PCM-against-reference validation (PSNR / SNR vs the
`expected.wav` next to the staged fixture, or against ffmpeg /
mpg123 black-box validator output) is pending an Auditor round.

Encoder is partially staged: the §2.4.1.4 / §2.4.3.1 CRC-16 write
primitives (`crc16_layer2` + streaming `crc16_update_*`) and the
§2.4.1.3 / §2.4.2.3 header writer (`FrameHeader::emit_bytes` +
`encode_bitrate` + `encode_sampling_frequency`) are in place. The
remaining encoder pieces are the Annex C polyphase analysis
filterbank (`forward_polyphase_analysis`, symmetric to the
existing synthesis path), §2.4.3.3.3 scalefactor extraction +
Table A.2 SCFSI selection, the iterative §C.1.5.2.7 bit-allocator,
and the §2.4.1.6 audio-data writer that ties everything together.

The ISO/IEC 13818-3 §2.4.2.3 LSF Layer II ladder (16 / 22.05 /
24 kHz) is a subsequent followup (gated on docs gap #1076).

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
