# Changelog

All notable changes to playa are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] — `dev` branch

### Wave 7-pre Round 3 — auto-attach v2, RevealMp4 OS-level, AppSettings sliced

Five commits (`8139d13` → `f9aa7f7`) closing the remaining app-level
deferred items from `progress.txt`. After this round `AppSettings` is
fully sliced (playback / cache / viewport / timeline / jobs) and the
auto-attach loop is end-to-end functional.

- **`8139d13` US-15 v2 — `auto_attach_mp4` actually imports the mp4 as
  a layer.** v1 logged the resolved path. v2 closes the loop: the
  `JobEvent::Completed` listener pushes the `PathBuf` through an mpsc
  channel; `update()` drains it per-frame and routes through
  `load_sequences` — the same `FileNode::detect_from_paths` import
  path drag-drop uses. Threading: `EventBus` callbacks may fire on any
  worker thread, but `Project` mutators expect the UI thread, so the
  channel decouples them. `auto_attach_tx: Option<Sender<PathBuf>>`
  and `auto_attach_rx: Mutex<Option<Receiver<PathBuf>>>` are both
  `#[serde(skip)] / None` default so post-deserialize re-init
  rebuilds the channel cleanly via the existing
  `auto_attach_subscribed` latch.

- **`6007f45` `JobsAction::RevealMp4` v2 — open containing folder in
  the platform file manager.** Previously log-only with a
  "`until we wire opener`" TODO. Now wired to `opener` 0.7
  (Explorer / Finder / xdg-open). We open the **parent directory**
  rather than the file so the user lands in a folder view (default
  media-player auto-play is rarely what "Reveal" means). True
  select-with-highlight reveal would require shell-out per-OS — out
  of scope for one button. Failure logs the path so it's still
  accessible.

- **`aef666c` US-03b PlaybackSettings — first slice extracted.** New
  `PlaybackSettings { fps_base, loop_enabled, preload_radius,
  preload_delay_ms }` lives in `playa-ui::dialogs::prefs` and is
  embedded in `AppSettings` via `#[serde(flatten)]`. Existing
  `playa.json` saves load unchanged — top-level fields absorb into
  the slice. 18-site consumer migration across 5 files. Three new
  unit tests in `prefs.rs::tests` cover the legacy-JSON →
  slice path, flat serialization round-trip, and default-value
  preservation.

- **`a74ae66` US-03b CacheSettings — second slice extracted.** Same
  pattern: `CacheSettings { cache_memory_percent,
  reserve_system_memory_gb, cache_strategy }`, 12-site migration,
  2 new tests.

- **`f9aa7f7` US-03b ViewportSettings + TimelineSettings — final two
  slices.** `AppSettings` now fully sliced (5 slices: playback /
  cache / viewport / timeline / jobs, plus `encode_dialog`). 21-site
  migration across `tabs.rs` and `prefs.rs`. Three new tests including
  a combined `all_slices_serialize_flat_combined` regression check
  that asserts every legacy top-level key is present **and** none of
  the slice keys (`playback`, `cache`, `viewport`, `timeline`) appear
  in the JSON — catches future drops of `#[serde(flatten)]`.

The slice extraction was originally pre-marked **gold-plating
relative to jobs goal** (per `progress.txt`) — landed pragmatically:
zero JSON shape change, mechanical consumer migration, comprehensive
back-compat tests. `cargo test` of all jobs/prefs crates: 158/158
green; `cargo check -p playa-app` green default +
`--no-default-features`.

### Wave 7-pre Round 2 — pluggable Preferences modal + budget gate + auto-attach hook

Closes the four deferred items from `progress.txt`'s "NEXT SESSION
PICKUP" block. Five commits on `dev` (`975acf1` → `1f775d4`); 158/158
tests across 6 crates green; `cargo check -p playa-app` green default
+ `--no-default-features`. Architect verdict: **APPROVED**.

