# AGENTS.md - Playa Architecture & AI Guide

This document describes Playa's architecture for both human developers and AI assistants (Claude Code, Codex). It combines component documentation, dataflow diagrams, and AI guidelines.

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Architecture Components](#architecture-components)
3. [Dataflow Diagrams](#dataflow-diagrams)
4. [Coordinate Systems](#coordinate-systems)
5. [AI Assistant Guidelines](#ai-assistant-guidelines)
6. [Recent Changes (dev4)](#recent-changes-dev4)

---

## Project Overview

**Playa** is an image sequence player and video compositor written in Rust. Key features:

- **Node-based compositing** with FileNode, CompNode, CameraNode, TextNode
- **Event-driven architecture** via pub/sub EventBus
- **Work-stealing thread pool** for background frame loading
- **LRU cache with epoch-based cancellation** for responsive scrubbing
- **REST API server** for remote control
- **egui/eframe UI** with OpenGL viewport

### Tech Stack

| Component | Technology |
|-----------|------------|
| UI Framework | egui 0.33 + eframe |
| Graphics | OpenGL via glow |
| EXR Loading | exrs (pure Rust) or openexr-rs (C++) |
| Video | FFmpeg via playa-ffmpeg crate |
| Concurrency | crossbeam channels + work-stealing deques |
| HTTP Server | rouille (sync) |

### Module Structure

```
src/
├── core/           # Engine (cache, events, player, workers)
│   ├── cache_man.rs        # Global memory manager
│   ├── event_bus.rs        # Pub/sub event system
│   ├── global_cache.rs     # Frame cache (comp_uuid -> frame_idx -> Frame)
│   ├── player.rs           # Playback state machine
│   ├── player_events.rs    # Player events (Play, Pause, SetFrame)
│   └── workers.rs          # Work-stealing thread pool
│
├── entities/       # Data models
│   ├── attrs.rs            # Generic key-value attributes
│   ├── attr_schemas.rs     # Attribute metadata (DAG, keyframable)
│   ├── comp_node.rs        # Composition with layers
│   ├── file_node.rs        # Image/video source
│   ├── camera_node.rs      # Camera transform
│   ├── text_node.rs        # Text overlay
│   ├── frame.rs            # Pixel buffer (U8/F16/F32)
│   ├── compositor.rs       # CPU blending
│   ├── transform.rs        # 3D affine transforms
│   ├── space.rs            # Coordinate space conversions
│   └── project.rs          # Top-level container
│
├── widgets/        # UI components
│   ├── viewport/           # Image display, gizmo, shaders
│   ├── timeline/           # Timeline editor, layers
│   ├── project/            # Media pool panel
│   ├── ae/                 # Attribute editor
│   └── status/             # Status bar
│
├── dialogs/        # Modal windows
│   ├── encode/             # Video export
│   └── prefs/              # Preferences
│
├── server/         # REST API
│   ├── mod.rs              # Server lifecycle
│   └── api.rs              # HTTP endpoints
│
└── main_events.rs  # Central event handler
```

---

## Architecture Components

### 1. EventBus - Pub/Sub Communication

**File**: `src/core/event_bus.rs`

Decoupled component communication via typed events.

```
╔═══════════════════════════════════════════════════════════════╗
║                         EventBus                              ║
╠═══════════════════════════════════════════════════════════════╣
║  emit<E>(event)                                               ║
║    ├─► IMMEDIATE: Invoke subscribers synchronously            ║
║    │              (callbacks run in current thread)           ║
║    │                                                          ║
║    └─► DEFERRED:  Push to event queue                         ║
║                   (retrieved via poll() in main loop)         ║
╚═══════════════════════════════════════════════════════════════╝
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
╔══════════════════════════════════════════════════════════════╗
║              Workers (Work-Stealing Thread Pool)             ║
╠══════════════════════════════════════════════════════════════╣
║  injector: Arc<Injector<Job>>      ← Global task queue       ║
║  handles: Vec<JoinHandle<()>>      ← Thread pool             ║
║  current_epoch: Arc<AtomicU64>     ← Shared with CacheMan    ║
║  shutdown: Arc<AtomicBool>         ← Shutdown signal         ║
╚══════════════════════════════════════════════════════════════╝

Thread Pool (num_cpus * 3/4):

┌─────────────────────────────────────────────────────────────┐
│ Worker Thread:                                               │
│   Loop:                                                      │
│     1. Try own queue (LIFO - cache locality)                │
│     2. Try global injector                                   │
│     3. Try stealing from other workers (FIFO - oldest)      │
│     4. Check shutdown signal                                 │
│     5. Sleep 1ms if no work                                  │
└─────────────────────────────────────────────────────────────┘
```

**Epoch-Based Cancellation**:
```rust
// User scrubs timeline rapidly:
SetFrameEvent(100) → epoch=1
SetFrameEvent(150) → epoch=2  ← increment_epoch()
SetFrameEvent(200) → epoch=3

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
╔══════════════════════════════════════════════════════════════╗
║               CacheManager (Global Singleton)                ║
╠══════════════════════════════════════════════════════════════╣
║  memory_usage: AtomicUsize      ← Total bytes allocated      ║
║  max_memory_bytes: AtomicUsize  ← User limit (75% of RAM)    ║
║  current_epoch: Arc<AtomicU64>  ← Cancellation counter       ║
║  dirty_repaint: Arc<AtomicBool> ← UI repaint trigger         ║
╚══════════════════════════════════════════════════════════════╝
```

**Preload Strategies**:
- `Spiral`: 0, +1, -1, +2, -2, ... (good for image sequences)
- `Forward`: center → end (optimized for video with expensive backward seek)

---

### 4. GlobalFrameCache - Frame Storage

**File**: `src/core/global_cache.rs`

Nested HashMap with LRU eviction.

```
╔══════════════════════════════════════════════════════════════╗
║            GlobalFrameCache (Nested HashMap)                 ║
╠══════════════════════════════════════════════════════════════╣
║  cache: Arc<RwLock<HashMap<Uuid, HashMap<i32, Frame>>>>      ║
║                      ▲comp_uuid    ▲frame_idx                ║
║                                                               ║
║  lru_order: Arc<Mutex<IndexSet<CacheKey>>>                   ║
║    Tracks insertion order for LRU eviction                   ║
╚══════════════════════════════════════════════════════════════╝
```

**Operations**:
- `get(comp, frame)` - O(1) lookup, updates LRU
- `insert(comp, frame, data)` - adds with eviction check
- `clear_comp(uuid, dehydrate)` - invalidate comp cache
  - `dehydrate=true`: Mark Loaded → Expired (fast, pixels stay)
  - `dehydrate=false`: Remove entirely (free memory)

---

### 5. Player - Playback State Machine

**File**: `src/core/player.rs`

Controls playback, FPS, and frame navigation.

```
States: Stopped ←→ Playing ←→ Paused

┌─────────────────────────────────────────────────────────────┐
│ Player                                                       │
├─────────────────────────────────────────────────────────────┤
│ fps_base: f32         ← Base FPS (user setting)             │
│ fps_play: f32         ← Current play FPS (affected by J/L)  │
│ is_playing: bool                                             │
│ loop_enabled: bool                                           │
│ active_comp: Option<Uuid>                                    │
│ direction: i32        ← +1 forward, -1 backward             │
└─────────────────────────────────────────────────────────────┘
```

**JKL Shuttle**:
- `J` - Jog backward (cumulative speed increase)
- `K` - Stop
- `L` - Jog forward (cumulative speed increase)

---

### 6. Project - Top-Level Container

**File**: `src/entities/project.rs`

Holds all nodes (media pool) and compositions.

```
╔══════════════════════════════════════════════════════════════╗
║                        Project                               ║
╠══════════════════════════════════════════════════════════════╣
║  attrs: Attrs                                                ║
║    ├─ order: Vec<Uuid>         ← UI display order           ║
║    ├─ selection: Vec<Uuid>     ← Multi-selection            ║
║    └─ active: Option<Uuid>     ← Currently active comp      ║
║                                                               ║
║  media: Arc<RwLock<HashMap<Uuid, Arc<NodeKind>>>>            ║
║    ├─ FileNode: Image sequences, videos                     ║
║    ├─ CompNode: Nested compositions                         ║
║    ├─ CameraNode: Camera transforms                         ║
║    └─ TextNode: Text overlays                               ║
║                                                               ║
║  compositor: Mutex<CompositorType>  ← CPU/GPU blending      ║
║  cache_manager: Arc<CacheManager>                            ║
║  global_cache: Arc<GlobalFrameCache>                         ║
║  event_emitter: Option<EventEmitter>  ← Auto-emit changes   ║
╚══════════════════════════════════════════════════════════════╝
```

**Why Arc<NodeKind>?**

Worker threads need to read nodes during frame computation, but UI thread needs write access for playhead updates. Without Arc, workers hold read lock during long compute operations (50-500ms), blocking UI writes → jank.

With Arc<NodeKind>:
- Workers clone Arc (nanoseconds), release lock immediately
- UI can acquire write lock without waiting for compute
- Arc::make_mut provides copy-on-write for mutations

---

### 7. REST API Server

**File**: `src/server/api.rs`

HTTP server for remote control of Playa.

```
╔══════════════════════════════════════════════════════════════╗
║                     REST API Server                          ║
╠══════════════════════════════════════════════════════════════╣
║  Thread:  Background (rouille sync HTTP)                     ║
║  Port:    Configurable in Settings -> Web Server             ║
║  CORS:    Enabled for browser access                         ║
╚══════════════════════════════════════════════════════════════╝

Commands (sent via mpsc channel to main thread):
  Play, Pause, Stop, SetFrame(i32), SetFps(f32),
  ToggleLoop, LoadSequence(String), Screenshot, Exit
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

### User Input → Rendering

```
User Action (keyboard / mouse / UI widget)
    │
    ▼
egui Event Handler
    │
    ▼
╔═══════════════════════════════════════╗
║           EventBus                    ║
║  emit() → subscribers + deferred queue║
╚═══════════════════╤═══════════════════╝
                    │
    ┌───────────────┴───────────────┐
    ▼                               ▼
Immediate Callbacks          Deferred Queue
                                    │
                                    ▼
                          Main Loop (60Hz)
                          poll() → handle_app_event()
                                    │
                    ┌───────────────┴───────────────┐
                    ▼                               ▼
            Player Control              Project/Comp Changes
            (play, seek)                (attrs, layers)
                                                │
                                                ▼
                                    Cache Invalidation
                                    increment_epoch()
                                    clear_comp(uuid)
                                                │
                                                ▼
                                    Workers enqueue new loads
                                                │
                                                ▼
                                    Viewport Render
                                    get_frame() → display
```

### Frame Loading Pipeline

```
SetFrameEvent(42)
    │
    ▼
global_cache.get(comp, 42)
    │
    ├─► HIT: return cached Frame
    │
    └─► MISS: continue
            │
            ▼
        Frame::new_composing()
        Insert placeholder to prevent double-load
            │
            ▼
        workers.execute_with_epoch(epoch, || {
            │
            ├─► Check epoch (stale? → skip)
            │
            └─► compose_internal(comp, 42, ctx)
                    │
                    ▼
                For each visible layer:
                    │
                    ├─► Get source node (FileNode or nested CompNode)
                    ├─► Compute source frame (recursive)
                    ├─► Apply transform (position, rotation, scale)
                    └─► Collect (frame, opacity, blend_mode)
                    │
                    ▼
                CPU Compositor: blend layers
                    │
                    ▼
                global_cache.insert(comp, 42, result)
        })
```

### Composition Pipeline

```
┌──────────────────────────────────────────────────────────────┐
│ STEP 1: CYCLE DETECTION                                      │
├──────────────────────────────────────────────────────────────┤
│ COMPOSE_STACK.with(|stack| {                                 │
│   if stack.contains(&comp.uuid()) {                          │
│     return Error("Cyclic dependency detected");              │
│   }                                                          │
│   stack.insert(comp.uuid());                                 │
│ })                                                           │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ STEP 2: COLLECT VISIBLE LAYERS                               │
├──────────────────────────────────────────────────────────────┤
│ layers.iter().rev()  ← Bottom-to-top render order           │
│   .filter(|layer| {                                          │
│     layer.is_visible() &&                                    │
│     layer.work_area().contains(parent_frame)                 │
│   })                                                          │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ STEP 3: COMPUTE SOURCE FRAMES                                │
├──────────────────────────────────────────────────────────────┤
│ For each layer:                                              │
│   local_frame = layer.parent_to_local(parent_frame)          │
│   source = media.get(layer.source_uuid)                      │
│   frame = source.compute_frame(local_frame, ctx)             │
│     ├─ FileNode: load from disk                              │
│     └─ CompNode: recursive compose                           │
│                                                               │
│   Apply transform:                                           │
│     matrix = build_inverse_matrix(pos, rot, scale, pivot)    │
│     frame = transform_frame(frame, matrix, comp_dim)         │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ STEP 4: BLEND LAYERS                                         │
├──────────────────────────────────────────────────────────────┤
│ Blend Modes:                                                 │
│   Normal:     top_color                                      │
│   Screen:     1 - (1-bottom)*(1-top)                         │
│   Add:        bottom + top                                   │
│   Subtract:   bottom - top                                   │
│   Multiply:   bottom * top                                   │
│   Divide:     bottom / top                                   │
│   Difference: |bottom - top|                                 │
│                                                               │
│ Alpha Blending:                                              │
│   top_alpha = top[3] * opacity                               │
│   result = bottom * (1 - top_alpha) + blend(bottom, top) *   │
│            top_alpha                                         │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│ STEP 5: FINALIZE                                             │
├──────────────────────────────────────────────────────────────┤
│ composed_frame.status = min_status(all_sources)              │
│ global_cache.insert(comp, frame_idx, composed_frame)         │
│ COMPOSE_STACK.remove(comp.uuid)                              │
└──────────────────────────────────────────────────────────────┘
```

### Cache Invalidation Cascade

```
User changes layer opacity
    │
    ▼
project.modify_comp(uuid, |comp| {
    comp.set_child_attrs(layer, [("opacity", 0.5)]);
    → layer.attrs.set() → dirty=true
    → comp.mark_dirty()
});
    │
    ▼
Auto-emit: AttrsChangedEvent(comp_uuid)
    │
    ▼
handle_app_event():
    ├─► cache_manager.increment_epoch()
    │     → Cancels pending worker tasks
    │
    ├─► global_cache.clear_comp(uuid, dehydrate=true)
    │     → Marks Loaded → Expired
    │
    └─► invalidate_cascade()
          for parent in find_parent_comps(uuid) {
              global_cache.clear_comp(parent, true);
          }
    │
    ▼
Next viewport.ui():
    global_cache.get(comp, frame)
      status = Expired → Recompose with new values
```

---

## Coordinate Systems

### Three Spaces

```
┌─────────────────────────────────────────────────────────────┐
│ IMAGE SPACE                                                  │
│   Origin: top-left, +Y down (pixels)                        │
│   Use: texture sampling, screen pixels                       │
│                                                               │
│   (0,0)────────────► +X                                      │
│     │                                                        │
│     │                                                        │
│     ▼ +Y                                                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ FRAME SPACE (= Viewport Space)                              │
│   Origin: CENTER of comp, +Y up (pixels)                    │
│   Use: layer transforms, gizmo                              │
│                                                               │
│               +Y ▲                                           │
│                  │                                           │
│     -X ◄─────────┼─────────► +X                             │
│                  │                                           │
│                  ▼ -Y                                        │
│                                                               │
│   position = (0, 0, 0) = layer centered                     │
│   position = (100, 50, 0) = 100px right, 50px up            │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ OBJECT SPACE                                                 │
│   Origin: layer center, +Y up (pixels)                      │
│   Use: rotation/scale around pivot                          │
└─────────────────────────────────────────────────────────────┘
```

### Transform Pipeline

```
Screen pixel (image space)
    │  image_to_frame()
    ▼
Frame space (centered, Y-up)
    │  inverse model transform
    ▼
Object space (layer center)
    │  object_to_src()
    ▼
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

**DON'T**:
- Directly mutate `comp.layers` without calling `mark_dirty()`
- Block main thread with heavy computation (use Workers)
- Ignore epoch checks in worker tasks
- Forget to restore `event_emitter` after deserialization

### Event Downcasting

When using `downcast_event<E>(&event)` where `event: &BoxedEvent`:

```rust
// WRONG: May pick Box's blanket impl
let event_any = event.as_any();

// CORRECT: Force vtable dispatch
let event_any = (**event).as_any();
```

See `event_bus.rs::downcast_event()` for reference.

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
4. Document in this file under Event Types

### Key Files to Know

| File | Purpose |
|------|---------|
| `main_events.rs` | Central event handler - start here |
| `project.rs` | modify_comp() pattern, media pool |
| `comp_node.rs` | Layer management, composition |
| `global_cache.rs` | Frame caching, LRU eviction |
| `workers.rs` | Background loading, epoch |
| `event_bus.rs` | Pub/sub implementation |
| `transform.rs` | 3D affine transforms |
| `space.rs` | Coordinate conversions |

### Build Commands

```powershell
# Build (exrs backend - pure Rust, fast)
.\bootstrap.ps1 build

# Build with OpenEXR C++ (DWAA/DWAB support)
.\bootstrap.ps1 build --openexr

# Run tests
.\bootstrap.ps1 test

# Release build
cargo xtask build --release
```

### Environment

- **Platform**: Windows 11 (pwsh.exe preferred, not bash)
- **Vcpkg**: `$env:VCPKG_ROOT = "C:\vcpkg"`
- **Triplet**: `x64-windows-static-md-release`

---

## Recent Changes (dev4)

### New Features

1. **REST API Server** (`src/server/`)
   - HTTP endpoints for remote control
   - Screenshot capture API
   - Configurable in Settings

2. **Transform/Space System** (`entities/transform.rs`, `entities/space.rs`)
   - Full 3D affine transforms with perspective
   - Frame space (centered, Y-up) as primary coordinate system
   - Ray-plane intersection for perspective unproject

3. **Viewport Gizmo** (`widgets/viewport/gizmo.rs`)
   - Move/Rotate/Scale manipulation
   - Uses transform-gizmo-egui crate
   - Integrated with layer selection

4. **Timeline Improvements**
   - Trim hotkeys: `[`/`]` snap edges, `Alt-[`/`Alt-]` trim at cursor
   - Layer selection and multi-select
   - Jump to layer edges

5. **Attribute System Enhancements**
   - Schema-based validation
   - DAG vs non-DAG attributes
   - Auto-emit on dirty

6. **Layer Effects** (`entities/effects/`)
   - Per-layer post-processing: Gaussian Blur, Brightness/Contrast, HSV
   - Applied before transform (layer-local space)
   - UI in Attribute Editor (F3)

7. **Layer Picker** (`widgets/viewport/pick.rs`)
   - Left click in Select mode (Q) picks topmost layer
   - Raycast: screen → image → frame space → inverse transform → bounds check
   - Uses `ViewportState::screen_to_image()` + `space::image_to_frame()`

### Architecture Changes

- `Arc<NodeKind>` in media pool for lock-free worker access
- Improved epoch-based cancellation
- Unified coordinate space handling
- Camera node improvements

---

*This document is auto-generated and should be kept in sync with code changes.*
