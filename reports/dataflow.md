# Playa Architecture: Data Flow Documentation

## Overview

Playa is a video compositor application built with Rust + egui. It has 5 major components
plus a core entity hierarchy. This document describes what each component contains,
what it accepts and outputs, and how data flows between them.

```
                    ┌──────────────────────────────────────┐
                    │              main.rs (App)           │
                    │   owns: Player, EventBus, Workers    │
                    └────┬─────────┬──────────┬───────────┘
                         │         │          │
            ┌────────────▼───┐  ┌──▼──────┐  ┌▼────────────┐
            │    Project     │  │Timeline │  │  Viewport   │
            │(media, cache)  │  │ (state) │  │  (display)  │
            └────────┬───────┘  └────┬────┘  └──────┬──────┘
                     │               │              │
                     └───────────────┼──────────────┘
                                     │
                              ┌──────▼──────┐
                              │    Comp     │
                              │(dual-mode)  │
                              └──────┬──────┘
                                     │
                    ┌────────────────┴────────────────┐
                    │                                 │
              ┌─────▼─────┐                    ┌──────▼──────┐
              │  Layer    │                    │   File      │
              │  Mode     │                    │   Mode      │
              │(compose)  │                    │ (sequence)  │
              └─────┬─────┘                    └─────────────┘
                    │
           ┌───────▼───────┐
           │ children_attrs│
           │  per-child    │
           └───────────────┘
```

---

## 1. Project (src/entities/project.rs)

### Purpose
Top-level container for all media and scene state. Unit of serialization.

### Contains
```rust
pub struct Project {
    pub attrs: Attrs,                                    // Global project attrs (fps, resolution)
    pub media: Arc<RwLock<HashMap<String, Comp>>>,      // All comps keyed by UUID
    pub comps_order: Vec<String>,                        // Display order in UI
    pub selection: Vec<String>,                          // Selected items
    pub active: Option<String>,                          // Currently active comp UUID
    pub compositor: RefCell<CompositorType>,             // CPU/GPU compositor (runtime)
    cache_manager: Option<Arc<CacheManager>>,            // Memory management (runtime)
    pub global_cache: Option<Arc<GlobalFrameCache>>,     // Frame cache (runtime)
}
```

### Input (What it accepts)
| Method | Input | Effect |
|--------|-------|--------|
| `add_comp(comp)` | Comp | Injects cache + adds to media + order |
| `update_comp(comp)` | Comp | Updates existing comp (NO cache injection!) |
| `modify_comp(uuid, fn)` | UUID + closure | In-place mutation |
| `from_json(path)` | File path | Deserialize + rebuild_runtime |
| `set_cache_manager(mgr)` | CacheManager | Sets for all existing comps |
| `rebuild_with_manager(mgr, sender)` | CacheManager + EventSender | Full rebuild |

### Output (What it provides)
| Method | Output | Consumer |
|--------|--------|----------|
| `get_comp(uuid)` | Option<Comp> (cloned) | Timeline, Viewport, Encoder |
| `to_json(path)` | JSON file | Persistence |
| `cache_manager()` | Arc<CacheManager> | Workers, Comps |
| `global_cache` field | Arc<GlobalFrameCache> | Frame loading |

### Data Flow
```
                    ┌─────────────┐
  add_comp() ──────►│   Project   │◄────── from_json()
  update_comp() ───►│             │
                    │  media      │──────► get_comp() ──► Viewport, Encoder
                    │  cache      │──────► global_cache ──► Comp.compose()
                    └─────────────┘
```

### **CRITICAL ISSUE FOUND: update_comp() vs add_comp()**
```
update_comp() does NOT inject global_cache!
Used incorrectly for NEW comps in load_sequences() and "New Comp" creation.
Fix: Always use add_comp() for new comps.
```

---

## 2. Timeline (src/widgets/timeline/)

### Purpose
Visual timeline editor for layer arrangement and playback control.

### Contains
```rust
pub struct TimelineConfig {
    pub pixels_per_frame: f32,    // Base scale
    pub row_height: f32,          // Layer row height
}

pub struct TimelineState {
    pub zoom: f32,                // Zoom multiplier (0.1..4.0)
    pub pan_offset: f32,          // Horizontal scroll (frames)
    pub drag_state: Option<GlobalDragState>,  // Active drag
    pub snap_enabled: bool,       // Snap to edges
    pub snap_edges: Vec<i32>,     // Available snap points
    pub outline_width: f32,       // Outline panel width
}

pub struct GlobalDragState {
    pub kind: DragKind,           // What's being dragged
    pub layer_uuid: String,       // Target layer
    pub start_pos: Pos2,          // Drag origin
    pub snap_edges: Vec<i32>,     // Snap points
}
```

