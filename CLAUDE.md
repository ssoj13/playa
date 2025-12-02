# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa is an image sequence player built in Rust. It supports EXR (via pure Rust `exrs` or C++ OpenEXR), PNG, JPEG, TIFF, TGA, and video formats (MP4, MOV, AVI, MKV via FFmpeg). Features async loading, OpenGL rendering, video encoding with hardware acceleration (NVENC, QSV, AMF), and a single-binary distribution.

## Build Commands

**Bootstrap (recommended for first-time setup):**
```powershell
.\bootstrap.ps1 build           # Build with exrs backend (pure Rust, fast)
.\bootstrap.ps1 build --openexr # Build with OpenEXR C++ backend (DWAA/DWAB support)
.\bootstrap.ps1 test            # Run all tests
```

**xtask commands (after bootstrap):**
```powershell
cargo xtask build                    # Release build with exrs
cargo xtask build --debug            # Debug build
cargo xtask build --openexr          # OpenEXR backend (requires C++ compiler/CMake)
cargo xtask test                     # Run all tests
cargo xtask test --nocapture         # Tests with output visible
cargo xtask verify                   # Verify dependencies
cargo xtask deploy                   # Install to system
```

**Release workflow:**
```powershell
cargo xtask tag-dev patch            # Create v0.1.x-dev tag (triggers CI build)
cargo xtask pr v0.1.60               # Create PR: dev -> main
cargo xtask tag-rel patch            # Create release tag (must be on main)
cargo xtask changelog                # Regenerate CHANGELOG.md
```

## Architecture

**Main Modules (`src/`):**
- `main.rs` - Entry point, CLI parsing, eframe app loop
- `player.rs` - Playback state (play/pause, FPS, frame navigation)
- `cache_man.rs` - Memory management with LRU eviction, epoch-based cancellation
- `workers.rs` - Thread pool for async frame loading
- `entities/` - Core data types (Frame, Project, Compositor, GPU compositor)
- `widgets/` - UI components (viewport, timeline, project panel, status bar)
- `dialogs/` - Modal dialogs (encode, preferences)
- `config.rs` - Path configuration and settings persistence

**Key Patterns:**
- **Epoch counter** (`AtomicU64`): Incremented on scrub/seek to cancel stale load requests
- **Arc<Mutex<FrameData>>**: Thread-safe frame data with status tracking
- **Channel-based workers**: `crossbeam-channel` for load requests/responses
- **egui_dock**: Dockable panels (viewport, timeline, project, attributes)

**xtask (`xtask/`):**
- Build automation using Rust (no external scripts needed)
- Handles OpenEXR header patching on Linux, dependency copying, release tagging

## Environment

- **VCPKG_ROOT**: `C:\vcpkg` (FFmpeg dependencies)
- **VCPKGRS_TRIPLET**: `x64-windows-static-md-release` (static linking)
- **Rust edition**: 2024 (Rust 1.85+)

## EXR Backend Selection

- **Default (exrs)**: Pure Rust, no external deps, cannot read DWAA/DWAB compressed files
- **OpenEXR feature**: `cargo build --features openexr` - requires C++ compiler, CMake, supports all EXR formats

## CI/CD

- **main.yml**: Release workflow triggered by `v*` tags
- **warm-cache.yml**: Pre-builds vcpkg cache for faster CI
- Uses `cargo-packager` for installers (NSIS, MSI, DMG, DEB, AppImage)
- macOS builds are code-signed and notarized
