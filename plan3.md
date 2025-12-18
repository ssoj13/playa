# PLAYA Bug Hunt Report - Unified Analysis (Plan 3)

**Date**: 2025-12-17
**Version**: 0.1.133
**Branch**: dev3
**Based on**: Plan1 (Claude hunt) + Plan2 (previous analysis) + verification

---

## Executive Summary

| Category | Status |
|----------|--------|
| **Critical Issues** | 1 (traits.rs WIP not integrated) |
| **High Priority** | 5 (architecture improvements) |
| **Medium Priority** | 6 (code quality) |
| **Low Priority** | 4 (cleanup) |
| **False Positives Resolved** | 2 |

### Key Correction from Plan1

**traits.rs is NOT dead code** - it's a WIP implementation of Plan2's dependency inversion recommendation. The file exists but hasn't been connected to the module tree yet.

---

## 1. WIP: traits.rs Integration (CRITICAL)

### Current State

File exists: `src/entities/traits.rs` (not tracked in git: `??`)

**Purpose**: Implement dependency inversion per Plan2 recommendation:
- `FrameCache` trait - abstract cache interface
- `WorkerPool` trait - abstract worker interface
- `CacheStrategy` enum - moved here from core
- `CacheStatsSnapshot` - statistics struct
- Blanket impls for `Arc<T>`

**Problem**: File not connected to module system:
```rust
// src/entities/mod.rs - MISSING:
pub mod traits;
pub use traits::{FrameCache, WorkerPool, CacheStrategy};
```

**Impact**:
- Duplicate `CacheStrategy` in `core/global_cache.rs:24` and `traits.rs:14`
- `node.rs` still directly imports `core::global_cache` and `core::workers`
- Circular dependency entities <-> core not resolved

### Integration Plan

```
Phase 1: Connect traits.rs
[ ] Add `pub mod traits;` to entities/mod.rs
[ ] Re-export key types in lib.rs

Phase 2: Implement traits in core
[ ] impl FrameCache for GlobalFrameCache
[ ] impl WorkerPool for Workers
[ ] Add stats_snapshot() method to GlobalFrameCache

Phase 3: Update consumers
[ ] node.rs: use traits instead of concrete types
[ ] Change ComputeContext to use trait objects:
    - cache: &'a dyn FrameCache (instead of &'a Arc<GlobalFrameCache>)
    - workers: Option<&'a dyn WorkerPool>

Phase 4: Cleanup
[ ] Remove CacheStrategy from global_cache.rs (use traits::CacheStrategy)
[ ] Verify no circular imports remain
```

---

## 2. FALSE POSITIVES RESOLVED

### 2.1 panic!() in Production Code - FALSE

**Plan2 stated**: "panic! in production code at frame.rs:1312,1319,1326 and encode.rs:1283"

**Verification**:
```rust
// frame.rs:1307-1328 - ALL IN #[test] FUNCTION
#[test]
fn test_pixel_buffer_types() {
    match buf_u8 {
        PixelBuffer::U8(v) => assert_eq!(...),
        _ => panic!("Wrong variant"),  // OK in tests!
    }
}

// encode.rs:1282-1286 - IN TEST FUNCTION
if found_encoder.is_none() {
    panic!("NO VIDEO ENCODERS FOUND...Skipping test.");
}
```

**Status**: All panic!() calls are in test code. **No production panics found.**
Add comments in place, explaining that those are false alarms and why are they.

### 2.2 Dead Code traits.rs - FALSE

As explained above, traits.rs is WIP for Plan2 dependency inversion, not dead code.

---

## 3. HIGH PRIORITY ISSUES (Confirmed from Plan2)

### 3.1 Circular Dependency: entities <-> core

**Current state** (node.rs:24-25):
```rust
use crate::core::global_cache::GlobalFrameCache;
use crate::core::workers::Workers;
```

**Fix**: Complete traits.rs integration (see Section 1)

### 3.2 Thread-Local State in Domain Layer