- **US-12 (`975acf1`) — `AppSettings` `Clone+PartialEq` cascade +
  `PrefsWindow` modal lifecycle.** Adds `Clone+PartialEq` to
  `AppSettings`, `Layout`, and the 13-struct cascade in
  `playa-ui::dialogs::encode` (`EncodeDialogSettings`,
  `H264/H265/ProRes/AV1Settings`, `CodecSettings`,
  `Sequence{,Format}Settings`, `{Exr,Png,Jpeg,Tiff,Tga}SequenceSettings`)
  so `playa_prefs::PrefsWindow<AppSettings>` can keep a working copy
  and detect dirty state for the **Apply** button. `PlayaApp` carries
  `prefs_window: PrefsWindow<AppSettings>`; `update()` calls `show()`
  once per frame. **`Ctrl+,`** opens the modal via direct
  `ctx.input` check (no event-factory hop); the legacy `F12 →
  ToggleSettingsEvent` window keeps working while migration unfolds.
  No top menu bar exists in this app, so an `Edit > Preferences…`
  item was deliberately omitted as gold-plating.

- **US-13 (`18fb2d0`) — `AppSettings.jobs` slice +
  `register_default_prefs`.** `playa-ui` takes a direct path-dep on
  `playa-jobs-core` (default-features off — type is feature-agnostic);
  `AppSettings` gains `pub jobs: JobsSettings` with `#[serde(default)]`
  for backwards-compat with existing `playa.json` saves. `PlayaApp`
  drops the standalone `jobs_settings` field; `render_jobs_tab` reads
  `&mut self.settings.jobs`. `PlayaApp::default` registers the default
  prefs entry under `cfg(feature = "jobs")` via
  `playa_jobs::register_default_prefs(&mut registry, |s| &mut s.jobs)`.
  The "Jobs & Rendering" panel now appears in the Preferences modal.

- **US-14 (`4d429f8`) — daily budget enforcement at
  `JobQueue::submit`.** New `JobProvider::estimate_cost_usd(&Value) ->
  Option<f64>` trait method (default `None`). `SeedanceProvider`
  implements it parsing duration (int for i2v at $0.3024/s, str for
  t2v at $0.3034/s) — same accounting `report_cost_from_params` emits
  post-completion. `JobQueue` gains
  `budget_cap: RwLock<Option<f64>>` + `set_budget_cap()` / `budget_cap()`.
  `submit()` now resolves the provider via `Arc::clone`, computes
  `today_spent + estimated`, and rejects with
  `JobError::Provider("daily budget exceeded ($X.YZ today + $A.BC
  estimated > $D.EF cap)")` when the sum would exceed the cap; the
  rejected job is **never** inserted into the map (no orphan in
  `list()`). `PlayaApp.update()` writes
  `queue.set_budget_cap(if enabled { Some(cap) } else { None })` per
  frame from `settings.jobs.{daily_budget_enabled, daily_budget_usd}`.
  Provider with no estimate impl → counted as $0 against the cap
  (favours submit over false rejection). +3 tests in playa-jobs-core
  (45 → 48), +1 in playa-job-seedance (18 → 19).

- **US-15 (`6fc2fc2`) — `auto_attach_mp4` listener (subscribe to
  `JobEvent::Completed`).** `PlayaApp` registers an EventBus subscriber
  on jobs init (idempotent via an `auto_attach_subscribed` latch). The
  handler reads `auto_attach_enabled` — an `Arc<AtomicBool>` mirror of
  `settings.jobs.auto_attach_mp4` synced from `update()` each frame —
  so toggling the preference takes effect for future jobs without
  re-subscribing. v1 logs `auto-attach: job {id} mp4 ready at {path}`;
  full `Project::add_layer_from_file` integration is intentionally
  deferred to v2 (engine `Project` mutators aren't designed to be
  invoked from arbitrary subscriber threads — clean implementation
  routes via mpsc back to the UI thread). Subscription happens via
  `ensure_jobs_initialized` (`runner.rs:192`) so both fresh-boot and
  post-deserialize paths re-subscribe (the global `event_bus` is
  `#[serde(skip)]`, so all subscribers are lost on reload).

