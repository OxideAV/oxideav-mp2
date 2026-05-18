# oxideav-mp2

Pure-Rust **MPEG Audio Layer II** (MP2 / MUSICAM) codec — decode + encode
of MPEG-1 (ISO/IEC 11172-3) and MPEG-2 LSF (ISO/IEC 13818-3 §2.4)
elementary streams. Zero C dependencies.

Part of the [oxideav](https://github.com/OxideAV/oxideav-workspace)
framework but usable standalone.

## Installation

```toml
[dependencies]
oxideav-core = "0.1"
oxideav-codec = "0.1"
oxideav-mp2 = "0.0"
```

## Decoder

Accepts all Layer II combinations permitted by the spec: MPEG-1 at
32 / 44.1 / 48 kHz, MPEG-2 LSF at 16 / 22.05 / 24 kHz, every channel
mode (mono / stereo / joint-stereo / dual-channel), every bitrate on
each version's ladder. Frames carrying a CRC-16 are accepted (the two
bytes after the header are consumed but the CRC is not verified).
Output frames are interleaved `SampleFormat::S16` at 1152 samples per
channel.

```rust
use oxideav_core::{CodecId, CodecParameters, Frame, Packet, RuntimeContext, TimeBase};

let mut ctx = RuntimeContext::new();
oxideav_mp2::register(&mut ctx);

let params = CodecParameters::audio(CodecId::new("mp2"));
let mut dec = ctx.codecs.make_decoder(&params)?;

// Slice one Layer II frame out of the elementary stream (use
// `oxideav_mp2::header::parse_header` to get `frame_length()`).
let pkt = Packet::new(0, TimeBase::new(1, 48_000), frame_bytes.to_vec());
dec.send_packet(&pkt)?;
if let Ok(Frame::Audio(a)) = dec.receive_frame() {
    // a.format == SampleFormat::S16, a.samples == 1152
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

MPEG-2.5 (8 / 11.025 / 12 kHz) is outside the Layer II spec and is
rejected at sync-check time.

## Encoder

Layer II encoder covering MPEG-1 (32 / 44.1 / 48 kHz) and MPEG-2 LSF
(16 / 22.05 / 24 kHz). Emits mono, plain stereo, or joint stereo
(intensity); CRC-16 and free-format are not produced. Both **CBR**
and **VBR** are supported via the [`oxideav_mp2::options`] schema
(see below). Bitrate comes from `params.bit_rate` (CBR slot) or the
`vbr_quality` knob; it must land on the standard ladder for the
chosen version — MPEG-1 32..=384 kbps (subject to Table 3-B.2 mode
restrictions in CBR mode), MPEG-2 LSF 8..=160 kbps (all permitted in
any mode). Input must be interleaved `SampleFormat::S16`.

### Encoder options

Three switches are exposed via `CodecParameters::options`:

- `vbr_quality` (`u32`, 0..=9): switch the encoder to **VBR**. Each
  frame's bitrate slot is picked independently from the standard
  ladder. The encoder also prepends a Xing/Info header frame on
  flush, so downstream tools (ffmpeg, mediainfo) can show an accurate
  average bitrate.
- `joint_stereo` (`bool`): enable Layer II **intensity stereo** in
  stereo inputs. The encoder picks the smallest header-encodable
  bound (4 / 8 / 12 / 16) at which all upper subbands are L/R
  correlated enough to share spectral coefficients (per-subband
  correlation threshold relaxes 5–15% for the upper subbands where
  spatial hearing is less acute).
- `psy_model` (`string`, default `"ath"`): psychoacoustic bias for
  the bit allocator. `"ath"` enables an Absolute-Threshold-of-Hearing
  weighting (Terhardt analytic curve) that attenuates the score of
  subbands sitting outside the audible range, redirecting bits to
  audible mid-band content. `"none"` reproduces the strict-energy
  allocator from v0.0.8 for byte-exact reproducibility.

The options compose freely (CBR + joint stereo + ATH, VBR +
joint stereo, etc.).

```rust
use oxideav_core::{CodecId, CodecParameters, Frame, RuntimeContext, SampleFormat};

let mut ctx = RuntimeContext::new();
oxideav_mp2::register(&mut ctx);

let mut params = CodecParameters::audio(CodecId::new("mp2"));
params.channels = Some(2);
params.sample_rate = Some(48_000);
params.sample_format = Some(SampleFormat::S16);
params.bit_rate = Some(192_000);
let mut enc = ctx.codecs.make_encoder(&params)?;

enc.send_frame(&Frame::Audio(pcm_frame))?;
while let Ok(pkt) = enc.receive_packet() {
    // one Layer II frame per packet, 1152 samples / channel
}
enc.flush()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Bit allocation is a greedy scheme: subbands are awarded quantiser
upgrades in decreasing order of perceptually-weighted-energy-per-bit
until the frame budget is exhausted. The default `psy_model="ath"`
multiplies the raw subband energy by `weight²` where `weight ∈
(0, 1]` follows the inverse Terhardt absolute-threshold curve — bands
at the ear's most-sensitive frequencies (~3 kHz) score unattenuated,
bands at deep sub-bass or near Nyquist drop by 20–40 dB. In VBR mode
the allocator additionally stops when the energy/cost ratio of the
next-best upgrade drops below a quality-derived threshold; the smallest
standard ladder slot (filtered by the Table 3-B.2 mode restrictions
on MPEG-1 — 32/48 kbps in stereo, 224+ kbps in mono are skipped)
whose payload budget admits the chosen allocation is then written
into the header. Scalefactors are extracted from per-part subband
peaks, and SCFSI is picked so the transmitted triple exactly
represents the three scalefactors when possible. Output bitstreams
are raw elementary Layer II frames (no container, no CRC).

## Codec ID

- Codec: `"mp2"`; accepted sample format `S16`.

## License

MIT — see [LICENSE](LICENSE).
