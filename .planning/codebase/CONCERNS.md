# Codebase Concerns

**Analysis Date:** 2026-05-09
**Scope:** Playa workspace (`playa-app`, `playa-engine`, `playa-events`, `playa-io`, `playa-ui`, `xtask`); `playa-py` is excluded from the workspace and is analyzed separately at the end.
**Sources:** AGENTS.md ("Surprises and gotchas" table), TODO.md, task2.md, CHANGELOG.md, Cargo.toml, Cargo.lock, grep across all crates.

---

## 1. Known architectural gotchas (from AGENTS.md)

These are documented quirks where the obvious refactor would silently break the app. Restating from `AGENTS.md` lines 602–617:

| # | Gotcha | File | Why it bites |
|---|--------|------|--------------|
| 1 | `event_bus::downcast_event` must be `(**event).as_any()`, not `event.as_any()` | `crates/playa-engine/src/core/event_bus.rs` (referenced AGENTS.md L606) | Blanket `impl<T> Event for T` means `Box<dyn Event>` itself implements `Event` — naive `event.as_any()` resolves on the `Box`, not the inner type, so every downcast silently fails. **Do not "simplify" this.** |
| 2 | `project.set_event_emitter(...)` MUST be re-attached after deserialization | `crates/playa-app/src/runner.rs` | `event_emitter: Option<EventEmitter>` is `#[serde(skip)]`. Without restoring it, `modify_comp` succeeds but emits no `AttrsChangedEvent`, so cache is **never invalidated** — stale frames forever, with no error. |
| 3 | `compose_internal` iterates `layers.iter().rev()` | `crates/playa-engine/src/entities/comp_node.rs` | `layers[0]` is background, `layers[N-1]` is front. Reversing it inverts Z order. |
| 4 | `trim_in` / `trim_out` are **offsets, not absolutes** | layer / file_node attrs | `work_start = _in + trim_in`, `work_end = _out - trim_out`; on Layer they are in **source frames** then scaled by `speed`. Treating them as absolute timeline frames produces nonsense ranges. |
| 5 | `enum_dispatch` method shadowing | `crates/playa-engine/src/entities/node.rs` (NodeKind) | If you write `impl NodeKind { fn fps(&self) … }`, that hand-rolled impl **shadows** the trait dispatch and tests fail silently. Do not duplicate `fps/_in/_out/frame` on `NodeKind`. |
| 6 | Rotation sign: UI is CW+, glam is CCW+ | `crates/playa-engine/src/entities/space.rs` (`to_math_rot` / `from_math_rot`) | Forgetting to invert produces mirrored rotation only on some axes. |
| 7 | LRU must be `lru::LruCache`, not custom IndexSet | `crates/playa-engine/src/core/global_cache.rs` | `IndexSet::shift_remove` is O(n); a 5k-frame LRU degrades dramatically. The crate provides O(1) get/put/pop_lru. |
| 8 | Workers use `std::thread::sleep(1ms)`, no async | `crates/playa-engine/src/core/workers.rs` | Don't introduce Tokio inside worker loops — there is no runtime, futures will never poll. |
| 9 | `THREAD_COMPOSITOR` is `thread_local!` on purpose | `crates/playa-engine/src/entities/comp_node.rs` | One CPU compositor per worker thread. Fallback path when GPU bridge returns `NotQueued`. Do not lift it to a single shared instance — that re-introduces lock contention. |
| 10 | `GpuBlendBridge` / `GpuBlendReport` recovery on disconnect | `crates/playa-engine/src/entities/gpu_blend_bridge.rs` | Workers block in `delegate_blend_blocking` until UI drains; on `SendError` recovery uses `.0.frames` (Rust 1.95+ `std::sync::mpsc` shape). If UI thread skips `drain_gpu_blend_queue`, workers deadlock. |
| 11 | Cpu↔Gpu transform parity gap | `crates/playa-engine/src/entities/compositor.rs:222` (`// TODO for GPU compositing:`) | CPU compositor pre-warps pixels (ignores per-layer matrices); GPU consumes `u_top_transform`. Output differs subtly between backends. Documented as future work. |

---

## 2. Build / dependency risk

