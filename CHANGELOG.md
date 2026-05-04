# Changelog

All notable changes to playa are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] — `dev` branch

### GPU compositing — worker → UI bridge (`playa-engine`, `playa-app`)

Workers have no bound OpenGL context; when project prefs select the **Gpu** compositor, the **final**
`CompositorType::blend_with_dim` for `CompNode` must run where GL is current.

- **`GpuBlendBridge` / `GpuBlendReport`:** `GpuBlendBridge::delegate_blend_blocking` enqueues the
  stacked layer `Vec`; the host drains with `GpuBlendBridge::drain_into_compositor`.
  **`NotQueued`** returns the untouched `Vec` if the Ui receiver dropped — worker falls back to the
  thread-local `CpuCompositor` **without** preemptive cloning. **`ReplyDisconnected`** and
  **`Completed(None)`** are terminal outcomes for that hand-off (details in module rustdocs).
- **`CompNode::compose_internal`:** Gpu offload when `ComputeContext.gpu_blend_bridge` is `Some`;
  blocking **`get_frame`** keeps `gpu_blend_bridge: None`; nested preload contexts omit the bridge.
- **`PlayaApp`:** serde skips runtime handles — **`ensure_gpu_blend_initialized`** rebuilds sender/receiver after load;
  **`drain_gpu_blend_queue`** runs every frame **after** **`update_compositor_backend`**.
- **`CompNode::signal_preload`:** passes a bridge ref only when `project.compositor` is Gpu-backed.
- **Rustdocs:** updated `crates/playa-engine/src/entities/compositor.rs` and `gpu_compositor.rs` headers
  so they match the bridged Gpu path (removed stale “viewport-only / compose never Gpu” wording);
  `gpu_blend_bridge.rs` explains ownership; **`gpu_compositor.rs`** drops obsolete integration / compile-toggle scaffolding from the header.

Verify: **`python bootstrap.py build`** (project bootstrap).

### EXR Phase 1 — extended compression UI (commit `71ce94a`)

- `ExrCompression` extended from 4 to **12 variants** matching the full vfx-exr set:
  `None`, `Rle`, `Zips`, `Zip`, `Piz`, `Pxr24`, `B44`, `B44a`, `Dwaa`, `Dwab`,
  `HtJ2k32`, `HtJ2k256`. All wired through `write_exr_frame`.
- Added `dwa_quality: f32` (default 45.0 per OpenEXR convention) to `ExrSequenceSettings`.
- Encode dialog gains a **"DWA loss level"** slider (`0..=200`) shown only for
  DWAA/DWAB. Tooltip clarifies inverse semantics: lower = less loss, 45 =
  visually lossless. NOT the usual "quality 0-100".
- Removed dead `ExrBitDepth` enum + field (was never read; the global
  `OutputBitDepth` filtered to F16/F32 by `FormatCapabilities` is the actual
  source of truth).
- `vfx-exr` git dep gains `features = ["htj2k"]` so the HTJ2K codec is wired
  into the encode path.

### Build infra (commits `e249b2f`, `f481c98`)

- Switched `vfx-exr` from a path-resolved git submodule to a remote git
  dependency. Removed the `crates/vfx-rs` submodule.
- Added `[net] git-fetch-with-cli = true` so SSH auth flows through the
  system git (vendored libgit2 doesn't pick up the user's SSH agent on
  Windows).
- Added a `[patch."ssh://git@github.com/ssoj13/vfx-rs.git"]` block redirecting
  vfx-exr/vfx-io/vfx-core to the local `../vfx-rs` working copy during the
  in-flight Phase A refactor.
- Switched both vfx-exr and vfx-io pins from `rev = "..."` to `branch = "main"`.

### Phase H — vfx-io integration (commits `f481c98`, `600db77`, `b6d4ca2`, `ce38dd3`)

End-to-end OIIO-aligned EXR pipeline. Uses the new `vfx-io` crate (see
`vfx-rs/CHANGELOG.md` for the upstream Phase A milestones that made this
possible).

