# oxideav-mp2

A pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

Clean-room implementation. Every numeric table is read only from
ISO/IEC 11172-3 (1993) with Annex B, and from ISO/IEC 13818-3 (1997)
§2.4.2.3 / Annex B Table B.1 for the MPEG-2 LSF (Lower Sampling
Frequencies) extension. The decoder is complete end-to-end (frame →
PCM) and **validated against a real Layer II fixture** to within the
ISO floating-point-filterbank conformance bound (max abs ≤ 1 LSB);
the encoder is implemented through frame assembly and is wired to the
runtime registry as a decoder.

## What works today

**Decode** — Layer II frames to PCM, MPEG-1 and MPEG-2 LSF:

- **Frame header** (§2.4.1.3 / §2.4.2.3): the 32-bit header parsed into
  a typed `FrameHeader` with full validation — syncword, layer, the
  bitrate / sampling-frequency ladders (MPEG-1 and the LSF 8–160 kbit/s
  / 16 / 22.05 / 24 kHz tables), the §2.4.2.3 disallowed (bitrate, mode)
  matrix (MPEG-1 only), and reserved-code rejection.
- **Frame sizing** (`floor(144 · bitrate / Fs) + padding`) and cold
  sync search (`find_sync`).
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
- **Polyphase synthesis filterbank** (§2.4.3.2, Annex A Figure A.2):
  the 64×32 matrixing, the 512-tap Table 3-B.3 window, and the V ring
  buffer carried across frames — 1152 PCM samples per channel per frame.
- **Frame-level decode loop**: `decode_frame` / `decode_all_frames`
  parse a stream end-to-end with per-stream filterbank state and
  mid-stream resynchronisation.
- **PCM conformance vs. a real fixture**: the full decode chain is
  validated end-to-end against the staged `layer2-stereo-44100-192kbps`
  fixture's `expected.wav` (31 frames → 71 424 interleaved s16 samples).
  Sample count is exact and the per-sample error envelope is **max abs
  ≤ 1 LSB, rms < 0.6 LSB, > 70 % bit-exact** — the ISO conformance
  bound. (§2.4.3.2 / §2.4.3.3.5 specify the filterbank in floating
  point with no fixed accumulation order or integer-rounding rule, so
  an independent clean-room decoder reproduces a reference decoder's
  output only within that envelope, not byte-for-byte; ISO/IEC 11172-4
  defines conformance itself as a bounded difference signal.) The
  fractional→`i16` map uses the symmetric `2^15` full-scale
  (`−1.0 ↦ −32768`) matching the §2.4.3.3.4 "MSB represents −1"
  convention.

**Encode** — the frame-assembly path is in place: the CRC-16 write
primitives, the header writer (`FrameHeader::emit_bytes`), the §C.1.3
polyphase analysis filterbank, scalefactor extraction, the SCFSI
Table-C.4 selection, the §2.4.1.6 audio-data writer, the §C.1.5.2.7
iterative bit allocator, the §2.4.3.3.4 quantizer, and the frame-level
orchestrator (`encode_frame` / `encoder_frame` module).

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
(Step 9). For the MPEG-1 Layer II rates (32 / 44,1 / 48 kHz) the
allocation is psychoacoustically driven; a multi-frame streaming
auto-SMR encode round-trips through this crate's own decoder with the
reconstructed-tone residual energy a fraction of the signal energy, and
the auto allocation is verified to diverge from a flat-SMR allocation
on spectrally-uneven input. For the MPEG-2 LSF rates — which the
standard tabulates no Annex D Layer II masking curves for — the SMR
degenerates to a flat 0 dB table (rate-driven allocation). The four
original caller-supplied-SMR entry points (`encode_frame`,
`encode_frame_with`, and the two `_ancillary` variants) are unchanged;
a constant table still produces a syntactically valid, rate-driven
frame.

**Batch stream encode** — `encode_all_frames` / `encode_all_frames_with_smr`
are the encode-side counterpart of `decode_all_frames`: they turn one
continuous per-channel PCM buffer into the concatenated Layer II byte
stream, threading a single persistent `EncodeFrameState` (the §C.1.3
analysis-filterbank X ring buffer) through every frame so the
inter-frame continuity is byte-identical to a hand-rolled
`encode_frame_auto_with` loop. A per-channel length that is not a whole
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

## API

The crate exposes both the registry path
(`oxideav_core::register!("mp2", register)`, installed under WAVE format
tag `0x0050` and Matroska codec id `A_MPEG/L2`, with a layer-field probe
to disambiguate the shared `0x0050` tag from Layer I) and the direct
`codec_decoder::make_decoder` factory. Output is planar little-endian
`i16`.

## Not yet supported

- The §D.1 **Model 1** chain is now wired end-to-end into the encoder's
  automatic SMR selection (see **Encode** above): `encode_frame_auto`
  drives `psy::compute_smr_model1_frame` per frame to feed the
  §C.1.5.2.7 bit allocator. What remains on the perceptual side:
  - **Model 2 (§D.2)** is staged in `tables_model2` end-to-end to the
    signal-to-mask ratio — past the §D.2.4 step-(f) spreading convolution
    it runs the step-(g)…(n) threshold loop (tonality index (g), required
    SNR (h), power ratio (i), per-partition threshold (j), per-FFT-line
    spread (k), the absolute-threshold floor (l), and the
    per-coder-partition `SMR_n` (n) with the Table D.5 narrow/wide-band
    rule). Its calc-partition tables (D.3a/b/c) and absolute-threshold
    tables (D.4a/b/c) are complete for all three Layer II rates, selected
    by `calc_partition_table_for_rate` / `abs_threshold_table_for_rate`.
    A `compute_smr_model2_frame` driver + an `encode_frame_auto`-style
    Model-2 entry point is the next wiring step (Model 1 is wired; Model 2
    is staged but not yet driven from the encoder).
  - The §D.1 driver uses the current frame's first 1024 samples for the
    FFT; the §D.1 Step 1 net +192-sample window shift (which needs the
    next frame's lookahead) is a refinement that would tighten the
    time-alignment of the masking estimate to the allocated subband
    samples.
  - The MPEG-2 **LSF** rates (16 / 22,05 / 24 kHz) fall back to a flat
    0 dB SMR — the standard provides no Annex D Layer II masking tables
    for them (a docs/spec gap, not an implementation one).
- An ISO/IEC 11172-4 / 13818-4 *compliance-grade* SNR sweep across the
  full layered-test bitstream set. The single staged `layer2-…-192kbps`
  fixture is validated end-to-end (see **PCM conformance** above); a
  broader multi-rate / multi-mode reference corpus (mono, joint-stereo
  with a live intensity bound, 32 / 48 kHz, MPEG-2 LSF) would harden the
  envelope further but is not yet staged.

## Robustness

A `tests/malformed_input.rs` suite property-tests the header parser and
frame-decode loop against single-bit header flips and every truncated
prefix of a synthesized frame; a `cargo-fuzz` `decode` target exercises
the decode attacker surface for panic-freedom.

## License

MIT — see [LICENSE](./LICENSE).