### 2.1 Patched private git dependency `vfx-rs` — high risk
`Cargo.toml:76-79`:
```
[patch."ssh://git@github.com/ssoj13/vfx-rs.git"]
vfx-exr  = { path = "../vfx-rs/crates/exr/vfx-exr" }
vfx-io   = { path = "../vfx-rs/crates/oiio/vfx-io" }
vfx-core = { path = "../vfx-rs/crates/foundation/vfx-core" }
```
- This is a **relative-path** override pointing at a sibling working copy that **does not ship with this repo**. A fresh clone of `playa` alone will not build — the user must also clone `ssoj13/vfx-rs` next to it.
- The upstream is a private SSH repo (`ssh://git@github.com/...`); CI without the deploy key cannot resolve the unpatched dep either.
- `Cargo.lock:5640` shows `vfx-exr 1.74.0`, `vfx-io 0.1.0`, `vfx-core 0.1.0`, all sourced from the local path. Bumping them requires manual coordination across two repos (no semver guard).
- `CHANGELOG.md:51-61` notes the switch from submodule to remote git + patch override and the `[net] git-fetch-with-cli = true` workaround for libgit2 not seeing the system SSH agent on Windows. This is fragile.

**Mitigation path:** publish vfx-rs crates (even to a private registry) or vendor them as a submodule again; document the side-by-side checkout requirement in DEVELOP.md.

### 2.2 vcpkg / MSVC requirement (Windows)
- `bootstrap.py` orchestrates `VCPKG_ROOT`, `VCPKGRS_TRIPLET=x64-windows-static-md-release`, then merges MSVC environment via `vcvars64.bat` before invoking `cargo xtask` (AGENTS.md L472-501).
- Static FFmpeg link goes through vcpkg → any vcpkg upgrade can break the build with cryptic linker errors. `crates/playa-engine/build.rs` had to add `vfw32` link flag for static avdevice (CHANGELOG `[Unreleased]` section L31).
- `playa-ffmpeg = "8.0.3"` pinned with `static` feature in workspace deps (`Cargo.toml:30`). Any FFmpeg CVE requires re-bumping this exact version + re-vendoring vcpkg ports.

### 2.3 Rust edition 2024 toolchain pin
- `Cargo.toml:35` declares `edition = "2024"`. Stable rustc with edition 2024 is recent; older toolchains in CI/dev images will fail with confusing parse errors.
- AGENTS.md mentions `Rust 1.95+` for `std::sync::mpsc::SendError` API used in GPU bridge recovery — implicit MSRV is 1.95+, not declared in `Cargo.toml`.

### 2.4 `playa-py` excluded from workspace
- `Cargo.toml:11`: `exclude = ["crates/playa-py"]`.
- `cargo build` and `cargo test` from the root never compile/test playa-py. Easy to drift; signature changes in `playa-engine` public API can break the Python bindings silently.
- TODO.md item #6 ("Python API via RustPython - expose all major classes") implies an even larger surface is planned; the exclusion will become more painful.

---

## 3. Code-level concerns

### 3.1 `unsafe` is small and concentrated — good
Only **3 files** contain `unsafe { … }`, total **8 blocks**:

| File | Blocks | Purpose |
|------|--------|---------|
| `crates/playa-io/src/video/ffmpeg_imp.rs` | 4 | FFmpeg C FFI: `av_log_set_level`, `(*decoder_ctx).thread_type`, `av_rescale_q`, `av_seek_frame` |
| `crates/playa-ui/src/dialogs/encode/encode.rs` | 2 | At lines 1390, 1646 (encode pipeline FFmpeg interop) |
| `crates/xtask/src/env_setup.rs` | 2 | `std::env::set_var` / `remove_var` (Rust 2024 made these `unsafe`) — gated by SAFETY comment "main thread before spawning Cargo" |

No `unsafe` in `playa-engine`, `playa-events`, `playa-app`, or `playa-ui` outside encode. **Healthy.**

### 3.2 `unwrap()` / `expect()` — moderate, mostly in tests
**112 occurrences across 29 files.** Worst offenders:

