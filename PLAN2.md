# PLAYA Bug Hunt Report - Comprehensive Analysis

**Date**: 2025-12-17
**Version Analyzed**: 0.1.133
**Branch**: dev3
**Total Lines of Code**: ~15,000+ (Rust source)

---

## Executive Summary

Playa is a well-architected image sequence player with event-driven design. However, the bug hunt revealed several issues requiring attention:

| Severity | Count | Description |
|----------|-------|-------------|
| **Critical** | 2 | Circular deps, panic! in production code |
| **High** | 6 | Architecture violations, inconsistent error handling |
| **Medium** | 8 | Code duplication, magic numbers, leaky abstractions |
| **Low** | 5 | TODO/FIXME items, dead code with #[allow] |

---

## 1. CRITICAL ISSUES

### 1.1 Panic! in Production Code

**Files & Lines**:
- `src/dialogs/encode/encode.rs:1283` - panic in encoder
- `src/entities/frame.rs:1312, 1319, 1326` - panic in PixelBuffer matching

**Code**:
```rust
// frame.rs:1312-1326
_ => panic!("Wrong variant"),  // x3 occurrences
```

**Impact**: Application crash on unexpected state
**Fix**: Replace with `Result<T, E>` or `Option<T>.expect("context message")`

### 1.2 Circular Dependency: entities <-> core

**Problem**:
```
entities/comp_node.rs -> entities/node.rs (uses ComputeContext, Node trait)
entities/node.rs -> core/global_cache.rs, core/workers.rs
core/workers.rs -> entities/node.rs (Workers need Node trait)
```

**Impact**:
- Tight coupling prevents clean testing
- Violates dependency inversion principle

**Fix**:
1. Extract `ComputeContext` to `core/compute.rs`
2. Use dependency injection for cache/workers

---

## 2. HIGH PRIORITY ISSUES

### 2.1 Thread-Local State in Domain Layer

**File**: `src/entities/comp_node.rs:82-85`

```rust
thread_local! {
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = ...;
    static COMPOSE_STACK: RefCell<HashSet<Uuid>> = ...;
}
```

**Problem**: Domain entity manages threading concerns
**Fix**: Move to `ComputeContext`, pass explicitly

### 2.2 Inconsistent Error Handling (86 instances)

**Pattern mix**:
- `Result<T, FrameError>` - good
- `Result<T, String>` - poor
- `anyhow::Result<T>` - inconsistent
- `.unwrap()/.expect()` - dangerous (86 instances across 17 files)

**High-risk files**:
| File | unwrap/expect count |
|------|---------------------|
| frame.rs | 25 |
| project.rs | 17 |
| encode.rs | 8 |
| main.rs | 7 |

**Fix**: Create unified `PlayaError` enum, convert all unwrap to `?`

### 2.3 PlayaApp God Object (1950 lines)

**File**: `src/main.rs:61-157`

**Responsibilities** (13+):
1. Application state
2. Event handling
3. Keyboard input
4. Fullscreen mode
5. Settings persistence
6. Project I/O
7. Sequence detection
8. Frame preloading
9. Dock layout
10. Dialog management
11. Compositor selection
12. Worker pool
13. Cache management

**Fix**: Extract to `AppState`, `AppServices`, `DialogManager`, `InputRouter`

### 2.4 Project Contains Event Emission Logic

**File**: `src/entities/project.rs:145, 490-518`

```rust
// Domain entity knows about infrastructure
event_emitter: Option<EventEmitter>,

pub fn modify_comp(&self, uuid, f) {
    if dirty { emitter.emit(AttrsChangedEvent(uuid)); }
}
```

**Fix**: Return `ChangeResult` enum, let caller emit events

### 2.5 Player Contains Frame Fetching

**File**: `src/core/player.rs:265-282`

```rust
// Player should only manage playback state
pub fn get_current_frame(&self, project: &Project) -> Option<Frame>
```

**Fix**: Extract `FrameProvider` trait, Player emits `FrameNeeded` event

### 2.6 GPU Compositor Not Integrated

**Files**: `src/entities/compositor.rs:8-28`, `gpu_compositor.rs:244, 849`

```rust
// compositor.rs:18-21
// **NOT YET WORKING:**
// - GPU compositor not used for compose (requires GL context, can't run in workers)
```

**TODOs found**:
- `gpu_compositor.rs:244` - "TODO: implement GPU texture caching"
- `gpu_compositor.rs:849` - "TODO: Implement proper canvas-sized blending"

**Impact**: GPU path unused, performance opportunity lost

---

## 3. MEDIUM PRIORITY ISSUES

