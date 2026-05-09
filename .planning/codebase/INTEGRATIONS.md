# External Integrations

**Analysis Date:** 2026-05-09

Playa is a desktop application: there are no outbound API calls, no cloud SDKs, no auth providers, no databases. "Integrations" here means **format backends, native libraries, on-disk persistence, the inbound REST control surface, and the Python FFI**.

## File Formats Handled

Dispatch lives in `playa_io::loader::classify_ext` (referenced from `AGENTS.md` "Loaders"); each loader exposes `header_*` (cheap header read for `FileNode` creation) and `load_*` (full decode invoked from a worker).

| Class | Backend | Crate dep | Extensions |
|-------|---------|-----------|------------|
| EXR | `vfx-exr` (pure Rust, all compressions including DWAA/DWAB/HTJ2K) | `vfx-exr`, `vfx-io`, `vfx-core` (git, behind `playa-io/exr`) | `.exr` |
| Generic raster | `image` 0.25 (PNG, JPEG, TIFF, TGA, HDR features) | `image` workspace dep | `.png`, `.jpg`, `.jpeg`, `.tif`, `.tiff`, `.tga`, `.hdr` |
| Video | `playa-ffmpeg` 8.0.3 (FFmpeg static linkage) | `playa-ffmpeg` workspace dep (behind `playa-io/ffmpeg`) | `.mp4`, `.mov`, `.avi`, `.mkv` |
| Sequence detection | `scanseq` 0.1.5 | `scanseq` (`crates/playa-app`) | Auto-resolves siblings around any frame |

**Pixel formats** decoded into `Frame`: 8-bit u8, 16-bit half-float (`half::f16`), 32-bit float.

**Video metadata gotcha** (`AGENTS.md` Loaders section, BUG-04 / BUG-13): `VideoMetadata::from_file` guards `denom != 0` and rounds `frame_count = (duration_secs * fps).round()` rather than truncating with `as usize`.

**Frame state machine** (file-load FSM): `Placeholder | Header → try_claim → Loading → (Loaded | Error)`, plus `Loaded → dehydrate → Expired → Loading`. Atomic claim avoids two-worker TOCTOU races on the same file.

## Hardware Video Encoding

Exposed via the FFmpeg pipeline (`playa-ffmpeg` 8.0 + vcpkg `ffmpeg[...,nvcodec]`). Codecs surfaced in the Encode dialog (per README "Video Export"):

- **NVENC** — NVIDIA hardware encode
- **QSV** — Intel Quick Sync hardware encode
- **AMF** — AMD hardware encode
- **libx264** / **libx265** — software fallback (H.264 / H.265)

Range export honors B/N markers from the timeline.

## REST API (Inbound)

A `rouille` HTTP server runs on a dedicated thread; requests are translated into `ApiCommand` and forwarded to the main thread via `crossbeam-channel::Sender<ApiCommand>`. State is read from `Arc<RwLock<SharedApiState>>` updated every frame.

- **Bind**: `127.0.0.1:<port>` only (loopback) — `crates/playa-app/src/server/`
- **Lazy start**: enabled in Settings; first invocation lives in `PlayaApp::update` step "start_api_server()"
- **FPS validation**: `is_finite() && > 0.0 && <= 960.0` before `SetFpsEvent`

