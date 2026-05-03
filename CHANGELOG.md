# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