**File**: comp_node.rs:82-85
```rust
thread_local! {
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = ...;
    static COMPOSE_STACK: RefCell<HashSet<Uuid>> = ...;
}
```

**Fix**: Move to ComputeContext, pass explicitly

### 3.3 PlayaApp God Object (1950 lines)

**File**: main.rs - 13+ responsibilities

**Fix**: Extract to AppState, AppServices, DialogManager, InputRouter

### 3.4 Project Contains Event Emission Logic

**File**: project.rs:145, 490-518
```rust
event_emitter: Option<EventEmitter>,
pub fn modify_comp(&self, uuid, f) { ... emitter.emit(...) }
```

**Fix**: Return ChangeResult enum, let caller emit

### 3.5 GPU Compositor Not Integrated

**Files**: compositor.rs:8-28, gpu_compositor.rs

**Status**: Documented as WIP. GPU path only works for viewport, not compose pipeline.

**Fix Options**:
1. Run compose on main thread when GPU selected
2. Document GPU as viewport-only
3. Implement GL context sharing (complex)

---

## 4. MEDIUM PRIORITY ISSUES

### 4.1 Code Duplication: Node Methods

Duplicated across file_node.rs, comp_node.rs, camera_node.rs, text_node.rs:
- `_in()`, `_out()`, `fps()`, `dim()`, `frame_count()`
- `placeholder_frame()`
- `attach_schema()`
- `work_area()`/`work_area_abs()`

**Fix**: Add default impls to Node trait

### 4.2 Magic Numbers (50+ instances)

```rust
unwrap_or(24.0)    // fps in multiple files
unwrap_or(1.0)     // opacity, speed
1920, 1080         // default dimensions
```

**Fix**: Create `config.rs`:
```rust
pub const DEFAULT_FPS: f32 = 24.0;
pub const DEFAULT_OPACITY: f32 = 1.0;
pub const DEFAULT_DIM: (usize, usize) = (1920, 1080);
```

### 4.3 Hardcoded Colors (50+ instances)

Multiple widgets use literal Color32 values.

**Fix**: Move them to `config.rs` with named constants

### 4.4 Duplicated Attribute Schemas

Timing attrs repeated in 5 schema definitions.

**Fix**: Extract `TIMING_DEFS` slice and include in others
Check for other duplicate schemas, like Transform or such.

### 4.5 Attrs Setters Bypass Schema Check

Direct setters (`set_i8`, `set_uuid`, `set_list`) always mark dirty.

**Fix**: Route through `set()` which checks schema

### 4.6 Blend Logic Duplicated (F32/F16)

`blend_f32()` and `blend_f16()` have identical logic.

**Fix**: Generic implementation

---

## 5. LOW PRIORITY ISSUES

### 5.1 #[allow(dead_code)] Audit

| File | Item | Status |
|------|------|--------|
| frame.rs:58 | CropAlign enum | Remove if not planned |
| frame.rs:68-70 | F16, F32 PixelDepth | Keep - used by EXR loader |
| frame.rs:238,245,261 | new_f16/f32/from_u8 | Keep for API completeness |
| gpu_compositor.rs:245 | texture_cache | Remove or implement |
| viewport.rs:213,241 | transform methods | Integrate with gizmo |
| encode.rs:413 | Error variant | Actually used - remove annotation |

### 5.2 Redundant Serde Attributes

project.rs:120-122:
```rust
#[serde(skip)]
#[serde(default)]  // Redundant - skip implies default
pub selection_anchor: Option<usize>,
```

### 5.3 Inconsistent Error Handling

Mix of `Result<T, String>`, `Result<T, FrameError>`, and `.unwrap()`

**Fix**: Create unified `PlayaError` enum

### 5.4 Excessive Clone Calls (176 instances)

Audit for unnecessary clones, use `&` or `Cow<>` where possible.
Avoid of copying memory.

---

