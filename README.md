# Playa - Image Sequence Player

[![Build Status](https://github.com/ssoj13/playa/actions/workflows/release.yml/badge.svg)](https://github.com/ssoj13/playa/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/ssoj13/playa)](https://github.com/ssoj13/playa/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/ssoj13/playa/total)](https://github.com/ssoj13/playa/releases)
[![License](https://img.shields.io/github/license/ssoj13/playa)](LICENSE)
[![Lines of Code](https://img.shields.io/endpoint?url=https://ghloc.vercel.app/api/ssoj13/playa/badge?filter=.rs$&style=flat&label=Lines%20of%20Code)](https://github.com/ssoj13/playa)
[![Changelog](https://img.shields.io/badge/changelog-CHANGELOG.md-blue)](CHANGELOG.md)

Image sequence player for VFX workflows. Async loading, LRU caching, OpenGL rendering.

## Features

- **Multi-format support**: EXR, PNG, JPEG, TIFF, TGA
- **Async multi-threaded loading**: 75% of CPU cores for parallel frame loading
- **LRU caching**: Automatic memory management (50% of system RAM by default)
- **Epoch-based request cancellation**: Stale frame requests cancelled during scrubbing
- **Spiral preloading**: Frame preloading from current position
- **Load indicator**: Visual timeline bar showing frame load status (Header/Loading/Loaded/Error)
- **Interactive scrubbing**: Timeline navigation with mouse
- **Color-coded time slider**: Visual sequence boundaries with unique colors and dividers
- **Settings dialog**: TreeView-based preferences (F3) with dark/light theme and font size control
- **Viewport controls**: Zoom, pan, fit-to-window, 100% view
- **Custom shaders**: OpenGL shader support for display transformations
- **Resizable panels**: Playlist panel width persists across sessions (min 20px)
- **Playlist support**: Load and manage multiple sequences
- **JKL transport controls**: Playback controls
- **Cinema mode**: Fullscreen playback with hidden UI
- **Persistent settings**: Window state, playlists, panel sizes, and preferences saved across sessions

## Installation

### Download Pre-built Binaries

Download the latest release for your platform from the [Releases page](https://github.com/ssoj13/playa/releases/latest):

- **Windows**: `playa-x.x.x-x64-windows-setup.exe` (installer) or portable `.exe`
- **macOS**: `playa-x.x.x-x86_64-apple-darwin.dmg` or `playa-x.x.x-aarch64-apple-darwin.dmg`
- **Linux**: `playa-x.x.x-x86_64-linux.AppImage` or `.deb`

### Build from Source

**Prerequisites:**
- Rust 1.70+
- OpenEXR libraries (for EXR support)

```bash
git clone https://github.com/ssoj13/playa.git
cd playa

# Build with automatic dependency management (recommended)
cargo xtask build --release

# Or use the wrapper script
./build.sh
```

The compiled binary will be in `target/release/playa` (or `playa.exe` on Windows).

### Development Tools

For contributors and maintainers, these optional tools enhance the development workflow:

#### Required Tools (for releases and changelog)

```bash
# Cargo release automation
cargo install cargo-release

# Changelog generation from git commits
cargo install git-cliff
```

#### Optional Tools (for testing and code quality)

```bash
# Linting and formatting (usually via rustup)
rustup component add clippy rustfmt

# Security auditing
cargo install cargo-audit

# Code coverage (optional)
cargo install cargo-llvm-cov     # Recommended - uses LLVM coverage
cargo install cargo-tarpaulin    # Alternative - cross-platform
```

#### CI/CD Tools (auto-installed in GitHub Actions)

These are automatically installed in CI pipelines, but can be useful locally:

```bash
# Packaging tool (used in release workflow)
cargo install cargo-packager --version 0.11.7

# C/C++ compilation cache (optional, for faster openexr-sys builds)
# Installation: https://github.com/mozilla/sccache#installation
```

#### Quick Setup for Contributors

```bash
# Minimal setup for pull requests
rustup component add clippy rustfmt
cargo install git-cliff

# Full setup for maintainers
rustup component add clippy rustfmt llvm-tools-preview
cargo install cargo-release git-cliff cargo-llvm-cov cargo-audit
```

**Note**: `cargo-packager` is cached in CI and rarely needed locally unless testing packaging.

**About `cargo xtask`:**

`cargo xtask` is an idiomatic Rust build automation pattern using a helper binary crate in the workspace. It's a cross-platform alternative to Makefiles/shell scripts for handling build tasks that `cargo build` alone can't do (like copying native libraries, patching headers, or creating releases).

**What `cargo xtask build` does:**
- **Linux**: Automatically patches OpenEXR headers for GCC 11+ compatibility
- **All platforms**: Builds the project
- **All platforms**: Copies all required native libraries (OpenEXR, Imath, zlib) to the target directory
- **Linux**: Creates symlinks for proper library loading

**Alternative (manual build):**
```bash
# Manual build (requires manual library copying)
cargo build --release
cargo xtask post --release  # Copy native dependencies
```

### GitHub Actions CI/CD

The project uses GitHub Actions for automated builds on Windows and Linux. The CI/CD pipeline is optimized for speed and reliability with a multi-layered caching strategy.

#### Build Performance

**Initial build (cold cache):**
- cargo-packager compilation: ~5-6 minutes
- openexr-sys C++ compilation: ~30 minutes
- **Total: ~35 minutes**

**Subsequent builds (warm cache):**
- cargo-packager: ~10 seconds (from cache)
- openexr-sys: ~1-2 minutes (sccache)
- **Total: ~2-3 minutes** (93% faster!)

#### Caching Architecture

The pipeline uses three complementary caching layers:

**1. `baptiste0928/cargo-install` - Binary Tools Cache**
- **Purpose**: Cache pre-compiled `cargo-packager` binary
- **Location**: `~/.cargo-install/cargo-packager/`
- **Key**: `${{ runner.os }}-v1` (OS + manual version bump)
- **Why before rust-cache**: Prevents cache conflicts that delete the binary

**2. `Swatinem/rust-cache` - Rust Dependencies Cache**
- **Purpose**: Cache compiled Rust dependencies in `target/` and `~/.cargo/registry/`
- **Configuration**: `cache-bin: false` to avoid deleting cargo-packager
- **Why**: Standard rust-cache deletes `~/.cargo/bin` contents at cleanup if they appeared after its initialization

**3. `mozilla-actions/sccache-action` - C/C++ Compilation Cache**
- **Purpose**: Cache C++ object files from `openexr-sys` build.rs
- **Why needed**: openexr-sys compiles the entire OpenEXR C++ library (~30 min), which rust-cache can't help with
- **Requires**: `CARGO_INCREMENTAL=0` (set automatically by `dtolnay/rust-toolchain`) - incremental compilation conflicts with sccache
- **Fallback**: `continue-on-error: true` ensures builds succeed even if GitHub Cache API is down
- **Trade-off**: When GitHub Cache API is unavailable (rare), build takes full ~35 min but doesn't fail

**Note on CARGO_INCREMENTAL**: sccache works at the rustc compilation unit level, while incremental compilation caches at a different granularity. Having both enabled causes cache conflicts and negates sccache benefits. `dtolnay/rust-toolchain` automatically disables incremental compilation in CI environments for this reason.

#### Why This Order Matters

```yaml
1. Install sccache             # C++ compilation cache (optional, fails gracefully)
2. Install cargo-packager      # Binary tool (must install before rust-cache)
3. Cache cargo dependencies    # Rust deps (with cache-bin: false)
4. Build application           # Actual compilation
```

**Critical insight**: `Swatinem/rust-cache` tracks which files were in `~/.cargo/bin` at startup and deletes any new ones during cleanup. Installing cargo-packager *before* rust-cache ensures it's already present and won't be deleted.

**Design decisions**:
- **Stability over speed**: sccache is optional (continue-on-error) so builds don't fail when GitHub Cache API has issues
- **Incremental compilation disabled**: `CARGO_INCREMENTAL=0` (automatic via dtolnay/rust-toolchain) prevents conflicts with sccache
- **Cache key versioning**: Manual `-v1` suffix allows force-invalidation by bumping to `-v2`
- **No Cargo.lock in cache key**: Avoids cache invalidation on every dependency update (relies on rust-cache's smart detection instead)

#### Expected Behavior

**First run after code push**: ~35 minutes (builds everything, populates all caches)

**Subsequent runs**:
- Same code: ~2-3 minutes (all caches hit)
- Dependency update: ~5-10 minutes (rust-cache partial hit, sccache still helps with openexr-sys)
- cargo-packager version bump: ~7-8 minutes (recompile packager once, then cached)

**When GitHub Cache API is down**: ~35 minutes but build succeeds (sccache fails gracefully, falls back to normal compilation)

### Development Commands

**Available `cargo xtask` commands:**

```bash
# Build automation
cargo xtask pre                      # Patch OpenEXR headers (Linux only)
cargo xtask build [--release]        # Full build with dependency management
cargo xtask post [--release]         # Copy native libraries and shaders
cargo xtask verify [--release]       # Verify all dependencies present

# Release management
cargo xtask release patch            # Create patch release (0.1.13 -> 0.1.14)
cargo xtask release minor            # Create minor release (0.1.13 -> 0.2.0)
cargo xtask release major            # Create major release (0.1.13 -> 1.0.0)
cargo xtask release patch --dry-run  # Test release without committing
```

**Standard Rust development:**

```bash
# Testing
cargo test                           # Run all unit tests
cargo test --release                 # Run tests in release mode

# Documentation
cargo doc --open                     # Generate and open rustdoc documentation
cargo doc --no-deps --open           # Only document this crate

# Code quality
cargo clippy                         # Run linter (install: cargo install clippy)
cargo clippy -- -D warnings          # Treat warnings as errors
cargo fmt                            # Format code (install: rustup component add rustfmt)
cargo fmt -- --check                 # Check formatting without modifying

# Build variants
cargo build                          # Debug build
cargo build --release                # Release build (optimized)
cargo clean                          # Clean build artifacts (note: requires re-patching headers on Linux)
```

#### Linux-Specific Build Notes

**OpenEXR GCC 11+ Header Patching:**

OpenEXR 3.0.5 headers are missing `#include <cstdint>`, causing compilation errors with GCC 11+:
```
error: 'uint64_t' has not been declared
```

`cargo xtask pre` automatically patches 3 header files in `~/.cargo/registry/src/`:
- `ImfTiledMisc.h`
- `ImfDeepTiledInputFile.h`
- `ImfDeepTiledInputPart.h`

The patching is **idempotent** and **version-agnostic** - safe to run multiple times.

See: https://github.com/AcademySoftwareFoundation/openexr/issues/1157

**Native Libraries (7 Required):**

| Library | Purpose |
|---------|---------|
| OpenEXR Core (4 libs) | EXR reading/writing, utilities, threading, exceptions |
| Imath | Math library |
| Zlib | Compression |
| OpenEXR-C | C API wrapper from openexr-sys |

Libraries are copied from `target/release/lib/` and `target/release/build/openexr-sys-*/out/`.

**RPATH Configuration:**

`.cargo/config.toml` sets RPATH to `$ORIGIN`, so the executable searches for `.so` files in its own directory:
```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-Wl,-rpath,$ORIGIN"]
```

No `LD_LIBRARY_PATH` needed!

**Troubleshooting:**

Build fails with "uint64_t has not been declared":
```bash
cargo xtask pre
cargo build --release
```

Libraries not found when running:
```bash
cargo xtask verify --release
cargo xtask post --release  # If missing
```

After `cargo clean`:
```bash
cargo xtask build --release  # Re-patches automatically
```

## Usage

### Launch
```bash
# Start with empty player (drag-and-drop or file dialog)
playa

# Load specific file or sequence
playa path/to/image.0001.exr
```

### Keyboard Shortcuts

**Playback Controls:**
- `Space` - Play/Pause
- `J` / `,` / `←` - Jog backward / decrease speed
- `K` / `↓` - Stop playback / decrease FPS
- `L` / `.` / `→` - Jog forward / increase speed
- `↑` - Go to start
- `Ctrl+←` - Jump to start
- `Ctrl+→` - Jump to end
- `'` / `` ` `` - Toggle loop

**Viewport:**
- `F` - Fit to window (auto-fit mode)
- `A` / `1` / `Home` / `H` - 100% zoom
- `Mouse Wheel` - Zoom in/out (center on cursor)
- `Middle Mouse Drag` - Pan
- `Left Click + Drag` - Scrub timeline

**UI:**
- `F1` - Toggle help overlay
- `F2` - Toggle playlist panel
- `F3` - Toggle settings dialog
- `Z` - Toggle fullscreen (cinema mode)
- `ESC` - Exit fullscreen / Quit
- `Q` - Quit
- `Ctrl+R` - Reset settings to default

### Visual Sequence Navigation

The time slider provides visual feedback for multi-sequence playback:
- **Color-coded zones**: Each loaded sequence is displayed with a unique color on the timeline
- **Sequence boundaries**: White vertical dividers mark where sequences start/end
- **Load indicator bar**: Colored blocks below timeline show frame load status:
  - Dark gray: Placeholder (not requested)
  - Blue: Header only (detected but not loaded)
  - Orange: Currently loading
  - Green: Fully loaded
  - Red: Load error
- **Adaptive labels**: Sequence names appear on the timeline when space permits
- **Instant navigation**: Click or drag anywhere on the timeline to jump to that frame

This makes it easy to identify and navigate between different sequences in your playlist at a glance.

### Settings Dialog

Press `F3` to open the settings dialog with TreeView categories:

**UI Category:**
- **Font Size**: Adjust global UI font size (10-18px, default 13px)
- **Dark Mode**: Toggle between dark and light themes

Settings are automatically persisted to `playa.json`.

## Architecture

### Core Components

```
┌─────────────┐
│  PlayaApp   │  Main application (egui/eframe)
└──────┬──────┘
       │
       ├──── Player ───────┐
       │                   │
       │              ┌────▼────┐
       │              │  Cache  │  LRU cache + async loader + epoch counter
       │              └────┬────┘
       │                   │
       │              ┌────▼────────┐
       │              │  Sequences  │  Pattern-based frame lists
       │              └────┬────────┘
       │                   │
       │              ┌────▼────┐
       │              │ Frames  │  Individual images with status
       │              └─────────┘
       │
       ├──── Viewport ────┐
       │                  │
       │            ┌─────▼──────────┐
       │            │ ViewportState  │  Zoom/pan/fit modes
       │            └────────────────┘
       │
       ├──── Scrubber ────  Timeline interaction
       │
       ├──── TimeSlider ──  Custom time slider widget + load indicator
       │
       ├──── Shaders ─────  OpenGL display shaders
       │
       └──── Prefs ───────  Settings dialog with TreeView
```

### Module Breakdown

#### `main.rs`
Entry point and main application loop. Handles:
- CLI argument parsing
- Window initialization (egui/eframe)
- Event loop and UI rendering
- Keyboard/mouse input routing
- Settings persistence (JSON)
- Global font size application

#### `player.rs`
Playback state manager. Controls:
- Play/pause/stop
- Frame navigation (jog, shuttle)
- FPS control with presets
- Loop mode
- Delegates frame access to Cache

#### `cache.rs`
Intelligent caching system with multi-threaded architecture:
- **LRU eviction**: Manages memory budget (default 50% system RAM)
- **Epoch counter**: Atomic counter for cancelling stale load requests during scrubbing
- **Worker pool**: 75% of CPU cores for parallel loading
- **Load queue**: mpsc channel-based task distribution with epoch tagging
- **Preload thread**: Background spiral loading from current frame
- **Sequence management**: Multi-sequence playlist support
- **Frame status tracking**: Provides frame load state for visualization

**Caching strategy:**
1. On-demand loading: Loads frame when accessed
2. Spiral preload: Loads frames in order: 0, +1, -1, +2, -2, ...
3. Epoch-based cancellation: Workers skip requests with old epoch on scrub/seek
4. Memory-aware: Evicts least-recently-used frames when over budget
5. Status sync: Updates frame status (Header → Loading → Loaded/Error)

**Epoch Counter Pattern:**
- `current_epoch: Arc<AtomicU64>` increments on every scrub/seek
- Workers check `req.epoch != current_epoch` and skip stale requests
- Prevents wasted work on frames user has already moved past

#### `sequence.rs`
Pattern-based frame sequence detection:
- Auto-detects sequences from single file (e.g., `render.0001.exr` → `render.*.exr`)
- Glob pattern matching
- Frame number extraction with padding detection
- Directory scanning for multiple sequences
- Header-only resolution reading (fast)

#### `frame.rs`
Individual frame with thread-safe async loading:
- **Status states**: Placeholder → Header → Loading → Loaded/Error
- **Arc<Mutex<FrameData>>**: Thread-safe shared ownership
- **Format loaders**: EXR (OpenEXR), PNG/JPEG/TIFF (image-rs)
- **Color conversion**: Linear → sRGB for EXR
- **Green placeholder**: Visible indicator for unloaded frames
- **Status API**: `frame.status()` for load indicator visualization

#### `viewport.rs`
Display transformation and interaction:
- **Modes**: AutoFit (scales to window), Auto100 (1:1 pixels), Manual (user control)
- **Zoom**: Mouse wheel with cursor-centered scaling
- **Pan**: Middle-mouse drag
- **OpenGL rendering**: Custom shader pipeline

#### `scrub.rs`
Interactive timeline scrubbing:
- Left-click/drag to navigate frames
- Visual feedback (vertical line + frame number)
- Auto-pauses playback during scrub
- Maps mouse X to frame based on image bounds
- Triggers epoch counter increment for stale request cancellation

#### `timeslider.rs`
Custom time slider widget with sequence visualization:
- **Color-coded zones**: Each sequence rendered with unique color (hash-based)
- **Visual dividers**: Vertical lines marking sequence boundaries
- **Adaptive labels**: Sequence names/numbers displayed when space permits
- **Load indicator**: Colored blocks showing frame status (cached for performance)
- **Cache invalidation**: Uses `cached_frames_count()` to detect when to rebuild
- **Stateless immediate mode**: Fully synchronized with player state
- **Interactive**: Click/drag to navigate, automatic playhead tracking
- **HSV color generation**: Stable colors derived from sequence pattern hash

**Load Indicator Implementation:**
- Queries `cache.get_frame_stats()` for all frame statuses
- Caches result in `egui::Memory` with version key
- Invalidates cache when `cached_frames_count()` changes
- Draws colored blocks: Dark gray (Placeholder), Blue (Header), Orange (Loading), Green (Loaded), Red (Error)

#### `shaders.rs`
OpenGL shader management:
- Built-in shaders (default, checker, etc.)
- Custom shader loading from `shaders/` directory
- Runtime shader switching

#### `prefs.rs`
Settings dialog with TreeView navigation:
- **AppSettings struct**: Centralizes all user preferences
- **SettingsCategory enum**: General, UI categories
- **TreeView integration**: Uses `egui_ltreeview` for hierarchical navigation
- **Font size control**: Global UI font size (10-18px with live preview)
- **Theme toggle**: Dark/light mode switching
- **Persistence**: Selected category and all settings saved to JSON
- **Window layout**: 700×500 default, resizable with ScrollArea

## Data Flow

```
User Action (drag-drop / file dialog / CLI arg)
    │
    ▼
load_sequence(PathBuf)
    │
    ├──► cache.ingest(paths)
    │        │
    │        ├──► Sequence::detect() ──► Parse patterns
    │        │                           Extract frame numbers
    │        │                           Create Frame objects (status: Header)
    │        │
    │        └──► append_seq() ──────► Add to cache.sequences
    │                                   Update global frame range
    │                                   Rebuild frame_paths_cache
    │
    └──► signal_preload() ─────────► Preload thread wakes up
                                      Increments epoch counter
                                      Sends LoadRequests with current epoch

Playback Update Loop
    │
    ▼
player.update()
    │
    ├──► Advance frame based on FPS/direction
    │
    └──► cache.get_frame(idx)
             │
             ├──► Check LRU cache ───► HIT: update access time, return frame
             │
             └──► MISS: Send LoadRequest with current epoch
                         │
                         ▼
                  Worker threads (75% cores)
                         │
                         ├──► Check epoch ────► Stale? Skip request
                         │
                         ├──► frame.load() ─────► Detect format (EXR/PNG/etc)
                         │                        Update status: Loading
                         │                        Load pixels from disk
                         │                        Convert color space
                         │                        Update status: Loaded/Error
                         │
                         └──► Send LoadedFrame via channel
                                     │
                                     ▼
                              cache.process_loaded_frames()
                                     │
                                     ├──► Ensure space (LRU eviction)
                                     ├──► Insert into cache
                                     ├──► Update sequence frame reference
                                     └──► Send CacheMessage for UI updates

Scrub/Seek Event
    │
    ▼
    ├──► Increment epoch counter ────► Cancel all in-flight requests
    │
    └──► Trigger preload with new epoch

Render Loop
    │
    ▼
UI update
    │
    ├──► Apply global font size from settings
    │
    ├──► Apply theme (dark/light) from settings
    │
    ├──► Get current frame from cache
    │
    ├──► Upload texture to GPU (if frame changed)
    │
    ├──► TimeSlider with load indicator
    │        │
    │        ├──► Check cached_frames_count()
    │        ├──► Rebuild indicator cache if changed
    │        └──► Draw colored blocks for each frame
    │
    └──► ViewportRenderer.render()
             │
             └──► Apply viewport transform (zoom/pan)
                  Apply shader
                  Draw quad with texture

Settings Dialog (F3)
    │
    ▼
    ├──► TreeView navigation (General / UI)
    │
    ├──► Font size slider ───► Update AppSettings.font_size
    │                           Apply globally on next frame
    │
    ├──► Dark mode toggle ───► Update AppSettings.dark_mode
    │                           Switch theme immediately
    │
    └──► Auto-save to playa.json
```

## Performance Characteristics

- **Startup**: Instant (lazy loading)
- **Sequence detection**: Fast (header-only reads, ~1-5ms per file)
- **Frame loading**: Parallel (75% CPU cores)
- **Memory**: Self-limiting (50% system RAM, configurable)
- **Scrubbing**: Responsive (epoch-based cancellation + preloaded cache)
- **Playback**: Smooth (async loading stays ahead of playback)
- **Load indicator**: Efficient (cached, O(1) status lookups, rebuilds only on cache changes)
- **LRU cache**: Optimized (no stale keys in access_order, skips dead entries during eviction)

## Configuration

Settings auto-save to `playa.json` in the working directory:
- FPS
- Loop mode
- Shader selection
- Font size (global UI)
- Dark/light theme
- Viewport state (zoom/pan/mode)
- Playlist (sequence references)
- Window position/size
- Panel widths (playlist)
- Settings dialog state (selected category)

Cache state (sequences + current frame) auto-saves to `playa_cache.json` for instant restoration on restart.

## Technical Stack

- **UI**: egui 0.33 + eframe
- **TreeView**: egui_ltreeview 0.6.0 (with persistence feature)
- **Graphics**: OpenGL via glow + egui_glow
- **Image**: openexr 0.11 (EXR), image 0.25 (PNG/JPEG/TIFF)
- **Async**: std::thread + crossbeam-channel + mpsc
- **Concurrency**: AtomicU64 for epoch counter, Arc<Mutex> for shared state
- **CLI**: clap 4.5
- **Logging**: env_logger (set `RUST_LOG=debug` for verbose output)

## License

See LICENSE file for details.

## Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details on:
- Commit message conventions (Conventional Commits)
- Development workflow and tools
- Release process
- CI/CD architecture

See [CHANGELOG.md](CHANGELOG.md) for project history.

