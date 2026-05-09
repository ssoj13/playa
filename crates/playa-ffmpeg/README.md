# playa-ffmpeg - Modern FFmpeg Wrapper with vcpkg Integration

**Modified by:** Alex Joss (joss13@gmail.com)

This is a modernized fork with cross-platform build improvements and vcpkg integration.

## Key Modifications

- **vcpkg Integration**: Automatic FFmpeg installation and static linking on all platforms
- **NVENC Support**: Hardware encoding with NVIDIA NVENC/NVDEC (Windows/Linux, enabled by default)
- **Static Linking**: Single standalone binary, no external FFmpeg dependencies required
- **Optimized CI/CD**: GitHub Actions with aggressive vcpkg caching (20min → 3min builds)
- **Release-Only Builds**: Custom vcpkg triplets skip debug builds (50% faster on Windows)
- **Rust 2024 Edition**: Updated to latest Rust edition with modern syntax
- **FFmpeg 7.1+ Support**: Full support for FFmpeg 7.1 APIs via vcpkg
- **Unified Bootstrap Script**: Single script for building and publishing across all platforms
- **Enhanced Examples**: New video-info tool, improved frame dumping
- **Visual Studio Setup**: Automatic MSVC environment configuration on Windows

## Prerequisites

### vcpkg Installation