**Loader (`entities/loader.rs::header_exr`):**
- Reads EXR metadata via `vfx_exr::meta::MetaData::read_from_buffered` (one
  syscall, no pixel decode) and reports OIIO-aligned attrs:
  - `format` — `"EXR (piz)"` / `"EXR (dwaa:45)"` / `"EXR (htj2k:32)"` etc.
    via `vfx_io::exr::compression_str::format`.
  - `compression` — separate machine-readable OIIO-style string attr.
  - `channels` — count.
  - `channel_names` — comma-joined `R,G,B,A,Z,…`.
  - `layers` — total layer count.
  - `layer_names` — comma-joined names (only when > 1 layer).

**Encode dispatcher (`dialogs/encode/encode.rs`):**
- `write_exr_frame` rewritten to use `vfx_io::exr::write_layers`. Builds a
  single-layer `LayeredImage` with per-layer compression carried in
  `spec.attributes["compression"]` (OIIO string). The vfx-io writer reads
  this back per-layer — same path the future multi-layer encode will use.
- New `ExrCompression::to_oiio_string(dwa_quality)` formats the playa enum
  as the OIIO-canonical string ready to drop into `spec.attributes`.
- New `ExrEncodeMode { DisplayOnly, PassThrough }` enum on
  `ExrSequenceSettings`.
- New `write_exr_pass_through(project, frame_idx, dest)` — locates the first
  EXR `FileNode` in the project, reads the source via `vfx_io::exr::read_layers`
  (every layer with full `spec.attributes`), writes via
  `vfx_io::exr::write_layers` preserving every layer + per-layer compression /
  channelformats / custom EXR attrs. Falls back to `write_exr_frame` when no
  EXR source is found.
- `FileNode::resolve_frame_path` made public so the encode pass-through path
  can derive per-frame source paths without going through the compositor.

**SourceImage (`entities/source_image.rs`):**
- New thin wrapper around `vfx_io::LayeredImage`. `open_exr(path)` reads via
  `vfx_io::ExrReader` so every layer carries the OIIO-aligned spec.
- `pick_display_layer` heuristic — prefer canonical layer names
  (`""`, `"rgba"`, `"beauty"`); fall back to largest by pixel area.
- Convenience accessors: `layer_count()`, `layer_names()`,
  `layer_compressions()` (per-layer OIIO strings).

**Encode dialog UI (`dialogs/encode/encode_ui.rs`):**
- Mode combobox (Display only vs Pass-through) above the compression picker.
- Compression / DWA-quality controls disabled in Pass-through mode (source
  per-layer compression wins).
- New `render_exr_source_layer_info` panel — opens the first source EXR via
  `SourceImage` and shows layer count + per-layer name + compression string,
  with a `▶` marker on the auto-picked display layer.

**Build:** verified via `python bootstrap.py build` — release builds clean
in ~36s on the project's MSVC + vcpkg toolchain.

---

### Phase E in playa — byte-exact pass-through (commit `7cbc20c`)

`write_exr_pass_through` switched from decoded read+write
(`vfx_io::exr::read_layers` + `write_layers`) to byte-exact
(`read_layers_passthrough` + `write_layers_passthrough`) — uses the new
chunk-pass-through pipeline added upstream in vfx-rs commit `dfd56d1`.

The source EXR's `compressed_block` bytes (DWAA / DWAB / B44 / HTJ2K /
PIZ / ZIP / ...) flow through unchanged — no decompress + recompress,
so lossy formats keep their exact source quality. Custom EXR header
attrs (chromaticities, timecode, owner, capture_date, worldToCamera,
...) preserved verbatim because the source Header is fed back into
the chunk writer.

Verified via bootstrap (40.6s release build).

---

## Pending follow-ups

See `vfx-rs/TODO2.md` for the full multi-repo plan. Playa-specific items
still open:

- **Pass-through + overrides** — third encode mode where modified layers
  re-encode with the chosen compression and untouched layers stay byte-exact
  (mixed-write support pending in vfx-rs).
- **Layer picker widget** in the Project panel — switch which layer feeds
  the viewport `Frame` for multi-layer EXR sources.
- **`SourceImage` runtime cache on `FileNode`** — currently re-opened per
  dialog interaction and per encode frame.
- **Display-only mode preserves per-channel mixed types** — currently
  RGB+Z+ID gets flattened to RGBA by the compositor before encode.
- **Replace `image` crate with vfx-io** for PNG/JPEG/TIFF/HEIF on the loader
  side — single I/O stack across all formats.
