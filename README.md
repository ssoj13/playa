# Playa - Image Sequence Player

[![Release Status](https://github.com/ssoj13/playa/actions/workflows/main.yml/badge.svg?event=push)](https://github.com/ssoj13/playa/actions/workflows/main.yml)
[![Warm Cache Status](https://github.com/ssoj13/playa/actions/workflows/warm-cache.yml/badge.svg?event=push)](https://github.com/ssoj13/playa/actions/workflows/warm-cache.yml)
[![Release](https://img.shields.io/github/v/release/ssoj13/playa)](https://github.com/ssoj13/playa/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/ssoj13/playa/total)](https://github.com/ssoj13/playa/releases)
[![License](https://img.shields.io/github/license/ssoj13/playa)](LICENSE)
[![Lines of Code](https://img.shields.io/endpoint?url=https://ghloc.vercel.app/api/ssoj13/playa/badge?filter=.rs$&style=flat&label=Lines%20of%20Code)](https://github.com/ssoj13/playa)
[![Changelog](https://img.shields.io/badge/changelog-CHANGELOG.md-blue)](CHANGELOG.md)

**Small note**: This is a learning project. I'm really excited to discover the Rust universe and the rise of AI agentic coding techniques to quickly learn a new stack. I perfectly know what I want to build and supposed app architecture, but implementing that alone would be probably not possible within some reasonable timeframe (not within a week, definitely). Well, also now Rust users and open source community now have a half-decent cross-platform image sequence player made of a single binary. I really wanted to express my gratitude towards creators and maintainers of `exrs` and `openexr-rs` crates and of course the rest - Rust is amazing!

Short list of things resolved while building this tool:


![Screenshot](.github/screenshot.png)

Image sequence player for VFX workflows. Async loading, LRU caching, OpenGL rendering.

## Features

- **Dual EXR backends**: Choose between pure Rust (exrs) for fast builds or OpenEXR C++ for full DWAA/DWAB compression support
- **Native Rust Multi-format support**: EXR, PNG, JPEG, TIFF, TGA with fast parallel loading
- **HDR pixel precision**: Support for 8 / 16 / half-float / 32-bit float images
- **Drag-and-drop**: Drop any image file - automatically detects and loads the entire sequence
- **Smart sequence detection**: Load one frame (e.g., `render.0001.exr`) - finds all frames automatically
- **Persistent playlist**: Load multiple sequences, auto-saves and restores between sessions
- **Color-coded timeline**: Visual sequence boundaries with real-time frame load indicators
- **Responsive scrubbing**: Instant frame navigation - always responsive even during fast scrubbing, cancels stale loads automatically
- **Playback controls**: Standard transport controls (play/pause, JKL shuttle, loop)
- **Viewport controls**: Zoom, pan, fit-to-window, 100% pixel-perfect view, cursor-centered zoom
- **Custom GLSL shaders**: Load display shaders from `shaders/` directory - LUTs, color transforms, custom effects
- **Smart memory management**: Automatically manages cache size - never runs out of memory
- **Settings dialog**: Theme switching, font size, preferences (F3)
- **Cinema mode**: Fullscreen playback with hidden UI
- **Persistent settings**: Everything saves automatically - window layout, zoom level, shader selection

## Installation

### Classic Installation: cargo install

The standard Rust way - install directly from crates.io:

```bash
cargo install playa
```

**Backend comparison:**
- **exrs**: pure Rust, single binary, no external dependencies, fast startup
- **openexr**: Binary + native libraries (DLLs/.so files), full DWAA/DWAB support (see "Build from Source" below)

### Download Pre-built Binaries

Download the latest release from the [Releases page](https://github.com/ssoj13/playa/releases/latest):

**macOS (recommended: DMG):**
- 🎯 `playa-x.x.x-exrs.dmg` - **Recommended** - Drag to Applications (code-signed & notarized)
- `playa-x.x.x-openexr.dmg` - With DWAA/DWAB compression support (code-signed & notarized)
- Portable: `playa-exrs-aarch64-apple-darwin.zip` (single binary)

**Linux (recommended: AppImage):**
- 🎯 `playa-x.x.x-exrs.AppImage` - **Recommended** - Universal, runs everywhere
- `playa-x.x.x-exrs.deb` - Debian/Ubuntu package
- Portable: `playa-exrs-x86_64-unknown-linux-gnu.zip` (single binary)
- OpenEXR variants: `-openexr.AppImage` / `-openexr.deb` with DWAA/DWAB support

**Windows (choose one):**
- 🎯 `playa-x.x.x-exrs-x64-setup.exe` - **Installer** - System integration
- `playa-x.x.x-exrs-x64.msi` - **MSI** - Enterprise deployments
- `playa-exrs-x86_64-pc-windows-msvc.zip` - **Portable** - Single .exe (no DLLs)
- OpenEXR variants: `-openexr-` prefix - Include DLLs for DWAA/DWAB compression

**macOS Security Note:**
All DMG releases are code-signed with Developer ID and notarized by Apple. No Gatekeeper warnings - just drag to Applications and run.


### Build from Source

Playa supports two EXR backends:

| Backend | Build Command | Dependencies | DWAA/DWAB Support |
|---------|--------------|--------------|-------------------|
| **exrs** (default) | `cargo build --release` | None (pure Rust) | No |
| **OpenEXR** (optional) | `cargo xtask build --release --openexr` | C++ compiler, CMake | Yes |

#### Option 1: Default Build (exrs - Pure Rust)

Fast build with no external dependencies. Suitable for most workflows:

```bash
git clone https://github.com/ssoj13/playa.git
cd playa

# Build with exrs backend (pure Rust, no DLLs)
cargo build --release
```

The compiled binary will be in `target/release/playa` (or `playa.exe` on Windows).

**Limitations**: Cannot load EXR files with DWAA/DWAB compression. Will show helpful error message with build instructions.

#### Option 2: Full OpenEXR Support (C++ Backend)

Supports all EXR compression formats including DWAA/DWAB:

**Prerequisites:**
- Rust 1.85+ (edition 2024)
- C++ compiler and CMake

```bash
git clone https://github.com/ssoj13/playa.git
cd playa

# Build with OpenEXR backend (full format support)
cargo xtask build --release --openexr
```

**Note:** OpenEXR backend compiles C++ libraries (~5-10 minutes first build, then cached).

## Quick Start (New Contributors)

**Start here!** Bootstrap scripts handle all dependencies automatically:

### Windows
```cmd
bootstrap.cmd              # Show xtask help
bootstrap.cmd build        # Build with exrs (fast)
bootstrap.cmd build --openexr  # Build with full OpenEXR support
```

### Linux/macOS
```bash
./bootstrap.sh             # Show xtask help
./bootstrap.sh build       # Build with exrs (fast)
./bootstrap.sh build --openexr  # Build with full OpenEXR support
```

**What bootstrap does:**
1. Checks Rust installation (exits with error if missing)
2. Auto-installs dependencies via `cargo-binstall` (faster than `cargo install`):
   - `cargo-release` - Version bumping and changelog generation
   - `cargo-packager` v0.11.7 - Cross-platform installer generation
3. Builds `xtask` binary (project build automation)
4. Forwards all arguments to `cargo xtask`

**After bootstrap:** Use `cargo xtask <command>` directly or continue with `bootstrap.{sh|cmd} <command>`

### Using xtask - Project Build Automation

**Prerequisites:** Run `bootstrap.{sh|cmd}` first (see Quick Start above)

`xtask` is an idiomatic Rust pattern for build automation - a workspace helper binary providing cross-platform task automation without external dependencies (no Makefiles, no Python, no shell scripts).

**Why xtask?**
- **Cross-platform**: Same commands work identically on Windows, Linux, and macOS
- **Type-safe**: Catch errors at compile time, not runtime
- **Self-documenting**: Built-in `--help` with structured command definitions
- **Pure Rust**: Uses project's existing toolchain, no external tools needed

#### Available Commands

##### 🏗️ Build & Development
```bash
cargo xtask build [--release] [--openexr]  # Full build (default: exrs)
cargo xtask post [--release]               # Copy native libraries (OpenEXR only)
cargo xtask verify [--release]             # Verify dependencies present
cargo xtask deploy [--install-dir PATH]    # Install to system
  # Windows: %LOCALAPPDATA%\Programs\playa
  # Linux/macOS: ~/.local/bin/playa
```

##### 🧹 Maintenance
```bash
cargo xtask wipe [-v] [--dry-run]    # Remove executables/libs from ./target
cargo xtask wipe-wf                  # Delete ALL GitHub Actions runs (parallel)
```

##### 🚀 Release Management
```bash
cargo xtask tag-dev [patch|minor|major]  # Create v0.1.x-dev tag → trigger Build workflow
cargo xtask tag-rel [patch|minor|major]  # Create v0.1.x tag → trigger Release workflow
cargo xtask pr [version]                 # Create PR: dev → main with all commits
cargo xtask changelog                    # Preview unreleased CHANGELOG.md
```

##### 🔧 Platform-Specific
```bash
cargo xtask pre   # Linux only: Patch OpenEXR headers for GCC 11+ compatibility
```

#### What `cargo xtask build` Does

**Without `--openexr` (default - exrs backend):**
1. Runs `cargo build [--release]` with pure Rust exrs backend
2. Self-contained single binary (no dependencies copied)

**With `--openexr` (OpenEXR C++ backend):**
1. **Linux**: Patches OpenEXR headers for GCC 11+ compatibility
2. **All platforms**: Runs `cargo build [--release] --features openexr`
3. **All platforms**: Copies native libraries (OpenEXR, Imath, zlib, openexr-c) to target directory
4. **All platforms**: Copies shaders from project root
5. **Linux**: Creates necessary symlinks for library loading

#### Common Workflows

**Local development (fast):**
```bash
./bootstrap.sh build        # or cargo xtask build
./target/debug/playa
```

**Local development (full OpenEXR):**
```bash
./bootstrap.sh build --openexr
./target/debug/playa
```

**Install to system:**
```bash
cargo xtask build --release --openexr
cargo xtask deploy
# Now available as: playa
```

**Release workflow:**
```bash
# 1. Create PR from dev to main
cargo xtask pr v0.2.0

# 2. Merge PR on GitHub

# 3. Tag release on main
git checkout main && git pull
cargo xtask tag-rel patch

# 4. GitHub Actions builds installers and creates Release
```

## CI/CD Workflows

### Complete Workflow

**1. Development on main branch:**
- Commits to `main` → push triggers `warm-cache.yml`
- `warm-cache.yml` checks cache age (threshold: 12 hours)
- If cache is stale/missing → warms cache for all platforms (Windows, Linux, macOS)
- Cache is saved under `refs/heads/main`

**2. Creating a release:**
- Create git tag: `git tag v0.1.109` → `git push origin v0.1.109`
- Triggers `release.yml` → verifies tag is on `main` branch
- Runs builds for all platforms via `_build-platform.yml`
- **Cache is read from main** (automatic fallback via `actions/cache@v4`)
- For macOS: imports Developer ID certificate, signs `.app`
- Builds installers: `.msi` (Windows), `.deb`/`.AppImage` (Linux), `.dmg`/`.app.tar.gz` (macOS)
- Creates GitHub Release with artifacts

**3. Manual cache warming:**
- Actions → Warm Cache → Run workflow
- Choose backends: `openexr`, `exrs`, or `both`

**Cache strategy:**
- Cache is created **only on main**
- Tags **read** cache from main (don't create their own)
- No duplication, no isolation between tags

**macOS code signing:**
- Certificate: Developer ID Application (stored in GitHub Secrets)
- Workflow imports into temporary keychain
- `cargo-packager` uses `signing-identity` from `Cargo.toml`
- Verification: logs show `✅ App is signed with Developer ID`

### Technical Details

**Release Workflow:**
- Trigger: pushing a tag matching `v*` or manual run
- Behavior:
  - If tag points to commit on `main` → release path (publishes GitHub Release)
  - If tag not on `main` → dev path (builds artifacts without publishing)
- Manual run supports `build_type: auto | release | dev`

**Warm Cache Workflow:**
- Trigger: push to `main` or manual dispatch
- Gate: only executes automatically from `main` branch
- Cooldown: skips if successful run happened within last 12 hours
- Manual run ignores cooldown and always executes
- Backends: `openexr` (default), `exrs`, or `both`

**macOS Packaging:**
- Pre-packaging cleanup: detaches stale `/Volumes/Playa` mount, removes leftover `*.dmg`
- Retries up to 3 times with short delay to avoid `hdiutil: create failed - Resource busy`

**Permissions:**
- Unified workflow configured with `contents: write` for publishing releases

### Cargo Features

Playa uses Cargo features to provide flexible EXR backend selection:

| Feature | Default | Description | Use Case |
|---------|---------|-------------|----------|
| (none) | ✅ Yes | Pure Rust `exrs` backend | Fast builds, no external dependencies |
| `openexr` | ❌ No | C++ OpenEXR backend via `openexr-rs` | Full DWAA/DWAB compression support |

**Build commands:**
```bash
# Default (exrs backend)
cargo build --release

# OpenEXR backend (full compression support)
cargo build --release --features openexr

# Using xtask (handles dependencies automatically)
cargo xtask build              # exrs backend
cargo xtask build --openexr    # OpenEXR backend
```

**Backend comparison:**
- **exrs (default)**:
  - ✅ Pure Rust, fast compilation (~2-3 minutes)
  - ✅ No external dependencies
  - ❌ No DWAA/DWAB compression support
  - Use for: Development, quick iterations

- **openexr (feature flag)**:
  - ✅ Full OpenEXR feature support (DWAA/DWAB/etc)
  - ✅ Battle-tested C++ implementation
  - ❌ Requires C++ compiler, CMake
  - ❌ Slower compilation (~3-4 minutes)
  - Use for: Production builds, full compatibility

### Development Dependencies

**Auto-installed by bootstrap script:**
- `cargo-release` - Version bumping and tag creation
- `cargo-packager` - Cross-platform installer generation (v0.11.7)

**Standard Rust tools (usually pre-installed):**
- `rustup` - Rust toolchain manager
- `cargo` - Rust package manager
- `clippy` - Linter (`rustup component add clippy`)
- `rustfmt` - Code formatter (`rustup component add rustfmt`)

**Required for PR workflow:**
- `gh` - GitHub CLI (used by `cargo xtask pr`) - [Installation](https://cli.github.com/)

**Optional tools:**
- `git-cliff` - Changelog generation (used by `cargo xtask changelog`)
- `cargo-audit` - Security vulnerability scanning
- `cargo-llvm-cov` - Code coverage

### Standard Rust Development

```bash
# Testing
cargo test                           # Run all unit tests
cargo test --release                 # Run tests in release mode

# Documentation
cargo doc --open                     # Generate and open rustdoc documentation
cargo doc --no-deps --open           # Only document this crate

# Code quality
cargo clippy                         # Run linter
cargo clippy -- -D warnings          # Treat warnings as errors
cargo fmt                            # Format code
cargo fmt -- --check                 # Check formatting without modifying

# Build variants
cargo build                          # Debug build
cargo build --release                # Release build (optimized)
cargo clean                          # Clean build artifacts
```

#### Linux-Specific Build Notes

**Note:** These instructions apply only to the OpenEXR C++ backend (`--openexr` feature). The default exrs backend requires no external dependencies.

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

**Library Copy Process (`cargo xtask post`):**

1. **Locate libraries** compiled by `openexr-sys`:
   - Searches `target/release/build/openexr-sys-*/out/` for versioned `.so` files
   - Example: `libOpenEXR-3_2.so.31.0.0`, `libImath-3_1.so.29.9.0`

2. **Copy to target directory**:
   - Destination: `target/release/` (next to `playa` binary)
   - Preserves original versioned filenames

3. **Create SONAME symlinks**:
   - `libOpenEXR-3_2.so -> libOpenEXR-3_2.so.31.0.0`
   - `libOpenEXRCore-3_2.so -> libOpenEXRCore-3_2.so.31.0.0`
   - `libOpenEXRUtil-3_2.so -> libOpenEXRUtil-3_2.so.31.0.0`
   - `libImath-3_1.so -> libImath-3_1.so.29.9.0`
   - Plus OpenEXR-C wrapper lib

**Why this is needed:**
- `openexr-sys` build creates libraries with full SONAME versions
- Rust linker expects generic `.so` names without version suffixes
- Without symlinks: `error while loading shared libraries: libOpenEXR-3_2.so: cannot open shared object file`

**RPATH Configuration:**

`.cargo/config.toml` sets RPATH to `$ORIGIN`, so the executable searches for `.so` files in its own directory:
```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-Wl,-rpath,$ORIGIN"]
```

No `LD_LIBRARY_PATH` needed! Combined with symlinks from `cargo xtask post`, the binary is fully self-contained.

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

#### Windows-Specific Build Notes

**Note:** These instructions apply only to the OpenEXR C++ backend (`--openexr` feature). The default exrs backend requires no external DLLs.

**Native Libraries (DLL Management):**

Windows requires `.dll` files alongside the executable. The same 7 OpenEXR/Imath/zlib libraries are needed, just as `.dll` instead of `.so`.

**Library Copy Process (`cargo xtask post`):**

1. **Locate DLLs** compiled by `openexr-sys`:
   - Searches `target/release/build/openexr-sys-*/out/bin/` for `.dll` files
   - Example: `OpenEXR-3_2.dll`, `Imath-3_1.dll`, `zlib.dll`

2. **Copy to target directory**:
   - Destination: `target/release/` (next to `playa.exe`)
   - Windows DLLs don't use versioned SONAME - simpler than Linux

**Why this is needed:**
- Windows searches for DLLs in the same directory as the executable
- Without DLLs: `The code execution cannot proceed because OpenEXR-3_2.dll was not found`
- No PATH modification needed - self-contained binary

**No RPATH equivalent:**
- Windows automatically searches the executable's directory first
- No special linker flags required (unlike Linux `$ORIGIN`)

## macOS Code Signing & Notarization

### For Users

All macOS DMG releases are **code-signed** with Developer ID and **notarized** by Apple:
- ✅ No Gatekeeper warnings
- ✅ No "unidentified developer" dialogs
- ✅ Double-click DMG → drag to Applications → works immediately

### For Maintainers: CI/CD Setup

**How it works in CI (`_build-backend.yml`):**

**1. Certificate Import**
- Decodes `APPLE_CERTIFICATE` secret (base64 .p12 file)
- Creates temporary keychain
- Imports Developer ID Application certificate
- Unlocks keychain for build process

**2. Signing** (automatic via `cargo-packager`)
- Reads `signing-identity` from `Cargo.toml`:
  ```toml
  [package.metadata.packager.macos]
  signing-identity = "Developer ID Application: Name (TEAM_ID)"
  ```
- Signs all executables and frameworks in `.app` bundle
- Verifies signature with `codesign -dv`

**3. Notarization** (automatic via `cargo-packager`)
- Requires environment variables:
  - `APPLE_ID` - Apple ID email
  - `APPLE_PASSWORD` - App-specific password (NOT iCloud password!)
  - `APPLE_TEAM_ID` - Team ID from Developer Portal
- Submits signed `.app` to Apple notarization service
- Waits for approval (~1-5 minutes)
- Staples notarization ticket to DMG

**4. Verification Logs Show:**
```
✅ Certificate imported: Developer ID Application: Name (TEAM_ID)
✅ App signed successfully
✅ Notarization submitted (request ID: ...)
✅ Notarization approved
✅ Ticket stapled to DMG
```

**Setting Up Secrets (One-Time):**

Run helper script:
```bash
./apple_cert.sh  # Exports certificate and uploads to GitHub Secrets
```

Or manually:
```bash
gh secret set APPLE_CERTIFICATE          # Base64 .p12 file
gh secret set APPLE_CERTIFICATE_PASSWORD # Certificate password
gh secret set APPLE_ID                   # your-email@example.com
gh secret set APPLE_PASSWORD             # App-specific password (NOT iCloud!)
gh secret set APPLE_TEAM_ID              # Y8PQ7YASU9
```

**Certificate Details:**
- Type: "Developer ID Application" (NOT "Apple Development")
- Source: [Apple Developer Portal](https://developer.apple.com/account/resources/certificates/list)
- App-specific password: https://appleid.apple.com → Security → App-Specific Passwords

**Workflow Skip Behavior:**
- If `APPLE_CERTIFICATE` secret is empty → adhoc signature (for testing)
- If any notarization secret missing → builds but skips notarization

## Configuration

### Configuration Files

Playa uses platform-specific configuration directories with flexible override options.

**Priority order:**
1. **CLI argument**: `--config-dir /custom/path`
2. **Environment variable**: `PLAYA_CONFIG_DIR=/custom/path`
3. **Local folder** (backward compatibility): Uses current directory IF any config files already exist
4. **Platform defaults** (new installations):
   - **Linux**: `~/.config/playa/` (config), `~/.local/share/playa/` (data)
   - **macOS**: `~/Library/Application Support/playa/`
   - **Windows**: `%APPDATA%\playa\`

**Files:**
- `playa.json` - Settings (FPS, theme, viewport, etc.)
- `playa_cache.json` - Cache state (sequences, current frame)
- `playa.log` - Log file (when `--log` flag is used)

**Examples:**
```bash
# Use custom directory
playa --config-dir ~/.playa

# Use environment variable
export PLAYA_CONFIG_DIR=~/my-playa-config
playa

# Default behavior:
# - Existing users: Uses current directory (if files found)
# - New users: Uses platform-specific location
playa
```

**Settings auto-saved to `playa.json`:**
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

**Cache state auto-saved to `playa_cache.json`** for instant restoration on restart.

## Usage

### Launch
```bash
# Start with empty player (drag-and-drop or file dialog)
playa

# Load specific file or sequence
playa path/to/image.0001.exr

# Use custom config directory
playa --config-dir ~/.playa path/to/image.0001.exr

# Enable file logging
playa --log                          # Logs to playa.log
playa --log custom.log               # Logs to custom file
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

## Technical Stack

- **UI**: egui 0.33 + eframe
- **TreeView**: egui_ltreeview 0.6.0 (with persistence feature)
- **Graphics**: OpenGL via glow + egui_glow
- **Image**:
  - **EXR (default)**: exrs via image 0.25 (pure Rust)
  - **EXR (optional)**: openexr 0.11 (C++ bindings, `openexr` feature)
  - **Other formats**: image 0.25 (PNG/JPEG/TIFF/TGA/HDR)
- **Async**: std::thread + crossbeam-channel + mpsc
- **Concurrency**: AtomicU64 for epoch counter, Arc<Mutex> for shared state
- **CLI**: clap 4.5
- **Logging**: env_logger (set `RUST_LOG=debug` for verbose output)

## AI Dev experiment:

This project heavily relies on AI agents: Claude Code and Codex.
Without them development time could span months instead of a single week (still, pretty intensive).

**Human-designed architecture:**
- System design and component boundaries
- Performance targets and trade-offs
- UX workflows and user experience
- Security model and threat boundaries
- Release strategy and versioning

**AI-implemented components:**
- ✅ Build automation (`xtask` workspace - 11 commands, cross-platform)
- ✅ CI/CD workflows (cache warming API, branch detection, unified release)
- ✅ Bootstrap scripts (dependency management, error handling)
- ✅ Installer packaging (NSIS, MSI, DMG, DEB, AppImage)
- ✅ Apple signing pipeline (Developer ID, notarization, keychain management)
- ✅ Documentation (architecture diagrams, data flow, comprehensive README)

**Reality check:** AI agents make plenty of mistakes - wrong API usage, platform-specific bugs, over-engineered solutions. Human catches these through testing and directs corrections. Iteration is fast because agents are like instant encyclopaedia.

### What Works Well

**Speed:** Implement in minutes what would take days manually  
**Breadth:** Cross-platform knowledge (Windows/Linux/macOS quirks) instantly available  
**Consistency:** Code style, documentation, commit messages uniform across project  
**Tirelessness:** Agents iterate without frustration, test edge cases without boredom  

### What's not

**Logic:** "AI" is a great trickster.  
It can execute the task perfectly to your description, working completely incorrect and/or unexpected way.


## Contributing

I'm not looking for contributors, but if you think you can add some useful feature - be my guest.
Fork it, clone it, improve it, PR if you want.
Here's the [Contributing Guide](CONTRIBUTING.md) for details on:
- Commit message conventions (Conventional Commits)
- Development workflow and tools
- Release process
- CI/CD architecture


## Acknowledgements
Cool Halloween Cat app icon is taken from this cute [Flaticon icon pack by Yasashii std](http://flaticon.com/packs/halloween-18020037)  

See [CHANGELOG.md](CHANGELOG.md) for project history.