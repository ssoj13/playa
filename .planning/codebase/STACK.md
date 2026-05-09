# Technology Stack

**Analysis Date:** 2026-05-09

## Languages

**Primary:**
- Rust — edition **2024**, single-binary desktop app (`target/release/playa[.exe]`)
- Toolchain: stable (no `rust-toolchain.toml` pinning observed)

**Secondary:**
- Python 3 — `bootstrap.py` build driver (stdlib only, cross-platform)
- Python (PyO3) — `crates/playa-py` exposes a `playa` Python module

## Runtime / Toolchain

| Item | Value |
|------|-------|
| Cargo edition | 2024 (workspace-wide, see `Cargo.toml:35`) |
| Resolver | 2 (`Cargo.toml:12`) |
| Workspace package version | 0.1.142 (lockstep across crates) |
| Default members | `.` (root crate `playa`) |
| Excluded members | `crates/playa-py` (separate workspace, built via maturin) |
| Release profile | `strip = false`, `lto = false` (link speed > binary size, `Cargo.toml:72`) |

## Workspace Layout

The repo is a Cargo **workspace** with a thin root crate `playa` (`src/main.rs`, `src/lib.rs`) that re-exports the engine/UI/app surface; all logic lives in member crates.

| Crate | Path | Role |
|-------|------|------|
| `playa` | `./` | Binary entry + `lib.rs` aggregator (re-exports for `playa::` and Python bindings) |
| `playa-app` | `crates/playa-app` | Desktop host: `PlayaApp`, `main_events`, `runner`, `cli`, REST `server/`, `shell`, `config` |
| `playa-engine` | `crates/playa-engine` | Playback / compositing engine: `core` (cache, workers, events), `entities` (nodes, attrs, effects), `defaults`, `utils` |
| `playa-events` | `crates/playa-events` | Typed events + `EventBus` (single inter-module communication path) |
| `playa-io` | `crates/playa-io` | Media I/O: image/EXR/video loaders; gates `exr` & `ffmpeg` behind features |
| `playa-ui` | `crates/playa-ui` | egui widgets, dialogs, menu composition |
| `xtask` | `crates/xtask` | Build automation (`changelog`, `tag-dev/-rel`, `pr`, `build`, `test`, `deploy`, `wipe`, `wipe-wf`) |
| `playa-py` | `crates/playa-py` | **Excluded** from main workspace; PyO3 bindings, built via maturin |

Crate dependency direction:
```
playa-app + playa-ui ──▶ playa-engine ──▶ playa-io
                  ╲          │             │
                   ╲         ▼             │
                    └──▶ playa-events ◀────┘
```

## Frameworks (Workspace-pinned)

Defined under `[workspace.dependencies]` in root `Cargo.toml:15-30`.

**GUI / Windowing:**
- `eframe` 0.33 — `default-features = false` + features `accesskit, default_fonts, wgpu, wayland, web_screen_reader, x11, persistence` (`crates/playa-app/Cargo.toml:19`, `crates/playa-ui/Cargo.toml:17`)
- `egui_dock` 0.18 (with `serde`) — dockable panels (`Cargo.toml:20`)
- `egui-wgpu` 0.33 — wgpu integration for egui (`Cargo.toml:19`)
- `egui_extras` 0.33, `egui_dnd` 0.14, `egui_ltreeview` 0.6.1 (with `persistence`), `egui-snarl` 0.9 (with `serde`), `transform-gizmo-egui` 0.8 (`crates/playa-ui/Cargo.toml`)

**Graphics / GPU:**
- `wgpu` 27 — modern GPU backend (workspace pin, `Cargo.toml:29`)
- `glam` 0.32 — math (vec/mat/quat) (`Cargo.toml:22`)
- `bytemuck` 1.25 — POD casts for buffer uploads (`Cargo.toml:17`)
- `half` 2.7 (with `bytemuck`) — `f16` support for EXR/HDR pixels (`Cargo.toml:23`)

> Note: AGENTS.md repeatedly mentions OpenGL/glow shaders (`shaders/`, `glReadPixels`, `u_top_transform`, PBO) — but only `wgpu` and `egui-wgpu` appear in Cargo dependencies. Either glow is reached transitively through eframe's `wgpu` feature or the legacy GL paths are documentation-only. Verify before relying on a "GL backend" claim.

**Image / Codec I/O:**
- `image` 0.25 — `default-features = false`, features `png, jpeg, tiff, tga, hdr` (`Cargo.toml:24`)
- `playa-ffmpeg` 8.0.3 (workspace, `static` feature) — statically linked FFmpeg for video (`Cargo.toml:30`)
- `vfx-exr` (git, branch `main`, feature `htj2k`) — pure-Rust EXR backend with DWAA/DWAB/HTJ2K
- `vfx-io` (git, features `exr, htj2k`)
- `vfx-core` (git)
  All three from `ssh://git@github.com/ssoj13/vfx-rs.git`, declared in `crates/playa-io/Cargo.toml:26-28`.

**Concurrency / Cache:**
- `crossbeam` 0.8.4 (engine) — work-stealing primitives, atomics
- `crossbeam-channel` 0.5 (app) — sync channels
- `rayon` 1.11 (engine) — data parallelism inside loaders/effects
- `lru` 0.16 (engine) — `LruCache<CacheKey, ()>` ordering
- `num_cpus` 1.17 (app) — pool sizing (`num_cpus::get() * 3 / 4`)
- `sysinfo` 0.38 (engine) — available memory probe for cache budget
- `lazy_static` 1.5, `enum_dispatch` 0.3, `glob` 0.3 (engine)

