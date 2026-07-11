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

## Joint-stereo / dual-channel / CRC cells

No available black-box encoder emits Layer II `joint_stereo`
(§2.4.1.6 intensity stereo), `dual_channel`, or CRC-protected frames —
the ffmpeg `mp2`/`mp2fixed` encoders support mono/stereo only. Those
streams are therefore produced by **this crate's own encoder** through
its public batch API and reference-decoded by the *independent*
black-box decoders. That still breaks encoder/decoder symmetry: a
shared misreading of the §2.4.1.6 intensity wire syntax or the
§2.4.1.4 CRC coverage would diverge against the reference. The
premises (live intensity bound, per-frame CRC, mode) are pinned
bitstream-side by `r411_js_dual_crc_fixture_premises_hold`.

```sh
# encode (same multi-tone; see the example source for the exact cells):
cargo run --example gen_conformance_fixtures -- tests/fixtures

# reference decode, same as the black-box-encoded cells:
ffmpeg -y -c:a mp2float -i <stem>.mp2 -c:a pcm_f32le <stem>.ref.wav
```

| stem            | rate  | bitrate | mode          | note                                    |
| --------------- | ----- | ------- | ------------- | --------------------------------------- |
| js_b4_44k_128   | 44100 | 128k    | joint bound4  | B.2a, live intensity, padding           |
| js_b8_48k_192   | 48000 | 192k    | joint bound8  | B.2a, live intensity                    |
| js_b12_32k_192  | 32000 | 192k    | joint bound12 | B.2b, live intensity                    |
| js_b16_44k_256  | 44100 | 256k    | joint bound16 | B.2b, live intensity                    |
| js_b4_32k_64    | 32000 | 64k     | joint bound4  | B.2d narrow, live intensity             |
| js_b8_48k_96    | 48000 | 96k     | joint bound8  | B.2c: bound clamps to sblimit 8 (empty) |
| js_b4_22k_64    | 22050 | 64k     | joint bound4  | LSF Table B.1, live intensity, padding  |
| dual_44k_128    | 44100 | 128k    | dual_channel  | two independent programmes              |
| dual_24k_64     | 24000 | 64k     | dual_channel  | LSF                                     |
| crc_48k_192     | 48000 | 192k    | stereo        | §2.4.1.4 CRC-16 in every frame          |

Results, same three-way comparison as above (both independent decoders
**accept every stream** — mpg123 verifies the CRC cell's checksum —
and align at offset 0):

- float domain vs reference: **max ≤ 0.024 LSB** across all ten cells;
- s16 projection vs reference: 99.54 % … 99.80 % bit-exact, max ±1;
- s16 vs mpg123: 99.89 % … 99.96 % bit-exact, max ±1.

This closes the long-standing gap noted in the crate README: a stream
with a **live intensity-stereo bound** is now decoded against an
independent reference (previously only covered by round-trip and
adversarial-payload fuzz).

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
6d97647eb69222b02943fec7fbacc94acf978010ebca5d7847737e9580074b5c  js_b4_44k_128.mp2
93fef5c73e645e18fa6cad27148bddc504534a8a57220c56315675e7307822cb  js_b8_48k_192.mp2
4f965ff74d073ba061add544f4f9150d89dabd935fa521ef64d21bbcee499342  js_b12_32k_192.mp2
93def1797bbd7eba1259286e69c5ad0ea0ab5ab6959d8215999e69a86aeeaca9  js_b16_44k_256.mp2
ecb555a9e17501af3fa46092f653388f91d11c757b8c57b1393aab23f28dc926  js_b4_32k_64.mp2
ef12343330df8d9b5d57536203bc55b8ad2e6b9a9ff9db5139ef56cf48e8d329  js_b8_48k_96.mp2
0d966ca48128d1e5bbbf35e5c8bae96f6057b37991f7d8fa6f116d214c26ba46  js_b4_22k_64.mp2
51a26fa8526ff69849a939dd360cc06153cf9b005e89a2ffb15d89cda4d9b0cf  dual_44k_128.mp2
24af5fcfbfd1c425773f6b5acbc92b0e84f816ef838fcefbd9b0549e44818756  dual_24k_64.mp2
e839971e25b2810bea8fc4db871955a573a7560308ed76284cf10a4538eebefc  crc_48k_192.mp2
5619696d00ef41425b14195961dc34a2bf0779715a353c2eb5666b4e60ba49aa  js_b4_44k_128.ref.wav
91ef1a36b7ee4be8d0a9e0d4e42812556a4cbb8419ba49dc1f72ff24f6509c6e  js_b8_48k_192.ref.wav
da07b8697ff0d7c8852cdd6483a1d52a73b5e249187eac88e330756a230d7185  js_b12_32k_192.ref.wav
cc020061cccfe38cda302bbf4e921e66d5991e0b2decf871083cde15ec945514  js_b16_44k_256.ref.wav
5e210b54267c00ada137e9154a16de831c198d758bfd8860a2b98e5c648b6fb9  js_b4_32k_64.ref.wav
85a0736a34182fe512746ec49fa193be276d98d7e8a4e64e967bf0cb5d3d9739  js_b8_48k_96.ref.wav
9f045168df4ed341d1cc518e4dcc4858684771fefad5bea7ac2b3937cf322eed  js_b4_22k_64.ref.wav
c0a6fe24349d1ab04e2d18d0ef95e64c8aff57f7f9b59edf3db56429864751b1  dual_44k_128.ref.wav
68304ddd85e16f22a0cbaa4ef279dd50250850bc7e3d61e873691ab4a6f0600b  dual_24k_64.ref.wav
ce40cc9679965b7f85bf8edaf86e32915238a9c5f3bd64722aed4746a07d61f8  crc_48k_192.ref.wav
```