Endpoints (from `crates/playa-app/src/server/mod.rs:44-56` and `server/api.rs:282+`):

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/status` | Combined player + comp + cache snapshot |
| GET | `/api/player` | Player state |
| GET | `/api/comp` | Active comp info |
| GET | `/api/cache` | Cache memory stats |
| GET | `/api/health` | Health check |
| GET | `/api/screenshot` | Full window capture (`glReadPixels` after render) |
| GET | `/api/screenshot/frame` | Raw current frame as PNG (no UI) |
| POST | `/api/player/play` | Start playback |
| POST | `/api/player/pause` | Pause playback |
| POST | `/api/player/stop` | Pause + seek to 0 |
| POST | `/api/player/frame/{n}` | Seek to frame `n` (path-parameter parsed manually before `router!`, `api.rs:253`) |
| POST | `/api/player/fps/{n}` | Set FPS (validated) |
| POST | `/api/player/toggle-loop` | Toggle loop mode |
| POST | `/api/player/next` | Next layer/clip edge |
| POST | `/api/player/prev` | Previous layer/clip edge |
| POST | `/api/project/load` | Load sequence/playlist (JSON body) |
| POST | `/api/event` | Emit a custom event by name |

Screenshots use a `Screenshot { viewport_only: bool, response: crossbeam::Sender }` request struct so the HTTP thread can block on the rendered result.

## CLI Surface

Defined with `clap` derive in `crates/playa-app/src/cli.rs`. Flags (line numbers from grep):

| Short | Long | Arg | Notes |
|-------|------|-----|-------|
| `-f` | `--file` | `FILE` | Extra files (repeatable) |
| `-p` | `--playlist` | `PLAYLIST` | `Project::from_json` at startup |
| `-F` | `--fullscreen` | — | |
|  | `--frame` | `N` | Start frame |
| `-a` | `--autoplay` | — | |
| `-o` | `--loop` | `0|1` (default `1`) | |
|  | `--start` | `N` | Range start |
|  | `--end` | `N` | Range end |
|  | `--range` | `START END` | Shorthand (`num_args = 2`) |
| `-l` | `--log` | `[LOG_FILE]` | Optional file path |
| `-v` | `--verbose` | count (`-v`/`-vv`/`-vvv`) | warn/info/debug/trace |
| `-c` | `--config-dir` | `DIR` | Override platform config path |
|  | `--mem` | `PERCENT` | Hidden (`hide = true`); legacy worker-config ENV fallback |
|  | `--workers` | `N` | Hidden (`hide = true`); legacy worker-config ENV fallback |

Trailing positional argument is the primary `[FILE]`.

## Persistence Boundaries

No database, no remote storage. All state is local files.

| Item | Mechanism | Path |
|------|-----------|------|
| Window position/size | `eframe` built-in (`persist_window: true`, `persistence_path` set in `crates/playa-app/src/runner.rs`) | `<config_dir>/playa.json` |
| App state (`PlayaApp`) | `eframe::APP_KEY` JSON — `#[serde(default)]` on the struct, runtime fields marked `#[serde(skip)]` | Same `playa.json` |
| Project ("playlist") | `Project::to_json` / `Project::from_json` — separate on-disk format, loaded via `--playlist` | User-chosen path |
| Logs | `env_logger` to file when `--log [PATH]` is set | `<config_dir>/playa.log` (default name) |
| Layouts | `AppSettings.layouts: HashMap<String, Layout>` — embedded in `playa.json` | Inside app state |
| Custom shaders | `Shaders::load_shader_directory` scans `shaders/` next to the binary | `./shaders/*` |

**Platform config / data dirs** (via `dirs-next` 2.0):

| OS | Config | Data |
|----|--------|------|
| Linux | `~/.config/playa/` | `~/.local/share/playa/` |
| macOS | `~/Library/Application Support/playa/` | same |
| Windows | `%APPDATA%\playa\` | same |

**Override precedence** (highest first):
1. `--config-dir DIR` CLI flag
2. `PLAYA_CONFIG_DIR` environment variable
3. Local directory ("portable" mode — triggered when CWD already contains `playa.json` or `playa.log`)
4. Platform default via `dirs-next`

**`#[serde(skip)]` re-init contract** (`AGENTS.md` Surprises): after deserialization, `project.set_event_emitter(event_bus.emitter())` must be called or attribute mutations silently fail to invalidate the cache. Same applies to `cache_manager` and node schemas — restored in `crates/playa-app/src/runner.rs`.

## Native FFmpeg Linkage

