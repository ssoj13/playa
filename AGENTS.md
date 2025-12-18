# AGENTS.md - AI Agent Instructions for Playa

This document provides comprehensive instructions for AI agents working on the Playa codebase.

---

## Project Overview

**Playa** is an image sequence player built in Rust with egui/eframe UI framework.

- **Purpose**: VFX/animation image sequence playback with video encoding
- **Stack**: Rust 2024 edition, egui 0.33, OpenGL (glow), FFmpeg
- **Platforms**: Windows, Linux, macOS (cross-platform)
- **Repository**: `https://github.com/ssoj13/playa`

### Key Features
- Multi-format support: EXR (exrs/OpenEXR), PNG, JPEG, TIFF, TGA, MP4, MOV
- GPU-accelerated rendering with custom GLSL shaders
- Multi-threaded async frame loading with LRU cache
- Node-based compositing (FileNode, CompNode, CameraNode, TextNode)
- Video encoding via FFmpeg with hardware acceleration (NVENC, QSV, AMF)

---

## Architecture

### High-Level Structure

```
playa/
├── src/
│   ├── core/           # Engine (cache, events, player, workers)
│   ├── dialogs/        # Modal dialogs (prefs, encoder)
│   ├── entities/       # Data models (comp, frame, project, nodes)
│   ├── widgets/        # UI widgets (viewport, timeline, status, ae)
│   ├── main.rs         # Application entry point
│   ├── main_events.rs  # Centralized event handlers
│   ├── lib.rs          # Library crate exports
│   ├── cli.rs          # CLI argument parsing (clap)
│   ├── config.rs       # AppSettings persistence
│   ├── shell.rs        # OS integration (drag-drop, file dialogs)
│   ├── ui.rs           # UI utilities
│   └── utils.rs        # General utilities
├── xtask/              # Build automation (cross-platform)
├── shaders/            # Custom GLSL shaders
└── .github/workflows/  # CI/CD pipelines
```

### Core Modules

| Module | Purpose |
|--------|---------|
| `core/event_bus.rs` | Type-erased pub/sub event system |
| `core/global_cache.rs` | LRU frame cache (comp_uuid -> frame_idx -> Frame) |
| `core/cache_man.rs` | Memory budget management |
| `core/player.rs` | Playback state machine |
| `core/workers.rs` | Thread pool for async frame loading |

### Entity System (enum_dispatch)

All compositing elements use `enum_dispatch` for zero-cost polymorphism:

```rust
#[enum_dispatch(Node)]
pub enum NodeKind {
    FileNode,    // Image/video source
    CompNode,    // Composition with layers
    CameraNode,  // Pan/zoom/rotate transform
    TextNode,    // Rasterized text
}
```

**Node trait methods:**
- `compute(frame, ctx)` - render frame at given time
- `attrs()` / `attrs_mut()` - attribute access
- `play_range()` - visible frame range after trims
- `is_dirty()` / `mark_dirty()` - cache invalidation

### Event-Driven Communication

Components communicate via `EventBus`:
1. Widgets emit events (e.g., `SetFrameEvent`, `PlayEvent`)
2. `main_events.rs` handles all events centrally
3. Updates state and triggers redraws

**Event pattern:**
```rust
// Emit event
event_bus.emit(SetFrameEvent { frame: 42 });

// Handle in main_events.rs
if let Some(e) = downcast_event::<SetFrameEvent>(&event) {
    player.set_frame(e.frame);
}
```

### Frame Loading Pipeline

```
Request Frame → Check GlobalFrameCache →
  HIT: Return cached frame
  MISS: → Comp::get_frame() creates placeholder
        → Workers load in background (epoch check)
        → Loaded frame inserted into cache
        → Next render picks up frame
```

**Epoch mechanism**: Atomic counter increments on scrub/seek. Workers skip stale requests.

---

## Code Conventions

### Naming
- **Functions**: Short, meaningful names. `get_tr()` not `extract_translation()`
- **Variables**: Snake_case, descriptive
- **Types**: PascalCase
- **Constants**: SCREAMING_SNAKE_CASE

