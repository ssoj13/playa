# Developer Guide - rust-ffmpeg

Complete development setup for Windows, Linux, and macOS with FFmpeg 8.0 support.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Windows Setup](#windows-setup)
- [Linux Setup](#linux-setup)
- [macOS Setup](#macos-setup)
- [Building Examples](#building-examples)
- [Platform Differences](#platform-differences)
- [Troubleshooting](#troubleshooting)
- [vcpkg Details](#vcpkg-details)

---

## Quick Start

### Prerequisites (All Platforms)

- **Rust** 1.75+ (edition 2024)
- **Clang/LLVM** (for bindgen)
- **pkg-config** (Linux/macOS)
- **FFmpeg 3.4 - 8.0** (system or vcpkg)

### Clone and Build

```bash
git clone https://github.com/zmwangx/rust-ffmpeg.git
cd rust-ffmpeg

# Build
cargo build --release

# Run example
cargo run --example video-info -- video.mp4
```

---

## Windows Setup

### Method 1: vcpkg (Recommended)

**1. Install vcpkg:**

```powershell
# Clone vcpkg
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
cd C:\vcpkg
.\bootstrap-vcpkg.bat

# Set environment variable
$env:VCPKG_ROOT = "C:\vcpkg"
[Environment]::SetEnvironmentVariable("VCPKG_ROOT", "C:\vcpkg", "User")
```

**2. Install FFmpeg:**

```powershell
# Static linking (recommended)
vcpkg install ffmpeg:x64-windows-static-md

# Or dynamic linking
vcpkg install ffmpeg:x64-windows

# Or fully static (including CRT)
vcpkg install ffmpeg:x64-windows-static
```

**3. Install LLVM (for bindgen):**

```powershell
# Via Chocolatey
choco install llvm

# Or download from https://releases.llvm.org/
# Add to PATH: C:\Program Files\LLVM\bin
```

**4. Build:**

```powershell
cargo build --release
```

**Notes:**
- `.cargo/config.toml` already includes Windows system libraries
- Static build creates single `.exe` (~20-60 MB)
- No DLLs needed for deployment

### Method 2: Manual FFmpeg

**1. Download FFmpeg:**

From https://www.gyan.dev/ffmpeg/builds/
- Download "Shared" (for dev) or "Static" (for distribution)

**2. Set environment:**

```powershell
$env:FFMPEG_DIR = "C:\ffmpeg"
$env:PATH = "C:\ffmpeg\bin;$env:PATH"
```

**3. Build:**

```powershell
cargo build --release
```

---

## Linux Setup

### Method 1: System FFmpeg (Simplest)

**Ubuntu/Debian:**

```bash
# Install FFmpeg development libraries
sudo apt update
sudo apt install -y \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libavdevice-dev \
    libavfilter-dev \
    libswscale-dev \
    libswresample-dev \
    pkg-config \
    clang

# Build
cargo build --release
```

**Fedora/RHEL:**

```bash
# Enable RPM Fusion for FFmpeg
sudo dnf install -y \
    https://download1.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm

# Install FFmpeg
sudo dnf install -y \
    ffmpeg-devel \
    clang \
    pkg-config

# Build
cargo build --release
```

**Arch Linux:**

```bash
sudo pacman -S ffmpeg clang pkg-config
cargo build --release
```

### Method 2: vcpkg (Static Linking)

**1. Install vcpkg:**

```bash
# Clone vcpkg
git clone https://github.com/microsoft/vcpkg.git ~/vcpkg
cd ~/vcpkg
./bootstrap-vcpkg.sh

# Add to PATH
echo 'export VCPKG_ROOT="$HOME/vcpkg"' >> ~/.bashrc
echo 'export PATH="$VCPKG_ROOT:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**2. Install dependencies:**

```bash
# Ubuntu/Debian
sudo apt install -y curl zip unzip tar pkg-config clang

# Fedora
sudo dnf install -y curl zip unzip tar pkg-config clang

# Arch
sudo pacman -S curl zip unzip tar pkg-config clang
```

**3. Install FFmpeg:**

```bash
# Static linking
vcpkg install ffmpeg:x64-linux

# Build
cargo build --release
```

**4. Create `.cargo/config.toml` (if not exists):**

```toml
# .cargo/config.toml for Linux
[target.x86_64-unknown-linux-gnu]
rustflags = [
    # Optional: static linking for deployment
    # "-C", "target-feature=+crt-static",
]
```

### Method 3: Build FFmpeg from Source

```bash
# Install build dependencies
sudo apt install -y \
    build-essential \
    yasm \
    nasm \
    libx264-dev \
    libx265-dev \
    libvpx-dev \
    libopus-dev

# Download and build FFmpeg
wget https://ffmpeg.org/releases/ffmpeg-8.0.tar.xz
tar xf ffmpeg-8.0.tar.xz
cd ffmpeg-8.0

./configure \
    --prefix=/usr/local \
    --enable-shared \
    --enable-gpl \
    --enable-libx264 \
    --enable-libx265 \
    --enable-libvpx \
    --enable-libopus

make -j$(nproc)
sudo make install
sudo ldconfig

# Build rust-ffmpeg
cd ~/rust-ffmpeg
cargo build --release
```

---

## macOS Setup

### Method 1: Homebrew (Recommended)

**1. Install Homebrew:**

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

**2. Install FFmpeg:**

```bash
# Install FFmpeg with libraries
brew install ffmpeg pkg-config

# Install LLVM (for bindgen)
brew install llvm

# Add LLVM to PATH
echo 'export PATH="/opt/homebrew/opt/llvm/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

**3. Build:**

```bash
cargo build --release
```

### Method 2: vcpkg (Static Linking)

**1. Install vcpkg:**

```bash
git clone https://github.com/microsoft/vcpkg.git ~/vcpkg
cd ~/vcpkg
./bootstrap-vcpkg.sh

echo 'export VCPKG_ROOT="$HOME/vcpkg"' >> ~/.zshrc
echo 'export PATH="$VCPKG_ROOT:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

**2. Install FFmpeg:**

```bash
# Static linking
vcpkg install ffmpeg:arm64-osx  # Apple Silicon
# or
vcpkg install ffmpeg:x64-osx    # Intel Mac

# Build
cargo build --release
```

### Method 3: MacPorts

```bash
sudo port install ffmpeg pkgconfig
cargo build --release
```

---

## Building Examples

### video-info Example

Analyzes video files and tests FFmpeg functionality.

```bash
# Build
cargo build --example video-info --release

# Run
./target/release/examples/video_info video.mp4
```

**Expected output:**
```
=== FFmpeg Video Analyzer ===

File: video.mp4

📄 FILE METADATA
  Format: mov,mp4,m4a,3gp,3g2,mj2
  Duration: 10.00s
  Bitrate: 2.50 Mbps

📺 STREAMS (2 total)
  Stream #0 - Video: H264, 1920x1080, 30fps
  Stream #1 - Audio: AAC, 48000Hz, 2ch

✅ Analysis complete!
```

### All Examples

```bash
# Build all examples
cargo build --examples --release

# List built examples
ls -lh target/release/examples/

# Run specific example
cargo run --example transcode-x264 -- input.mp4 output.mp4
```

---

## Platform Differences

### Windows

**Linking:**
- Static: Requires Windows system libraries (bcrypt, user32, ole32, etc.)
- Dynamic: Requires DLLs in PATH or app directory
- Config: `.cargo/config.toml` includes all system libs

**Binary Size:**
- Static: ~20-60 MB (FFmpeg embedded)
- Dynamic: ~500 KB + DLLs (~20 MB)

**Deployment:**
- Static: Single `.exe`, works anywhere
- Dynamic: Distribute `.exe` + FFmpeg DLLs

### Linux

**Linking:**
- System FFmpeg: Dynamic by default
- vcpkg: Static linking available
- Usually smaller than Windows (~10-30 MB static)

**Library Paths:**
```bash
# Check FFmpeg libraries
pkg-config --libs libavcodec libavformat libavutil

# Check library location
ldconfig -p | grep libav
```

**Deployment:**
- Static: Single binary, portable
- Dynamic: May need to ship libs or use AppImage/Flatpak

### macOS

**Linking:**
- Homebrew: Dynamic linking
- vcpkg: Static linking
- Universal binaries supported (Intel + ARM)

**Library Paths:**
```bash
# Homebrew libs location
ls /opt/homebrew/lib/libav*  # Apple Silicon
ls /usr/local/lib/libav*     # Intel Mac
```

**Deployment:**
- Static: Single binary
- Dynamic: Use `install_name_tool` or bundle libs in .app

---

## Troubleshooting

### "FFmpeg not found"

**Linux/macOS:**
```bash
# Check pkg-config can find FFmpeg
pkg-config --modversion libavcodec

# If not found, set PKG_CONFIG_PATH
export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:$PKG_CONFIG_PATH"
```

**Windows:**
```powershell
# Check vcpkg installation
vcpkg list | Select-String ffmpeg

# Set VCPKG_ROOT if not detected
$env:VCPKG_ROOT = "C:\vcpkg"
```

### "clang not found" or bindgen errors

**Ubuntu/Debian:**
```bash
sudo apt install clang libclang-dev
export LIBCLANG_PATH=/usr/lib/llvm-14/lib  # Adjust version
```

**macOS:**
```bash
brew install llvm
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
```

**Windows:**
```powershell
# Install LLVM
choco install llvm

# Or download: https://releases.llvm.org/
# Set LIBCLANG_PATH if needed
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
```

### Linker errors on Windows

**Missing system libraries:**

Create `.cargo/config.toml`:
```toml
[target.x86_64-pc-windows-msvc]
rustflags = [
    "-l", "bcrypt",
    "-l", "user32",
    "-l", "ole32",
    "-l", "oleaut32",
    "-l", "mfuuid",
    "-l", "strmiids",
    "-l", "mfplat",
    "-l", "secur32",
    "-l", "ws2_32",
    "-l", "shlwapi",
    "-l", "gdi32",
    "-l", "vfw32",
]
```

### "undefined reference" errors on Linux

**Missing libraries:**
```bash
# Install missing dev packages
sudo apt install -y \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswscale-dev \
    libswresample-dev

# Or use vcpkg for static linking
vcpkg install ffmpeg:x64-linux
```

### macOS dylib errors

**Missing libraries:**
```bash
# Install FFmpeg
brew install ffmpeg

# Check library paths
otool -L target/release/examples/video_info

# Fix library paths (if needed)
install_name_tool -change \
    /old/path/libavcodec.dylib \
    /opt/homebrew/lib/libavcodec.dylib \
    target/release/examples/video_info
```

### vcpkg build takes too long

FFmpeg compilation can take 30-60 minutes on first install.

**Speed up:**
```bash
# Use binary cache (if available)
vcpkg install ffmpeg --binarysource=clear

# Or use prebuilt system packages
# Linux: Use apt/dnf instead of vcpkg
# macOS: Use Homebrew instead of vcpkg
# Windows: Download prebuilt from gyan.dev
```

---

## vcpkg Details

### Triplets Explained

| Triplet | Platform | Linking | CRT | Use Case |
|---------|----------|---------|-----|----------|
| `x64-windows` | Windows x64 | Dynamic | Dynamic | Development |
| `x64-windows-static` | Windows x64 | Static | Static | Fully static |
| `x64-windows-static-md` | Windows x64 | Static | Dynamic | Rust default |
| `x64-linux` | Linux x64 | Static | - | Portable binary |
| `arm64-osx` | macOS ARM | Static | - | Apple Silicon |
| `x64-osx` | macOS x64 | Static | - | Intel Mac |

### Installing Specific FFmpeg Version

```bash
# List available versions
vcpkg search ffmpeg

# Install specific version (if available)
vcpkg install ffmpeg@8.0.0:x64-windows-static-md

# Or use vcpkg.json manifest
cat > vcpkg.json <<EOF
{
  "dependencies": [
    {
      "name": "ffmpeg",
      "version>=": "8.0.0"
    }
  ]
}
EOF
```

### Custom vcpkg Features

```bash
# Install with specific features
vcpkg install ffmpeg[core,avcodec,avformat,swscale]:x64-windows-static-md

# Available features
vcpkg search ffmpeg --x-full-desc
```

---

## Cross-Compilation

### Linux → Windows (via MinGW)

```bash
# Install cross-compiler
sudo apt install -y mingw-w64

# Add Rust target
rustup target add x86_64-pc-windows-gnu

# Configure .cargo/config.toml
cat >> .cargo/config.toml <<EOF
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
EOF

# Build FFmpeg for Windows (via vcpkg)
vcpkg install ffmpeg:x64-mingw-static

# Cross-compile
cargo build --release --target x86_64-pc-windows-gnu
```

### macOS → iOS/tvOS

Requires additional FFmpeg build for iOS.

```bash
# Add target
rustup target add aarch64-apple-ios

# Build FFmpeg for iOS (manual or via xcframework)
# See: https://github.com/kewlbear/FFmpeg-iOS-build-script

# Cross-compile
cargo build --release --target aarch64-apple-ios
```

---

## Environment Variables

### Common Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `VCPKG_ROOT` | vcpkg installation | `/home/user/vcpkg` |
| `FFMPEG_DIR` | FFmpeg installation | `/usr/local` |
| `PKG_CONFIG_PATH` | pkg-config search path | `/usr/local/lib/pkgconfig` |
| `LIBCLANG_PATH` | Clang library path | `/usr/lib/llvm-14/lib` |

### Setting Environment Variables

**Linux/macOS (bash/zsh):**
```bash
# Temporary (current session)
export VCPKG_ROOT="$HOME/vcpkg"

# Permanent
echo 'export VCPKG_ROOT="$HOME/vcpkg"' >> ~/.bashrc
source ~/.bashrc
```

**Windows (PowerShell):**
```powershell
# Temporary (current session)
$env:VCPKG_ROOT = "C:\vcpkg"

# Permanent (user)
[Environment]::SetEnvironmentVariable("VCPKG_ROOT", "C:\vcpkg", "User")

# Permanent (system - requires admin)
[Environment]::SetEnvironmentVariable("VCPKG_ROOT", "C:\vcpkg", "Machine")
```

---

## Testing

### Run Tests

```bash
# All tests
cargo test

# Specific test
cargo test --test test_name

# With output
cargo test -- --nocapture
```

### Check Code

```bash
# Check without building
cargo check

# Check all targets
cargo check --all-targets

# Clippy lints
cargo clippy -- -D warnings

# Format code
cargo fmt
```

---

## Performance Tips

### Build Performance

```bash
# Use LLD linker (faster)
# Linux/macOS
cargo install -f cargo-binutils
rustup component add llvm-tools-preview

# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

# Parallel builds
cargo build -j 8  # Use 8 cores
```

### Runtime Performance

- Use `--release` for production builds
- Consider `--profile release-with-debug` for profiling
- Enable LTO for smaller binaries:

```toml
# Cargo.toml
[profile.release]
lto = true
codegen-units = 1
opt-level = 3
```

---

## Contributing

1. Fork the repository
2. Create feature branch: `git checkout -b feature/my-feature`
3. Make changes and test
4. Commit: `git commit -am 'Add feature'`
5. Push: `git push origin feature/my-feature`
6. Create Pull Request

---

## Resources

### Documentation

- [FFmpeg Documentation](https://ffmpeg.org/documentation.html)
- [FFmpeg Wiki](https://trac.ffmpeg.org/wiki)
- [Rust FFmpeg Wiki](https://github.com/zmwangx/rust-ffmpeg/wiki)

### Tools

- [vcpkg](https://vcpkg.io/)
- [Homebrew](https://brew.sh/)
- [Chocolatey](https://chocolatey.org/)

### Support

- [GitHub Issues](https://github.com/zmwangx/rust-ffmpeg/issues)
- [FFmpeg mailing lists](https://ffmpeg.org/contact.html)

---

## Quick Reference

### Common Commands

```bash
# Build
cargo build --release

# Run example
cargo run --example video-info -- file.mp4

# Clean build
cargo clean

# Update dependencies
cargo update

# Check for outdated deps
cargo outdated

# Install locally
cargo install --path .
```

### File Locations

- **Source**: `src/`
- **Examples**: `examples/`
- **Tests**: `tests/`
- **Config**: `.cargo/config.toml`
- **Binary**: `target/release/`

### Important Files

- `Cargo.toml` - Dependencies and metadata
- `build.rs` - Build script (vcpkg detection)
- `.cargo/config.toml` - Compiler flags, linker settings
- `GUIDE.md` - User guide with examples
- `DEVELOP.md` - This file

---

**Last Updated:** 2025-11-08
**FFmpeg Version:** 8.0
**Rust Edition:** 2024