### Input (What it accepts)
| Source | Data | Method |
|--------|------|--------|
| main.rs | &Comp | render_canvas(comp, ...) |
| main.rs | &mut TimelineState | Mutable state reference |
| User | Mouse/keyboard | egui interactions |

### Output (What it provides)
| Output | Type | Consumer |
|--------|------|----------|
| AppEvent | Events via dispatch closure | EventBus |
| TimelineActions | { hovered: bool } | main.rs input routing |
| Selection changes | Via AppEvent | Project.selection |

### Events Emitted
```rust
// Transport
AppEvent::JumpToStart
AppEvent::JumpToEnd
AppEvent::TogglePlayback
AppEvent::SetPlaySpeed(f32)

// Frame navigation
AppEvent::SetCurrentFrame(i32)
AppEvent::ScrubStarted
AppEvent::ScrubEnded
AppEvent::Scrubbing(i32)

// Layer manipulation
AppEvent::SetLayerStart(comp_uuid, layer_uuid, frame)
AppEvent::SetLayerEnd(comp_uuid, layer_uuid, frame)
AppEvent::MoveLayerBlock(comp_uuid, Vec<layer_uuid>, delta)
AppEvent::SetWorkAreaStart(comp_uuid, layer_uuid, frame)
AppEvent::SetWorkAreaEnd(comp_uuid, layer_uuid, frame)

// Selection
AppEvent::SelectLayers(comp_uuid, Vec<layer_uuid>, anchor)
```

### Data Flow
```
   TimelineState
        │
   ┌────▼─────┐    egui input    ┌──────────┐
   │ Timeline │◄─────────────────│   User   │
   │  render  │                  └──────────┘
   └────┬─────┘
        │ dispatch(AppEvent)
        ▼
   ┌────────────┐
   │  EventBus  │
   └────┬───────┘
        │
        ▼
   ┌────────────┐
   │  Project   │ mutations
   │  Comp      │
   └────────────┘
```

---

## 3. Viewport (src/widgets/viewport/)

### Purpose
Frame display with pan/zoom and OpenGL rendering.

### Contains
```rust
pub struct ViewportState {
    pub zoom: f32,                    // Display zoom
    pub pan: egui::Vec2,              // Pan offset
    pub mode: ViewportMode,           // Fit/Fill/100%/Auto
    pub image_size: egui::Vec2,       // Current frame size (runtime)
    pub viewport_size: egui::Vec2,    // Panel size (runtime)
    pub scrubber: ViewportScrubber,   // Drag-to-scrub state
}

pub struct ViewportScrubber {
    is_active: bool,
    last_frame: Option<i32>,
    start_frame: Option<i32>,
}

pub struct ViewportRenderer {
    // OpenGL state: VAO, VBO, program, textures
    // Shader compilation and uniform management
}
```

### Input (What it accepts)
| Source | Data | Method |
|--------|------|--------|
| main.rs | Option<&Frame> | render(frame, ...) |
| main.rs | &mut ViewportState | Mutable state |
| main.rs | ViewportRenderer | OpenGL rendering |
| main.rs | Shaders | Shader selection |
| User | Mouse drag | Pan/zoom/scrub |
| User | File drop | Load sequence |

### Output (What it provides)
| Output | Type | Consumer |
|--------|------|----------|
| ViewportActions | { load_sequence, hovered } | main.rs |
| render_time_ms | f32 | Status bar |
| Scrub events | Via dispatch | EventBus |

### Data Flow
```
   Frame (from Comp.get_frame)
        │
        ▼
   ┌────────────┐   ViewportState
   │  Viewport  │◄──────────────
   │  render()  │
   └────┬───────┘
        │
        ▼ OpenGL
   ┌────────────┐
   │ Renderer   │──────► Display
   │ (GPU)      │
   └────────────┘
```

### Scrubbing Flow
```
Mouse drag in viewport
        │
        ▼
ViewportScrubber.start()
        │
        ├──► AppEvent::ScrubStarted
        │
   [dragging]
        │
        ├──► AppEvent::Scrubbing(frame)
        │
ViewportScrubber.end()
        │
        └──► AppEvent::ScrubEnded
```

