# oxideav-mp2

A pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
codec for the
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
ISO/IEC 13818-3 (1997)
(`docs/audio/mp3/ISO_IEC_13818-3-MPEG2-audio-1997.pdf`, for the
§2.4.2.3 low-sampling-rate header tables and Annex B Table B.1
bit-allocation table), and the accompanying clean-room markdown
extracts under `docs/audio/mp3/`.

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
  `BitAllocTable::allocation_index(sb, nb_steps) -> Option<u32>` is
  the encoder-side inverse of `nb_steps(sb, index)`: it returns the
  `nbal`-bit `allocation[ch][sb]` field code that the decoder would
  read back into the given `nb_steps`, accepting the §2.4.2.3 "no
  bits allocated" sentinel `nb_steps == 0` (which always maps to
  index 0) and rejecting both `sb ≥ sblimit` and off-row `nb_steps`
  values (the encoder cannot emit a `nb_steps` that does not appear
  in the row for the targeted subband).
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

321 lib tests + 6 LSF integration tests + 14 malformed-input
property tests cover the MPEG-1 + LSF bitrate / sampling-frequency
ladders end-to-end (decoding and encoding inverses cross-checked
across all 14 × 3 = 42 LSF cells and all 168 LSF (bitrate, mode)
round-trips), sync detection (positive + negative paths), the
§2.4.2.3 MPEG-1 disallowed bitrate/mode combinations (rejection +
the matching allow paths), every emphasis value (including the
reserved-`'10'` rejection), every mode and mode-extension code,
the LSF-acceptance path (`ID == 0` now parses successfully and
routes to the §13818-3 §2.4.3.1 Table B.1 path), short-buffer rejection, the reserved-`'00'` layer rejection,
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
MPEG-1 sf codes (and rejects LSF 16/22.05/24 kHz at the MPEG-1
encoder + reserved sample-rates); the symmetric
`encode_bitrate_lsf` / `encode_sampling_frequency_lsf` pair
covers the §13818-3 §2.4.2.3 LSF ladders (8..160 kbit/s and
16/22.05/24 kHz), `FrameHeader::emit_bytes` round-trips the canonical
192 kbit/s / 44.1 kHz / Stereo header byte-for-byte, walks every
allowed cell of the (14 bitrate × 3 sf × 4 mode) §2.4.2.3 matrix
(120 allowed / 48 rejected per the matrix), exercises all four
`mode_extension` codes and all three `emphasis` values, rejects
unsupported bitrate / sample-rate / disallowed (bitrate, mode), and
emits the spec-mandated syncword `0xFFF` + ID `'1'` + layer `'10'`
with the padding and protection bits in the correct positions.

Round 162 added a `tests/malformed_input.rs` integration suite (+14
tests, 107 → 121) that property-tests the §2.4.1.3 / §2.4.2.3 header
parser and the §2.4.3.1 / §2.4.1.6 frame-decode loop against
malformed inputs: every single-bit flip of the canonical 4-byte
header (32 cases) must either round-trip to a structurally-valid
`FrameHeader` (`channels ∈ {1, 2}`, `frame_size_bytes ≥ 4`) or
return one of the documented `HeaderError` variants — the match arm
in the test is wildcard-free, so any future variant must be wired
through the test before it can compile; the syncword (bits 31..20),
`ID` (bit 19), and `layer` (bits 18..17) flips are additionally
pinned to the specific `BadSync` / `LsfNotSupported` /
`UnsupportedLayer(_)` they must produce; the §2.4.2.3 reserved
`emphasis = '10'` and `sampling_frequency = '11'` flips are pinned
to `ReservedEmphasis` and `ReservedSamplingFrequency`; the
high-order `bitrate_index` flip from `0b1010` (192 kbit/s) to
`0b0010` (48 kbit/s) is pinned to the §2.4.2.3
`DisallowedBitrateModeCombination { bit_rate: 48000, mode: Stereo }`
matrix rejection; the semantic-only `private_bit` / `copyright` /
`original` / `padding` / `mode_extension` flips are pinned to round-
trip through `parse` with the affected field reflected (and the
`padding` flip is additionally pinned to shift `frame_size_bytes`
by exactly 1). `decode_frame` against every truncated prefix
`0..626` of a synthesized 192k/44.1k/Stereo/no-CRC frame returns
either `Header(BufferTooShort)` (prefix < 4) or
`Truncated { have: prefix_len, need: 626 }` (4 ≤ prefix < 626) —
the §2.4.3.1 frame-size check fires before the §2.4.1.6 bit reader,
so a one-byte-short payload is surfaced as `Truncated` rather than
`AudioData(UnexpectedEnd)`; a `protection_bit = 0` header with only
5 bytes (one CRC slot byte missing) is pinned to `Truncated { have:
5, need: 6 }`. `find_sync` is exercised exhaustively over the 256
second-byte values whose top nibble differs from `0xF` (must
report `None`) and over the 255 first-byte values different from
`0xFF` (must also report `None`); `FrameHeader::parse` is
exhaustively confirmed to report `BadSync` across the same 240
non-`0xF_` second-byte values without panicking.

Round 175 added five encoder-side tests (121 → 126) for the new
`BitAllocTable::allocation_index` inverse mapping: an exhaustive
round-trip across every defined `(table, sb, index)` triple of all
four B.2 sub-tables (B.2a / B.2b / B.2c / B.2d — every cell of
every row, including the entry-0 sentinel) confirms that
`allocation_index(sb, nb_steps(sb, idx))` returns the original
`idx`; the §2.4.2.3 zero sentinel `nb_steps == 0` is pinned to
`Some(0)` for every in-range subband; subbands at or past `sblimit`
return `None` regardless of `nb_steps` (including the sentinel);
off-row `nb_steps` values (`5` and `9` against B.2a's wide row
sb=0..=2, `63` against the nbal=3 row, `7` against the nbal=2 row,
plus a battery of arbitrary out-of-table values) all return `None`
without false positives; and the B.2c / B.2d row-by-row encoder
mapping matches the literal PDF page-48/49 ordering for every
column.

