# AGENTS.md - Playa Codebase Guide for AI Assistants

## Project Overview

**Playa** is an image sequence player and compositor written in Rust. It supports:
- Image sequences (EXR, PNG, JPEG, TIFF, TGA, HDR)
- Video files (MP4, MOV, AVI, MKV via FFmpeg)
- Real-time compositing with blend modes
- Hardware-accelerated video encoding (NVENC, QSV, AMF)

**Version**: 0.1.133  
**Edition**: Rust 2024  
**Primary UI**: egui 0.33 + eframe  
**Rendering**: OpenGL via glow  

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                           PlayaApp (main.rs)                        │
│   - egui/eframe App implementation                                  │
│   - Event handling loop                                             │
│   - Dock-based panel layout (egui_dock)                            │
└─────────────────────────────────────────────────────────────────────┘
         │
         ├─── Player ──────────────────────── Playback state (JKL controls)
         │                                    Does NOT own Project
         │
         ├─── Project ─────────────────────── Scene container
         │       │                            - media: HashMap<Uuid, Comp>
         │       │                            - selection, active, comps_order
         │       │
         │       └─── Comp ────────────────── Composition (dual-mode)
         │               │                    - File mode: loads sequences
         │               │                    - Layer mode: composes children
         │               │
         │               └─── Frame ───────── Single image with pixels
         │
         ├─── EventBus ────────────────────── Pub/Sub event system
         │       │                            - Immediate callbacks
         │       │                            - Deferred queue for poll()
         │       │
         │       └─── EventEmitter ────────── Lightweight handle for widgets
         │
         ├─── CacheManager ────────────────── Global memory tracking
         │                                    - Atomic memory counters
         │                                    - Epoch for stale request cancel
         │
         ├─── GlobalFrameCache ────────────── Nested HashMap LRU cache
         │                                    - HashMap<Uuid, HashMap<i32, Frame>>
         │                                    - CacheStrategy: LastOnly/All
         │
         ├─── Workers ─────────────────────── Thread pool (crossbeam)
         │                                    - Work-stealing deques
         │                                    - Epoch-based cancellation
         │
         └─── Compositor ──────────────────── Frame blending
                 ├─── CpuCompositor           - Software fallback
                 └─── GpuCompositor           - OpenGL accelerated
```

---

## Directory Structure

```
playa/
├── src/
│   ├── main.rs              # Entry point, PlayaApp, dock layout
│   ├── lib.rs               # Library re-exports
│   │
│   ├── core/                # Engine (independent of UI)
│   │   ├── mod.rs
│   │   ├── cache_man.rs     # Memory manager + epoch counter
│   │   ├── event_bus.rs     # Pub/Sub + deferred queue
│   │   ├── global_cache.rs  # LRU frame cache
│   │   ├── player.rs        # Playback state, JKL controls
│   │   ├── player_events.rs # Player-related events
│   │   ├── project_events.rs# Project-related events
│   │   └── workers.rs       # Work-stealing thread pool
│   │
│   ├── entities/            # Data models
│   │   ├── mod.rs
│   │   ├── attrs.rs         # Key-value attribute system
│   │   ├── comp.rs          # Composition (File/Layer modes)
│   │   ├── comp_events.rs   # Composition events
│   │   ├── compositor.rs    # CPU blending backend
│   │   ├── gpu_compositor.rs# GPU blending backend
│   │   ├── frame.rs         # Single image data
│   │   ├── keys.rs          # Attribute key constants
│   │   ├── layer.rs         # Layer/Track types
│   │   ├── loader.rs        # Image sequence loader
│   │   ├── loader_video.rs  # Video file loader (FFmpeg)
│   │   └── project.rs       # Scene container
│   │
│   ├── widgets/             # UI components
│   │   ├── mod.rs
│   │   ├── ae/              # Attributes Editor panel
│   │   ├── node_editor/     # Node graph visualization
│   │   ├── project/         # Project/Media panel
│   │   ├── status/          # Status bar + progress
│   │   ├── timeline/        # Timeline + transport controls
│   │   └── viewport/        # Image display + shaders
│   │
│   ├── dialogs/             # Modal windows
│   │   ├── mod.rs
│   │   ├── encode/          # Video encoding dialog
│   │   └── prefs/           # Settings dialog + hotkeys
│   │
│   ├── cli.rs               # Command-line arguments (clap)
│   ├── config.rs            # Path configuration
│   ├── help.rs              # Help overlay content
│   ├── main_events.rs       # App-level event handlers
│   ├── shell.rs             # Shell integration
│   ├── ui.rs                # UI helpers
│   └── utils.rs             # Utility functions
│
├── xtask/                   # Build automation
│   └── src/
│       ├── main.rs          # xtask entry point
│       ├── pre_build.rs     # Pre-build tasks
│       ├── post_build.rs    # Post-build tasks
│       ├── release.rs       # Release automation
│       └── lib_discovery.rs # Library discovery
│
├── .github/workflows/       # CI/CD
├── Cargo.toml               # Dependencies
├── build.rs                 # Build script
└── bootstrap.ps1/.sh        # Bootstrap scripts
```

---

## Key Concepts

### 1. Comp Dual-Mode Architecture

`Comp` is the central entity with two modes (stored in `attrs["mode"]`):

```rust
// Mode constants (entities/keys.rs)
pub const COMP_NORMAL: i8 = 0;  // Layer mode: composes children
pub const COMP_FILE: i8 = 1;    // File mode: loads from disk
```

**File mode**: 
- `attrs["file_mask"]` contains glob pattern (e.g., `render.####.exr`)
- `attrs["file_start"]`, `attrs["file_end"]` define frame range
- `get_frame()` calls `Loader::load()` to read from disk