### 3.1 Code Duplication: Node Methods

**Files**: file_node.rs, comp_node.rs, camera_node.rs, text_node.rs, node_kind.rs

**Duplicated methods**:
- `_in()`, `_out()`, `fps()`, `dim()`, `frame_count()`
- `placeholder_frame()` - identical in file_node.rs:137-140 and comp_node.rs:847-850
- `attach_schema()` - pattern repeated 5x
- `work_area()`/`work_area_abs()` - similar logic in 3 places

**Fix**: Add to `Node` trait as default methods, or use derive macro

### 3.2 Magic Numbers Throughout

**Examples** (50+ instances):
```rust
unwrap_or(0)       // timing defaults
unwrap_or(1.0)     // opacity, speed
unwrap_or(24.0)    // fps in file_node.rs:91, comp_node.rs:287
64                 // default dimensions
1920, 1080         // comp_node.rs:248-249
```

**Fix**: Create `defaults.rs`:
```rust
pub const DEFAULT_FPS: f32 = 24.0;
pub const DEFAULT_OPACITY: f32 = 1.0;
pub const DEFAULT_DIM: (usize, usize) = (1920, 1080);
```

### 3.3 Hardcoded Colors in Widgets (50+ instances)

**Files**: timeline_ui.rs, timeline_helpers.rs, project_ui.rs, node_graph.rs

**Examples**:
```rust
Color32::from_gray(30)     // multiple files
Color32::from_gray(35)     // multiple files
Color32::from_rgb(255, 220, 100)  // playhead color, 5+ places
Color32::from_rgba_unmultiplied(100, 220, 255, 180)  // drop preview
```

**Fix**: Create `theme.rs` or `colors.rs` with constants

### 3.4 Duplicated Attribute Schemas

**File**: `src/entities/attr_schemas.rs`

Timing attrs (`in`, `out`, `trim_in`, `trim_out`, `src_len`, `speed`) repeated in:
- FILE_DEFS (39-44)
- COMP_DEFS (63-68)
- LAYER_DEFS (89-94)
- CAMERA_DEFS (175-180)
- TEXT_DEFS (209-215)

**Fix**: Create reusable `TIMING_DEFS` slice

### 3.5 Dead Code with #[allow(dead_code)]

**File: frame.rs**:
- Line 58: `CropAlign` enum
- Line 68-70: `F16`, `F32` variants of `PixelDepth`
- Line 238, 245, 261: Various Frame methods

**File: gpu_compositor.rs:245**: Texture caching field

**File: viewport.rs:213, 241**: Viewport state fields

**File: encode.rs:413**: Encoder state variant

**Assessment**: Some are legitimately unused (reserved for future), others may be forgotten refactoring remnants

### 3.6 TODO/FIXME Comments

| Location | Content |
|----------|---------|
| compositor.rs:272 | "TODO for GPU compositing" |
| gpu_compositor.rs:244 | "TODO: implement GPU texture caching" |
| gpu_compositor.rs:849 | "TODO: Implement proper canvas-sized blending" |
| lib.rs:5 | "Clippy: allow complex signatures (refactoring TODO)" |

### 3.7 Excessive Clone Calls (176 instances)

**Files with most clones**:
| File | clone() count |
|------|---------------|
| main.rs | 29 |
| main_events.rs | 29 |
| encode_ui.rs | 13 |
| viewport_renderer.rs | 11 |
| project.rs | 11 |

**Impact**: Potential performance issue with large data
**Fix**: Audit and use `&` references or `Cow<>` where possible

### 3.8 Attrs Type Confusion

**Problem**: Every read requires `.unwrap_or(default)`:
```rust
let uuid = attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil);
let name = attrs.get_str(A_NAME).unwrap_or("Untitled");
let frame = attrs.get_i32(A_FRAME).unwrap_or(0);
```

**Fix**: Add typed accessors:
```rust
impl Attrs {
    pub fn uuid(&self) -> Uuid { ... }
    pub fn name(&self) -> &str { ... }
}
```

---

## 4. DATAFLOW DIAGRAM