Round 182 wired the codec through `oxideav_core`'s `Decoder` trait
(`codec_decoder` module): `Mp2CoreDecoder` adapts the existing
`decode_frame_with` primitive to the framework's packet-in / frame-out
contract; `make_decoder(&CodecParameters) -> Box<dyn Decoder>` is the
factory the registry hands out; `register_codecs(&mut CodecRegistry)`
installs the factory under id `"mp2"` and claims two container tags —
the Win32 WAVE format `0x0050` (`WAVE_FORMAT_MPEG`, shared with Layer
I) and the Matroska `A_MPEG/L2` CodecID. The `0x0050` tag collision
with `oxideav-mp1` is disambiguated by a §2.4.1.3-layer-field probe:
given a first-packet hint, the probe inspects bits 18..17 of the
syncword'd header (`'10'` = Layer II → confidence 1.0; `'11'` /
`'01'` = Layer I / III → 0.0; no packet hint → 0.5). The top-level
`register(&mut RuntimeContext)` now calls `register_codecs(&mut
ctx.codecs)`; `oxideav_core::register!("mp2", register)` makes that
reachable via `oxideav-meta`. Output is planar little-endian `i16` in
`Frame::Audio` (`data[ch].len() == 1152 * 2`); per-frame samples are
clamped at `[i16::MIN, i16::MAX]` after the §2.4.3.4.7.1 nominal
`[-1.0, +1.0]` float-to-int rescaling.

22 new tests on `codec_decoder` (126 → 134 in `src`, +8 there plus
13 in the trait surface itself = 22 new — total now 148 across the
crate): `make_decoder` accepts mono and stereo and rejects every
non-{1,2}-channel hint, defaults are applied when params are blank;
the staged 31-frame stereo fixture decodes packet-by-packet through
the trait surface and yields one `Frame::Audio` per packet with the
expected planar shape; PTS is propagated from `Packet::pts` to
`AudioFrame::pts` verbatim; `receive_frame` without a prior
`send_packet` returns `Error::NeedMore`; `flush` followed by a drain
yields `Error::Eof`; `send_packet` after `flush` is rejected;
`reset` re-enables the surface; a truncated packet surfaces the
`decode_frame_with` truncation error through the trait error channel;
the layer-field probe scores 1.0 against Layer II headers, 0.0
against Layer I and Layer III, 0.5 with no packet hint, and below
0.5 on bad sync / short packets; the same probe scores 1.0 against
the staged fixture's first 4 bytes; `register_codecs` installs a
discoverable decoder factory and resolves both container tags
(`WAVE_FORMAT_MPEG` + `A_MPEG/L2`) back to id `"mp2"` via
`resolve_tag_ref`; and the `f64 → i16 LE` plane converter is pinned
at the {0, ±0.5, ±1, ±1.5, ±2} reference points (the ±1.5 and ±2
inputs clamp at the i16 endpoints).

## What does not work yet

Bit-exact PCM-against-reference validation (PSNR / SNR vs the
`expected.wav` next to the staged fixture, or against black-box
validator output) is pending an Auditor round.

Encoder is mostly staged: the §2.4.1.4 / §2.4.3.1 CRC-16 write
primitives (`crc16_layer2` + streaming `crc16_update_*`), the
§2.4.1.3 / §2.4.2.3 header writer (`FrameHeader::emit_bytes` +
`encode_bitrate` + `encode_sampling_frequency`), the §C.1.3
Annex C polyphase analysis filterbank
(`AnalysisFilterbank::push_audio`, added in round 192), the
§2.4.3.3.3 / Annex C §C.1.5.2.6 scalefactor extraction
(`compute_scalefactors` / `extract_scalefactor_index` /
`pick_scalefactor_index`, added in round 195), the Annex C
§C.1.5.2.5 / §C.1.5.2.6 SCFSI Table-C.4 selection (`select_scfsi`
/ `classify_difference` / `ScfsiSelection`, added in round 202),
the §2.4.1.6 audio-data writer (`write_audio_data` /
`write_audio_data_with_section_bits`, added in round 208), the
§C.1.5.2.7 iterative bit-allocator (`allocate_bits` /
`fixed_bit_budget` / `snr_db` / `sample_bits_for`, added in round
214), and the §2.4.3.3.4 sub-band sample quantizer
(`quantize_sample` / `quantize_scaled` / `write_triplet` /
`write_triplet_scaled`, added in round 220) are in place, and the
round-227 §2.4 / Annex C frame-level encode orchestrator
(`encode_frame` / `encode_frame_with` / `EncodeFrameState` /
`EncodeError`, `encoder_frame` module) ties them together. The
remaining piece is the §D.1 / §D.2 psychoacoustic model: the
encoder accepts a caller-supplied `SmrTable` (per-(channel,
sub-band) signal-to-mask ratio in dB) so a real perceptual model
can be slotted in later; a constant 0 dB table produces a
syntactically-valid bit-allocated frame whose subjective quality
is rate-driven only. The masker → masking-threshold half of
Model 1 (§D.1 Steps 6 and 7) is starting to land in the `psy`
module — see **Round 238** below.

**Round 256 (2026-06-08)** added the Annex D Model 1 §D.1 Step 4(b)
**tonal-component listing sweep** (`psy` module). One new
spec-text-only entry point drives the per-FFT-line Step 4(b) loop
across the already-landed `is_tonal_layer2` / `tonal_spl_db` /
`zero_tonal_neighbourhood_layer2` primitives:
`list_tonal_layer2(spl_db: &mut [f64]) -> Vec<TonalCandidate>`
visits `k` in ascending order from 3 up to `min(500, spl_db.len() -
2)` (the §D.1 Step 4(b) tonality-defined range `2 < k <= 500`
intersected with the spectrum length), runs the tonality test at
each `k`, and on a positive classification appends a
`TonalCandidate { k, spl_db }` carrier and applies the spec's
"all spectral lines within the examined frequency range are set
to −∞ dB" zero-out in place. Subsequent `k`s that fall inside an
already-claimed neighbourhood naturally cannot satisfy either the
local-maximum rule or the 7 dB inequality against the now-`-inf`
bins, so the spec's exclusion rule "examined frequency range …
set to −∞ dB" is honoured without a separate skip list. The new
`TonalCandidate` carrier intentionally omits the Bark position —
the FFT-line → Bark mapping lives in the PNG-only Annex D
Table D.1d / D.1e / D.1f Layer II columns (note `#1262`); when
that material lands the caller may promote each candidate to a
full `Masker { kind: Tonal, z_bark, spl_db }` via the table
lookup. The sweep + zero-out composes cleanly with the
already-landed `list_non_tonal_layer2` Step 4(c) pass — bands
fully zeroed by the tonal sweep drop out of the non-tonal listing
via `non_tonal_spl_db == None`. 9 new lib tests (321 → 330) pin:
single-isolated-peak detection plus the `X_tm` three-line
power-sum value, neighbourhood zero-out coverage (centre + both
`j = ±2` neighbours at −∞, outside lines untouched), subthreshold
rejection at 5 dB below the 7 dB inequality, within-prior-
neighbourhood suppression (two peaks 2 bins apart → one
survivor), well-separated multi-peak emission at `k = 30` /
`k = 80` (densest-row j=±2 and second-row j ∈ {-3,-2,+2,+3}
neighbourhoods both consumed), ascending-`k` output ordering on
four peaks, edge-of-domain candidates ignored (k=1, k=600 in a
1024-bin spectrum), end-to-end composition with
`list_non_tonal_layer2`, and the short-spectrum (`len < 4`)
no-op guard.

