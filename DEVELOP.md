# Developer Guide

This document covers building Playa from source, setting up a development environment, and contributor-oriented technical notes.

**For users**: See [README.md](README.md) for installation and usage.  
**For architecture**: See [AGENTS.md](AGENTS.md) for component design and dataflow.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Prerequisites](#prerequisites)
3. [Workspace & EXR Backend](#workspace--exr-backend)
4. [FFmpeg Setup](#ffmpeg-setup)
5. [Bootstrap and xtask](#bootstrap-and-xtask)
6. [Platform-Specific Notes](#platform-specific-notes)
7. [Architecture Overview](#architecture-overview)
8. [Technical Stack](#technical-stack)
9. [Contributing](#contributing)
10. [Troubleshooting](#troubleshooting)

---

## Quick Start

Unified entry point (**Python 3**, stdlib only):

```powershell
python bootstrap.py build          # release (default via xtask)
python bootstrap.py build -d      # debug
python bootstrap.py test
```

Equivalent without the script (once Rust + FFmpeg/vcpkg are configured below):

```bash
cargo build -p playa --release
# or:
cargo xtask build
cargo xtask test
```

---

## Prerequisites

### Required

- **Rust** with **edition 2024** (toolchain pinned by upstream dependencies; prefer current stable ≥ **1.85**)
- **Git**

### Video (strongly recommended)

- **vcpkg** with **FFmpeg** static triplets as in [AGENTS.md](AGENTS.md) / [FFmpeg Setup](#ffmpeg-setup) below. Builds use **playa-ffmpeg** (linked against vcpkg artifacts).

---

## Workspace & EXR Backend

The repo is a **Cargo workspace** (see root `Cargo.toml`):

| Path | Package | Role |
|------|---------|------|
| `.` | `playa` | Binary + thin library re-exports for `playa::…` consumers (e.g. Python bindings). |
| `crates/playa-app` | `playa-app` | PlayaApp, `main_events`, runner, CLI glue, REST server, shell, config. |
| `crates/playa-engine` | `playa-engine` | Core engine (`core`, `entities`, loaders, compositor helpers, …). |
| `crates/playa-events` | `playa-events` | Typed UI / app events shared across crates. |
| `crates/playa-io` | `playa-io` | FFmpeg init, decoding, EXR & media I/O façade (uses **vfx-exr**, **playa-ffmpeg**, **image**, …). |
| `crates/playa-ui` | `playa-ui` | egui widgets, dialogs, viewport renderer integration. |
| `crates/xtask` | `xtask` | Build/release helper CLI invoked as `cargo xtask …`. |
| `crates/playa-py` | — | Separate workspace (`[workspace.exclude]`); build with **maturin** / `bootstrap.py python`. |

**EXR**: default stack is **`vfx-exr`** — pure Rust, including **DWAA / DWAB / HTJ2K** (see `playa --version`). No separate “switch EXR backend” toggle in `cargo xtask build`.

---

## FFmpeg Setup

Required for video playback and encoding. Install FFmpeg via **vcpkg** for best compatibility.

### Windows

```powershell
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat

setx VCPKG_ROOT "C:\vcpkg"
setx VCPKGRS_TRIPLET "x64-windows-static-md-release"

C:\vcpkg\vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-windows-static-md-release
```

Activate the MSVC environment before compiling (adjust edition if needed):

```cmd
"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
```

`bootstrap.py` merges this environment automatically when MSVC is detected.

### Linux

```bash
git clone https://github.com/microsoft/vcpkg.git /usr/local/share/vcpkg
/usr/local/share/vcpkg/bootstrap-vcpkg.sh

export VCPKG_ROOT=/usr/local/share/vcpkg
export VCPKGRS_TRIPLET=x64-linux-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/$VCPKGRS_TRIPLET/lib/pkgconfig

vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-linux-release
```

### macOS

```bash
git clone https://github.com/microsoft/vcpkg.git /usr/local/share/vcpkg
/usr/local/share/vcpkg/bootstrap-vcpkg.sh

export VCPKG_ROOT=/usr/local/share/vcpkg

# Apple Silicon
export VCPKGRS_TRIPLET=arm64-osx-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/arm64-osx-release/lib/pkgconfig
vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:arm64-osx-release

# Intel
export VCPKGRS_TRIPLET=x64-osx-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/x64-osx-release/lib/pkgconfig
vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:x64-osx-release
```

**Note**: Hardware encoding paths (e.g. VideoToolbox on macOS) depend on FFmpeg feature flags chosen at vcpkg build time.

### Verify Installation

```bash
pkg-config --modversion libavcodec libavformat libavutil libswscale
```

---

## Bootstrap and xtask

### bootstrap.py

- Cross-platform (**Windows / Linux / macOS**): no `bootstrap.ps1` / `.sh` in-tree.
- **build / test**: sets `VCPKG_ROOT`, `VCPKGRS_TRIPLET`, and on Windows merges the MSVC toolchain environment; builds the **`xtask`** binary once (`cargo build -p xtask`) when missing under `target/debug/`; forwards to **`cargo xtask`**.
- **package / publish**: ensures optional tooling (**cargo-packager**, **cargo-release**) via **cargo-binstall** when needed.
- **Extra Cargo features**: `python bootstrap.py build -f profiler` forwards `--features` to the underlying **`cargo`** invocation.
- **`python`-related commands**: operate on `crates/playa-py/` (venv + **maturin**).

See `bootstrap.py --help` / embedded `HELP_TEXT`.

### cargo xtask (package `crates/xtask`)

Invoked via **`.cargo/config.toml`**: `cargo xtask` → **`cargo run -p xtask --`**.

Subcommands (**current codebase**):

| Command | Purpose |
|---------|---------|
| `build [--release \| --debug]` | `cargo build -p playa` in the chosen profile. |
| `test [--debug] [--nocapture]` | Runs the workspace test suite. |
| `deploy [--install-dir PATH]` | Copies `playa` into a conventional install prefix. |
| `changelog` | Regenerates **`CHANGELOG.md`**. |
| `tag-dev` / `tag-rel` / `pr` | Maintainer release automation (needs `git` / CI expectations). |
| `wipe` / `wipe-wf` | Cleans select `target/` artifacts; workflow-run cleanup (**`wipe-wf`** uses **gh** CLI). |

Legacy helper sources under `crates/xtask/src/` (for example **`pre_build`** / **`post_build`**) remain for compatibility with older pipelines but are **not** exposed as `cargo xtask` subcommands anymore.

---

## Platform-Specific Notes

### Windows

- Prefer **PowerShell** for running `bootstrap.py`; use **Developer PowerShell** or run `vcvars64.bat` so **link.exe** finds MSVC libs.

### Linux / macOS

- Match **`VCPKGRS_TRIPLET`** to your OS/architecture (see FFmpeg section).

### macOS

**Code signing (releases):** DMG artifacts are Developer-ID signed (`AGENTS.md` metadata).

For local **`target/release/playa`** builds blocked by Gatekeeper:

```bash
xattr -cr target/release/playa
```

---

## Architecture Overview

Authoritative diagrams and module maps live in **[AGENTS.md](AGENTS.md)**.

Summary:

```
crates/playa-engine   ← core playback, caching, entities, loaders
crates/playa-app    ← eframe shell, PlayaApp, main_events, REST
crates/playa-ui     ← egui widgets / dialogs / viewport glue
crates/playa-io     ← decode / FFmpeg / EXR façade
crates/playa-events ← shared event types + EventBus types
(playa-py)          ← separate workspace; consumes `libplaya`
```

Thin root **`src/`** contains only **`main.rs`**, **`lib.rs`**, **`README.md`**.

---

## Technical Stack

| Component | Technology |
|-----------|------------|
| UI Framework | egui ~0.33 + eframe |
| Graphics | OpenGL (**glow**), viewport gpu compositing path optional |
| EXR | **vfx-exr** (Rust, DWAA/DWAB/HTJ2K-capable stacks) via **playa-io** |
| Still images | **image** 0.25 (PNG/JPEG/TIFF/…) |
| Video | **FFmpeg** via **playa-ffmpeg** |
| Concurrency | crossbeam, work-stealing worker pool (**playa-engine**) |
| HTTP server | rouille |

---

## Contributing

### Commit Conventions

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` new feature  
- `fix:` bug fix  
- `docs:` documentation  
- `refactor:` code refactoring  
- `chore:` maintenance  

### Workflow

1. Fork and clone  
2. `python bootstrap.py build`  
3. Make changes  
4. `python bootstrap.py test` (or `cargo xtask test`)  
5. `cargo fmt`, `cargo clippy` — before opening a PR  
6. Submit PR  

### AI-Assisted Development

See **[AGENTS.md](AGENTS.md)** for conventions when editing this codebase.

---

## Troubleshooting

### FFmpeg / vcpkg not found

```
error: failed to run custom build command for `playa-ffmpeg`
```

Verify:

```powershell
echo $env:VCPKG_ROOT
echo $env:VCPKGRS_TRIPLET
```

and **`PKG_CONFIG_PATH`** on Unix (see FFmpeg section).

### macOS Gatekeeper blocks app

See [Platform-Specific Notes](#platform-specific-notes).

---

*Architecture: [AGENTS.md](AGENTS.md) · Users: [README.md](README.md)*
