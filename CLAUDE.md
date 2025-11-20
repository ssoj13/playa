# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Playa is an image sequence player built in Rust with egui/eframe for UI and OpenGL rendering. It's an active learning project combining Rust systems programming with VFX/compositing workflows. The codebase supports dual EXR backends (pure Rust `exrs` and C++ `openexr`) plus FFmpeg integration for video playback/encoding.

## Essential Build Commands

### Quick Start (Bootstrap Scripts)

**Always use bootstrap scripts for initial builds** - they handle all dependency setup automatically:

```powershell
# Windows (PowerShell)
.\bootstrap.ps1 build          # Build with exrs backend (fast, pure Rust)
.\bootstrap.ps1 build --openexr  # Build with OpenEXR C++ backend (full DWAA/DWAB)
.\bootstrap.ps1 test           # Run all tests
```

```bash
# Linux/macOS
./bootstrap.sh build
./bootstrap.sh build --openexr
./bootstrap.sh test
```

### Direct xtask Commands (After Bootstrap)

Once bootstrap has run, you can use `cargo xtask` directly:

```bash
# Build variants
cargo xtask build                    # Release build, exrs backend
cargo xtask build --debug            # Debug build, exrs backend
cargo xtask build --openexr          # Release build, OpenEXR C++ backend
cargo xtask build --debug --openexr  # Debug build, OpenEXR C++ backend

# Testing
cargo xtask test                     # Run all tests (unit + integration)
cargo xtask test --debug             # Run tests in debug mode
cargo xtask test --nocapture         # Show println! output

# Development helpers
cargo xtask verify                   # Verify dependencies present
cargo xtask deploy                   # Install to system (Windows: %LOCALAPPDATA%\Programs, Linux: ~/.local/bin)

# Release workflow
cargo xtask tag-dev patch            # Create dev tag → trigger CI build workflow
cargo xtask pr v0.1.60               # Create PR: dev → main
cargo xtask tag-rel patch            # Tag release on main → trigger Release workflow
```

### Standard Cargo Commands

```bash
# Direct cargo (exrs backend only, no dependency management)
cargo build --release
cargo run --release -- path/to/image.0001.exr

# With OpenEXR backend (requires --features flag)
cargo build --release --features openexr
```

**Important**: Direct `cargo build` works for exrs backend but won't handle native library copying for OpenEXR. Use `cargo xtask build --openexr` for full functionality.

## Architecture Overview

### Application Structure

Playa uses a **composition-based architecture** inspired by After Effects/Nuke:

```
PlayaApp (main.rs)
  ├─ Player (playback engine)
  │   └─ Project (playlist + composition hierarchy)
  │       └─ Comp (unified composition/clip entity)
  │           ├─ Layer mode: Composes children comps recursively
  │           └─ File mode: Loads image sequence from disk
  │
  ├─ Workers (global thread pool)
  │   ├─ Frame loading workers (75% CPU cores)
  │   └─ Video encoding worker (background)
  │
  ├─ EventBus (application-wide events)
  │   └─ CompEventSender (comp-specific events)
  │
  └─ UI Widgets
      ├─ Viewport (OpenGL rendering)
      ├─ Timeline (scrubbing, play range)
      ├─ Project panel (playlist)
      └─ Status bar
```

### Key Architectural Concepts

**Comp (src/entities/comp.rs)**
- Unified entity replacing old Clip/Sequence split
- Dual-mode operation: Layer composition OR File sequence loading
- All editable properties stored in `attrs: Attrs` (type-safe HashMap)
- Children managed via UUID references, not direct ownership
- Supports transform attributes (position, rotation, scale, opacity)
- Work area (play_start/play_end) defines visible timeline range

**Project (src/entities/project.rs)**
- Central registry: `media: HashMap<String, Comp>` (UUID → Comp)
- Playlist: `clips_order: Vec<String>` (ordered UUIDs for playback)
- Owns all comps; player references by UUID
- Handles comp lifecycle (create, delete, reorder)

**Player (src/player.rs)**
- Stateless playback controller
- References active comp via `active_comp: Option<String>` (UUID)
- JKL shuttle controls with FPS presets (1, 2, 4, 8, 12, 24, 30, 60, 120, 240)
- Frame-accurate timing (not wall-clock based)
- Playback loop advances by frame count, not time delta

