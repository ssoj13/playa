# Playa - Image Sequence Player

[![Release](https://img.shields.io/github/v/release/ssoj13/playa)](https://github.com/ssoj13/playa/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/ssoj13/playa/total)](https://github.com/ssoj13/playa/releases)
[![License](https://img.shields.io/github/license/ssoj13/playa)](LICENSE)
[![Changelog](https://img.shields.io/badge/changelog-CHANGELOG.md-blue)](CHANGELOG.md)

Professional image sequence player and video compositor. Single binary, cross-platform.

![Screenshot](.github/screenshot.jpg)

---

## Key Features

### Playback & Performance
- **Instant response** - Epoch-based cache cancellation keeps UI responsive during fast scrubbing
- **Smart memory management** - Automatic LRU cache, never runs out of memory
- **Parallel loading** - Uses 75% of CPU cores for background frame loading
- **JKL shuttle control** - Professional transport controls with speed ramping

### Video & Encoding
- **Video playback** - MP4, MOV, AVI, MKV via FFmpeg
- **Hardware encoding** - NVENC (NVIDIA), QSV (Intel), AMF (AMD)
- **Software encoding** - H.264, H.265 via libx264/libx265
- **Play range export** - Encode only selected frame range (B/N markers)

### Compositing
- **Node-based** - FileNode, CompNode, CameraNode, TextNode
- **Blend modes** - Normal, Screen, Add, Subtract, Multiply, Divide, Difference
- **3D transforms** - Position, Rotation, Scale with perspective
- **Interactive gizmos** - Move/Rotate/Scale manipulation in viewport
- **Layer effects** - Gaussian Blur, Brightness/Contrast, HSV Adjust (CPU)

### Format Support
- **EXR** - Via exrs (pure Rust) or OpenEXR C++ (DWAA/DWAB)
- **Images** - PNG, JPEG, TIFF, TGA, HDR
- **Pixel formats** - U8, F16 (half-float), F32

### Additional Features
- **Smart sequence detection** - Load one frame, finds all automatically
- **Custom GLSL shaders** - Drop shaders in `shaders/` folder
- **REST API** - Remote control via HTTP endpoints
- **Persistent settings** - Remembers state between sessions
- **Cross-platform** - Windows, macOS, Linux

---

## Installation

### Download Pre-built Binaries (Recommended)

Download from [Releases](https://github.com/ssoj13/playa/releases/latest):

| Platform | Recommended | Alternative |
|----------|-------------|-------------|
| **Windows** | `playa-x.x.x-exrs-x64-setup.exe` | `.msi`, portable `.zip` |
| **macOS** | `playa-x.x.x-exrs.dmg` | OpenEXR variant for DWAA/DWAB |
| **Linux** | `playa-x.x.x-exrs.AppImage` | `.deb` package |

**macOS**: All DMG releases are code-signed and notarized - no Gatekeeper warnings.

### Build from Source

See [DEVELOP.md](DEVELOP.md) for build instructions, FFmpeg setup, and vcpkg configuration.

```bash
# Quick build (requires Rust)
git clone https://github.com/ssoj13/playa.git && cd playa
.\bootstrap.ps1 build   # Windows
./bootstrap.sh build    # Linux/macOS
```

---

## Quick Start

```bash
# Launch empty (drag-drop files)
playa

# Load sequence (auto-detects all frames)
playa render.0001.exr

# Load with options
playa -f sequence.exr --frame 50 -a -F    # Start at frame 50, autoplay, fullscreen
```

---

## User Interface

### Panels

| Panel | Hotkey | Description |
|-------|--------|-------------|
| **Viewport** | - | Main image display with zoom/pan |
| **Timeline** | - | Layer timeline with trim, move, selection |
| **Project** | `F2` | Media pool - all loaded clips |
| **Attributes** | `F3` | Properties of selected layer/node |
| **Encode** | `F4` | Video export dialog |
| **Settings** | `F12` | Preferences dialog |
| **Help** | `F1` | Keyboard shortcuts overlay |

### Viewport Controls

| Action | Control |
|--------|---------|
| **Zoom** | Mouse wheel (centers on cursor) |
| **Pan** | Middle mouse drag |
| **Fit to window** | `F` |
| **100% zoom** | `A` or `H` |
| **Fullscreen** | `Z` |
| **Scrub** | Right click + drag on image |
| **Pick layer** | Left click (Select mode Q) |

### Timeline Controls

| Action | Control |
|--------|---------|
| **Select layer** | Click |
| **Multi-select** | Ctrl/Shift + click |
| **Move layer** | Drag selected layer |
| **Zoom timeline** | `-` / `=` / `+` or Mouse wheel |
| **Pan timeline** | Middle mouse drag |
| **Align start to cursor** | `[` |
| **Align end to cursor** | `]` |
| **Trim start to cursor** | `Alt+[` |
| **Trim end to cursor** | `Alt+]` |
| **Delete layer** | `Delete` |

### Tool Modes

| Key | Tool | Description |
|-----|------|-------------|
| `Q` | **Select** | Scrub/selection mode |
| `W` | **Move** | Position gizmo |
| `E` | **Rotate** | Rotation gizmo |
| `R` | **Scale** | Scale gizmo |

---

## Keyboard Shortcuts

### Playback

| Key | Action |
|-----|--------|
| `Space` / `Insert` | Play/Pause |
| `K` / `/` | Stop |
| `J` / `,` | Jog backward (cumulative speed) |
| `L` / `.` | Jog forward (cumulative speed) |
| `Left` / `Right` | Step 1 frame |
| `Shift+Arrows` | Step 25 frames |
| `Ctrl+Left` / `Ctrl+Right` | Jump to Start/End |
| `1` / `Home` | Jump to start |
| `2` / `End` | Jump to end |
| `;` / `'` | Prev/Next layer edge |
| `` ` `` | Toggle loop |

### FPS Control

| Key | Action |
|-----|--------|
| `-` | Decrease base FPS |
| `=` / `+` | Increase base FPS |

Presets: 1, 2, 4, 8, 12, 24, 30, 60, 120, 240

### Play Range (Work Area)

| Key | Action |
|-----|--------|
| `B` | Set range start (begin) |
| `N` | Set range end |
| `Ctrl+B` | Reset to full range |

Used for: loop playback, encoding (F4)

### UI

| Key | Action |
|-----|--------|
| `F1` | Help overlay |
| `F2` | Project panel |
| `F3` | Attributes panel |
| `F4` | Encode dialog |
| `F12` | Settings/Preferences |
| `Z` | Fullscreen |
| `Backspace` | Toggle frame numbers |
| `Esc` | Exit fullscreen / Quit |
| `Ctrl+S` | Save project |
| `Ctrl+O` | Open project |

---

## Typical Workflows

### Review Image Sequence

1. Drag-drop folder or `playa render.0001.exr`
2. Press `Space` to play
3. Use `J`/`L` for shuttle control
4. Mouse wheel to zoom, middle-drag to pan

### Export to Video

1. Load sequence
2. Set play range: `B` (start), `N` (end)
3. Press `F4` to open encode dialog
4. Select codec and quality
5. Click "Encode"

### Composite Layers

1. Create composition (right-click in Project panel)
2. Drag clips to timeline
3. Select layer, use Move/Rotate/Scale tools
4. Adjust blend mode and opacity in Attributes panel
5. Add effects in Effects section (Gaussian Blur, Brightness/Contrast, HSV)

### Remote Control

Enable in Settings > Web Server, then:

```bash
curl http://localhost:8080/api/status
curl -X POST http://localhost:8080/api/player/play
curl -X POST http://localhost:8080/api/player/frame/100
```

---

## Timeline Visual Feedback

The timeline provides visual cues for multi-sequence work:

- **Color-coded zones** - Each sequence has unique color
- **Load indicator** - Shows frame status:
  - Dark gray: Not requested
  - Blue: Header only
  - Orange: Loading
  - Green: Loaded
  - Red: Error
- **Play range** - Highlighted region between B/N markers
- **Layer edges** - White dividers between sequences

---

## Architecture

```
┌──────────────────────────────────────────────────┐
│                    PlayaApp                       │
├──────────────────────────────────────────────────┤
│  Player          │  Viewport      │  Timeline    │
│  (state machine) │  (OpenGL)      │  (layers)    │
├──────────────────────────────────────────────────┤
│  EventBus        │  GlobalCache   │  Workers     │
│  (pub/sub)       │  (LRU + epoch) │  (parallel)  │
├──────────────────────────────────────────────────┤
│  Project (media pool) │ Nodes (File/Comp/Camera) │
└──────────────────────────────────────────────────┘
```

**Key concepts:**
- **Event-driven** - UI emits events, handlers update state
- **Epoch-based cancellation** - Stale frame requests are skipped
- **Work-stealing** - Background loading distributes work across cores

For detailed architecture, see [AGENTS.md](AGENTS.md).

---

## Documentation

| Document | Description |
|----------|-------------|
| [DEVELOP.md](DEVELOP.md) | Build from source, FFmpeg setup, xtask commands |
| [AGENTS.md](AGENTS.md) | Architecture, dataflow diagrams, AI guidelines |
| [CHANGELOG.md](CHANGELOG.md) | Version history |

---

## About

This is a learning project exploring Rust and AI-assisted development. Built with Claude Code and Codex.

**Acknowledgements:**
- App icon from [Flaticon by Yasashii std](http://flaticon.com/packs/halloween-18020037)
- Powered by exrs, openexr-rs, rust-ffmpeg, and the amazing Rust ecosystem

---

*For build instructions, see [DEVELOP.md](DEVELOP.md).*  
*For architecture details, see [AGENTS.md](AGENTS.md).*
