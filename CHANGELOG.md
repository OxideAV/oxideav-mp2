# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Xing/Info VBR seek TOC (table of contents)**. The VBR header frame
  now carries the de-facto 100-byte seek table (Xing flag `0x4`), built
  from the per-frame byte sizes so a player seeking to percentage `p`
  jumps to byte `(toc[p] / 256) · total_bytes`. The header magic was
  switched to `"Xing"` (the variable-bitrate spelling) and the flags
  word to `0x7` (Frames + Bytes + TOC). The TOC walk starts from the
  Xing frame at byte 0 so it agrees with the `Frames`/`Bytes` totals
  (which count the header frame per the LAME convention). ffprobe reads
  the resulting stream's duration and the table is monotonic from `0`
  (start of stream) to the last frame start, verified out-of-tree.
- **Decoder Xing/Info/VBRI frame recognition**. The leading VBR-header
  frame is not ISO Layer II audio — its sample region holds the tag
  block (frame/byte counts, the new 100-byte TOC, …), so decoding it as
  audio yields out-of-range codewords. The decoder now scans the leading
  frame's body for the `"Xing"`/`"Info"`/`"VBRI"` magic (trace-doc §7)
  the way ffmpeg/mediainfo do and emits an exact-silence 1152-sample
  placeholder for it, keeping PTS accounting aligned. The scan is
  restricted to the first frame (and re-armed on `reset()`) so dense
  bit-allocation bytes in a genuine audio frame are never mistaken for a
  tag. New unit tests cover magic detection (`Xing`/`Info`/`VBRI` at the
  canonical offsets, no false positive deep in the body) and the TOC
  builder (endpoints, monotonicity, byte-offset tracking on variable
  frame sizes, degenerate inputs); the VBR integration suite gains a
  monotonic-seek-TOC round-trip and the first-frame test now asserts
  exact silence.
- **Header metadata-flag encoder options** — `copyright`, `original`,
  `emphasis`, and `private_bit` — covering the trailing header fields of
  ISO/IEC 11172-3 §2.4.2.3 that the encoder previously hardcoded to 0.
  `copyright`/`original`/`private_bit` are booleans; `emphasis` is a
  string accepting `"none"` (default, `00`), `"50/15"` (`01`,
  50/15 µs), or `"ccitt"` (`11`, CCITT J.17) — the reserved `10` code
  is not exposed. The flags are pure metadata: the audio payload is
  byte-identical to the flags-clear emission. Both the data frames and
  the VBR Xing/Info placeholder frame carry the chosen values so a
  scanning tool sees a consistent header throughout the stream. The
  decoder's `Header` now exposes the parsed `copyright`, `original`,
  `private_bit`, and `emphasis` fields (with an `Emphasis` enum that
  also flags the reserved `10` code). New `tests/encoder_header_flags.rs`
  round-trips each flag through encode → re-parse and confirms the
  audio body is unchanged; ffmpeg black-box-decodes the flagged stream
  cleanly (verified out-of-tree).
- **ATH (Absolute Threshold of Hearing) psychoacoustic bias** in the
  encoder's bit allocator. New `psy.rs` module computes a per-subband
  perceptual weight from the Terhardt analytic ATH curve at the
  stream's sample rate; the allocator scales each subband's energy
  by `weight²` before scoring, so bands far outside the audible range
  (deep sub-bass, near-Nyquist ultrasonic) drop in priority by
  20–40 dB. On music-like inputs with appreciable ultrasonic content
  the change cuts VBR file sizes by 30–60% at mid quality levels with
  no audible degradation. Enabled by default; set the new
  `psy_model="none"` encoder option to opt back into the strict-energy
  v0.0.8 allocator for byte-exact reproducibility.
- **Per-subband intensity-stereo correlation threshold relaxation**
  (5%/10%/15% for subbands 8–15 / 16–23 / 24–31 respectively) so the
  joint-stereo decision engages on "almost correlated" high-band
  material without giving up bits the low bands need.
- `psy_model` encoder option string accepting `"ath"` (default),
  `"none"`, or `""` (alias for default).