---

## 4. Prefs (src/dialogs/prefs/)

### Purpose
Application settings dialog for cache, compositor, and behavior configuration.

### Contains
```rust
pub struct AppSettings {
    // Cache settings
    pub cache_threshold: f32,         // Memory pressure threshold (0.0..1.0)
    pub cache_target_usage: f32,      // Target after eviction (GB)

    // Compositor settings
    pub use_gpu_compositor: bool,     // CPU vs GPU compositing

    // Behavior settings
    pub auto_preload: bool,           // Preload around playhead
    pub preload_radius: usize,        // Frames to preload
}
```

### Input (What it accepts)
| Source | Data |
|--------|------|
| main.rs | Current AppSettings |
| User | UI changes |
| User | "Apply" click |

### Output (What it provides)
| Output | Consumer |
|--------|----------|
| Modified AppSettings | main.rs |
| AppEvent::ApplySettings | EventBus |
| Project.set_compositor() | Compositor switch |

### Data Flow
```
   AppSettings (current)
        │
        ▼
   ┌────────────┐
   │  Prefs     │◄──── User input
   │  dialog    │
   └────┬───────┘
        │
        ▼
   Modified AppSettings
        │
        ├──► CacheManager.update_thresholds()
        ├──► Project.set_compositor()
        └──► Workers.set_preload_radius()
```

---

## 5. Encoder (src/dialogs/encode/)

### Purpose
Export compositions to video files via FFmpeg.

### Contains
```rust
pub struct EncoderSettings {
    pub output_path: PathBuf,
    pub codec: VideoCodec,            // H264/H265/AV1/ProRes
    pub container: Container,         // MP4/MOV/MKV
    pub hw_accel: HardwareAccel,      // NVENC/QSV/AMF/CPU
    pub quality: QualityPreset,
    pub bitrate: Option<u32>,
    pub fps: f32,
    pub resolution: (u32, u32),
}

pub enum VideoCodec {
    H264, H265, AV1, ProRes,
}

pub enum HardwareAccel {
    None, NVENC, QSV, AMF, VideoToolbox,
}
```

### Input (What it accepts)
| Source | Data | Method |
|--------|------|--------|
| main.rs | &Comp | encode_comp(comp, settings) |
| main.rs | &Project | Frame resolution via compose() |
| User | EncoderSettings | Dialog configuration |
| User | Frame range | Start/end frames |

### Output (What it provides)
| Output | Consumer |
|--------|----------|
| Video file | Filesystem |
| Progress updates | UI progress bar |
| Error messages | Dialog display |

### Data Flow
```
   Comp + Project
        │
        ▼
   ┌────────────┐   for frame in range
   │  Encoder   │───────────────────────┐
   │  dialog    │                       │
   └────────────┘                       ▼
                                 ┌────────────┐
                                 │Comp.compose│
                                 │ (frame)    │
                                 └─────┬──────┘
                                       │
                                       ▼ Frame
                                 ┌────────────┐
                                 │  FFmpeg    │
                                 │  stdin     │
                                 └─────┬──────┘
                                       │
                                       ▼
                                 Video file
```

---

## 6. Comp (src/entities/comp.rs)

### Purpose
Unified composition entity with dual-mode operation.

### Modes
```
┌─────────────────────────────────────────────────────────────────┐
│                           Comp                                  │
├─────────────────────────────┬───────────────────────────────────┤
│       Layer Mode            │           File Mode               │
├─────────────────────────────┼───────────────────────────────────┤
│ - Composes children         │ - Loads image sequence            │
│ - children: Vec<String>     │ - file_mask: "*.exr"              │
│ - children_attrs: HashMap   │ - file_start/file_end             │
│ - compose() recursion       │ - resolve_frame_path()            │
│ - BlendMode per child       │ - frame_from_path()               │
└─────────────────────────────┴───────────────────────────────────┘
```

### Contains
```rust
pub struct Comp {
    pub uuid: String,
    pub mode: CompMode,                    // Layer | File
    pub attrs: Attrs,                      // name, start, end, fps, play_start, play_end

    // Layer mode fields
    pub parent: Option<String>,            // Parent comp UUID
    pub children: Vec<String>,             // Ordered child UUIDs
    pub children_attrs: HashMap<String, Attrs>,  // Per-child attributes

    // File mode fields
    pub file_mask: Option<String>,         // Glob pattern
    pub file_start: Option<i32>,           // Sequence start
    pub file_end: Option<i32>,             // Sequence end

    // Common
    pub current_frame: i32,
    pub layer_selection: Vec<String>,

    // Runtime (not serialized)
    event_sender: CompEventSender,
    cache_manager: Option<Arc<CacheManager>>,
    global_cache: Option<Arc<GlobalFrameCache>>,
}
```

