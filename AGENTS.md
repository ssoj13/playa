# AGENTS.md - Playa Architecture Guide for AI Agents

> **Philosophy**: Simplicity enables complexity. This app strives to be simple while providing powerful features.

## Overview

Playa is an image sequence and video player built in Rust with egui. It supports:
- Image sequences (EXR, PNG, JPEG, TIFF, TGA)
- Video files (MP4, MOV, AVI, MKV via FFmpeg)
- Multi-layer compositing with blend modes
- Hardware-accelerated video encoding

**Core design principle**: Make complex things accessible through simple abstractions.

---

## Project Structure

```
playa/
├── src/
│   ├── main.rs              # Entry point, app initialization
│   ├── lib.rs               # Library root, re-exports
│   ├── main_events.rs       # Central event handler (~1000 lines)
│   │
│   ├── core/                # Engine (UI-independent)
│   │   ├── event_bus.rs     # Pub/sub messaging system
│   │   ├── player.rs        # Playback state (JKL controls, FPS)
│   │   ├── player_events.rs # Playback event types
│   │   ├── cache_man.rs     # Global memory management
│   │   ├── global_cache.rs  # Frame cache with LRU eviction
│   │   └── workers.rs       # Thread pool with work-stealing
│   │
│   ├── entities/            # Data structures
│   │   ├── attrs.rs         # Generic key-value storage
│   │   ├── project.rs       # Top-level container (media pool)
│   │   ├── comp_node.rs     # Composition with layers
│   │   ├── comp_events.rs   # Composition event types
│   │   ├── file_node.rs     # Image sequence / video reference
│   │   ├── node.rs          # Node trait (compute interface)
│   │   ├── node_kind.rs     # Enum: FileNode | CompNode
│   │   ├── frame.rs         # Single frame data
│   │   ├── loader.rs        # Image loading (EXR, PNG, etc)
│   │   ├── loader_video.rs  # Video frame extraction
│   │   └── compositor.rs    # Multi-layer blending
│   │
│   ├── widgets/             # UI components
│   │   ├── timeline/        # Timeline with layer bars
│   │   ├── viewport/        # OpenGL frame display
│   │   ├── project/         # Media pool panel
│   │   ├── ae/              # Attribute editor
│   │   ├── node_editor/     # Node graph (egui-snarl)
│   │   └── status/          # Status bar, progress
│   │
│   └── dialogs/             # Modal windows
│       ├── encode/          # Video encoding dialog
│       └── prefs/           # Settings dialog
│
├── xtask/                   # Build automation (xtask pattern)
└── .github/workflows/       # CI/CD pipelines
```

---

## Core Architecture

### The Simplicity-First Design

The architecture follows a few key principles:

1. **Single source of truth**: `PlayaApp.project` is the only Project instance
2. **Event-driven**: UI emits events → handler processes → state updates
3. **No ownership tangles**: Player receives `&mut Project`, doesn't own it
4. **Atomic dirty tracking**: Cache invalidation via dirty flags, not hash comparison

### Component Relationships

```
┌─────────────────────────────────────────────────────────────────┐
│                         PlayaApp                                 │
│  ┌──────────┐  ┌─────────┐  ┌──────────┐  ┌─────────────────┐  │
│  │  Player  │  │ Project │  │ EventBus │  │ CacheManager    │  │
│  │  (state) │  │ (data)  │  │ (comms)  │  │ (memory/epoch)  │  │
│  └────┬─────┘  └────┬────┘  └────┬─────┘  └────────┬────────┘  │
│       │             │            │                  │           │
│       └──────┬──────┴────────────┴──────────────────┘           │
│              │                                                   │
│              ▼                                                   │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    main_events.rs                         │   │
│  │              handle_app_event() dispatcher                │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Data Structures

### Attrs - Generic Key-Value Storage

The simplest yet most powerful abstraction. Used everywhere.

```rust
pub struct Attrs {
    map: HashMap<String, AttrValue>,
    dirty: AtomicBool,  // Thread-safe cache invalidation
}

pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int(i32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Uuid(Uuid),
    Json(String),  // For complex nested data
    // ... more
}
```

**Key methods**:
- `set(key, value)` - Sets value AND marks dirty (triggers cache invalidation)
- `set_silent(key, value)` - Sets value WITHOUT marking dirty (playhead, UI state)
- `is_dirty()` / `clear_dirty()` - Cache invalidation check
- `get_json<T>()` / `set_json()` - Serialize/deserialize complex types

**Why this design**: Attrs provides a flexible schema-less storage that can evolve without breaking serialization. Every entity (Frame, Layer, Comp, Project) uses Attrs for its properties.

### Project - Top-Level Container

```rust
pub struct Project {
    pub attrs: Attrs,                                    // comps_order, selection, active
    pub media: Arc<RwLock<HashMap<Uuid, NodeKind>>>,    // All nodes
    pub global_cache: Option<Arc<GlobalFrameCache>>,     // Runtime cache
    event_emitter: Option<EventEmitter>,                 // Auto-emit on modify
}
```

**Key patterns**:
- `modify_comp(uuid, |comp| {...})` - Safe mutation with auto-dirty emission
- `with_comp(uuid, |comp| {...})` - Read-only access
- `compute_frame(comp_uuid, frame_idx)` - Get composed frame

### CompNode - Composition

```rust
pub struct CompNode {
    uuid: Uuid,
    attrs: Attrs,           // name, fps, in, out, frame, trim_in, trim_out
    pub layers: Vec<Layer>, // Bottom to top (render order)
    pub layer_selection: Vec<Uuid>,
}

pub struct Layer {
    pub uuid: Uuid,        // Instance UUID (unique per layer)
    pub source_uuid: Uuid, // References FileNode or CompNode in media pool
    pub attrs: Attrs,      // in, src_len, trim_in, trim_out, opacity, blend_mode, speed
}
```

**Layer order**: `layers[0]` is background, `layers[N-1]` is foreground.

**Trim values are OFFSETS**, not absolute frames:
- `trim_in = 0` means no trim from start
- `trim_out = 0` means no trim from end

### Frame - Single Image

```rust
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub data: PixelBuffer,    // RGB u8 / RGBA u8 / RGBA f16 / RGBA f32
    pub status: FrameStatus,  // Placeholder | Loading | Loaded | Error
    pub attrs: Attrs,         // Metadata
}
```

---

## Event Bus - Decoupled Communication

The EventBus is the nervous system of the app. It enables components to communicate without direct references.

### Architecture

```rust
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}
```

**Two modes of operation**:
1. **Immediate**: `subscribe()` callbacks fire instantly on `emit()`
2. **Deferred**: Events also queue for batch processing in main loop

### Usage Pattern

```rust
// Subscribe (typically in init)
event_bus.subscribe::<SetFrameEvent, _>(move |e| {
    // Immediate callback
});

// Emit (from anywhere)
event_bus.emit(SetFrameEvent(100));

// Poll in main loop
for event in event_bus.poll() {
    if let Some(e) = downcast_event::<SetFrameEvent>(&event) {
        player.set_frame(e.0, project);
    }
}
```

### Event Types

Located in `*_events.rs` files:

| File | Events |
|------|--------|
| `player_events.rs` | `TogglePlayPauseEvent`, `StopEvent`, `SetFrameEvent`, `JogForwardEvent`, etc. |
| `comp_events.rs` | `AddLayerEvent`, `MoveLayerEvent`, `AttrsChangedEvent`, `LayersChangedEvent` |
| `timeline_events.rs` | `TimelineZoomChangedEvent`, `TimelineFitEvent` |
| `viewport_events.rs` | `ZoomViewportEvent`, `FitViewportEvent` |
| `project_events.rs` | `AddClipEvent`, `SaveProjectEvent`, `SelectMediaEvent` |

### Cache Invalidation Flow

```
User changes opacity
    │
    ▼
comp.set_child_attrs()  →  attrs.set() marks dirty
    │
    ▼
modify_comp() detects is_dirty()
    │
    ▼
emits AttrsChangedEvent(comp_uuid)
    │
    ▼
main_events handler:
  - cache_manager.increment_epoch()  // Cancel pending workers
  - global_cache.clear_comp(uuid)    // Evict cached frames
    │
    ▼
