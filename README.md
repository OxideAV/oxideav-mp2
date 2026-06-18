# oxideav-mp2

A pure-Rust **MPEG-1 / MPEG-2 LSF Audio Layer II** (MP2 / MUSICAM)
codec for the
[oxideav](https://github.com/OxideAV/oxideav-workspace) framework.

## Status

Clean-room implementation. Every numeric table is read only from
ISO/IEC 11172-3 (1993) with Annex B, and from ISO/IEC 13818-3 (1997)
§2.4.2.3 / Annex B Table B.1 for the MPEG-2 LSF (Lower Sampling
Frequencies) extension. The decoder is complete end-to-end (frame →
PCM); the encoder is implemented through frame assembly and is wired to
the runtime registry as a decoder.

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

**Encode** — the frame-assembly path is in place: the CRC-16 write
primitives, the header writer (`FrameHeader::emit_bytes`), the §C.1.3
polyphase analysis filterbank, scalefactor extraction, the SCFSI
Table-C.4 selection, the §2.4.1.6 audio-data writer, the §C.1.5.2.7
iterative bit allocator, the §2.4.3.3.4 quantizer, and the frame-level
orchestrator (`encode_frame` / `encoder_frame` module). The encoder
accepts a caller-supplied per-(channel, sub-band) signal-to-mask-ratio
table; a constant table produces a syntactically valid frame whose
quality is rate-driven.

## API

The crate exposes both the registry path
(`oxideav_core::register!("mp2", register)`, installed under WAVE format
tag `0x0050` and Matroska codec id `A_MPEG/L2`, with a layer-field probe
to disambiguate the shared `0x0050` tag from Layer I) and the direct
`codec_decoder::make_decoder` factory. Output is planar little-endian
`i16`.

## Not yet supported

- A full Annex D §D.1 / §D.2 psychoacoustic model to drive the encoder's
  SMR table automatically. The Model 1 §D.1 chain is now staged in `psy`
  end-to-end through Step 9 — including **Step 3** (the overall-bit-rate
  absolute-threshold offset, −12 dB ≥ 96 kbit/s/ch) and **Step 5(a)**
  threshold-in-quiet decimation (`X(k) ≥ LTq(k)`), reading the Layer II
  Annex D Table D.1d / D.1e / D.1f `LTq` curves now text-transcribed into
  `tables_d2` (`LtqEntry`). The Model 2 §D.2 chain in `tables_model2` now
  reaches the **signal-to-mask ratio** end to end: past the §D.2.4
  step-(f) spreading convolution it runs the step-(g)…(n) threshold loop
  — tonality index (g), required SNR (h), power ratio (i), per-partition
  threshold (j), per-FFT-line spread (k), the absolute-threshold floor
  (l), and the per-coder-partition `SMR_n` (n, with the Table D.5
  narrow/wide-band rule). The §D.2 calc-partition tables are now
  complete for all three Layer II sampling rates — Table D.3a (32 kHz,
  49 partitions), **D.3b (44,1 kHz, 57 partitions)** and **D.3c (48 kHz,
  58 partitions)** — selected by `calc_partition_table_for_rate`, so the
  step-(f) spreading convolution and the step-(g)…(n) threshold loop run
  at every rate. What remains is the D.4 per-line absolute-threshold
  tables (staged as CSVs under `docs/audio/mp3/`) and wiring the chain
  into the encoder's automatic SMR selection.
- Bit-exact PCM-against-reference validation (PSNR / SNR) pending an
  audit pass.

## Robustness

A `tests/malformed_input.rs` suite property-tests the header parser and
frame-decode loop against single-bit header flips and every truncated
prefix of a synthesized frame; a `cargo-fuzz` `decode` target exercises
the decode attacker surface for panic-freedom.

## License

MIT — see [LICENSE](./LICENSE).