- **`dual_channel` encoder option** for emitting `mode = 0b10`
  (dual_channel) on two-channel inputs. Per ISO/IEC 11172-3 §2.4.2.3
  the dual_channel bitstream is byte-identical to plain stereo apart
  from the 2-bit header `mode` field — both channels independent,
  no shared subbands — but the flag tells the decoder the two
  channels are unrelated audio streams (e.g. bilingual broadcast).
  Ignored for mono input; silently overridden by `joint_stereo = true`
  (the two modes share the header `mode` bits). New
  `tests/encoder_dual_channel.rs` covers header-mode emission, plain-
  stereo bitstream parity past the mode field, the
  joint_stereo-overrides-dual_channel precedence, and an unrelated-
  per-channel round-trip.

### Fixed

- **VBR mode no longer picks invalid (mode, bitrate) combinations**
  on MPEG-1 Layer II. The slot picker now respects Table 3-B.2
  restrictions: stereo streams skip the 32/48 kbps slots and mono
  streams cap at 192 kbps. Previously the allocator could pick
  e.g. 32 kbps stereo, producing a frame whose own header reader
  would reject it. MPEG-2 LSF is unaffected (all 14 slots are valid
  in every mode there).

## [0.0.8](https://github.com/OxideAV/oxideav-mp2/compare/v0.0.7...v0.0.8) - 2026-05-06

### Other

- drop dead `linkme` dep
- auto-register via oxideav_core::register! macro (linkme distributed slice)
- unify entry point on register(&mut RuntimeContext) ([#502](https://github.com/OxideAV/oxideav-mp2/pull/502))
- replace never-match regex with semver_check = false

### Changed

- **`register` entry point unified on `RuntimeContext`** (task #502).
  The legacy `pub fn register(reg: &mut CodecRegistry)` is renamed to
  `register_codecs` and a new `pub fn register(ctx: &mut
  oxideav_core::RuntimeContext)` calls it internally. Breaking change
  for direct callers passing a `CodecRegistry`; switch to either the
  new `RuntimeContext` entry or the explicit `register_codecs` name.

## [0.0.7](https://github.com/OxideAV/oxideav-mp2/compare/v0.0.6...v0.0.7) - 2026-05-02

### Other

- migrate to centralized OxideAV/.github reusable workflows
- add joint-stereo (intensity) + VBR with Xing/Info header
- adopt slim VideoFrame/AudioFrame shape
- pin release-plz to patch-only bumps

### Added

- Joint-stereo (intensity stereo, ISO/IEC 11172-3 §2.4.2.6) emission
  in the encoder. Bound is picked per frame from `{4, 8, 12, 16}` by
  per-subband L/R correlation. Enable via the `joint_stereo` encoder
  option.
- VBR rate-control mode in the encoder. Per-frame bitrate slot is
  picked from the standard ladder so the smallest fitting slot is
  chosen, and a Xing/Info header is prepended on flush so the
  downstream stream reports the right average bitrate. Enable via
  the `vbr_quality` (0..=9) encoder option.

## [0.0.6](https://github.com/OxideAV/oxideav-mp2/compare/v0.0.5...v0.0.6) - 2026-04-25

### Other

- drop oxideav-codec/oxideav-container shims, import from oxideav-core
- document + test Layer II intensity stereo
- decode every frame in a multi-frame packet

## [0.0.5](https://github.com/OxideAV/oxideav-mp2/compare/v0.0.4...v0.0.5) - 2026-04-19

### Other

- drop Cargo.lock — this crate is a library
- bump oxideav-core / oxideav-codec dep examples to "0.1"
- bump to oxideav-core 0.1.1 + codec 0.1.1
- migrate register() to CodecInfo builder
- bump oxideav-core + oxideav-codec deps to "0.1"

## [0.0.4](https://github.com/OxideAV/oxideav-mp2/compare/v0.0.3...v0.0.4) - 2026-04-19

### Other

- claim WAVEFORMATEX tag via oxideav-codec CodecTag registry
- bump oxideav-core to 0.0.5
- migrate to oxideav_core::bits shared BitReader / BitWriter
- rewrite README to match real encode + decode capabilities
- add MPEG-2 LSF encoder — 16/22.05/24 kHz via TABLE_LSF
