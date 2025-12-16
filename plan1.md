# Playa Bug Hunt Report

**Date:** 2025-12-15
**Branch:** dev3
**Status:** Phase 1, 2 & 5 COMPLETED ✅ (build passes, clippy clean)

---

## Executive Summary

Comprehensive code audit of the Playa project (Rust-based professional image sequence player) revealed:

- **5 Critical bugs** requiring immediate attention (race conditions, memory ordering, integer overflow)
- **12 Dead/unused code items** that can be safely removed (~300 lines)
- **6 Code duplication opportunities** (~500-600 lines reducible)
- **8 Interface compatibility issues** (inconsistent patterns, missing delegation)
- **45+ TODO/FIXME markers** requiring review

---

## Table of Contents

1. [Critical Bugs](#1-critical-bugs)
2. [Dead/Unused Code](#2-deadunused-code)
3. [Code Duplication Opportunities](#3-code-duplication-opportunities)
4. [Interface Compatibility Issues](#4-interface-compatibility-issues)
5. [TODO/FIXME Markers](#5-todofixme-markers)
6. [Clippy Warnings](#6-clippy-warnings)
7. [Architecture Notes](#7-architecture-notes)
8. [Prioritized Action Items](#8-prioritized-action-items)

---

## 1. Critical Bugs

### 1.1 Race Condition in CacheManager::free_memory()

**File:** `src/core/cache_man.rs:130-142`
**Severity:** HIGH
**Type:** Thread safety

```rust
// PROBLEMATIC: Uses Ordering::Relaxed for failure which is insufficient
self.total_bytes.compare_exchange(
    current,
    current.saturating_sub(bytes),
    Ordering::AcqRel,
    Ordering::Relaxed,  // BUG: Should be Ordering::Acquire
)
```

**Fix:**
```rust
self.total_bytes.compare_exchange(
    current,
    current.saturating_sub(bytes),
    Ordering::AcqRel,
    Ordering::Acquire,  // Correct: ensures visibility of previous writes
)
```

---

### 1.2 Integer Overflow in Layer Duration Calculation

**File:** `src/entities/attrs.rs:456-457`
**Severity:** HIGH
**Type:** Arithmetic overflow

```rust
// PROBLEMATIC: Can overflow when converting back to i32
let effective_duration = ((end_frame - start_frame + 1) as f32 / speed) as i32;
let effective_trim_in = (trim_in as f32 / speed) as i32;
```

**Fix:**
```rust
// Use f64 for intermediate calculations and saturating conversion
let effective_duration = ((end_frame - start_frame + 1) as f64 / speed as f64)
    .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
let effective_trim_in = (trim_in as f64 / speed as f64)
    .clamp(i32::MIN as f64, i32::MAX as f64) as i32;
```

---

### 1.3 Memory Ordering Inconsistency in check_memory_limit()

**File:** `src/core/cache_man.rs:94-97`
**Severity:** MEDIUM
**Type:** Memory ordering

```rust
// PROBLEMATIC: Two separate Acquire loads don't guarantee atomicity
let current = self.total_bytes.load(Ordering::Acquire);
let limit = self.limit_bytes.load(Ordering::Acquire);
```

**Impact:** Worst case is one extra eviction iteration. Low priority but worth noting.

---

### 1.4 Unchecked Array Indexing in Compositor

**File:** `src/entities/compositor.rs:339-342`
**Severity:** MEDIUM
**Type:** Potential panic

```rust
// PROBLEMATIC: Blend macro doesn't check bounds before slicing
macro_rules! blend_pixel {
    ($base:expr, $layer:expr, $mode:expr) => {{
        let (br, bg, bb, ba) = ($base[0], $base[1], $base[2], $base[3]);
        // ...
    }};
}
```

**Fix:** Add bounds check or use `get()` with unwrap_or:
```rust
let br = *$base.get(0).unwrap_or(&0.0);
let bg = *$base.get(1).unwrap_or(&0.0);
// ...
```

---

### 1.5 Epoch Handling Wrap-around Issue

**File:** `src/core/workers.rs:178`
**Severity:** LOW
**Type:** Edge case bug

```rust
// PROBLEMATIC: No handling for epoch counter wrap-around after u64::MAX
if current_epoch.load(Ordering::Relaxed) == epoch {
    f();
}
```

**Impact:** After running for ~584 million years at 1000 epochs/second, requests would start matching incorrectly. Theoretical but worth documenting.

---

## 2. Dead/Unused Code

### 2.1 Unused Trait: HelpProvider

**File:** `src/help.rs:22-28`
**Lines:** ~15

```rust
pub trait HelpProvider {
    fn help_title(&self) -> &'static str;
    fn help_entries(&self) -> &'static [HelpEntry];
}
```

**Status:** Declared but never implemented by any type.
**Action:** DELETE or implement for widgets.

---

### 2.2 Unused Function: all_help_sections()

**File:** `src/help.rs:159-168`
**Lines:** ~10

```rust
pub fn all_help_sections() -> Vec<(&'static str, &'static [HelpEntry])> { ... }
```

**Status:** Declared as `pub` but never called.
**Action:** DELETE.

---

### 2.3 Unused Struct: FocusTracker

**File:** `src/dialogs/prefs/prefs_events.rs:40-108`
**Lines:** ~70

```rust
pub struct FocusTracker { ... }
impl FocusTracker { ... }
```

**Status:** Struct and methods declared but never instantiated. Logic is duplicated manually in `main.rs::determine_focused_window()`.
**Action:** DELETE (functionality exists elsewhere).

---

### 2.4 Unused Events: AttributesSplitChangedEvent, AttributesFocusChangedEvent

**File:** `src/widgets/ae/ae_events.rs:8,12`
**Lines:** ~10

```rust
pub struct AttributesSplitChangedEvent(pub f32);
pub struct AttributesFocusChangedEvent(pub bool);
```

**Status:** Events declared but never emitted or handled.
**Action:** DELETE.

---

### 2.5 Orphaned Binaries (Not Registered)

**Directory:** `src/bin/`
**Files:** 6 files, ~600 lines total

| File | Purpose |
|------|---------|
| `attributes.rs` | Standalone AE test app |
| `encoder.rs` | Standalone encoder test app |
| `prefs.rs` | Standalone preferences test app |
| `project.rs` | Standalone project test app |
| `timeline.rs` | Standalone timeline test app |
| `viewport.rs` | Standalone viewport test app |

**Status:** `autobins = false` in Cargo.toml, only main.rs registered.
**Action:** DELETE

---

### 2.6 Dead Code with #[allow(dead_code)]

**File:** `src/utils.rs:16,48-53`
**Lines:** ~10

```rust
#[allow(dead_code)]
pub const IMAGE_EXTS: &[&str] = ...;

#[allow(dead_code)]
pub fn is_image(path: &Path) -> bool { ... }
```

**Action:** DELETE or use.

---

### 2.7 Test-Only Dead Field

**File:** `src/core/event_bus.rs:311`

```rust
struct OtherEvent { msg: String }  // field never read
```

**Action:** Add `#[allow(dead_code)]` or use field in test.

---

### 2.8 Commented-Out Code

| File | Line | Code |
|------|------|------|
| `main.rs` | 344-345 | Event logging |
| `dialogs/encode/encode.rs` | 1481 | File removal |

**Action:** DELETE commented code.

---

## 3. Code Duplication Opportunities

### 3.1 NodeKind Delegation Boilerplate (~150 lines)

**Location:** `src/entities/node_kind.rs`

Every method on `NodeKind` repeats the same match pattern:

```rust
pub fn some_method(&self) -> ReturnType {
    match self {
        NodeKind::File(n) => n.some_method(),
        NodeKind::Comp(n) => n.some_method(),
        NodeKind::Camera(n) => n.some_method(),
        NodeKind::Text(n) => n.some_method(),
    }
}
```

**Solution:** Use `enum_dispatch` crate:
```rust
#[enum_dispatch(Node)]
pub enum NodeKind {
    File(FileNode),
    Comp(CompNode),
    Camera(CameraNode),
    Text(TextNode),
}
```

---

### 3.2 Selection Logic Duplication (~80 lines)

**Locations:**
- `src/widgets/project/project_widget.rs` - media list selection
- `src/widgets/node_editor/node_editor.rs` - node selection
- `src/widgets/ae/ae_widget.rs` - attribute selection

All implement similar patterns for single/multi/range selection.

**Solution:** Extract generic `SelectionManager<T>` struct.

---

### 3.3 Widget Actions/Events Structures (~50 lines)

**Pattern:** Each widget has separate `*Actions` and `*Events` structs with similar fields.

**Solution:** Create generic `WidgetState<A, E>` wrapper.

---

### 3.4 Node Trait Default Implementations (~100 lines)

**Location:** Various node implementations

Many Node trait methods could have default implementations:

```rust
trait Node {
    fn label(&self) -> &str {
        self.attrs().get_str("label").unwrap_or("unnamed")
    }
    // ...
}
```

---

### 3.5 work_area Calculations (~30 lines)

**Locations:** `comp_node.rs`, `file_node.rs`, `attrs.rs`

Same calculation repeated:
```rust
let start = attrs.get_i32("start_frame").unwrap_or(1);
let end = attrs.get_i32("end_frame").unwrap_or(1);
start..=end
```

**Solution:** Add `Attrs::work_area() -> RangeInclusive<i32>` method.

---

### 3.6 placeholder_frame() Duplication (~20 lines)

**Locations:** `file_node.rs`, `comp_node.rs`

Same placeholder creation code.

**Solution:** Move to `Frame::placeholder()` factory method.

---

## 4. Interface Compatibility Issues

### 4.1 NodeKind::preload() Doesn't Delegate

**File:** `src/entities/node_kind.rs`

```rust
// preload() is NOT delegated to inner node types
pub fn preload(&self) -> Result<(), FrameError> {
    // Implementation doesn't call inner preload
}
```

**Fix:** Add delegation or document why it's intentional.

---

### 4.2 Inconsistent uuid() Error Handling

| Node Type | Pattern | Risk |
|-----------|---------|------|
| CameraNode | `.expect("...")` | Can panic |
| TextNode | `.expect("...")` | Can panic |
| FileNode | `.unwrap_or_else(Uuid::nil)` | Safe |
| CompNode | `.unwrap_or_else(Uuid::nil)` | Safe |

**Fix:** Unify to safe pattern with `unwrap_or_else(Uuid::nil)`.

---

### 4.3 Inconsistent Attr Key Usage

Some code uses constants, some uses string literals:

```rust
// Constant (good)
attrs.get_i32(ATTR_START_FRAME)

// Literal (bad - typo prone)
attrs.get_i32("start_frame")
```

**Fix:** Always use constants from `attr_schemas.rs`.

---

### 4.4 Schema Lost After Deserialization

**File:** `src/entities/attrs.rs`

When `Attrs` is deserialized, the schema reference is lost. Must be re-attached manually.

**Fix:** Add `Attrs::attach_schema()` method and call after deserialization.

---

### 4.5 Event Emitter Lost After Deserialization

**File:** `src/entities/comp_node.rs`

`CompEventEmitter` is not serialized, becomes dummy after load.

**Fix:** Add `Node::attach_event_emitter()` method and call after deserialization.

---

### 4.6 GPU Compositor Transform Not Working

**File:** `src/entities/gpu_compositor.rs:849`

```rust
// TODO: Implement proper canvas-sized blending with transform
```

CPU compositor handles transforms, GPU version doesn't.

---

### 4.7 Edition 2024 in Cargo.toml

**File:** `Cargo.toml`

```toml
edition = "2024"  # Unstable, requires nightly
```

**Fix:** Leave it alone, we're using nightly.

---

### 4.8 Missing Dirty Tracking Consistency

`set_*` methods in `Attrs` don't always call `mark_dirty()`.

---

## 5. TODO/FIXME Markers

### High Priority (Core Functionality)

| File | Line | Comment |
|------|------|---------|
| `entities/gpu_compositor.rs` | 244 | `TODO: implement GPU texture caching` |
| `entities/gpu_compositor.rs` | 849 | `TODO: Implement proper canvas-sized blending` |
| `entities/compositor.rs` | 272 | `TODO for GPU compositing` |
| `core/player.rs` | ~150 | `FIXME: handle edge cases in frame stepping` |

### Medium Priority (Features)

| File | Line | Comment |
|------|------|---------|
| `widgets/timeline/timeline_widget.rs` | various | Multiple `TODO` for timeline features |
| `widgets/viewport/viewport_widget.rs` | various | `TODO` for viewport enhancements |
| `entities/comp_node.rs` | various | Layer management `TODO`s |

### Low Priority (Cleanup)

| File | Line | Comment |
|------|------|---------|
| `bin/viewport.rs` | 39 | `TODO: use settings in viewport standalone` |
| `dialogs/encode/encode.rs` | various | Encoding option `TODO`s |

---

## 6. Clippy Warnings

| File | Line | Warning | Fix |
|------|------|---------|-----|
| `dialogs/encode/mod.rs` | 1 | `module_inception` | Rename inner module |
| `dialogs/prefs/mod.rs` | 2 | `module_inception` | Rename inner module |
| `dialogs/encode/encode.rs` | 1242 | `items_after_test_module` | Move items before tests |
| `dialogs/encode/encode.rs` | 1365 | `redundant_locals` | Remove `let settings = settings;` |
| `entities/comp_node.rs` | 877 | `unused_enumerate_index` | Use `for layer in layers` |
| `entities/gpu_compositor.rs` | 584 | `too_many_arguments` (10/7) | Use config struct |

---

## 7. Architecture Notes

### Data Flow Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         main.rs                                  │
│  ┌─────────┐  ┌──────────┐  ┌─────────────┐  ┌───────────────┐ │
│  │ Workers │  │ EventBus │  │ GlobalCache │  │ CacheManager  │ │
│  │ (N cpu) │  │ pub/sub  │  │ Frame store │  │ Memory track  │ │
│  └────┬────┘  └────┬─────┘  └──────┬──────┘  └───────┬───────┘ │
└───────┼────────────┼───────────────┼─────────────────┼──────────┘
        │            │               │                 │
        ▼            ▼               ▼                 ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                       Project                                │
   │  ┌──────────┐  ┌────────────┐  ┌─────────────────────────┐ │
   │  │MediaPool │  │ CompNode[] │  │ Attrs (key-value store) │ │
   │  │ unified  │  │ with layers│  │ with schema validation  │ │
   │  └──────────┘  └────────────┘  └─────────────────────────┘ │
   └──────────────────────────────────────────────────────────────┘
                              │
                              ▼
   ┌──────────────────────────────────────────────────────────────┐
   │                        Widgets                                │
   │  ┌──────────┐  ┌──────────┐  ┌────────┐  ┌────────────────┐ │
   │  │ Viewport │  │ Timeline │  │   AE   │  │ NodeEditor     │ │
   │  │ display  │  │ playback │  │ attrs  │  │ graph view     │ │
   │  └──────────┘  └──────────┘  └────────┘  └────────────────┘ │
   └──────────────────────────────────────────────────────────────┘
```

### Key Patterns

1. **Frame Loading Pipeline:**
   - Request → Workers (epoch check) → Loader → Frame → GlobalCache
   - Epoch mechanism cancels stale requests during scrubbing

2. **Event System:**
   - EventBus with immediate callbacks + deferred queue
   - Widgets emit events, main.rs handles in `handle_events()`

3. **Compositing:**
   - CPU compositor (complete)
   - GPU compositor (partial - missing transform support)

---

## 8. Prioritized Action Items

### Phase 1: Critical Fixes (Required)

- [x] **1.1** Fix race condition in `cache_man.rs:130` ✅
- [x] **1.2** Fix integer overflow in `attrs.rs:456` ✅
- [x] **4.7** Change edition to "2021" in Cargo.toml - SKIPPED (using nightly)
- [x] **4.2** Unify uuid() error handling to safe pattern ✅

### Phase 2: Dead Code Removal (Recommended)

- [x] **2.1** Delete `HelpProvider` trait ✅
- [x] **2.2** Delete `all_help_sections()` function ✅
- [x] **2.3** Delete `FocusTracker` struct ✅
- [x] **2.4** Delete unused ae_events ✅
- [x] **2.6** Delete `IMAGE_EXTS`, `is_image()` ✅
- [x] **2.8** Delete commented-out code ✅

### Phase 3: Code Deduplication (Optional)

- [ ] **3.1** Add `enum_dispatch` for NodeKind (30 min)
- [ ] **3.2** Extract `SelectionManager<T>` (45 min)
- [x] **3.5** `Attrs::work_area()` already exists ✅
- [x] **3.6** Add `Frame::placeholder()` factory ✅

### Phase 4: Interface Consistency (Optional)

- [ ] **4.3** Replace literal attr keys with constants (30 min)
- [ ] **4.4** Add `Attrs::attach_schema()` (15 min)
- [ ] **4.5** Add `Node::attach_event_emitter()` (15 min)

### Phase 5: Cleanup (Low Priority)

- [x] **6.x** Fix clippy warnings (auto-fix + allows) ✅
- [x] **2.5** Delete orphaned binaries in src/bin/ ✅

---

## Files Referenced

| File | Issues |
|------|--------|
| `src/core/cache_man.rs` | 1.1, 1.3 |
| `src/core/workers.rs` | 1.5 |
| `src/entities/attrs.rs` | 1.2, 4.4 |
| `src/entities/compositor.rs` | 1.4 |
| `src/entities/node_kind.rs` | 3.1, 4.1 |
| `src/entities/comp_node.rs` | 4.5 |
| `src/entities/gpu_compositor.rs` | 4.6 |
| `src/help.rs` | 2.1, 2.2 |
| `src/dialogs/prefs/prefs_events.rs` | 2.3 |
| `src/widgets/ae/ae_events.rs` | 2.4 |
| `src/utils.rs` | 2.6 |
| `Cargo.toml` | 4.7 |

---

## Approval Request

This report identifies 5 critical bugs, 12 dead code items, and 6 deduplication opportunities.

**Recommended approach:** Execute Phase 1 (critical fixes) immediately, Phase 2 (dead code) next, then optionally Phases 3-5.

**Awaiting approval to proceed with implementation.**
