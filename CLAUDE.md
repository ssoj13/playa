# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa is a cross-platform image sequence player written in Rust. Key features:
- Async frame loading with LRU cache and epoch-based cancellation
- OpenGL rendering via egui/eframe
- Two EXR backends: exrs (pure Rust, default) and OpenEXR (C++, optional, full DWAA/DWAB support)
- FFmpeg-based video playback/encoding with hardware acceleration (NVENC, QSV, AMF)
- Multi-sequence playlist with visual timeline
- Project/composition system for multi-layer blending

**Important**: This is on Windows. Always use PowerShell (`pwsh.exe -Command`) for commands, never bash.

## Build System

### Bootstrap Scripts (Recommended Entry Point)

**Always use PowerShell on Windows:**
```powershell
.\bootstrap.ps1                    # Show help
.\bootstrap.ps1 build              # Build with exrs (pure Rust, fast)
.\bootstrap.ps1 build --openexr    # Build with OpenEXR C++ backend
.\bootstrap.ps1 test               # Run all tests
```

Bootstrap scripts handle:
1. Rust installation check
2. vcpkg environment setup (`VCPKG_ROOT`, `VCPKGRS_TRIPLET`)
3. Visual Studio environment configuration
4. cargo-binstall, cargo-release, cargo-packager installation
5. xtask binary compilation
6. Command forwarding to xtask

### xtask - Build Automation

After bootstrap, use `cargo xtask` directly:

```powershell
# Build commands
cargo xtask build [--release] [--debug] [--openexr]
cargo xtask post [--release]       # Copy OpenEXR libs (post-build)
cargo xtask verify [--release]     # Verify dependencies present

# Testing
cargo xtask test [--debug] [--nocapture]

# Release management
cargo xtask tag-dev [patch|minor|major]    # Create v0.1.x-dev tag → Build workflow
cargo xtask tag-rel [patch|minor|major]    # Create v0.1.x tag → Release workflow (main branch only)
cargo xtask pr [version]                   # Create PR: dev → main
cargo xtask changelog                      # Regenerate CHANGELOG.md

# Utilities
cargo xtask deploy [--install-dir PATH]    # Install to system
cargo xtask wipe [--dry-run] [-v]          # Clean target/*.exe, *.dll, *.so
cargo xtask wipe-wf                        # Delete all GitHub Actions runs
```

**Build process (OpenEXR backend)**:
1. Pre-build: Patch OpenEXR headers (Linux GCC 11+) or zlib (macOS)
2. Build: `cargo build [--release] --features openexr`
3. Post-build: Copy native libs + shaders to target/{profile}/

**Build process (exrs backend)**:
- Single step: `cargo build [--release]` (pure Rust, no dependencies)

### Environment Variables

**vcpkg** (required for FFmpeg):
- `VCPKG_ROOT` - Path to vcpkg installation (default: C:\vcpkg on Windows)
- `VCPKGRS_TRIPLET` - Platform triplet (e.g., x64-windows-static-md-release)
- `PKG_CONFIG_PATH` - Auto-set by bootstrap: `$VCPKG_ROOT/installed/$VCPKGRS_TRIPLET/lib/pkgconfig`

## Architecture

### Module Structure

```
src/
├── main.rs              # Application entry, eframe setup, CLI parsing, dock layout
├── player.rs            # Playback state (play/pause, FPS, frame navigation)
├── workers.rs           # Global worker pool (rayon) for async tasks
├── events.rs            # Event bus for comp/layer/frame changes
├── ui.rs                # Top-level UI composition (dock tabs)
├── entities/
│   ├── project.rs       # Top-level project (playlist + compositions)
│   ├── comp.rs          # Composition (sequence or multi-layer blend)
│   ├── frame.rs         # Individual frame with async loading
│   ├── compositor.rs    # CPU compositor for multi-layer blending
│   ├── loader.rs        # Frame loader (EXR/PNG/JPEG/TIFF via image-rs)
│   ├── loader_video.rs  # Video loader (FFmpeg)
│   └── attrs.rs         # Global project attributes (FPS defaults, etc.)
├── widgets/
│   ├── viewport/        # OpenGL rendering, zoom/pan, shaders
│   ├── timeline/        # Timeline slider + scrubbing + load indicator
│   ├── project/         # Project/playlist panel
│   ├── status/          # Status bar
│   └── ae/              # After Effects-style layer stack UI
├── dialogs/
│   ├── prefs/           # Settings dialog with TreeView
│   └── encode/          # Video encoding dialog
└── utils/
    └── sequences.rs     # Sequence pattern detection (frame.0001.exr)

xtask/
├── main.rs              # CLI commands (build, test, release)
├── pre_build.rs         # Platform-specific patching (OpenEXR headers)
├── post_build.rs        # Copy native libs + shaders
├── lib_discovery.rs     # Find and verify OpenEXR/Imath/zlib libs
└── release.rs           # cargo-release integration
```

