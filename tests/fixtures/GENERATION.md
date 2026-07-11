# r411 conformance-corpus generation notes

Every fixture below was produced **exclusively by black-box binary
invocations** — the workspace clean-room policy permits running validator
binaries as opaque input→output processes; their source is never read.
Naming those binaries here (fixture-generation notes) is the one place
the policy allows it.

Tools (versions at generation time, 2026-07-11):

- `ffmpeg 8.1` — encoders `mp2` (float) and `mp2fixed` (fixed-point),
  decoder `mp2float`, all invoked as opaque processes.
- `mpg123 1.33.4` — second, independent reference decoder used for the
  latitude study below (its decodes are not committed).

## Commands

Each cell encodes 0.6 s of a rate-relative multi-tone with slow
amplitude modulation (so scalefactors and scfsi patterns vary across
frames):

```sh
# channel 0 (mono uses only this):
E0="(0.6+0.4*sin(2*PI*3*t))*(0.32*sin(2*PI*0.011*R*t)+0.22*sin(2*PI*0.07*R*t)+0.18*sin(2*PI*0.19*R*t)+0.12*sin(2*PI*0.36*R*t))"
# channel 1 (stereo cells):
E1="(0.7+0.3*sin(2*PI*5*t))*(0.30*sin(2*PI*0.017*R*t)+0.24*sin(2*PI*0.09*R*t)+0.16*sin(2*PI*0.23*R*t)+0.10*sin(2*PI*0.41*R*t))"

# encode (R = sample rate, BR = bitrate, CH = 1|2, ENC = mp2|mp2fixed):
ffmpeg -y -f lavfi -i "aevalsrc=exprs='$E0[|$E1]':s=R:d=0.6" \
       -c:a $ENC -b:a ${BR}k -ac $CH <stem>.mp2

# reference decode — float PCM, captured before any integer conversion:
ffmpeg -y -c:a mp2float -i <stem>.mp2 -c:a pcm_f32le <stem>.ref.wav
```

| stem            | rate    | ch | bitrate | encoder  | 3-B.2 sub-table (per-ch kbit/s)  |
| --------------- | ------- | -- | ------- | -------- | -------------------------------- |
| mono_44k_32     | 44100   | 1  | 32k     | mp2      | B.2c (32) — MPEG-1 ladder floor  |
| stereo_44k_64   | 44100   | 2  | 64k     | mp2fixed | B.2c (32)                        |
| stereo_44k_128  | 44100   | 2  | 128k    | mp2      | B.2a (64)                        |
| stereo_44k_256  | 44100   | 2  | 256k    | mp2      | B.2b (128)                       |
| mono_48k_48     | 48000   | 1  | 48k     | mp2fixed | B.2c (48)                        |
| mono_48k_56     | 48000   | 1  | 56k     | mp2      | B.2a (56)                        |
| stereo_48k_96   | 48000   | 2  | 96k     | mp2      | B.2c (48)                        |
| stereo_48k_384  | 48000   | 2  | 384k    | mp2      | B.2a (192) — ladder top          |
| mono_32k_48     | 32000   | 1  | 48k     | mp2      | B.2d (48)                        |
| mono_32k_56     | 32000   | 1  | 56k     | mp2fixed | B.2a (56)                        |
| stereo_32k_64   | 32000   | 2  | 64k     | mp2      | B.2d (32)                        |
| stereo_32k_224  | 32000   | 2  | 224k    | mp2      | B.2b (112)                       |
| mono_16k_8      | 16000   | 1  | 8k      | mp2      | LSF Table B.1 — LSF ladder floor |
| stereo_22k_96   | 22050   | 2  | 96k     | mp2fixed | LSF Table B.1 — padding-heavy    |
| mono_24k_144    | 24000   | 1  | 144k    | mp2      | LSF Table B.1 — LSF-only index   |
| stereo_24k_160  | 24000   | 2  | 160k    | mp2      | LSF Table B.1 — LSF ladder top   |

Padding coverage (§2.4.2.3 fractional rates): the 44.1 kHz cells carry
11–22 padded frames of 23, `stereo_22k_96` 11 of 12. No cell sets the
CRC (`protection_bit = 1`); CRC decode is pinned by
`tests/layer2_pcm_conformance.rs` and the crate's own encode path.

## Why the references are float WAVs

The first corpus cut stored the reference decoder's **s16** output and
hit a puzzle: mono cells matched our decode ≥ 99.6 % bit-exact, but
every stereo cell showed exactly-half the samples off by exactly +1 —
a uniform half-LSB reference offset. Decoding the same streams to raw
f32 showed the two decoders' *float* outputs agree to ≤ 0.025 LSB
everywhere, so the offset lived entirely in the pipeline's black-box
float→s16 conversion step (which evidently rounds differently for the
packed-stereo path), not in Layer II decoding. Capturing the reference
**before** integer conversion (`pcm_f32le`) removes that confound and
tightens the assertable bound by a factor of ~40:

- float-domain agreement, all 16 cells: **max |ours − ref| ≤ 0.025 LSB**
  (bound asserted at 0.05), i.e. ≈ 7·10⁻⁷ of full scale — the reference
  computes its §2.4.3.2 filterbank in f32 (24-bit mantissa), so this is
  its own precision floor; our chain is f64 end-to-end.
- s16 projection (both sides through the same round-to-nearest ±clamp
  map): ≥ 99.5 % bit-exact per cell, residual differences all ±1 and
  confined to samples whose float value sits within the 0.025-LSB wobble
  of a rounding boundary.

## Reference-vs-reference latitude study

