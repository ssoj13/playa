# playa-io — working notebook

> Local notebook for this crate. Keep current: what / why / how / gotchas / TODO.
> Authorship of the workflow conventions: Alex Joss <joss13@gmail.com>.

## What
Unified media I/O for playa: header probe + raster decode for EXR, video
(FFmpeg), and generic images. Feature-gated: `exr` (vfx-io/vfx-core), `ffmpeg`
(playa-ffmpeg), `webcodecs` (wasm scaffold).

## Key entry points (`src/dispatch.rs`)
- `header_attrs(path) -> Vec<(String, AttrKv)>` — cheap header probe, no pixels.
- `decode_raster(path) -> DecodedRaster` — full pixel decode.
- `AttrKv` — the engine-bridge attribute carrier (see below).

## Metadata absorption pipeline (the important subsystem)
Flow: EXR file → `header_exr` → `Vec<(String, AttrKv)>` → (engine) `attrs_from_io`
→ node `Attrs` → attribute editor + project persistence; on encode the reverse
bridge writes them back.

- `header_exr` (dispatch.rs) sources from `vfx_io::exr::read_layers_passthrough`
  — the **exhaustive TYPED** attribute source (`spec.attributes`). `read()` only
  gives a stringified summary; do NOT use it for round-trip.
- It emits unprefixed **derived** convenience keys (width/height/format/
  compression/channels/layers) for the UI, PLUS the **full** authored attribute
  set namespaced `exr:<name>` (part 0) / `exr:<layer>:<name>` (parts > 0). Nothing
  the file carries is dropped.
- `kv_from_core` maps `vfx_core::AttrValue` 1:1 → `AttrKv`.

## AttrKv (dispatch.rs)
`Str / UInt / Float / Int64 / IntArray / FloatArray / Matrix3([f32;9]) /
Matrix4([f32;16])`. Mirrors `vfx_core::AttrValue` so the whole EXR header
round-trips losslessly. `Float` is f32 here (EXR floats are f32 on disk →
bit-exact). `Int64` exists because EXR `TimeCode`/`KeyCode` pack u32 flag bits
that overflow i32.

## GOTCHA — three different `AttrValue` types
1. `vfx_core::AttrValue` (7 variants) = `ImageSpec.attributes`; re-exported by
   `playa_io::exr_layered::AttrValue`. **This is the EXR header type.**
2. `vfx_io::attrs::AttrValue` (14 EXIF variants) = `Metadata.attrs`. Different.
3. `playa_engine::entities::AttrValue` (15 variants) = engine node attrs.
Never confuse them; the encode path bridges (1)↔(3) explicitly.

## Build
This crate alone (no ffmpeg): `cargo check -p playa-io --no-default-features
--features exr` (fast). Full workspace needs the ffmpeg toolchain — build via
`python bootstrap.py b --debug` (→ `cargo xtask build`), which sets the
manifest-mode `VCPKG_ROOT` (`.vcpkg`, triplet `x64-windows-static-md-release`) +
MSVC + pkg-config. A bare `cargo check -p playa-engine` fails: ffmpeg-sys-next
can't find ffmpeg without that env.

## Video + generic metadata absorption (B1/B2)
- `header_video` now also emits, when present: `video:codec`, `video:bitrate`,
  `video:pix_fmt`, `video:color_space/primaries/transfer/range`, container tags
  `format:<tag>` (creation_time/encoder/title/…) and stream tags `video:tag:<k>`.
  Source: extended `VideoMetadata` (ffmpeg_imp.rs) via `ictx.metadata()`,
  `stream.metadata()`, decoder `codec()/bit_rate()/format()` + `color::*::name()`
  (returns None for unspecified → key skipped). Stub mirrors the fields so the
  shared dispatcher compiles without ffmpeg.
- `header_generic` reads EXIF via `kamadak-exif` (lib `exif`): primary-IFD fields
  as `exif:<TagName>` = display value. Best-effort: no EXIF / parse error → no
  keys, never fails the probe.

## TODO
- [x] E5+.7: absorb full video/generic metadata (done — B1/B2 above).