This crate uses [vcpkg](https://vcpkg.io/) for FFmpeg dependency management with static linking.

#### Install vcpkg

**Windows:**
```powershell
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
cd C:\vcpkg
.\bootstrap-vcpkg.bat
setx VCPKG_ROOT "C:\vcpkg"
```

**Linux/macOS:**
```bash
git clone https://github.com/microsoft/vcpkg.git /usr/local/share/vcpkg
cd /usr/local/share/vcpkg
./bootstrap-vcpkg.sh
export VCPKG_ROOT=/usr/local/share/vcpkg
# Add to ~/.bashrc or ~/.zshrc for persistence
```

### Install FFmpeg via vcpkg

**Windows (MSVC) - Release only (faster builds):**

First create a custom triplet for release-only builds:
```powershell
# Create triplet file
$tripletContent = @"
set(VCPKG_TARGET_ARCHITECTURE x64)
set(VCPKG_CRT_LINKAGE dynamic)
set(VCPKG_LIBRARY_LINKAGE static)
set(VCPKG_BUILD_TYPE release)
"@
New-Item -Path "$env:VCPKG_ROOT\triplets\community" -ItemType Directory -Force
Set-Content -Path "$env:VCPKG_ROOT\triplets\community\x64-windows-static-md-release.cmake" -Value $tripletContent

# Install FFmpeg (release only - ~2x faster than default)
# NOTE: avdevice/avfilter are intentionally omitted — see "Why no avfilter?" below.
vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale,nvcodec]:x64-windows-static-md-release
```

#### Manifest-mode (pinned baseline) — recommended

The playa workspace ships `vcpkg.json` + `vcpkg-configuration.json` that lock
FFmpeg to a specific microsoft/vcpkg revision. Running the install through
manifest mode guarantees CI and local dev get bit-identical port versions
regardless of how stale or fresh the global vcpkg checkout is. Run **once**
from the workspace root:

```powershell
vcpkg install --x-manifest-root . --x-install-root .vcpkg/installed --triplet x64-windows-static-md-release
```

`xtask::env_setup` auto-detects `.vcpkg/installed/<triplet>/` and points
`VCPKG_ROOT` at it; the global vcpkg state becomes irrelevant for playa
builds. To bump the baseline: replace the SHA in `vcpkg-configuration.json`,
delete `.vcpkg/installed/<triplet>/`, and rerun the command above.

**Windows (MSVC) - Debug + Release (default):**
```powershell
vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale,nvcodec]:x64-windows-static-md
```

**Linux:**
```bash
vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale,nvcodec]:x64-linux-release
```

**macOS (Intel):**
```bash
vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale]:x64-osx-release
```

**macOS (Apple Silicon):**
```bash
vcpkg install ffmpeg[core,avcodec,avformat,swresample,swscale]:arm64-osx-release
```

### Why no `avfilter` / `avdevice`?

Starting with **FFmpeg 8.1**, vcpkg's `avfilter` build pulls in the
`vsrc_gfxcapture` source filter (Windows.Graphics.Capture WinRT + C++
`<regex>`). On Windows that introduces an unresolved
`__std_regex_transform_primary_char` against a different MSVC C++ STL than
the one currently active in your toolchain — and there's no way to disable
that filter via vcpkg features alone.

Since playa only consumes `avcodec`, `avformat`, `swresample`, `swscale`
(plus `nvcodec` for hardware encode), the `playa-ffmpeg` `default` features
explicitly exclude `device` / `filter`. If you need them, install the
matching vcpkg ports **and** opt into the features in your `Cargo.toml`:

```toml
playa-ffmpeg = { path = "...", default-features = false, features = ["static", "codec", "format", "filter"] }
```

If linking still fails with the WinRT regex symbol, downgrade vcpkg's
`ffmpeg` port baseline to an 8.0.x build via `vcpkg-configuration.json`.

**Platform-Specific Notes:**

| Platform | NVENC Support | Hardware Encoding Alternative | Build Time (Release-Only) |
|----------|---------------|-------------------------------|---------------------------|
| **Windows** | ✅ Enabled by default | N/A | ~10 min (first build) |
| **Linux** | ✅ Enabled by default | VAAPI (Intel/AMD) | ~15 min (first build) |
| **macOS** | ❌ Not available | VideoToolbox (Apple Silicon/Intel) | ~15 min (first build) |

**Important Notes:**
- **NVENC Runtime Requirements**: NVENC headers are statically linked, but you need NVIDIA GPU + drivers at runtime
- **Windows Optimization**: Use `x64-windows-static-md-release` triplet to skip debug builds (50% faster)
- **Subsequent Builds**: After first build, vcpkg cache makes rebuilds ~3 minutes
- **CI/CD**: GitHub Actions uses release-only triplets and persistent caching for optimal performance

## Quick Start

### Windows
```cmd
bootstrap.cmd build
```

### Linux/macOS
```bash
./bootstrap.sh build
```

See [examples/README.md](examples/README.md) for detailed usage examples.

### Quick Test: List Available Codecs

```bash
# Build and run video-info example
cargo build --example video-info --release

# List all available codecs (hardware + software)
cargo run --example video-info --release -- ls
```

**Output:**
- Video decoders: H264, H265, VP9, AV1, MPEG4, etc.
- Video encoders: libx264, libx265, NVENC (if GPU available), etc.
- Audio decoders: AAC, MP3, Opus, Vorbis, etc.
- Audio encoders: AAC, MP3, Opus, etc.

**Why use this:**
- Verify NVENC is available on your system
- Check which codecs are enabled
- Confirm FFmpeg is properly configured

## Build System

### Cargo Features

The crate supports multiple hardware encoding/decoding backends through Cargo features:

```toml
[features]
default = ["codec", "device", "filter", "format", "software-resampling", "software-scaling", "nvenc"]

# Core FFmpeg components
codec = []
device = []
filter = []
format = []
software-resampling = []
software-scaling = []

# Hardware encoding/decoding (platform-specific)
nvenc = []           # NVIDIA NVENC/NVDEC (Windows/Linux only, enabled by default)
vaapi = []           # Intel/AMD VAAPI (Linux only)
videotoolbox = []    # Apple VideoToolbox (macOS only)
qsv = []             # Intel Quick Sync Video (Windows/Linux)
```

**Usage examples:**

```bash
# Default build with NVENC
cargo build --release

# Build without NVENC (software encoding only)
cargo build --release --no-default-features --features "codec,device,filter,format,software-resampling,software-scaling"

# Build with VAAPI for Intel/AMD GPUs on Linux
cargo build --release --features "vaapi"

# Build with VideoToolbox on macOS
cargo build --release --features "videotoolbox"
```

### Build Options

```bash
bootstrap build           # Build release (default)
bootstrap build --release # Build release (explicit)
bootstrap build --debug   # Build debug
bootstrap test           # Run all tests
```

### Build System Details

The crate uses a custom `build.rs` that:
- Automatically detects and uses vcpkg-installed FFmpeg
- Respects `VCPKG_DEFAULT_TRIPLET` environment variable for custom triplets
- Falls back to system FFmpeg if vcpkg not found
- Attempts automatic vcpkg installation if `VCPKG_ROOT` is set
- Emits proper linking flags for static/dynamic linking

### Testing

Run tests to verify FFmpeg integration:

```bash
# All tests
bootstrap test

# Or directly with cargo
cargo test --examples
```

**What it does:**
- Verifies FFmpeg libraries are properly linked
- Tests basic codec functionality
- Validates video/audio decoding
- Checks frame extraction and color space conversion

**Test output location:** `target/debug/` or `target/release/`

## Publishing (Maintainers)

```bash
bootstrap crate          # Dry-run (preview changes)
bootstrap crate publish  # Publish to crates.io
```

Uses [cargo-release](https://github.com/crate-ci/cargo-release) - automatically installed on first use.

## CI/CD Pipeline

### Optimized Build Strategy

The GitHub Actions workflow is optimized for minimal build times:

**First Build (cold cache):**
- Linux: ~15 minutes (FFmpeg compilation)
- macOS: ~15 minutes (FFmpeg compilation)
- Windows: ~10 minutes (release-only FFmpeg)

**Subsequent Builds (warm cache):**
- All platforms: ~3 minutes (Rust code only)

### Caching Strategy

1. **Persistent FFmpeg Cache**: vcpkg artifacts cached indefinitely (only rebuilds when cache key manually bumped)
2. **Release-Only Triplets**: Windows uses custom `x64-windows-static-md-release` triplet to skip debug builds
3. **Conditional Installation**: FFmpeg installation skipped if cache hit

**Cache locations:**
```yaml
Linux/macOS:
  - /usr/local/share/vcpkg/installed
  - /usr/local/share/vcpkg/buildtrees
  - /usr/local/share/vcpkg/downloads
  - /usr/local/share/vcpkg/packages

Windows:
  - C:\vcpkg\installed
  - C:\vcpkg\buildtrees
  - C:\vcpkg\downloads
  - C:\vcpkg\packages
```

### Platform Matrix

| Platform | Triplet | NVENC | Cache Key |
|----------|---------|-------|-----------|
| **Linux** | `x64-linux-release` | ✅ | `Linux-vcpkg-ffmpeg-nvcodec-v2` |
| **macOS Intel** | `x64-osx-release` | ❌ | `macOS-x64-osx-release-vcpkg-ffmpeg-v2` |
| **macOS ARM** | `arm64-osx-release` | ❌ | `macOS-arm64-osx-release-vcpkg-ffmpeg-v2` |
| **Windows** | `x64-windows-static-md-release` | ✅ | `Windows-vcpkg-ffmpeg-nvcodec-release-only-v3` |

---

[![Crates.io](https://img.shields.io/crates/v/playa-ffmpeg.svg)](https://crates.io/crates/playa-ffmpeg)
[![Documentation](https://docs.rs/playa-ffmpeg/badge.svg)](https://docs.rs/playa-ffmpeg)
[![build](https://github.com/ssoj13/playa-ffmpeg/workflows/build/badge.svg)](https://github.com/ssoj13/playa-ffmpeg/actions)
[![License](https://img.shields.io/crates/l/playa-ffmpeg.svg)](LICENSE)

This is a fork of [ffmpeg-next](https://crates.io/crates/ffmpeg-next) (originally based on the [ffmpeg](https://crates.io/crates/ffmpeg) crate by [meh.](https://github.com/meh/rust-ffmpeg)).

This fork focuses on modern Rust (2024 edition) with FFmpeg 8.0 support and simplified cross-platform builds via vcpkg.

## Hardware Encoding Support

### NVENC (NVIDIA GPUs)

NVENC support is **enabled by default** on Windows and Linux builds.

**Requirements:**
- NVIDIA GPU with NVENC support (GTX 600+, Quadro Kxxx+, Tesla Kxx+)
- NVIDIA drivers (no CUDA SDK required for compilation)
- `nvcodec` feature in vcpkg FFmpeg installation

**Runtime behavior:**
- On systems **with** NVIDIA GPU: Hardware encoding available
- On systems **without** GPU: Gracefully falls back to CPU encoders
- Headers-only dependency - no runtime CUDA requirement

**Not available on macOS** (NVENC is NVIDIA-specific hardware).

### Platform-Specific Hardware Encoding

- **Windows/Linux**: NVENC via `nvcodec` feature
- **macOS**: VideoToolbox (built into macOS, no additional setup)
- **Intel GPUs**: QuickSync (optional, not enabled by default)

## CI/CD Setup

For GitHub Actions or other CI environments:

### Required Environment Variables

```yaml
env:
  VCPKG_ROOT: /usr/local/share/vcpkg  # Linux/macOS
  # or C:\vcpkg on Windows
  PKG_CONFIG_PATH: /usr/local/share/vcpkg/installed/{triplet}/lib/pkgconfig
```

### Example GitHub Actions Workflow

```yaml
- name: Install FFmpeg via vcpkg
  run: |
    vcpkg install ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:x64-linux-release

- name: Set environment variables
  run: |
    echo "PKG_CONFIG_PATH=/usr/local/share/vcpkg/installed/x64-linux-release/lib/pkgconfig" >> $GITHUB_ENV
    echo "VCPKG_ROOT=/usr/local/share/vcpkg" >> $GITHUB_ENV

- name: Build
  run: cargo build --release
```

### vcpkg Caching

Speed up CI builds with vcpkg caching:

```yaml
- name: Cache vcpkg
  uses: actions/cache@v4
  with:
    path: |
      /usr/local/share/vcpkg/installed
      ~/.cache/vcpkg
    key: ${{ runner.os }}-vcpkg-x64-linux-release-${{ hashFiles('.github/workflows/build.yml') }}
    restore-keys: |
      ${{ runner.os }}-vcpkg-x64-linux-release-
```

**Result:** First build ~30-40 min, cached builds ~3-5 min.

## Cargo Features

```toml
[features]
default = ["codec", "device", "filter", "format", "software-resampling", "software-scaling", "nvenc"]

# Hardware encoding (nvenc enabled by default)
nvenc = []           # NVIDIA NVENC/NVDEC
vaapi = []           # Linux VA-API (optional)
videotoolbox = []    # macOS VideoToolbox (optional)
qsv = []             # Intel QuickSync (optional)
```

Build without NVENC:
```bash
cargo build --no-default-features --features codec,device,filter,format,software-resampling,software-scaling
```

## Triplet Reference

| Platform | Triplet | Static Linking | NVENC |
|----------|---------|----------------|-------|
| Windows MSVC | `x64-windows-static-md` | ✅ | ✅ |
| Windows MinGW | `x64-mingw-static` | ✅ | ✅ |
| Linux x64 | `x64-linux-release` | ✅ | ✅ |
| macOS Intel | `x64-osx-release` | ✅ | ❌ |
| macOS ARM64 | `arm64-osx-release` | ✅ | ❌ |

## Documentation

- [API docs](https://docs.rs/playa-ffmpeg/) - Rust API documentation
- [FFmpeg user manual](https://ffmpeg.org/ffmpeg-all.html) - Official FFmpeg manual
- [FFmpeg Doxygen](https://ffmpeg.org/doxygen/trunk/) - C API reference
- [vcpkg FFmpeg port](https://github.com/microsoft/vcpkg/tree/master/ports/ffmpeg) - vcpkg FFmpeg features

See [CHANGELOG.md](CHANGELOG.md) for version history and upgrade notes.
