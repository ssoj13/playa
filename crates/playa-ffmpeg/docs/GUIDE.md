# Rust FFmpeg - Quick Guide

## Installation

```toml
[dependencies]
ffmpeg-next = "8.0"
```

**Supports FFmpeg 3.4 - 8.0** (FFmpeg 8.0 recommended).

**Quick setup with vcpkg (Windows):**

```powershell
# Install FFmpeg 8.x via vcpkg (static linking)
vcpkg install ffmpeg:x64-windows-static-md

# Build - vcpkg auto-detected
cargo build --release
```

No additional configuration needed - vcpkg support is built-in!

---

## Writing Frames to MP4

### Example 1: Basic Recording (YUV frames)

```rust
use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, encoder, format, frame, Packet, Rational};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ffmpeg::init()?;

    let output_file = "output.mp4";
    let width = 1920;
    let height = 1080;
    let fps = 30;
    let total_frames = 300; // 10 seconds

    // Create output context
    let mut octx = format::output(&output_file)?;

    // Setup H.264 encoder
    let codec = encoder::find(codec::Id::H264)
        .ok_or("H264 encoder not found")?;

    let mut ost = octx.add_stream(codec)?;

    let mut video_encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;

    video_encoder.set_width(width);
    video_encoder.set_height(height);
    video_encoder.set_format(format::Pixel::YUV420P);
    video_encoder.set_frame_rate(Some(Rational::new(fps, 1)));
    video_encoder.set_time_base(Rational::new(1, fps));

    // Open encoder with H.264 settings
    let encoder = video_encoder.open_with({
        let mut opts = ffmpeg::Dictionary::new();
        opts.set("preset", "medium");  // ultrafast, fast, medium, slow
        opts.set("crf", "23");         // quality: 0 (best) - 51 (worst)
        opts
    })?;

    ost.set_parameters(&encoder);

    // Write MP4 header
    octx.write_header()?;

    // Create frame
    let mut frame = frame::Video::new(format::Pixel::YUV420P, width, height);

    // Write frames
    for i in 0..total_frames {
        // Fill frame with data (example - gradient)
        fill_yuv_frame(&mut frame, i);

        frame.set_pts(Some(i as i64));

        // Send frame to encoder
        encoder.send_frame(&frame)?;

        // Receive encoded packets
        flush_encoder(&encoder, &mut octx, ost.index())?;
    }

    // Finalize - flush encoder
    encoder.send_eof()?;
    flush_encoder(&encoder, &mut octx, ost.index())?;

    // Write trailer
    octx.write_trailer()?;

    Ok(())
}

fn flush_encoder(
    encoder: &encoder::Video,
    octx: &mut format::context::Output,
    stream_index: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        encoded.set_stream(stream_index);
        encoded.write_interleaved(octx)?;
    }
    Ok(())
}

fn fill_yuv_frame(frame: &mut frame::Video, index: i64) {
    // Example YUV data filling
    let width = frame.width() as usize;
    let height = frame.height() as usize;

    unsafe {
        let y_plane = std::slice::from_raw_parts_mut(
            frame.data_mut(0).as_mut_ptr(),
            height * frame.stride(0)
        );

        // Y component (luminance)
        for y in 0..height {
            for x in 0..width {
                y_plane[y * frame.stride(0) + x] =
                    ((x + y + index as usize) % 256) as u8;
            }
        }

        // U and V components (chrominance) - half size for YUV420
        let uv_height = height / 2;
        let uv_width = width / 2;

        for plane in 1..=2 {
            let uv_plane = std::slice::from_raw_parts_mut(
                frame.data_mut(plane).as_mut_ptr(),
                uv_height * frame.stride(plane)
            );

            for y in 0..uv_height {
                for x in 0..uv_width {
                    uv_plane[y * frame.stride(plane) + x] = 128; // neutral color
                }
            }
        }
    }
}
```

---

### Example 2: RGB → MP4 (with swscale)