**Layer mode**:
- `children: Vec<(Uuid, Attrs)>` stores child layers
- `get_frame()` recursively composes children via `Compositor`
- Children can be File comps or nested Layer comps

### 2. Attrs Key-Value System

All properties are stored in `Attrs` (essentially `HashMap<String, AttrValue>`):

```rust
pub enum AttrValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Str(String),
    Vec3([f32; 3]),
    Json(String),  // For complex types serialized as JSON
}
```

**Key constants** in `entities/keys.rs`:
```rust
pub const A_UUID: &str = "uuid";
pub const A_NAME: &str = "name";
pub const A_MODE: &str = "mode";
pub const A_IN: &str = "in";
pub const A_OUT: &str = "out";
pub const A_FRAME: &str = "frame";
pub const A_OPACITY: &str = "opacity";
pub const A_BLEND_MODE: &str = "blend_mode";
// ... many more
```

### 3. Event-Driven Architecture

**EventBus** (`core/event_bus.rs`) provides pub/sub:

```rust
// Emit event (callbacks fire immediately, also queued)
event_bus.emit(MyEvent { data: 42 });

// Poll for batch processing in main loop
for event in event_bus.poll() {
    if let Some(e) = downcast_event::<MyEvent>(&event) {
        // Handle event
    }
}
```

**Key events** (`entities/comp_events.rs`):
- `AttrsChangedEvent(Uuid)` - comp attributes modified
- `LayersChangedEvent` - children changed
- `CurrentFrameChangedEvent` - playhead moved
- `ViewportRefreshEvent` - force viewport redraw

**Flow example**:
```
UI change → set_child_attr() → emit AttrsChangedEvent
                                       ↓
                            main.rs handler:
                            - increment_epoch() (cancel workers)
                            - cache.clear_comp() (invalidate)
                            - invalidate_cascade() (parents)
```

### 4. Cache and Memory Management

**CacheManager** (`core/cache_man.rs`):
- Atomic memory tracking (`AtomicUsize`)
- Epoch counter for stale request cancellation
- Configurable memory limits (% of system RAM)

**GlobalFrameCache** (`core/global_cache.rs`):
- Nested `HashMap<Uuid, HashMap<i32, Frame>>` 
- O(1) `clear_comp()` - just remove outer key
- LRU eviction via `IndexSet` (preserves insertion order)
- `CacheStrategy::LastOnly` or `CacheStrategy::All`

**Epoch pattern**:
```rust
// On scrub/seek:
cache_manager.increment_epoch();

// Workers check before execution:
workers.execute_with_epoch(current_epoch, || {
    // Job runs only if epoch still matches
});
```

### 5. Workers Thread Pool

**Workers** (`core/workers.rs`) uses crossbeam work-stealing:

```rust
// Execute async task
workers.execute(move || {
    frame.load(...);
});

// With epoch check (cancellable)
workers.execute_with_epoch(epoch, move || {
    // Skipped if epoch changed
});
```

- 75% of CPU cores for workers, 25% for UI
- Work-stealing: idle workers steal from busy ones
- FIFO within worker, LIFO for cache locality

### 6. Player Architecture

**Player** (`core/player.rs`) does NOT own Project:
```rust
// Methods receive &mut Project
fn set_frame(&mut self, frame: i32, project: &mut Project);
fn current_frame(&self, project: &Project) -> i32;
```

**JKL shuttle controls**:
- J: jog backward (tap to increase speed)
- K: pause
- L: jog forward (tap to increase speed)
- FPS presets: 1, 2, 4, 8, 12, 24, 30, 60, 120, 240

### 7. Compositor Backends