| File | Count | Notes |
|------|-------|-------|
| `crates/playa-engine/src/entities/frame.rs` | 21 | All `unwrap()` are inside `#[cfg(test)]` modules per inspection of file's bottom half — production code uses `Result<_, FrameError>`. **Acceptable.** |
| `crates/playa-engine/src/entities/project.rs` | 20 | Need audit — none in unwrap form on plain grep, but `expect(` count is non-zero. Likely tests. |
| `crates/playa-app/src/server/api.rs` | 8 | REST handler glue; review for production paths. |
| `crates/playa-engine/src/entities/comp_node.rs` | 4 | Compose hot path — any `unwrap` here is a runtime panic risk. |
| `crates/playa-engine/src/entities/file_node.rs` | 4 | Loader paths; same concern. |
| `crates/playa-ui/src/widgets/viewport/renderer.rs` | 5 | OpenGL state access — could panic on context loss. |
| `crates/playa-ui/src/dialogs/encode/encode.rs` | 9 | Encode pipeline; mix of test + prod. |

**AGENTS.md L540 explicitly forbids `unwrap`/`expect` in production code** apart from tests and `PoisonError` recovery. A targeted audit (grep with `-B 5` to confirm `#[cfg(test)]` context) is warranted but not urgent — count is not catastrophic.

### 3.3 `panic!()` / `unimplemented!()` / `todo!()`
- **No `unimplemented!()` and no `todo!()` macros anywhere.** Excellent.
- `panic!()` appears **7 times**, all annotated as test-only:
  - `frame.rs:1190, 1197, 1204` — test assertions, comment `"Test-only: unreachable in prod"`.
  - `gpu_blend_bridge.rs:177` — test on `NotQueued` shape.
  - `encode.rs:2060, 2063` — test skip when FFmpeg lacks encoders.
- `frame.rs:1182` carries the explicit comment: *"Note: panic!() calls below are TEST-ONLY assertions, not production code."* Good discipline.

### 3.4 Large files (refactor candidates)
| LOC | File | Concern |
|-----|------|---------|
| 3170 | `crates/playa-ui/src/dialogs/encode/encode.rs` | Encode dialog has grown to a god-module: format settings, FFmpeg dispatcher, EXR pass-through, TIFF, video, image variants, plus all `unsafe` outside `playa-io`. Split by codec family. |
| 1840 | `crates/playa-engine/src/entities/comp_node.rs` | The compose hot path; understandable, but `compose_internal`, GPU bridge plumbing, and preload signaling could split. |
| 1652 | `crates/playa-ui/src/widgets/timeline/timeline_ui.rs` | Single timeline UI module. |
| 1365 | `crates/playa-ui/src/dialogs/encode/encode_ui.rs` | Mirror of `encode.rs`; same refactor bag. |
| 1364 | `crates/playa-app/src/main_events.rs` | Central `handle_app_event` switch. AGENTS.md L160 says all event-driven mutations land here — natural growth. |
| 1223 | `crates/playa-engine/src/entities/frame.rs` | FSM + bytemuck variants + tests. |
| 1147 | `crates/playa-engine/src/entities/project.rs` | Single source of truth for media; growth tracks feature surface. |

These are not bugs, but each one >1k LOC is a velocity tax — code review and IDE jump-to-def both slow. Encode at 3.1k is the clear refactor target.

### 3.5 `#[allow(...)]` audit
22 `#[allow]` attributes; nearly all are `dead_code` (planned but not yet wired API surface) or `clippy::upper_case_acronyms` on FFmpeg-style enum variants in `encode.rs`. No `#[allow(unsafe_op_in_unsafe_fn)]` or other safety-suppressing allows. **Healthy.**

### 3.6 `TODO`/`FIXME` inventory
Only **4 in-source TODO comments** (no `FIXME`/`HACK`/`XXX`):
1. `crates/playa-engine/src/entities/compositor.rs:222` — *"TODO for GPU compositing:"* (CPU↔GPU transform parity gap).
2. `crates/playa-engine/src/entities/comp_node.rs:1270` — *"TODO: Cache effected frames separately from composed frames."*
3. `crates/playa-ui/src/dialogs/encode/encode.rs:2735` — *"TODO: image crate doesn't expose TIFF compression settings easily"*.
4. `crates/playa-ui/src/dialogs/encode/encode.rs:2781` — *"TODO: RLE compression when image crate supports it"*.

Repo-level `TODO.md`:
1. Timecode support
2. EDL / OpenTimelineIO input
3. OCIO / OIIO integration
4. Shotgrid integration
5. Headless mode (core without GUI, Python API only)
6. Python API via RustPython — expose all major classes/widgets