### Key Concepts

**Project → Comp → Frame hierarchy**:
- `Project`: Top-level container (serialized to .json)
  - `media: HashMap<UUID, Comp>` - All compositions/clips
  - `comps_order: Vec<UUID>` - UI order
  - `compositor: CompositorType` - CPU compositor (GPU future)
- `Comp`: Composition (sequence OR multi-layer blend)
  - File mode: Single sequence (e.g., render.####.exr)
  - Layer mode: Multiple layers with blend modes + opacity
  - `compose(frame_idx)` → blends layers → returns single Frame
- `Frame`: Individual image with async loading
  - Status: Placeholder → Header → Loading → Loaded/Error
  - `PixelBuffer`: U8/F16/F32 pixel data (RGBA)

**Worker pool** (`workers.rs`):
- Global rayon threadpool (75% CPU cores)
- Shared across frame loading + video encoding
- Uses `crossbeam::channel` for task distribution

**Event system** (`events.rs`):
- `CompEvent`: Frame changed, layer added/removed, cache invalidated
- `EventBus`: App-wide events (project loaded, settings changed)
- Decouples UI updates from data changes

**Epoch counter** (async frame loading):
- `Arc<AtomicU64>` increments on scrub/seek
- Workers check epoch and skip stale requests
- Prevents wasted work during fast scrubbing

### Data Flow

```
User drags file → load_sequence()
  ↓
Sequence::detect() → Parse pattern (frame.####.exr)
  ↓
Create Comp (File mode) → Add to Project.media
  ↓
Comp.build_cache() → Spawn frame loader workers
  ↓
Worker thread: Frame.load() → image-rs or FFmpeg
  ↓
Send LoadedFrame via channel → Update cache
  ↓
UI: Comp.get_frame(idx) → Returns cached/loading/error frame
  ↓
Viewport: Upload texture → Render with shader
```

**Multi-layer blending**:
```
Comp (Layer mode) with 3 layers:
  Layer 0: Base image (opacity 1.0)
  Layer 1: Overlay (opacity 0.5, blend mode: Normal)
  Layer 2: Adjustment (opacity 0.3)
    ↓
Comp.compose(frame_idx):
  1. Get Frame from each layer's cache
  2. Convert all to same pixel format (F32)
  3. Compositor.blend(vec![(frame0, 1.0), (frame1, 0.5), (frame2, 0.3)])
  4. Return blended Frame
```

## Development Workflow

### Local Development

```powershell
# Quick iteration (debug build, exrs)
.\bootstrap.ps1 build
.\target\debug\playa.exe

# Full OpenEXR support (debug)
.\bootstrap.ps1 build --debug --openexr
.\target\debug\playa.exe

# Release build
.\bootstrap.ps1 build --release --openexr
.\target\release\playa.exe
```

### Testing

```powershell
# Run all tests (unit + integration)
.\bootstrap.ps1 test

# Run with output visible
cargo xtask test --nocapture

# Run specific test
cargo test --test integration_test -- test_name
```

**Test structure**:
- Unit tests: Inline `#[cfg(test)]` modules
- Integration tests: `tests/*.rs` (encoding, cache, sequence detection)

### Release Process

**Dev builds** (testing CI artifacts):
```powershell
# 1. Create dev tag (triggers Build workflow, NOT Release)
cargo xtask tag-dev patch              # v0.1.59 → v0.1.60-dev

# 2. GitHub Actions builds both backends (exrs + OpenEXR)
# 3. Download artifacts from Actions tab to test locally
```

**Production releases** (from main branch):
```powershell
# 1. Create PR from dev to main
cargo xtask pr v0.1.60

# 2. Merge PR on GitHub UI

# 3. Switch to main and pull
git checkout main
git pull

# 4. Create release tag (triggers Release workflow + GitHub Release)
cargo xtask tag-rel patch              # v0.1.59 → v0.1.60
```

**CI/CD**:
- `.github/workflows/main.yml` - Unified Build/Release workflow
  - `check-branch` job: Detects if tag is on main (release) or dev (build)
  - Conditional installers: Only on main branch releases
- `.github/workflows/warm-cache.yml` - Pre-warms Cargo/vcpkg caches
- `.github/workflows/_build-backend.yml` - Reusable backend build (exrs/OpenEXR)
- `.github/workflows/_build-platform.yml` - Reusable platform build (Windows/Linux/macOS)

### Code Style

- **Rust edition 2024** (requires Rust 1.85+)
- **Error handling**: `anyhow::Result` for application errors, `Result<T, String>` for domain errors
- **Logging**: Use `log::debug!()`, `log::info!()`, `log::warn!()`, `log::error!()`
- **Concurrency**: Prefer `crossbeam::channel` over `std::sync::mpsc`, use `Arc<Mutex<T>>` for shared state
- **Comments**: Concise explanatory comments for non-obvious logic, module-level `//!` doc comments
- **Naming**: Short meaningful names (e.g., `get_tr()` instead of `extract_translation()`)

### Common Pitfalls

1. **Windows bash**: Never use bash on Windows - always use PowerShell (`pwsh.exe -Command`)
2. **vcpkg paths**: Bootstrap script sets up environment - don't hardcode paths
3. **OpenEXR headers**: Linux GCC 11+ needs patching (`cargo xtask pre`) before build
4. **Epoch counter**: Always increment when user seeks/scrubs to cancel stale loads
5. **Frame status**: Check `frame.status()` before rendering (may be Loading/Error)
6. **Compositor**: Must convert all frames to same pixel format before blending
7. **Event bus**: Use `CompEvent` for cache updates to trigger UI refresh

## FFmpeg Integration

**Dependencies** (via vcpkg):
- Static linking: `x64-windows-static-md-release` triplet
- Features: `core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec`

**Hardware encoders** (video encoding):
- `h264_nvenc` / `hevc_nvenc` - NVIDIA (GTX 600+)
- `h264_qsv` / `hevc_qsv` - Intel Quick Sync
- `h264_amf` / `hevc_amf` - AMD
- `libx264` / `libx265` - CPU fallback

**Video playback**:
- `playa-ffmpeg` crate wraps rust-ffmpeg bindings
- `loader_video.rs` handles demuxing + decoding
- Frames stored in cache like image sequences

## Debugging

**Enable logging**:
```powershell
$env:RUST_LOG="debug"
.\target\debug\playa.exe --log playa.log
```

**Common log patterns**:
- `[entities::frame]` - Frame loading status
- `[workers]` - Worker pool task distribution
- `[entities::comp]` - Composition cache operations
- `[widgets::viewport]` - OpenGL texture uploads

**Performance profiling**:
- Timeline shows load indicator (Placeholder/Header/Loading/Loaded/Error)
- Status bar displays cache stats (loaded frames / total)
- `--vvv` flag enables trace-level logging (very verbose)

## Platform-Specific Notes

**Windows**:
- vcpkg default: `C:\vcpkg`
- Visual Studio environment: Auto-configured by bootstrap.ps1
- Installer: NSIS (.exe) + MSI (.msi)

**Linux**:
- OpenEXR headers need patching for GCC 11+ (`cargo xtask pre`)
- RPATH set in `.cargo/config.toml` for runtime library loading
- Installer: AppImage + DEB

**macOS**:
- Code signing: Developer ID Application certificate
- Notarization: Handled in CI (requires APPLE_ID + password in secrets)
- zlib patching for OpenEXR compatibility
- Installer: DMG (drag-to-Applications)

## External Dependencies

**Rust crates** (key dependencies):
- `eframe`/`egui` 0.33 - UI framework
- `egui_glow` - OpenGL backend
- `egui_dock` 0.18 - Docking layout
- `image` 0.25 - PNG/JPEG/TIFF/TGA/HDR + exrs
- `openexr` 0.11 - Optional C++ OpenEXR bindings
- `playa-ffmpeg` 8.0.3 - FFmpeg bindings
- `rayon` 1.11 - Parallel iterator / worker pool
- `crossbeam` 0.8.4 - Channel + atomic utilities

**Native libraries** (OpenEXR feature):
- OpenEXR 3.0.5 (4 libs: core, util, thread, exc)
- Imath 3.x
- Zlib
- openexr-c (from openexr-sys)

**Build tools**:
- `cargo-binstall` - Fast binary installation
- `cargo-release` - Version bumping + changelog
- `cargo-packager` 0.11.7 - Cross-platform installers
- `git-cliff` - Changelog generation

## Tips

- Use `cargo xtask wipe` to clean stale artifacts before switching backends (exrs ↔ OpenEXR)
- Test both backends when changing frame loading logic
- Profile with `--release` builds - debug builds are 10-100x slower
- Check GitHub Actions logs for CI build issues (especially vcpkg cache)
- Use `--dry-run` with release commands to preview changes
- Timeline load indicator helps debug async loading issues
