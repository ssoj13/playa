# Playa - Image Sequence Player

[![Release](https://img.shields.io/github/v/release/ssoj13/playa)](https://github.com/ssoj13/playa/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/ssoj13/playa/total)](https://github.com/ssoj13/playa/releases)
[![License](https://img.shields.io/github/license/ssoj13/playa)](LICENSE)
[![Changelog](https://img.shields.io/badge/changelog-CHANGELOG.md-blue)](CHANGELOG.md)

![Screenshot](.github/screenshot.jpg)

---

## What is Playa?

Initially started as a simple EXR Image Image Sequence Player in Rust. The aim was to actually learn Rust and create something useful.
LLMs and agentic applications like Claude Code, Codex, Qwen and the rest provide enormous help here and allow to perform 10x-100x faster,
so I started to add some more features upon my friends requests: flexible viewport, layers, timeline, attribute editor, h264/265/ProRes encoding,
REST api, multithreaded composing with LRU cache and more.


## Key Features

### System
- **Single binary, cross-platform** - One executable, no dependencies.
- **Download and run on Windows, macOS, or Linux**

### Performance
- **Instant scrubbing** - Epoch-based cache keeps UI responsive at any speed
- **Parallel loading** - Work-stealing across CPU cores
- **Smart memory** - LRU cache with configurable memory limit
- **JKL shuttle** - Industry-standard transport with speed ramping

### Format Support
- **EXR** - Via exrs (pure Rust) or OpenEXR C++ (DWAA/DWAB compression)
- **Images** - PNG, JPEG, TIFF, TGA, HDR
- **Video** - MP4, MOV, AVI, MKV via FFmpeg
- **Pixel formats** - 8-bit, 16-bit half-float, 32-bit float

### Video Export
- **Hardware encoding** - NVENC (NVIDIA), QSV (Intel), AMF (AMD)
- **Software encoding** - H.264, H.265 via libx264/libx265
- **Range export** - Encode only selected frame range (B/N markers)

### Compositing
- **Node-based** - FileNode, CompNode, CameraNode, TextNode
- **Blend modes** - Normal, Screen, Add, Subtract, Multiply, Divide, Difference
- **3D transforms** - Position, Rotation, Scale with perspective camera
- **Layer effects** - Gaussian Blur, Brightness/Contrast, HSV (CPU)
- **Interactive gizmos** - Move/Rotate/Scale manipulation in viewport

### Integration
- **Smart sequence detection** - Load one frame, finds all automatically
- **REST API** - Remote control via HTTP endpoints
- **Custom GLSL shaders** - Drop shaders in `shaders/` folder
- **Persistent state** - Remembers settings between sessions

---

## Installation

### Download Pre-built Binaries

Download from [Releases](https://github.com/ssoj13/playa/releases/latest):

| Platform | Recommended | Alternative |
|----------|-------------|-------------|
| **Windows** | `playa-x.x.x-exrs-x64-setup.exe` | `.msi`, portable `.zip` |
| **macOS** | `playa-x.x.x-exrs.dmg` | OpenEXR variant for DWAA/DWAB |
| **Linux** | `playa-x.x.x-exrs.AppImage` | `.deb` package |

**macOS**: All DMG releases are code-signed and notarized.

### Build from Source

See [DEVELOP.md](DEVELOP.md) for build instructions.

```powershell
git clone https://github.com/ssoj13/playa.git && cd playa
./bootstrap.ps1 build              # Windows (exrs backend)
./bootstrap.ps1 build --openexr    # Windows (OpenEXR C++)
./bootstrap.sh build               # Linux/macOS
```

---

## Quick Start

```bash
# Launch empty (drag-drop files)
playa

# Load sequence (auto-detects all frames)
playa render.0001.exr

# Load with options
playa -f sequence.exr --frame 50 -a -F    # Frame 50, autoplay, fullscreen
```

**Version info** (`-V`):
```
playa 0.1.138
EXR:    openexr-rs 0.11 (C++, DWAA/DWAB)
Video:  playa-ffmpeg 8.0 (static)
Target: x86_64-windows
```

---

## User Interface

### Panels

| Panel | Hotkey | Description |
|-------|--------|-------------|
| **Viewport** | - | Image display with zoom/pan |
| **Timeline** | - | Layer timeline with trim/move |
| **Project** | `F2` | Media pool |
| **Attributes** | `F3` | Layer properties |
| **Encode** | `F4` | Video export |
| **Settings** | `F12` | Preferences |
| **Help** | `F1` | Keyboard shortcuts |

### Viewport

| Action | Control |
|--------|---------|
| **Zoom** | Mouse wheel (centers on cursor) |
| **Pan** | Middle mouse drag |
| **Fit** | `F` |
| **100%** | `A` or `H` |
| **Fullscreen** | `Z` |
| **Scrub** | Right click + drag |
| **Pick layer** | Left click (Select mode Q) |

### Tools

| Key | Tool |
|-----|------|
| `Q` | Select/Scrub |
| `W` | Move |
| `E` | Rotate |
| `R` | Scale |

---

## Keyboard Shortcuts

### Playback

| Key | Action |
|-----|--------|
| `Space` | Play/Pause |
| `K` | Stop |
| `J` / `L` | Jog backward/forward (cumulative) |
| `Left` / `Right` | Step 1 frame |
| `Shift+Arrows` | Step 25 frames |
| `Home` / `End` | Jump to start/end |
| `;` / `'` | Prev/Next layer edge |
| `` ` `` | Toggle loop |
| `-` / `=` | Decrease/Increase FPS |

### Play Range

| Key | Action |
|-----|--------|
| `B` | Set range start |
| `N` | Set range end |
| `Ctrl+B` | Reset to full range |

### Timeline

| Key | Action |
|-----|--------|
| `[` | Align start to cursor |
| `]` | Align end to cursor |
| `Alt+[` | Trim start to cursor |
| `Alt+]` | Trim end to cursor |
| `Ctrl+D` | Duplicate layers |
| `Delete` | Delete layer |

### Global

| Key | Action |
|-----|--------|
| `F1` | Help |
| `F2` | Project panel |
| `F3` | Attributes panel |
| `F4` | Encode dialog |
| `F12` | Settings |
| `Z` | Fullscreen |
| `Ctrl+S` | Save project |
| `Ctrl+O` | Open project |

---

## Workflows

### Review Sequence

1. Drag-drop folder or `playa render.0001.exr`
2. `Space` to play, `J`/`L` for shuttle
3. Mouse wheel to zoom, middle-drag to pan

### Export to Video

1. Load sequence
2. Set range: `B` (start), `N` (end)
3. `F4` - encode dialog
4. Select codec, click "Encode"

### Composite Layers

1. Create composition (right-click in Project)
2. Drag clips to timeline
3. Transform with W/E/R tools
4. Adjust blend mode in Attributes (F3)
5. Add effects (Blur, Brightness, HSV)

### Remote Control

Enable in Settings > Web Server:

```bash
curl http://localhost:8080/api/status
curl -X POST http://localhost:8080/api/player/play
curl -X POST http://localhost:8080/api/player/frame/100
```

---

## Architecture

```
+--------------------------------------------------+
|                    PlayaApp                       |
+--------------------------------------------------+
|  Player          |  Viewport      |  Timeline    |
|  (state machine) |  (OpenGL)      |  (layers)    |
+--------------------------------------------------+
|  EventBus        |  GlobalCache   |  Workers     |
|  (pub/sub)       |  (LRU + epoch) |  (parallel)  |
+--------------------------------------------------+
|  Project (media pool) | Nodes (File/Comp/Camera) |
+--------------------------------------------------+
```

**How acceleration works:**

1. **User scrubs** - SetFrameEvent emitted
2. **Epoch increments** - Previous frame requests marked stale
3. **Cache check** - Return immediately if cached
4. **Worker dispatch** - Job added to work-stealing queue
5. **Parallel load** - Workers compete to process jobs
6. **Epoch validation** - Workers skip stale requests
7. **Cache insert** - Result stored, UI repainted

For detailed architecture, see [AGENTS.md](AGENTS.md).

---

## Documentation

| Document | Description |
|----------|-------------|
| [DEVELOP.md](DEVELOP.md) | Build from source, FFmpeg setup |
| [AGENTS.md](AGENTS.md) | Architecture, dataflow diagrams |
| [CHANGELOG.md](CHANGELOG.md) | Version history |

---

## About

Built with Rust. Powered by exrs, openexr-rs, playa-ffmpeg, egui, and the Rust ecosystem.

**Icon**: [Flaticon by Yasashii std](http://flaticon.com/packs/halloween-18020037)

---

*For build instructions, see [DEVELOP.md](DEVELOP.md).*
*For architecture details, see [AGENTS.md](AGENTS.md).*
