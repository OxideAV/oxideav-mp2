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
use oxideav_codec::CodecRegistry;
use oxideav_core::{CodecId, CodecParameters, Frame, Packet, TimeBase};

let mut codecs = CodecRegistry::new();
oxideav_mp2::register(&mut codecs);

let params = CodecParameters::audio(CodecId::new("mp2"));
let mut dec = codecs.make_decoder(&params)?;

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

CBR Layer II encoder covering MPEG-1 (32 / 44.1 / 48 kHz) and MPEG-2
LSF (16 / 22.05 / 24 kHz). Emits mono or plain stereo; joint-stereo,
CRC-16, and free-format are not produced. Bitrate comes from
`params.bit_rate` and must land on the standard ladder for the chosen
version — MPEG-1 32..=384 kbps (subject to Table 3-B.2 mode
restrictions), MPEG-2 LSF 8..=160 kbps (all permitted in any mode).
Input must be interleaved `SampleFormat::S16`.

```rust
use oxideav_codec::CodecRegistry;
use oxideav_core::{CodecId, CodecParameters, Frame, SampleFormat};

let mut codecs = CodecRegistry::new();
oxideav_mp2::register(&mut codecs);

let mut params = CodecParameters::audio(CodecId::new("mp2"));
params.channels = Some(2);
params.sample_rate = Some(48_000);
params.sample_format = Some(SampleFormat::S16);
params.bit_rate = Some(192_000);
let mut enc = codecs.make_encoder(&params)?;

enc.send_frame(&Frame::Audio(pcm_frame))?;
while let Ok(pkt) = enc.receive_packet() {
    // one Layer II frame per packet, 1152 samples / channel
}
enc.flush()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Bit allocation is a non-psychoacoustic greedy scheme: subbands are
awarded quantiser upgrades in decreasing order of energy-per-extra-bit
until the frame budget is exhausted. Scalefactors are extracted from
per-part subband peaks, and SCFSI is picked so the transmitted triple
exactly represents the three scalefactors when possible. Output
bitstreams are raw elementary Layer II frames (no container, no CRC).

## Codec ID

- Codec: `"mp2"`; accepted sample format `S16`.

## License

MIT — see [LICENSE](LICENSE).
