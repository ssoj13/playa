# Developer Guide

This document covers building Playa from source, setting up development environment, and technical details for contributors.

**For users**: See [README.md](README.md) for installation and usage.  
**For architecture**: See [AGENTS.md](AGENTS.md) for component design and dataflow.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Prerequisites](#prerequisites)
3. [Build Options](#build-options)
4. [FFmpeg Setup](#ffmpeg-setup)
5. [xtask Commands](#xtask-commands)
6. [Platform-Specific Notes](#platform-specific-notes)
7. [Architecture Overview](#architecture-overview)
8. [Technical Stack](#technical-stack)
9. [Contributing](#contributing)

---

## Quick Start

Bootstrap scripts handle all dependencies automatically:

```powershell
# Windows (PowerShell)
.\bootstrap.ps1 build              # Build with exrs (fast, pure Rust)
.\bootstrap.ps1 build --openexr    # Build with OpenEXR (DWAA/DWAB support)
.\bootstrap.ps1 test               # Run all tests
```

```bash
# Linux/macOS
./bootstrap.sh build
./bootstrap.sh build --openexr
./bootstrap.sh test
```

**What bootstrap does:**
1. Checks Rust installation
2. Sets up vcpkg environment variables automatically
3. Installs dev tools via cargo-binstall (cargo-release, cargo-packager)
4. Builds xtask helper binary
5. Forwards to `cargo xtask` with correct configuration

---

## Prerequisites

### Required
- **Rust 1.85+** (edition 2024)
- **Git**

### Optional (for OpenEXR backend)
- C++ compiler (MSVC on Windows, GCC/Clang on Linux/macOS)
- CMake 3.16+

### For Video Support
- vcpkg with FFmpeg libraries (see [FFmpeg Setup](#ffmpeg-setup))

---

## Build Options

### EXR Backends

| Backend | Command | Dependencies | DWAA/DWAB |
|---------|---------|--------------|-----------|
| **exrs** (default) | `cargo build --release` | None (pure Rust) | No |
| **OpenEXR** | `cargo xtask build --release --openexr` | C++, CMake | Yes |

### Default Build (exrs - Pure Rust)

Fast build, no external dependencies:

```bash
git clone https://github.com/ssoj13/playa.git
cd playa
cargo build --release
```

Binary: `target/release/playa` (or `playa.exe` on Windows)

**Limitation**: Cannot load DWAA/DWAB compressed EXR files.

### OpenEXR Build (C++ Backend)

Full EXR format support:

```bash
git clone https://github.com/ssoj13/playa.git
cd playa
cargo xtask build --release --openexr
```

**Note**: First build compiles C++ libraries (~5-10 minutes, then cached).

---

## FFmpeg Setup

Required for video playback and encoding. Install via vcpkg for best compatibility.

### Windows

```powershell
# Install vcpkg
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat

# Set environment variables (add permanently to system)
setx VCPKG_ROOT "C:\vcpkg"
setx VCPKGRS_TRIPLET "x64-windows-static-md-release"

# Install FFmpeg with hardware acceleration
C:\vcpkg\vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-windows-static-md-release
```

**Before building**, setup Visual Studio environment:
```cmd
"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
```

### Linux

```bash
# Install vcpkg
git clone https://github.com/microsoft/vcpkg.git /usr/local/share/vcpkg
/usr/local/share/vcpkg/bootstrap-vcpkg.sh

# Set environment variables
export VCPKG_ROOT=/usr/local/share/vcpkg
export VCPKGRS_TRIPLET=x64-linux-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/$VCPKGRS_TRIPLET/lib/pkgconfig

# Install FFmpeg
vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-linux-release
```

### macOS

```bash
# Install vcpkg
git clone https://github.com/microsoft/vcpkg.git /usr/local/share/vcpkg
/usr/local/share/vcpkg/bootstrap-vcpkg.sh

export VCPKG_ROOT=/usr/local/share/vcpkg

# M1/M2 Macs (Apple Silicon)
export VCPKGRS_TRIPLET=arm64-osx-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/arm64-osx-release/lib/pkgconfig
vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:arm64-osx-release

# Intel Macs
export VCPKGRS_TRIPLET=x64-osx-release
export PKG_CONFIG_PATH=$VCPKG_ROOT/installed/x64-osx-release/lib/pkgconfig
vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:x64-osx-release
```

**Note**: macOS hardware encoding (VideoToolbox) requires system FFmpeg with `--enable-videotoolbox`.

### Verify Installation

```bash
pkg-config --modversion libavcodec libavformat libavutil libswscale
```

---

## xtask Commands

`xtask` is an idiomatic Rust build automation pattern - cross-platform, type-safe, no external tools needed.

### Build & Development

```bash
cargo xtask build [--release] [--openexr]  # Full build
cargo xtask post [--release]               # Copy native libraries (OpenEXR only)
cargo xtask verify [--release]             # Verify dependencies present
cargo xtask deploy [--install-dir PATH]    # Install to system
```

Default install locations:
- Windows: `%LOCALAPPDATA%\Programs\playa`
- Linux/macOS: `~/.local/bin/playa`

### Release Management

```bash
cargo xtask tag-dev [patch|minor|major]  # Create v0.1.x-dev tag
cargo xtask tag-rel [patch|minor|major]  # Create v0.1.x release tag
cargo xtask pr [version]                 # Create PR: dev -> main
cargo xtask changelog                    # Preview unreleased changes
```

### Platform-Specific

```bash
cargo xtask pre   # Linux only: Patch OpenEXR headers for GCC 11+
```

---

## Platform-Specific Notes

### Windows

- Use PowerShell, not cmd.exe
- Visual Studio 2022 Build Tools required for OpenEXR
- Static linking produces single .exe with no DLLs (exrs backend)

### Linux

**OpenEXR GCC 11+ Header Patching:**

OpenEXR 3.0.5 headers are missing `#include <cstdint>`:
```
error: 'uint64_t' has not been declared
```

`cargo xtask pre` patches these headers automatically:
- `ImfTiledMisc.h`
- `ImfDeepTiledInputFile.h`
- `ImfDeepTiledInputPart.h`

**Native Libraries (OpenEXR build):**

| Library | Purpose |
|---------|---------|
| OpenEXR Core (4 libs) | EXR reading/writing |
| Imath | Math library |
| Zlib | Compression |
| OpenEXR-C | C API wrapper |

### macOS

**Code Signing & Notarization:**

DMG releases are signed with Developer ID and notarized by Apple:
- No Gatekeeper warnings
- No "unidentified developer" dialogs

For development builds, run from terminal or disable Gatekeeper temporarily.

**Apple Silicon vs Intel:**
- M1/M2: Use `arm64-osx-release` triplet
- Intel: Use `x64-osx-release` triplet

---

## Architecture Overview

See [AGENTS.md](AGENTS.md) for detailed architecture documentation.

### Module Structure

```
src/
├── core/           # Engine (cache, events, player, workers)
├── entities/       # Data models (comp, frame, project, nodes)
├── widgets/        # UI components (viewport, timeline, project)
├── dialogs/        # Modal windows (encode, prefs)
├── server/         # REST API
└── main_events.rs  # Central event handler
```

### Key Patterns

1. **Event-Driven**: UI emits events → EventBus → handlers → state changes
2. **Work-Stealing**: Background frame loading with epoch-based cancellation
3. **LRU Cache**: Memory-managed frame cache with automatic eviction
4. **Arc<NodeKind>**: Lock-free worker access to media nodes

### Data Flow

```
User Input → EventBus → Handler → State Change → Cache Invalidation → Worker Load → Render
```

---

## Technical Stack

| Component | Technology |
|-----------|------------|
| UI Framework | egui 0.33 + eframe |
| Graphics | OpenGL via glow |
| EXR (default) | exrs via image 0.25 |
| EXR (optional) | openexr 0.11 (C++ bindings) |
| Video | FFmpeg via playa-ffmpeg |
| Concurrency | crossbeam channels, work-stealing deques |
| HTTP Server | rouille |
| CLI | clap 4.5 |

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
2. Run bootstrap script
3. Make changes
4. Run tests: `.\bootstrap.ps1 test` / `./bootstrap.sh test`
5. Submit PR

### AI-Assisted Development

This project uses Claude Code and Codex for development. See [AGENTS.md](AGENTS.md) for AI guidelines when working with this codebase.

---

## Troubleshooting

### FFmpeg not found

```
error: failed to run custom build command for `playa-ffmpeg`
```

Verify environment variables:
```powershell
echo $env:VCPKG_ROOT
echo $env:VCPKGRS_TRIPLET
```

### OpenEXR headers missing cstdint

Run: `cargo xtask pre` (Linux only)

### macOS Gatekeeper blocks app

For development builds:
```bash
xattr -cr target/release/playa
```

### Out of memory during build

OpenEXR C++ compilation is memory-intensive. Close other applications or use exrs backend.

---

*For architecture details, see [AGENTS.md](AGENTS.md).*  
*For user documentation, see [README.md](README.md).*