### Style
- **Comments**: Concise, explain WHY not WHAT
- **Docstrings**: Required for public functions
- **Type annotations**: Use Rust's type inference where clear
- **Error handling**: Use `anyhow::Result` for recoverable errors

### Imports
```rust
// Prefer explicit imports over glob
use crate::core::event_bus::EventBus;
use crate::entities::{Comp, Frame, NodeKind};
```

### Attribute System

Attributes use schema flags (`attr_schemas.rs`):
- `DAG` - Changes invalidate render cache
- `DISP` - Show in Attribute Editor UI
- `KEY` - Keyframable

```rust
// Non-DAG attributes don't trigger recompute
pub const A_NODE_POS: &str = "node_pos";  // Node Editor position only
```

### Attribute Keys

Standard attribute keys defined in `entities/keys.rs`:
```rust
pub const A_IN: &str = "in";           // In-point (trim start)
pub const A_OUT: &str = "out";         // Out-point (trim end)
pub const A_SPEED: &str = "speed";     // Playback speed multiplier
pub const A_BLEND: &str = "blend";     // Blend mode
pub const A_OPACITY: &str = "opacity"; // Layer opacity
```

---

## Build System

### Prerequisites
- Rust 1.85+ (edition 2024)
- vcpkg with FFmpeg (`VCPKG_ROOT` env var)
- C++ compiler (for OpenEXR feature)

### Commands

**Windows (PowerShell only, never bash):**
```powershell
.\bootstrap.ps1 build              # Default exrs backend
.\bootstrap.ps1 build --openexr    # OpenEXR C++ backend
.\bootstrap.ps1 build --release    # Release build
.\bootstrap.ps1 test               # Run tests
```

**Linux/macOS:**
```bash
./bootstrap.sh build
./bootstrap.sh build --openexr
./bootstrap.sh test
```

### xtask Commands

```bash
cargo xtask build [--release] [--openexr]  # Full build
cargo xtask test                            # Run tests
cargo xtask verify [--release]              # Verify dependencies
cargo xtask deploy [--install-dir PATH]     # Install to system
cargo xtask tag-dev [patch|minor|major]     # Create dev tag
cargo xtask tag-rel [patch|minor|major]     # Create release tag
cargo xtask pr [version]                    # Create PR dev→main
cargo xtask changelog                       # Preview changelog
```

### Environment Variables

| Variable | Purpose | Example |
|----------|---------|---------|
| `VCPKG_ROOT` | vcpkg installation path | `C:\vcpkg` |
| `VCPKGRS_TRIPLET` | vcpkg triplet | `x64-windows-static-md-release` |
| `RUST_LOG` | Logging level | `debug`, `trace` |

---

## Testing

### Run Tests
```bash
cargo test                    # All tests
cargo test -- --nocapture     # With stdout
cargo test test_name          # Specific test
```

### Linting
```bash
cargo clippy -- -D warnings   # Must pass with no warnings
cargo fmt --check             # Format check
```

### Pre-commit Checklist
1. `cargo fmt`
2. `cargo clippy -- -D warnings`
3. `cargo test`
4. Build succeeds on target platform

---

## CI/CD

### Workflows (`.github/workflows/`)

| File | Trigger | Purpose |
|------|---------|---------|
| `main.yml` | Push to main, tags | Build & release |
| `warm-cache.yml` | Schedule | Keep vcpkg cache warm |
| `_build-backend.yml` | Reusable | Backend-specific build |
| `_build-platform.yml` | Reusable | Platform-specific build |

### Release Process

1. `cargo xtask tag-dev patch` - Create dev tag, triggers build workflow
2. Test artifacts from build
3. `cargo xtask pr` - Create PR dev→main
4. Merge PR
5. `cargo xtask tag-rel patch` - Create release tag, triggers release workflow

### Commit Messages (Conventional Commits)