- **`1f775d4` — deslop pass.** Two minor cleanups: stale comment in
  `render_jobs_tab` ("until a full Preferences modal lands" / "Persists
  via `PlayaApp.jobs_settings`") replaced with live behaviour; one
  redundant what-it-does comment dropped above the
  `prefs_window.show` match.

Remaining deferred from `progress.txt`: **US-03b** (Viewport / Cache /
Playback / Hotkey slice extraction with `#[serde(flatten)]` for
back-compat — gold-plating relative to jobs goal; tackle when
independently motivated) and `gsd-extract-learnings` to convert
`progress.txt` + `.bughunt/plan1.md` into wiki entries.

### Pinned vcpkg FFmpeg via manifest mode

`vcpkg.json` + `vcpkg-configuration.json` at the workspace root lock
microsoft/vcpkg to a specific baseline (currently
`4bc07e3eb00c5a9539a5a7a83415150a9260f8db`, 2026-05-07). Install once with
`vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed
--triplet <triplet>` — `xtask::env_setup::try_manifest_mode_vcpkg` then
points `VCPKG_ROOT` at the local `.vcpkg/` so CI and local dev link
bit-identical FFmpeg builds. Falls back to the global `VCPKG_ROOT` until
manifest install is populated. `.vcpkg/` is in `.gitignore`.

### Vendored `playa-ffmpeg` + automatic MSVC/vcpkg env via `xtask`

`crates/playa-ffmpeg/` is now a workspace member instead of a `crates.io` dep
— the FFmpeg Rust wrapper is under our control and can be patched in-tree
when vcpkg ships a breaking FFmpeg point release. Default features no longer
include `device` (avdevice) / `filter` (avfilter) — vcpkg's FFmpeg 8.1+
`avfilter` build pulls in `vsrc_gfxcapture_winrt` (WinRT + C++ `<regex>`)
which causes MSVC STL link mismatches on Windows. Forward-compat wildcard
`_ =>` arms added to all `AVCodecID` / `AVPacketSideDataType` /
`AVFrameSideDataType` / `AVColorPrimaries` / `AVColorTransferCharacteristic`
match blocks so future FFmpeg 8.x point releases compile cleanly.

`crates/xtask` now uses `vcv-rs` (git dep, `https://github.com/ssoj13/vcv-rs`)
to discover the active Visual Studio install + Windows SDK + UCRT and prepend
`INCLUDE` / `LIB` / `LIBPATH` / `PATH` for the forked `cargo build`. It also
sets `VCPKG_ROOT` / `VCPKGRS_TRIPLET` / `PKG_CONFIG_PATH` automatically.
A vanilla `cargo build` from a non-Developer-PowerShell shell will *not*
have that env — always go through `cargo xtask build` (or `python bootstrap.py
build`, which now delegates `--features` and profile flags straight to xtask).

`vfx-rs` deps switched from `ssh://git@github.com/ssoj13/vfx-rs.git` to
`https://github.com/ssoj13/vfx-rs.git`; the dead `[patch."ssh://..."]` block
pointing at a non-existent local `../vfx-rs/` checkout was removed from the
workspace `Cargo.toml`.

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

Verify: **`python bootstrap.py build`** and **`python bootstrap.py test`** (workspace unit tests + members).

- **Tests / CI:** `cargo xtask test` runs **`cargo test --workspace`** so `playa-engine` unit tests execute;
  `GpuBlendBridge` gains `NotQueued` + CPU round-trip coverage. **Windows:** `playa-engine/build.rs` links
  **`vfw32`** for static FFmpeg **avdevice**. **Rustdoc** examples in `playa-engine` use **`playa_engine::`**
  imports so **doctests** compile. **`bootstrap.py test`:** forwards **`--debug`** to xtask with **`-d`**;
  no longer passes invalid **`--release`** to **`xtask test`**; **`--nocapture`** maps to xtask’s **`--nocapture`**.

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