### children_attrs Structure
```rust
// Each child in a Layer mode comp has its own Attrs:
children_attrs: HashMap<String, Attrs> = {
    "child_uuid_1": Attrs {
        "uuid": source_comp_uuid,      // Source reference
        "start": 0,                    // Position in parent timeline
        "end": 100,                    // End position
        "play_start": 10,              // Work area start (trimmed)
        "play_end": 90,                // Work area end (trimmed)
        "width": 1920,                 // Cached dimensions
        "height": 1080,
        "opacity": 1.0,                // Blend opacity (0.0..1.0)
        "blend_mode": "normal",        // BlendMode
        "speed": 1.0,                  // Playback speed multiplier
        "visible": true,               // Layer visibility
    },
    "child_uuid_2": Attrs { ... },
}
```

### Input (What it accepts)
| Method | Input | Effect |
|--------|-------|--------|
| `set_current_frame(n)` | frame number | Updates playhead, emits event |
| `add_layer(uuid, attrs)` | source UUID + attrs | Adds child to composition |
| `remove_child(uuid)` | child UUID | Removes layer |
| `set_cache_manager(mgr)` | CacheManager | For memory tracking |
| `set_global_cache(cache)` | GlobalFrameCache | For frame caching |
| `signal_preload(workers, project)` | Workers + Project | Background loading |

### Output (What it provides)
| Method | Output | Consumer |
|--------|--------|----------|
| `get_frame(idx, project)` | Option<Frame> | Viewport |
| `compose(idx, project, use_gpu)` | Option<Frame> | get_frame, Encoder |
| `get_file_frame(idx, project)` | Option<Frame> | File mode loading |
| `dim()` | (width, height) | Layout, Encoder |
| `work_area()` | (start, end) | Playback range |

### Frame Resolution Flow (Layer Mode)
```
get_frame(frame_idx, project)
        │
        ├─► Check global_cache
        │       │
        │       ├── HIT: return cached frame
        │       │
        │       └── MISS: ──────────────────┐
        │                                   │
        ▼                                   ▼
compose(frame_idx, project, use_gpu)
        │
        │   for child in children.rev()
        │       │
        │       ├─► Get child_attrs from children_attrs[child_uuid]
        │       │
        │       ├─► Check if frame_idx in child's range
        │       │       child_start <= frame_idx <= child_end
        │       │
        │       ├─► Convert to local frame:
        │       │       local = (frame_idx - child_start) * speed + play_start
        │       │
        │       ├─► Get source comp from project.media[source_uuid]
        │       │
        │       └─► RECURSE: source.get_frame(local, project)
        │
        ▼
Blend all frames with compositor (CPU or GPU)
        │
        └─► Cache result in global_cache
```

### Frame Resolution Flow (File Mode)
```
get_frame(frame_idx, project)
        │
        ├─► Check global_cache
        │       │
        │       ├── HIT: return cached frame
        │       │
        │       └── MISS: ──────────────────┐
        │                                   │
        ▼                                   ▼
get_file_frame(frame_idx, project)
        │
        ├─► resolve_frame_path(frame_idx)
        │       Maps comp frame to sequence number
        │
        ├─► frame_from_path(path)
        │       Creates Frame::new_unloaded(path)
        │       *** BUG: Returns green placeholder! ***
        │
        └─► Cache in global_cache
                *** BUG: Caches unloaded frame! ***
```

### Preload Flow
```
signal_preload(workers, project, preload_fn)
        │
        ├─► Calculate frames around playhead
        │
        └─► for frame in preload_range
                │
                └─► enqueue_frame(workers, project, epoch, frame)
                        │
                        ├─► Check self.global_cache
                        │       │
                        │       ├── EXISTS: skip
                        │       │
                        │       └── NONE: early return
                        │           *** BUG: Silent failure! ***
                        │
                        └─► Queue to Workers for background load
```

### CompEvent Emissions
```rust
CompEvent::CurrentFrameChanged { frame }  // After set_current_frame()
CompEvent::CompDirty { uuid }             // After attr changes
CompEvent::FrameLoaded { uuid, frame }    // After background load
```

---

## Event System