## 6. DATAFLOW DIAGRAM

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
└─────────┘              └──────┬───────┘               └──────┬──────┘
                                │                              │
                                │ poll()                       │ load frames
                                ▼                              ▼
┌───────────────────────────────────────────────────────────────────┐
│                         PlayaApp.update()                          │
│                                                                    │
│  Project ◄─────── modify_comp() ◄─────── UI Widgets                │
│     │                    │                                         │
│     │ dirty flag         │ AttrsChangedEvent                       │
│     ▼                    ▼                                         │
│  GlobalFrameCache ◄── clear_comp(dehydrate=true)                   │
│     │                                                              │
│     │ get()/insert()                                               │
│     ▼                                                              │
│  Frame ─────────────────▶ ViewportRenderer (OpenGL)                │
└───────────────────────────────────────────────────────────────────┘

PROPOSED (with traits.rs):
┌─────────────┐
│   entities  │◀──── defines traits: FrameCache, WorkerPool
│   (domain)  │
└──────┬──────┘
       │ uses traits (not concrete types)
       ▼
┌─────────────┐
│    core     │──── implements traits: GlobalFrameCache, Workers
│ (infra)     │
└─────────────┘
```

---

## 7. ACTION PLAN

### Phase 1: Complete traits.rs Integration (PRIORITY)
- [ ] Add `pub mod traits;` to `src/entities/mod.rs`
- [ ] Implement `FrameCache` for `GlobalFrameCache`
- [ ] Implement `WorkerPool` for `Workers`
- [ ] Remove duplicate `CacheStrategy` from `global_cache.rs`
- [ ] Update `node.rs` ComputeContext to use trait objects

### Phase 2: Code Quality (Week 1-2)
- [ ] Create `defaults.rs` for magic numbers
- [ ] Create `theme.rs` for colors
- [ ] Route Attrs setters through `set()` for schema check
- [ ] Remove redundant `#[serde(default)]`

### Phase 3: Architecture (Week 2-4)
- [ ] Move thread_local from comp_node to ComputeContext
- [ ] Extract FrameProvider from Player
- [ ] Split PlayaApp into smaller components
- [ ] Decouple Project from EventBus

### Phase 4: Cleanup (Backlog)
- [ ] Audit and resolve #[allow(dead_code)]
- [ ] Create unified PlayaError enum
- [ ] Document GPU compositor status in prefs UI
- [ ] Consider generic blend implementation

---

## 8. METRICS SUMMARY

| Metric | Value | Notes |
|--------|-------|-------|
| Source files | 73+ | |
| Lines of code | ~15,000+ | |
| TODO in code | 4 | gpu_compositor.rs (2), compositor.rs (1), lib.rs (1) |
| #[allow(dead_code)] | 10 | Most justified |
| panic!() in tests | 4 | All in #[test] - OK |
| panic!() in prod | 0 | False positive resolved |
| Clone calls | 176 | Needs audit |
| Magic numbers | 50+ | Create defaults.rs |
| Color literals | 50+ | Create theme.rs |
| Circular deps | 1 | entities <-> core (fix with traits.rs) |

---

## 9. KEY ENTITIES (Memory MCP)

For context recovery after compaction:

- **Project** - top container, owns `media: HashMap<Uuid, Arc<NodeKind>>`
- **NodeKind** - enum: File/Comp/Camera/Text (uses enum_dispatch)
- **Node** trait - compute(), attrs(), is_dirty(), preload()
- **ComputeContext** - cache ref, media ref, workers ref, epoch
- **Attrs** - key-value store with schema, dirty tracking
- **GlobalFrameCache** - `HashMap<Uuid, HashMap<i32, Frame>>` with LRU
- **traits.rs (WIP)** - `FrameCache`, `WorkerPool` traits for DI

**Data flow**:
User edit → modify_comp() → Attrs.set() → dirty flag → AttrsChangedEvent → cache.clear_comp(dehydrate) → workers recompute → cache.insert() → viewport refresh

---

**Report awaiting approval. Ready to begin traits.rs integration on confirmation.**
