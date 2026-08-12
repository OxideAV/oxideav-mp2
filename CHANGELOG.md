# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate adheres
to [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Official ISO/IEC 13818-4 audio-conformance sweep — the crate's
  last "lacks" closed.** `tests/iso13818_4_conformance.rs` sweeps the
  first-party ISO 13818-4 audio test-bitstream suite (35 archives from
  ISO's standards-maintenance portal; fetch recipe + SHA-256 manifest
  staged in the workspace docs at
  `docs/audio/mp3/iso-13818-4-audio-conformance.md`). The vectors are
  ISO use-licensed, so they are never committed: the harness is gated
  on `OXIDEAV_MP2_ISO13818_4_DIR` and skips silently when unset.
  Results, pinned by the assertions:
  - The two §2.5.4 *accuracy* bitstreams (`test34` LSF 24 kHz,
    `test35` MPEG-1 44,1 kHz — −20 dB sine sweeps with 24-bit
    reference PCM, the exact setup §2.5.4.1 prescribes) pass the
    normative "ISO/IEC 13818-3 audio decoder" criterion with ~70×
    headroom: RMS ≤ 1,3·10⁻⁷ against the 1/(2¹⁵·√12) ≈ 8,8·10⁻⁶
    bound, max abs ≤ 7,6·10⁻⁷ against the 2⁻¹⁴ bound.
  - All fifteen 16-bit-reference Layer II cells (MPEG-1 44,1/48 kHz
    stereo up to 384 kbit/s incl. CRC-protected frames; LSF
    16/22,05/24 kHz from the 16 kbit/s ladder floor to 160 kbit/s,
    rotating per-frame joint-stereo bounds 4/8/12/16 ↔ stereo,
    single-channel and dual-channel) agree with the supplied
    reference PCM to **≤ 1 s16 LSB at every sample** (the §2.5.4.1
    max-abs bound allows 2), five of them ≥ 99,9 % bit-exactly;
    per-cell bit-exact floors are pinned. The comparison honours
    §2.5.4.1's P′-bit rule: 16-bit references are compared in the
    saturating s16 domain (several suite streams carry deliberate
    near-full-scale content whose float reconstruction overshoots
    ±1,0 and clips in any 16-bit rendering — the sweep's one real
    finding, resolved as measurement-domain, not decode, divergence).
  - Every remaining Layer II multichannel base stream (whose matrixed
    per-channel references cannot be compared against a two-channel
    base decode), including both VBR streams, decodes cleanly to the
    exact frame-count sample total.
  - Stream premises (rotating modes really rotate, `test30`/`test31`
    are single/dual channel, CRC protection present) are pinned from
    the wire, mirroring the staged-fixture premise pins.

- **J.17 absolute-gain convention resolved from the Recommendation
  itself (ask #256) and pinned by test.** The staged ITU-T
  Rec. J.17 (11/88) PDF settles what was previously an
  implementation-chosen convention: Table 1/J.17 tabulates the
  pre-emphasis insertion loss absolutely (18.75 dB at DC, 13.10 dB at
  800 Hz, 0 dB at `f → ∞`), the Recommendation's closing Note
  delegates absolute programme level to the per-equipment
  Recommendations (the "6.5 dB at 800 Hz" figure is ITU-T J.34 §2's
  equipment alignment — a flat 6.60 dB slide that cancels in any
  matched pre-/de-emphasis pair), and ISO/IEC 11172-3 cites only
  J.17, so the decoder inherits the shape alone. The DC-unity
  normalisation this crate already implemented is now the *ruled*
  convention — the only one consistent with 11172-3's −1.0 … +1.0
  decoder-output-range clause. New `j17` tests lock the resolution:
  every Table 1/J.17 row against the analytic curve in the DC-unity
  frame, every fitted per-rate digital cascade against Table 1 via
  the Recommendation's own ± 0.25 dB / 800 Hz-alignment acceptance
  procedure, a pure-attenuator (`|H(f)| ≤ 1`) sweep at all six Layer
  II rates, and a shape cross-check against the staged note's 5-pole
  44.1 kHz reference fit (alongside the existing 3-pole check).

- **Emphasis header-rewrite probe on the real staged fixture + a CRC
  protection pin.** The staged note's §5 survey shows the surveyed
  third-party decoders parse-and-discard `emphasis` (byte-identical
  PCM under a header rewrite), so no external PCM fixture can
  validate de-emphasis; `tests/deemphasis.rs` now runs that exact
  rewrite ('00' → '01' and '00' → '11', walking real padded frame
  boundaries) against the staged `layer2-stereo-44100-192kbps`
  fixture and requires the patched decode to equal the plain decode
  passed through the reference filter sample-for-sample — the
  44,1 kHz J.17 fit exercised inside the full pipeline on real
  content. A companion test pins that the emphasis bits sit *inside*
  the §2.4.1.4 / Table B.5 CRC-protected header half (`bitrate_index`
  through `emphasis`): tampering them on a CRC-protected frame is
  detected as `CrcMismatch`, while the same edit on a CRC-absent
  frame decodes (the property the rewrite probes rely on).

- **§2.4.1.8 `ancillary_data()` is now surfaced on decode.**
  `DecodedFrame` gains an `ancillary: Ancillary` field capturing the
  raw frame tail — the exact §2.4.2.8 `no_of_ancillary_bits` count,
  the sub-byte residue left by the (non-byte-granular) §2.4.3.3.4
  sample loop, and the whole tail bytes to the frame end — closing
  the asymmetry with the long-standing encode-side
  `encode_frame_with_ancillary` path. `tests/ancillary.rs` round-trips
  an encoder payload byte-for-byte (zero residue, payload at the
  first whole tail byte, §2.4.2.1 zero-fill beyond), pins that the
  tail is **outside** the Table B.5 CRC-protected region (rewriting
  tail bytes of a CRC-protected frame decodes cleanly with
  bit-identical PCM — the dual of the emphasis-bits pin), and checks
  the §2.4.2.8 length identity on every frame of the staged
  black-box-encoded fixture. The free-format off-ladder test
  additionally pins that a grown slot lands in the surfaced tail (one
  extra byte, +8 bits, unchanged residue), and the `decode` fuzz
  target now asserts the length identity on every successful
  `decode_frame` — raw and crafted — so the tail extraction sits on
  the fuzzed surface as an invariant, not just as panic-freedom
  (bounded local runs: ≈ 2.1 M execs, zero findings).

- **Per-frame ancillary payloads in the batch encoder.**
  `encode_all_frames_with_ancillary` copies one §2.4.1.8 payload into
  each frame's tail (`per_frame_ancillary[f]` → frame `f`; the list
  length must equal the frame count, else the new
  `EncodeError::AncillaryFrameCountMismatch`), and
  `encode_frame_auto_with_ancillary` is its per-frame auto-SMR
  building block (the §D.1 counterpart of
  `encode_frame_with_state_and_ancillary`). The batch output is
  test-pinned byte-identical to a hand-rolled loop threading the same
  `PaddingScheduler` + `EncodeFrameState` at 44,1 kHz (payloads
  coexist with the §2.4.2.3 padding schedule), with every payload
  recovered through `DecodedFrame::ancillary`.

- **Full four-column Annex D Table D.1d/e/f transcription
  (`tables_d2::LtqEntry`)**. The Layer II "Frequencies, critical band
  rates and absolute threshold" tables now carry the printed
  `Frequency [Hz]` and `Crit.Band Rate [z]` columns alongside the FFT
  line and `LTq` — the §D.1 Step 6 `z(i)` and Step 8 `f(i)` are
  spec-cited to these very tables. New in-module cross-checks pin the
  frequency column to the Fs/1024 analysis-FFT grid, the Bark column
  to strict monotonicity, and every D.2 critical-band-boundary row to
  its D.1 row (the `index F&CB` column is a **D.1 subsampled index**,
  not a raw FFT line — now enforced by test at all rates).

- **ISO/IEC 13818-3 Annex D LSF Model-1 tables (`tables_lsf`)**. The
  13818-3 standard carries its own Annex D ("Psychoacoustic Model 1
  for Lower Sampling Frequencies") with Layer II tables for 16 /
  22,05 / 24 kHz: Tables D.1d/e/f ("Frequencies, critical band rates
  and absolute threshold", 132 entries each, topping at FFT line 480)
  and D.2d/e/f ("Critical band boundaries", 21 / 23 / 23 bands —
  matching the Step 4(c) prose counts exactly). Transcribed with
  machine validation against the `line·Fs/1024` frequency grid, Bark
  monotonicity, the 1/2/4/8 subsampling map, and the D.2 → D.1 row
  cross-print (all four checks also run as unit tests).

- **ISO/IEC 13818-3 Annex D LSF Model-2 partition tables
  (`tables_model2::TABLE_LSF_D_3A/B/C_CALC_PARTITION_*`)**. The
  13818-3 D.2 clause replaces the Model-2 partition tables for the
  LSF rates with Tables D.3.a/b/c (24 / 22,05 / 16 kHz "long
  blocks", printed in the Layer III column layout). Carried in the
  Layer I/II `CalcPartition` form with documented, test-pinned column
  derivations: ω-ranges from the cumulative `FFT-lines` counts (60 /
  60 / 56 partitions covering lines 1–491 / 1–489 / 1–509), printed
  `bval`/`minval` verbatim, and `tmn` via the relation
  `max(24,5, bval + 14,5) dB` — which reproduces the printed TMN
  column of all 164 partitions of the 11172-3 Layer II D.3 tables
  (new pinning test). Step-(l) per-line absolute thresholds for the
  LSF rates are served from the 13818-3 D.1d/e/f transcriptions
  (`lsf_abs_threshold_layer2_16/22k05/24`; 13818-3 prints no
  D.4-style table, and at the MPEG-1 rates D.4 ≡ the Layer II D.1
  threshold-in-quiet column entry-for-entry bar documented print
  errata).

- **LSF encoding is now psychoacoustically driven — both models, all
  six Layer II rates.** `SamplingRate` gains the `Fs16kHz` /
  `Fs22k05Hz` / `Fs24kHz` variants (with `is_lsf()`), and
  `psy::annex_d_sampling_rate` maps every Layer II sampling frequency,
  so the `encode_frame_auto*` / `encode_all_frames*` families and the
  registry encoder derive real SMR tables at 16 / 22,05 / 24 kHz
  instead of the former flat-0 dB fallback (which now only covers
  non-Layer-II frequencies). LSF-specific §D.1 adaptations per the
  13818-3 printed text: rate-dependent Step 4(b) tonality
  neighbourhoods (`tonal_neighbourhood_layer2_for_rate` — `j = ±4`
  innermost row, three rows, `4 < k < 500` domain, with `is_tonal` /
  `zero` / `list_tonal` `_for_rate` companions), and Step 3 applied
  **without** the 11172-3 overall-bit-rate −12 dB offset (the 13818-3
  Step 3 text omits it). New tests pin the LSF neighbourhood rows, a
  signal-shaped (non-flat) Model-1 and Model-2 SMR at every LSF rate,
  and auto-vs-flat allocation divergence in a real LSF encode.

- **LSF psychoacoustic conformance suite
  (`tests/lsf_psy_conformance.rs`)**. The LSF counterpart of the
  MPEG-1 allocation-shaping suite plus measured round-trip SNR
  floors: at every LSF rate the Model-1/Model-2 encodes differ from
  the flat (rate-driven) encode byte-wise, in the first-frame
  per-subband allocation, and from each other, and both models
  **beat** the flat baseline's delay-aligned time-domain SNR by
  ≥ 2,7 dB measured (16 kHz: 6,07 → 9,43 dB; 22,05 kHz: 8,92 →
  11,95 dB; 24 kHz: 10,17 → 12,97 dB at 64 kbit/s stereo; asserted
  floors sit ~3 dB under the measured values with a 1 dB
  beats-flat margin).

- **r419 fixture corpus: independent validation of
  psychoacoustically-driven and intensity streams** (11 new cells,
  `tests/fixtures/`, `examples/gen_conformance_fixtures.rs`,
  `tests/decode_matrix_conformance.rs`). Model-1 and Model-2 encodes
  at 16 / 22,05 / 24 kHz (plain stereo and joint-stereo intensity at
  bounds 8 / 12 / 16), one MPEG-1 Model-2 joint-stereo cell
  (previously uncovered combination), the Annex G.1 demand-driven
  per-frame policy at a starved bitrate (every frame elects joint
  stereo), and an Annex G.1 sum-signal content pin (channel 1 carries
  a 0,19·Fs tone channel 0 lacks; the decoded right channel must keep
  it). Both independent black-box decoders accept every stream;
  against the float reference the decode agreement is **max
  ≤ 0,0171 LSB** with ≥ 99,66 % bit-exact s16 projection (asserted at
  the corpus-wide 0,05-LSB bound). Premises (mode, live intensity,
  demand-driven joint-stereo election, right-only content split) are
  pinned bitstream-side by `r419_lsf_psy_fixture_premises_hold`.

### Fixed

- **§D.1 Step 4(c) critical-band ranges resolved to the raw FFT-line
  domain (`psy::critical_band_line_ranges`).** The Table D.2 boundary
  tables print D.1 *subsampled indices*, which the non-tonal grouping
  previously consumed as raw FFT lines — truncating the Step 4(c)
  sweep at (e.g.) line 126 instead of 432 at 48 kHz, so every
  non-tonal masker above ~5,9 kHz was silently dropped and Bark
  positions above the printed index saturated early. The boundaries
  are now resolved through the same rate's D.1 table (`index F&CB` →
  D.1 row → top FFT line), the non-tonal bands tile the full
  `[0, top-of-band]` line range, and `bark_for_line_layer2` reads
  `z(i)` from the D.1 `Crit.Band Rate` column per the Step 6 prose
  ("the critical band rates z(j) and z(i) can be found in tables
  D.1…") instead of quantizing to the D.2 band-top Bark. Non-tonal
  maskers are now positioned at the Bark of their representative
  (geometric-mean) line, as Step 4(c) lists them.

- **Table D.2e band 17 Bark: 16.116 → 16.110.** The staged extract's
  resolved errata (D.2e prints `16,11`; the D.1e i = 62 cross-print
  reads 16,110 — a dropped trailing zero, not a clipped digit)
  supersedes the earlier best-fit guess; the new D.2 ≡ D.1
  row-for-row cross-check test caught the stale in-tree value.

- **r411 whole-stream decode-conformance corpus
  (`tests/fixtures/`, `tests/decode_matrix_conformance.rs`)**. Sixteen
  new black-box-encoded Layer II streams widen the reference-validated
  matrix along the bit-allocation axis: every Table 3-B.2 sub-table
  (B.2a/b/c/d) in both channel modes, the bitrate-ladder extremes
  (32 kbit/s mono … 384 kbit/s stereo, LSF 8 … 160 kbit/s), the
  LSF-only 144 kbit/s index, and heavy §2.4.2.3 padding at the
  fractional rates (up to 22 of 23 frames padded at 44.1 kHz). The new
  cells store the reference decoder's **float** PCM (`pcm_f32le`),
  tightening the assertable decode bound from the ISO ±1 LSB envelope
  to **≤ 0.05 LSB in the float domain** (measured ≤ 0.025 LSB across
  all cells — the reference's own f32 precision floor) plus a ≥ 99 %
  bit-exact s16 projection with residual flips confined to
  rounding-boundary straddles. A two-independent-reference latitude
  study (recorded in `tests/fixtures/GENERATION.md` with the exact
  generation commands and SHA-256 sums) shows the references disagree
  with *each other* at the same ±1 s16 magnitude, pinning the residual
  divergence on ISO/IEC 11172-4's bounded-difference rounding latitude
  rather than any decode-chain difference; one cell (`mono_16k_8`)
  reaches 100 % s16 bit-exactness against the second reference.

- **Joint-stereo / dual-channel / CRC conformance cells + generator
  example (`examples/gen_conformance_fixtures.rs`)**. No black-box
  encoder emits Layer II `joint_stereo` / `dual_channel` / CRC frames,
  so ten further corpus cells are encoded by this crate's own public
  batch API and reference-decoded by the independent black-box
  decoders — closing the README's long-standing gap: a stream with a
  **live §2.4.1.6 intensity-stereo bound** is now validated against an
  independent reference (every `mode_extension` bound incl. narrow
  B.2d, the B.2c bound-clamp edge, and LSF joint stereo), plus
  dual-channel and §2.4.1.4 CRC-protected streams (both independent
  decoders accept the CRC cell). Float-domain agreement ≤ 0.024 LSB
  across all ten; a new `r411_js_dual_crc_fixture_premises_hold` test
  pins the fixture premises (per-frame mode, clamped bound, allocated
  above-bound subband, CRC presence) directly from the bitstream.

- **§2.4.2.4 output de-emphasis on decode (`src/deemphasis.rs`,
  `src/frame.rs`)**. The header emphasis field ("indicates the type of
  de-emphasis that shall be used") was parsed but never applied. The
  decoder now runs a first-order de-emphasis IIR on the reconstructed
  PCM when the header signals the 50/15 µs (`'01'`) curve; the
  coefficients are derived clean-room from the `τ1 = 50 µs` /
  `τ2 = 15 µs` time constants via the bilinear transform (unity DC gain,
  −10.458 dB HF shelf). Per-channel filter state is threaded across
  frames through `FrameDecodeState` and re-zeroed on `reset()`. The
  `'00'` (none) mode is delivered unfiltered. New: `deemphasis` unit
  tests + a `tests/deemphasis.rs` integration test that patches an
  unaltered stream's header emphasis bits and pins the decode against
  the reference filter.

- **§2.4.2.4 CCITT J.17 de-emphasis / pre-emphasis (`src/j17.rs`,
  `src/deemphasis.rs`)**. The `'11'` emphasis mode is now implemented
  on both decode and encode from the staged
  `docs/audio/mp3/mpeg-audio-emphasis-j17-deemphasis.md` note: a
  first-order shelf (pre-emphasis zero ≈ 477.5 Hz, pole ≈ 4134 Hz,
  `10·log10(75) = 18.75 dB` asymptote span, ± 0.25 dB tolerance),
  normalised here to unity DC gain like the 50/15 µs pair. Because the
  bilinear warp breaks a single digital section's tolerance across the
  Layer II rates, the realisation is an order-3 minimum-phase cascade
  of real-pole/zero sections fitted **per sample rate** by a
  Levenberg–Marquardt log-magnitude fit seeded from the bilinear shelf
  (`DeEmphasis`/`PreEmphasis` are now section cascades; `Section` is
  public). The fit stays < 0.02 dB from the analytic curve at all six
  rates (measured to Nyquist on an independent dense grid) and the
  44.1 kHz result is cross-checked against the note's reference 3-pole
  fit; the pre→de cascade is identity to machine precision. New:
  `j17` unit tests (fit tolerance / stability / minimum phase /
  reference cross-check) plus `tests/deemphasis.rs` J.17 header-patch
  and acoustic round-trip integration tests.

- **§2.4.2.4 pre-emphasis on encode (`src/deemphasis.rs`,
  `src/encoder_frame.rs`)**. Symmetric encode counterpart: when a header
  signals the 50/15 µs or CCITT J.17 curve the encoder pre-emphasises
  the PCM (per channel, state threaded through `EncodeFrameState`)
  before both the §C.1.3 analysis filterbank and the Annex D
  psychoacoustic model. `PreEmphasis` is the exact algebraic inverse of
  `DeEmphasis` (per-section inversion, `PreEmphasis::for_header` mirrors
  the decode-side selection); the pre→de cascade is identity to machine
  precision, verified across all six Layer II rates, plus acoustic
  encode→decode round-trip tests for both curves.

- **Registry-encoder `emphasis` + `copyright` / `original` / `private`
  options (`src/codec_encoder.rs`)**. `make_encoder` gains
  `emphasis=50/15` and `emphasis=j17` (applies the matching §2.4.2.4
  pre-emphasis and signals the field; default `none`) and the three
  §2.4.2.3 header metadata flags as booleans (round-tripped verbatim on
  decode). Unrecognised values are rejected; both are covered by
  round-trip tests.

- **Emphasis hardening (`tests/deemphasis.rs`,
  `fuzz/fuzz_targets/decode.rs`)**. A mid-stream emphasis-switching
  integration test pins the decoder's per-channel filter lifecycle
  ('01'→'01' carries IIR state across the frame boundary, '01'→'11'
  rebuilds a fresh J.17 filter, →'00' drops it), and an LSF-rate
  (24 kHz) J.17 encode→decode acoustic round-trip exercises an LSF
  filter fit inside the real pipeline. The decode fuzz target's crafted
  headers now draw the emphasis field from all three accepted §2.4.2.3
  codes instead of hardcoding '00', so both de-emphasis IIRs and their
  cross-frame state machine sit on the fuzzed attacker surface
  (320k-run bounded session clean).

- **§2.4.2.3 padding-bit rate control (`PaddingScheduler`,
  `src/header.rs` / `src/encoder_frame.rs` / `src/codec_encoder.rs`)**.
  The encoder previously emitted every frame unpadded, so at the
  fractional rates (44,1 / 22,05 kHz — where `144·bitrate/Fs` is not an
  integer) the emitted stream undershot the signalled bitrate ("Padding
  is necessary with a sampling frequency of 44,1 kHz"). The new public
  `PaddingScheduler` implements the spec's verbatim `rest`/`dif`
  decision procedure (first frame forced unpadded; thereafter
  `dif = (144·bitrate) mod Fs`, `rest −= dif`, borrow-and-pad when
  negative), keeping the accumulated coded length strictly within one
  slot of the exact `Σ 144·bitrate/Fs` target at every frame boundary.
  Both the batch `encode_all_frames` family and the registry
  `Mp2CoreEncoder` now drive one internally, interleaving `N+1`-slot
  padded frames with `N`-slot frames at 44,1 / 22,05 kHz (at the
  evenly-dividing rates `dif == 0` and the output is byte-identical to
  before); a hand-rolled `encode_frame_auto_with` loop reproduces the
  batch bytes by threading `PaddingScheduler::next_header`. New
  coverage: scheduler unit tests (first-frame rule, `dif == 0` rates
  never pad, the sub-slot accumulated-length envelope over 2000 frames,
  reset replay, `next_header` field hygiene) plus
  `tests/padding_rate_control.rs` — a frame-walk of the emitted batch
  stream against the algorithm's prescription, the mean-bitrate
  envelope, decode symmetry, registry packet sizing, and a **padded
  free-format** stream (bitrate nibble cleared per frame) resolving
  through the §2.4.2.3 `N` / `N+1` sync-to-sync measurement to
  bit-identical PCM ("Padding may also be required in free format").

- **Mixed-bitrate stream decode pin
  (`tests/roundtrip_multirate.rs`)**. §2.4.2.3 leaves variable-bitrate
  support optional for a Layer II decoder ("the decoder is not
  required to support a continuously variable bitrate when in Layer I
  or II"); ours sizes and allocates every frame from its own §2.4.1.3
  header, so a stream whose frames switch ladder bitrates (192 →
  256 kbit/s, crossing B.2 sub-tables) decodes frame-by-frame with the
  exact sample count. Now pinned by
  `mixed_bitrate_stream_decodes_frame_by_frame`.

- **§D.1 Step-1 delayed analysis window (net 192-sample delay,
  `src/encoder_frame.rs`)**. The Model-1 auto-SMR chain previously fed
  the 1024-point FFT with each frame's first 1024 samples; the spec's
  Step 1 says "for a coincidence in time between the bit allocation and
  the corresponding subband samples, the PCM-samples entering the FFT
  have to be delayed" — 256 samples compensating the §C.1.3 analysis
  filterbank delay (item a) minus 64 for centring the 1024-point window
  in the 1152-sample Layer II frame (item b). Frame `f`'s window is now
  `stream[f·1152 − 192 .. f·1152 + 832]`: a per-channel 192-sample
  history (new `EncodeFrameState` field, zero-filled at stream start
  and on `reset()`, matching the filterbank's zeroed X buffer) followed
  by the frame's first 832 samples. New public
  `MODEL1_WINDOW_DELAY_SAMPLES` constant; the batch and registry
  Model-1 paths inherit the alignment automatically. Tests pin the
  256 − 64 = 192 arithmetic against the existing psy constants, the
  window layout, that the history genuinely reaches the FFT (two states
  differing only in history produce different SMR tables), the
  fresh-state zero-padded-head equivalence against a hand-built window
  through `compute_smr_model1_frame`, and the history advance to the
  frame tail. This resolves the README's "window shift needs lookahead"
  refinement note — the spec asks for a *delay*, which needs history,
  not lookahead.

- **Annex G.1 demand-driven automatic joint-stereo selection
  (`src/encoder_bit_allocator.rs` / `src/encoder_frame.rs` /
  `src/codec_encoder.rs`)**. Implements the Annex G.1 encoder flow —
  "an estimation is made of the required bitrate for both left and
  right channel. If the required bitrate exceeds the available bitrate,
  the required bitrate can be decreased by setting a number of subbands
  to intensity stereo mode" — as a per-frame coding-mode policy. New
  allocator primitives: `demand_bits` (the §C.1.5.2.7 cost of bringing
  every slot to `MNR ≥ 0` — merged intensity slots pay one shared
  codeword sized against the more demanding channel per G.1's
  "higher of the bit allocations", plus both channels' scalefactor
  overhead) and `available_data_bits` (the `adb` identity). New
  `choose_stereo_coding` walks full `Stereo` → `JointStereo`
  Bound16/12/8/4 and returns the widest candidate whose demand fits
  (Bound4 as the final fallback), preserving every other header field.
  New encode entry points `encode_frame_auto_js_with` /
  `encode_frame_auto_js_model2` / `encode_all_frames_js` apply the
  choice after the psychoacoustic model resolves each frame's SMR (the
  filterbank/SMR are mode-independent, so the substitution is safe),
  legally mixing `Stereo` and `JointStereo` frames in one stream
  (§2.4.1.3 — each frame carries its own `mode`). The registry
  encoder's `bound` option gains an `"auto"` value (requires
  `mode=joint_stereo`; works with both `psymodel`s). Tests: demand
  exactness on a one-hot table walk, zero demand at non-positive SMR,
  monotone non-increase as the bound narrows, the `adb` identity,
  choice monotonicity over a rising-SMR sweep with the Bound4 fallback,
  single-channel pass-through, a same-tone 384 kbit/s → `Stereo` vs
  192 kbit/s → `JointStereo` frame pin, a batch tone-comb stream that
  intensity-codes every frame at 96 kbit/s and full-stereo-codes at
  384 kbit/s (mode-mixed streams decode cleanly), and registry
  `bound=auto` acceptance/rejection wiring.

- **§C.1.5.2.7 merged-slot sample cost counted once
  (`src/encoder_bit_allocator.rs`)** *(Fixed)*. The allocator charged
  the joint-stereo merged slot's marginal sample bits **twice**
  (`d_bspl *= 2`) even though §2.4.1.6 puts a *single* shared triplet
  on the wire for `bound <= sb < sblimit` (§2.4.2.6) — which is exactly
  what the frame writer emits. The double charge made the allocator
  stop early, wasting one full copy of the committed above-bound sample
  bits (≈1600 bits ≈ 32 % of the data budget in a 192 kbit/s Bound4
  frame) as dead §2.4.1.8 tail instead of spending it on quantization
  depth. The merged slot now pays the shared codeword once (scalefactor
  + scfsi overhead still counts both channels, per the syntax). New
  regression `joint_stereo_allocator_saturates_the_budget_at_single_
  shared_codeword_cost` re-parses an encoded joint-stereo frame,
  recomputes the actual on-wire spend, and bounds the leftover by the
  worst-case-scalefactor slack + one unaffordable step — it fails
  against the double-charged allocator (1602 bits unused vs the 1336
  permitted) and passes now.

- **Annex G.1 sum-signal intensity-stereo encode
  (`src/encoder_frame.rs`)**. In the `bound <= sb < sblimit` intensity
  region the shared on-wire codeword is now the Annex G.1 **sum
  signal** `L + R`, quantized against the sum's own (untransmitted)
  scalefactor — "instead of transmitting separate left and right
  subband samples, only the sum-signal is transmitted, but with
  scalefactors for both the left and right channels". Previously the
  encoder wrote channel 0's samples as the shared codeword, which
  silently *discarded* channel 1's above-bound content (a
  right-channel-only tone decoded to near-silence). Each decoder
  channel still rescales the shared codeword by its own §2.4.3.3.3
  transmitted scalefactor, reproducing the sum's temporal envelope at
  each channel's original level; the sum's ≤ 2.0 amplitude is covered
  by Table 3-B.1 index 0. New
  `intensity_sum_signal_preserves_right_only_content_above_bound`
  integration test (all six wide-table rates) pins that a channel-1-only
  tone above `Bound4` survives the encode at close to its input RMS,
  localises spectrally, and leaves the silent channel 0 quiet — it
  fails against the previous channel-0-codeword implementation. All
  existing joint-stereo round-trip / pan / silence / clamp tests pass
  unchanged.

- **`crc` registry-encoder option (`src/codec_encoder.rs`)**. The
  registry encoder hard-coded `protection_bit = '1'` (no CRC), so the
  crate's §2.4.1.4 / §2.4.3.1 CRC *write* path (already used by the
  direct `encode_frame` API when handed a protected header) was
  unreachable through `make_encoder`. A new `crc` codec option
  (`"true"` / `"1"`) sets `protection_bit = '0'` on the stream header,
  making every emitted frame carry the 16-bit CRC word over the Annex B
  Table B.5 protected fields. Two unit tests pin that protected packets
  parse with `protection_bit == '0'`, decode cleanly through
  `decode_all_frames`, and that a corrupted bit-allocation byte is
  detected as `FrameError::CrcMismatch`; the default stays
  unprotected and an unrecognised `crc` value is rejected at build
  time.

- **§2.4.2.3 free-format (`bitrate_index == '0000'`) decode
  (`src/freeformat.rs`, `src/frame.rs`)**. Free-format Layer II streams
  signal no bitrate in the header; the constant frame size is recovered by
  measuring the distance between consecutive syncwords (§2.4.2.3 "a frame
  contains either N or N+1 slots, depending on the value of the padding
  bit"), with a two-frame sync-lock that rejects false-positive sync
  patterns inside the audio payload. The recovered base slot count `N` is
  inverted through the §2.4.3.1 size formula to recover the constant
  bitrate, which selects the standard Annex B bit-allocation table; an
  off-ladder free-format bitrate (whose Annex B table the standard leaves
  undefined) is reported as `FreeFormatError::UnsupportedBitrate` rather
  than guessed. New public surface: `FrameHeader::parse_allow_free_format`,
  `FrameHeader::is_free_format`, the `freeformat` module
  (`measure_base_slots` / `bitrate_from_base_slots` / `resolve` /
  `header_with_recovered_bitrate` / `FreeFormatLayout` / `FreeFormatError`),
  `frame::decode_free_format_stream`, and
  `frame::decode_frame_with_known_header`. A new `FrameError::FreeFormat`
  variant carries the determination error. `tests/free_format.rs` proves a
  free-format stream (synthesised by rewriting the encoder's standard-rate
  output's `bitrate_index` nibble to `'0000'`) decodes **bit-exact
  identically** to the standard-bitrate stream across MPEG-1 stereo /
  single-channel and MPEG-2 LSF rates, that padded frames are sized per
  their own padding bit, and that an off-ladder size is rejected.

- **Free-format support in the registry decoder (`src/codec_decoder.rs`)**.
  `Mp2CoreDecoder::send_packet` now transparently handles free-format
  packets: because a demuxer hands one frame per packet, the packet length
  is the frame size, so the constant bitrate is recovered directly from
  `packet_len − padding_bit` (no sync-to-sync measurement needed) and the
  recovered-bitrate header drives the standard decode path. A new unit test
  confirms a free-format packet decodes byte-identically to the equivalent
  standard-bitrate packet through the `Decoder` trait.

- **`freeformat` registry-encoder option (`src/codec_encoder.rs`)**.
  `Mp2CoreEncoder` now accepts a `freeformat` `CodecParameters::options`
  key (`"true"` / `"1"`); when set it emits §2.4.2.3 free-format frames
  (`bitrate_index == '0000'`) at the configured constant bitrate by
  clearing each emitted frame's bitrate_index nibble. This completes the
  free-format story symmetrically through the registry (the decoder
  already handles free-format packets). A unit test confirms the produced
  stream parses as free format and decodes byte-identically to the
  standard-bitrate encode; an unrecognised `freeformat` value is rejected.

- **Free-format robustness suite (`tests/free_format_robustness.rs`)**.
  Panic-freedom coverage for the §2.4.2.3 free-format size-measurement
  surface — `measure_base_slots`, `resolve`, `decode_free_format_stream`,
  and `FrameHeader::parse_allow_free_format` — against dense sync runs
  (the worst case for the sync-lock scanner), every truncated prefix of a
  free-format frame, a 2000-case deterministic pseudo-random corpus, and
  empty / tiny buffers. Every entry point always returns a `Result`.

- **Free-format encode path (`src/freeformat.rs`)**.
  `rewrite_to_free_format` / `to_free_format` convert a standard-bitrate
  Layer II stream to §2.4.2.3 free format by clearing each frame's
  `bitrate_index` nibble to `'0000'` — the §2.4.3.1 free-format frame size
  at a ladder bitrate is byte-identical to the standard frame's size, so
  the payload and frame boundaries are untouched and the constant bitrate
  is recoverable on decode. Paired with the standard encoder, this emits a
  free-format stream that round-trips through `decode_free_format_stream`
  to bit-identical PCM (`tests/free_format.rs`).

- **Registry-encoder codec options (`src/codec_encoder.rs`)**. The
  `Mp2CoreEncoder` now reads three `CodecParameters::options` keys so the
  already-built joint-stereo, dual-channel and Model-2 capabilities are
  reachable through the registry (previously the registry encoder could
  only emit plain stereo + Model-1): `mode` (`stereo` / `joint_stereo` /
  `dual_channel` for 2-channel streams), `bound` (joint-stereo intensity
  bound `4` / `8` / `12` / `16`), and `psymodel` (`model1` / `model2` —
  routing `encode_one` through `encode_frame_auto_with` or
  `encode_frame_auto_model2`). Unrecognised values and channel-count /
  mode mismatches are rejected at `make_encoder` time. Four new unit
  tests cover the mode selection round-trip, every intensity bound, the
  Model-1 ≠ Model-2 byte divergence, and option rejection.

- **Psychoacoustic-model allocation-shaping proof
  (`tests/psy_model_shapes_allocation.rs`)**. Two integration tests pin
  that the §D.1 Model-1 and §D.2 Model-2 auto-SMR chains genuinely
  influence the §C.1.5.2.7 bit allocation, not just produce a
  syntactically-valid frame: for a structured two-tone-plus-noise signal
  at a constrained 128 kbit/s, both perceptual encodes differ
  byte-for-byte from the flat-0 dB-SMR baseline *and* differ in the
  first-frame per-subband `nb_steps` allocation, while still decoding
  back to a stream whose dominant 600 Hz masker tone is preserved. A
  second test asserts Model-1 ≠ Model-2 output for the same input,
  guarding against an accidental wiring that routes both auto paths
  through one model.

- **Genuine intensity-stereo per-channel-level decode test
  (`tests/joint_stereo_matrix.rs`)**. The existing joint-stereo
  round-trip tests feed both channels identical input, so their
  above-`bound` scalefactors coincide and a decoder bug that reused
  channel 0's scalefactor for both channels would still pass. The new
  `joint_stereo_reconstructs_per_channel_levels_in_the_intensity_region`
  encodes a 6 dB intensity pan (channel 1 = half-amplitude copy of
  channel 0) on a tone whose subband clears `Bound4`, then asserts the
  decoded channel-1/channel-0 RMS ratio reflects the pan (well below
  1.0) across the whole wide-table rate matrix — direct evidence the
  §2.4.3.3.3 Region-2 loop rescales the shared codeword by **each
  channel's own** scalefactor (frame.rs:350-358), not a shared one.

- **`oxideav_core::Encoder` registry wiring (`src/codec_encoder.rs`)**.
  Adapts the `encode_frame_auto_with` primitive into the framework's
  frame-in / packet-out `Encoder` trait — the encode-side dual of
  `Mp2CoreDecoder`. `make_encoder` builds an `Mp2CoreEncoder` whose
  `FrameHeader` is fixed at construction from `CodecParameters`
  (sample_rate ∈ the six Layer II rates, channels 1→SingleChannel /
  2→Stereo, optional bit_rate with a per-rate `default_bitrate_bps`
  fallback), validating the §2.4.2.3 bitrate/mode matrix and the
  §2.4.3.1 allocation-table coverage up front. `send_frame` decodes
  planar S16 → f64 `[-1,+1]`, accumulates per channel, and emits one
  Layer II packet (`frame_size_bytes()` bytes, keyframe-flagged,
  sample-count pts/duration) every 1152 samples; `flush` zero-pads a
  partial trailing frame. `register_codecs` now carries both factories
  under `"mp2"` with `with_encode()` — the registry exposes MP2 encode
  for the first time. Ten unit tests cover parameter validation
  (bad channels / missing or non-Layer-II rate / §2.4.2.3 matrix
  violations / per-rate defaults), the 1152-sample buffering boundary,
  flush zero-padding, post-flush rejection, planar-shape rejection, and
  a full registry encode → `decode_all_frames` round-trip with tone
  localisation.

- **§D.2 Model-2 auto-SMR encoder selection
  (`encode_frame_auto_model2` / `encode_all_frames_model2`,
  `src/encoder_frame.rs`)**. Wires the §D.2.1 Layer II twice-per-frame
  Model-2 driver (`compute_smr_model2_layer2_frame`) into the encoder
  as a selectable SMR source — the Model-2 counterpart of the existing
  §D.1 `encode_frame_auto` family. A new `SmrSource::AutoModel2` variant
  routes through `compute_auto_smr_table_model2`, which drives the
  twice-per-frame threshold generator once per channel. Because Model 2
  is **stateful** (rolling two-block spectral predictor + 448-sample
  inter-call carry per channel), the per-channel `Model2Layer2State`
  vector is threaded through `EncodeFrameState` alongside the §C.1.3
  analysis filterbank — and reset by `EncodeFrameState::reset`. For the
  MPEG-2 LSF rates (no Annex D Layer II masking tables) the SMR
  degenerates to a flat 0 dB table, identical to the Model-1 fallback.
  Four integration tests (`tests/model2_encode.rs`) pin: the full
  encode → decode round-trip envelope (shape / reconstruction energy /
  spectral localisation) across the whole rate matrix; exact-zero
  silence; batch-equals-hand-rolled-stateful-loop byte equality; and a
  divergence test proving the rolling predictor history genuinely
  influences later frames (a fresh-state re-encode of the same frame
  differs from the streamed continuation). The stale lib.rs "the
  psychoacoustic model is not yet built" note is corrected — both
  Annex D example models now drive the encoder end-to-end at the MPEG-1
  rates.

- **§D.2.1 Layer II twice-per-frame Model-2 driver
  `compute_smr_model2_layer2_frame` (`src/psy.rs`)**. Implements the
  verbatim §D.2.1 rule "In Layer II, the psychoacoustic masking ratios
  must be calculated twice during each coder frame. The more stringent
  of each pair of ratios is used for bit allocation." Runs the
  `compute_smr_model2_frame` chain twice over one 1152-sample frame —
  once per `IBLEN_LAYER2` (= 576) half — reconstructing each call's
  1024-sample analysis window from the 448-sample inter-call carry held
  in a new `Model2Layer2State` (predictor history + sample tail,
  zeroed-startup), then returns the per-subband **maximum** of the two
  SMR tables (the more-stringent ratio). Two tests pin the per-subband
  max-dominance over a single call and the carry/predictor advance
  across streamed frames. With this, the §D.2 Model-2 SMR producer is
  complete end-to-end including the Layer II frame logic; only an
  `encode_frame_auto`-style encoder selection remains.

- **§D.2 Psychoacoustic Model 2 per-frame SMR driver
  `compute_smr_model2_frame` (`src/psy.rs`)**. Chains the full Model-2
  chain — the newly-added analysis front-end (steps a–e) plus the
  existing §D.2.4 step (f)…(n) threshold loop in `src/tables_model2.rs` —
  into a per-subband `SMR_sb(n)` table, the Model-2 counterpart of
  `compute_smr_model1_frame`. Per frame it runs the polar FFT (b), the
  two-block prediction (c) against a caller-owned, frame-streaming
  `Model2PredictorState`, the unpredictability measure (d), the
  partition energy/unpredictability (e), the spreading convolution +
  renormalisation (f), the per-line threshold energy (g…k), the
  absolute-threshold floor (l) — with the dB→energy conversion anchored
  to a +1-lsb-sine FFT reference per the spec's step-(l) note — and the
  step-(n) per-coder-partition SMR, mapping each 16-FFT-line Table D.5
  coder partition `n` (`n ≥ 1`) to subband `n − 1`. Silent/undefined
  partitions degenerate to 0 dB SMR so the allocator's `MNR = SNR − SMR`
  stays finite. Four tests pin tone localisation, all-rate silence
  finiteness, predictor advance across streamed frames, and the
  reference-energy positivity. This is the wiring step the README's "Not
  yet supported" note named: Model 2 is now driven end-to-end to a
  per-subband SMR (an `encode_frame_auto`-style Model-2 encode entry
  point remains for a future round).

- **§D.2 Psychoacoustic Model 2 analysis front-end — steps (a)–(e)
  (`src/psy.rs`)**. Completes the Model-2 chain ahead of its threshold
  loop (steps (f)…(n), already in `src/tables_model2.rs`): the §D.2.4
  step (b) bare raised-cosine analysis window `model2_hann_window_layer2`
  (`h(i) = 0,5 − 0,5·cos(2π(i − 0,5)/1024)`, distinct from Model 1's
  `sqrt(8/3)`-scaled power window), the step (b) polar
  `complex_spectrum_polar_layer2` returning per-bin magnitude `r_ω` and
  phase `f_ω`, the step (c) two-block `Model2PredictorState`
  (`r̂_ω = 2·r(t-1) − r(t-2)`, `f̂_ω = 2·f(t-1) − f(t-2)`, zeroed-startup),
  the step (d) `unpredictability_measure` (`c_ω`, the Cartesian-distance
  ratio between observation and prediction, with the spec's 0,3 default
  for never-excited lines), and the step (e)
  `partition_energy_and_unpredictability` (`e_b = Σ r_ω²`,
  `c_b = Σ r_ω²·c_ω` over each Table D.3 calculation partition). Six unit
  tests pin the window bounds/symmetry, single-tone polar peak recovery,
  the perfect-prediction / orthogonal-prediction `c_ω` limits, the
  zero-denominator fallback, the predictor's zeroed-startup roll, and the
  partition-energy span sums. Sourced verbatim from the staged ISO PDF
  Annex D pages 129–130.

- **Joint-stereo (intensity) + dual-channel mode×rate robustness matrix
  (`tests/joint_stereo_matrix.rs`)**. The existing
  `tests/roundtrip_multirate.rs` exercises only `Mode::Stereo` with
  `ModeExtension::Bound4`; this new suite broadens the channel-mode axis so
  the §2.4.1.6 intensity-stereo region (`bound ≤ sb < sblimit`) and the
  `dual_channel` two-independent-mono path get equal coverage across the whole
  sampling-rate ladder. It pins five properties: (1) joint-stereo encode →
  decode round-trips at **every** `mode_extension` bound (4 / 8 / 12 / 16) ×
  every MPEG-1 and MPEG-2 LSF rate, with a successful frame-size-exact decode
  proving the §2.4.2.6 shared-codeword loop stays bit-aligned through the
  intensity region; (2) the intensity region is genuinely non-empty for the
  wide-table cases (parsed `bound` matches the clamped expectation and at
  least one above-bound subband is allocated, with `nb_steps[0] == nb_steps[1]`
  enforced); (3) the §2.4.2.3 `bound = min(mode_extension_bound, sblimit)`
  **clamp** at the low-rate B.2c (sblimit 8, 48 kHz) / B.2d (sblimit 12,
  32 kHz) tables — `Bound12` / `Bound16` collapse the intensity region to
  empty and the decoder must not read phantom codewords; (4) `dual_channel`
  reconstructs two *independent* tones (a cross-channel leak would put one
  channel's tone in the other); and (5) joint-stereo silence round-trips to
  exact-zero PCM at every bound. Reconstruction is asserted as the ISO
  bounded-difference envelope (sample count, error/signal energy ratio,
  Goertzel spectral localisation), matching `roundtrip_multirate.rs`.
- **Joint-stereo / dual-channel decode-robustness fuzz (in
  `tests/joint_stereo_matrix.rs`)**. The round-trip tests pair our encoder
  with our decoder, so a *shared* bug in the §2.4.1.6 intensity-region loop
  could cancel out. These fuzz tests bypass the encoder entirely: they
  synthesise raw joint-stereo and dual-channel frames with adversarial payload
  byte patterns (all-zero, all-ones max-allocation, and alternating
  bit-walk patterns our encoder would never emit) and assert `decode_frame`
  never panics, never overruns the buffer, and either returns correctly-shaped
  finite PCM or one of the documented `FrameError` variants (matched
  exhaustively, no wildcard). Coverage spans all four `mode_extension` bounds
  at 192 kbit/s (B.2b/B.2a wide tables) and at 64 kbit/s (B.2c/B.2d narrow
  tables where the bound clamps), plus an exhaustive truncated-prefix walk of a
  joint-stereo frame confirming every too-short prefix is rejected gracefully.
- **Full Layer II decode-conformance matrix (`tests/decode_matrix_conformance.rs`)**.
  The §2.4.3 decode chain (header → bit-allocation table → §2.4.3.3.4
  requantization → §2.4.3.3.3 scalefactor rescaling → §2.4.3.2 / Annex A
  Figure A.2 synthesis filterbank) is now validated against an independent
  black-box reference decoder across **every** Layer II channel-mode ×
  sampling-rate combination — MPEG-1 single-channel and stereo at 32 / 44.1 /
  48 kHz, and MPEG-2 LSF at 16 / 22.05 / 24 kHz — not just the single staged
  44.1 kHz stereo stream. Each fixture (`tests/fixtures/<name>.mp2` with its
  reference `<name>.ref.wav`) decodes to within the ISO floating-point
  filterbank conformance envelope (max abs ≤ 1 LSB, rms ≈ 0.5 LSB, ~75 %
  bit-exact). The fixtures are opaque encode→decode products consumed only as
  bytes — see `tests/fixtures/README.md`. A regression localised to one rate's
  allocation table or LSF sizing can no longer hide behind the others. The
  envelope is additionally pinned **per individual 1152-sample frame** (the
  cold-start frame 0, where the §2.4.3.3.5 V ring buffer is zero per Annex A
  Figure A.2 footnote 1, is already within ≤1 LSB on every fixture), and a
  streaming-equivalence check confirms frame-by-frame `decode_frame_with` with
  a persisted `FrameDecodeState` is bit-identical (f64) to the batch
  `decode_all_frames` path — guarding the inter-frame V-buffer threading.

- **Batch stream encode: `encode_all_frames` / `encode_all_frames_with_smr`**.
  The encode-side counterpart of `decode_all_frames`: turns one continuous
  per-channel PCM buffer into the concatenated Layer II byte stream, threading
  a single persistent `EncodeFrameState` (the §C.1.3 analysis-filterbank X ring
  buffer) through every frame so inter-frame continuity is byte-identical to a
  hand-rolled `encode_frame_auto_with` loop. `encode_all_frames` derives each
  frame's SMR via the §D.1 Model-1 chain; `encode_all_frames_with_smr` applies
  a caller table verbatim. A per-channel length that is not a whole multiple of
  `PCM_SAMPLES_PER_CHANNEL` (1152) is rejected with the new
  `EncodeError::ShortPcmTail` (the partial trailing frame has no defined Layer II
  encoding — callers own any zero-pad-to-boundary policy); unequal channel
  lengths surface as `EncodeError::BadPcmLen`. Output feeds straight back into
  `decode_all_frames`.

- **Auto-SMR encode path: `encode_frame_auto` / `encode_frame_auto_with`**.
  The encoder now computes the §C.1.5.2.7 bit-allocator's
  signal-to-mask-ratio table **automatically** from each frame's PCM via the
  §D.1 Model-1 chain (`psy::compute_smr_model1_frame`), instead of requiring
  a caller-supplied table. For the MPEG-1 Layer II rates (32 / 44,1 / 48 kHz)
  the allocation is psychoacoustically driven; the §D.1 Step 2 `scf_max(n)`
  operand is taken from the encoder's independently-extracted scalefactors
  (largest Table 3-B.1 multiplier across the three granules) and the
  Step 3 offset from `bit_rate / channels`. For the MPEG-2 LSF rates — which
  the standard tabulates no Annex D Layer II masking curves for — the SMR
  degenerates to a flat 0 dB table (rate-driven allocation). A multi-frame
  streaming auto-SMR encode now round-trips through the decoder, with the
  reconstructed-tone residual energy a fraction of the signal energy
  (`tests::auto_smr_stream_round_trips_within_bound`); the auto allocation is
  verified to differ from the flat-SMR allocation for spectrally-uneven
  input (`tests::auto_smr_shapes_allocation_differently_from_flat_smr`). The
  four existing caller-supplied-SMR entry points are unchanged.

- **§D.1 Model-1 signal-to-mask-ratio driver `psy::compute_smr_model1_frame`**.
  Chains the previously-isolated §D.1 Step 1…9 primitives into the single
  per-subband `SMR_sb(n)` table the §C.1.5.2.7 bit allocator consumes:
  Hann-windowed 1024-point FFT power-density spectrum (Step 1) → 96 dB SPL
  normalisation → per-subband sound pressure level `L_sb(n)` (Step 2) →
  tonal / non-tonal masker extraction (Step 4) → threshold-in-quiet +
  bit-rate-offset decimation (Step 3 + Step 5a) → 0.5-Bark tonal decimation
  (Step 5b) → per-FFT-line global masking threshold `LTg(i)` (Step 6/7) →
  per-subband minimum masking threshold `LT_min(n)` (Step 8) →
  `SMR_sb(n) = L_sb(n) − LT_min(n)` (Step 9), with the §C.1.5.2.4 fallback
  for subbands carrying no masking line. `psy::annex_d_sampling_rate` maps a
  Layer II sampling frequency to the Annex D table selector (MPEG-1 rates
  only; the LSF rates have no Annex D Layer II masking tables).

- **End-to-end Layer II decode → PCM conformance against a real fixture**
  (`tests/layer2_pcm_conformance.rs`). The complete §2.4 decode chain
  (header → §2.4.1.4 CRC → §2.4.2.1 bit-allocation table → §2.4.3.3
  scalefactor/scfsi → §2.4.3.3.4 requantization → §2.4.3.2 / Annex A
  Figure A.2 polyphase synthesis filterbank) is now validated against
  the staged `layer2-stereo-44100-192kbps` fixture's `expected.wav`
  (31 frames → 71 424 interleaved s16 samples). Decoded sample count is
  exact; the per-sample error envelope is **max abs ≤ 1 LSB, rms < 0.6
  LSB, > 70 % bit-exact** — the ISO floating-point-filterbank
  conformance bound (§2.4.3.2 / §2.4.3.3.5 specify the filterbank in
  floating point with no fixed accumulation order or integer-rounding
  rule, so an independent clean-room decoder reproduces a reference
  decoder's integer output only within that envelope; ISO/IEC 11172-4
  defines conformance itself as a bounded difference signal). A second
  test asserts the §2.4.3.4.7.1 `[-1, +1]` output range and near-zero
  DC offset (a guard on the §2.4.3.3.4 `D` requant constant).

### Fixed

- **Free-format bit-allocation table selection (§2.4.2.3 / Annex B)** —
  found by decoding a free-format rewrite with an independent black-box
  reference decoder (0.5 % of samples agreed; every decode stage after
  the table diverged). The Annex B table for a free-format frame is
  fixed by the **sampling frequency alone**: the Table 3-B.2a header
  (PDF p. 46) lists "Fs = 48 kHz … and free format" and the Table
  3-B.2b header (PDF p. 47) lists free format under 44,1 and 32 kHz;
  B.2c/B.2d have no free-format row. The decoder previously recovered
  the bitrate from the measured frame size and keyed the table on it —
  correct only when the two selections happened to coincide.
  `select_table` now routes a free-format header (`bit_rate == 0`) by
  sampling frequency directly; both free-format decode paths (stream +
  registry packet) decode with the original free-format header. After
  the fix our free-format decode is **100 % s16 bit-exact** against the
  independent reference on a real 48 kHz stream
  (`free_format_rewrite_of_real_48k_fixture_decodes_identically` pins
  the equivalence in-tree).

  Consequences implemented with it:
  - **Off-ladder free-format rates decode** ("a fixed bitrate which
    does not need to be in the list"): the ladder-match requirement is
    gone; `bitrate_from_base_slots` returns the nominal `⌈N·Fs/144⌉` as
    metadata for an off-ladder size and only rejects sizes above the
    per-standard Layer II free-format decoder-support ceiling — 11172-3
    §2.4.2.3 **384 kbit/s** for MPEG-1 (new `FREE_FORMAT_MAX_BIT_RATE`),
    13818-3 §2.4.2.3 **160 kbit/s** for LSF (new
    `FREE_FORMAT_MAX_BIT_RATE_LSF`; "The decoder is not required to
    support bitrates higher than 256 kbit/s, 160 kbit/s, 160 kbit/s in
    respect to Layer I, II and III when in free format mode"). Pinned
    by an ancillary-extended off-ladder stream decoding bit-identically
    and by boundary tests at both standards' exact ladder tops.
  - **No (bitrate, mode) matrix for free format**: the §2.4.2.3 matrix
    row for free format reads "all modes"; `resolve` no longer rejects
    recovered-rate/mode pairs (`recovered_pair_is_valid` is now
    constant-true, retained for API continuity).
  - **Registry encoder `freeformat` guard**: `freeformat=true` with a
    bitrate whose signalled Annex B table differs from the free-format
    table (e.g. 96 kbit/s stereo at 48 kHz → B.2c laid out, B.2a read)
    is rejected at construction — such a stream is well-formed but
    decodes to garbage on every conforming decoder. Valid: per-channel
    ≥ 56 kbit/s at 48 kHz, ≥ 96 kbit/s at 44,1/32 kHz, any LSF rate.
  - `header_with_recovered_bitrate` is metadata-only now (feeding it to
    decode would re-key the table on the recovered rate); docs state
    this explicitly.

### Changed

- **Fractional → `i16` PCM map is now the symmetric `2^15` full scale**
  (`codec_decoder::float_plane_to_s16_le`). The §2.4.3.3.4 requantizer
  interprets each codeword as a two's-complement fraction "where the
  MSB represents −1", so the matching integer map is `−1.0 ↦ −32768`
  (multiply by `2^15 = 32768`, round, clamp to `i16::MAX` on full-scale
  `+1.0`). The previous `× i16::MAX` scale biased every nonzero sample
  toward zero by a fraction of an LSB; switching to `2^15` halves the
  worst-case error against the reference (max abs 2 → 1 LSB) and roughly
  doubles the bit-exact-sample ratio.

### Added (psychoacoustic model, prior)

- **§D.2.4 step (l) Model-2 absolute-threshold tables D.4a / D.4b / D.4c
  (32 / 44,1 / 48 kHz) + per-line expander** (`tables_model2`). The Annex D
  Table D.4a (132 entries), D.4b (130 entries) and D.4c (126 entries)
  per-FFT-line absolute-threshold (threshold-in-quiet) tables are
  transcribed verbatim from the staged CSVs
  `docs/audio/mp3/annex-d-table-D4{a,b,c}-absolute-threshold-*.csv`
  (`line_lower` / `line_higher` / `threshold_db` per range) into the new
  `AbsThrEntry` carrier. Each row's `(line_lower ..= line_higher)` is the
  inclusive 1-based range of 1024-point-analysis-FFT lines sharing one
  threshold value; the ranges tile the band one-line-per-row at the bottom
  and widen to 2-/4-/8-line groups toward the top (topmost line 480 / 464 /
  432). The as-printed **D.4** divergences from the matching Layer II D.1
  tables are reproduced faithfully: D.4a's `51.03` dB top (vs D.1d's 51.04),
  D.4b's surprising `69.13` dB saturation ceiling (vs D.1e's 68.00), and
  D.4c's `68.00` dB ceiling matching D.1f. New
  `abs_threshold_table_for_rate(SamplingRate)` dispatches the
  `&[AbsThrEntry]` table per rate, and
  `absolute_threshold_db_per_line(table, line_count)` expands a D.4 table
  into a per-FFT-line dB slice over the analysis-FFT working range
  (broadcasting each range's value across its lines, holding the
  top-of-band ceiling for lines above the last tabulated range), ready for
  the caller's dB→energy conversion and the step-(l)
  `include_absolute_threshold` floor. This closes the last un-transcribed
  Annex D table the README "lacks" tail called out — the §D.2 chain can now
  feed step (l) at all three Layer II sampling rates. 14 new lib tests
  (438 → 452): row counts (132/130/126), contiguous one-based range tiling,
  topmost-line spot checks (480/464/432), single-line low groups, verbatim
  head/tail cells, the D.4b 69.13 dB ceiling + its 369–376 onset, the D.4c
  68.00 dB ceiling, dispatcher pointer identity, per-line broadcast +
  ceiling-hold-above-last-range, caller-driven output length, empty/zero
  safe responses, and an end-to-end step-(l) floor composition.
- **§D.2 Model-2 calculation-partition tables D.3b / D.3c (44,1 / 48 kHz)
  + rate dispatcher** (`tables_model2`). The Annex D Table D.3b (57
  partitions, 44,1 kHz) and Table D.3c (58 partitions, 48 kHz)
  calculation-partition tables are transcribed verbatim from the staged
  CSVs `docs/audio/mp3/annex-d-table-D3{b,c}-calc-partition-*.csv`
  (`omega_low`/`omega_high`/`bval`/`minval`/`tmn` per partition), joining
  the existing 32 kHz Table D.3a. New `calc_partition_table_for_rate`
  dispatches the `&[CalcPartition]` table on the `SamplingRate` enum, so
  the step-(f) spreading convolution and the step-(g)…(n) threshold loop
  now run for **all three** Layer II sampling rates instead of 32 kHz
  only. 7 new unit tests: partition counts (49/57/58), contiguity +
  Nyquist coverage (lines tile 1..=513 exactly), non-decreasing `bval`,
  the single-FFT-line low-partition runs (16 for D.3b, 17 for D.3c),
  verbatim tail-cell spot checks, dispatcher pointer identity, and an
  all-rates spreading-convolution smoke. Only the D.4 per-line
  absolute-threshold tables remain un-transcribed.
- **§D.2.4 Model-2 threshold-calculation loop, steps (g)…(n)**
  (`tables_model2`). The Model-2 chain now continues past the step-(f)
  spreading convolution to the signal-to-mask ratio. New primitives,
  each transcribed verbatim from ISO/IEC 11172-3:1993 Annex D PDF pages
  131–132 (printed 125–126): `tonality_index` (step g, `tb_b = −0,299 −
  0,43·ln(cb_b)` clamped to `[0, 1]`, with the silent-partition
  `cb_b ≤ 0` → maximally-tonal safe response), `required_snr_db`
  (step h, `SNR_b = max(minval_b, tb_b·TMN_b + (1−tb_b)·NMT_b)`) backed
  by the new `NMT_DB` = 5,5 dB noise-masking-tone constant, `power_ratio`
  (step i, `bc_b = 10^(−SNR_b/10)`), `actual_energy_threshold` (step j,
  `nb_b = en_b·bc_b`), `line_energy_threshold` (the per-rate partition
  loop running g…k and spreading each partition's threshold energy
  uniformly over its FFT lines, `nb_ω = nb_b / line_count_b`),
  `include_absolute_threshold` (step l, `thr_ω = max(nb_ω, absthr_ω)`,
  taking a caller-supplied energy-domain `absthr_ω`), and
  `signal_to_mask_ratio_db` (step n, `SMR_n = 10·log10(epart_n /
  npart_n)` per Table D.5 coder partition, honouring the narrow-band
  `width = 1` threshold-sum vs wide-band `width = 0` smallest-positive-
  threshold-times-line-count rule). Step (m) pre-echo control is Layer
  III only and omitted for Layers I/II per the spec. 24 new unit tests
  (formula/clamp boundaries, minval-floor override, energy conservation
  across the per-line spread, narrow/wide SMR rules, zero-threshold and
  out-of-range safe responses, and an end-to-end g…n walk). The
  end-to-end Model-2 chain remains 32 kHz-only pending the D.3b/D.3c
  partition tables and the D.4 per-line absolute-threshold tables (both
  staged as CSVs under `docs/audio/mp3/`).
- **§D.1 Step 3 absolute-threshold offset + Step 5(a) threshold-in-quiet
  decimation** (`psy`, `tables_d2`). The Layer II Annex D Table D.1d /
  D.1e / D.1f threshold-in-quiet (`LTq`) curves (132 / 130 / 126 entries
  at 32 / 44.1 / 48 kHz) are transcribed into `tables_d2` as a typed
  `LtqEntry` table (top FFT line + threshold dB), keyed off the staged
  Annex D CSVs; the thresholds are read from the **D.1** Layer II column
  (not the Model-2 D.4 column, which diverges by the documented last-digit
  / 69.13 dB ceiling errata at 32 / 44.1 kHz) and the FFT-line ranges from
  the deterministic `higher = round(f / (Fs/1024))` mapping.
  `SamplingRate::ltq_table_layer2()` dispatches per rate. New `psy`
  primitives: `absolute_threshold_offset_db` (Step 3: −12 dB for ≥ 96
  kbit/s/ch, 0 dB below — verbatim spec), `ltq_db_at_line` (per-FFT-line
  `LTq(k)` lookup with the Step 3 offset folded in), `NonTonalCandidate`
  + `list_non_tonal_candidates_layer2` (the `k`-carrying Step 4(c)
  companion of `list_non_tonal_layer2`), `bark_for_line_layer2`
  (Step 6 input-transform Bark position from the D.2 boundary table), and
  `decimate_below_threshold_in_quiet` (Step 5(a): keep a tonal/non-tonal
  masker iff `X(k) ≥ LTq(k)`, dropping untabulated lines; composes before
  the existing Step 5(b) `decimate_tonal_maskers`). This closes the
  `#1262` "Step 5(a) requires the PNG-only D.1d/e/f curves" gate noted in
  `psy`'s module docs — those curves are now text-transcribed. 26 new
  unit tests (table integrity, errata-cell provenance, offset boundary,
  line-range walking, decimation classification, 5(a)→5(b) composition).

### Fixed

- **Round-318 §2.4.1.6 intensity-stereo sample loop — one shared sample
  codeword above `bound`** (`frame` decode loop + `encoder_frame` write
  loop). The §2.4.1.6 `audio_data()` sample syntax has two regions per
  granule: for `sb < bound` one triplet is read/written **per channel**
  (`samplecode[ch][sb][gr]`), but for `bound ≤ sb < sblimit` (the
  intensity-stereo region in `joint_stereo` mode) only **one** triplet is
  on the wire (`samplecode[0][sb][gr]`), valid for both channels
  (§2.4.2.6: "for subbands in intensity_stereo mode the coded
  representation of the sample is valid for both channels"). Each channel
  still rescales that shared codeword by its own §2.4.3.3.3 scalefactor.
  Both the decoder and the encoder previously read/wrote a separate
  triplet per channel across the whole `sblimit` range, so a real-world
  `joint_stereo` MP2 stream desync'd on decode and our encoder emitted a
  non-conformant frame that over-ran the §C.1.5.2.7 bit budget the
  allocator had already sized for a single shared codeword above `bound`.
  The two regions are now split to match the syntax; the non-joint modes
  (`bound == sblimit`) reduce to the prior flat per-channel loop and are
  unaffected. New tests:
  `frame::joint_stereo_above_bound_shares_codeword_across_channels` (both
  channels reconstruct from one codeword) and
  `encoder_frame::joint_stereo_above_bound_writes_one_shared_codeword_per_subband`
  (the frame stays within `frame_size_bytes`). Sourced from ISO/IEC
  11172-3:1993 §2.4.1.6 / §2.4.2.6.

### Added

- **Round-310 Annex D §D.2.4 step (f) — Model 2 partition-domain
  spreading convolution + normalization** (`tables_model2` module).
  The clause D.2.4 step (f) machinery that convolves the
  threshold-calculation partition energies / unpredictabilities with the
  already-landed clause D.2.3 `spreading_function` across the Bark axis,
  transcribed verbatim from ISO/IEC 11172-3:1993 Annex D (PDF pages
  130–131 / printed 124–125):
  * `convolve_partition_spreading(table, quantity) -> Vec<f64>` is the
    shared `ecb_b = Σ_bb e_bb · sprdngf(bval_bb, bval_b)` (and
    identically `cf_b = Σ_bb e_bb·c_bb · sprdngf(...)`) convolution over
    every calculation partition `bb` into target partition `b`, indexed
    by the 0-based row of the supplied `CalcPartition` table (e.g.
    `TABLE_D_3A_CALC_PARTITION_32KHZ`). A length mismatch returns an
    empty vector.
  * `rnorm_coefficient(table, b) -> Option<f64>` is the
    spreading-function normalization coefficient `rnorm_b = 1 /
    Σ_bb sprdngf(bval_bb, bval_b)` (the reciprocal of the spreading
    row-sum into partition `b`); always finite and positive because the
    self-spread is > 0. `None` for `b` out of range.
  * `normalize_spread_energy(table, ecb) -> Vec<f64>` applies the
    coefficient pointwise: `en_b = ecb_b · rnorm_b`.
  * `renormalize_unpredictability(cf, ecb) -> Vec<f64>` is the
    energy-weighting renormalization `cb_b = cf_b / ecb_b`; a silent
    partition (`ecb_b == 0`) yields `cb_b = 0.0` rather than NaN.
  The spec's `bb = 1..bmax` summation (the clause D.2.2 "partition
  numbering starts at 1" convention) maps to summing over every row of
  the 0-based `table`. 13 new lib tests (379 → 392): the convolution
  against an independent from-spec reference on D.3a, the unit-impulse
  identity (`ecb` reproduces the spreading row), linearity in the source
  quantity, length-mismatch safe return; `rnorm` as the exact row-sum
  reciprocal across all 49 partitions with finite/positive guarantees,
  out-of-range `None`, and the flat-spread-normalizes-to-unity identity;
  `normalize_spread_energy` pointwise application and length-mismatch
  safe return; `renormalize_unpredictability` as exact `cf/ecb`, the
  silent-partition zero, and length-mismatch safe return; plus an
  end-to-end step (f) pipeline check that constant unpredictability is
  preserved through the energy-weighted convolution + renormalization
  for any energy profile. The companion D.3b (44,1 kHz) / D.3c (48 kHz)
  calculation-partition tables and the D.4a–c absolute-threshold tables
  remain PNG-only renders; the step (f) primitives are table-agnostic
  and work against whichever `CalcPartition` table the caller supplies.

- **Round-303 Annex D Table D.5 — Layer I / Layer II coder partition
  table** (`tables_model2` module). The 33-row (`n = 0..=32`)
  Model 1 + Model 2 coder-partition boundary table, transcribed
  verbatim from ISO/IEC 11172-3:1993 Annex D Table D.5 (PDF page 145 /
  printed 139), lands as `TABLE_D_5_CODER_PARTITION: [CoderPartition;
  33]` with the `boundary` (`ωhigh_n` / `ωlow_{n+1}`) and `width_n`
  columns. The table is common to all three sampling rates and both
  Layers. Two accessors map between coder partitions and FFT lines:
  `coder_partition_span(n) -> Option<(u32, u32)>` returns the 1-based
  inclusive FFT-line span of partition `n` (partition 0 is the single
  DC line; partition `n ≥ 1` covers `boundary(n-1)+1 ..= boundary(n)`),
  and `coder_partition_of_line(omega) -> Option<usize>` is its inverse
  over the `1..=513` analysis-FFT working range. `CODER_PARTITION_COUNT`
  exposes the row count. 10 new lib tests (369 → 379): row count, the
  literal page-145 boundary endpoints / interior rows, the uniform
  16-line boundary step above partition 0, strict-increase-to-Nyquist,
  the `width_n` 0→1 flip at partition 13, contiguous span tiling of
  `1..=513`, span↔line round-trip over every partition, boundary
  anchors, and out-of-range `None` guards.

### Fixed

- `frame::decode_all_frames` no longer panics when a Layer II stream
  switches channel count mid-stream (e.g. a §2.4.1.3 single-channel
  frame followed by a stereo frame — each frame header carries its own
  `mode` field, so this is legal). The per-channel PCM accumulator was
  sized once from the first frame; a later, wider frame indexed past
  its end (`index out of bounds: the len is 1 but the index is 1`).
  It now grows to the running maximum channel count. Found by the new
  round-296 `decode` libFuzzer target; covered by the
  `decode_all_frames_handles_channel_count_change_mid_stream`
  regression test.

### Added

- **Round-296 depth-mode `decode` cargo-fuzz target**
  (`fuzz/fuzz_targets/decode.rs`). A coverage-guided panic-freedom
  fuzzer over the Layer II decode attacker surface: the `decode_frame`
  free function, the streaming `decode_all_frames` chain (sync-resync
  skip + multi-frame chaining + cross-frame `FrameDecodeState`
  filterbank carry-over), and the registered `Decoder` trait object
  (`send_packet` / `receive_frame` / `flush` / `reset`). Constructs
  structurally-valid MPEG-1 and MPEG-2 LSF headers from attacker bytes
  so the deep degroup / requantise / synthesis chain is reached on
  essentially every iteration, with raw-byte frames interleaved for the
  `BadSync` / short-frame rejections. The self-contained `fuzz/`
  sub-crate has its own `[workspace]` so the umbrella `crates/*` glob
  cannot pull it in. Ran clean for 2 000 000 executions after the
  channel-count fix above.

- Annex D **Model 2** (clause §D.2) opening stage (`tables_model2`
  module): the *calculation partition table* and the Model-2
  *spreading function*.
  * `TABLE_D_3A_CALC_PARTITION_32KHZ: [CalcPartition; 49]` is Table
    D.3a (Fs = 32 kHz, 49 threshold-calculation partitions),
    transcribed verbatim from the staged ISO/IEC 11172-3:1993 PDF
    page 139 (printed 133). Each `CalcPartition` carries the spec
    columns `ωlow` / `ωhigh` (first / last 1-based FFT line of the
    partition), `bval` (median Bark value), `minval` (minimum
    masking-spread value, dB), and `tmn` (tone-masking-noise offset,
    dB). The partitions tile the 1024-point analysis FFT's lines
    1…513 with no gaps (`ωlow[n+1] = ωhigh[n]+1`, last `ωhigh = 513`
    Nyquist). `CalcPartition::line_count()` returns `ωhigh − ωlow +
    1`.
  * `spreading_function(bval_from, bval_into) -> f64` is the clause
    D.2.3 Model-2 spreading function `sprdngf(i, j)` (PDF page 129,
    printed 123): `tmpx = 1,05·(j−i)`, `x = 8·min((tmpx−0,5)² −
    2·(tmpx−0,5), 0)`, `tmpy = 15,811389 + 7,5·(tmpx+0,474) −
    17,5·√(1 + (tmpx+0,474)²)`, returning `10^((x+tmpy)/10)` and
    clamping to 0 when `tmpy < −100`. `i` is the Bark value of the
    source partition, `j` the Bark value of the target partition.
  * 11 new lib tests pin the 49-partition count, the contiguous
    tile-to-Nyquist invariant (summed line count = 513),
    non-decreasing `bval`, the constant `minval = 4,5 dB` from
    partition 17, the monotone TMN tail, and the spreading
    function's self-peak / asymmetry / far-below decay-to-zero.
  * Companion tables D.3b (44,1 kHz, 57 partitions) and D.3c
    (48 kHz, 58 partitions) are PNG-only renders not yet transcribed.
- Annex D Model 1 §D.1 Step 1 power-density spectrum + 96 dB SPL
  normalisation (`psy` module). With these, every prose-only Step 1
  item is in place and Model 1's Steps 1 → 2 → 8 → 9 compose
  end-to-end from raw PCM:
  * `power_density_spectrum_layer2(s: &[f64; 1024]) -> Vec<f64>` is
    the verbatim spec equation `X(k) = 10·log10 |(1/N)·Σ h(l)·s(l)·
    e^(-j·k·l·2π/N)|² dB` for `k = 0…N/2` (PDF p.116, printed 110),
    with `h(l)` the already-landed `hann_window_layer2` window and
    `N = LAYER2_FFT_LEN = 1024` (the spec's Layer II transform
    length). The transform is an in-crate textbook radix-2
    decimation-in-time FFT (private `fft_radix2_in_place`),
    cross-checked against a literal O(N²) evaluation of the spec
    sum. Output has `LAYER2_FFT_BINS = 513` entries (DC through
    Nyquist inclusive); zero-energy bins yield `-inf` dB,
    consistent with the Step 4(b) zero-out representation.
  * `normalize_to_spl_reference(spl_db: &mut [f64]) -> f64`
    implements the verbatim "A normalization to the reference
    level of 96 dB SPL (Sound Pressure Level) has to be done in
    such a way that the maximum value corresponds to 96 dB"
    sentence: adds `96 - max(X)` to every entry (returning the
    offset), anchoring the finite maximum at the new
    `SPL_REFERENCE_LEVEL_DB = 96.0` constant while preserving all
    pairwise dB differences. `-inf` bins stay `-inf`; `NaN`
    entries are skipped for the max determination; an all-`-inf`
    (silent) spectrum is the documented no-op safe response.
  * The §D.1 Step 1 window-shift prose lands as two documented
    constants: `FFT_DELAY_COMPENSATION_SHIFT_SAMPLES = 256` (item
    (a): "A window shift of 256 samples is required to compensate
    for the delay in the analysis subband filter") and
    `LAYER2_FFT_ADDITIONAL_WINDOW_SHIFT_SAMPLES = -64` (item (b):
    "For Layer II an additional window shift of minus 64 samples
    is required") — net Layer II shift 192 samples.

  8 new lib tests (349 → 357): the unit-DC anchor
  `X(0) = 20·log10(sqrt(8/3)·0.5)` with the window's own ±1-bin
  leakage at `20·log10(C/2)` and noise-floor bins below −200 dB;
  the bin-centred-sinusoid anchor (`X(m) = 20·log10(C/2)`,
  `X(m±1) = 20·log10(C/4)`, local-maximum property, exact
  `+20·log10(2)` gain under amplitude doubling); the radix-2 ↔
  naive-DFT cross-check on a deterministic xorshift64* broadband
  signal (all 513 bins within 1e-6 dB); the zero-signal all-`-inf`
  degenerate; max-anchored-at-96 normalisation with offset /
  difference preservation and `-inf` passthrough; NaN-skip +
  all-`-inf` + empty-slice safe responses; the window-shift
  constants (256, −64, net 192); and the Step 1 → Step 2
  composition (a k = 100 sinusoid normalises to exactly 96 dB and
  produces `L_sb(6) = 96 dB` through
  `sound_pressure_level_subband`).

- Annex D Model 1 §D.1 Step 2 sound-pressure-level determination
  (`psy` module). Three new spec-text-only primitives produce the
  per-subband `L_sb(n)` array the §D.1 Step 9
  `signal_to_mask_ratio_subband` (round 253) consumes:
  * `scalefactor_spl_term_db(scf_max) -> f64` is the verbatim
    scalefactor operand `20·log10(scf_max(n)·32768) - 10` dB
    (PDF p.116, printed 110). `scf_max` is, for Layer II, "the
    maximum of the three scalefactors of subband n within a
    frame" — the Annex B Table 3-B.1 multiplier value, not the
    6-bit index. The spec's "-10 dB" term "corrects for the
    difference between peak and RMS level"; the `32768 = 2^15`
    factor maps the `[-1, +1)` scalefactor domain onto the 16-bit
    full-scale axis the Step 1 96-dB normalisation establishes.
    Exposed as the new constants `SPL_FULL_SCALE = 32768.0` and
    `SPL_PEAK_RMS_CORRECTION_DB = 10.0`.
  * `sound_pressure_level_subband(spl_db, line_subband, scf_max,
    method) -> [f64; 32]` runs the verbatim Step 2 reduction
    `L_sb(n) = MAX[ X(k), 20·log10(scf_max(n)·32768) - 10 ]` over
    `X(k) in subband n` for every subband. The
    `SubbandSplMethod::MaxLine` variant is the primary "spectral
    line with the maximum amplitude in the frequency range
    corresponding to subband n" estimator;
    `SubbandSplMethod::PowerSum` is the spec's documented
    alternative `X_spl(n) = 10·log10( Σ_k 10^(X(k)/10) )` dB
    (PDF p.117, printed 111). Every output slot is defined — the
    scalefactor operand always exists, so a subband receiving no
    FFT line degenerates to the scf term alone. `NaN` lines are
    dropped; the `usize::MAX` "outside the audio band" sentinel
    and out-of-range subband indices are skipped (consistent with
    the Step 8 `minimum_masking_threshold_subband` conventions);
    a `spl_db` / `line_subband` length mismatch returns the scf
    terms alone as the documented safe response.
  * `fft_line_to_subband_layer2(k) -> usize` is the closed-form
    Layer II FFT-line → subband map: Step 1's "Technical data of
    the FFT" table gives a `fs/1024` frequency resolution and the
    §2.4.3.2 filterbank splits `[0, fs/2)` into 32 equal `fs/64`
    subbands, so subband `n` spans FFT lines `16n ..= 16n + 15`
    (`k/16`); lines at or above the Nyquist index (`k >= 512`)
    map to the `usize::MAX` sentinel.
  * Model 1's Steps 2 → 8 → 9 now compose end-to-end on a shared
    `line_subband` axis. Remaining §D.1 gaps: the Step 1
    power-density spectrum (`X(k)` FFT + 96 dB normalisation) is
    the next prose-only step; Steps 3 and 5(a) stay DOCS-BLOCKED
    on the PNG-only Table D.1d/e/f inner rows (#1262).
  * 12 new lib tests (337 → 349) pin the unity-scalefactor anchor
    (`80.30899869919435 dB = 300·log10(2) − 10`), the doubling
    `+20·log10(2)` identity and Table 3-B.1 monotonicity of the
    term, loudest-line selection, scf-term dominance over a quiet
    spectrum (both methods), the three-equal-lines power-sum
    identity (`90 + 10·log10(3)`), the `PowerSum >= MaxLine`
    dominance property across a 512-line spectrum, the no-lines
    scf-term degenerate (both methods), sentinel / out-of-range
    skipping, NaN drop, the length-mismatch safe return, the
    16-lines-per-subband boundary sweep, and the Step 2 → 8 → 9
    SMR composition (`SMR_sb(1) = 95 − 60 = 35 dB`).
- Annex D Model 1 §D.1 Step 7 masker-range pre-filter (`psy`
  module). Two new spec-text-only primitives implementing the
  verbatim "For a given `i` the range of `j` may be reduced to
  maskers within `-8…+3` Bark of `i`" optimisation sentence
  (PDF p.120, printed 114):
  * `masker_in_target_window(z_j_bark, z_i_bark) -> bool` is the
    pointwise predicate: returns `true` iff
    `dz = z(i) - z(j) ∈ [-3, 8)`, the identical window
    `masking_function_vf` uses. The half-open / half-closed
    asymmetry is reproduced from the spec — a masker at
    `z(j) = z(i) - 8` is excluded (dz = 8) and one at
    `z(j) = z(i) + 3` is included (dz = -3). `NaN` on either
    argument returns `false`.
  * `relevant_maskers_for_target_line(maskers, z_i_bark) ->
    Vec<Masker>` filters a masker list to just the entries the
    predicate accepts, preserving input order. Pre-filtering is
    purely a performance optimisation —
    `global_masking_threshold_db` already drops out-of-window
    entries via `masking_function_vf` returning `None`. The
    equivalence is pinned by a unit test that sweeps the target
    Bark axis and checks `LTg(filtered) == LTg(unfiltered)`
    bit-for-bit on a 5-masker / 7-target mix.
  * 7 new unit tests pin: window endpoint inclusion/exclusion at
    `dz = -3` and `dz = 8`, predicate-`individual_masking_threshold_db`
    agreement across a 12-sample Bark sweep, `NaN` handling on
    both arguments, in-window survivor extraction with input-order
    preservation, all-out-of-window collapse to empty, `LTg`
    invariance under pre-filtering, and idempotence under double
    filtering at the same target.
- Annex D Model 1 §D.1 Step 4(b) tonal-component listing sweep
  (`psy` module). One new spec-text-only function that drives the
  per-FFT-line Step 4(b) loop already factored across the existing
  `is_tonal_layer2` / `tonal_spl_db` /
  `zero_tonal_neighbourhood_layer2` primitives:
  * `list_tonal_layer2(spl_db: &mut [f64]) -> Vec<TonalCandidate>`
    visits `k` in ascending order from 3 up to `min(500,
    spl_db.len() - 2)`, runs the §D.1 Step 4(b) tonality test at
    each `k`, and on a positive classification appends a
    `TonalCandidate { k, spl_db }` carrier and applies the spec's
    "all spectral lines within the examined frequency range are
    set to −∞ dB" zero-out in place (per
    `tonal_neighbourhood_layer2`'s per-`k` width row). The
    in-place zero-out naturally suppresses subsequent `k`s that
    fall inside an already-claimed neighbourhood — they cannot
    satisfy either the local-maximum rule or the 7 dB inequality
    against the now-`−∞` bins.
  * `TonalCandidate { k, spl_db }` is a new carrier — it
    intentionally omits the masker's Bark position because the
    FFT-line → Bark mapping lives in the PNG-only Annex D
    Table D.1d / D.1e / D.1f Layer II columns gated by note
    `#1262`. When that material lands the caller may promote each
    candidate to a full `Masker { kind: Tonal, z_bark, spl_db }`
    via the table lookup.
  * The sweep + zero-out output composes cleanly with the existing
    `list_non_tonal_layer2` Step 4(c) pass — bands fully zeroed by
    the tonal sweep drop out of the non-tonal listing via
    `non_tonal_spl_db == None`.
  * 9 new unit tests pin: single-isolated-peak detection +
    `X_tm` math, neighbourhood zero-out coverage, subthreshold
    rejection (5 dB below the 7 dB inequality), within-prior-
    neighbourhood suppression (two peaks 2 bins apart →
    one survivor), well-separated multi-peak emission, ascending-`k`
    output ordering, edge-of-domain candidates ignored (k=1, k>500),
    end-to-end composition with `list_non_tonal_layer2`, and the
    short-spectrum (`len < 4`) no-op guard.
- Annex D Model 1 §D.1 Step 5(b) tonal-masker decimation and
  §D.1 Steps 8 + 9 minimum-masking-threshold-per-subband /
  signal-to-mask-ratio primitives (`psy` module). Three new
  spec-text-only pure functions wire together the
  "collapse near-Bark tonal clusters then reduce per-subband"
  half of Model 1 between the already-landed Step 4(c) (round
  250) and the §C.1.5.2.7 iterative bit-allocator (round 214):
  * `decimate_tonal_maskers(maskers) -> Vec<Masker>` runs the
    verbatim §D.1 Step 5(b) procedure ("Decimation of two or
    more tonal components within a distance of less than
    0.5 Bark: Keep the component with the highest power, and
    remove the smaller component(s) from the list of tonal
    components. For this operation, a sliding window in the
    critical band domain is used with a width of 0.5 Bark.",
    PDF page 113). The implementation splits the input by
    `MaskerKind`, sorts the tonal list by `z_bark`, walks
    sorted runs anchored on each run's first entry, and emits
    the highest-`spl_db` member of each run. The half-open
    `< 0.5 Bark` window is reproduced exactly (a pair at
    exactly 0.5 Bark is NOT merged); ties on `spl_db` keep
    the lowest-`z_bark` entry deterministically; the
    chained-run case `(5.0, 5.4, 5.8)` produces the documented
    two-survivor result (the spec's sliding-window reading
    requires every pair in the window to be < 0.5 Bark of
    every other). Non-tonal maskers pass through untouched per
    the spec scope ("two or more **tonal** components"); the
    output emits non-tonal in input order then surviving tonal
    in ascending Bark order. The procedure is documented
    idempotent.
  * `minimum_masking_threshold_subband(ltg_db, line_subband)
    -> [Option<f64>; 32]` runs the verbatim §D.1 Step 8
    reduction `LT_min(n) = MIN[ LT_g(i) ]` over
    `f(i) in subband n` (PDF page 114). The caller hands in
    the FFT-line → subband index map (the spec's `f(i)`
    frequency vector lives in the PNG-only Table D.1 inner
    rows; the caller derives the equivalent map from whatever
    source they have). The output slot is `None` for subbands
    that received no FFT line; `usize::MAX` acts as a
    documented "outside audio band" sentinel and is filtered
    out; `NaN` values are dropped from the minimum to keep the
    remaining finite values well-defined; a length mismatch
    between `ltg_db` and `line_subband` returns an all-`None`
    result as the documented safe response to a caller error.
  * `signal_to_mask_ratio_subband(l_sb_db, lt_min_db) ->
    [Option<f64>; 32]` is the verbatim §D.1 Step 9 elementwise
    subtraction `SMR_sb(n) = L_sb(n) - LT_min(n)` (PDF page
    115). Slots whose `lt_min_db` is `None` return `None` so
    the caller's §C.1.5.2.4 fallback can substitute.
  * New public constants `TONAL_DECIMATION_WINDOW_BARK = 0.5`
    (the §D.1 Step 5(b) sliding-window width) and
    `NUM_SUBBANDS_LAYER2 = 32` (the Layer II subband count).
  * Step 5(a) (threshold-in-quiet drop `X_tm(k) >= LT_q(k)`)
    still depends on the PNG-only Annex D Table D.1d/e/f LTq
    curves (#1262) and is not landed this round.
  * 23 new lib tests (298 → 321) covering the 0.5-Bark window
    constant, half-open endpoint, loudest-wins reduction,
    equal-power tie-break, non-tonal passthrough, mixed-class
    output order, the chained-cluster non-merge case
    `(5.0, 5.4, 5.8)`, empty / singleton inputs, idempotence,
    sort-independence; Step 8 OOB-index filtering, NaN drop,
    length-mismatch safe return, and trivial bijection
    properties; Step 9 negative-SMR pass-through and the
    end-to-end Step 7 → 8 → 9 composition.

- Annex D Model 1 §D.1 Step 4(b) tonal-neighbourhood zero-out and
  §D.1 Step 4(c) non-tonal-component listing (`psy` module + new
  `tables_d2` module). Four new spec-text-only pure functions wire
  together the "set the tonal neighbourhood to −∞ dB then power-sum
  the remaining lines per critical band" half of Step 4 the spec
  prose calls out:
  * `zero_tonal_neighbourhood_layer2(spl_db, k)` walks the same
    per-`k` `j`-neighbourhood used by Step 4(b) tonality testing and
    sets every line within it (plus the centre `k` itself) to
    `f64::NEG_INFINITY`, reproducing the verbatim spec sentence
    "all spectral lines within the examined frequency range are set
    to −∞ dB" (PDF page 112).
  * `non_tonal_spl_db(spl_db, lo, hi) -> Option<f64>` is the
    per-critical-band power sum `X_nm = 10 * log10(Sum 10^(X(k)/10))`
    over `k in [lo, hi]`, ignoring `-inf` lines exactly (since
    `10^(-inf/10) = 0`); returns `None` for empty or fully-zeroed
    bands.
  * `non_tonal_band_index(lo, hi) -> Option<usize>` picks the FFT
    line "nearest to the geometric mean of the critical band" per
    the spec phrasing (PDF page 113). The geometric mean is computed
    on the integer band `[lo, hi]` (with `lo = 0` substituted as
    `1` to avoid `sqrt(0)` collapse) and the nearest integer in
    `[lo, hi]` is returned; ties round down.
  * `list_non_tonal_layer2(spl_db, fs) -> Vec<Masker>` is the
    per-sampling-rate Step 4(c) sweep — it iterates the Annex D
    Table D.2d / D.2e / D.2f boundaries in order, calls
    `non_tonal_spl_db` on each `(prev_top + 1, top]` band, and
    produces one `Masker { kind: NonTonal, z_bark, spl_db }` per
    non-empty band. The masker's `z_bark` is set to the boundary's
    top-line Bark column.
  * New module `tables_d2` carries the Layer-II critical-band
    boundary tables verbatim: `TABLE_D_2D_LAYER_II_32KHZ` (25
    entries), `TABLE_D_2E_LAYER_II_44K1HZ` (27 entries),
    `TABLE_D_2F_LAYER_II_48KHZ` (27 entries). One illegible-digit
    cell in the staged PDF (D.2e band 17, Bark `16,11[illegible]`)
    is reproduced as the best-fit `16.116` and the gap is
    documented in the constant's doc comment. A new
    `SamplingRate { Fs32kHz, Fs44k1Hz, Fs48kHz }` enumeration
    (re-exported as `PsyAnnexDSamplingRate`) lets the
    `list_non_tonal_layer2` caller pick the right boundary table.
  Total lib tests 271 → 298 (+27).

- Annex D Model 1 §D.1 Step 1 Hann window and §D.1 Step 4 tonality
  classifier primitives (`psy` module). Five new spec-text-only
  pure functions land the FFT-windowing and tonal/non-tonal labelling
  halves of Model 1:
  * `hann_window_layer2() -> [f64; 1024]` reproduces the verbatim
    spec equation `h(i) = sqrt(8/3) * 0.5 * (1 - cos(2 * pi * i /
    N))` for the Layer II 1024-sample FFT.
  * `is_local_maximum(spl_db, k) -> bool` runs the verbatim
    Step 4(a) rule `X(k) > X(k-1) AND X(k) >= X(k+1)` (strict
    on the lower side, non-strict on the upper side).
  * `tonal_neighbourhood_layer2(k) -> Option<&'static [i32]>`
    returns the per-`k` Layer II `j` neighbourhood: `{-2, +2}`
    for `2 < k < 63`, `{-3, -2, +2, +3}` for `63 <= k < 127`,
    `{-6, ..., -2, +2, ..., +6}` for `127 <= k < 255`, `{-12,
    ..., -2, +2, ..., +12}` for `255 <= k <= 500`.
  * `is_tonal_layer2(spl_db, k) -> bool` runs the verbatim
    Step 4(b) inequality `X(k) - X(k+j) >= 7 dB` for every `j`
    in the neighbourhood, with the Step 4(a) local-maximum
    precondition checked first.
  * `tonal_spl_db(spl_db, k) -> Option<f64>` is the three-line
    power sum `X_tm = 10 * log10(10^(X(k-1)/10) + 10^(X(k)/10)
    + 10^(X(k+1)/10))` the spec applies to a confirmed tonal
    line.
  Two new public constants: `LAYER2_FFT_LEN = 1024` (Layer II
  FFT length per PDF page 116) and `LAYER2_FFT_BINS = 513`
  (working range `k = 0..N/2` inclusive), plus
  `TONALITY_THRESHOLD_DB = 7.0` (the spec inequality constant).
  Total lib tests 254 → 271 (+17).

- Annex D Model 1 §D.1 Step 6 masking-function `vf` and §D.1 Step 7
  global-masking-threshold `LTg` primitives (`psy` module). Annex D
  is informative; the encoder's §C.1.5.2.7 iterative bit-allocator
  needs a per-(channel, sub-band) signal-to-mask-ratio (SMR) table
  and Models 1 / 2 are the spec's worked examples for producing one.
  Five new pure functions land the masker → masking-threshold half
  of Model 1 (Steps 6 and 7):
  * `masking_index_tonal(z_j_bark) -> f64` reproduces the verbatim
    spec equation `av_tm = -1.525 - 0.275 * z(j) - 4.5` dB.
  * `masking_index_non_tonal(z_j_bark) -> f64` reproduces
    `av_nm = -1.525 - 0.175 * z(j) - 0.5` dB.
  * `masking_function_vf(dz_bark, x_db) -> Option<f64>` is the
    four-branch piecewise `vf` defined on the half-open Bark window
    `[-3, 8)` — outside the window the function returns `None`
    (the spec's "masker ignored, `LT = -inf dB`" semantics). The
    four branches are: `17·(dz+1) − (0.4·X + 6)` on `[-3, -1)`,
    `(0.4·X + 6)·dz` on `[-1, 0)`, `-17·dz` on `[0, 1)`,
    `-(dz−1)·(17 − 0.15·X) − 17` on `[1, 8)`. Continuous at
    `dz = 0` (both adjacent branches yield 0 dB).
  * `individual_masking_threshold_db(masker, z_i_bark) ->
    Option<f64>` composes the per-masker individual masking
    threshold `LT = SPL + av + vf`, returning `None` when the
    Bark distance falls outside the `vf` window.
  * `global_masking_threshold_db(maskers, z_i_bark, ltq_db) -> f64`
    is the Step-7 energy sum `LTg(i) = 10·log10( 10^(LTq/10) +
    Σ 10^(LT_j/10) )` over every in-range masker, with the
    threshold-in-quiet `LTq` carried in dB.
  Two new public types — `MaskerKind { Tonal, NonTonal }` and
  `Masker { kind, z_bark, spl_db }` — and two public constants
  (`MASKING_FUNCTION_DZ_LO = -3.0`, `MASKING_FUNCTION_DZ_HI = 8.0`)
  expose the masker carrier and the window endpoints used by the
  `vf` window guard. The primitives operate on caller-supplied
  Bark coordinates — Steps 1..5 of Model 1 (1024-sample FFT, SPL
  conversion, tonality classifier, decimation / reorganisation,
  masker selection) remain unimplemented because they depend on
  the PNG-only inner rows of Annex D Tables D.1d–f (Layer II
  threshold-in-quiet) and Tables D.2d–f / D.3 / D.4 (Bark / Hz /
  FFT-line mapping) — see DOCS-GAP `#1262`. Eighteen new lib
  tests (236 → 254, all green) validate every piecewise branch
  with hand-computed numeric anchors, the `[-3, 8)` window
  boundaries, continuity at `dz = 0`, the `LT = SPL + av` identity
  at `z(i) = z(j)`, the tonal-below-non-tonal ordering at matched
  parameters, and the four Step-7 invariants: no maskers ⇒
  `LTg = LTq`, distant masker ⇒ `LTg = LTq`, strong local masker
  dominates `LTq`, two equal-power co-located maskers add exactly
  `10·log10(2) ≈ +3.0103` dB.

- §2.4.1.8 `ancillary_data()` emission on the Layer II encoder
  (`encoder_frame` module). Two new public entry points,
  `encode_frame_with_ancillary(header, pcm, smr_db, banc, ancillary)`
  and `encode_frame_with_state_and_ancillary(header, pcm, smr_db,
  banc, ancillary, &mut state)`, copy a caller-supplied
  `ancillary_data()` byte payload into the §2.4.2.1 frame tail that
  begins immediately after the §2.4.1.6 audio-data + §2.4.3.3.4
  sample-codeword region. The payload starts on the first byte
  boundary past the sample region (the encoder calls
  `BitWriter::align_to_byte` before the copy so the §2.4.1.8 tail is
  always byte-granular regardless of how the §2.4.3.3.4 codewords
  ended). Any frame bytes the payload does not fill are zero-padded
  so the byte count still matches `header.frame_size_bytes()`. The
  caller's `banc` reservation continues to steer the §C.1.5.2.7
  iterative allocator — a typical call picks `banc >= ancillary.len()
  * 8` so the allocator leaves at least the payload-sized tail
  unfilled. A new `EncodeError::AncillaryTooLarge { space, got }`
  variant surfaces over-long payloads with both the actual tail
  capacity (`space`) and the rejected length (`got`); the legacy
  `encode_frame` / `encode_frame_with` entry points are now thin
  shims over the same shared `encode_frame_inner` implementation,
  passing `ancillary = &[]`. The §2.4.3.1 CRC patch runs after the
  ancillary copy and continues to verify clean — Annex B Table B.5
  protects the second half of the header + §2.4.1.6 audio-data
  (allocation + scfsi) but excludes the §2.4.1.8 tail, so the
  stored CRC word at frame bytes 4..6 is byte-identical to the
  no-ancillary reference frame for any payload. Six new lib tests
  (230 → 236): empty-ancillary call matches `encode_frame` byte-
  for-byte; a 32-byte distinctive payload lands at the §2.4.1.8 tail
  start (located via an all-`0xCC` marker-frame probe) and the
  trailing pad is zero; the §2.4.3.1 CRC word matches between the
  empty-ancillary and non-empty-ancillary frames (Table B.5
  exclusion); a frame-size-long payload surfaces `AncillaryTooLarge`
  with `got == huge.len()` and `space < got`; the stateful entry
  point preserves the §C.1.3 X ring-buffer evolution (same input +
  same payload + fresh state yields byte-identical first frames; a
  second frame from the persistent state differs); and a payload
  sized exactly `space` fits while `space + 1` is rejected with the
  same reported `space` value.
- §2.4 / Annex C frame-level encode loop (`encoder_frame` module).
  `encode_frame(header, pcm, smr_db, banc) -> Result<Vec<u8>,
  EncodeError>` pulls the previously-landed encoder primitives
  together into a single `pcm-in → byte-stream-out` call: analysis
  filterbank → per-(channel, sub-band, granule) scalefactor
  extraction → §C.1.5.2.7 bit allocation against the supplied
  signal-to-mask-ratio table → §C.1.5.2.5 / Table C.4 SCFSI selection
  (which rewrites `audio.scalefactor[ch][sb]` to the Table C.4
  `used` triple and populates `audio.scfsi[ch][sb]`) → §2.4.1.3
  header bytes → §2.4.1.4 CRC slot (reserved, patched after) →
  §2.4.1.6 audio-data section → §2.4.3.3.4 sample codewords (the
  same `(sample_gr, sb, ch)` ordering the decoder reads) → §2.4.1.10
  `banc` ancillary reservation → zero-padding to
  `header.frame_size_bytes()` → §2.4.3.1 CRC patch (re-extracting
  the protected region from the just-emitted bytes and writing the
  16-bit CRC into the reserved slot when `protection_bit == 0`).
  `encode_frame_with(header, pcm, smr_db, banc, &mut state)` takes a
  caller-supplied `EncodeFrameState` so the §C.1.3 X ring buffer
  persists across successive frames (the encoder dual of
  `frame::FrameDecodeState`); `EncodeFrameState::reset` re-zeros
  every channel's X buffer on a seek / discontinuity. The emitted
  frame is exactly `header.frame_size_bytes()` bytes long; the
  bytes round-trip through `frame::decode_frame` to recover the
  same header, the same `nb_steps` table, and a per-channel PCM
  vector of `frame::PCM_SAMPLES_PER_CHANNEL` samples. The frame's
  §2.4.3.1 CRC verifies on the decode side; flipping any
  CRC-payload bit afterwards makes `decode_frame` reject the
  frame with `FrameError::CrcMismatch`. `EncodeError` wraps the
  sub-stage errors (`HeaderError`, `BitAllocError`,
  `AudioDataWriteError`, `SampleWriteError`) plus PCM-shape
  validation (`BadPcmChannelCount`, `BadPcmLen`). 15 new lib tests
  (215 → 230): zero-input emits a well-formed frame whose header
  round-trips; a 1 kHz / 0.5-amplitude stereo tone round-trips
  through `decode_frame`; a single-channel 64 kbit/s 440 Hz tone
  round-trips; the §2.4.1.6 audio-data section parses back via
  `parse_audio_data_with_section_bits` with non-zero `alloc_bits`,
  non-zero `scfsi_bits`, and at least one non-zero `nb_steps`
  entry; the §2.4.3.1 CRC patch passes on the emitted frame; a
  high-bit flip of the first byte of the bit-allocation section
  triggers `FrameError::CrcMismatch`; mismatched
  `pcm.len()` / `pcm[ch].len()` raise `BadPcmChannelCount` /
  `BadPcmLen` with the expected `have`/`need` fields; a `banc=256`
  reservation produces a still-decodable frame; a `banc` larger
  than the data-bit budget surfaces
  `BitAllocError::InsufficientFrameSize`; encoding the same input
  twice with a persistent `EncodeFrameState` produces a second
  frame that differs from the first (proving X accumulates) but
  matches when run against a fresh state for the first frame;
  `EncodeFrameState::reset` restores the first-frame identity;
  the no-CRC path (`protection_bit == 1`) round-trips through
  `decode_frame`; the joint-stereo above-`bound` allocation
  balance invariant (`nb_steps[0][sb] == nb_steps[1][sb]`) holds;
  and boosting per-(channel, sub-band) SMR for the low / high
  sub-band groups never reduces the allocator's spend on the
  boosted band.
- §2.4.3.3.4 encoder sub-band sample quantizer (`encoder_samples`
  module). `quantize_sample(class, s'') -> u32` inverts the
  normative decode mapping: divide out the Table 3-B.4 linear
  formula (`s''' = s''/C − D`), clamp the integer `k = round(s''' ·
  2^(n−1))` into the legal range for the active class (ungrouped:
  `[−2^(n−1), 2^(n−1) − 1]`; grouped: `[−2^(n−1), nb_steps − 1 −
  2^(n−1)]` because §2.4.3.3.4 degrouping yields three digits in
  `[0, nb_steps)`), encode as `n`-bit two's complement, then
  re-invert the MSB to match what
  `crate::requant::requantize_code` would consume. The returned
  code is the radix-`nlevels` digit for grouped classes and the
  raw `bits_per_codeword`-bit codeword for ungrouped classes.
  `quantize_scaled(class, sf_index, s')` divides out the Table
  3-B.1 multiplier before quantizing (rejects the reserved
  scalefactor index `63`). `write_triplet(class, &[s''; 3],
  writer)` drives an `oxideav_core::bits::BitWriter` through one
  (subband, granule) triplet: for grouped classes it packs the
  three digits via the radix-`nlevels` rule `combined = s[0] +
  nlevels·s[1] + nlevels²·s[2]` (exact inverse of
  `requant::degroup`) and writes one `bits_per_codeword`-bit
  field; for ungrouped classes it writes three independent
  `bits_per_codeword`-bit codes. `write_triplet_scaled(class,
  sf_index, &[s'; 3], writer)` layers the §2.4.3.3.3 Table 3-B.1
  division on top. The writer advances by exactly the bit count
  `crate::requant::read_triplet` would consume on the decoder
  side. 13 new lib tests (202 → 215): every defined raw code of
  every Table 3-B.4 class round-trips through `requantize_code →
  quantize_sample` back to itself (the bin-centre identity); an
  arbitrary `s''` produces a code whose `requantize_code`-decoded
  bin centre is within one quantization step (`C / 2^(n−1)`); the
  grouped-class digit never falls outside `[0, nb_steps)`
  (otherwise `degroup`'s range check would fire); out-of-range
  positive / negative inputs clamp to `nb_steps − 1` / `0` for
  grouped and `2^n − 1` / `0` for ungrouped; `group_combined`
  inverts `requant::degroup` exhaustively for every triple of
  grouped classes 3 / 5 / 9 (27 + 125 + 729 = 881 combinations
  walked); `quantize_scaled` reproduces `quantize_sample(s' /
  factor)` and rejects index 63; `write_triplet` advances the
  writer by `bits_per_codeword` (grouped) / `3 · bits_per_codeword`
  (ungrouped); `write_triplet` then `read_triplet` round-trips
  every bin-centre triplet for nb_steps ∈ {3, 5, 7, 9, 15, 31, 63,
  127, 255, 511}; `write_triplet_scaled` then `requantize_scaled`
  round-trips bin-centre triplets across five scalefactor indices
  (unity, doubling, mid-range, near-max-attenuation); the
  symmetric input / code property around the zero point holds
  (s'' = C·D maps to code = 2^(n−1)); and every level triplet of
  every grouped class round-trips through write → read exhaustively
  (3³ + 5³ + 9³ = 881 triplets).

- §C.1.5.2.7 encoder iterative bit-allocator (`encoder_bit_allocator`
  module). `allocate_bits(&FrameHeader, &SmrTable, banc) ->
  Result<AudioData, BitAllocError>` runs the Annex C iterative loop:
  initialise every `nb_steps[ch][sb] = 0`, compute the constant-budget
  terms (`bhdr=32`, `bcrc ∈ {0, 16}`, `bbal = Σ nbal(sb) × channels
  or shared above bound`) via the public
  `fixed_bit_budget(&FrameHeader)` helper, then repeatedly pick the
  lowest-MNR `(channel, sub-band)` slot, advance its B.2 row position,
  charge the marginal sample-bit cost (and on first-time non-zero the
  worst-case 2-bit scfsi + 18-bit scalefactor reservation), back out
  the step if `adb` would go negative, otherwise update the slot's
  MNR with the new `SNR(nb_steps)` from Table C.5. The Annex C Table
  C.5 SNR table is exposed via `snr_db(nb_steps) -> Option<f64>`
  (`0 → 0.00 dB` through `65535 → 98.01 dB`, monotonically
  increasing); the per-(channel, sub-band) sample-codeword cost is
  exposed via `sample_bits_for(nb_steps) -> u32` (grouped classes
  pack 3 samples per codeword → `12 × bits_per_codeword`; ungrouped
  → `36 × bits_per_codeword`). The §2.4.1.6 joint-stereo
  above-`bound` shared-allocation rule is enforced inline (a merged
  slot's `nb_steps` advances for both channels simultaneously; both
  per-channel scfsi + scalefactor reservations still count; the
  merged MNR feeds from the *worse* of the two channels' MNRs so the
  joint allocation chases the noisier channel). 14 new lib tests
  (188 → 202): Table C.5 landmarks, strict monotonicity, every
  Table 3-B.4 step having a C.5 entry, `sample_bits_for` against
  grouped vs ungrouped classes, the `fixed_bit_budget` arithmetic
  across canonical 192k/44.1k stereo + joint-stereo bound=4 +
  single-channel 80 kbit/s, the budget invariant under both
  uniformly-negative and uniformly-+100 dB SMR, the priority
  property (single high-SMR slot ends with the largest `nb_steps`),
  every emitted `nb_steps` reachable through
  `BitAllocTable::allocation_index`, the joint-stereo above-`bound`
  `nb_steps[0][sb] == nb_steps[1][sb]` invariant, the
  `InsufficientFrameSize` error path, and an end-to-end round-trip
  through `write_audio_data` → `parse_audio_data` confirming the
  allocator's `nb_steps` survives the audio-data wire format.

- §2.4.1.6 audio-data writer (encoder side): `write_audio_data` and
  `write_audio_data_with_section_bits` in the `audio_data` module are
  the bit-for-bit inverse of `parse_audio_data` /
  `parse_audio_data_with_section_bits`. For one Layer II frame, the
  writer emits, in order, the per-(sb, ch) `nbal`-bit allocation
  indices (with the §2.4.1.6 `sb >= bound` joint-stereo branch writing
  ONE shared index per subband), the 2-bit `scfsi[ch][sb]` for every
  (sb, ch) with non-zero allocation, and the 1/2/3 on-wire 6-bit
  scalefactor indices per the chosen `scfsi` schedule. Allocation
  indices are derived via `BitAllocTable::allocation_index`
  (round 175); scfsi codes derive from the `Scfsi` enum the §C.1.5.2.5
  selector (round 202) hands the writer; scalefactor indices come from
  `compute_scalefactors` (round 195) after SCFSI selection has
  arranged the `[scf1, scf2, scf3]` triple to match the schedule's
  reconstruction rule. `write_audio_data_with_section_bits` returns
  the bit-lengths of the §2.4.1.6 bit-allocation and scfsi sections so
  the yet-to-be-built §2.4.3.1 encoder CRC accumulator can index
  Annex B Table B.5 without re-parsing. A new `AudioDataWriteError`
  enum reports the encoder-side self-inconsistencies:
  `NoBitallocTable`, `InconsistentLayout` (`AudioData` disagrees with
  header on `table` / `channels` / `bound`),
  `IntensityStereoAllocationMismatch` (above-`bound` subband has
  unequal per-channel `nb_steps` — forbidden by §2.4.1.6),
  `UnencodableNbSteps` (`nb_steps` not in any row of the active
  sub-table), and `ReservedScalefactorIndex` (scalefactor index 63 is
  reserved per §2.4.2.5). 10 new lib tests (178 -> 188): uniform
  192 kbit/s stereo round-trip, joint-stereo above-bound round-trip,
  zero-allocation skip path, all four scfsi schedules round-trip,
  section bit-count parity with `parse_audio_data_with_section_bits`,
  the four error paths (inconsistent layout / unencodable `nb_steps` /
  reserved scalefactor 63 / intensity-stereo allocation mismatch),
  and an exhaustive (every B.2 sub-table, every scfsi schedule) mono
  round-trip walking 4 tables x 4 scfsi schedules through write ->
  parse and asserting equality.

- Annex C §C.1.5.2.5 / §C.1.5.2.6 encoder-side SCFSI selection
  (`encoder_scfsi` module: `select_scfsi`, `classify_difference`,
  `DifferenceClass`, `TransmissionPattern`, `ScfsiSelection`). For
  each `(channel, subband)` slot, consumes the three Table 3-B.1
  scalefactor indices produced by `compute_scalefactors` and emits
  the §C.1.5.2.5 "adjusted" `used` triple, the §C.1.5.2.5
  transmission pattern (which slots are physically written), and
  the 2-bit `scfsi` code matching one of the four `audio_data::Scfsi`
  schedules. Classification of `dscf1 = scf1 - scf2` and
  `dscf2 = scf2 - scf3` into the five spec classes (PDF page 73)
  indexes Table C.4 (PDF page 76). The "4 = max scalefactor"
  recipe at row (2,4) maps to the minimum index because Table 3-B.1
  is monotonically decreasing (larger multiplier ↔ smaller index).
  9 new lib tests (169 → 178): full 25-row Table C.4 lookup pin
  (input chosen to land in each target row, then `used` / pattern /
  `scfsi` cross-checked column-by-column against the PDF), every
  classifier boundary, all-identical-triplet → ShareAll, large
  strictly-changing-indices in both monotonic directions, the (2,4)
  max-recipe semantics, transmitted-slot-count consistency per row,
  the wire round-trip (writing the on-wire 6-bit slots under the
  chosen `scfsi` schedule reconstructs the encoder's claimed `used`
  triple), purity / determinism, and the "at least one slot
  transmitted" lower bound.

- §2.4.3.3.3 / Annex C §C.1.5.2.6 encoder scalefactor extraction
  (`encoder_scalefactors` module: `compute_scalefactors`,
  `extract_scalefactor_index`, `pick_scalefactor_index`). For each
  scalefactor-granule of 12 sub-band samples, selects the smallest
  Table 3-B.1 multiplier that is `>= max(|sample|)` and emits its
  6-bit index — the inverse of the §2.4.3.3.3 decode lookup
  (Table 3-B.1 is monotonically decreasing, so this is the largest
  index whose entry still covers the granule peak). All-zero
  granules map to index 62; out-of-range input clamps to index 0
  per the §2.4.3.4.7.1 `[-1, +1)` precondition. 8 new lib tests
  (161 → 169).

## [0.0.8](https://github.com/OxideAV/oxideav-mp2/releases/tag/v0.0.8) - 2026-05-30

### Other

- §C.1.3 Annex C polyphase analysis filterbank (encoder side)
- ISO/IEC 13818-3 LSF Layer II support (decode + emit + Table B.1)
- wire `oxideav_core::Decoder` trait surface + registry tags
- mp2 r175 encoder step 2: §2.4.2.3 bit-allocation inverse mapping
- mp2 r162: malformed-input property tests (+14, 107 → 121)
- mp2 r157 encoder step 1: §2.4.2.3 frame-header writer
- rebuild step 6 fix-up: skip fixture-driven tests when docs/ is absent
- rebuild step 6: §2.4.1.6 / §2.4.3.1 / §2.4.3.2 frame-level decode loop
- rebuild step 5: §2.4.3.2 / §2.4.3.3.5 polyphase synthesis filterbank
- rebuild step 4: §2.4.1.4 / §2.4.3.1 CRC-16 over Annex B Table B.5 protected fields
- rebuild step 3: §2.4.3.3.4 Layer II sample requantizer
- rebuild step 2: §2.4.1.6 audio-data side info (bit allocation + scfsi + scalefactors)
- rebuild step 1: Layer II frame header + Annex B Table B.1 scalefactors
- rustfmt — split Display write! across lines (Linux CI parity)
- orphan rebuild: clean-room scaffold post-audit 2026-05-24

### Added (round 192 — §C.1.3 Annex C analysis filterbank for encoder side)

- **§C.1.3 Annex C polyphase analysis subband filterbank.** The
  encoder's time-reversed dual of the §2.4.3.3.5 decoder filterbank,
  packaged as a `tables_analysis` module (Annex C Table C.1
  coefficients C[i], all 512 entries) and an `analysis` module
  (`AnalysisFilterbank::push_audio(&[f64; 32], &mut [f64; 32])`,
  exactly mirroring the existing `SynthesisFilterbank::push_subbands`
  API). One call consumes 32 PCM input samples and produces 32
  subband samples through the §C.1.3 pipeline: shift X buffer →
  insert most-recent-at-X[0] → window `Z = X * C` → compact
  `Y_i = sum_{j = 0..8} Z[i + 64*j]` → matrix
  `S_i = sum_{k = 0..64} M_ik * Y_k`. The 32×64 `M_ik` matrix is
  precomputed at construction from the §C.1.3 closed form
  `M_ik = cos[(2i + 1)(k - 16) * pi / 64]`. The 512 C[i] values
  are read verbatim from Annex C Table C.1 (PDF pages 67-69) via
  300-DPI tesseract OCR on `pdftoppm`-rendered PNGs, with two
  independent `pdftotext` extractions as tie-breakers against
  index-side OCR noise. The spec-paired filterbank-window identity
  `D[i] == 32 * C[i]` (where D is Annex B Table 3-B.3, the synthesis
  side) is honoured to within 1 ULP at the 9-decimal-digit grid;
  this is cross-checked by the
  `c_matches_d_over_32_within_rounding` unit test as a transcription
  oracle (both tables come from the same PDF; the spec pairs them by
  the prototype low-pass response, with the synthesis side
  compensating the 32-subband critical sampling). The
  `most_recent_sample_lands_at_x0` and
  `shift_then_insert_preserves_old_samples_at_offset_32` tests pin
  the §C.1.3 X-buffer convention literally.

- **Cross-checks.** 22 new lib tests (161 total, was 139): C table
  size = 512, `C[0] == 0`, `C[256] == 0.035780907` (global peak),
  `|C[69]| == |C[70]| == |C[442]| == |C[443]| == 0.000108719`
  (secondary-peak symmetric pair), 7 sign-block boundaries flip at
  documented indices (64 / 128 / 192 / 256 / 320 / 384 / 448),
  magnitude anti-mirror identity `|C[256 + k]| == |C[256 - k]|`
  for k = 1..=255, and the D/32 invariant across all 512 entries.
  M_ik matrix matches the closed form for every (i, k) ∈ [0, 32) ×
  [0, 64), hits the algebraic landmarks `M[0, 16] = M[8, 16] =
  M[31, 16] = cos(0) = 1` and `M[0, 0] = cos(-pi/4) = sqrt(2)/2`,
  and is bounded by 1 in magnitude. Filterbank invariants: X buffer
  starts at zero, zero input across 100 frames produces exactly zero
  output, `reset()` re-zeros X, two independent instances given
  different inputs produce different outputs, unit DC input across
  64 frames stays finite + bounded.

### Added (round 185 — ISO/IEC 13818-3 LSF Layer II support)

- **Low-sampling-rate (LSF) Layer II decode + emit.** `FrameHeader`
  now decodes both ISO/IEC 11172-3 (`ID == 1`, MPEG-1) and ISO/IEC
  13818-3 (`ID == 0`, LSF) Layer II headers. A new
  `FrameHeader::lsf: bool` field captures the parsed `ID` bit; the
  bitrate (8 / 16 / 24 / 32 / 40 / 48 / 56 / 64 / 80 / 96 / 112 /
  128 / 144 / 160 kbit/s, ISO/IEC 13818-3 §2.4.2.3 PDF page 21) and
  sampling-frequency (16 / 22.05 / 24 kHz, same page) tables are
  exposed as `decode_bitrate_lsf` / `encode_bitrate_lsf` and
  `decode_sampling_frequency_lsf` / `encode_sampling_frequency_lsf`.
  `FrameHeader::emit_bytes` round-trips LSF headers bit-for-bit.
  The ISO 11172-3 §2.4.2.3 "not all (bitrate, mode) combinations
  are allowed" matrix is restricted to MPEG-1; the 13818-3 LSF
  extension does not restate the matrix, so every LSF (bitrate,
  mode) pair is accepted by both `parse` and `emit_bytes`.
  `HeaderError::LsfNotSupported` (previously raised on every
  `ID == 0` frame) is removed.

- **ISO/IEC 13818-3 Annex B Table B.1 bit-allocation table.**
  `BitAllocTable::B1Lsf` is a new variant covering the single
  Layer II bit-allocation table that ISO/IEC 13818-3 §2.4.3.1
  mandates in place of the four ISO 11172-3 sub-tables B.2a..d for
  every LSF Layer II frame. Layout per PDF page 71: `sblimit = 30`,
  per-subband `nbal` widths `4/4/4/4/3/3/3/3/3/3/3/2 * 19/0 * 2`,
  sum of `nbal = 75`. The 30-row table is encoded as three shared
  PDF-row slices plus the trailing two empty rows. `select_table`
  unconditionally routes `header.lsf == true` to `B1Lsf`; every
  Table B.1 cell is covered by the existing
  `every_b2_table_cell_resolves_to_a_known_b4_class` and
  `allocation_index_is_total_inverse_of_nb_steps_for_every_cell`
  exhaustive tests (Table 3-B.4 is shared between 11172-3 and
  13818-3 per §2.4.3.1 prose).

- **Tests.** Five new lib tests (`b1_lsf_sblimit_and_nbal_layout_*`,
  `b1_lsf_subbands_*_to_*_decode_to_iso_13818_3_table_b1_*_row`,
  `select_table_routes_every_lsf_header_to_b1_lsf`) plus a fresh
  `tests/lsf_layer2.rs` integration file with six end-to-end
  scenarios (header parse over the 42-cell LSF bitrate × sample-rate
  grid, 168-cell parse/emit round-trip including all four modes,
  padding-bit byte-count check, sblimit drives audio_data
  iteration, all-zero-payload truncation diagnostics, all-zero
  allocation → bit-exact silence at 1152 samples per channel via
  `decode_frame`). Total: 139 lib + 14 malformed_input + 6 LSF
  integration = 159 tests, all green.

### Added (round 182 — `oxideav_core::Decoder` trait wiring + registry tags)

- **`codec_decoder` module** wraps the existing
  `frame::decode_frame_with` primitive in the framework's packet-in /
  frame-out [`oxideav_core::Decoder`] trait so containers can route
  Layer II streams through the registry. The new public surface is:
  - `make_decoder(&CodecParameters) -> Result<Box<dyn Decoder>>` —
    factory used by `oxideav_core::CodecRegistry::first_decoder` /
    `make_decoder`; defaults blank `sample_rate` to 44_100 and
    blank `channels` to 1, rejects `channels != 1 && channels != 2`
    (the §2.4.2.3 `mode` field encodes at most two channels), and
    re-derives the real rate / channel count from each frame header
    on the first `send_packet`.
  - `Mp2CoreDecoder` — packet-to-frame adaptor; threads the Annex A
    Figure A.2 V ring buffer across packets via an internal
    [`FrameDecodeState`], queues one [`AudioFrame`] per
    `send_packet`, and surfaces `decode_frame_with` errors through
    the trait's `Error::other` channel. `reset()` wipes the
    filterbank state per the trait contract; `flush()` blocks
    further `send_packet` calls and causes a subsequent
    `receive_frame` to return `Error::Eof` once the pending-frames
    queue drains.
  - `register_codecs(&mut CodecRegistry)` — installs the codec under
    id `"mp2"` with `CodecCapabilities::audio("mp2").with_decode()`
    and claims two container tags:
    `CodecTag::wave_format(WAVE_FORMAT_MPEG)` (Win32 `mmreg.h`
    `0x0050`, shared with Layer I per §B.1.6.6) and
    `CodecTag::matroska("A_MPEG/L2")`. A `probe_mp2` disambiguates
    the `0x0050` collision with `oxideav-mp1` by inspecting the
    §2.4.1.3 layer field of the first packet (bits 18..17): `'10'`
    Layer II → 1.0; `'11'` / `'01'` Layer I / III → 0.0; no packet
    hint → 0.5; bad sync / short packet → 0.1.
  - `CODEC_ID_STR = "mp2"` and `WAVE_FORMAT_MPEG = 0x0050` are
    re-exported at the crate root for downstream containers.
- **Top-level `register(&mut RuntimeContext)`** now installs the
  decoder into `ctx.codecs` via `codec_decoder::register_codecs`
  (the prior body was a no-op stub). The existing
  `oxideav_core::register!("mp2", register)` macro at the crate
  root makes that reachable from `oxideav-meta`.
- **Output PCM format**: planar little-endian `i16` in
  `Frame::Audio` — `data.len() == channels`, each `data[ch]` is
  `1152 * 2` bytes per packet (§2.4.2.1 "1 152 for Layer II").
  Float-to-int rescaling is `clamp(s_f64 * 32767, i16::MIN,
  i16::MAX).round() as i16` so out-of-range physically-unrealisable
  samples saturate at the i16 endpoints without panicking.
- **22 new tests** under `codec_decoder::tests` covering the factory
  shape (mono/stereo accepted, every non-{1,2}-channel hint rejected,
  blank-params default path), trait surface behaviour (one frame
  per packet across the 31-frame staged stereo fixture with
  per-packet PTS propagation, `Error::NeedMore` before any
  `send_packet`, `Error::Eof` after `flush`-then-drain, post-flush
  `send_packet` rejection, `reset` re-enables the surface,
  truncated packet surfaces as a decode error), the layer-field
  probe (returns 1.0 for Layer II, 0.0 for Layer I and Layer III,
  0.5 with no packet hint, < 0.5 on bad sync / short packets, and
  1.0 against the staged fixture's first 4 bytes), the registry
  wiring (`register_codecs` installs a `first_decoder`-discoverable
  factory and resolves both `WAVE_FORMAT_MPEG` and `A_MPEG/L2` back
  to id `"mp2"` via `resolve_tag_ref`), and the `f64 → i16 LE`
  plane converter at the {0, ±0.5, ±1, ±1.5, ±2} reference points
  (with the ±1.5 / ±2 inputs clamped at the i16 endpoints). The
  staged-fixture tests skip cleanly when `docs/` is absent
  (standalone-crate CI checkouts).
- **Re-exports** at the crate root: `make_decoder`, `register_codecs`,
  `Mp2CoreDecoder`, `CODEC_ID_STR`, `WAVE_FORMAT_MPEG`.

The encoder factory is intentionally **not** wired in this round —
the §2.4.1.6 audio-data writer, §C.1.5.2.7 bit-allocation iteration,
and Annex C polyphase analysis filterbank are still pending per
earlier rollups. When those land, the `register_codecs` builder
picks up a `.encoder(make_encoder)` line alongside the existing
decoder factory and `CodecCapabilities` gains `with_encode()`.

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