`task2.md` adds: WASM build (gate ffmpeg+exr behind features, replace with `image` + WebCodecs), full UI WASM compatibility, online editor variant.

---

## 4. Concurrency / correctness traps

| Trap | Where | Symptom if violated |
|------|-------|---------------------|
| Cache invalidation requires `event_emitter` reattach after deserialize | `crates/playa-app/src/runner.rs` (post-load init) | UI shows stale frame after attribute changes; no error. |
| Direct `comp.layers.push/insert/remove` bypasses setters → must `comp.attrs.mark_dirty()` | `crates/playa-engine/src/entities/comp_node.rs`, called from `crates/playa-app/src/main_events.rs` | Same as above — modify_comp gate doesn't fire. AGENTS.md L162-166. |
| `try_claim_for_loading` TOCTOU pattern in frame loader | `crates/playa-engine/src/entities/frame.rs:425, 504` | Two workers race the same file; atomic Header→Loading transition prevents it. Don't refactor into separate read+write. |
| Worker epoch comparison drift mid-job | `crates/playa-engine/src/core/workers.rs` (epochs `Arc<AtomicU64>`) | Worker reads epoch at job start; if epoch bumps mid-compose the result is computed but discarded. **In-flight GPU uploads are NOT currently re-validated** — could waste a UI-thread blend cycle. Worth instrumenting. |
| `GpuCompositor` is UI-thread only; `drain_gpu_blend_queue` MUST run every frame | `crates/playa-app/src/app/run.rs` (main loop), `gpu_blend_bridge.rs` | Workers `delegate_blend_blocking` block until drained. Skipping the drain (e.g. early-return in update) deadlocks all GPU-mode workers. AGENTS.md L375, L388. |
| Direct push to `comp.layers` outside `project.modify_comp(...)` | n/a (style rule) | Auto-invalidation skipped — see row 2. |

---

## 5. Persistence / migration risk

| Surface | File / Key | Risk |
|---------|-----------|------|
| eframe `APP_KEY` blob | `crates/playa-app/src/runner.rs` (`config::config_file("playa.json")`) | Whole `PlayaApp` serialized via serde; adding a non-`#[serde(default)]` field on an existing struct breaks every existing user's session. |
| Project on-disk JSON | `Project::to_json` / `Project::from_json` | No version field visible in AGENTS.md description. Schema evolution will need an explicit version tag + migration shim. |
| Layouts schema | `AppSettings.layouts: HashMap<String, Layout>` | `SaveLayoutEvent`/`LoadLayoutEvent` were **removed** (AGENTS.md L463); replaced with auto-named `LayoutSelected/Created/Deleted/Updated/Renamed`. Old saved layouts using the removed events will not round-trip. |
| `serde(skip)` runtime fields | `event_emitter`, schemas, `cache_manager`, GPU bridge handles | All must be re-attached on load. `ensure_gpu_blend_initialized` rebuilds the bridge (CHANGELOG L20). Forgetting any one of these silently desyncs state. |

---

## 6. Platform-specific risks

| Platform | Risk |
|----------|------|
| **Windows** | `x64-windows-static-md-release` triplet; MSVC toolchain mandatory; `vfw32` linked manually for static FFmpeg avdevice; libgit2 SSH-agent broken → forced to `git-fetch-with-cli = true`. |
| **macOS** | `Cargo.toml:102` hard-codes signing identity `Developer ID Application: Alexander Khalyavin (Y8PQ7YASU9)`. Anyone else building a release artifact will fail at notarization without overriding the metadata; this is a personal Apple ID embedded in the public repo. |
| **Linux** | `Cargo.toml:85-87` packager resources include `target/release/*.dll` AND `target/release/*.so*` — the `.dll` glob is a no-op on Linux but signals the bundler config wasn't split per-OS. `.so*` glob may pull in unintended sysroot libraries. |
| **All** | `playa-ffmpeg 8.0.3` pinned with `static` — every host needs vcpkg to provide compatible FFmpeg 8.0 builds. A vcpkg version skew across dev machines silently produces different binaries. |

---

## 7. Performance hotspots / unfinished optimisations

