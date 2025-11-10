# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa is a cross-platform image sequence player for VFX workflows built with Rust, egui, and OpenGL. It supports EXR, PNG, JPEG, TIFF formats with async loading, LRU caching, and hardware-accelerated video encoding via FFmpeg.

## Build Commands

### Quick Start

```bash
# Windows
bootstrap.cmd              # Show help
bootstrap.cmd build        # Build with exrs (pure Rust)
bootstrap.cmd test         # Run encoding integration test
bootstrap.cmd install      # Install playa from crates.io (with FFmpeg setup)

# macOS / Linux
./bootstrap.sh             # Show help
./bootstrap.sh build       # Build with exrs (pure Rust)
./bootstrap.sh test        # Run encoding integration test
./bootstrap.sh install     # Install playa from crates.io (with FFmpeg setup)
```

### Build Backends

```bash
# Pure Rust backend (default) - no external dependencies
cargo build --release

# OpenEXR backend (C++) - supports DWAA/DWAB compression
cargo xtask build --release --openexr

# Debug builds
cargo build
cargo xtask build --debug --openexr
```

### Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_encode_placeholder_frames

# Run encoding integration test (creates test_encode_output.mp4)
cargo test --release test_encode_placeholder_frames -- --nocapture
```

### Code Quality

```bash
cargo fmt                           # Format code
cargo clippy -- -D warnings         # Lint with warnings as errors
```

### Release Workflow

```bash
# Step 1: Create dev release from dev branch
cargo xtask tag-dev patch           # 0.1.23 -> 0.1.24
cargo xtask tag-dev minor           # 0.1.23 -> 0.2.0
cargo xtask tag-dev major           # 0.1.23 -> 1.0.0

# Step 2: Test artifacts from GitHub Actions

# Step 3: Merge to main → Release workflow triggers automatically

# Manual changelog regeneration
cargo xtask changelog
```

## Architecture

### Core Components

**Frame Pipeline** (`src/frame.rs`, `src/sequence.rs`, `src/cache.rs`):
- `Frame`: Individual image with lazy loading (Header → Loaded states)
- `Sequence`: Collection of frames with metadata (path pattern, resolution, frame range)
- `Cache`: LRU cache with background worker pool for async loading
  - Uses crossbeam channels for work distribution
  - Maintains memory budget based on system RAM
  - Evicts least-recently-used frames when full

**Player** (`src/player.rs`):
- Central state manager coordinating Cache, Viewport, UI
- Handles playback loop, JKL shuttle controls, frame navigation
- Manages multiple sequences in playlist

**Viewport** (`src/viewport.rs`):
- OpenGL renderer using egui_glow
- Supports zoom/pan, fit-to-window, 100% pixel view
- Custom GLSL shader pipeline loaded from `shaders/` directory

**Video Support** (`src/video.rs`, `playa-ffmpeg`):
- FFmpeg-based video decoding (MP4, MOV, AVI, MKV)
- Frame-level seeking with on-demand YUV→RGBA conversion
- Videos treated as sequences internally (`video.mp4@17` notation)

**Encoding** (`src/encode.rs`, `src/ui_encode.rs`):
- Hardware-accelerated encoding (NVENC, QSV, AMF) with CPU fallback
- Multi-threaded background encoding with progress updates
- Automatic RGB→YUV420P conversion for hardware encoders
- Cancellable with 2-second timeout and force reset

### Key Design Patterns

**Async Frame Loading**:
```rust
// Cache spawns worker threads that pull load requests from channel
// Frames transition: Placeholder → Header → Loaded
let (tx, rx) = crossbeam_channel::unbounded();
// Workers call frame.load() which delegates to format-specific loaders
```

**Video Frame Addressing**:
```rust
// Videos use @N suffix for frame indexing
// Example: "video.mp4@17" = frame 17 of video.mp4
// Sequence::from_video_file() creates virtual sequence with frame count
```

**EXR Backend Selection**:
```rust
#[cfg(feature = "openexr")]
use crate::exr::OpenExr as ExrBackend;
#[cfg(not(feature = "openexr"))]
use crate::exr::Exr as ExrBackend;
// Abstracted via trait for compile-time backend switching
```

**Encoding Cancellation**:
```rust
// AtomicBool shared between UI and encoder thread
// Checked at multiple points: after send_frame(), in receive_packet() loops
// UI waits 2 seconds for join(), then force-resets state
```

## FFmpeg Integration

### Required Environment Variables

**Windows (development)**:
```powershell
setx VCPKG_ROOT "C:\vcpkg"
setx VCPKGRS_TRIPLET "x64-windows-static-md"
```

**CI/CD (all platforms)**:
```yaml
VCPKG_ROOT: C:\vcpkg (Windows) / /usr/local/share/vcpkg (Unix)
VCPKGRS_TRIPLET: x64-windows-static-md-release (Windows)
                 x64-linux-release (Linux)
                 arm64-osx-release / x64-osx-release (macOS)