```
feat: Add new feature
fix: Fix bug
refactor: Code refactoring
docs: Documentation update
chore: Maintenance tasks
perf: Performance improvement
test: Add/update tests
```

---

## Platform-Specific Notes

### Windows
- **Shell**: Always use PowerShell (`pwsh.exe`), never bash
- **Paths**: Use double backslashes in strings: `"C:\\path\\file"`
- **vcpkg triplet**: `x64-windows-static-md-release`
- **Build**: Requires Visual Studio Build Tools

### Linux
- **OpenEXR**: May need header patching for GCC 11+ (`cargo xtask pre`)
- **vcpkg triplet**: `x64-linux-release`
- **RPATH**: Configured in `.cargo/config.toml`

### macOS
- **Code signing**: Developer ID + notarization for DMG
- **vcpkg triplet**: `arm64-osx-release` (M1/M2) or `x64-osx-release` (Intel)
- **Universal binary**: Not currently supported

---

## Common Tasks

### Adding a New Node Type

1. Create `entities/my_node.rs`:
```rust
pub struct MyNode {
    uuid: Uuid,
    attrs: Attrs,
}

impl Node for MyNode {
    fn compute(&self, frame: i32, ctx: &mut ComputeContext) -> Option<Frame> { ... }
    fn attrs(&self) -> &Attrs { &self.attrs }
    fn attrs_mut(&mut self) -> &mut Attrs { &mut self.attrs }
    // ... other trait methods
}
```

2. Add to `entities/node_kind.rs`:
```rust
#[enum_dispatch(Node)]
pub enum NodeKind {
    FileNode,
    CompNode,
    CameraNode,
    TextNode,
    MyNode,  // Add here
}
```

3. Register attribute schema in `attr_schemas.rs`

### Adding a New Event

1. Define event in appropriate `*_events.rs`:
```rust
pub struct MyEvent {
    pub data: i32,
}
```

2. Handle in `main_events.rs`:
```rust
if let Some(e) = downcast_event::<MyEvent>(&event) {
    // Handle event
}
```

3. Emit from widget:
```rust
event_bus.emit(MyEvent { data: 42 });
```

### Adding a New Widget

1. Create `widgets/mywidget/mod.rs`:
```rust
pub mod mywidget;
pub mod mywidget_ui;
pub mod mywidget_events;

pub use mywidget::MyWidget;
```

2. Implement widget state and UI:
```rust
pub struct MyWidget {
    // State
}

impl MyWidget {
    pub fn ui(&mut self, ui: &mut egui::Ui, event_bus: &mut EventBus) {
        // egui UI code
    }
}
```

3. Add to `widgets/mod.rs`

### Adding a New Dialog

1. Create `dialogs/mydialog/mod.rs`:
```rust
pub mod mydialog;
pub mod mydialog_ui;

pub use mydialog::MyDialog;
```

2. Implement dialog with `egui::Window`:
```rust
impl MyDialog {
    pub fn show(&mut self, ctx: &egui::Context, event_bus: &mut EventBus) -> bool {
        let mut open = true;
        egui::Window::new("My Dialog")
            .open(&mut open)
            .show(ctx, |ui| {
                // UI content
            });
        open
    }
}
```

---

## Performance Considerations

### Frame Cache
- Default: 50% system RAM
- LRU eviction when over budget
- Epoch-based cancellation prevents wasted work

### Workers
- 75% of CPU cores for frame loading
- Work-stealing for load balancing

### Rendering
- OpenGL texture upload only when frame changes
- Shader hot-reloading for development

### Optimization Tips
- Use `Arc<Mutex<>>` sparingly, prefer message passing
- Batch UI updates, avoid per-frame allocations
- Profile with `puffin` feature: `cargo build --features profiler`

---

## Anti-Patterns to Avoid

### DO NOT:
1. **Use bash on Windows** - Always PowerShell
2. **Use Unicode in build files** - Causes encoding issues
3. **Commit directly to main** - Use feature branches
4. **Ignore clippy warnings** - Must be clean
5. **Add unnecessary dependencies** - Keep binary small
6. **Over-engineer** - Simple solutions preferred
7. **Forget epoch checks** - Leads to stale frame loading
8. **Block UI thread** - Use workers for IO

