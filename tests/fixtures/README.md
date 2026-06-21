# Layer II decode-conformance fixtures

Each `<name>.mp2` is a real MPEG Audio Layer II elementary stream; the
matching `<name>.ref.wav` is its decoded PCM produced by a **black-box
reference decoder** (an opaque encode→decode of a synthetic multi-tone
signal). Only the emitted bytes of that decoder are consumed here — its
source is never read. This is the same black-box-validator arrangement
the workspace clean-room policy permits: the validator is an opaque
process that turns input into output, and we compare our bytes to its
bytes.

The fixtures span the full Layer II channel-mode × sampling-rate matrix
so the §2.4.3 decode chain (header → bit-allocation table → §2.4.3.3.4
requantization → §2.4.3.3.3 scalefactor rescaling → §2.4.3.2 / Annex A
Figure A.2 synthesis filterbank) is exercised under every rate ladder
the ISO syntax permits, not just the single 44.1 kHz stereo stream
under `docs/audio/mp3/fixtures/`:

| fixture                | mode           | rate     | std       |
| ---------------------- | -------------- | -------- | --------- |
| `mono_44k_128`         | single-channel | 44.1 kHz | MPEG-1    |
| `mono_32k_96`          | single-channel | 32 kHz   | MPEG-1    |
| `stereo_48k_192`       | stereo         | 48 kHz   | MPEG-1    |
| `mono_22k_64`          | single-channel | 22.05 kHz| MPEG-2 LSF|
| `stereo_24k_64`        | stereo         | 24 kHz   | MPEG-2 LSF|
| `stereo_16k_64`        | stereo         | 16 kHz   | MPEG-2 LSF|

The reference decoder applies no startup delay on these streams (decoded
sample count == frames × 1152 exactly, alignment offset 0), so the
comparison in `tests/decode_matrix_conformance.rs` is sample-aligned
with no search.

## Why the bound is the ISO envelope, not bit-identity

ISO/IEC 11172-3 §2.4.3.2 / §2.4.3.3.5 define the synthesis filterbank's
64×32 cosine matrixing and 512-tap windowing as floating-point
operations with no prescribed accumulation order or intermediate
fixed-point quantisation. An independent clean-room decoder therefore
cannot reproduce a particular reference decoder's integer PCM
bit-for-bit; conformance in ISO/IEC 11172-4 / 13818-4 is a *bounded*
difference signal (rms below `2^-15 / sqrt(12)`-grade), not bit-identity.
Across this whole matrix our decoder matches the reference to **max abs
≤ 1 LSB** and **rms ≈ 0.5 LSB** — the residual is exactly the ±1
boundary jitter inherent to two independent float implementations, and
any decode-stage regression (wrong allocation class, requant constant,
scalefactor, synthesis window, channel order) would blow straight past
it.