PKG_CONFIG_PATH: $VCPKG_ROOT/installed/$TRIPLET/lib/pkgconfig
```

**FFmpeg Features** (vcpkg):
```bash
# Required features (same across all platforms)
ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]

# Windows/Linux: includes nvcodec (NVIDIA hardware acceleration)
# macOS: excludes nvcodec (not supported)
```

### Critical Timestamp Handling

When encoding video, ensure proper MP4 metadata:
```rust
// Set stream time_base to match encoder
ost.set_time_base(encoder.time_base());

// Set packet duration and DTS for timeline scrubbing
encoded.set_duration(1);  // 1 frame in time_base units
if encoded.dts().is_none() {
    encoded.set_dts(encoded.pts());  // For I-frame sequences
}
```

## File Structure

```
src/
├── main.rs              # Entry point, CLI args, window setup
├── player.rs            # Central state manager
├── cache.rs             # LRU cache with async workers
├── sequence.rs          # Frame collection with metadata
├── frame.rs             # Individual frame with lazy loading
├── viewport.rs          # OpenGL renderer
├── ui.rs                # Main UI layout (playlist, controls)
├── encode.rs            # Video encoding logic
├── ui_encode.rs         # Encoding dialog UI
├── video.rs             # FFmpeg video decoding
├── exr.rs               # EXR backend abstraction
├── timeslider.rs        # Timeline scrubber widget
├── shaders.rs           # GLSL shader management
└── prefs.rs             # Persistent settings

xtask/                   # Build automation (cargo xtask)
bootstrap.{cmd,sh}       # Project setup scripts
cliff.toml               # git-cliff changelog config
Cargo.toml               # Workspace + metadata
```

## Testing

### Integration Tests

**Encoding Test** (`src/encode.rs::tests`):
- Creates 100 placeholder frames (640x480 green RGBA)
- Encodes frames 10-49 (40 frames) to test_encode_output.mp4
- Tests encoder discovery, frame conversion, progress updates
- Verifies output file exists and is non-empty

**Test Execution**:
```bash
# Quick test (uses first available encoder)
cargo test test_encode_placeholder_frames

# Full output with encoder selection
cargo test --release test_encode_placeholder_frames -- --nocapture
```

## Common Pitfalls

1. **FFmpeg build failures**: Ensure `VCPKGRS_TRIPLET` is set before `cargo build`
2. **Missing timeline in MP4**: Always set stream time_base, packet duration, and DTS
3. **Encoding hangs on cancel**: Check cancel_flag in all encode loops (send_frame, receive_packet)
4. **Different frame sizes**: Validation happens before encoding, returns `InconsistentFrameSizes` error
5. **OpenEXR DWAA/DWAB**: Requires `--openexr` feature, not available in pure Rust backend

## Commit Conventions

Follow [Conventional Commits](https://www.conventionalcommits.org/):
```
feat: Add HDR tone mapping support
fix: Resolve memory leak in image cache
docs: Update build instructions for Windows
chore: Bump image crate to 0.25
perf: Optimize EXR decoding with parallel loading
```

CHANGELOG.md is auto-generated by git-cliff during release process.

## CI/CD Notes

- **Build time**: ~35min cold cache, ~2-3min warm cache
- **Caching**: rust-cache for dependencies, sccache for C++ (Linux only)
- **Binary tools**: cargo-binstall used instead of compilation (cargo-packager)
- **Release triplets**: Use `-release` suffix for optimized static builds
- **Windows installer**: NSIS with perMachine installation mode
- **macOS signing**: Code-signed with Developer ID, notarized by Apple

## Bootstrap Script Workflow

1. Verifies Rust/Cargo installation
2. Configures Visual Studio environment (Windows)
3. Sets up vcpkg environment variables (if available)
4. Installs cargo-binstall, cargo-release, cargo-packager
5. Builds xtask if needed
6. Executes requested command or shows help
