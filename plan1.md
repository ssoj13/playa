# Playa Bug Hunt Report - December 13, 2025

## Executive Summary

Complete code audit of Playa video compositor application. Found **23 issues** requiring attention:
- **4 CRITICAL** - Must fix immediately
- **5 HIGH** - Should fix before release
- **8 MEDIUM** - Improvements
- **6 LOW** - Minor/cosmetic

## Project State

- **Branch**: dev
- **Clean build**: Yes (149 clippy warnings, 0 errors)
- **Status**: Major refactoring from old Comp architecture to Node architecture is ~80% complete

---

## CRITICAL Issues

### C1: Duplicate Layer Type Definitions
**Files**: `src/entities/layer.rs` vs `src/entities/comp_node.rs`

Two different `Layer` structs exist:
- `layer.rs::Layer` - old: `{ instance_uuid, attrs }` with `source_uuid()` getter from attrs
- `comp_node.rs::Layer` - new: `{ uuid, source_uuid, attrs }` with direct field access

**Impact**: `layer.rs` is exported but NEVER used - `comp_node.rs::Layer` is actually used everywhere.

**Evidence**:
```rust
// src/entities/mod.rs line 24 - exported but unused
pub use comp_node::{CompNode, Layer as NodeLayer};
// src/entities/mod.rs line 28 - legacy export
pub use layer::{Layer, Track};
```

**Fix**: Delete `src/entities/layer.rs` entirely - it's 326 lines of dead code.

---

### C2: Incomplete todo.md Migration Checklist
**File**: `todo.md`

The Node Architecture Migration Plan shows many unchecked items:
- [ ] Change `media: HashMap<Uuid, NodeKind>`
- [ ] Update `get_comp()` -> `get_node()`
- [ ] Update `add_comp()` -> `add_node()`
- [ ] Remove old `CompIterator`
- [ ] Step 4: Update all Comp usages (many files)
- [ ] Serialization tests
- [ ] Unit tests for new Node types

**Impact**: Architecture is half-migrated, causing confusion.

**Fix**: Either complete migration or update todo.md to reflect what's actually done.

---

### C3: Missing _events.rs Files (from task.md)
**Files**: Missing for `ae`, `node_editor`, `project`

Current state:
- `timeline/timeline_events.rs` ✅
- `project/` - no events file
- `widgets/ae/` - no events file  
- `widgets/node_editor/` - no events file

**Impact**: Inconsistent architecture, events scattered in wrong places.

**Fix**: Create `*_events.rs` files for each widget module.

---

### C4: F/A Hotkeys Global Instead of Context-Aware (from task.md)
**Issue**: F and A keys work globally instead of per-panel.

Timeline needs: Fit All / Fit Selected by TIME
Node Editor needs: Fit All / Fit Selected by NODES (zoom to nodes)

**Evidence**: `task.md` says "мы пытались исправить это много раз, но оно не работает"

**Fix**: Implement context-aware hotkey routing per HotkeyWindow enum.

---

## HIGH Issues

### H1: Node Editor Not Using Project.active_comp
**Issue**: Node editor shows "0 nodes" until double-click on comp in project.

**Expected**: Should subscribe to `ProjectActiveChangedEvent` and sync automatically.

**Fix**: In `node_graph.rs`, subscribe to `ProjectActiveChangedEvent` and call `set_comp(uuid)`.

---

### H2: 149 Clippy Warnings
**Impact**: Code quality issues, potential bugs hidden.

Warning categories:
- `collapsible_if` - 8 occurrences
- `clone_on_copy` - 2 occurrences (main.rs:586,588)
- `redundant_locals` - 1 occurrence (encode.rs:1365)
- `module_inception` - 1 occurrence (dialogs/encode/mod.rs)
- Dead code warning for `OtherEvent.msg` field

**Fix**: Run `cargo clippy --fix --lib -p playa` to auto-fix 129 suggestions.

---

### H3: #[allow(dead_code)] Markers
**Files**: 13 occurrences across codebase

Found `#[allow(dead_code)]` in:
- `utils.rs` (2)
- `viewport.rs` (2)
- `frame.rs` (5)
- `gpu_compositor.rs` (1)
- `encode.rs` (1)
- `viewport.rs` bin (1)

**Impact**: Hiding potentially removable code, some may be bugs.

**Fix**: Review each `#[allow(dead_code)]` - remove code or add explanation comment.

---

### H4: Recursive Read Lock Risk in load_node_pos
**File**: `src/widgets/node_editor/node_graph.rs`

Previous session fixed this by adding `drop(media)` before Phase 2, but pattern is fragile.

**Risk**: Same-thread recursive RwLock reads can deadlock on some platforms.

**Fix**: Document this clearly, consider refactoring to single-lock design.

---

### H5: TODO Comments Requiring Action
**Files**: 4 TODOs found

1. `main.rs:300` - Preload radius hint not implemented
2. `gpu_compositor.rs:244` - GPU texture caching not implemented
3. `gpu_compositor.rs:813` - Canvas-sized blending not implemented
4. `bin/viewport.rs:39` - Settings not used in standalone

**Fix**: Implement or remove with explanation.

---

## MEDIUM Issues

### M1: Deprecated comp.rs Module
**File**: `src/entities/mod.rs` line 6

```rust
// pub mod comp;  // DEPRECATED: replaced by comp_node
```

