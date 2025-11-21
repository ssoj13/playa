# Playa - Project Context & Guide

## Project Overview
**Playa** is a high-performance, cross-platform image sequence player written in Rust. It is designed for VFX and animation workflows, supporting EXR (including DWAA/DWAB via optional OpenEXR backend), PNG, JPEG, TIFF, and various video formats via FFmpeg.

**Key Technologies:**
- **UI:** `egui`, `eframe`, `egui_dock` (docking layout), `egui_glow` (OpenGL backend).
- **Concurrency:** `rayon` (worker pool), `crossbeam` (channels), `AtomicU64` (epoch counters).
- **Media:** `image` crate (Rust-native formats), `playa-ffmpeg` (video decoding/encoding), `openexr` (optional C++ bindings).
- **Build System:** `xtask` (Rust-based automation) + `bootstrap.ps1`/`.sh`.

## 🚀 Quick Start & Build System

### Windows (PowerShell) - **MANDATORY**
On Windows, **ALWAYS** use PowerShell (`pwsh.exe` or `powershell.exe`). Never use `cmd` or `bash`.

```powershell
# 1. Bootstrap (Handles dependencies, vcpkg, environment)
.\bootstrap.ps1              # Show help
.\bootstrap.ps1 build        # Build with default 'exrs' backend (Pure Rust, Fast)
.\bootstrap.ps1 build --openexr # Build with OpenEXR C++ backend (Full support)
.\bootstrap.ps1 test         # Run all tests

# 2. Run
.\target\debug\playa.exe
```

### Linux / macOS (Bash)
```bash
./bootstrap.sh build
./target/debug/playa
```

### `cargo xtask` Automation
The project uses `xtask` for all build and release operations. After bootstrapping, you can use:

- `cargo xtask build [--release] [--openexr]` - Build the project.
- `cargo xtask test` - Run tests.
- `cargo xtask wipe` - Clean artifacts (useful when switching backends).
- `cargo xtask deploy` - Install to system.

## 🏗️ Architecture

### Core Concepts
1.  **Project (`Project`)**: The root container. Holds a playlist of `Comp` objects and global settings.
2.  **Composition (`Comp`)**: Can be a single sequence (File Mode) or a multi-layer blend (Layer Mode).
3.  **Frame (`Frame`)**: The fundamental unit. Loaded asynchronously. State transitions: `Placeholder` -> `Header` -> `Loading` -> `Loaded` / `Error`.
4.  **Workers**: A global `rayon` thread pool handles frame loading and video encoding.
5.  **Epoch Counter**: Used for async cancellation. Incrementing the epoch invalidates pending load requests from previous seek/scrub actions.

### Data Flow
1.  **User Interaction**: Drag-drop file or scrub timeline.
2.  **Event Bus**: UI events are sent to the `EventBus`.
3.  **Sequence Detection**: `utils::sequences` detects patterns (e.g., `render.####.exr`).
4.  **Loading**: `Frame` requests are sent to `workers`.
5.  **Caching**: Loaded frames are stored in an LRU cache (`cache.rs`).
6.  **Rendering**: `ViewportRenderer` uploads textures to GPU via OpenGL (`glow`).

### Directory Structure & Key Files

- **`src/`**
    - **`main.rs`**: Entry point. App state, `egui_dock` setup, main loop.
    - **`ui.rs`**: High-level UI layout and rendering delegates.
    - **`player.rs`**: Playback logic (play, pause, seek, FPS control).
    - **`workers.rs`**: Global thread pool configuration.
    - **`events.rs`**: `EventBus` and `CompEvent` definitions.
    - **`entities/`**: Core data models.
        - `project.rs`: `Project` struct.
        - `comp.rs`: `Comp` (Composition) logic.
        - `frame.rs`: `Frame` loading and state management.
        - `loader.rs`: Image decoding logic.
    - **`widgets/`**: Reusable UI components.
        - `viewport/`: OpenGL rendering, zooming, panning.
        - `timeline/`: Scrubbing, frame ticks, load indicators.
    - **`dialogs/`**: Modal windows (Settings, Encode).
    - **`utils/sequences.rs`**: File pattern matching and sequence detection.

- **`xtask/`**: Build automation logic (Rust).
- **`bootstrap.ps1` / `bootstrap.sh`**: Environment setup scripts.

## ⚠️ Conventions & Guidelines

- **Shell on Windows**: Use **PowerShell**. Do not use `cmd` or Git Bash for build commands.
- **Dependencies**: `vcpkg` is required for FFmpeg and OpenEXR on Windows. The bootstrap script configures `VCPKG_ROOT` and `VCPKGRS_TRIPLET` automatically.
- **Code Style**: Rust 2024 edition (1.85+). Standard Rust formatting (`cargo fmt`).
- **Logging**: Use `log::info!`, `debug!`, etc. Run with `RUST_LOG=debug` to see output.
- **Refactoring**: The project recently moved to `egui_dock`. Ensure new widgets respect the dock layout.
- **Testing**: Run `.\bootstrap.ps1 test` before committing.

## 🐛 Debugging

- **Logs**: `.\target\debug\playa.exe --log` writes to `playa.log`.
- **Visual Debugging**:
    - **Timeline**: Colored bars show frame status (Grey=Placeholder, Blue=Header, Orange=Loading, Green=Ready).
    - **Status Bar**: Shows cache usage and worker status.

## Release Process
1.  `cargo xtask tag-dev patch` (Creates dev tag, triggers CI build)
2.  `cargo xtask pr <version>` (Creates PR to main)
3.  Merge PR.
4.  `cargo xtask tag-rel patch` (Creates release tag, triggers release workflow)