**Workers (src/workers.rs)**
- Global thread pool: `Arc<Workers>` shared across app
- Async frame loading (rayon ParIter for parallel decode)
- Background video encoding (cancellable via `Arc<AtomicBool>`)
- Message passing via crossbeam channels

**EventBus (src/events.rs)**
- Application-wide events: `AppEvent` (global state changes)
- Composition events: `CompEvent` (frame changes, layer updates)
- Decouples UI from business logic
- Enables reactive updates across widgets

### Module Responsibilities

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `main.rs` | Entry point, CLI args, main loop | `PlayaApp`, `DockTab` |
| `player.rs` | Playback engine | `Player` |
| `entities/comp.rs` | Composition/clip descriptor | `Comp`, `CompMode` |
| `entities/project.rs` | Central registry | `Project` |
| `entities/frame.rs` | Image data container | `Frame`, `FrameData` |
| `entities/loader.rs` | Image decoding (EXR/PNG/JPEG/TIFF) | `load_frame()` |
| `entities/loader_video.rs` | FFmpeg video decoding | `VideoDecoder` |
| `entities/attrs.rs` | Type-safe attribute system | `Attrs`, `AttrValue` |
| `workers.rs` | Thread pool management | `Workers` |
| `events.rs` | Event bus | `EventBus`, `CompEvent` |
| `widgets/viewport/*` | OpenGL rendering | `ViewportRenderer`, `ViewportState`, `Shaders` |
| `widgets/timeline/*` | Timeline UI | `TimelineState`, timeline helpers |
| `widgets/project/*` | Playlist panel | Project UI rendering |
| `widgets/status/*` | Status bar | `StatusBar` |
| `dialogs/encode/*` | Video encoding | `EncodeDialog` |
| `dialogs/prefs/*` | Settings | `AppSettings` |

## Critical Implementation Details

### Comp Attribute System

All comp properties are stored in `attrs: Attrs`:

```rust
// Reading attributes
let name = comp.get_attr_str("name").unwrap_or("Untitled");
let start = comp.get_attr_uint("start").unwrap_or(0);
let fps = comp.get_attr_float("fps").unwrap_or(24.0);

// Writing attributes
comp.set_attr("name", AttrValue::Str("My Comp".into()));
comp.set_attr("start", AttrValue::UInt(101));
comp.set_attr("fps", AttrValue::Float(30.0));

// Transform attributes
comp.set_attr("position", AttrValue::Vec3(0.0, 0.0, 0.0));
comp.set_attr("rotation", AttrValue::Vec3(0.0, 0.0, 45.0));
comp.set_attr("transparency", AttrValue::Float(0.8));
```

**Why this matters**: Don't add new fields to `Comp` struct. Add new attributes to the `attrs` HashMap. Keeps struct serializable and extensible.

### UUID-Based References

Never store `Comp` instances directly. Always reference by UUID:

```rust
// ❌ BAD: Direct ownership
struct Player {
    active_comp: Option<Comp>,  // Will cause lifetime issues
}

// ✅ GOOD: UUID reference
struct Player {
    active_comp: Option<String>,  // UUID
    project: Project,             // Owns all comps
}

// Access via project
fn get_active(&self) -> Option<&Comp> {
    self.active_comp.as_ref()
        .and_then(|uuid| self.project.media.get(uuid))
}
```

### Event-Driven Updates

Use EventBus for cross-widget communication:

```rust
// Sending events
self.comp_event_sender.send(CompEvent::FrameChanged {
    comp_uuid: comp.uuid.clone(),
    frame: new_frame
});

// Receiving events (in update loop)
while let Ok(event) = self.comp_event_receiver.try_recv() {
    match event {
        CompEvent::FrameChanged { comp_uuid, frame } => {
            // Update UI, invalidate cache, etc.
        }
        // ...
    }
}
```

### FFmpeg Integration

Video support via `playa-ffmpeg` crate:

- **Decoding**: `VideoDecoder` (src/entities/loader_video.rs) - seeks frames, converts pixel formats
- **Encoding**: `EncodeDialog` (src/dialogs/encode/encode.rs) - background worker, progress tracking
- **Hardware acceleration**: NVENC, QSV, AMF detected automatically
- **Static linking**: Uses vcpkg for FFmpeg libraries (VCPKG_ROOT env var)

## Development Workflow

### Adding New Features