Old comp.rs may still exist in filesystem - verify and delete.

---

### M2: Type Alias May Cause Confusion
**File**: `src/entities/mod.rs` line 23

```rust
pub type Comp = CompNode;
```

While backwards-compatible, this hides the actual type name and may confuse developers.

---

### M3: Attrs Dirty Tracking with Cell<bool>
**Issue**: `Cell<bool>` for dirty tracking has thread-safety limitations.

Previous sessions identified this as blocking background workers.

**Status**: May already be fixed (check if AtomicBool was added).

---

### M4: main.rs Clone on Copy Types
**File**: `main.rs:586,588`

```rust
self.focused_window = focused_window.clone();  // HotkeyWindow is Copy
```

**Fix**: Remove unnecessary `.clone()` calls.

---

### M5: Collapsible If Statements
**Files**: encode_ui.rs, input_handler.rs, main.rs

8 occurrences of nested if statements that can be collapsed.

**Fix**: Use `if let Some(x) = y && condition { }` syntax.

---

### M6: module_inception Warning
**File**: `src/dialogs/encode/mod.rs`

Module has same name as containing module.

**Fix**: Rename inner module or restructure.

---

### M7: Redundant Locals
**File**: `encode.rs:1365`

```rust
let settings = settings;  // Redundant
```

**Fix**: Remove redundant let binding.

---

### M8: Dead Field in Test
**File**: `event_bus.rs:311`

```rust
struct OtherEvent { msg: String }  // msg never read
```

**Fix**: Add `#[allow(dead_code)]` with comment or use field.

---

## LOW Issues

### L1-L6: Documentation and Cleanup

- L1: Update CHANGELOG.md with recent fixes
- L2: Clean up task.md after completion
- L3: Update todo.md to match actual state
- L4: Add module-level documentation for node_graph.rs
- L5: Remove commented-out code blocks
- L6: Standardize error handling (some use `?`, some use `unwrap`)

---

## Dataflow Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        PLAYA ARCHITECTURE                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────┐    ┌──────────┐    ┌─────────────┐                │
│  │ Project │───>│  Media   │───>│  NodeKind   │                │
│  │         │    │ HashMap  │    │ File | Comp │                │
│  └────┬────┘    └──────────┘    └──────┬──────┘                │
│       │                                 │                        │
│       │ events                          │ compute()              │
│       v                                 v                        │
│  ┌─────────┐    ┌──────────┐    ┌─────────────┐                │
│  │EventBus │───>│  Player  │───>│GlobalCache  │                │
│  │         │    │          │    │  (frames)   │                │
│  └────┬────┘    └──────────┘    └─────────────┘                │
│       │                                                          │
│       │ dispatch                                                 │
│       v                                                          │
│  ┌──────────────────────────────────────────────────┐          │
│  │                    WIDGETS                         │          │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌────────┐ │          │
│  │  │Timeline │ │ Viewport │ │ Project│ │NodeEdit│ │          │
│  │  └─────────┘ └──────────┘ └────────┘ └────────┘ │          │
│  └──────────────────────────────────────────────────┘          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Key Data Types

1. **NodeKind** - enum wrapping FileNode or CompNode
2. **CompNode** - composition with layers, attrs, selection
3. **Layer** (comp_node.rs) - instance with uuid, source_uuid, attrs
4. **Attrs** - HashMap with dirty tracking
5. **Frame** - pixel buffer with status (Loading/Loaded/Error)

### Event Flow

1. User action → Widget creates Event
2. Event → EventBus.send()
3. main.rs loop → EventBus.drain()
4. Event handler → Project.modify_comp() / modify_node()
5. Mutation → Attrs.mark_dirty()
6. Next frame → cache invalidation → recompute

---

## Recommended Fix Priority

### Phase 1: Critical (Today)
1. [ ] Delete `src/entities/layer.rs` (C1)
2. [ ] Fix F/A hotkeys context routing (C4)
3. [ ] Node editor subscribe to active_comp events (H1)

### Phase 2: High (This Week)
1. [ ] Run `cargo clippy --fix` (H2)
2. [ ] Review `#[allow(dead_code)]` markers (H3)
3. [ ] Complete todo.md checklist or update (C2)
4. [ ] Create missing `*_events.rs` files (C3)

### Phase 3: Medium (Next Week)
1. [ ] Fix collapsible if statements (M5)
2. [ ] Fix module_inception (M6)
3. [ ] Remove redundant code (M4, M7)

### Phase 4: Low (When Time Permits)
1. [ ] Documentation updates (L1-L6)
2. [ ] Code cleanup

---

## Files Modified in Recent Sessions

Based on memory graph, recent fixes include:
- `src/entities/comp_node.rs` - child_start/end, trim_layers, parent_to_local
- `src/widgets/timeline/timeline_ui.rs` - double-click source_uuid fix
- `src/widgets/node_editor/node_graph.rs` - deadlock fix, load_node_pos refactor
- `src/main.rs` - render_node_editor call with comp_uuid

---

## Conclusion

The codebase is in good working state but has accumulated technical debt from the incomplete Node architecture migration. The critical issues are:

1. **Dead code** (`layer.rs`) that should be deleted
2. **Inconsistent architecture** (half-migrated Node system)
3. **Missing event routing** for widgets

Estimated total fix time: **16-24 hours**

---

*Report generated: December 13, 2025*
*Awaiting approval to proceed with fixes*