**Round 253 (2026-06-08)** added Annex D Model 1 §D.1 Step 5(b)
tonal-masker decimation and §D.1 Steps 8 + 9 minimum-masking-
threshold-per-subband / signal-to-mask-ratio primitives (`psy`
module). Three new spec-text-only entry points wire the
"collapse near-Bark tonal clusters then reduce per-subband" half
of Model 1 between the already-landed Step 4(c) (round 250) and
the §C.1.5.2.7 iterative bit-allocator (round 214):
`decimate_tonal_maskers(maskers)` runs the verbatim §D.1 Step 5(b)
procedure ("Decimation of two or more tonal components within a
distance of less than 0.5 Bark: Keep the component with the
highest power, and remove the smaller component(s) from the list
of tonal components. For this operation, a sliding window in the
critical band domain is used with a width of 0.5 Bark." — PDF
page 113), splitting the input by `MaskerKind`, sorting the tonal
list by `z_bark`, walking sorted runs anchored on each run's
first entry, and emitting the highest-`spl_db` member of each
run. The half-open `< 0.5 Bark` window is exact: a pair at
exactly 0.5 Bark distance does NOT merge; ties on `spl_db` keep
the lowest-`z_bark` entry deterministically; the chained-run
case `(5.0, 5.4, 5.8)` produces the documented two-survivor
result rather than collapsing all three (the spec's
sliding-window reading requires every pair in the window to be
< 0.5 Bark of every other). Non-tonal maskers pass through
untouched per the spec scope ("two or more **tonal** components");
the output vector emits non-tonal in input order, then surviving
tonal in ascending Bark order (the spec is order-invariant
downstream but the deterministic interleave makes the output
round-tripable). The procedure is documented idempotent — the
test `decimate_tonal_maskers_idempotent` runs the decimation
twice on a 6-masker mix and asserts the second pass is a no-op.
Step 5(a) (the threshold-in-quiet drop `X_tm(k) >= LT_q(k)`)
still depends on the PNG-only Annex D Table D.1d/e/f LTq curves
(#1262) and is not landed this round.
`minimum_masking_threshold_subband(ltg_db, line_subband)` runs
the verbatim §D.1 Step 8 reduction `LT_min(n) = MIN[ LT_g(i) ]
for f(i) in subband n` (PDF page 114): the caller hands in the
`f(i)` → subband index map (the spec's `f(i)` frequency vector
lives in the PNG-only Table D.1 inner rows; the caller produces
the equivalent FFT-line → subband index map from whatever source
they have), and the primitive runs the bare minimum-over-mask
reduction. The output `[Option<f64>; 32]` slot `n` carries
`None` for subbands that received no FFT line; `usize::MAX`
acts as a documented "outside audio band" sentinel and is
filtered out; `NaN` LTg values are dropped from the minimum to
keep the remaining finite values well-defined; a length
mismatch between `ltg_db` and `line_subband` is a caller error
and returns an all-`None` result.
`signal_to_mask_ratio_subband(l_sb_db, lt_min_db)` is the
verbatim §D.1 Step 9 elementwise subtraction `SMR_sb(n) = L_sb(n)
- LT_min(n)` (PDF page 115); slots whose `lt_min_db` is `None`
return `None` so the caller's §C.1.5.2.4 fallback (the subband
has no masking line in range) can substitute. Two new public
constants `TONAL_DECIMATION_WINDOW_BARK = 0.5` and
`NUM_SUBBANDS_LAYER2 = 32` expose the spec values explicitly.
23 new lib tests (298 → 321): the 0.5 Bark window-width
constant, the half-open `< 0.5` window endpoint, the
loudest-wins reduction inside a window, the equal-power
first-wins tie-break, the non-tonal-passthrough property, the
mixed-class output order (non-tonal in input order then surviving
tonal in Bark order), the chained-cluster non-merge case
`(5.0, 5.4, 5.8)`, empty / singleton inputs, idempotence on a
6-masker mix, sort-independence (ascending / descending /
arbitrary permutation produce identical output); subband
count constant pinned at 32, the per-subband min-reduction on a
multi-line / multi-subband synthetic case, the `usize::MAX`
sentinel and OOB indices filtered out, the length-mismatch
all-`None` safe return, the empty-input all-`None`, the NaN-drop
property, the descending-LTg running-min property over 10 lines
in one subband, and the trivial bijection L_sb(n) = LTg(n) per
subband; SMR equation pinned on three subbands with hand-computed
values, `None` LT_min propagated to `None` SMR, negative SMR
pass-through (no clamp), and an end-to-end Step 7 → 8 → 9
composition on a 4-line / 2-subband synthetic case asserting
`SMR_sb(3) == 35 dB`, `SMR_sb(7) == 35 dB`, and every other
subband `None`.

