# Unified vcpkg Build - All Platforms

**Single binary, consistent FFmpeg version, no dependencies** - perfect for video pipelines.

---

## Quick Start (All Platforms)

### Windows

```powershell
# Run build script (installs vcpkg + FFmpeg + builds project)
.\build.cmd

# Or manual:
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat
C:\vcpkg\vcpkg install ffmpeg:x64-windows-static-md
$env:VCPKG_ROOT = "C:\vcpkg"
cargo build --release
```

### Linux

```bash
# Run build script (installs vcpkg + FFmpeg + builds project)
chmod +x build.sh
./build.sh
```

### macOS

```zsh
# Run build script (installs vcpkg + FFmpeg + builds project)
chmod +x build-mac.sh
./build-mac.sh
```

### Manual Setup (Linux / macOS)

```bash
# Or manual:
git clone https://github.com/microsoft/vcpkg.git ~/vcpkg
~/vcpkg/bootstrap-vcpkg.sh
export VCPKG_ROOT="$HOME/vcpkg"

# Linux
~/vcpkg/vcpkg install ffmpeg:x64-linux

# macOS (Apple Silicon)
~/vcpkg/vcpkg install ffmpeg:arm64-osx

# macOS (Intel)
~/vcpkg/vcpkg install ffmpeg:x64-osx

cargo build --release
```

---

## Why vcpkg for Video Pipelines?

✅ **Same FFmpeg version** everywhere - reproducible builds
✅ **Single binary** - no DLL/dylib dependencies
✅ **Portable** - works on any machine without FFmpeg installed
✅ **Consistent behavior** - same codecs, same output
✅ **CI/CD friendly** - automated builds work identically

---

## Binary Sizes

| Platform | Triplet | Size | Notes |
|----------|---------|------|-------|
| Windows | `x64-windows-static-md` | ~20-60 MB | Dynamic CRT, static FFmpeg |
| Linux | `x64-linux` | ~10-30 MB | Static FFmpeg + libs |
| macOS ARM | `arm64-osx` | ~15-35 MB | Universal binary compatible |
| macOS Intel | `x64-osx` | ~15-35 MB | Intel-only |

---

## Configuration

This project includes `.cargo/config.toml` with **all required system libraries** for each platform:

- **Windows**: bcrypt, user32, ole32, gdi32, vfw32, etc.
- **Linux**: pthread, dl, m
- **macOS**: CoreFoundation, CoreMedia, VideoToolbox, Security, etc.

**No additional configuration needed!**

---

## Build Times

| Action | Time (First) | Time (Cached) |
|--------|-------------|---------------|
| vcpkg install ffmpeg | 30-60 min | ~1 min |
| cargo build | 5-10 min | 30 sec |

**Tip:** Use `--binarysource` for vcpkg binary caching in CI/CD.

---

## Verification

```bash
# Build example
cargo build --example video-info --release

# Test with video file
./target/release/examples/video_info sample.mp4

# Check no dynamic dependencies (Linux/macOS)
ldd target/release/examples/video_info
# Should show only system libraries (libc, pthread, etc.)

# Check binary size
ls -lh target/release/examples/video_info
```

**Windows:**
```powershell
# Check dependencies
dumpbin /dependents target\release\examples\video_info.exe
# Should show only KERNEL32.dll, USER32.dll, etc. (no avcodec-XX.dll)
```

---

## CI/CD Example

### GitHub Actions

```yaml
name: Build
on: [push]

jobs:
  build:
    strategy:
      matrix:
        os: [windows-latest, ubuntu-latest, macos-latest]

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install vcpkg
        run: |
          git clone https://github.com/microsoft/vcpkg.git
          ./vcpkg/bootstrap-vcpkg.sh

      - name: Install FFmpeg (Linux)
        if: runner.os == 'Linux'
        run: ./vcpkg/vcpkg install ffmpeg:x64-linux

      - name: Install FFmpeg (macOS)
        if: runner.os == 'macOS'
        run: ./vcpkg/vcpkg install ffmpeg:arm64-osx

      - name: Install FFmpeg (Windows)
        if: runner.os == 'Windows'
        run: .\vcpkg\vcpkg install ffmpeg:x64-windows-static-md

      - name: Build
        run: cargo build --release
        env:
          VCPKG_ROOT: ${{ github.workspace }}/vcpkg
```

---

## Troubleshooting

### "vcpkg FFmpeg not found"

```bash
# Check VCPKG_ROOT is set
echo $VCPKG_ROOT  # Linux/macOS
echo $env:VCPKG_ROOT  # Windows

# Check FFmpeg is installed
vcpkg list | grep ffmpeg
```

### "Linker errors on Windows"

Ensure `.cargo/config.toml` exists in project root with Windows system libraries.

### "Missing frameworks on macOS"

macOS frameworks are in `.cargo/config.toml`. If still failing:

```bash
# Check Xcode CLI tools installed
xcode-select --install
```

### "Build takes forever"

FFmpeg compilation takes 30-60 minutes **first time only**. Subsequent builds use cache.

Use binary caching:
```bash
vcpkg install ffmpeg:x64-linux --binarysource=clear
```

---

## Alternative: System Packages vs vcpkg

| Method | Pros | Cons |
|--------|------|------|
| **vcpkg** | Same version everywhere, static binary | Long first build |
| **apt/brew** | Fast install | Different versions, dynamic linking |
| **Build from source** | Full control | Very long build, complex |

**For production pipelines: Use vcpkg** ✅

---

## Version Consistency

Check FFmpeg version:

```bash
# After vcpkg install
vcpkg list | grep ffmpeg
# Example: ffmpeg:x64-linux   7.1.1#4

# In your app
ffmpeg::init()?;
println!("FFmpeg version: {}", ffmpeg_sys_next::avcodec_version());
```

All platforms will have **identical FFmpeg version** when using vcpkg.

---

## Deployment

### Single Binary Distribution

```bash
# Package just the binary
tar czf myapp-linux-x64.tar.gz target/release/myapp

# Users can run directly
./myapp video.mp4
```

**No FFmpeg installation needed on target machine!**

### Docker Example

```dockerfile
FROM rust:1.75 as builder

# Install vcpkg
RUN git clone https://github.com/microsoft/vcpkg.git /vcpkg && \
    /vcpkg/bootstrap-vcpkg.sh

# Install FFmpeg
RUN /vcpkg/vcpkg install ffmpeg:x64-linux

# Build app
WORKDIR /app
COPY . .
ENV VCPKG_ROOT=/vcpkg
RUN cargo build --release

# Runtime image
FROM debian:bookworm-slim
COPY --from=builder /app/target/release/myapp /usr/local/bin/
CMD ["myapp"]
```

---

## See Also

- [DEVELOP.md](DEVELOP.md) - Complete developer guide
- [GUIDE.md](GUIDE.md) - Usage guide with examples
- [examples/README.md](examples/README.md) - Example applications

---

**Recommended Setup:** Run the included setup scripts for automated installation!