**Serialization / IDs / Logging:**
- `serde` 1.0 (with `derive`), `serde_json` 1.0
- `uuid` 1.22 (features `v4, serde`)
- `log` 0.4, `env_logger` 0.11
- `anyhow` 1.0
- `clap` 4.6 (with `derive`)

**HTTP / OS:**
- `rouille` 3.6 — synchronous HTTP server bound loopback for REST API (`crates/playa-app/Cargo.toml:42`)
- `dirs-next` 2.0 — cross-platform config/data dirs (`crates/playa-app/Cargo.toml:38`)
- `rfd` 0.17 — native file dialogs (app + ui)
- `regex` 1.12, `scanseq` 0.1.5 (sequence detection), `const_format` 0.2

**Text rendering:**
- `cosmic-text` 0.18 (engine) — TextNode shaping/layout

## Optional Cargo Features

Root crate (`Cargo.toml:47-49`):
- `default = []`
- `profiler = ["dep:puffin", "dep:puffin_egui"]` — pulls `puffin` 0.20 + `puffin_egui` 0.30 for in-app profiling

`playa-io` features (`crates/playa-io/Cargo.toml:12-18`):
- `default = ["exr", "ffmpeg"]`
- `exr` → `vfx-exr` + `vfx-io` + `vfx-core`
- `ffmpeg` → `playa-ffmpeg`
- `webcodecs` (placeholder for browser builds; no implementation yet)

Non-wasm targets force `playa-io` with `["exr", "ffmpeg"]` (see `Cargo.toml:64-65`, mirrored in `playa-engine` and `playa-ui`).

## Patched Dependencies

Root `Cargo.toml:76-79`:

```toml
[patch."ssh://git@github.com/ssoj13/vfx-rs.git"]
vfx-exr  = { path = "../vfx-rs/crates/exr/vfx-exr" }
vfx-io   = { path = "../vfx-rs/crates/oiio/vfx-io" }
vfx-core = { path = "../vfx-rs/crates/foundation/vfx-core" }
```

Local checkout of `vfx-rs` is **required** at `../vfx-rs` (sibling to `playa/`); cargo will not build without it unless the patch block is removed.

## Build System

| Layer | Tool |
|-------|------|
| Build driver | `bootstrap.py` (Python 3, stdlib only) — sets `VCPKG_ROOT`, `VCPKGRS_TRIPLET`, MSVC env, then calls `cargo xtask build` |
| Workspace orchestrator | `cargo xtask` (`crates/xtask`) — release/debug, deploy, changelog (git-cliff), tags, GitHub Actions cleanup |
| `build.rs` | Trivial: only `cargo:rerun-if-changed=build.rs` (`build.rs`) |
| Native deps | **vcpkg** (FFmpeg statically linked via `playa-ffmpeg`'s `static` feature) |
| Packaging | `cargo-packager` 0.11.7 (NSIS installer, MSI, DMG, AppImage, .deb) — config in `[package.metadata.packager]` |
| Release | `cargo-release` + `git-cliff` (changelog) — see `[package.metadata.release]` |
| Python wheel | `maturin` (via `bootstrap.py python`) inside `.venv` |

`xtask` itself depends on `fs_extra` 1.3, `indicatif` 0.18, and **on Windows** `vcv-rs` (git, `https://github.com/ssoj13/vcv-rs`) for fast MSVC env probing (`crates/xtask/Cargo.toml:13-14`).

## Native Dependencies (vcpkg)

Required triplets (pick per platform):

| Platform | Triplet |
|----------|---------|
| Windows | `x64-windows-static-md-release` (default, see `bootstrap.py:48`) |
| Linux | `x64-linux-release` |
| macOS Intel | `x64-osx-release` |
| macOS Apple Silicon | `arm64-osx-release` |

Required ports (from `bootstrap.py:592` install hint):
`ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]` plus `pkgconf`.

Env vars consumed:
- `VCPKG_ROOT` (default `C:\vcpkg` on Windows, `~/vcpkg` elsewhere)
- `VCPKGRS_TRIPLET`
- `PKG_CONFIG_PATH` (auto-derived from the triplet)
- `LIBCLANG_PATH` (auto-cleared if it points to ESP32/Xtensa clang, `bootstrap.py:236`)

## Python Bindings (separate workspace)

`crates/playa-py/Cargo.toml`:
- `edition = "2021"` (lone crate not on edition 2024)
- `crate-type = ["cdylib"]`, lib name `playa`
- `pyo3` 0.23 (`extension-module`)
- Depends on `playa = { path = "../.." }` — re-uses the full main crate
- `clap` 4.5, `log` 0.4, `env_logger` 0.11

Built via `maturin` in `.venv` by `bootstrap.py python [--install]`. Not part of the main workspace (`Cargo.toml:11` `exclude`).

## Platform Requirements

**Development:**
- Windows 11 + MSVC toolchain (Developer PowerShell or `vcvars64.bat`); PowerShell 7+ assumed in tooling
- Linux: GCC/Clang + system deps for vcpkg ffmpeg build
- macOS: Xcode CLT; signing identity `Developer ID Application: Alexander Khalyavin (Y8PQ7YASU9)` (`Cargo.toml:102`)

**Production:**
- Single static binary; on Windows ships with no DLLs (triplet `static-md`)
- macOS DMGs are code-signed and notarized (per README)
- Linux ships AppImage / `.deb` (cargo-packager metadata)

---

*Stack analysis: 2026-05-09*
