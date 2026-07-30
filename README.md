# oxideav-mp2

[![CI](https://github.com/OxideAV/oxideav-mp2/actions/workflows/ci.yml/badge.svg)](https://github.com/OxideAV/oxideav-mp2/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/oxideav-mp2.svg)](https://crates.io/crates/oxideav-mp2) [![docs.rs](https://docs.rs/oxideav-mp2/badge.svg)](https://docs.rs/oxideav-mp2) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

Clean-room implementation. Every numeric table is read only from
ISO/IEC 11172-3 (1993) with Annex B, and from ISO/IEC 13818-3 (1997)
§2.4.2.3 / Annex B Table B.1 for the MPEG-2 LSF (Lower Sampling
Frequencies) extension. The decoder is complete end-to-end (frame →
PCM) and **validated against real Layer II fixtures spanning the whole
channel-mode × sampling-rate matrix** (MPEG-1 mono/stereo at
32/44,1/48 kHz + MPEG-2 LSF at 16/22,05/24 kHz) to within the ISO
floating-point-filterbank conformance bound (max abs ≤ 1 LSB, per-frame).
The encoder is complete through frame assembly with **both** Annex D
psychoacoustic models (§D.1 Model 1 and §D.2 Model 2) driving the
§C.1.5.2.7 bit allocator automatically at **all six** Layer II
sampling rates — the 11172-3 tables at the MPEG-1 rates and ISO/IEC
13818-3's own Annex D ("Psychoacoustic model 1/2 for Lower Sampling
Frequencies") at the LSF rates — and **both decoder and encoder are
wired into the runtime registry** (frame-in / packet-out
`Mp2CoreEncoder`).

## What works today

**Decode** — Layer II frames to PCM, MPEG-1 and MPEG-2 LSF:

- **Frame header** (§2.4.1.3 / §2.4.2.3): the 32-bit header parsed into
  a typed `FrameHeader` with full validation — syncword, layer, the
  bitrate / sampling-frequency ladders (MPEG-1 and the LSF 8–160 kbit/s
  / 16 / 22.05 / 24 kHz tables), the §2.4.2.3 disallowed (bitrate, mode)
  matrix (MPEG-1 only), and reserved-code rejection.
- **Frame sizing** (`floor(144 · bitrate / Fs) + padding`) and cold
  sync search (`find_sync`).
- **Free format** (§2.4.2.3, `bitrate_index == '0000'`): free-format
  streams carry no signalled bitrate, so the constant frame size is
  recovered by measuring the distance between consecutive syncwords
  (with a two-frame sync-lock that rejects false-positive sync patterns
  inside the payload — §2.4.2.3 "a frame contains either N or N+1 slots,
  depending on the value of the padding bit"). The Annex B
  bit-allocation table is fixed by the **sampling frequency alone** —
  the Table 3-B.2a header lists free format at 48 kHz and the Table
  3-B.2b header lists it at 44,1 / 32 kHz (r411 correction: the table
  was previously keyed on the recovered bitrate, which an independent
  reference decoder's free-format output disproved — it now agrees
  **100 % bit-exactly**). The fixed rate need not be on the §2.4.2.3
  ladder ("a fixed bitrate which does not need to be in the list"):
  off-ladder streams decode, with the nominal rate `⌈N·Fs/144⌉`
  recovered as metadata, bounded only by the per-standard free-format
  decoder-support ceiling (11172-3: 384 kbit/s; 13818-3 LSF:
  160 kbit/s); the §2.4.2.3 bitrate/mode
  matrix's free-format row is "all modes", so no mode restriction
  applies either. `decode_free_format_stream` walks a whole free-format
  stream; the registry `Mp2CoreDecoder` also handles a free-format
  packet directly (the packet length is the frame size). The §2.4.2.3
  free-format **encode** path (`to_free_format` /
  `rewrite_to_free_format`, and the registry `freeformat` option — which
  now rejects a bitrate whose signalled table differs from the
  free-format table, the configuration that would decode to garbage on
  every conforming decoder) emits a free-format stream that round-trips
  bit-exactly back through the decoder (`tests/free_format.rs`).
- **Bit allocation, scfsi and scalefactors** (§2.4.1.6 / §2.4.3.3):
  the Annex B Tables 3-B.2a..d, Table 3-B.4 quantization classes, and
  the 13818-3 Table B.1 LSF allocation table; `select_table` routes
  each header to the correct sub-table.
- **Sample requantization** (§2.4.3.3.4): MSB-invert → two's-complement
  fraction → `s'' = C · (s''' + D)`, radix-`nlevels` degrouping for the
  grouped classes, and Table 3-B.1 rescaling.
- **Intensity stereo** (§2.4.1.6 / §2.4.2.6): in `joint_stereo` mode the
  sample loop reads one shared sample codeword per subband above `bound`
  (`samplecode[0][sb][gr]`, valid for both channels), and each channel
  rescales it by its own scalefactor — keeping the bitstream aligned
  through the intensity region. The four `mode_extension` bounds (4 / 8 /
  12 / 16) are honoured.
- **CRC-16** (§2.4.1.4 / §2.4.3.1) over the Annex B Table B.5 protected
  fields, verified on decode.
- **§2.4.2.4 de-emphasis** ("emphasis — indicates the type of
  de-emphasis that shall be used"): the header emphasis field, formerly
  parsed but never acted on, now drives a de-emphasis IIR on the
  reconstructed PCM. The `'01'` 50/15 µs curve's coefficients are
  derived clean-room from its two time constants (`τ1 = 50 µs`,
  `τ2 = 15 µs`) via the bilinear transform (unity DC gain; the HF shelf
  asymptote `τ2/τ1 = 0.3` = −10.458 dB). The `'11'` **CCITT J.17**
  curve is read from the staged **ITU-T Rec. J.17 (11/88)** itself
  (`docs/audio/mp3/T-REC-J.17-198811-I.pdf` + clean-room note:
  first-order shelf, pre-emphasis zero ≈ 477.5 Hz / pole ≈ 4134 Hz,
  18.75 dB span, ± 0.25 dB tolerance), whose Table 1/J.17 also settles
  the once-open **absolute-gain convention** (ask #256): J.17 fixes
  the shape only — the "6.5 dB @ 800 Hz" figure is ITU-T J.34's
  equipment-specific flat alignment — and, since ISO/IEC 11172-3 cites
  J.17 alone and bounds decoder output to −1,0 … +1,0, the **DC-unity**
  normalisation (0 dB at DC → −18,75 dB at HF, a pure attenuator) is
  the ruled convention. Realised as an order-3 minimum-phase cascade
  fitted per sample rate — a plain bilinear first-order section cannot
  hold the tolerance against the warp — with the fit staying < 0.02 dB
  from the analytic curve at all six rates, every Table 1/J.17 row
  pinned by test (analytically, and per fitted cascade via the
  Recommendation's own ± 0.25 dB / 800 Hz-alignment acceptance
  procedure), and the 44.1 kHz result cross-checked against both of
  the note's reference fits (see `src/j17.rs`). Because the note's §5
  survey shows third-party decoders parse-and-discard the field (no
  external PCM fixture can exist), `tests/deemphasis.rs` replays the
  note's header-rewrite probe on the staged 44,1 kHz fixture against
  this decoder's own honouring chain, and pins that the emphasis bits
  sit inside the Table B.5 CRC-protected header half. Per-channel
  filter state is threaded across frames (re-zeroed on `reset`);
  `'00'` (none) is delivered unfiltered.
- **§2.4.1.8 `ancillary_data()`** — the raw frame tail (the §2.4.2.8
  `no_of_ancillary_bits` = frame budget minus header / error-check /
  audio-data spend; content user-definable) is surfaced on every
  `DecodedFrame` as `Ancillary`: the exact tail bit count, the
  sub-byte residue left by the non-byte-granular §2.4.3.3.4 sample
  loop, and the whole tail bytes — closing the round trip with the
  encode-side `encode_frame_with_ancillary` payload path
  (`tests/ancillary.rs`: byte-for-byte payload recovery, the tail
  pinned *outside* the Table B.5 CRC-protected region, and the
  length identity checked across the staged fixture's frames).
- **Polyphase synthesis filterbank** (§2.4.3.2, Annex A Figure A.2):
  the 64×32 matrixing, the 512-tap Table 3-B.3 window, and the V ring
  buffer carried across frames — 1152 PCM samples per channel per frame.
- **Frame-level decode loop**: `decode_frame` / `decode_all_frames`
  parse a stream end-to-end with per-stream filterbank state and
  mid-stream resynchronisation. Every frame is sized and allocated
  from its **own** header, so streams whose frames switch ladder
  bitrates (or `mode`) decode frame-by-frame — §2.4.2.3 leaves
  variable-bitrate support optional for a Layer II decoder; this one
  provides it (`mixed_bitrate_stream_decodes_frame_by_frame`).
- **PCM conformance vs. real fixtures across the whole rate ×
  allocation matrix**: the full decode chain is validated end-to-end
  against the staged `layer2-stereo-44100-192kbps` fixture's
  `expected.wav` (31 frames → 71 424 interleaved s16 samples) **and**
  against an independent black-box reference decoder over a
  43-stream corpus (`tests/decode_matrix_conformance.rs`, fixtures +
  generation notes with SHA-256 sums under `tests/fixtures/`) spanning
  the complete Layer II matrix: MPEG-1 mono/stereo at 32 / 44,1 /
  48 kHz **plus** every Table 3-B.2 bit-allocation sub-table
  (B.2a/b/c/d) in both channel modes, the bitrate-ladder extremes
  (32 kbit/s mono … 384 kbit/s stereo), MPEG-2 LSF at 16 / 22,05 /
  24 kHz including the ladder extremes (8 … 160 kbit/s) and the
  LSF-only 144 kbit/s index, padding-heavy fractional-rate streams
  (up to 22 of 23 frames padded), joint-stereo at **every**
  `mode_extension` bound with a live §2.4.1.6 intensity region (plus
  the B.2c bound-clamp edge), dual-channel, §2.4.1.4 CRC-protected
  frames, and (r419) psychoacoustically-driven cells: Model-1 /
  Model-2 stereo and joint-stereo intensity at the LSF rates, the
  Annex G.1 demand-driven per-frame stereo/joint-stereo policy, and a
  right-only-above-bound sum-signal content pin (all ≤ 0.0171 LSB vs
  the float reference, premises pinned bitstream-side). The r411 cells store the reference decoder's **float** PCM,
  so the assertable bound is **≤ 0.05 LSB in the float domain**
  (measured ≤ 0.025 LSB — the reference's own f32 precision floor; our
  chain is f64 end-to-end) with a ≥ 99 % bit-exact s16 projection whose
  residual ±1 flips sit only on rounding-boundary straddles; a
  two-independent-reference latitude study in
  `tests/fixtures/GENERATION.md` shows the references disagree with
  *each other* at that same magnitude (ISO/IEC 11172-4 defines
  conformance as a bounded difference signal; §2.4.3.2 / §2.4.3.3.5
  specify the filterbank in floating point with no fixed accumulation
  order), and one cell is 100 % s16 bit-exact against the second
  reference. The envelope holds **per individual 1152-sample frame**
  (including the cold-start frame 0 whose §2.4.3.3.5 V buffer is zero
  per Annex A Figure A.2 footnote 1). A streaming-equivalence check
  confirms frame-by-frame `decode_frame_with` with persisted state is
  bit-identical to the batch `decode_all_frames` path. The
  fractional→`i16` map uses the symmetric `2^15` full-scale
  (`−1.0 ↦ −32768`) matching the §2.4.3.3.4 "MSB represents −1"
  convention.

**Encode** — the frame-assembly path is in place: the CRC-16 write
primitives, the header writer (`FrameHeader::emit_bytes`), the §C.1.3
polyphase analysis filterbank, scalefactor extraction, the SCFSI
Table-C.4 selection, the §2.4.1.6 audio-data writer, the §C.1.5.2.7
iterative bit allocator (the joint-stereo merged slot pays its single
shared codeword **once**, per the §2.4.1.6 wire syntax), the
§2.4.3.3.4 quantizer, and the frame-level orchestrator (`encode_frame`
/ `encoder_frame` module).

**§2.4.2.4 pre-emphasis.** The encode counterpart of decode
de-emphasis: when a frame header signals the 50/15 µs or CCITT J.17
curve the encoder pre-emphasises the PCM (per channel, IIR state
threaded across frames through `EncodeFrameState`) *before* both the
§C.1.3 analysis filterbank and the Annex D psychoacoustic model, so the
encoded signal and its bit allocation stay consistent and the decoder's
de-emphasis restores the original spectral balance. `PreEmphasis` is
the exact algebraic inverse of `DeEmphasis` (each first-order section
inverted; well-defined because both curves' realisations are
minimum-phase); the pre→de cascade is identity to machine precision for
both curves, and acoustic round-trip tests confirm a pre-emphasis
encode → de-emphasis decode reproduces both a low and a high-frequency
tone (`tests/deemphasis.rs`).

**§2.4.2.3 padding-bit rate control.** The public `PaddingScheduler`
implements the spec's verbatim `rest`/`dif` decision procedure; the
batch `encode_all_frames` family and the registry encoder drive one
per stream, so at the fractional rates (44,1 / 22,05 kHz — "Padding is
necessary with a sampling frequency of 44,1 kHz") padded `N+1`-slot
frames interleave to hold the accumulated coded length strictly within
one slot of the exact `Σ 144·bitrate/Fs` target
(`tests/padding_rate_control.rs` walks the emitted frames against the
algorithm, checks the mean-bitrate envelope, and resolves a **padded
free-format** stream to bit-identical PCM).

**Annex G.1 intensity stereo.** Above `bound` the shared on-wire
codeword is the Annex G.1 **sum signal** `L + R`, quantized against
the sum's own (untransmitted) scalefactor while each channel's own
scalefactor is transmitted — so channel-1-only content above the bound
survives the encode (pinned by
`intensity_sum_signal_preserves_right_only_content_above_bound`). The
Annex G.1 **demand-driven selection** is also implemented: per frame,
`choose_stereo_coding` estimates the required bits
(`demand_bits` — every slot to `MNR ≥ 0`, the merged slot sized
against the more demanding channel) and picks full `Stereo` when it
fits the budget, else the widest `JointStereo` bound that fits
(16 / 12 / 8 / 4, Bound4 fallback). Exposed as
`encode_frame_auto_js_with` / `encode_frame_auto_js_model2` /
`encode_all_frames_js` and the registry `bound=auto` option; one
stream may legally mix `Stereo` and `JointStereo` frames (§2.4.1.3 —
each frame carries its own `mode`).

**§D.1 Step-1 window placement.** The Model-1 analysis FFT reads the
spec's *delayed* window — 256 samples of filterbank-delay compensation
minus 64 of Layer II centring, i.e. frame `f` analyses
`stream[f·1152 − 192 .. f·1152 + 832]` — via a per-channel 192-sample
history in `EncodeFrameState` (`MODEL1_WINDOW_DELAY_SAMPLES`),
zero-filled at stream start.

The encoder now has an **auto-SMR (psychoacoustically-driven) encode
path** — `encode_frame_auto` / `encode_frame_auto_with` — that derives
the §C.1.5.2.7 bit-allocator's signal-to-mask-ratio table automatically
from each frame's PCM through the §D.1 Model-1 chain
(`psy::compute_smr_model1_frame`): a Hann-windowed 1024-point FFT
power-density spectrum (Step 1) → 96 dB SPL normalisation → per-subband
sound-pressure level `L_sb(n)` (Step 2) → tonal / non-tonal masker
extraction (Step 4) → threshold-in-quiet + bit-rate-offset decimation
(Step 3 + 5a) → 0.5-Bark tonal decimation (Step 5b) → per-line global
masking threshold `LTg(i)` (Step 6/7) → per-subband minimum masking
threshold `LT_min(n)` (Step 8) → `SMR_sb(n) = L_sb(n) − LT_min(n)`
(Step 9). The allocation is psychoacoustically driven at **all six**
Layer II sampling rates: the MPEG-1 rates (32 / 44,1 / 48 kHz) run the
11172-3 Annex D tables, and the MPEG-2 LSF rates (16 / 22,05 / 24 kHz)
run ISO/IEC 13818-3's **own** Annex D ("Psychoacoustic model 1 for
Lower Sampling Frequencies") — its Layer II Tables D.1d/e/f
(frequencies / critical-band rates / absolute threshold, 132 entries
each) and D.2d/e/f (critical band boundaries, 21 / 23 / 23 bands) are
transcribed in `tables_lsf`, with the 13818-3-printed adaptations
honoured: rate-dependent Step 4(b) tonality neighbourhoods (`j = ±4`
innermost row, `4 < k < 500` domain) and Step 3 applied **without**
the 11172-3 −12 dB overall-bit-rate offset (the 13818-3 text omits
it). A multi-frame streaming auto-SMR encode round-trips through this
crate's own decoder with the reconstructed-tone residual energy a
fraction of the signal energy, and the auto allocation is verified to
diverge from a flat-SMR allocation on spectrally-uneven input at every
rate. Measured LSF round-trip SNR (64 kbit/s stereo, structured
two-tone + noise, `tests/lsf_psy_conformance.rs`): the psychoacoustic
encodes **beat the flat rate-driven baseline by ≈ 3 dB** at every LSF
rate (16 kHz: 6,07 → 9,43 dB; 22,05 kHz: 8,92 → 11,95 dB; 24 kHz:
10,17 → 12,97 dB). The four original caller-supplied-SMR entry points
(`encode_frame`, `encode_frame_with`, and the two `_ancillary`
variants) are unchanged; a constant table still produces a
syntactically valid, rate-driven frame.

The §D.2 **Model 2** chain is also wired as a selectable auto-SMR source
— `encode_frame_auto_model2` / `encode_all_frames_model2` — driving the
§D.2.1 *twice-per-frame, more-stringent-of-the-pair* Layer II threshold
generator (`psy::compute_smr_model2_layer2_frame`). Model 2 is stateful
(a rolling two-block spectral predictor + 448-sample inter-call carry per
channel) and threads its `Model2Layer2State` through the same
`EncodeFrameState` as the analysis filterbank. At the LSF rates it runs
the 13818-3 D.2 replacement partition tables (D.3.a/b/c "long blocks",
carried in the Layer I/II form with documented, test-pinned column
derivations) with step-(l) absolute thresholds served from the 13818-3
D.1 transcriptions. Integration tests
(`tests/psy_model_shapes_allocation.rs`,
`tests/lsf_psy_conformance.rs`) confirm that for a structured signal at
a constrained bitrate **both** models produce encodes that differ —
byte-for-byte and in the first-frame per-subband allocation — from the
flat-0 dB baseline and from each other, at the MPEG-1 **and** the LSF
rates.

**Registry encoder** — `make_encoder` builds an `oxideav_core::Encoder`
(`Mp2CoreEncoder`) that adapts the auto-SMR encode path into the
framework's frame-in / packet-out trait: it accepts planar-S16
`Frame::Audio`, buffers per channel, and emits one Layer II `Packet`
every 1152 samples (zero-padding a partial trailing frame on `flush`).
`register_codecs` now carries both decoder and encoder factories under
the `"mp2"` id, so the registry exposes MP2 encode for the first time.
The `CodecParameters::options` keys that tune it are: `mode` (`stereo` /
`joint_stereo` / `dual_channel`), `bound` (joint-stereo intensity bound
`4` / `8` / `12` / `16`, or `auto` for the Annex G.1 demand-driven
per-frame policy), `psymodel` (`model1` / `model2`), `freeformat`
(`true` to emit §2.4.2.3 free-format frames at the configured constant
bitrate), `crc` (`true` to emit the §2.4.1.4 CRC-16 word in every
frame), `emphasis` (`50/15` or `j17` to apply the matching §2.4.2.4
pre-emphasis and signal the header field; default `none`), and the
§2.4.2.3 header
metadata flags `copyright` / `original` / `private` (booleans,
round-tripped verbatim on decode). The §2.4.2.3 padding schedule is
always applied.

**Batch stream encode** — `encode_all_frames` /
`encode_all_frames_with_smr` / `encode_all_frames_model2` /
`encode_all_frames_js` / `encode_all_frames_with_ancillary` (one
§2.4.1.8 payload per frame, refused on a frame-count mismatch) are the
encode-side counterpart of
`decode_all_frames`: they turn one continuous per-channel PCM buffer
into the concatenated Layer II byte stream, threading a single
persistent `EncodeFrameState` (the §C.1.3 analysis-filterbank X ring
buffer, the Model-2 predictor and the §D.1 window history) and the
§2.4.2.3 `PaddingScheduler` through every frame so the inter-frame
continuity is byte-identical to a hand-rolled
`encode_frame_auto_with` loop that threads the same scheduler. A per-channel length that is not a whole
multiple of 1152 samples is rejected with `EncodeError::ShortPcmTail`
(the partial trailing frame has no defined Layer II encoding); the
output feeds straight back into `decode_all_frames`.

**Full-matrix encode → decode round-trip.** The complete public
pipeline (`encode_all_frames` → `decode_all_frames`) is validated end
to end across **every** Layer II sampling rate — the three MPEG-1 rates
(32 / 44,1 / 48 kHz) **and** the three MPEG-2 LSF rates (16 / 22,05 /
24 kHz) — for a continuous multi-frame tone
(`tests/roundtrip_multirate.rs`). Per rate the test pins four envelope
properties: exact sample count (`n_frames × 1152` per channel),
reconstruction residual energy below half the signal energy after the
filterbank group delay, Goertzel-bin spectral localisation (the tone
bin dominates an unrelated probe bin by &gt;100×, proving the *right*
tone is reproduced rather than broadband noise), and bit-exact-zero
silence round-trip. Conformance is asserted as a bounded difference
signal per ISO/IEC 11172-4, consistent with the floating-point
filterbank definition.

**Joint-stereo + dual-channel mode×rate matrix.**
`tests/joint_stereo_matrix.rs` broadens the channel-mode axis beyond the
stereo/Bound4-only round-trip above. It round-trips `joint_stereo` at
**every** `mode_extension` bound (4 / 8 / 12 / 16) × every MPEG-1 and
LSF rate, verifies the §2.4.1.6 intensity region is genuinely non-empty
for the wide tables (parsed `bound` matches the clamped expectation, an
above-bound subband is allocated, `nb_steps[0] == nb_steps[1]`),
exercises the §2.4.2.3 `bound = min(bound, sblimit)` clamp at the narrow
B.2c (sblimit 8) / B.2d (sblimit 12) tables where the intensity region
collapses to empty, reconstructs two *independent* tones through
`dual_channel`, and round-trips joint-stereo silence to exact zero. A
companion **encoder-independent fuzz** synthesises raw joint-stereo and
dual-channel frames with adversarial payloads (all-zero, all-ones
max-allocation, alternating bit-walks) and asserts `decode_frame` never
panics — catching shared encoder/decoder intensity-loop bugs that a
symmetric round-trip would mask.

## API

The crate exposes both the registry path
(`oxideav_core::register!("mp2", register)`, installed under WAVE format
tag `0x0050` and Matroska codec id `A_MPEG/L2`, with a layer-field probe
to disambiguate the shared `0x0050` tag from Layer I — carrying **both**
the decoder and encoder factories) and the direct
`codec_decoder::make_decoder` / `codec_encoder::make_encoder` factories.
Decoder output is planar little-endian `i16`; the encoder accepts the
same planar-S16 layout.

## Model 2 (§D.2) internals

**Model 2** is driven **end-to-end to a per-subband signal-to-mask
ratio** by `psy::compute_smr_model2_frame`. Per frame it runs the
§D.2.4 step-(a)…(n) chain: the step-(b) raised-cosine analysis window
+ polar `(r_ω, f_ω)` FFT (`model2_hann_window_layer2` /
`complex_spectrum_polar_layer2`), the step-(c) two-block `r̂/f̂`
prediction (`Model2PredictorState`, advanced across streamed frames),
the step-(d) unpredictability `c_ω` (`unpredictability_measure`), the
step-(e) partition energy + weighted unpredictability
(`partition_energy_and_unpredictability`), the step-(f) spreading
convolution + renormalisation, the step-(g)…(k) threshold loop, the
step-(l) absolute-threshold floor (dB→energy converted against a
+1-lsb-sine FFT reference per the spec's step-(l) note), and the
step-(n) per-coder-partition `SMR_n` mapped to subbands (Table D.5
coder partition `n` ↦ subband `n − 1`). Its calc-partition and
absolute-threshold tables are complete for **all six** Layer II rates
— 11172-3 D.3a/b/c + D.4a/b/c at the MPEG-1 rates, and the 13818-3
D.2-clause replacement tables D.3.a/b/c ("long blocks", carried in
the Layer I/II `CalcPartition` form with documented, test-pinned
column derivations: ω-ranges from the cumulative `FFT-lines` counts,
`bval`/`minval` verbatim, `tmn = max(24,5, bval + 14,5)` dB — a
relation reproducing the printed TMN column of all 164 MPEG-1 Layer
II partitions) with step-(l) thresholds served from the 13818-3 D.1
transcriptions at the LSF rates — selected by
`calc_partition_table_for_rate` / `abs_threshold_table_for_rate`. The
§D.2.1 Layer II *twice-per-frame* rule is also implemented:
`psy::compute_smr_model2_layer2_frame` runs the chain twice per
1152-sample frame (once per `IBLEN_LAYER2` = 576-sample half,
reconstructing each call's 1024-sample window from the 448-sample
inter-call carry held in `Model2Layer2State`) and returns the
per-subband **maximum** of the pair — "the more stringent of each
pair of ratios is used for bit allocation".

## Not yet supported

Both Annex D psychoacoustic models drive the encoder end-to-end at all
six Layer II sampling rates, and both the decoder and encoder are
registry-wired. What remains:

- An ISO/IEC 11172-4 / 13818-4 *compliance-grade* SNR sweep across the
  official layered-test bitstream set (the ISO test bitstreams
  themselves are not staged; the conformance corpus above is built
  from black-box encoders and this crate's own encoder instead). The
  former live-intensity-bound and psy-driven-LSF fixture gaps are
  closed: joint-stereo streams at every `mode_extension` bound —
  including narrow-table B.2d and the B.2c bound-clamp edge —
  dual-channel, CRC-protected, and (r419) Model-1 / Model-2 /
  demand-driven / right-only-intensity streams at the LSF rates are
  decoded against an independent reference decoder's float PCM to
  ≤ 0.024 LSB (r419 cells ≤ 0.0171 LSB), with both independent
  decoders accepting every crate-encoded stream (see **PCM
  conformance** above and `tests/fixtures/GENERATION.md`).

## Robustness

A `tests/malformed_input.rs` suite property-tests the header parser and
frame-decode loop against single-bit header flips and every truncated
prefix of a synthesized frame; `tests/joint_stereo_matrix.rs` adds
encoder-independent panic-freedom fuzz for adversarial joint-stereo and
dual-channel payloads across all four `mode_extension` bounds and the
wide / narrow allocation tables (plus a truncated-prefix walk of a
joint-stereo frame); `tests/free_format_robustness.rs` adds
panic-freedom coverage for the §2.4.2.3 free-format size-measurement
surface (`measure_base_slots` / `resolve` / `decode_free_format_stream` /
`parse_allow_free_format`) against dense sync runs, every truncated
prefix of a free-format frame, and a deterministic pseudo-random corpus;
and a `cargo-fuzz` `decode` target exercises the decode attacker surface
for panic-freedom, with the crafted headers drawing the §2.4.2.3
emphasis field from all three accepted codes so both de-emphasis IIRs
(50/15 µs and CCITT J.17) and their cross-frame rebuild logic sit on
the fuzzed surface.

## License

MIT — see [LICENSE](./LICENSE).