```rust
use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, encoder, format, frame, software::scaling, Packet, Rational};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ffmpeg::init()?;

    let output_file = "output.mp4";
    let width = 1920;
    let height = 1080;
    let fps = 30;

    // Create output context
    let mut octx = format::output(&output_file)?;

    // Setup encoder
    let codec = encoder::find(codec::Id::H264)
        .ok_or("H264 encoder not found")?;

    let mut ost = octx.add_stream(codec)?;
    let mut video_encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;

    video_encoder.set_width(width);
    video_encoder.set_height(height);
    video_encoder.set_format(format::Pixel::YUV420P);
    video_encoder.set_frame_rate(Some(Rational::new(fps, 1)));
    video_encoder.set_time_base(Rational::new(1, fps));
    video_encoder.set_bit_rate(5_000_000); // 5 Mbps

    let encoder = video_encoder.open_with({
        let mut opts = ffmpeg::Dictionary::new();
        opts.set("preset", "fast");
        opts.set("crf", "20");
        opts
    })?;

    ost.set_parameters(&encoder);
    octx.write_header()?;

    // Create scaler for RGB → YUV conversion
    let mut scaler = scaling::Context::get(
        format::Pixel::RGB24,           // input format
        width,
        height,
        format::Pixel::YUV420P,        // output format (for H.264)
        width,
        height,
        scaling::Flags::BILINEAR,
    )?;

    // Create frames
    let mut rgb_frame = frame::Video::new(format::Pixel::RGB24, width, height);
    let mut yuv_frame = frame::Video::new(format::Pixel::YUV420P, width, height);

    // Your RGB data (example - red screen)
    let rgb_data = vec![255u8, 0u8, 0u8; (width * height) as usize]; // RGB red

    for i in 0..300 {
        // Copy RGB data to frame
        copy_rgb_to_frame(&mut rgb_frame, &rgb_data);

        // Convert RGB → YUV
        scaler.run(&rgb_frame, &mut yuv_frame)?;

        yuv_frame.set_pts(Some(i));

        // Encode
        encoder.send_frame(&yuv_frame)?;
        flush_encoder(&encoder, &mut octx, ost.index())?;
    }

    encoder.send_eof()?;
    flush_encoder(&encoder, &mut octx, ost.index())?;
    octx.write_trailer()?;

    Ok(())
}

fn copy_rgb_to_frame(frame: &mut frame::Video, rgb_data: &[u8]) {
    unsafe {
        let plane = std::slice::from_raw_parts_mut(
            frame.data_mut(0).as_mut_ptr(),
            (frame.height() * frame.stride(0)) as usize
        );

        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let stride = frame.stride(0);

        for y in 0..height {
            for x in 0..width {
                let src_offset = (y * width + x) * 3;
                let dst_offset = y * stride + x * 3;

                plane[dst_offset..dst_offset + 3]
                    .copy_from_slice(&rgb_data[src_offset..src_offset + 3]);
            }
        }
    }
}

fn flush_encoder(
    encoder: &encoder::Video,
    octx: &mut format::context::Output,
    stream_index: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        encoded.set_stream(stream_index);
        encoded.write_interleaved(octx)?;
    }
    Ok(())
}
```

---

### Example 3: RGBA → MP4

```rust
use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, encoder, format, frame, software::scaling, Packet, Rational};

fn encode_rgba_to_mp4(
    output_file: &str,
    rgba_frames: &[Vec<u8>],  // each Vec<u8> = RGBA data (width*height*4 bytes)
    width: u32,
    height: u32,
    fps: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    ffmpeg::init()?;

    let mut octx = format::output(&output_file)?;

    let codec = encoder::find(codec::Id::H264)
        .ok_or("H264 encoder not found")?;

    let mut ost = octx.add_stream(codec)?;
    let mut video_encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;

    video_encoder.set_width(width);
    video_encoder.set_height(height);
    video_encoder.set_format(format::Pixel::YUV420P);
    video_encoder.set_frame_rate(Some(Rational::new(fps, 1)));
    video_encoder.set_time_base(Rational::new(1, fps));

    let encoder = video_encoder.open_as(codec)?;
    ost.set_parameters(&encoder);
    octx.write_header()?;

    // RGBA → YUV scaler
    let mut scaler = scaling::Context::get(
        format::Pixel::RGBA,
        width,
        height,
        format::Pixel::YUV420P,
        width,
        height,
        scaling::Flags::BILINEAR,
    )?;

    let mut rgba_frame = frame::Video::new(format::Pixel::RGBA, width, height);
    let mut yuv_frame = frame::Video::new(format::Pixel::YUV420P, width, height);

    for (i, rgba_data) in rgba_frames.iter().enumerate() {
        // Copy RGBA → frame
        unsafe {
            let plane = std::slice::from_raw_parts_mut(
                rgba_frame.data_mut(0).as_mut_ptr(),
                rgba_data.len()
            );
            plane.copy_from_slice(rgba_data);
        }

        // RGBA → YUV
        scaler.run(&rgba_frame, &mut yuv_frame)?;
        yuv_frame.set_pts(Some(i as i64));

        encoder.send_frame(&yuv_frame)?;

        let mut packet = Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(ost.index());
            packet.write_interleaved(&mut octx)?;
        }
    }

    encoder.send_eof()?;
    let mut packet = Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(ost.index());
        packet.write_interleaved(&mut octx)?;
    }

    octx.write_trailer()?;
    Ok(())
}
```