```
┌─────────────────────────────────────────────────────────────────────┐
│                           PLAYA DATAFLOW                            │
└─────────────────────────────────────────────────────────────────────┘

User Input                    EventBus                     Workers
    │                            │                            │
    ▼                            ▼                            ▼
┌─────────┐    emit()    ┌─────────────┐    submit()   ┌─────────────┐
│ Hotkeys │───────────▶ │  EventBus    │──────────────▶│  Workers    │
│ Mouse   │              │  (pub/sub)   │               │ (thread pool)│
│ D&D     │              └──────┬───────┘               └──────┬──────┘
└─────────┘                     │                              │
                                │ poll()                       │ load frames
                                ▼                              ▼
┌───────────────────────────────────────────────────────────────────┐
│                         PlayaApp.update()                          │
│  ┌─────────────┐  ┌─────────────┐  ┌───────────────┐             │
│  │ handle_     │  │ player.     │  │ debounced_    │             │
│  │ events()    │  │ update()    │  │ preloader     │             │
│  └──────┬──────┘  └──────┬──────┘  └───────┬───────┘             │
│         │                │                 │                      │
│         └────────────────┴─────────────────┘                      │
│                          │                                        │
│                          ▼                                        │
│                   ┌─────────────┐                                 │
│                   │   Project   │◀───── Cache invalidation        │
│                   │   (media)   │        (epoch bump)             │
│                   └──────┬──────┘                                 │
│                          │                                        │
│         ┌────────────────┼────────────────┐                       │
│         ▼                ▼                ▼                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐               │
│  │ GlobalCache │  │  CompNode   │  │  FileNode   │               │
│  │ (frames)    │  │  (compose)  │  │  (source)   │               │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘               │
│         │                │                │                       │
│         └────────────────┴────────────────┘                       │
│                          │                                        │
│                          ▼                                        │
│                   ┌─────────────┐                                 │
│                   │    Frame    │                                 │
│                   │ (pixels)    │                                 │
│                   └──────┬──────┘                                 │
└──────────────────────────┼────────────────────────────────────────┘
                           │
                           ▼
              ┌────────────────────────┐
              │     ViewportRenderer   │
              │    (OpenGL texture)    │
              └────────────────────────┘

Event Types:
  - SetFrameEvent         → scrub/playback
  - AttrsChangedEvent     → cache invalidation + viewport refresh
  - LayersChangedEvent    → recomposition
  - ViewportRefreshEvent  → texture re-upload

Cache Strategy:
  - Epoch counter for stale request cancellation
  - LRU eviction when memory limit hit
  - Dehydrate mode keeps old pixels during recompute
```

---

## 5. REFACTORING PRIORITY

### Phase 1: Critical (Week 1)
- [ ] Remove `panic!()` calls, replace with `Result<T, E>`
- [ ] Create `PlayaError` enum for unified error handling
- [ ] Replace 86 `.unwrap()/.expect()` with proper error propagation

### Phase 2: High (Week 2-3)
- [ ] Break circular dependency entities <-> core
- [ ] Extract `FrameProvider` from Player
- [ ] Decouple Project from EventBus
- [ ] Split PlayaApp (extract services)

### Phase 3: Medium (Week 4-5)
- [ ] Create `defaults.rs` for magic numbers
- [ ] Create `theme.rs` for colors
- [ ] Consolidate Node trait implementations
- [ ] DRY attribute schemas

### Phase 4: Low (Backlog)
- [ ] Audit dead_code markers
- [ ] Complete GPU compositor TODOs
- [ ] Reduce clone() calls
- [ ] Add typed Attrs accessors

---

## 6. METRICS SUMMARY

| Metric | Value |
|--------|-------|
| Source files | 70+ |
| Lines of code | ~15,000+ |
| TODO/FIXME | 4 |
| #[allow(dead_code)] | 10 |
| panic!() calls | 4 |
| unwrap()/expect() | 86 |
| clone() calls | 176 |
| Color32 literals | 50+ |
| Magic number defaults | 50+ |
| Duplicated methods | 15+ |
| God objects | 1 (PlayaApp) |
| Circular deps | 1 |

---

## 7. POSITIVE OBSERVATIONS

Despite issues, the codebase has good practices:

1. **Event-Driven Architecture** - Clean decoupling via EventBus
2. **Arc<RwLock<>> Pattern** - Correct thread-safe shared ownership
3. **Dirty Flag Propagation** - Efficient cache invalidation
4. **Epoch-Based Versioning** - Smart stale task cancellation
5. **Node Trait Abstraction** - Polymorphic frame computation
6. **Schema-Driven Attributes** - Declarative, type-safe attrs
7. **Comprehensive Documentation** - Module docs explain "why"

---

## 8. RECOMMENDED ACTIONS

**Immediate** (before next release):
1. Replace panic!() with proper errors
2. Add default values constants

**Short-term** (1-2 sprints):
1. Unified error handling
2. Break up PlayaApp

**Long-term** (tech debt):
1. GPU compositor completion
2. Full test coverage
3. Performance profiling

---

*Report generated by Claude Code Bug Hunt*
