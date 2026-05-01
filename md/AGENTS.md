# AGENTS.md - Playa Architecture & AI Guide

This document describes Playa's architecture for both human developers and AI assistants (Claude Code, Codex).

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Architecture Components](#architecture-components)
3. [Dataflow Diagrams](#dataflow-diagrams)
4. [Coordinate Systems](#coordinate-systems)
5. [AI Assistant Guidelines](#ai-assistant-guidelines)

---

## Project Overview

**Playa** is an image sequence player and video compositor written in Rust. Version: **0.1.135**

### Key Features

- Node-based compositing with FileNode, CompNode, CameraNode, TextNode
- Event-driven architecture via pub/sub EventBus
- Work-stealing thread pool for background frame loading
- LRU cache with epoch-based cancellation for responsive scrubbing
- REST API server for remote control
- egui/eframe UI with OpenGL viewport
- Layer effects (blur, brightness, HSV)
- 3D transforms with perspective camera

### Tech Stack

| Component | Technology |
|-----------|------------|
| UI Framework | egui 0.33 + eframe |
| Graphics | OpenGL via glow |
| EXR Loading | exrs (pure Rust) OR openexr-rs 0.11 (C++, DWAA/DWAB) |
| Video | playa-ffmpeg 8.0 (static FFmpeg) |
| Concurrency | crossbeam channels + work-stealing deques |
| HTTP Server | rouille (sync) |

### CLI

```
playa [OPTIONS] [FILE]

Options:
  -f, --file <FILE>      Additional files to load
  -p, --playlist <FILE>  Load playlist from JSON
  -F, --fullscreen       Start in fullscreen mode
  -a, --autoplay         Auto-play on startup
  -v, --verbose          Increase logging (-v, -vv, -vvv)
  -V, --version          Print version info
  -h, --help             Print help
```

Version output (`-V`):
```
playa 0.1.135
EXR:    openexr-rs 0.11 (C++, DWAA/DWAB)  # or: exrs (pure Rust)
Video:  playa-ffmpeg 8.0 (static)
Target: x86_64-windows
```

### Module Structure

```
src/
├── main.rs             # Entry point
├── cli.rs              # Clap argument parsing
├── config.rs           # Settings persistence
├── help.rs             # Help overlay (F1)
├── main_events.rs      # Central event handler
├── ui.rs               # Main UI composition
├── runner.rs           # App runner loop
├── shell.rs            # Shell integration (drag-drop)
├── utils.rs            # Utilities
│
├── app/                # Application state
│   ├── api.rs          # App-level API
│   ├── events.rs       # App events
│   ├── layout.rs       # Dock/panel layout
│   ├── project_io.rs   # Project save/load
│   ├── run.rs          # Main run loop
│   └── tabs.rs         # Tab management
│
├── core/               # Engine (cache, events, player, workers)
│   ├── cache_man.rs        # Global memory manager
│   ├── debounced_preloader.rs  # Debounced frame preloading
│   ├── event_bus.rs        # Pub/sub event system
│   ├── global_cache.rs     # Frame cache (comp_uuid -> frame_idx -> Frame)
│   ├── layout_events.rs    # Layout change events
│   ├── player.rs           # Playback state machine
│   ├── player_events.rs    # Player events (Play, Pause, SetFrame)
│   └── workers.rs          # Work-stealing thread pool
│
├── entities/           # Data models
│   ├── attrs.rs            # Generic key-value attributes
│   ├── attr_schemas.rs     # Attribute metadata (DAG, keyframable)
│   ├── camera_node.rs      # Camera transform
│   ├── comp_node.rs        # Composition with layers
│   ├── comp_events.rs      # Composition events
│   ├── compositor.rs       # CPU blending
│   ├── file_node.rs        # Image/video source
│   ├── frame.rs            # Pixel buffer (U8/F16/F32)
│   ├── gpu_compositor.rs   # GPU blending (experimental)
│   ├── keys.rs             # Keyframe animation
│   ├── loader.rs           # Image loading (EXR, PNG, JPEG, TIFF)
│   ├── loader_video.rs     # Video loading (FFmpeg)
│   ├── node.rs             # Node trait
│   ├── node_kind.rs        # NodeKind enum (FileNode|CompNode|...)
│   ├── project.rs          # Top-level container
│   ├── space.rs            # Coordinate space conversions
│   ├── text_node.rs        # Text overlay
│   ├── traits.rs           # Common traits
│   ├── transform.rs        # 3D affine transforms
│   └── effects/            # Layer effects
│       ├── blur.rs         # Gaussian blur
│       ├── brightness.rs   # Brightness/contrast
│       ├── hsv.rs          # Hue/saturation/value
│       └── mod.rs
│
├── widgets/            # UI components
│   ├── actions.rs          # Menu actions
│   ├── file_dialogs.rs     # File open/save dialogs
│   ├── viewport/           # Image display, gizmo, shaders
│   │   ├── coords.rs       # Coordinate helpers
│   │   ├── gizmo.rs        # Transform gizmo
│   │   ├── pick.rs         # Layer picking
│   │   ├── renderer.rs     # OpenGL rendering
│   │   ├── shaders.rs      # GLSL shaders
│   │   ├── tool.rs         # Viewport tools
│   │   ├── viewport.rs     # Viewport state
│   │   ├── viewport_events.rs
│   │   └── viewport_ui.rs
│   ├── timeline/           # Timeline editor
│   │   ├── timeline.rs
│   │   ├── timeline_events.rs
│   │   ├── timeline_helpers.rs
│   │   └── timeline_ui.rs
│   ├── project/            # Media pool panel
│   │   ├── project.rs
│   │   ├── project_events.rs
│   │   └── project_ui.rs
│   ├── ae/                 # Attribute editor
│   │   └── ae_ui.rs
│   ├── node_editor/        # Node graph editor
│   │   ├── node_events.rs
│   │   └── node_graph.rs
│   └── status/             # Status bar
│       ├── progress_bar.rs
│       └── status.rs
│
├── dialogs/            # Modal windows
│   ├── encode/             # Video export
│   │   ├── encode.rs
│   │   └── encode_ui.rs
│   └── prefs/              # Preferences
│       ├── input_handler.rs
│       ├── prefs.rs
│       └── prefs_events.rs
│
└── server/             # REST API
    ├── mod.rs              # Server lifecycle
    └── api.rs              # HTTP endpoints
```

---

## Architecture Components

### 1. EventBus - Pub/Sub Communication

**File**: `src/core/event_bus.rs`

Decoupled component communication via typed events.

```
+---------------------------------------------------------------+
|                         EventBus                              |
+---------------------------------------------------------------+
|  emit<E>(event)                                               |
|    |-> IMMEDIATE: Invoke subscribers synchronously            |
|    |              (callbacks run in current thread)           |
|    |                                                          |
|    +-> DEFERRED:  Push to event queue                         |
|                   (retrieved via poll() in main loop)         |
+---------------------------------------------------------------+
```

**Event Types**:

| Category | Events |
|----------|--------|
| Player | `SetFrameEvent`, `TogglePlayPauseEvent`, `StepForward/BackwardEvent` |
| Comp | `AttrsChangedEvent`, `LayersChangedEvent`, `AddLayerEvent` |
| Project | `AddClipEvent`, `AddFolderEvent`, `SelectMediaEvent` |
| Viewport | `ZoomEvent`, `PanEvent`, `SetToolEvent` |
| Timeline | `TrimLayerEvent`, `MoveLayersEvent`, `JumpToEdgeEvent` |

**Usage**:
```rust
// Emit event (immediate + queued)
event_bus.emit(SetFrameEvent(42));

// Subscribe (immediate callback)
event_bus.subscribe::<SetFrameEvent, _>(|e| {
    println!("Frame: {}", e.0);
});

// Poll in main loop (deferred)
for event in event_bus.poll() {
    handle_app_event(event, ...);
}
```

---

### 2. Workers - Work-Stealing Thread Pool

**File**: `src/core/workers.rs`

Background frame loading with epoch-based cancellation.

```
+--------------------------------------------------------------+
|              Workers (Work-Stealing Thread Pool)             |
+--------------------------------------------------------------+
|  injector: Arc<Injector<Job>>      <- Global task queue      |
|  handles: Vec<JoinHandle<()>>      <- Thread pool            |
|  current_epoch: Arc<AtomicU64>     <- Shared with CacheMan   |
|  shutdown: Arc<AtomicBool>         <- Shutdown signal        |
+--------------------------------------------------------------+

Thread Pool (num_cpus * 3/4):

+-------------------------------------------------------------+
| Worker Thread:                                               |
|   Loop:                                                      |
|     1. Try own queue (LIFO - cache locality)                |
|     2. Try global injector                                   |
|     3. Try stealing from other workers (FIFO - oldest)      |
|     4. Check shutdown signal                                 |
|     5. Sleep 1ms if no work                                  |
+-------------------------------------------------------------+
```

**Epoch-Based Cancellation**:
```rust
// User scrubs timeline rapidly:
SetFrameEvent(100) -> epoch=1
SetFrameEvent(150) -> epoch=2  <- increment_epoch()
SetFrameEvent(200) -> epoch=3

// Worker checks epoch before loading:
workers.execute_with_epoch(epoch, || {
    if current_epoch() == request_epoch {
        load_frame();  // Still valid
    } else {
        skip;  // Stale, user moved on
    }
});
```

---

### 3. CacheManager - Memory Management

**File**: `src/core/cache_man.rs`

Global memory tracking with LRU eviction.

```
+--------------------------------------------------------------+
|               CacheManager (Global Singleton)                |
+--------------------------------------------------------------+
|  memory_usage: AtomicUsize      <- Total bytes allocated     |
|  max_memory_bytes: AtomicUsize  <- User limit (75% of RAM)   |
|  current_epoch: Arc<AtomicU64>  <- Cancellation counter      |
|  dirty_repaint: Arc<AtomicBool> <- UI repaint trigger        |
+--------------------------------------------------------------+
```

**Preload Strategies**:
- `Spiral`: 0, +1, -1, +2, -2, ... (good for image sequences)
- `Forward`: center -> end (optimized for video with expensive backward seek)

---

### 4. GlobalFrameCache - Frame Storage

**File**: `src/core/global_cache.rs`

Nested HashMap with LRU eviction.

```
+--------------------------------------------------------------+
|            GlobalFrameCache (Nested HashMap)                 |
+--------------------------------------------------------------+
|  cache: Arc<RwLock<HashMap<Uuid, HashMap<i32, Frame>>>>      |
|                      ^comp_uuid    ^frame_idx                |
|                                                               |
|  lru_order: Arc<Mutex<IndexSet<CacheKey>>>                   |
|    Tracks insertion order for LRU eviction                   |
+--------------------------------------------------------------+
```

**Operations**:
- `get(comp, frame)` - O(1) lookup, updates LRU
- `insert(comp, frame, data)` - adds with eviction check
- `clear_comp(uuid, dehydrate)` - invalidate comp cache
  - `dehydrate=true`: Mark Loaded -> Expired (fast, pixels stay)
  - `dehydrate=false`: Remove entirely (free memory)

---

### 5. Player - Playback State Machine

**File**: `src/core/player.rs`

Controls playback, FPS, and frame navigation.

```
States: Stopped <-> Playing <-> Paused

+-------------------------------------------------------------+
| Player                                                       |
+-------------------------------------------------------------+
| fps_base: f32         <- Base FPS (user setting)            |
| fps_play: f32         <- Current play FPS (affected by J/L) |
| is_playing: bool                                             |
| loop_enabled: bool                                           |
| active_comp: Option<Uuid>                                    |
| direction: i32        <- +1 forward, -1 backward            |
+-------------------------------------------------------------+
```

**JKL Shuttle**:
- `J` - Jog backward (cumulative speed increase)
- `K` - Stop
- `L` - Jog forward (cumulative speed increase)

---

### 6. Loader - Image/Video Loading

**Files**: `src/entities/loader.rs`, `src/entities/loader_video.rs`

Unified loading interface for all formats.

**Image Formats** (loader.rs):
- PNG, JPEG, TIFF, TGA, HDR via `image` crate
- EXR via `exrs` (pure Rust) or `openexr-rs` (C++, optional)

**EXR Backend Selection**:
```rust
#[cfg(feature = "openexr")]
fn load_exr(path: &Path) -> Result<Frame, FrameError> {
    // Uses openexr-rs with DWAA/DWAB support
    // Detects HALF vs FLOAT pixel types
}

#[cfg(not(feature = "openexr"))]
fn load_exr(path: &Path) -> Result<Frame, FrameError> {
    // Uses exrs crate (pure Rust, no DWAA)
}
```

**Video Formats** (loader_video.rs):
- MP4, MOV, AVI, MKV via playa-ffmpeg
- Frame-accurate seeking
- Audio track extraction

---

### 7. REST API Server

**File**: `src/server/api.rs`

HTTP server for remote control of Playa.

```
+--------------------------------------------------------------+
|                     REST API Server                          |
+--------------------------------------------------------------+
|  Thread:  Background (rouille sync HTTP)                     |
|  Port:    Configurable in Settings -> Web Server             |
|  CORS:    Enabled for browser access                         |
+--------------------------------------------------------------+
```

**Endpoints**:
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/status` | Full status (player/comp/cache) |
| GET | `/api/player` | Player state only |
| POST | `/api/player/play` | Start playback |
| POST | `/api/player/pause` | Pause playback |
| POST | `/api/player/stop` | Stop and seek to start |
| POST | `/api/player/frame/{n}` | Seek to frame |
| POST | `/api/player/fps/{n}` | Set FPS |
| POST | `/api/player/next` | Next frame |
| POST | `/api/player/prev` | Previous frame |
| POST | `/api/screenshot` | Capture current frame as PNG |

---

## Dataflow Diagrams

### User Input -> Rendering

```
User Action (keyboard / mouse / UI widget)
    |
    v
egui Event Handler
    |
    v
+=======================================+
|           EventBus                    |
|  emit() -> subscribers + deferred queue|
+==================|====================+
                   |
    +--------------+---------------+
    v                              v
Immediate Callbacks          Deferred Queue
                                   |
                                   v
                         Main Loop (60Hz)
                         poll() -> handle_app_event()
                                   |
                   +---------------+---------------+
                   v                               v
           Player Control              Project/Comp Changes
           (play, seek)                (attrs, layers)
                                               |
                                               v
                                   Cache Invalidation
                                   increment_epoch()
                                   clear_comp(uuid)
                                               |
                                               v
                                   Workers enqueue new loads
                                               |
                                               v
                                   Viewport Render
                                   get_frame() -> display
```

### Frame Loading Pipeline

```
SetFrameEvent(42)
    |
    v
global_cache.get(comp, 42)
    |
    +-> HIT: return cached Frame
    |
    +-> MISS: continue
            |
            v
        Frame::new_composing()
        Insert placeholder to prevent double-load
            |
            v
        workers.execute_with_epoch(epoch, || {
            |
            +-> Check epoch (stale? -> skip)
            |
            +-> compose_internal(comp, 42, ctx)
                    |
                    v
                For each visible layer:
                    |
                    +-> Get source node (FileNode or nested CompNode)
                    +-> Compute source frame (recursive)
                    +-> Apply transform (position, rotation, scale)
                    +-> Collect (frame, opacity, blend_mode)
                    |
                    v
                CPU Compositor: blend layers
                    |
                    v
                global_cache.insert(comp, 42, result)
        })
```

---

## Coordinate Systems

### Three Spaces

```
+-------------------------------------------------------------+
| IMAGE SPACE                                                  |
|   Origin: top-left, +Y down (pixels)                        |
|   Use: texture sampling, screen pixels                       |
|                                                               |
|   (0,0)--------------> +X                                    |
|     |                                                        |
|     |                                                        |
|     v +Y                                                     |
+-------------------------------------------------------------+

+-------------------------------------------------------------+
| FRAME SPACE (= Viewport Space)                              |
|   Origin: CENTER of comp, +Y up (pixels)                    |
|   Use: layer transforms, gizmo                              |
|                                                               |
|               +Y ^                                           |
|                  |                                           |
|     -X <---------+---------> +X                             |
|                  |                                           |
|                  v -Y                                        |
|                                                               |
|   position = (0, 0, 0) = layer centered                     |
|   position = (100, 50, 0) = 100px right, 50px up            |
+-------------------------------------------------------------+

+-------------------------------------------------------------+
| OBJECT SPACE                                                 |
|   Origin: layer center, +Y up (pixels)                      |
|   Use: rotation/scale around pivot                          |
+-------------------------------------------------------------+
```

### Transform Pipeline

```
Screen pixel (image space)
    |  image_to_frame()
    v
Frame space (centered, Y-up)
    |  inverse model transform
    v
Object space (layer center)
    |  object_to_src()
    v
Source pixel (texture sampling)
```

### Rotation Conventions

- **Order**: ZYX (After Effects style) - rotate Z first, then Y, then X
- **Sign**: Clockwise-positive when looking down axis (user convention)
- **glam**: Uses CCW+ (math convention), so angles are NEGATED when calling glam

---

## AI Assistant Guidelines

### Working with This Codebase

**DO**:
- Use `project.modify_comp()` for all comp mutations (auto-emits events)
- Check event handling in `main_events.rs` before adding new events
- Use `attrs.set()` for DAG attributes (triggers dirty flag)
- Prefer `Arc::clone()` over cloning large data
- Use work-stealing pattern for background tasks
- Build with `bootstrap.ps1` (not direct cargo commands)

**DON'T**:
- Directly mutate `comp.layers` without calling `mark_dirty()`
- Block main thread with heavy computation (use Workers)
- Ignore epoch checks in worker tasks
- Forget to restore `event_emitter` after deserialization
- Use bash on Windows (use pwsh.exe)

### Event Downcasting

When using `downcast_event<E>(&event)` where `event: &BoxedEvent`:

```rust
// WRONG: May pick Box's blanket impl
let event_any = event.as_any();

// CORRECT: Force vtable dispatch
let event_any = (**event).as_any();
```

### Adding New Node Types

1. Create struct in `entities/` implementing `Node` trait
2. Add variant to `NodeKind` enum in `node_kind.rs`
3. Define attribute schema in `attr_schemas.rs`
4. Add compute logic for frame generation
5. Handle in composition pipeline if needed

### Adding New Events

1. Define event struct (any `Send + Sync + 'static` type)
2. Emit via `event_bus.emit(MyEvent { ... })`
3. Handle in `main_events.rs` with `downcast_event::<MyEvent>`

### Key Files to Know

| File | Purpose |
|------|---------|
| `main_events.rs` | Central event handler - start here |
| `project.rs` | modify_comp() pattern, media pool |
| `comp_node.rs` | Layer management, composition |
| `global_cache.rs` | Frame caching, LRU eviction |
| `workers.rs` | Background loading, epoch |
| `event_bus.rs` | Pub/sub implementation |
| `loader.rs` | EXR/image loading |
| `loader_video.rs` | Video loading (FFmpeg) |
| `transform.rs` | 3D affine transforms |
| `space.rs` | Coordinate conversions |

### Build Commands

```powershell
# Build (exrs backend - pure Rust, fast)
./bootstrap.ps1 build

# Build with OpenEXR C++ (DWAA/DWAB support)
./bootstrap.ps1 build --openexr

# Run tests
./bootstrap.ps1 test

# Package for distribution
./bootstrap.ps1 package
```

### Environment

- **Platform**: Windows 11 (pwsh.exe only, no bash)
- **Vcpkg**: `$env:VCPKG_ROOT = "C:\vcpkg"`
- **Triplet**: `x64-windows-static-md-release`

---

## Features Summary

| Feature | Description |
|---------|-------------|
| Node-based compositing | FileNode, CompNode, CameraNode, TextNode |
| Layer effects | Gaussian Blur, Brightness/Contrast, HSV |
| 3D transforms | Position, Rotation, Scale with perspective |
| Viewport gizmo | Move/Rotate/Scale manipulation |
| Layer picker | Click to select topmost layer |
| JKL shuttle | Industry-standard playback controls |
| REST API | Remote control via HTTP |
| Timeline | Trim, move, multi-select layers |
| Keyboard shortcuts | See Help (F1) |

---

*Last updated: 2024-12-24*