- `Cargo.toml:72-74`: release profile is **link-speed tuned, not size/perf tuned**:
  ```
  [profile.release]
  strip = false
  lto = false
  # codegen-units = 1   ← commented out (AGENTS.md L498)
  ```
  Enabling `lto = "thin"` or `lto = true` and `codegen-units = 1` typically gives 5–15% perf in compositors and 20–40% binary-size reduction. Currently optimised for fast iteration.
- **CPU↔GPU transform parity gap**: `compositor.rs:222` TODO. CPU pre-warps pixels in compose; GPU applies `u_top_transform` in shader. Outputs diverge for sub-pixel transforms.
- **Cached effected frames**: `comp_node.rs:1270` TODO — currently effects re-run every compose; caching post-effect frames separately would help heavy GaussianBlur workloads.
- **TIFF / RLE compression**: encode dialog has placeholder fallbacks (`encode.rs:2735, 2781`) because the `image` crate doesn't expose those settings. Consider switching to `tiff` crate directly for those formats.
- **Worker sleep loop is 1 ms**: fine for current load, but on very fast SSDs and small EXR tiles the sleep can dominate. Consider parking-lot condvar wakeup if profiling shows it.

---

## 8. Security / safety

| Area | Status | Notes |
|------|--------|-------|
| REST API bind address | OK | `crates/playa-app/src/server/api.rs:213` uses `127.0.0.1:{port}` — loopback only, never `0.0.0.0`. |
| REST input validation | OK | FPS validated `is_finite() && > 0.0 && <= 960.0` (AGENTS.md L447). |
| FFmpeg CVE surface | Risk | Static link via `playa-ffmpeg 8.0.3` — every CVE requires bumping this dep + vcpkg ports + rebuilding all platforms. No automated alerts visible. |
| EXR parser | Risk | `vfx-exr` is a private fork; bug parity with upstream OpenEXR (which has had CVEs around DWAA decoding) is not tracked here. Untrusted EXR inputs reach the parser via drag-drop and `--file`. |
| Video parser | Risk | FFmpeg parses untrusted MP4/MOV/AVI/MKV. Static link means we ship whatever vcpkg's `ffmpeg[8.0.3,static]` last built — pin date matters. |
| Path traversal in `--playlist` / `--file` | Unchecked | CLI accepts arbitrary paths; OK for a desktop app, but if Python API exposes load-by-path to remote callers (TODO.md item 6), this surface widens. |
| Bundled secrets | None found | No API keys, tokens, or credentials in the repo (verified by absence of common patterns). The macOS Apple Developer ID is the only PII. |

**No fuzzing harness in tree.** With the EXR + video attack surface this is the biggest gap; consider `cargo fuzz` targets for `header_exr` and the video header probe.

---

## 9. Top issues — prioritised

| Priority | Issue | Action |
|----------|-------|--------|
| **P0** | `[patch."ssh://..."]` private dep makes fresh clone unbuildable | Vendor `vfx-rs` as submodule OR publish to a registry; document side-by-side checkout in DEVELOP.md. |
| **P0** | macOS signing identity hard-coded in public `Cargo.toml` | Move to env var (`PLAYA_SIGNING_IDENTITY`), or to a `.cargo/config.toml` template. |
| **P1** | `playa-py` excluded from workspace → silent API drift | Add a `cargo xtask check-py` that builds it on CI, or fold it back into the workspace once maturin friction is acceptable. |
| **P1** | CPU↔GPU compositor parity gap | Track in TODO.md; align matrix application order in CPU compose. |
| **P1** | No fuzzing of EXR / video loaders | Add `cargo fuzz` targets for `header_exr`, video probe. |
| **P2** | `encode.rs` is 3170 LOC | Split by codec family. |
| **P2** | Release profile isn't perf-tuned (`lto = false`) | Benchmark `lto = "thin"` + `codegen-units = 1` for shipped builds. |
| **P2** | `unwrap()` audit in non-test code in `comp_node.rs`, `file_node.rs`, `renderer.rs` | Manual inspection; replace with `?` or `unwrap_or_else(|e| log::warn!)`. |
| **P3** | Project JSON has no schema version | Add `schema_version: u32` field with migration shim before next breaking change. |
| **P3** | TIFF/RLE compression placeholders in encode | Switch to `tiff` crate directly. |

---

*Concerns audit: 2026-05-09. Cite the file paths above to verify each item.*