**CompositorType** enum (`entities/compositor.rs`):
```rust
pub enum CompositorType {
    Cpu(CpuCompositor),   // Software fallback
    Gpu(GpuCompositor),   // OpenGL accelerated
}
```

**Blend modes**: Normal, Screen, Add, Subtract, Multiply, Divide, Difference

**Thread-local CPU compositor**:
```rust
thread_local! {
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = ...;
}
```

---

## Common Patterns

### Adding a New Event

1. Define event struct in appropriate `*_events.rs`:
```rust
#[derive(Clone, Debug)]
pub struct MyNewEvent {
    pub comp_uuid: Uuid,
    pub data: i32,
}
```

2. Emit from component:
```rust
self.event_bus.emit(MyNewEvent { comp_uuid, data: 42 });
```

3. Handle in `main.rs` or `main_events.rs`:
```rust
if let Some(e) = downcast_event::<MyNewEvent>(&event) {
    // Handle
}
```

### Modifying Comp Attributes

**ALWAYS use unified methods** (never mutate children directly):
```rust
// Single attribute
comp.set_child_attr(layer_uuid, "opacity", AttrValue::Float(0.5));

// Multiple attributes
comp.set_child_attrs(layer_uuid, vec![
    ("opacity".to_string(), AttrValue::Float(0.5)),
    ("visible".to_string(), AttrValue::Bool(true)),
]);
```

This automatically:
1. Updates the attribute
2. Marks comp dirty
3. Emits `AttrsChangedEvent`

### Adding a Widget

1. Create module in `src/widgets/my_widget/`:
   - `mod.rs` - module definition
   - `my_widget.rs` - main widget code
   - `my_widget_events.rs` - widget-specific events
   - `my_widget_ui.rs` - UI rendering

2. Export in `src/widgets/mod.rs`:
```rust
pub mod my_widget;
```

3. Add to dock layout in `main.rs`:
```rust
enum DockTab {
    // ...existing...
    MyWidget,
}
```

### Frame Loading Flow

```
User scrubs timeline
    ↓
Player.set_frame() → Comp.set_frame() → emit CurrentFrameChangedEvent
    ↓
main.rs handler → enqueue_frame_loads_around_playhead()
    ↓
Comp.signal_preload() → Workers.execute_with_epoch()
    ↓
Worker thread: Loader::load() or Comp::compose()
    ↓
GlobalFrameCache.put() → CacheManager.add_memory()
    ↓
Viewport.render() → cache.get() → Frame displayed
```

---

## Dependencies (Key Crates)

| Crate | Purpose |
|-------|---------|
| `egui` 0.33 | Immediate mode UI |
| `eframe` 0.33 | egui integration |
| `egui_dock` 0.18 | Dockable panels |
| `egui_glow` 0.33 | OpenGL backend |
| `glow` | OpenGL bindings |
| `image` 0.25 | Image loading (PNG, JPEG, etc.) |
| `openexr` 0.11 | EXR loading (optional feature) |
| `playa-ffmpeg` 8.0.3 | FFmpeg bindings |
| `crossbeam` 0.8 | Work-stealing concurrency |
| `serde` 1.0 | Serialization |
| `uuid` 1.18 | Unique identifiers |
| `half` 1.8 | f16 support |
| `clap` 4.5 | CLI argument parsing |

---

## Build Commands

```bash
# Development build (pure Rust EXR)
cargo build

# Release build
cargo build --release

# With OpenEXR C++ backend (DWAA/DWAB support)
cargo xtask build --release --openexr

# Run tests
cargo test

# Bootstrap (sets up vcpkg, installs tools)
.\bootstrap.ps1 build        # Windows
./bootstrap.sh build         # Linux/macOS
```

---

## Feature Flags

| Feature | Description |
|---------|-------------|
| `openexr` | Enable OpenEXR C++ backend for DWAA/DWAB compression |

---

## Important Notes for AI Assistants

1. **Project is the source of truth** - Player receives `&mut Project`, doesn't own it

2. **Always use `set_child_attr`/`set_child_attrs`** for layer modifications to ensure events fire

3. **EventBus has two modes**: 
   - `subscribe()` + `emit()` for immediate callbacks
   - `poll()` for deferred batch processing

4. **Epoch mechanism** is critical for responsive scrubbing - increment on seek/scrub

5. **Cache keys** use `(comp_uuid, frame_idx)` - changing any attr invalidates cache via hash

6. **Thread-local compositors** avoid locks in worker threads

7. **Attrs are the unified property system** - use key constants from `entities/keys.rs`

8. **GPU compositor requires main thread** (OpenGL context) - workers use CPU fallback

9. **Bootstrap scripts** handle FFmpeg/vcpkg setup - prefer `cargo xtask` for builds

10. **No bash on Windows** - use PowerShell exclusively
