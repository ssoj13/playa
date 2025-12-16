# Playa Architecture - Comprehensive Dataflow Diagram

> **Generated**: 2025-12-15
> **Purpose**: Complete system architecture documentation for understanding data flow from user input to rendering

---

## Table of Contents

1. [System Overview](#system-overview)
2. [Core Components](#core-components)
3. [Event System Architecture](#event-system-architecture)
4. [Frame Loading Pipeline](#frame-loading-pipeline)
5. [Cache System Dataflow](#cache-system-dataflow)
6. [Composition Pipeline](#composition-pipeline)
7. [Project State Management](#project-state-management)
8. [Complete User Input → Rendering Flow](#complete-user-input--rendering-flow)
9. [Thread Safety & Concurrency](#thread-safety--concurrency)

---

## System Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           PLAYA ARCHITECTURE                            │
│                     Image Sequence Player (Rust)                        │
└─────────────────────────────────────────────────────────────────────────┘

       USER INPUT
          │
          ▼
   ┌──────────────┐
   │  egui UI     │ ◄── Main Thread (60Hz)
   │  Keyboard    │
   │  Mouse       │
   └──────┬───────┘
          │
          ▼
   ┌──────────────────┐
   │   EventBus       │ ◄── Pub/Sub Event System
   │  - subscribe()   │     - Immediate callbacks
   │  - emit()        │     - Deferred queue
   │  - poll()        │
   └──────┬───────────┘
          │
          ├──────────────────────────────────────────┐
          ▼                                          ▼
   ┌──────────────┐                         ┌──────────────┐
   │   Player     │                         │   Project    │
   │ - State      │                         │ - Media Pool │
   │ - Playback   │                         │ - Comps      │
   └──────┬───────┘                         └──────┬───────┘
          │                                         │
          ▼                                         ▼
   ┌──────────────────────────────────────────────────────┐
   │           CacheManager + GlobalFrameCache            │
   │  - Memory tracking                                   │
   │  - Epoch-based cancellation                          │
   │  - LRU eviction                                      │
   └──────┬───────────────────────────────────────────────┘
          │
          ▼
   ┌──────────────────┐
   │    Workers       │ ◄── Background Threads
   │  (Thread Pool)   │     - Work-stealing deques
   │                  │     - Priority execution
   └──────┬───────────┘
          │
          ├─────────────────────┬──────────────────┐
          ▼                     ▼                  ▼
   ┌──────────┐        ┌──────────────┐    ┌─────────────┐
   │ Loader   │        │  Compositor  │    │   GPU       │
   │ - EXR    │        │  - Blend     │    │ Compositor  │
   │ - PNG    │        │  - Transform │    │ (Optional)  │
   │ - Video  │        │  - CPU/GPU   │    └─────────────┘
   └──────┬───┘        └──────┬───────┘
          │                    │
          ▼                    ▼
   ┌──────────────────────────────────┐
   │           Frame                  │
   │  - PixelBuffer (U8/F16/F32)      │
   │  - Status (Header/Loading/...)   │
   │  - Atomic state transitions      │
   └──────┬───────────────────────────┘
          │
          ▼
   ┌──────────────┐
   │  Viewport    │ ◄── Render to screen
   │  (OpenGL)    │
   └──────────────┘
```

---

## Core Components

### 1. **EventBus** (`src/core/event_bus.rs`)

**Architecture**: Pub/Sub pattern with dual-mode operation

```rust
EventBus {
    subscribers: Arc<RwLock<HashMap<TypeId, Vec<Callback>>>>,
    queue: Arc<Mutex<Vec<BoxedEvent>>>,
}
```

**Data Flow**:
```
UI Widget
   │
   ├─ emit(Event)
   │     │
   │     ├──> Immediate: Invoke all subscribers synchronously
   │     │         (callbacks run immediately in current thread)
   │     │
   │     └──> Deferred: Push to event queue
   │             (retrieved via poll() in main loop)
   │
   └─ Main Loop (60Hz)
         │
         └─ poll() → Vec<BoxedEvent>
                │
                └─ handle_app_event() for each event
```

**Key Features**:
- **Immediate callbacks**: Execute synchronously during `emit()`
- **Deferred queue**: Batch processing in main loop via `poll()`
- **Type-safe**: Generic `subscribe<E>()` with compile-time checks
- **Eviction**: Auto-evicts oldest 50% when queue exceeds 1000 events

**Example Flow**:
```rust
// Timeline widget emits frame change
event_bus.emit(SetFrameEvent(42));

// Immediate callbacks fire:
//   - Player updates current_frame
//   - Cache invalidation triggers

// Main loop polls:
for event in event_bus.poll() {
    if let Some(e) = downcast_event::<SetFrameEvent>(&event) {
        // Enqueue frame loading
        // Update viewport
    }
}
```

---

### 2. **Player** (`src/core/player.rs`)

**Purpose**: Playback state manager (does NOT own Project)

**State Storage**:
```rust
Player {
    attrs: Attrs {              // Serializable state
        active_comp: Option<Uuid>,
        is_playing: bool,
        fps_base: f32,
        fps_play: f32,
        loop_enabled: bool,
        play_direction: f32,    // 1.0 = forward, -1.0 = backward
    },
    last_frame_time: Option<Instant>, // Runtime-only
}
```

**Key Methods**:
- `update(&mut self, project: &mut Project)` - Called at 60Hz, advances frames
- `set_active_comp(uuid, project)` - Switches composition, resets selection
- `jog_forward()` / `jog_backward()` - J/K/L shuttle controls
- `set_frame(frame, project)` - Playhead navigation

**Timing Model**:
```
Frame-accurate timing (not wall-clock):
  - Each frame has duration = 1/fps seconds
  - No dropped frames from timer
  - If frame not loaded: display last good frame
```

---

### 3. **Project** (`src/entities/project.rs`)

**Purpose**: Top-level scene container with unified media pool

**Structure**:
```rust
Project {
    attrs: Attrs {
        comps_order: Vec<Uuid>,     // UI display order
        selection: Vec<Uuid>,        // Multi-selection
        active: Option<Uuid>,        // Currently active comp
    },
    media: Arc<RwLock<HashMap<Uuid, NodeKind>>>, // Thread-safe media pool
    compositor: Mutex<CompositorType>,           // CPU/GPU compositor
    cache_manager: Option<Arc<CacheManager>>,
    global_cache: Option<Arc<GlobalFrameCache>>,
    event_emitter: Option<EventEmitter>,         // Auto-emit on changes
}
```

**Unified Media Pool**:
```
media: HashMap<Uuid, NodeKind>
  ├─ FileNode: Image sequences, video files
  ├─ CompNode: Nested compositions with layers
  ├─ CameraNode: Camera transforms
  └─ TextNode: Text overlays
```

**modify_comp() Pattern** (Auto-Emit):
```rust
project.modify_comp(uuid, |comp| {
    comp.set_child_attrs(...);  // attrs.set() → dirty=true
});
// ▲ Auto-emits AttrsChangedEvent if comp/layers dirty
//   → Triggers cache.clear_comp() and viewport refresh
```

---

### 4. **CacheManager** (`src/core/cache_man.rs`)

**Purpose**: Global memory tracking with epoch-based cancellation

```rust
CacheManager {
    memory_usage: Arc<AtomicUsize>,      // Current usage (bytes)
    max_memory_bytes: AtomicUsize,       // Memory limit
    current_epoch: Arc<AtomicU64>,       // Cancellation epoch
}
```

**Memory Calculation**:
```
Available Memory:
  system.available_memory() - reserve_gb

Usage Limit:
  (available * mem_fraction)
  Example: 75% of (64GB - 2GB reserve) = 46.5GB
```

**Epoch Mechanism** (Cancels Stale Preloads):
```
User scrubs timeline rapidly:
  SetFrameEvent(100) → epoch=1
  SetFrameEvent(150) → epoch=2  ← increment_epoch()
  SetFrameEvent(200) → epoch=3

Worker threads check epoch before loading:
  if current_epoch() == request_epoch {
      load_frame();  // Still valid
  } else {
      skip;  // Stale request, user moved on
  }
```

---

### 5. **GlobalFrameCache** (`src/core/global_cache.rs`)

**Purpose**: Nested HashMap cache with LRU eviction

**Structure**:
```rust
GlobalFrameCache {
    cache: Arc<RwLock<HashMap<Uuid, HashMap<i32, Frame>>>>,
    //                ▲comp_uuid    ▲frame_idx
    lru_order: Arc<Mutex<IndexSet<CacheKey>>>,  // Insertion order
    cache_manager: Arc<CacheManager>,
    strategy: Arc<Mutex<CacheStrategy>>,  // LastOnly | All
    capacity: usize,  // Max frames before eviction
}
```

**Nested Structure Benefits**:
```
O(1) clear_comp(uuid):
  cache.remove(&uuid)  // Removes entire inner HashMap

O(1) lookup:
  cache.get(&uuid)?.get(&frame_idx)
```

**Cache Strategies**:
- **LastOnly**: Keep only most recent frame per comp (minimal memory)
- **All**: Cache all frames in work area (maximum performance)

**LRU Eviction**:
```
1. Insert: frame added to back of IndexSet (most recent)
2. Access: frame moved to back (via shift_remove + re-insert)
3. Evict: Remove from front (oldest first)

Eviction triggers:
  - Memory limit exceeded (CacheManager.check_memory_limit())
  - Capacity limit exceeded (len() > capacity)
```

**Dehydration vs Full Clear**:
```rust
// Dehydrate: Mark Loaded → Expired (pixels stay valid)
cache.clear_comp(uuid, dehydrate=true);
  → Frames remain in cache, status=Expired
  → Fast to recompute (data still in memory)

// Full Clear: Remove from cache entirely
cache.clear_comp(uuid, dehydrate=false);
  → Frames removed, memory freed
  → Used when deleting node or major structure change
```

---

### 6. **Workers** (`src/core/workers.rs`)

**Purpose**: Global thread pool with work-stealing for priority execution

**Structure**:
```rust
Workers {
    injector: Arc<Injector<Job>>,         // Global task queue
    handles: Vec<thread::JoinHandle<()>>, // Thread pool
    current_epoch: Arc<AtomicU64>,        // Shared with CacheManager
    shutdown: Arc<AtomicBool>,
}
```

**Work-Stealing Deques**:
```
Thread Pool (num_cpus * 3/4):

  Worker 1:
    [New Tasks] ← push to front (high priority)
    [Old Tasks] ← steal from back

  Worker 2:
    [Own Queue] → pop from front (LIFO, cache locality)
    ↓ Steal from others when idle

  Worker N:
    Checks: own queue → injector → steal from others
```

**Priority Execution**:
```
New tasks (recent SetFrameEvent):
  - Pushed to injector
  - Workers check injector before stealing
  - Effectively high priority

Old tasks (stale preloads):
  - Age to back of deques
  - Stolen last
  - Cancelled if epoch changed
```

**Epoch-Based Cancellation**:
```rust
workers.execute_with_epoch(epoch, || {
    if current_epoch() == epoch {
        load_frame();  // Still valid
    }
    // Otherwise: silently skip (epoch changed)
});
```

---

## Event System Architecture

### Event Types & Hierarchy

```
Events (Trait: Any + Send + Sync + 'static)
│
├── Player Events (src/core/player_events.rs)
│   ├── StopEvent
│   ├── TogglePlayPauseEvent
│   ├── SetFrameEvent(i32)
│   ├── StepForward/BackwardEvent
│   ├── JumpToStart/EndEvent
│   ├── JogForward/BackwardEvent
│   └── IncreaseFPS/DecreaseFPSBaseEvent
│
├── Comp Events (src/entities/comp_events.rs)
│   ├── CurrentFrameChangedEvent { comp_uuid, old_frame, new_frame }
│   ├── LayersChangedEvent { comp_uuid, affected_range: Option<(i32, i32)> }
│   ├── AttrsChangedEvent(uuid)  ◄── Cache invalidation
│   ├── AddLayerEvent { comp_uuid, source_uuid, start_frame, insert_idx }
│   ├── RemoveLayerEvent { comp_uuid, layer_idx }
│   ├── MoveLayerEvent { comp_uuid, layer_idx, new_start }
│   ├── SetLayerAttrsEvent { comp_uuid, layer_uuids, attrs }
│   └── CompSelectionChangedEvent { comp_uuid, selection, anchor }
│
├── Project Events (src/widgets/project/project_events.rs)
│   ├── AddClipEvent(PathBuf)
│   ├── AddClipsEvent(Vec<PathBuf>)
│   ├── AddFolderEvent(PathBuf)
│   ├── RemoveMediaEvent(Uuid)
│   ├── SelectMediaEvent(Uuid)
│   └── ProjectActiveChangedEvent { uuid, target_frame }
│
├── Timeline Events (src/widgets/timeline/timeline_events.rs)
│   ├── TimelineZoomChangedEvent(f32)
│   ├── TimelinePanChangedEvent(f32)
│   ├── TimelineSnapChangedEvent(bool)
│   └── TimelineFitEvent { selected_only: bool }
│
└── Viewport Events (src/widgets/viewport/viewport_events.rs)
    ├── ZoomViewportEvent(f32)
    ├── ResetViewportEvent
    └── FitViewportEvent
```

### Event Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                    EVENT LIFECYCLE                              │
└─────────────────────────────────────────────────────────────────┘

1. USER ACTION
   │
   ▼
┌──────────────────┐
│  UI Widget       │
│  (egui)          │
│                  │
│  if clicked {    │
│    emit(Event)   │
│  }               │
└─────────┬────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────────┐
│  EventBus::emit<E>(event)                                        │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ 1. IMMEDIATE: Invoke subscribers synchronously             │ │
│  │    for cb in subscribers[TypeId::of::<E>()] {              │ │
│  │        cb(&event);  ◄── Runs immediately                   │ │
│  │    }                                                        │ │
│  │                                                             │ │
│  │ 2. DEFERRED: Push to queue for main loop                   │ │
│  │    queue.push(Box::new(event));                            │ │
│  └────────────────────────────────────────────────────────────┘ │
└───────────────────────┬──────────────────────────────────────────┘
                        │
          ┌─────────────┴──────────────┐
          │                            │
          ▼                            ▼
┌─────────────────────┐      ┌─────────────────────┐
│ IMMEDIATE CALLBACKS │      │  DEFERRED QUEUE     │
│                     │      │                     │
│ Example:            │      │ Main Loop (60Hz):   │
│  - Cache.clear()    │      │   for ev in poll() {│
│  - Dirty flags      │      │     handle(ev);     │
│  - State updates    │      │   }                 │
└─────────────────────┘      └──────┬──────────────┘
                                    │
                                    ▼
                          ┌─────────────────────┐
                          │ handle_app_event()  │
                          │                     │
                          │ Match event type:   │
                          │  - Player control   │
                          │  - Project changes  │
                          │  - UI updates       │
                          │  - Layer ops        │
                          └──────┬──────────────┘
                                 │
                   ┌─────────────┼─────────────┐
                   ▼             ▼             ▼
              [Player]      [Project]     [Workers]
                   │             │             │
                   └─────────────┴─────────────┘
                                 ▼
                           [Viewport Refresh]
```

### Cache Invalidation Events

```
AttrsChangedEvent(comp_uuid)
  │
  ├─ Emitted by: project.modify_comp() when comp.is_dirty()
  │
  ├─ Triggered by:
  │   ├─ comp.set_child_attrs() - Layer attributes change
  │   ├─ comp.add_layer() - Layer added
  │   ├─ comp.remove_layer() - Layer removed
  │   ├─ comp.move_layers() - Layer position change
  │   └─ comp.trim_layers() - Trim adjustments
  │
  └─ Handler Actions:
      ├─ 1. cache_manager.increment_epoch()
      │      → Cancels pending worker tasks
      │
      ├─ 2. global_cache.clear_comp(uuid, dehydrate=true)
      │      → Marks cached frames as Expired
      │
      └─ 3. Invalidate parent comps (cascade)
           → Recursively clear comps that reference this one

LayersChangedEvent { comp_uuid, affected_range }
  │
  ├─ Emitted by: Direct layer structure changes
  │
  ├─ affected_range:
  │   ├─ Some(start, end): Clear only this frame range
  │   └─ None: Clear entire comp cache
  │
  └─ Handler: Similar to AttrsChangedEvent but range-aware
```

---

## Frame Loading Pipeline

### Overview

```
┌────────────────────────────────────────────────────────────────┐
│              FRAME LOADING STATE MACHINE                       │
└────────────────────────────────────────────────────────────────┘

Frame States (FrameStatus):

  Placeholder ──────────► Header ──────────► Loading ──────────► Loaded
      │                     │                   │                  │
      │                     │                   ▼                  │
      │                     │                 Error                │
      │                     │                                      │
      │                     └──────────────────────────────────────┤
      │                                                            │
      └────────────────────────────────────────────────────────────┤
                                                                   │
                                  ┌────────────────────────────────┘
                                  │
                                  ▼
                              Composing ───────────► Loaded
                                  │                    │
                                  ▼                    ▼
                                Error              Expired
                                                (Stale, needs refresh)

State Descriptions:
  - Placeholder: No filename, green placeholder pixels
  - Header: Filename set, resolution known, placeholder pixels
  - Loading: Async file loading in progress (FileNode)
  - Composing: Async composition in progress (CompNode)
  - Loaded: Cached (File: image loaded, Comp: composed result)
  - Expired: Was Loaded, now stale - pixels valid but need recompute
  - Error: Loading/composition failed
```

### Loading Flow (FileNode)

```
User adds clip to timeline
   │
   ▼
FileNode::new(path)
   │
   ├─ Frame::new_unloaded(path)
   │    status = Header
   │    resolution = 1x1 (placeholder)
   │
   └─ Background: load_header()
        │
        ├─ Video: get_dimensions() via FFmpeg
        │   └─ Update: width, height, status=Header
        │
        └─ Image: Loader::header()
             ├─ EXR: openexr-rs header OR image crate
             ├─ PNG/JPG: image crate
             └─ Update: width, height, channels, status=Header

User sets frame (playhead moves)
   │
   ▼
SetFrameEvent(42) emitted
   │
   ├─ Main Loop: handle_app_event()
   │    └─ enqueue_frames = true
   │
   └─ enqueue_current_frame()
        │
        └─ check global_cache.get(comp_uuid, 42)
             │
             ├─ Cache HIT: return cached frame
             │
             └─ Cache MISS:
                  │
                  ├─ Create placeholder: Frame::new_composing()
                  │    └─ global_cache.insert(comp, 42, placeholder)
                  │         status = Composing
                  │
                  └─ workers.execute_with_epoch(epoch, move || {
                         │
                         ├─ Collect source frames for composition
                         │    │
                         │    └─ For each layer:
                         │         │
                         │         ├─ FileNode.compute_frame(local_frame)
                         │         │    │
                         │         │    └─ global_cache.get_or_insert(file_uuid, local_frame, || {
                         │         │         │
                         │         │         ├─ Frame::new_composing()
                         │         │         │    status = Loading (atomic claim)
                         │         │         │
                         │         │         ├─ frame.load() ◄── ACTUAL FILE I/O
                         │         │         │    │
                         │         │         │    ├─ try_claim_for_loading()
                         │         │         │    │    Header → Loading (atomic)
                         │         │         │    │
                         │         │         │    ├─ Detect format (EXR/PNG/Video)
                         │         │         │    │
                         │         │         │    ├─ Loader::load(path)
                         │         │         │    │    ├─ EXR: openexr-rs OR image crate
                         │         │         │    │    │    → PixelBuffer::F16 or F32
                         │         │         │    │    ├─ PNG/JPG: image crate
                         │         │         │    │    │    → PixelBuffer::U8
                         │         │         │    │    └─ Video: FFmpeg decode
                         │         │         │    │         → PixelBuffer::U8
                         │         │         │    │
                         │         │         │    └─ status = Loaded
                         │         │         │
                         │         │         └─ global_cache.insert(file, frame, frame)
                         │         │              └─ cache_manager.add_memory(size)
                         │         │
                         │         └─ Return frame
                         │
                         ├─ Compositor.blend(source_frames)
                         │    └─ See Composition Pipeline
                         │
                         └─ global_cache.insert(comp, 42, composed_frame)
                              status = Loaded
                     })
```

### Preload Strategy (Spiral Pattern)

```
Current Frame: 100
  │
  └─ Preload order (Spiral strategy):
       100 (current, priority=highest)
       101 (+1)
       99  (-1)
       102 (+2)
       98  (-2)
       103 (+3)
       97  (-3)
       ...

Each preload enqueued with:
  workers.execute_with_epoch(current_epoch, || {
      if current_epoch() == request_epoch {
          // Load frame
      } else {
          // Skip (user moved on)
      }
  });

Alternative: Forward strategy for video
  100, 101, 102, 103... (backward seeking expensive)
```

---

## Cache System Dataflow

### Cache Hierarchy

```
┌─────────────────────────────────────────────────────────────────┐
│                     CACHE ARCHITECTURE                          │
└─────────────────────────────────────────────────────────────────┘

CacheManager (Global Singleton)
  │
  ├─ memory_usage: AtomicUsize ◄── Tracks total bytes
  │                                 Updated by:
  │                                   - global_cache.insert()
  │                                   - global_cache.evict()
  │
  ├─ max_memory_bytes: AtomicUsize ◄── User-configurable limit
  │                                     Example: 46.5GB (75% of 64GB - 2GB)
  │
  └─ current_epoch: Arc<AtomicU64> ◄── Shared with Workers
                                        Incremented on SetFrameEvent

GlobalFrameCache
  │
  ├─ cache: Arc<RwLock<HashMap<Uuid, HashMap<i32, Frame>>>>
  │          ▲comp_uuid         ▲frame_idx
  │
  │   Nested structure benefits:
  │     - O(1) clear_comp(uuid): remove entire inner HashMap
  │     - O(1) lookup: cache[uuid][frame_idx]
  │     - Concurrent reads: RwLock allows multiple readers
  │
  ├─ lru_order: Arc<Mutex<IndexSet<CacheKey>>>
  │          Insertion-order tracking for LRU eviction
  │          CacheKey { comp_uuid, frame_idx }
  │
  ├─ cache_manager: Arc<CacheManager>
  │          Shared memory tracker
  │
  ├─ strategy: Arc<Mutex<CacheStrategy>>
  │     LastOnly: Keep only most recent frame per comp
  │     All: Cache all frames in work area
  │
  └─ capacity: usize
         Max frames before eviction (default: 10000)
```

### Cache Operations Flow

#### INSERT Operation

```
global_cache.insert(comp_uuid, frame_idx, frame)
  │
  ├─ 1. Apply strategy
  │     │
  │     └─ if strategy == LastOnly:
  │          clear_comp(comp_uuid, dehydrate=false)
  │
  ├─ 2. Check eviction triggers
  │     │
  │     ├─ MEMORY LIMIT:
  │     │    while cache_manager.check_memory_limit() {
  │     │        evict_oldest();
  │     │    }
  │     │
  │     └─ CAPACITY LIMIT:
  │          while len() > capacity {
  │              evict_oldest();
  │          }
  │
  ├─ 3. Acquire locks (nested)
  │     │
  │     ├─ cache.write()
  │     │    └─ Exclusive access to cache HashMap
  │     │
  │     └─ lru_order.lock()
  │          └─ Exclusive access to eviction queue
  │
  ├─ 4. Replace existing if present
  │     │
  │     └─ if let Some(old_frame) = cache[comp][frame_idx] {
  │            cache_manager.free_memory(old_frame.mem());
  │            lru_order.shift_remove(&key);
  │        }
  │
  ├─ 5. Insert new frame
  │     │
  │     ├─ cache.entry(comp_uuid).or_default().insert(frame_idx, frame);
  │     ├─ lru_order.insert(CacheKey { comp_uuid, frame_idx });
  │     └─ cache_manager.add_memory(frame.mem());
  │
  └─ 6. Release locks
```

#### GET Operation (with LRU update)

```
global_cache.get(comp_uuid, frame_idx)
  │
  ├─ 1. Read cache (shared lock)
  │     │
  │     └─ cache.read()
  │          .get(&comp_uuid)?
  │          .get(&frame_idx)
  │          .cloned()
  │
  ├─ 2. Update statistics
  │     │
  │     ├─ Cache HIT: stats.record_hit()
  │     └─ Cache MISS: stats.record_miss()
  │
  └─ 3. Update LRU order (if hit)
       │
       └─ lru_order.lock() {
              shift_remove(&key);  // Remove from current position
              insert(key);         // Add to back (most recent)
          }
```

#### EVICT Operation (LRU)

```
evict_oldest()
  │
  ├─ 1. Acquire locks
  │     │
  │     ├─ cache.write()
  │     └─ lru_order.lock()
  │
  ├─ 2. Get oldest key
  │     │
  │     └─ key = lru_order.shift_remove_index(0)?
  │               ▲ Front of IndexSet = oldest
  │
  ├─ 3. Remove from cache
  │     │
  │     └─ if let Some(evicted) = cache[key.comp_uuid].remove(&key.frame_idx) {
  │            cache_manager.free_memory(evicted.mem());
  │
  │            // Remove empty inner HashMap
  │            if cache[key.comp_uuid].is_empty() {
  │                cache.remove(&key.comp_uuid);
  │            }
  │        }
  │
  └─ 4. Release locks
```

#### CLEAR_COMP Operation

```
global_cache.clear_comp(comp_uuid, dehydrate)
  │
  ├─ dehydrate=true: Mark frames as Expired (pixels stay)
  │    │
  │    └─ cache.write() {
  │           for frame in cache[comp_uuid].values() {
  │               if frame.status() == Loaded {
  │                   frame.set_status(Expired);
  │               }
  │           }
  │       }
  │
  └─ dehydrate=false: Remove frames entirely
       │
       ├─ cache.write()
       ├─ lru_order.lock()
       │
       └─ if let Some(frames) = cache.remove(&comp_uuid) {
              for (_, frame) in frames {
                  cache_manager.free_memory(frame.mem());
              }
              lru_order.retain(|k| k.comp_uuid != comp_uuid);
          }
```

### Memory Tracking Flow

```
Frame Lifecycle → Memory Updates:

1. LOAD:
   frame.load()
     ├─ Read file: 4K EXR (16-bit RGBA) = 4096×2160×4×2 = 70MB
     └─ PixelBuffer::F16(vec![...; 70MB])

2. INSERT TO CACHE:
   global_cache.insert(comp, frame_idx, frame)
     └─ cache_manager.add_memory(70MB)
          └─ memory_usage.fetch_add(70MB)

3. EVICTION CHECK:
   if memory_usage > max_memory_bytes {
       evict_oldest()
         └─ cache_manager.free_memory(70MB)
              └─ memory_usage.saturating_sub(70MB)
   }

4. CLEAR_COMP:
   global_cache.clear_comp(uuid, false)
     └─ for each frame:
          cache_manager.free_memory(frame.mem())
```

---

## Composition Pipeline

### CompNode Architecture

```
CompNode {
    attrs: Attrs {
        uuid: Uuid,
        name: String,
        in: i32,           // Timeline start
        out: i32,          // Timeline end
        trim_in: i32,      // Offset from in (work area)
        trim_out: i32,     // Offset from out (work area)
        fps: f32,
        frame: i32,        // Current playhead
        width: u32,
        height: u32,
    },
    layers: Vec<Layer>,    // Bottom-to-top render order
    layer_selection: Vec<Uuid>,
}

Layer {
    attrs: Attrs {
        uuid: Uuid,           // Layer instance UUID
        source_uuid: Uuid,    // Source node in media pool
        name: String,
        in: i32,              // Start on parent timeline
        src_len: i32,         // Source duration
        trim_in: i32,         // Offset in source frames
        trim_out: i32,        // Offset in source frames
        opacity: f32,
        visible: bool,
        solo: bool,
        blend_mode: String,   // "normal", "screen", "add", etc.
        speed: f32,           // 1.0 = normal, 2.0 = 2x faster
        // Transform:
        position: Vec3,
        rotation: Vec3,
        scale: Vec3,
        pivot: Vec3,
    }
}
```

### Composition Flow

```
┌─────────────────────────────────────────────────────────────────┐
│             FRAME COMPOSITION PIPELINE                          │
└─────────────────────────────────────────────────────────────────┘

User views CompNode at frame 100
  │
  ▼
project.compute_frame(comp_uuid, 100)
  │
  └─ comp.compute_frame(100, ctx)
       │
       ├─ Check cache:
       │    global_cache.get(comp_uuid, 100)
       │      ├─ HIT: return cached frame
       │      └─ MISS: continue to compose
       │
       ├─ Reserve cache slot:
       │    global_cache.insert(comp, 100, Frame::new_composing())
       │      └─ Prevents race condition (multiple workers composing same frame)
       │
       └─ workers.execute_with_epoch(epoch, move || {
              compose_internal(comp, 100, ctx)
          })

compose_internal(comp, parent_frame, ctx)
  │
  ├─ 1. CYCLE DETECTION
  │     │
  │     └─ COMPOSE_STACK.with(|stack| {
  │            if stack.contains(&comp.uuid()) {
  │                return Error("Cyclic dependency");
  │            }
  │            stack.insert(comp.uuid());
  │        })
  │
  ├─ 2. COLLECT VISIBLE LAYERS
  │     │
  │     └─ layers.iter().rev()  ◄── Reverse: bottom-to-top
  │          .filter(|layer| {
  │              layer.is_visible() &&
  │              layer.work_area().contains(parent_frame)
  │          })
  │
  ├─ 3. COMPUTE SOURCE FRAMES
  │     │
  │     └─ for layer in visible_layers {
  │            │
  │            ├─ Convert parent frame → local source frame:
  │            │    local_frame = layer.parent_to_local(parent_frame)
  │            │
  │            │    Example:
  │            │      layer.start = 50
  │            │      layer.speed = 2.0
  │            │      parent_frame = 100
  │            │
  │            │      offset = 100 - 50 = 50
  │            │      local = 50 * 2.0 = 100
  │            │
  │            ├─ Get source node from media pool:
  │            │    source = media.get(layer.source_uuid)?
  │            │
  │            ├─ Recursively compute source frame:
  │            │    frame = source.compute_frame(local_frame, ctx)?
  │            │      │
  │            │      ├─ FileNode: global_cache.get() or load from disk
  │            │      └─ CompNode: Recursive compose_internal()
  │            │
  │            ├─ Apply CPU transform (if needed):
  │            │    │
  │            │    └─ Build inverse transform matrix:
  │            │         matrix = transform::build_inverse_matrix_3x3(
  │            │             position, rotation, scale, pivot
  │            │         )
  │            │
  │            │         frame = transform::transform_frame(
  │            │             &frame, matrix, comp.dim()
  │            │         )
  │            │
  │            └─ Collect: (frame, opacity, blend_mode, matrix)
  │        }
  │
  ├─ 4. BLEND LAYERS (CPU Compositor)
  │     │
  │     └─ THREAD_COMPOSITOR.with(|comp| {
  │            comp.blend_with_dim(source_frames, comp.dim())
  │        })
  │          │
  │          ├─ Create canvas from first frame
  │          │    (cropped/padded to comp dimensions)
  │          │
  │          └─ For each subsequent layer:
  │               │
  │               ├─ Determine overlap region
  │               │
  │               ├─ Match pixel format (U8/F16/F32):
  │               │    PixelBuffer::F32 × F32 → blend_f32()
  │               │    PixelBuffer::F16 × F16 → blend_f16()
  │               │    PixelBuffer::U8 × U8   → blend_u8()
  │               │
  │               └─ Apply blend mode per pixel:
  │                    Normal:     top_color
  │                    Screen:     1 - (1-b)*(1-t)
  │                    Add:        b + t
  │                    Subtract:   b - t
  │                    Multiply:   b * t
  │                    Divide:     b / t
  │                    Difference: |b - t|
  │
  │                    Alpha blend:
  │                      top_alpha = top[i+3] * opacity
  │                      inv_alpha = 1.0 - top_alpha
  │
  │                      result[i] = bottom[i] * inv_alpha +
  │                                  blend(bottom[i], top[i]) * top_alpha
  │
  ├─ 5. SET COMPOSED STATUS
  │     │
  │     └─ Calculate minimum status from all source frames:
  │          Error → Placeholder → Header → Loading/Composing → Loaded
  │
  │          composed_frame.status = min_status
  │
  ├─ 6. INSERT TO CACHE
  │     │
  │     └─ global_cache.insert(comp, 100, composed_frame)
  │          └─ cache_manager.add_memory(frame.mem())
  │
  └─ 7. CLEANUP
       │
       └─ COMPOSE_STACK.with(|stack| {
              stack.remove(&comp.uuid());
          })
```

### Compositor Backends

```
CompositorType:
  │
  ├─ CPU (CpuCompositor)
  │    │
  │    ├─ Blend functions:
  │    │    blend_f32(bottom, top, opacity, mode, result)
  │    │    blend_f16(bottom, top, opacity, mode, result)
  │    │    blend_u8(bottom, top, opacity, mode, result)
  │    │
  │    ├─ Transform: PRE-APPLIED via transform::transform_frame()
  │    │    (matrix parameter ignored, kept for API compatibility)
  │    │
  │    └─ Used by: Worker threads (no GL context)
  │
  └─ GPU (GpuCompositor) [Optional, requires OpenGL context]
       │
       ├─ Fragment shader blending (10-50x faster)
       ├─ Transform: GPU shader mat3 uniform
       │    (not yet integrated with compose_internal)
       │
       └─ Used by: Viewport rendering (main thread only)
```

### GPU Transform Integration (Work-in-Progress)

```
Current State:
  ✓ API ready: blend() accepts transform matrix
  ✓ GPU shader ready: u_top_transform uniform
  ✓ Matrix builder ready: build_inverse_matrix_3x3()
  ✗ NOT integrated: compose_internal uses CPU transforms

To Enable GPU Compositing:
  1. Run compose_internal in main thread (has GL context)
  2. Pass Project.compositor to compose_internal via ComputeContext
  3. Remove CPU transform preprocessing
  4. Let GPU shader handle transforms via matrix uniform
```

---

## Project State Management

### State Storage

```
Project State Hierarchy:

  Project.attrs {
      comps_order: Vec<Uuid>,     // UI display order
      selection: Vec<Uuid>,        // Multi-selection (Ctrl+click)
      active: Option<Uuid>,        // Currently active comp
  }

  Project.media: Arc<RwLock<HashMap<Uuid, NodeKind>>> {
      uuid_1 → FileNode {
          attrs { uuid, name, path, in, out, width, height, ... }
          frames: HashMap<i32, Frame>  // REMOVED: now in GlobalFrameCache
      }

      uuid_2 → CompNode {
          attrs { uuid, name, in, out, trim_in, trim_out, fps, frame, ... }
          layers: Vec<Layer> {
              Layer {
                  attrs { uuid, source_uuid, name, in, src_len, trim_in,
                          trim_out, opacity, visible, blend_mode, speed,
                          position, rotation, scale, pivot, ... }
              }
          }
          layer_selection: Vec<Uuid>
          layer_selection_anchor: Option<Uuid>
      }

      uuid_3 → CameraNode { attrs {...} }
      uuid_4 → TextNode { attrs {...} }
  }

  Player.attrs {
      active_comp: Option<Uuid>,   // Duplicates Project.active (legacy)
      is_playing: bool,
      fps_base: f32,
      fps_play: f32,
      loop_enabled: bool,
      play_direction: f32,
  }
```

### Serialization (Project → JSON)

```
project.to_json("scene.json")
  │
  ├─ Serialized (serde):
  │    ├─ attrs: Attrs
  │    ├─ media: HashMap<Uuid, NodeKind>
  │    └─ (All nested attrs in FileNode/CompNode/Layer)
  │
  └─ NOT Serialized (#[serde(skip)]):
       ├─ compositor: Mutex<CompositorType>
       ├─ cache_manager: Option<Arc<CacheManager>>
       ├─ global_cache: Option<Arc<GlobalFrameCache>>
       ├─ event_emitter: Option<EventEmitter>
       └─ last_save_path: Option<PathBuf>

Deserialization:

  Project::from_json("scene.json")
    │
    ├─ 1. serde_json::from_str() → Project
    │
    ├─ 2. rebuild_runtime(None)
    │      ├─ Create new CacheManager
    │      ├─ Create new GlobalFrameCache
    │      └─ compositor = CPU (default)
    │
    ├─ 3. attach_schemas()
    │      ├─ project.attrs.attach_schema(&PROJECT_SCHEMA)
    │      └─ For each node in media:
    │           node.attach_schema() (recursively)
    │
    └─ 4. Caller: project.set_event_emitter(emitter)
            └─ Restore event-driven cache invalidation
```

### Dirty Tracking & Auto-Emit

```
Two Separate Dirty Systems:

1. COMP DIRTY (Attrs.is_dirty())
   │
   ├─ Purpose: Marks CompNode for recomputation
   │
   ├─ Set by: attrs.set(...) when value changes
   │
   ├─ Checked by: project.modify_comp()
   │
   └─ Effect: Emits AttrsChangedEvent
        └─ Triggers cache.clear_comp()

2. NODE EDITOR DIRTY (NodeEditorState.mark_dirty())
   │
   ├─ Purpose: Marks graph UI for redraw
   │
   └─ Effect: Visual refresh only, no cache invalidation

modify_comp() Auto-Emit Pattern:

  project.modify_comp(uuid, |comp| {
      // Mutations here may set dirty=true
      comp.add_layer(...);           // Calls attrs.mark_dirty()
      comp.set_child_attrs(...);     // Calls attrs.mark_dirty()
      comp.layers.push(...);         // REQUIRES manual mark_dirty()
  });

  // After closure returns:
  if comp.is_dirty() {
      event_emitter.emit(AttrsChangedEvent(uuid));
      comp.clear_dirty();
  }

Methods that AUTO mark_dirty():
  - add_layer()
  - remove_layer()
  - move_layers()
  - trim_layers()
  - set_child_attrs()

Methods that DON'T mark_dirty():
  - set_frame() (playhead is non-DAG in schema)

Direct field changes REQUIRE manual mark_dirty():
  comp.layers = reordered;       // Direct assignment
  comp.layers.insert(idx, layer); // Direct insert
  layer.attrs.set(...);           // Direct layer attr change
  // → comp.attrs.mark_dirty()
```

### Schema System (Attrs)

```
Attrs {
    schema: Option<AttrSchema>,  // Metadata (name, type, dirty behavior)
    data: HashMap<String, AttrValue>,
    dirty_keys: HashSet<String>,
}

AttrSchema:
  - Defines attribute names and types
  - Marks attributes as "dag" (dirties comp) or "non-dag" (UI-only)

Example:

  COMP_SCHEMA:
    "uuid"       → Uuid,    non-dag
    "name"       → Str,     non-dag
    "in"         → Int,     dag  ◄── Changing bounds dirties comp
    "out"        → Int,     dag
    "frame"      → Int,     non-dag  ◄── Playhead doesn't dirty comp
    "opacity"    → Float,   dag

  When attrs.set("frame", 100):
    → dirty_keys NOT updated (non-dag)

  When attrs.set("opacity", 0.5):
    → dirty_keys.insert("opacity")  (dag attribute)
```

---

## Complete User Input → Rendering Flow

### Example: User Presses Space (Play/Pause)

```
┌─────────────────────────────────────────────────────────────────┐
│  USER ACTION: Press Space                                      │
└─────────────────────────────────────────────────────────────────┘

1. KEYBOARD INPUT
   │
   egui keyboard handler
     └─ if ctx.input(|i| i.key_pressed(Key::Space)) {
            event_bus.emit(TogglePlayPauseEvent);
        }

2. EVENT BUS
   │
   EventBus::emit(TogglePlayPauseEvent)
     │
     ├─ Immediate callbacks: (none for this event)
     │
     └─ Deferred queue: push to event queue

3. MAIN LOOP (60Hz)
   │
   for event in event_bus.poll() {
       handle_app_event(event, player, project, ...);
   }
     │
     └─ if downcast_event::<TogglePlayPauseEvent>(event) {
            player.set_is_playing(!player.is_playing());
            if player.is_playing() {
                player.last_frame_time = Some(Instant::now());
            }
        }

4. PLAYBACK UPDATE (called at 60Hz while playing)
   │
   player.update(project)
     │
     ├─ Check elapsed time:
     │    now - last_frame_time >= frame_duration (1/fps)?
     │
     ├─ Yes: advance_frame(project)
     │    │
     │    └─ project.modify_comp(active_comp, |comp| {
     │           comp.set_frame(current + 1);
     │       });
     │         │
     │         └─ Emits: CurrentFrameChangedEvent
     │
     └─ Update: last_frame_time = now

5. FRAME LOADING (triggered by SetFrameEvent or CurrentFrameChangedEvent)
   │
   enqueue_current_frame()
     │
     └─ global_cache.get(comp_uuid, frame)
          │
          ├─ Cache HIT: return cached frame
          │
          └─ Cache MISS:
               │
               └─ workers.execute_with_epoch(epoch, || {
                      compose_internal(comp, frame, ctx)
                        │
                        └─ See Composition Pipeline
                  })

6. VIEWPORT RENDERING
   │
   viewport.ui(ui, player, project, ...)
     │
     ├─ Get current frame:
     │    frame = player.get_current_frame(project)
     │      └─ project.compute_frame(comp_uuid, frame_idx)
     │
     ├─ Upload to GPU:
     │    texture = viewport.upload_frame(frame)
     │
     └─ Render quad:
          shader.draw(texture, transform_matrix)
```

### Example: User Adds Clip to Timeline

```
┌─────────────────────────────────────────────────────────────────┐
│  USER ACTION: Drag file to timeline                            │
└─────────────────────────────────────────────────────────────────┘

1. DRAG & DROP
   │
   Timeline widget
     └─ if let Some(dropped) = ctx.input(|i| i.raw.dropped_files) {
            event_bus.emit(AddClipsEvent(paths));
        }

2. EVENT HANDLING
   │
   handle_app_event(AddClipsEvent)
     │
     └─ result.load_sequences = Some(paths);

3. MAIN LOOP (after event processing)
   │
   if let Some(paths) = result.load_sequences {
       for path in paths {
           load_clip(path, project);
       }
   }

4. LOAD_CLIP
   │
   load_clip(path, project)
     │
     ├─ 1. Detect sequence:
     │      seq = scanseq::scan_path(path)
     │        └─ "render.%04d.exr" → first=1, last=100
     │
     ├─ 2. Create FileNode:
     │      file_node = FileNode::new(name, first, last, path_pattern)
     │        │
     │        ├─ For each frame in range:
     │        │    frame = Frame::new_unloaded(path)
     │        │      └─ status = Header, 1x1 placeholder
     │        │
     │        └─ Background: load_header() for resolution
     │             └─ Loader::header(path)
     │                  ├─ Video: FFmpeg metadata
     │                  └─ Image: image crate header
     │
     ├─ 3. Insert to media pool:
     │      project.media.write().insert(uuid, NodeKind::File(file_node))
     │
     ├─ 4. Add to UI order:
     │      project.push_comps_order(uuid)
     │
     └─ 5. Select as active:
            player.set_active_comp(Some(uuid), project)
              │
              ├─ project.set_selection(vec![uuid])
              ├─ project.set_active(Some(uuid))
              └─ Emit: CurrentFrameChanged
                   └─ Triggers frame loading
```

### Example: User Changes Layer Opacity

```
┌─────────────────────────────────────────────────────────────────┐
│  USER ACTION: Drag opacity slider in Attribute Editor          │
└─────────────────────────────────────────────────────────────────┘

1. UI WIDGET
   │
   Attribute Editor
     └─ ui.add(Slider::new(&mut opacity, 0.0..=1.0));
          if opacity changed {
              event_bus.emit(SetLayerAttrsEvent {
                  comp_uuid,
                  layer_uuids: vec![layer_uuid],
                  attrs: vec![("opacity", AttrValue::Float(opacity))],
              });
          }

2. EVENT HANDLING
   │
   handle_app_event(SetLayerAttrsEvent)
     │
     └─ project.modify_comp(e.comp_uuid, |comp| {
            comp.set_child_attrs(layer_uuid, vec![
                ("opacity", AttrValue::Float(opacity))
            ]);
        });
          │
          ├─ layer.attrs.set("opacity", AttrValue::Float(opacity))
          │    └─ dirty_keys.insert("opacity")  (dag attribute)
          │
          └─ comp.attrs.mark_dirty()

3. AUTO-EMIT (in modify_comp)
   │
   if comp.is_dirty() {
       event_emitter.emit(AttrsChangedEvent(comp_uuid));
       comp.clear_dirty();
   }

4. CACHE INVALIDATION (AttrsChangedEvent handler)
   │
   handle_app_event(AttrsChangedEvent)
     │
     ├─ 1. Increment epoch:
     │      cache_manager.increment_epoch()
     │        → Cancels pending worker tasks
     │
     ├─ 2. Clear comp cache:
     │      global_cache.clear_comp(uuid, dehydrate=true)
     │        → Marks Loaded frames as Expired
     │
     └─ 3. Invalidate parents:
          for parent in find_parent_comps(uuid) {
              global_cache.clear_comp(parent, dehydrate=true);
          }

5. VIEWPORT REFRESH
   │
   Next frame:
     viewport.ui() calls player.get_current_frame()
       │
       └─ global_cache.get(comp, frame)
            │
            ├─ Status = Expired: Recompose
            │
            └─ workers.execute_with_epoch(new_epoch, || {
                   compose_internal(comp, frame, ctx)
                     └─ Uses new opacity value
               })
```

---

## Thread Safety & Concurrency

### Lock Hierarchy

```
READ HIERARCHY (least → most restrictive):

  Arc<RwLock<...>>  ◄── Multiple concurrent readers
    └─ Used by:
         - Project.media (media pool)
         - GlobalFrameCache.cache (frame storage)

  Arc<Mutex<...>>   ◄── Exclusive access
    └─ Used by:
         - GlobalFrameCache.lru_order (eviction queue)
         - Project.compositor (blend operations)
         - Frame.data (atomic state transitions)

  AtomicUsize/AtomicU64/AtomicBool  ◄── Lock-free
    └─ Used by:
         - CacheManager.memory_usage
         - CacheManager.current_epoch
         - Workers.shutdown
```

### Deadlock Prevention

```
Lock Ordering Rules:

1. NEVER acquire locks in reverse order

   ✓ Good:
      cache.write() → lru.lock()

   ✗ Bad:
      lru.lock() → cache.write()  ◄── DEADLOCK RISK

2. Hold locks for MINIMAL time

   ✓ Good:
      let result = cache.read().get(...).cloned();
      drop(cache);  // Release before processing

   ✗ Bad:
      let cache = cache.read();
      process(cache.get(...));  // Lock held during processing

3. Avoid nested locking across boundaries

   ✓ Good:
      {
          let cache = cache.write();
          let lru = lru.lock();
          // Both locks in same scope
      }

   ✗ Bad:
      let cache = cache.write();
      call_function_that_locks_lru();  // Hidden nested lock
```

### Atomic State Transitions

```
Frame Loading Race Prevention:

Problem:
  Thread 1 checks: frame.status() == Header
  Thread 2 checks: frame.status() == Header
  Both start loading → wasted work, double memory usage

Solution: try_claim_for_loading()

  fn try_claim_for_loading(&self) -> bool {
      let mut data = self.data.lock().unwrap();
      if data.status == FrameStatus::Header {
          data.status = FrameStatus::Loading;  // Atomic claim
          true  // Caller MUST load
      } else {
          false  // Already claimed/loaded, caller MUST skip
      }
  }

Usage:
  if frame.try_claim_for_loading() {
      // Only ONE thread gets here
      frame.load()?;
      frame.set_status(Loaded);
  }
```

### Worker Thread Model

```
Main Thread (egui):
  ├─ UI rendering (60Hz)
  ├─ Event polling & handling
  ├─ OpenGL context (viewport, GPU compositor)
  └─ Enqueues tasks to Workers

Worker Threads (num_cpus * 3/4):
  ├─ Frame loading (Loader::load)
  ├─ Composition (compose_internal + CPU compositor)
  ├─ NO OpenGL context (CPU compositor only)
  └─ Epoch-based cancellation

Thread Communication:

  Main → Workers:
    workers.execute_with_epoch(epoch, || { ... })
      └─ Injector::push(job)

  Workers → Main:
    global_cache.insert(comp, frame, result)
      └─ Main thread reads cache on next viewport.ui()
```

### Arc/Mutex Cloning Patterns

```
Efficient Sharing (Cheap Clone):

  Arc<CacheManager>:
    ├─ Cloned to: Workers, GlobalFrameCache, Project
    └─ Cost: Increment atomic refcount (lock-free)

  Arc<GlobalFrameCache>:
    ├─ Cloned to: Project, ComputeContext (passed to workers)
    └─ Cost: Increment atomic refcount

Mutex Interior Mutability:

  CompositorType wrapped in Mutex<...>:
    └─ Allows &Project (immutable ref) to mutate compositor

    Usage:
      let mut compositor = project.compositor.lock().unwrap();
      compositor.blend(frames)?;
      // Lock released when compositor goes out of scope

RwLock Read/Write:

  Project.media: Arc<RwLock<HashMap<...>>>

    Concurrent reads:
      let media = project.media.read().unwrap();
      // Multiple threads can hold read lock simultaneously

    Exclusive write:
      let mut media = project.media.write().unwrap();
      // Blocks all readers and other writers
```

---

## Performance Characteristics

### Cache Performance

```
Operation              | Time Complexity | Lock Contention
-----------------------|-----------------|------------------
get(comp, frame)       | O(1)            | Read lock (low)
insert(comp, frame)    | O(1)            | Write lock (high)
evict_oldest()         | O(1)            | Write lock (high)
clear_comp(uuid)       | O(n_frames)     | Write lock (high)
clear_range(start,end) | O(range_size)   | Write lock (high)

LRU Update on Cache Hit:
  shift_remove() → O(n) for IndexSet
  Re-insert() → O(1)

  → Acceptable: Cache hits are fast path, LRU update amortized
```

### Memory Profile (Typical 4K EXR Sequence)

```
4K EXR Frame (16-bit RGBA):
  4096 × 2160 × 4 channels × 2 bytes = 70 MB/frame

Cache Capacity:
  10000 frames × 70 MB = 700 GB (theoretical max)

  Actual usage limited by:
    - CacheManager.max_memory_bytes (user-configurable)
    - GlobalFrameCache.capacity (10000 frames default)

LRU Eviction:
  Keeps working set in memory
  Example: 100 frames × 70 MB = 7 GB active
```

### Composition Performance

```
CPU Compositor:
  1920×1080 RGBA8 blend: ~1-2ms
  4K RGBA8 blend: ~5-10ms
  4K RGBA16F blend: ~15-30ms (f16 conversions)

GPU Compositor (when integrated):
  Any resolution blend: <1ms
  10-50x faster than CPU
  Requires OpenGL context (main thread only)

Bottlenecks:
  - File I/O: EXR decode ~50-200ms
  - CPU blending: ~10-30ms for 4K
  - Memory bandwidth: F16↔F32 conversions
```

---

## Debugging & Observability

### Logging Levels

```rust
RUST_LOG=playa=trace    // Verbose: all operations
RUST_LOG=playa=debug    // Cache ops, composition
RUST_LOG=playa=info     // Lifecycle events
RUST_LOG=playa=warn     // Errors, warnings
```

### Key Log Points

```
Cache Operations:
  - "Cached frame: {uuid}:{idx} ({bytes} bytes)"
  - "LRU evicted: {uuid}:{idx} (freed {mb} MB)"
  - "Cleared comp {uuid}: {count} frames, {mb} MB freed"

Composition:
  - "compose_internal(comp={uuid}, frame={idx})"
  - "Composed {layers} layers → {width}×{height}"
  - "Cycle detected: {path}"

Workers:
  - "Worker {id} started"
  - "Workers shutting down ({count} threads)..."
  - "Epoch incremented: {epoch}"

Events:
  - "[EVENT] SetFrameEvent → {frame}"
  - "[EVENT] AttrsChangedEvent → comp={uuid}"
  - "EventBus queue full ({count} events), evicting oldest {n}"
```

---

## Future Improvements

### Planned Features

1. **GPU Compositor Integration**
   - Run compose_internal in main thread
   - Pass Project.compositor to ComputeContext
   - Remove CPU transform preprocessing
   - Let GPU shaders handle transforms

2. **Advanced Caching**
   - Predictive preload (ML-based frame prediction)
   - Compressed cache (LZ4/Zstd for inactive frames)
   - Disk cache spillover for large projects

3. **Performance**
   - SIMD optimizations for CPU blending
   - Multi-threaded composition (parallel layer rendering)
   - Incremental composition (dirty region tracking)

4. **Concurrency**
   - Lock-free cache structures (concurrent HashMap)
   - Reduce write lock contention
   - Fine-grained locking per comp

---

## Conclusion

This architecture document provides a complete overview of Playa's data flow from user input to rendering. Key architectural decisions:

- **Event-driven**: UI never computes directly, all work via EventBus
- **Cache-centric**: GlobalFrameCache with LRU eviction and epoch cancellation
- **Thread-safe**: Arc/RwLock/Mutex for safe concurrent access
- **Modular**: Clean separation of concerns (Player, Project, Workers, Cache)
- **Extensible**: Plugin architecture for FileNode/CompNode/CameraNode/TextNode

For implementation details, refer to individual module documentation in source files.