### Event Types
```rust
// User actions → App mutations
pub enum AppEvent {
    // Transport
    TogglePlayback,
    JumpToStart,
    JumpToEnd,
    SetPlaySpeed(f32),
    SetCurrentFrame(i32),

    // Scrubbing
    ScrubStarted,
    ScrubEnded,
    Scrubbing(i32),

    // Layer manipulation
    SetLayerStart(String, String, i32),
    SetLayerEnd(String, String, i32),
    MoveLayerBlock(String, Vec<String>, i32),

    // Selection
    SelectLayers(String, Vec<String>, Option<String>),
    SetActiveComp(String),

    // Settings
    ApplySettings(AppSettings),
}

// Comp internal events
pub enum CompEvent {
    CurrentFrameChanged { frame: i32 },
    CompDirty { uuid: String },
    FrameLoaded { uuid: String, frame: i32 },
}
```

### Event Flow
```
   UI Widget (Timeline/Viewport/Project)
        │
        │ dispatch(AppEvent)
        ▼
   ┌────────────┐
   │  EventBus  │
   └─────┬──────┘
         │
         ▼
   ┌────────────┐
   │  main.rs   │ handle_event()
   │  App       │
   └─────┬──────┘
         │
         ├──► Project mutations
         ├──► Comp mutations
         ├──► Player state changes
         └──► UI state updates
```

---

## Known Issues & Logical Holes

### Issue 1: update_comp() Cache Injection (FIXED)
**Location:** `src/main.rs:238`, `src/main.rs:1324`
**Problem:** New comps created via `update_comp()` don't receive `global_cache`
**Fix:** Use `add_comp()` instead

### Issue 2: enqueue_frame() Silent Failure
**Location:** `src/entities/comp.rs:690-697`
**Problem:** If `self.global_cache` is None, preload silently aborts
**Impact:** Background loading disabled without warning

### Issue 3: get_file_frame() Caches Unloaded Frames
**Location:** `src/entities/comp.rs:853-866`
**Problem:** `frame_from_path()` returns green placeholder, which gets cached
**Impact:** Green screen persists even after fix attempt

### Issue 4: children_attrs Inconsistency
**Problem:** `children_attrs` uses instance UUID as key, but stores source UUID in "uuid" attr
**Flow:**
```
children: ["instance_uuid_1", "instance_uuid_2"]
children_attrs: {
    "instance_uuid_1": { "uuid": "source_comp_uuid", ... }
}
```
This creates confusion between instance (layer) and source (clip/comp).

### Issue 5: Dual Cache References
**Problem:** Comp has both `self.global_cache` and uses `project.global_cache`
**Location:** `enqueue_frame()` uses `self.global_cache`, `get_file_frame()` uses `project.global_cache`
**Impact:** Inconsistent behavior when caches diverge

### Issue 6: compose() Thread Safety
**Problem:** `compose()` can be called from main thread (GPU) or worker thread (CPU)
**Solution:** Uses `use_gpu` flag, but THREAD_COMPOSITOR is thread-local
**Risk:** If called incorrectly, could cause data races

---

## Initialization Flow

### Correct Initialization (via add_comp)
```
Project::new(cache_manager)
    │
    └─► Creates global_cache
        │
        ▼
add_comp(comp)
    │
    ├─► comp.set_cache_manager(cache_manager)
    ├─► comp.set_global_cache(global_cache)
    ├─► media.insert(uuid, comp)
    └─► comps_order.push(uuid)
```

### Broken Initialization (via update_comp)
```
update_comp(comp)
    │
    ├─► media.insert(uuid, comp)  // Just overwrites!
    └─► NO cache injection!       // BUG!
```

### Deserialization Flow
```
Project::from_json(path)
    │
    ├─► serde deserialize (no runtime fields)
    │
    └─► rebuild_runtime(event_sender)
            │
            ├─► For each comp in media:
            │       comp.set_event_sender()
            │       comp.set_global_cache()  // Only if self.global_cache exists!
            │
            └─► Note: cache_manager must be set BEFORE via rebuild_with_manager()
```

---

## Recommendations

1. **Unify cache injection** - Single entry point for all comp additions
2. **Add logging to enqueue_frame()** - Warn on missing cache
3. **Fix frame_from_path()** - Either load synchronously or don't cache unloaded
4. **Clarify instance vs source** - Rename children_attrs key to layer_uuid
5. **Single cache reference** - Always use project.global_cache, remove self.global_cache
