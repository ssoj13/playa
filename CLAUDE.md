# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa is an image sequence player for VFX workflows. Rust + egui + OpenGL. Cross-platform (Windows, macOS, Linux).

**Key features**: EXR/PNG/JPEG/TIFF support, video playback (FFmpeg), video encoding (H.264/H.265/AV1), GPU compositing, multi-layer timeline, LRU frame caching, custom GLSL shaders.

## Build Commands

```powershell
# Bootstrap (first time setup - installs cargo-release, cargo-packager)
.\bootstrap.ps1 build              # Windows PowerShell
./bootstrap.sh build               # Linux/macOS

# Build with exrs backend (default, pure Rust, fast)
cargo xtask build                  # Release
cargo xtask build --debug          # Debug

# Build with OpenEXR C++ backend (DWAA/DWAB support)
cargo xtask build --openexr        # Release
cargo xtask build --debug --openexr

# Direct cargo (skips xtask automation)
cargo build --release              # exrs only
cargo build --release --features openexr

# Run tests
cargo xtask test                   # Release mode
cargo xtask test --debug           # Debug mode
cargo xtask test --nocapture       # Show println! output

# Other xtask commands
cargo xtask --help                 # Full command list
cargo xtask verify                 # Check dependencies present
cargo xtask deploy                 # Install to system
cargo xtask changelog              # Regenerate CHANGELOG.md
cargo xtask tag-dev patch          # Create dev tag (v0.1.x-dev)
cargo xtask tag-rel patch          # Create release tag (from main)
cargo xtask pr v0.1.x              # Create PR dev->main
```

## Architecture

```
src/
├── main.rs           # eframe::App entry point (PlayaApp struct)
├── lib.rs            # Library crate re-exports
├── core/             # Engine (cache, events, player, workers)
│   ├── cache_man.rs  # Memory manager with LRU eviction
│   ├── event_bus.rs  # Type-erased event system
│   ├── global_cache.rs # HashMap<comp_uuid, HashMap<frame_idx, Frame>>
│   ├── player.rs     # Playback state machine (JKL controls)
│   └── workers.rs    # Work-stealing thread pool
├── entities/         # Data models
│   ├── comp.rs       # Composition (timeline container with children)
│   ├── frame.rs      # Frame buffer (U8/F16/F32), loading, tonemap
│   ├── project.rs    # Project container (media library, active comp)
│   ├── attrs.rs      # Generic key-value attribute storage
│   ├── compositor.rs # CPU frame blending
│   └── gpu_compositor.rs # GPU compositing via OpenGL
├── widgets/          # UI components
│   ├── viewport/     # Image display, pan/zoom, shader preview
│   ├── timeline/     # Timeline editor, layers, work area
│   ├── project/      # Project/playlist panel
│   ├── status/       # Status bar, memory usage
│   └── ae/           # Attribute editor (After Effects style)
├── dialogs/          # Modal windows
│   ├── prefs/        # Preferences (cache, playback, hotkeys)
│   └── encode/       # FFmpeg export dialog
└── cli.rs, config.rs, shell.rs, ui.rs, utils.rs

xtask/                # Build automation (cargo xtask commands)
```

## Key Patterns

**Event-driven communication**: Components emit typed events via `EventBus`, handled in `main_events.rs`. Avoids tight coupling.

**Frame loading pipeline**: `Player::get_current_frame()` → `GlobalFrameCache` lookup → cache miss → `Workers` background load → epoch check → insert cache → viewport picks up.

**Epoch-based cancellation**: `AtomicU64` counter increments on scrub/seek. Workers skip stale requests where `req.epoch != current_epoch`.

**Memory management**: `CacheManager` tracks global usage. `GlobalFrameCache` evicts LRU frames when limit exceeded (default 50% system RAM).

**Attrs system**: Generic key-value storage (`entities/attrs.rs`) for entity metadata. All persistent settings flow through `Attrs` with automatic dirty tracking.

## FFmpeg Integration

Requires vcpkg with FFmpeg. Environment variables:
- `VCPKG_ROOT` - vcpkg installation path
- `VCPKGRS_TRIPLET` - Platform triplet (e.g., `x64-windows-static-md-release`)

The `playa-ffmpeg` crate (workspace dependency) wraps FFmpeg for video decode/encode.

## Testing

```powershell
cargo test                         # All tests
cargo test --lib                   # Unit tests only
cargo test --test integration      # Integration tests only
cargo test cache::                 # Tests matching pattern
```

## Crate Features

- `default` - exrs backend (pure Rust EXR)
- `openexr` - OpenEXR C++ backend (DWAA/DWAB compression, requires C++ compiler/CMake)

## Dependencies

Key crates: `eframe`/`egui` (UI), `glow` (OpenGL), `image` (format loaders), `playa-ffmpeg` (video), `crossbeam-channel` (async), `sysinfo` (memory tracking), `uuid` (entity IDs).

## Platform Notes

**Windows**: Use PowerShell. VCPKG at `c:\vcpkg`. Visual Studio environment required for OpenEXR builds.

**Linux**: May need `cargo xtask pre` to patch OpenEXR headers for GCC 11+.

**macOS**: Code signing configured in `Cargo.toml` metadata for DMG builds.