---

## Pixel Formats

| Format | Description | Usage |
|--------|-------------|-------|
| `RGB24` | RGB without alpha (3 bytes/pixel) | Standard images |
| `RGBA` | RGB + alpha (4 bytes/pixel) | With transparency |
| `YUV420P` | YUV planar | **For H.264/H.265** |
| `YUV444P` | YUV without subsampling | High quality |
| `GRAY8` | Grayscale | Monochrome |

---

## H.264 Encoder Settings

```rust
let mut opts = ffmpeg::Dictionary::new();

// Preset (speed ↔ quality)
opts.set("preset", "ultrafast"); // ultrafast, fast, medium, slow, veryslow
opts.set("preset", "medium");    // balanced (recommended)

// CRF (quality): 0 = lossless, 23 = default, 51 = worst
opts.set("crf", "18");  // high quality
opts.set("crf", "23");  // standard
opts.set("crf", "28");  // low quality, small size

// H.264 profile
opts.set("profile", "baseline"); // for compatibility
opts.set("profile", "high");     // for high quality

// Level
opts.set("level", "4.0");

// Tune (optimization)
opts.set("tune", "film");      // for movies
opts.set("tune", "animation"); // for animation
opts.set("tune", "zerolatency"); // for streaming
```

---

## Useful Functions

```rust
// List available codecs
for codec in ffmpeg::encoder::list() {
    if codec.is_video() {
        println!("{:?}: {}", codec.id(), codec.name());
    }
}

// List supported formats for codec
let codec = encoder::find(codec::Id::H264).unwrap();
for format in codec.formats() {
    println!("{:?}", format);
}

// Set bitrate instead of CRF
video_encoder.set_bit_rate(5_000_000); // 5 Mbps
```

---

## Building the Project

```powershell
# Windows
cargo build --release

# If FFmpeg not found, specify path
$env:FFMPEG_DIR = "C:\ffmpeg"
cargo build --release
```

---

## Examples in Project

Check `examples/` for complete examples:
- `transcode-x264.rs` - transcode to H.264
- `dump-frames.rs` - extract frames
- `remux.rs` - remux without encoding

---

## Common Errors

### 1. "Encoder not found"
```rust
// Check that FFmpeg is built with libx264
ffmpeg::init()?;
if encoder::find(codec::Id::H264).is_none() {
    panic!("H264 not supported");
}
```

### 2. Invalid pixel format
```rust
// H.264 requires YUV420P (or use swscale)
video_encoder.set_format(format::Pixel::YUV420P); // ✓
```

### 3. Not calling `send_eof()`
```rust
// Always flush encoder after all frames
encoder.send_eof()?;
while encoder.receive_packet(&mut packet).is_ok() {
    // process remaining packets
}
```

---

## Quick Start: RGBA Frames → MP4

```rust
use ffmpeg_next as ffmpeg;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    ffmpeg::init()?;

    // Your frames in memory (example: 100 red frames 1920x1080)
    let width = 1920u32;
    let height = 1080u32;
    let mut frames = Vec::new();

    for _ in 0..100 {
        let rgba_data = vec![255u8, 0, 0, 255; (width * height) as usize];
        frames.push(rgba_data);
    }

    // Write to MP4
    encode_rgba_to_mp4("output.mp4", &frames, width, height, 30)?;

    Ok(())
}

// Use function from Example 3 above
```

**Done!** File `output.mp4` created.

---

## Deployment

### ⭐ Recommended: vcpkg Static Linking (Windows)

**Best option for Windows deployment** - FFmpeg embedded in your executable, no DLLs needed.

**1. Install FFmpeg via vcpkg:**

```powershell
# Static linking (recommended for Rust)
vcpkg install ffmpeg:x64-windows-static-md

# Or fully static (including CRT)
vcpkg install ffmpeg:x64-windows-static
```

**2. Configure Windows system libraries (Windows only):**

vcpkg FFmpeg requires Windows system libraries. This project includes `.cargo/config.toml` with all required libraries:

```toml
# .cargo/config.toml (already included in rust-ffmpeg)
[target.x86_64-pc-windows-msvc]
rustflags = [
    "-l", "bcrypt",    # For random number generation
    "-l", "user32",    # For window management
    "-l", "ole32",     # For COM
    "-l", "oleaut32",  # For OLE automation
    "-l", "mfuuid",    # For Media Foundation
    "-l", "strmiids",  # For DirectShow
    "-l", "mfplat",    # For Media Foundation Platform
    "-l", "secur32",   # For security
    "-l", "ws2_32",    # For Windows Sockets
    "-l", "shlwapi",   # For shell API
    "-l", "gdi32",     # For GDI functions
    "-l", "vfw32",     # For Video for Windows
]
```