1. **Don't add struct fields** - use Comp attributes instead
2. **Use EventBus** - don't directly mutate shared state from UI
3. **Reference by UUID** - never store Comp instances directly
4. **Test in both modes** - verify Layer mode and File mode behavior
5. **Check both backends** - test with exrs and openexr builds

### Common Pitfalls

**Issue**: Build fails with "cannot find FFmpeg libraries"
**Fix**: Ensure `VCPKG_ROOT` environment variable is set, vcpkg has FFmpeg installed

**Issue**: OpenEXR build fails on Linux with "uint64_t not declared"
**Fix**: Run `cargo xtask pre` to patch OpenEXR headers (GCC 11+ compatibility)

**Issue**: Native libraries not found at runtime (OpenEXR backend)
**Fix**: Run `cargo xtask post` or use `cargo xtask build --openexr` which does it automatically

**Issue**: Changes to Comp not persisting
**Fix**: Attributes are stored in HashMap - ensure you're calling `set_attr()`, not modifying cached values

### Testing Strategy

- Unit tests: Component-level behavior
- Integration tests: End-to-end workflows (encoding, cache, sequence detection)
- Manual testing: Use `cargo xtask build --debug` for faster iteration

## Release Process

Playa uses a **dev → main PR workflow** with automated CI/CD:

1. **Development**: Work on `dev` branch
2. **Dev tag**: `cargo xtask tag-dev patch` → triggers CI build (artifacts, no release)
3. **Pull Request**: `cargo xtask pr v0.1.60` → creates dev → main PR
4. **Merge PR**: Review and merge on GitHub
5. **Release tag**: `git checkout main && git pull && cargo xtask tag-rel patch` → triggers Release workflow
6. **Installers**: CI builds DMG, MSI, DEB, AppImage, code-signs macOS

**Branch detection**: CI uses git to check if tag is on `main` or `dev` - determines whether to create GitHub Release.

## Platform-Specific Notes

### Windows
- Uses PowerShell (`pwsh.exe` or `powershell.exe`) - **never use bash**
- Bootstrap script: `.\bootstrap.ps1`
- Installer: NSIS (.exe) and MSI
- Hardware encoding: NVENC (NVIDIA), QSV (Intel), AMF (AMD)

### Linux
- OpenEXR headers need patching for GCC 11+: `cargo xtask pre`
- Bootstrap script: `./bootstrap.sh`
- Installer: DEB, AppImage
- Hardware encoding: NVENC (requires CUDA drivers)

### macOS
- Code-signed and notarized DMG installers
- Bootstrap script: `./bootstrap.sh`
- Hardware encoding: VideoToolbox (requires custom FFmpeg build)
- Apple Silicon: Use `arm64-osx-release` vcpkg triplet

## Code Style

- **Rust 2024 edition** - use latest language features
- **Concise names**: `get_tr()` not `extract_translation()`
- **Doc comments**: Explain "why", not "what"
- **Error handling**: Use `anyhow::Result` for fallible operations
- **Logging**: `log::info!()` for state changes, `log::debug!()` for verbose
- **No warnings**: Fix all compiler warnings before committing

## Dependencies to Know

- **UI**: egui 0.33, eframe, egui_dock 0.18, egui_ltreeview 0.6
- **Graphics**: OpenGL via glow, egui_glow
- **Image formats**:
  - exr: `image` crate with `exr` feature (pure Rust)
  - openexr: `openexr` crate (C++ bindings, optional)
  - Other: PNG/JPEG/TIFF/TGA via `image` crate
- **Video**: `playa-ffmpeg` 8.0.3 (custom wrapper around rust-ffmpeg)
- **Concurrency**: rayon, crossbeam-channel, std::sync (Arc, Mutex, AtomicBool)
- **CLI**: clap 4.5 with derive macros
- **Serialization**: serde, serde_json

## Files to Check Before Major Changes

- `Cargo.toml` - dependency versions, feature flags
- `xtask/src/main.rs` - build automation logic
- `.github/workflows/main.yml` - CI/CD pipeline
- `bootstrap.ps1` / `bootstrap.sh` - dependency setup scripts
- `src/entities/comp.rs` - core composition logic
- `src/events.rs` - event types and bus

## Helpful Resources

- README.md - User-facing documentation, installation, usage
- CHANGELOG.md - Version history, breaking changes
- Architecture diagrams in README.md - visual guide to data flow
- xtask help: `cargo xtask --help` - full command reference