Next render: compute() regenerates frame
```

---

## Threading Model

### Workers - Work-Stealing Thread Pool

```rust
pub struct Workers {
    injector: Arc<Injector<Job>>,     // Global queue
    handles: Vec<JoinHandle<()>>,     // Worker threads
    current_epoch: Arc<AtomicU64>,    // For stale request cancellation
    shutdown: Arc<AtomicBool>,
}
```

**Work-stealing algorithm**:
1. Worker tries own queue (LIFO for cache locality)
2. Worker tries global injector
3. Worker steals from other workers' queues
4. If no work, short sleep to avoid CPU spin

**Epoch-based cancellation**:
```rust
workers.execute_with_epoch(epoch, move || {
    // This job will be skipped if epoch changed before execution
});
```

When timeline scrubs fast, `cache_manager.increment_epoch()` is called, causing all pending frame load jobs to skip execution.

### CacheManager - Memory Control

```rust
pub struct CacheManager {
    memory_usage: Arc<AtomicUsize>,     // Current bytes used
    max_memory_bytes: AtomicUsize,      // Limit
    current_epoch: Arc<AtomicU64>,      // For cancellation
}
```

All atomic operations = lock-free = fast.

---

## Composition Pipeline

### Frame Computation

```rust
// In comp_node.rs
impl Node for CompNode {
    fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        // 1. Check cycle detection (prevent infinite recursion)
        // 2. Collect source frames from visible layers
        // 3. Blend layers using compositor
        // 4. Return composed frame
    }
}
```

### Blend Modes

```rust
pub enum BlendMode {
    Normal,     // Over compositing
    Screen,     // 1 - (1-A)(1-B)
    Add,        // A + B (clamped)
    Subtract,   // A - B (clamped)
    Multiply,   // A * B
    Divide,     // A / B
    Difference, // |A - B|
}
```

---

## Key Patterns

### 1. modify_comp() for Mutations

```rust
// WRONG - doesn't trigger cache invalidation
project.media.write().get_mut(&uuid).set_something();

// RIGHT - auto-emits AttrsChangedEvent if dirty
project.modify_comp(uuid, |comp| {
    comp.set_child_attrs(layer_uuid, vec![("opacity", AttrValue::Float(0.5))]);
});
```

### 2. Dirty Flags for Performance

Instead of computing hashes on every frame:
```rust
// In attrs.rs
pub fn set(&mut self, key: impl Into<String>, value: AttrValue) {
    self.map.insert(key.into(), value);
    self.dirty.store(true, Ordering::Relaxed);  // O(1) flag
}
```

### 3. Thread-Safe Read Access

```rust
// project.rs uses Arc<RwLock<HashMap>> for media
project.with_comp(uuid, |comp| comp.frame())  // Takes read lock
project.modify_comp(uuid, |comp| ...)         // Takes write lock
```

### 4. Event Result Accumulation

```rust
pub struct EventResult {
    pub load_project: Option<PathBuf>,
    pub save_project: Option<PathBuf>,
    pub load_sequences: Option<Vec<PathBuf>>,
    // ...
}

// Multiple events can accumulate results
result.merge(other_result);
```

---

## Implementation Notes

### Serialization

- `Attrs` serializes as `HashMap<String, AttrValue>`
- Runtime-only fields use `#[serde(skip)]`
- After deserialization, call `rebuild_runtime()` to restore caches/emitters

### Error Handling

- Use `anyhow::Result` for operations that can fail
- Log errors with `log::error!()`, don't panic
- Return `Option` for queries that may not find data

### Memory Management

- LRU cache with configurable memory budget
- Default: 50% of available RAM
- Frames evicted when limit exceeded
- Epoch mechanism prevents loading stale frames

---

## Quick Reference

### Adding a New Event Type

1. Define in `*_events.rs`:
```rust
#[derive(Clone, Debug)]
pub struct MyNewEvent {
    pub value: i32,
}
```

2. Handle in `main_events.rs`:
```rust
if let Some(e) = downcast_event::<MyNewEvent>(event) {
    // Handle event
    return Some(result);
}
```

3. Emit from UI:
```rust
emitter.emit(MyNewEvent { value: 42 });
```

### Adding a New Attribute

Just use it - Attrs is schema-less:
```rust
comp.attrs.set("my_new_attr", AttrValue::Float(1.0));
let val = comp.attrs.get_float("my_new_attr").unwrap_or(0.0);
```

### Adding a New Node Type

1. Create `my_node.rs` in `entities/`
2. Implement `Node` trait with `compute()` method
3. Add variant to `NodeKind` enum
4. Register in project media pool

---

## Summary

| Concept | Implementation | Purpose |
|---------|---------------|---------|
| **Attrs** | `HashMap<String, AttrValue>` + dirty flag | Flexible properties, cache invalidation |
| **EventBus** | Pub/sub with immediate + deferred modes | Decoupled component communication |
| **Project** | `Arc<RwLock<HashMap<Uuid, NodeKind>>>` | Thread-safe media pool |
| **Workers** | Crossbeam work-stealing deques | Parallel frame loading |
| **CacheManager** | Atomic counters | Memory limit, epoch cancellation |
| **modify_comp()** | Closure with auto-emit | Safe mutations with cache invalidation |

**The goal**: Complex features (multi-layer compositing, async loading, cache management) exposed through simple, composable abstractions.