ISO/IEC 11172-4 defines Layer II decoder conformance as a *bounded
difference signal*, because §2.4.3.2 / §2.4.3.3.5 prescribe no
accumulation order for the float filterbank. To pin the residual ±1
flips on that latitude (and not on this crate), each stream was also
decoded with a second, independent black-box decoder (`mpg123 -w`,
s16 output, sample-aligned at offset 0 on all 16 cells) and all three
outputs compared pairwise in the s16 domain:

| pair                          | bit-exact ratio (range over 16 cells) | max abs |
| ----------------------------- | ------------------------------------- | ------- |
| ours vs reference (f32→s16)   | 99.56 % … 99.86 %                     | 1 LSB   |
| mpg123 vs reference (f32→s16) | 99.54 % … 99.86 %                     | 1 LSB   |
| ours vs mpg123                | 99.89 % … **100.00 %** (`mono_16k_8`) | 1 LSB   |

The two independent reference decoders disagree with *each other* at
the same magnitude as either disagrees with us — and we agree with
mpg123 (whose synthesis runs at higher precision than f32) strictly
better than the references agree between themselves, reaching full
bit-exactness on `mono_16k_8`. That is the documented rounding
latitude; there is no decode-chain divergence left to root-cause.

## SHA-256

```
101aed06930b1d96aa7dd8b009c75f139eda91c64f41e04716282ec63b831877  mono_44k_32.mp2
9d42390d5dfd0eb3be451049b1f7d085111fc48308fc531fdb63d17b1a67a8f7  stereo_44k_64.mp2
82a2396b890dff6e9035c1db7059910ba526f9531617605f59a3c1c573e2f09b  stereo_44k_128.mp2
22f4aa0854516b95ebe61691eb8615678d18d58880d492bf4a65267edb12a619  stereo_44k_256.mp2
4ff887ae286c1d655e27971f4325b8c10d177bfd95fa8bbbd5b9b3bfe55876cd  mono_48k_48.mp2
9f468bbc1d6b17da085c815a2932a28e10852abdadffc465fe6cdf139d883ef3  mono_48k_56.mp2
657d4a929b9851c67baa205c0b040d608821b9a85bea6706d48b4a9a2a765c6c  stereo_48k_96.mp2
307196ab6325b1749f9ae46b84a0366af54a14a2845f051aaf80eeebdea794d7  stereo_48k_384.mp2
abea329f51bc11ba0d81e1b34d36ceeb245622525e3dab633ab14517f81a5406  mono_32k_48.mp2
b7ef6fd175311f83d5fd3e0640c033949f4e8897d1818a0edcf0ec95c51fa86c  mono_32k_56.mp2
b08bed022558daf92100c6bfced311e4fd8ff3e82f68f28f5755fba1ffd811f6  stereo_32k_64.mp2
53db83e6067ecd7931f76abe902cf91efced266c9d3dc3801574f6f5896f86bc  stereo_32k_224.mp2
df0c8d620cd9d2f76a2db1bffe04105b8a8064da4360a0931cefc9649e9f5d80  mono_16k_8.mp2
dea4e32630997aa68daeea0e882650b2beccaf60728be8f01de87767ab4000ad  stereo_22k_96.mp2
bad8651df8b44207d13b1d7ae295cb3802e325b3460298c4b27f182c0d63415c  mono_24k_144.mp2
385635431ecfac6019c2b9972db9e3c1ecd88ef1eb952c3eef054b2269d68c08  stereo_24k_160.mp2
cfcf3e4cfc1f2921b09e85f9385cb4eb8ba57937dc4918ce20bc5cf3475cdbea  mono_44k_32.ref.wav
96eb96b5011477058ae5d3b157b5c925b32e53c0dc460a94eb56063adeb33fc8  stereo_44k_64.ref.wav
030c270f74f2e068c5ca3feba5502d95fc37743dc11268ae77ebaba8737fb6f2  stereo_44k_128.ref.wav
2ae1e84c855ae9664daed8a1536b06f17f37c8a3f53fdb1bbee7d04078a822e4  stereo_44k_256.ref.wav
ae9d1e4d8e3bce4916b119a8201729542657b8a1ef4696ff5dfe50642f5a3543  mono_48k_48.ref.wav
f4a0f72c48d8a26f87edd645b8ba804f1b23763cd52974764c3e45cad0523955  mono_48k_56.ref.wav
50e7a2204700cfe77e3ca7af844599e6dc774d34f58c8c0f54b0b72ff40220ad  stereo_48k_96.ref.wav
603bfad1d704c6442ee7a99c89253b1d65ec4162cac9b29a326f50674a40a2ce  stereo_48k_384.ref.wav
135ba61b28b8a57770e8c00d7d887676a3e023eb6758e6487c99c6bfec55a97c  mono_32k_48.ref.wav
d380e3cd7c1c0e45709c3e18ab8a54286e3e15379f0319ef6792f0eb7e1f1a85  mono_32k_56.ref.wav
332c6c2c38623d3ef87b47a4f1d89250c86f82740040e5fbc0fb2d161d03fd73  stereo_32k_64.ref.wav
f2831894c5a775beb60e89dea6e398039e52d90af7d37d2e9c90f6218cfe1342  stereo_32k_224.ref.wav
fc36421d0992c09bfdc3f7f9bef991bab7c200add9f575be2c5f0552574cc5b4  mono_16k_8.ref.wav
3905035625db8d3a296d1d82794b61b12b762a79c9648f196a681e7ca64c88cf  stereo_22k_96.ref.wav
e4b23980b9f1c983c3649f284e17b54565499332f8d29225baa2e7814059ecd7  mono_24k_144.ref.wav
1e0e77d42425a1ba2edff1d80dc45194df35253a112dec11ae12e6bf875222f7  stereo_24k_160.ref.wav
```