### DO:
1. **Use event bus** for cross-component communication
2. **Mark nodes dirty** when attributes change
3. **Check frame status** before rendering
4. **Handle errors gracefully** with proper logging
5. **Test on multiple platforms** for cross-platform changes
6. **Update CHANGELOG.md** via git-cliff (automated)

---

## Key Files Reference

| File | Purpose |
|------|---------|
| `src/main.rs` | Application entry, event loop |
| `src/main_events.rs` | Centralized event handling |
| `src/core/event_bus.rs` | Pub/sub event system |
| `src/core/global_cache.rs` | Frame caching |
| `src/core/player.rs` | Playback state machine |
| `src/core/workers.rs` | Async frame loading |
| `src/entities/node.rs` | Node trait definition |
| `src/entities/comp.rs` | Composition container |
| `src/entities/frame.rs` | Frame buffer and loading |
| `src/config.rs` | AppSettings persistence |
| `src/cli.rs` | Command-line arguments |
| `xtask/src/main.rs` | Build automation |
| `Cargo.toml` | Dependencies and features |
| `.cargo/config.toml` | Build configuration |

---

## Debugging

### Logging
```rust
use log::{debug, info, warn, error, trace};

debug!("Frame {} loaded in {}ms", idx, elapsed);
```

Enable with environment variable:
```powershell
$env:RUST_LOG = "debug"
.\target\debug\playa.exe
```

### Profiling
```bash
cargo build --features profiler
# Opens puffin profiler window in app
```

### Common Issues

| Issue | Cause | Solution |
|-------|-------|----------|
| Frame not loading | Stale epoch | Check epoch in worker |
| UI freeze | Blocking on main thread | Move to workers |
| Memory leak | Cache not evicting | Check CacheManager limits |
| Shader error | GLSL syntax | Check shaders/ directory |
| FFmpeg not found | vcpkg not configured | Set VCPKG_ROOT |

---

## Dependencies (Key Crates)

| Crate | Version | Purpose |
|-------|---------|---------|
| `eframe` | 0.33 | egui framework wrapper |
| `egui_glow` | 0.33 | OpenGL backend |
| `egui_dock` | 0.18 | Docking panel system |
| `egui-snarl` | 0.9 | Node graph editor |
| `egui_ltreeview` | 0.6 | TreeView widget |
| `playa-ffmpeg` | 8.0.3 | FFmpeg bindings |
| `image` | 0.25 | Image format support |
| `openexr` | 0.11 | OpenEXR C++ bindings (optional) |
| `glam` | 0.30 | Math (vectors, matrices) |
| `crossbeam` | 0.8 | Concurrent primitives |
| `clap` | 4.5 | CLI argument parsing |
| `serde` | 1.0 | Serialization |
| `anyhow` | 1.0 | Error handling |

---

## Contact & Resources

- **Repository**: https://github.com/ssoj13/playa
- **Issues**: https://github.com/ssoj13/playa/issues
- **Changelog**: See `CHANGELOG.md`
- **Architecture**: See `README.md` and `src/README.md`

---

## Agent-Specific Instructions

### For Code Changes:
1. Read existing code first before modifying
2. Follow established patterns in the codebase
3. Run `cargo fmt` and `cargo clippy` before committing
4. Test on Windows primarily (main development platform)

### For Documentation:
1. Keep README.md focused on user-facing info
2. Keep src/README.md focused on code structure
3. Update this file for agent-specific guidance

### For Bug Fixes:
1. Reproduce the issue first
2. Add test if possible
3. Fix with minimal changes
4. Document the root cause

### For New Features:
1. Discuss architecture first
2. Follow existing module patterns
3. Add to appropriate location (entities/widgets/dialogs)
4. Update relevant documentation

---

*Last updated: 2025-12-17*