- Crate: `playa-ffmpeg` 8.0.3 (vendored under `crates/playa-ffmpeg/`) with feature `static` — statically linked, no system FFmpeg DLL/SO at runtime
- Resolved via vcpkg through `playa-ffmpeg/build.rs` (`vcpkg::find_package("ffmpeg")`). Env (`VCPKG_ROOT` / `VCPKGRS_TRIPLET` / `PKG_CONFIG_PATH`) is set up automatically by `xtask::env_setup`
- Required vcpkg port: `ffmpeg[core,avcodec,avformat,swresample,swscale,nvcodec]` — **without** `avdevice` and `avfilter` (vcpkg's FFmpeg 8.1+ avfilter pulls in `vsrc_gfxcapture_winrt` which causes MSVC STL link mismatches; see `crates/playa-ffmpeg/README.md`)
- `pkgconf` (also via vcpkg) is required for cargo to find the .pc files

## vfx-rs External Repository

EXR support is supplied by a sibling git repo (`https://github.com/ssoj13/vfx-rs.git`, public, branch `main`), declared in `crates/playa-io/Cargo.toml:26-28`:

| Sub-crate | Used for |
|-----------|----------|
| `vfx-exr` | EXR decode/encode (DWAA/DWAB/HTJ2K capable) |
| `vfx-io` | OIIO-flavoured I/O surface |
| `vfx-core` | Foundation types |

These resolve directly via Cargo over HTTPS — no `[patch]` override, no sibling checkout required.

## Python FFI Boundary

`crates/playa-py` (separate workspace, `Cargo.toml:11` `exclude`):

- PyO3 0.23, `extension-module` feature
- Re-exports `playa = { path = "../.." }`, so the Python module embeds the full app
- Built outside cargo: `python bootstrap.py python [--install]` calls `maturin develop|build` against `.venv`
- Module name: `playa` (`crate-type = ["cdylib"]`, `lib.name = "playa"`)
- Entry usage: `import playa; playa.run(file=..., autoplay=True)` (per `bootstrap.py:561`)

This is an **inbound** integration: Python embeds Rust. Rust never calls back into Python.

## Outbound Integrations

None. There is no Tokio, no `reqwest`, no AWS/GCP/Azure SDK, no telemetry, no auth provider. The only network surface is the inbound `rouille` server bound to loopback. (`AGENTS.md` "Tokio / Async": *"There is no Tokio in the project. ... HTTP is `rouille` (synchronous). Don't introduce an async runtime without a clear need."*)

## CI / Release Tooling

GitHub Actions workflows are managed via `cargo xtask`:

- `cargo xtask tag-dev` — push a dev tag (triggers Build workflow)
- `cargo xtask tag-rel` — push a release tag (triggers Release workflow)
- `cargo xtask pr` — open `dev → main` PR (uses `gh` CLI)
- `cargo xtask wipe-wf` — bulk-delete workflow runs (uses `gh` CLI)
- `cargo xtask changelog` — regenerate `CHANGELOG.md` via `git-cliff` (configured in `[package.metadata.release]`, `Cargo.toml:104-106`)

Distribution is produced by `cargo-packager` 0.11.7 (`bootstrap.py package`):

- Windows: NSIS installer (`installer-mode = "perMachine"`), MSI, portable ZIP
- macOS: signed DMG (identity `Y8PQ7YASU9`)
- Linux: AppImage, `.deb`
- File associations registered for `.exr`, `.png`, `.jpg`, `.jpeg`, `.tif`, `.tiff` (`Cargo.toml:90-93`)

## Environment Variables Consumed

| Variable | Consumer | Purpose |
|----------|----------|---------|
| `VCPKG_ROOT` | `bootstrap.py`, cargo (vcpkg-rs) | Native dep root |
| `VCPKGRS_TRIPLET` | `bootstrap.py`, cargo | Triplet selection |
| `PKG_CONFIG_PATH` | cargo (FFmpeg link) | Auto-set from triplet |
| `LIBCLANG_PATH` | bindgen (vfx-rs) | Auto-cleared if pointing to ESP32/Xtensa clang |
| `PLAYA_CONFIG_DIR` | `playa-app::config` | Override platform config dir |
| `RUST_LOG` (implicit) | `env_logger` | Standard log filter |

No `.env` file is used or expected by the application.

---

*Integration audit: 2026-05-09*