**Round 250 (2026-06-07)** added Annex D Model 1 §D.1 Step 4(b)
tonal-neighbourhood zero-out and §D.1 Step 4(c) non-tonal listing
(`psy` module + new `tables_d2` module). Four new spec-text-only
entry points wire the "set the tonal neighbourhood to −∞ dB then
power-sum the remaining lines per critical band" half of Step 4:
`zero_tonal_neighbourhood_layer2(spl_db, k)` walks the same per-`k`
`j`-neighbourhood used by Step 4(b) tonality testing and sets every
line within it (plus the centre `k`) to `f64::NEG_INFINITY`,
reproducing the verbatim spec sentence "all spectral lines within
the examined frequency range are set to −∞ dB" (PDF page 112);
`non_tonal_spl_db(spl_db, lo, hi) -> Option<f64>` is the
per-critical-band power sum
`X_nm = 10·log10(Sum 10^(X(k)/10))` over `k in [lo, hi]`,
ignoring `-inf` lines exactly (`10^(-inf/10) = 0`) and returning
`None` for empty or fully-zeroed bands; `non_tonal_band_index(lo,
hi) -> Option<usize>` picks the FFT line "nearest to the geometric
mean of the critical band" per the spec phrasing (PDF page 113) on
the integer band `[lo, hi]` (with `lo = 0` substituted as `1` to
avoid `sqrt(0)` collapse), returning the nearest integer with ties
rounded down; `list_non_tonal_layer2(spl_db, fs) -> Vec<Masker>`
is the per-sampling-rate Step 4(c) sweep — it walks the Annex D
Table D.2d / D.2e / D.2f boundaries in order, calls
`non_tonal_spl_db` on each `(prev_top + 1, top]` band, and emits
one `Masker { kind: NonTonal, z_bark, spl_db }` per non-empty band
with `z_bark` taken from the boundary's top-line Bark column. A
new `tables_d2` module carries the Layer-II critical-band
boundary tables verbatim from the Annex D PDF: 25 entries
(`TABLE_D_2D_LAYER_II_32KHZ`), 27 entries
(`TABLE_D_2E_LAYER_II_44K1HZ`), 27 entries
(`TABLE_D_2F_LAYER_II_48KHZ`); one illegible-digit cell at D.2e
band 17 (Bark `16,11[illegible]` in the staged PDF) is reproduced
as best-fit `16.116` and the gap is documented in the constant's
doc comment. A new `SamplingRate { Fs32kHz, Fs44k1Hz, Fs48kHz }`
enumeration (re-exported as `PsyAnnexDSamplingRate`) lets the
`list_non_tonal_layer2` caller pick the right boundary table.
Steps 2 / 3 / 5 still depend on the PNG-only inner rows of Annex D
Tables D.1 / D.3 / D.4 (#1262) and remain DOCS-BLOCKED. 27 new
lib tests (271 → 298): boundary monotonicity in all three
dimensions (top-line index, frequency, Bark) for D.2d/D.2e/D.2f
and row-count parity with the spec column headings; tonality
zero-out on a `{-2, +2}` row and on the widest `{-12, …, +12}`
row, with the immediately-adjacent out-of-window lines verified
unchanged and the no-op behaviour for k ≤ 2 / k > 500; non-tonal
power-sum identity for three equal-power lines
(`60 + 10·log10(3) ≈ 64.7712`), `-inf` line exclusion (two finite
60 dB lines power-sum to `60 + 10·log10(2)`), all-`-inf` band
returns `None`, dominant-line band anchors `X_nm` to the dominant
line within 1 mdB, and the empty-band rejection cases; geometric
mean nearest-integer pick on the simple `sqrt(64) = 8`,
`sqrt(9) = 3`, `sqrt(225) = 15` cases plus the singleton, DC-bin,
and fractional rounding rules; per-sampling-rate sweep returns
one masker per band on a flat spectrum, drops all bands on a
fully-zeroed spectrum, propagates `z_bark` from the Table D.2
boundary verbatim, sums equal-width bands to identical `X_nm`,
and locates a single 100 dB line in the correct band's masker
within 1 mdB.

**Round 248 (2026-06-07)** added Annex D Model 1 §D.1 Step 1 Hann
window and §D.1 Step 4 tonality classifier primitives (`psy` module).
Five new spec-text-only entry points land the FFT-windowing and the
tonal/non-tonal labelling halves of Model 1:
`hann_window_layer2() -> [f64; 1024]` reproduces the verbatim spec
equation `h(i) = sqrt(8/3) * 0.5 * (1 - cos(2*pi*i/N))` with
`N = LAYER2_FFT_LEN = 1024` (the spec's Layer II FFT length per the
PDF page 116 "Technical data of the FFT" table; the front coefficient
is the spec's RMS-matching normalization for the Hann window's power
gain). `is_local_maximum(spl_db, k) -> bool` runs the verbatim Step
4(a) rule `X(k) > X(k - 1) AND X(k) >= X(k + 1)` with the
asymmetry-preserving strict `>` on the lower side and non-strict
`>=` on the upper side. `tonal_neighbourhood_layer2(k) ->
Option<&'static [i32]>` returns the per-`k` Layer II `j`
neighbourhood (verbatim spec table at PDF p.117): `{-2, +2}` for
`2 < k < 63`, `{-3, -2, +2, +3}` for `63 <= k < 127`, `{-6, …, -2,
+2, …, +6}` for `127 <= k < 255`, `{-12, …, -2, +2, …, +12}` for
`255 <= k <= 500`. `is_tonal_layer2(spl_db, k) -> bool` runs the
verbatim Step 4(b) inequality `X(k) - X(k + j) >= 7 dB` for every
`j` in the neighbourhood (with `TONALITY_THRESHOLD_DB = 7.0` the
spec constant) after verifying the Step 4(a) local-maximum
precondition. `tonal_spl_db(spl_db, k) -> Option<f64>` is the
three-line power sum `X_tm(k) = 10 * log10(10^(X(k-1)/10) +
10^(X(k)/10) + 10^(X(k+1)/10))` the spec applies to a confirmed
tonal line. Two new public constants `LAYER2_FFT_LEN = 1024` and
`LAYER2_FFT_BINS = 513` expose the FFT working range
(`k = 0 .. N/2` inclusive). 17 new lib tests (254 → 271): Hann
window pinned at `h(0) = 0` and `h(N/2) = sqrt(8/3)`, symmetry
around `i = N/2` at six sampled offsets, and the `[0, sqrt(8/3)]`
codomain bound across all 1024 samples; local-maximum on a simple
peak, on a two-bin plateau (left index passes, right index fails by
the strict `>` rule), and at edge indices (always `false`);
neighbourhood-row dispatch on each spec boundary (`k = 3, 62, 63,
126, 127, 254, 255, 500, 501`) with length matching the spec row,
strict symmetry around `j = 0`, and `j = 0` never present; tonality
classifier above-threshold acceptance (a clean 50 dB peak), narrow
below-threshold rejection (6 dB above neighbours), single-neighbour
rejection (one `j` slot fails by 2 dB), local-maximum-precondition
enforcement (a non-maximum bin is never tonal), and window-edge
rejection; tonal-SPL three-equal-power identity (`60 + 60 + 60` →
`60 + 10·log10(3) ≈ 64.7712`), centre-dominated case (`80` dB peak
above `0` dB shoulders ≈ 80 dB), edge-returns-None for the first /
last bin, and monotone-in-power lower-bound (the three-line sum is
always at least the centre bin's SPL).

**Round 238 (2026-06-05)** added Annex D Model 1 §D.1 Step 6
masking-function `vf` and §D.1 Step 7 global-masking-threshold
`LTg` primitives (`psy` module). Five new pure functions land
the masker → masking-threshold half of Model 1:
`masking_index_tonal(z_j_bark)` reproduces the verbatim spec
equation `av_tm = -1.525 - 0.275 * z(j) - 4.5` dB;
`masking_index_non_tonal(z_j_bark)` reproduces
`av_nm = -1.525 - 0.175 * z(j) - 0.5` dB;
`masking_function_vf(dz_bark, x_db) -> Option<f64>` is the
four-branch piecewise function defined on the half-open Bark
window `[-3, 8)` — outside the window it returns `None` (the
spec's "masker ignored, `LT = -inf dB`" semantics). The four
branches are `17·(dz+1) − (0.4·X + 6)` on `[-3, -1)`,
`(0.4·X + 6)·dz` on `[-1, 0)`, `-17·dz` on `[0, 1)`, and
`-(dz−1)·(17 − 0.15·X) − 17` on `[1, 8)`; the function is
continuous at `dz = 0`. `individual_masking_threshold_db(masker,
z_i_bark)` composes the per-masker individual threshold
`LT = SPL + av + vf`, returning `None` outside the `vf` window;
`global_masking_threshold_db(maskers, z_i_bark, ltq_db)` is the
Step-7 energy sum `LTg(i) = 10·log10( 10^(LTq/10) +
Σ 10^(LT_j/10) )` over every in-range masker, with the
threshold-in-quiet `LTq` carried in dB. Two new public types
(`MaskerKind { Tonal, NonTonal }` and `Masker { kind, z_bark,
spl_db }`) and two public constants (`MASKING_FUNCTION_DZ_LO =
-3.0`, `MASKING_FUNCTION_DZ_HI = 8.0`) expose the masker carrier
and the window endpoints. The primitives operate on
caller-supplied Bark coordinates — Steps 1..5 of Model 1
(1024-sample FFT, SPL conversion, tonality classifier,
decimation / reorganisation, masker selection) remain
unimplemented because they depend on the PNG-only inner rows of
Annex D Tables D.1d–f (Layer II threshold-in-quiet) and Tables
D.2d–f / D.3 / D.4 (Bark / Hz / FFT-line mapping). Eighteen new
lib tests (236 → 254, all green) validate every piecewise branch
with hand-computed numeric anchors, the `[-3, 8)` window
boundaries, continuity at `dz = 0`, the `LT = SPL + av` identity
at `z(i) = z(j)`, the tonal-below-non-tonal ordering at matched
parameters, and the four Step-7 invariants: no maskers ⇒
`LTg = LTq`, distant masker ⇒ `LTg = LTq`, strong local masker
dominates `LTq`, two equal-power co-located maskers add exactly
`10·log10(2) ≈ +3.0103` dB.

**Round 234 (2026-06-04)** added §2.4.1.8 `ancillary_data()`
emission to the Layer II encoder. Two new public entry points
extend the `encoder_frame` module:
`encode_frame_with_ancillary(header, pcm, smr_db, banc, ancillary)`
and `encode_frame_with_state_and_ancillary(header, pcm, smr_db,
banc, ancillary, &mut state)`. Both copy a caller-supplied byte
payload into the §2.4.2.1 frame tail that begins immediately after
the §2.4.1.6 audio-data + §2.4.3.3.4 sample-codeword region. The
copy starts on the next byte boundary past the sample region
(`BitWriter::align_to_byte` runs first so any sub-byte residue at
the end of the §2.4.3.3.4 codewords is zero-padded to the next
whole byte) and any frame bytes the payload does not fill are
zero-padded so the byte count still equals
`header.frame_size_bytes()`. The §C.1.5.2.7 `banc` reservation
continues to steer the iterative allocator — a typical call picks
`banc >= ancillary.len() * 8` so the §2.4.1.8 tail is wide enough
for the payload regardless of how the marginal-cost loop spends the
data-bit budget. A new `EncodeError::AncillaryTooLarge { space,
got }` variant surfaces over-long payloads with both the actual
tail capacity (`space`) and the rejected length (`got`); the legacy
`encode_frame` / `encode_frame_with` entry points still exist and
are now thin shims over the shared `encode_frame_inner`
implementation that pass `ancillary = &[]`. The §2.4.3.1 CRC patch
runs after the ancillary copy and continues to verify clean —
Annex B Table B.5 protects the second half of the header + the
§2.4.1.6 audio-data section (allocation + scfsi) but excludes the
§2.4.1.8 tail, so the stored CRC word at frame bytes 4..6 is
byte-identical to the no-ancillary reference frame for any payload.
Six new lib tests (230 → 236, all green): empty-ancillary call is
byte-for-byte identical to `encode_frame`; a 32-byte distinctive
payload lands at the §2.4.1.8 tail start (located via an
all-`0xCC` marker-frame probe) with the trailing pad still zero;
the CRC word matches between empty and non-empty ancillary frames
(Table B.5 exclusion proof); a frame-size-long payload surfaces
`AncillaryTooLarge` with `got == huge.len()` and `space < got`; the
stateful entry point preserves the §C.1.3 X ring-buffer evolution
(same input + same payload + fresh state yields byte-identical
first frames, a second frame from the persistent state differs);
and a payload sized exactly `space` fits while `space + 1` is
rejected with the same reported `space` value.

**Round 227 (2026-06-04)** added the §2.4 / Annex C frame-level
encode orchestrator (`encoder_frame` module). `encode_frame(header,
pcm, smr_db, banc) -> Result<Vec<u8>, EncodeError>` pulls the
previously-landed encoder primitives together into a single
`pcm-in → byte-stream-out` call: §C.1.3 analysis filterbank →
§C.1.5.2.6 scalefactor extraction per (channel, sub-band, granule)
→ §C.1.5.2.7 bit allocation against the supplied `SmrTable` →
§C.1.5.2.5 / Table C.4 SCFSI selection (rewriting
`audio.scalefactor[ch][sb]` to the Table C.4 `used` triple and
populating `audio.scfsi[ch][sb]`) → §2.4.1.3 header bytes →
§2.4.1.4 CRC slot (reserved, patched after the protected region
exists) → §2.4.1.6 audio-data section → §2.4.3.3.4 sample
codewords in the spec's `(sample_gr, sb, ch)` order → §2.4.1.10
`banc` ancillary reservation → zero-padding to
`header.frame_size_bytes()` → §2.4.3.1 CRC-16 patch (extracting
the protected region from the just-emitted bytes and writing the
16 bits into the reserved slot when `protection_bit == 0`).
`encode_frame_with(header, pcm, smr_db, banc, &mut state)` takes
a caller-supplied `EncodeFrameState` whose per-channel
`AnalysisFilterbank` X ring buffers persist across successive
frames — the encoder dual of `frame::FrameDecodeState`;
`EncodeFrameState::reset` re-zeros every channel's X buffer on a
seek / discontinuity. The emitted frame is exactly
`header.frame_size_bytes()` bytes long; the bytes round-trip
through `frame::decode_frame` to recover the same header, the
same `nb_steps` table, and a per-channel PCM vector of
`frame::PCM_SAMPLES_PER_CHANNEL` samples. The §2.4.3.1 CRC verifies
on the decode side; flipping any CRC-payload bit afterwards makes
`decode_frame` reject the frame with `FrameError::CrcMismatch`.
`EncodeError` wraps the sub-stage errors (`HeaderError`,
`BitAllocError`, `AudioDataWriteError`, `SampleWriteError`) plus
PCM-shape validation (`BadPcmChannelCount` /
`BadPcmLen`).

**Round 220 (2026-06-03)** added the §2.4.3.3.4 encoder sub-band
sample quantizer (`encoder_samples` module). `quantize_sample(class,
s'') -> u32` is the documented arithmetic inverse of the
§2.4.3.3.4 decode mapping: divide out the Table 3-B.4 linear
formula (`s''' = s''/C − D`), clamp the integer `k = round(s''' ·
2^(n−1))` into the legal range for the active class (ungrouped:
`[−2^(n−1), 2^(n−1) − 1]`; grouped: `[−2^(n−1), nb_steps − 1 −
2^(n−1)]`, narrower because §2.4.3.3.4 degrouping yields three
digits in `[0, nb_steps)`), encode as `n`-bit two's complement,
then re-invert the MSB so the returned code is exactly what
`requant::requantize_code` would consume — the radix-`nlevels`
digit for a grouped class, the raw `bits_per_codeword`-bit
codeword for an ungrouped class. `write_triplet(class, &[s''; 3],
writer)` drives an `oxideav_core::bits::BitWriter` through one
(subband, granule) triplet: grouped classes pack the three digits
via the radix-`nlevels` rule (exact inverse of `requant::degroup`)
and emit one `bits_per_codeword`-bit field; ungrouped classes emit
three independent `bits_per_codeword`-bit codes. The writer
advances by exactly the bit count `requant::read_triplet` would
consume on the decoder side. `quantize_scaled` /
`write_triplet_scaled` layer the §2.4.3.3.3 Table 3-B.1 division
on top (rejects the reserved index `63` as
`SampleWriteError::ReservedScalefactorIndex`). 13 new lib tests
(202 → 215): every defined raw code of every Table 3-B.4 class
round-trips through `requantize_code → quantize_sample` back to
itself (the bin-centre identity); an arbitrary `s''` produces a
code whose `requantize_code`-decoded bin centre is within one
quantization step (`C / 2^(n−1)`) of the input; the grouped-class
digit never falls outside `[0, nb_steps)` (otherwise `degroup`'s
range check would fire); out-of-range positive / negative inputs
clamp to `nb_steps − 1` / `0` for grouped and `2^n − 1` / `0` for
ungrouped; `group_combined` inverts `requant::degroup` exhaustively
for every triple of grouped classes 3 / 5 / 9 (27 + 125 + 729 =
881 combinations walked); `quantize_scaled` reproduces
`quantize_sample(s' / factor)` and rejects the reserved index 63;
`write_triplet` advances the writer by `bits_per_codeword`
(grouped) / `3 × bits_per_codeword` (ungrouped); `write_triplet`
then `read_triplet` round-trips every bin-centre triplet for
`nb_steps ∈ {3, 5, 7, 9, 15, 31, 63, 127, 255, 511}`;
`write_triplet_scaled` then `requantize_scaled` round-trips
bin-centre triplets across five scalefactor indices (unity,
doubling, mid-range, near-max-attenuation); the symmetric input /
code property around the zero point holds (`s'' = C·D` maps to
code `= 2^(n−1)`); and every level triplet of every grouped class
round-trips through write → read exhaustively (3³ + 5³ + 9³ = 881
triplets).

**Round 214 (2026-06-03)** added the §C.1.5.2.7 encoder iterative
bit-allocator (`encoder_bit_allocator` module). `allocate_bits(&FrameHeader,
&SmrTable, banc) -> Result<AudioData, BitAllocError>` runs the Annex C
"each iteration loop" procedure verbatim: it (a) computes the
constant-budget terms (`bhdr = 32`, `bcrc ∈ {0, 16}`, `bbal = Σ
nbal(sb) × channels-or-shared-above-bound`) via the public
`fixed_bit_budget(&FrameHeader)` helper, (b) initialises every
`nb_steps[ch][sb] = 0` and `MNR[ch][sb] = SNR(0) − SMR[ch][sb] =
−SMR[ch][sb]`, then (c) repeatedly picks the lowest-MNR slot,
advances its B.2 row position by one column, charges the marginal
sample-bit cost (and on first-time non-zero, the worst-case 2-bit
scfsi + 18-bit scalefactor reservation), backs the step out if it
would push `adb` negative, otherwise updates that slot's MNR using
the new `SNR(nb_steps)` from Table C.5. Termination is the spec's
"adb is not less than any possible increase" condition. The Annex C
Table C.5 SNR-vs-`nb_steps` table (PDF page 76) is exposed via
`snr_db(nb_steps) -> Option<f64>` (`0 -> 0.00 dB` through `65535 ->
98.01 dB`, monotonically increasing); the per-frame
sample-codeword bit cost is exposed via `sample_bits_for(nb_steps)
-> u32` (a grouped class with `nb_steps ∈ {3, 5, 9}` packs 3
samples per codeword → `12 × bits_per_codeword`; an ungrouped class
costs `36 × bits_per_codeword`). The §2.4.1.6 joint-stereo
above-`bound` constraint (`allocation[1][sb] = allocation[0][sb]`)
is enforced inline: a single "merged" slot covers both channels at
once, both channels' `nb_steps` advance together, the marginal
sample-bit cost doubles (two channels' codewords), the scfsi +
scalefactor cost also doubles (both channels still carry
independent per-channel scalefactor + scfsi above bound per
§2.4.1.6), and the merged MNR feeds the iteration from the
*worse* of the two channels' MNRs so the joint allocation chases
the noisier channel. The bit budget is intentionally conservative:
the actual scfsi schedule, decided later by `select_scfsi`
(round 202), can only *reduce* the per-slot scalefactor count from
the worst-case 3 to 1, 2, or 3 transmitted slots, so the actual
frame can never overrun `adb`. The `BitAllocError::InsufficientFrameSize`
error surfaces frames whose constant budget already exceeds `cb`
(possible only with an oversized `banc` reservation, never for a
real Layer II header). 14 new lib tests (188 → 202): Table C.5
landmark spot-checks (PDF page 76 `0 → 0`, `3 → 7`, `9 → 20.84`,
`63 → 37.75`, `1023 → 61.96`, `65535 → 98.01`), strict
monotonicity of the SNR column (the iteration's invariant), every
Table 3-B.4 `nb_steps` having a Table C.5 entry, the closed-form
`sample_bits_for` against grouped (`{3, 5, 9}`) and ungrouped
classes, the `fixed_bit_budget` arithmetic across canonical
192k/44.1k stereo (`cb=5008`, `bbal=188`), joint-stereo with
`bound=4` (`bbal=32+78=110`), and single-channel 80 kbit/s
(`bbal=88`), the allocator's budget invariant under uniformly
negative SMR (termination still holds), the budget invariant
under uniformly +100 dB SMR, the priority property (a single
high-SMR slot ends up with the largest `nb_steps` of the frame),
every emitted `nb_steps` being reachable through
`BitAllocTable::allocation_index` (the writer's prerequisite),
the joint-stereo above-`bound` shared-allocation invariant
(`nb_steps[0][sb] == nb_steps[1][sb]`), the
`InsufficientFrameSize` error path, and an end-to-end round-trip
through `write_audio_data` → `parse_audio_data` confirming the
allocator's `nb_steps` are bit-exact preserved.

**Round 208 (2026-06-02)** added the §2.4.1.6 audio-data writer
(encoder side). `write_audio_data(&FrameHeader, &AudioData, &mut
BitWriter)` and `write_audio_data_with_section_bits(...)` in the
`audio_data` module are the bit-for-bit inverse of
`parse_audio_data` / `parse_audio_data_with_section_bits`: for one
Layer II frame they emit, in order, the per-(sb, ch) `nbal`-bit
allocation indices (the §2.4.1.6 `sb >= bound` joint-stereo branch
writes ONE shared index per subband), the 2-bit `scfsi[ch][sb]`
for every (sb, ch) with non-zero allocation, and the 1/2/3
on-wire 6-bit scalefactor indices per the chosen `scfsi`
schedule. Allocation indices are derived via
`BitAllocTable::allocation_index` (round 175); scfsi codes derive
from the `Scfsi` enum the §C.1.5.2.5 selector (round 202) hands
the writer; scalefactor indices come from `compute_scalefactors`
(round 195) after SCFSI selection has arranged the
`[scf1, scf2, scf3]` triple to match the schedule's
reconstruction rule. `write_audio_data_with_section_bits` returns
the bit-lengths of the §2.4.1.6 bit-allocation and scfsi sections
so the yet-to-be-built §2.4.3.1 encoder CRC accumulator can index
Annex B Table B.5 without re-parsing. A new
`AudioDataWriteError` enum reports the encoder-side
self-inconsistencies: `NoBitallocTable`, `InconsistentLayout`
(`AudioData` disagrees with header on `table` / `channels` /
`bound`), `IntensityStereoAllocationMismatch` (above-`bound`
subband has unequal per-channel `nb_steps` — forbidden by
§2.4.1.6), `UnencodableNbSteps` (`nb_steps` not in any row of the
active sub-table), and `ReservedScalefactorIndex` (scalefactor
index 63 is reserved per §2.4.2.5). 10 new lib tests (178 -> 188):
uniform 192 kbit/s stereo round-trip, joint-stereo above-bound
round-trip, zero-allocation skip path, all four scfsi schedules
round-trip, section bit-count parity with the parser's
`parse_audio_data_with_section_bits`, the four error paths
(inconsistent layout / unencodable `nb_steps` / reserved
scalefactor 63 / intensity-stereo allocation mismatch), and an
exhaustive (every B.2 sub-table, every scfsi schedule) mono
round-trip walking 4 tables x 4 scfsi schedules through write ->
parse and asserting equality.

**Round 202 (2026-06-01)** added Annex C §C.1.5.2.5 / §C.1.5.2.6
encoder-side SCFSI selection (`encoder_scfsi` module). For each
`(channel, subband)` slot of a Layer II frame, `select_scfsi(&[u8;
3])` consumes the three per-granule Table 3-B.1 scalefactor indices
produced by `compute_scalefactors` and returns a `ScfsiSelection`
carrying (a) the §C.1.5.2.5 "scalefactor used in encoder" triple
the decoder will reconstruct (the spec's "adjusted" scalefactors),
(b) the `TransmissionPattern` selecting which of the three slots
are physically written, and (c) the 2-bit `scfsi[ch][sb]` code
matching one of the four `audio_data::Scfsi` schedules. The
§C.1.5.2.5 procedure runs as: compute `dscf1 = scf1 - scf2` and
`dscf2 = scf2 - scf3`, classify each into one of the five spec
classes (PDF page 73: `dscf <= -3`, `-3 < dscf < 0`, `dscf == 0`,
`0 < dscf < 3`, `dscf >= 3`), and index Table C.4 (PDF page 76) by
the `(class1, class2)` pair. The "4 = max scalefactor" recipe at
row (2,4) maps to the *minimum index* because Table 3-B.1 is
monotonically decreasing (larger multiplier ↔ smaller index).
9 new lib tests (169 → 178): every-boundary classifier pin, the
full 25-row Table C.4 lookup pin (input chosen to land in each
target row, then `used` / pattern / `scfsi` cross-checked against
the PDF column-by-column), all-identical-triplet → ShareAll,
large-strictly-changing-indices in both monotonic directions,
the (2,4) max-recipe semantics, transmitted-slot-count consistency
per row, the wire round-trip (writing the on-wire 6-bit slots
under the chosen `scfsi` schedule reconstructs the encoder's
claimed `used` triple), purity / determinism, and the "at least
one slot transmitted" lower bound.

**Round 195 (2026-05-31)** added §2.4.3.3.3 / Annex C §C.1.5.2.6
encoder scalefactor extraction (`encoder_scalefactors` module).
For each scalefactor-granule of 12 sub-band samples produced by
`AnalysisFilterbank::push_audio`, `pick_scalefactor_index(max_abs)`
returns the largest index `i` whose Table 3-B.1 entry
`SCALEFACTORS[i] >= max_abs` — i.e. the smallest multiplier still
large enough to cover the granule's peak, the inverse of the
§2.4.3.3.3 decode lookup. Because Table 3-B.1 is monotonically
decreasing (entry 0 = 2.0, entry 62 ≈ 1.2e-6), "smallest value
`>= max_abs`" is equivalently "largest qualifying index".
`extract_scalefactor_index(&[f64; 12])` wraps the per-granule
max-abs; `compute_scalefactors(&[[f64; 36]; 32], sblimit)` returns
the per-frame `[granule][sub-band]` indices for one channel (3
granules × 32 sub-bands, sub-bands ≥ `sblimit` left at index 62).
All-zero granules map to index 62; input beyond the table
(`> SCALEFACTORS[0]`) clamps to index 0 per the §2.4.3.4.7.1
`[-1, +1)` precondition. 8 new lib tests (169 total, was 161) pin
the exact / slightly-below / slightly-above lookup property across
every entry, the all-zero and top-of-table-clamp edge cases, the
in-range invariant, a 1000-vector round-trip envelope property
(chosen multiplier covers every sample, and is the tightest such),
and `sblimit` gating.

**Round 192 (2026-05-30)** added the §C.1.3 Annex C polyphase
analysis subband filterbank
(`analysis::AnalysisFilterbank::push_audio(&[f64; 32], &mut [f64;
32])`) and the supporting Annex C Table C.1 transcription
(`tables_analysis::C`, 512 entries). The filterbank is the
time-reversed dual of the existing `synthesis::SynthesisFilterbank`:
one call drives a 512-entry X ring buffer through the documented
`shift X by 32` → `insert audio (most recent at X[0])` → `window Z =
X * C` → `Y_i = Σ_{j=0..8} Z[i + 64j]` → `S_i = Σ_{k=0..64} M_ik *
Y_k` pipeline. The 32×64 `M_ik` matrix is precomputed at
construction from the §C.1.3 closed form `M_ik = cos[(2i + 1)(k −
16) π/64]`; the 512 C[i] window coefficients were transcribed from
ISO/IEC 11172-3 (1993) PDF pages 67-69 via 300-DPI tesseract OCR on
`pdftoppm` renders, with `pdftotext -layout` + `pdftotext -raw` as
tie-breakers against index-side OCR noise. The spec-paired
filterbank-window identity `D[i] == 32 * C[i]` (where D is Annex B
Table 3-B.3, the synthesis-side window) is honoured to within 1
ULP at the 9-decimal-digit grid and is cross-checked by the
`c_matches_d_over_32_within_rounding` unit test as a transcription
oracle — both tables come from the same PDF and the spec pairs
them by their shared prototype low-pass response. 22 new lib tests
(161 total, was 139) pin the C[] endpoint readings (C[0] = 0;
C[256] = 0.035780907 global peak; |C[69]| = |C[70]| = |C[442]| =
|C[443]| = 0.000108719 secondary-peak symmetric pair), the 7
sign-block boundaries at indices 64 / 128 / 192 / 256 / 320 / 384
/ 448, the magnitude anti-mirror identity |C[256+k]| = |C[256-k]|
for k = 1..=255, the M_ik landmarks (M[0,16] = M[8,16] = M[31,16]
= cos(0) = 1, M[0,0] = cos(-π/4) = √2/2), and the §C.1.3
"most-recent at position 0" X-buffer convention (literally pinned
by `most_recent_sample_lands_at_x0` and
`shift_then_insert_preserves_old_samples_at_offset_32`).

**Round 185 (2026-05-29)** wired ISO/IEC 13818-3 §2.4.2.3 LSF
Layer II support. `FrameHeader::lsf` captures the parsed `ID` bit;
`decode_bitrate_lsf` / `decode_sampling_frequency_lsf` (and their
encode-side inverses) handle the LSF 8..160 kbit/s ladder and the
16 / 22.05 / 24 kHz sampling-frequency table from PDF page 21.
`BitAllocTable::B1Lsf` reflects the §2.4.3.1 mandate ("instead of
tables B.2 ..., table B.1 ... of this part of ISO/IEC 13818 should
be used"): a single allocation table covering all LSF
(`sblimit = 30`, `Sum of nbal = 75`, PDF page 71). `select_table`
routes every LSF header to `B1Lsf`. The §2.4.2.3 "not all
combinations of total bitrate and mode are allowed" matrix is
scoped to MPEG-1 — the 13818-3 LSF extension does not restate the
matrix, so every (LSF bitrate, mode) pair is accepted by both
`parse` and `emit_bytes`. The MPEG-1 polyphase synthesis path
applies verbatim to LSF (§2.4.3.2 reuses 11172-3 §2.4.3 for
Layer II), so end-to-end LSF `decode_frame` works today; an
all-zero-allocation LSF frame round-trips to exactly 1152 silent
samples per channel.

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