**For your own project:** Copy `.cargo/config.toml` from this repo to your project root.

**3. Build your project:**

```powershell
# vcpkg will be detected automatically if VCPKG_ROOT is set
cargo build --release
```

If vcpkg is not in your PATH, set the root:

```powershell
$env:VCPKG_ROOT = "C:\vcpkg"
cargo build --release
```

**4. Result:**

✅ **Single `.exe` file** - no DLLs needed
✅ **30-60 MB executable** (FFmpeg embedded)
✅ **Works anywhere** - no FFmpeg installation required
✅ **No version conflicts** - your FFmpeg version is locked

**Triplet explanation:**
- `x64-windows-static-md` - Static vcpkg libs + dynamic CRT (best for Rust)
- `x64-windows-static` - Fully static (requires `RUSTFLAGS=-Ctarget-feature=+crt-static`)
- `x64-windows` - Dynamic DLLs (see below)

---

### Dynamic Linking (Default)

By default, `ffmpeg-next` uses **dynamic linking** - it links to FFmpeg DLLs on your system.

**Windows deployment steps:**

1. **Download FFmpeg shared libraries:**
   - Visit https://www.gyan.dev/ffmpeg/builds/
   - Download "Shared" build (contains .dll files)
   - Extract to a folder (e.g., `C:\ffmpeg`)

2. **During development:**
   ```powershell
   # Set FFmpeg path
   $env:FFMPEG_DIR = "C:\ffmpeg"

   # Add FFmpeg bin to PATH
   $env:PATH = "C:\ffmpeg\bin;$env:PATH"

   # Build your project
   cargo build --release
   ```

3. **For deployment, include these DLLs with your .exe:**
   - `avcodec-XX.dll`
   - `avformat-XX.dll`
   - `avutil-XX.dll`
   - `swscale-XX.dll`
   - `swresample-XX.dll` (if using audio)
   - `avfilter-XX.dll` (if using filters)

   Copy from `C:\ffmpeg\bin\` to your application directory:
   ```powershell
   # Copy FFmpeg DLLs to your release folder
   Copy-Item "C:\ffmpeg\bin\*.dll" -Destination "target\release\"
   ```

4. **Distribution structure:**
   ```
   your-app/
   ├── your-app.exe
   ├── avcodec-61.dll
   ├── avformat-61.dll
   ├── avutil-59.dll
   ├── swscale-8.dll
   └── swresample-5.dll
   ```

**⚠️ Legal requirements:**
- When distributing FFmpeg DLLs, you must comply with FFmpeg's license (LGPL 2.1+)
- Provide FFmpeg source code corresponding to your DLL versions
- Don't rename DLLs to obfuscate them (adding prefix/suffix is OK)
- See: https://ffmpeg.org/legal.html

---

### Static Linking

Static linking embeds FFmpeg into your executable - larger binary but **no DLL dependencies**.

**Enable static linking:**

```toml
[dependencies]
ffmpeg-next = { version = "8.0", features = ["static", "build"] }
```

**Build with static FFmpeg:**

```toml
# Full static build example
[dependencies]
ffmpeg-next = {
    version = "8.0",
    features = [
        "static",
        "build",
        "build-lib-x264",  # Enable H.264
        "build-lib-x265",  # Enable H.265
    ]
}
```

This will:
- Download and compile FFmpeg from source
- Link statically into your binary
- Take **much longer** to build (first time)
- Result in **larger** executable
- **No DLL dependencies** needed for deployment

**Pros:** Simple deployment (single .exe)
**Cons:** Large binary size, long build times

---

### Checking DLL Dependencies

To verify which DLLs your executable needs:

```powershell
# Using dumpbin (Visual Studio)
dumpbin /dependents target\release\your-app.exe

# Or use Dependencies.exe (GUI tool)
# Download from: https://github.com/lucasg/Dependencies
```

---

### Recommended Deployment Strategy

**🥇 Best (Windows):** vcpkg static linking (`ffmpeg:x64-windows-static-md`)
- Single executable, no DLLs, works anywhere
- Perfect for production apps

**🥈 Quick prototypes:** Dynamic linking + copy DLLs
- Fast development, manual DLL distribution

**🥉 Cross-platform:** Feature `build` (compiles FFmpeg from source)
- Longest build time, but works on any platform
- Good for open-source projects

**For installers:** Use dynamic linking with NSIS/WiX to bundle DLLs
