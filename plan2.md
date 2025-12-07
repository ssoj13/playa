# Timeline Bug Hunt Report - December 6, 2025

## Executive Summary

Comprehensive analysis of the Timeline window in Playa project.
**Issues found: 8** (3 from task.md + 5 additional discovered)

| Issue | Severity | Status |
|-------|----------|--------|
| #1 Column alignment | MEDIUM | Analysis complete |
| #2 Remove reorder box | LOW | Ready to fix |
| #3 Layers disappear | HIGH | Root cause found |
| #4 Viewport dirty refresh | MEDIUM | Already implemented |
| #5-8 Additional issues | MEDIUM-LOW | Documented |

---

## Data Flow: Timeline Rendering

```
┌─────────────────────────────────────────────────────────────────┐
│                        render_timeline()                         │
│                    (timeline_ui.rs:139-1209)                     │
└────────────────────────────┬────────────────────────────────────┘
                             │
          ┌──────────────────┴──────────────────┐
          │                                     │
          ▼                                     ▼
┌─────────────────────┐               ┌─────────────────────┐
│   render_outline()  │               │   render_canvas()   │
│   (lines 162-402)   │               │   (lines 405-1209)  │
│                     │               │                     │
│ - ScrollArea + DnD  │               │ - ScrollArea        │
│ - 6 fixed columns:  │               │ - Frame ruler       │
│   * drag 20px       │               │ - Layer bars        │
│   * vis  20px       │               │ - Interaction pass  │
│   * name 150px      │               │                     │
│   * opacity 60px    │               │                     │
│   * blend 90px      │               │                     │
│   * speed 50px      │               │                     │
└─────────────────────┘               └─────────────────────┘
          │                                     │
          │                                     ▼
          │                           ┌─────────────────────┐
          │                           │ compute_layer_rows()│
          │                           │ (comp.rs:1666-1699) │
          │                           │                     │
          │                           │ Auto-layout: finds  │
          │                           │ first non-overlapping│
          │                           │ row for each layer  │
          └───────────────────────────┴─────────────────────┘
```

---

## Issue #1: Column Alignment

### Current State
- **File**: `timeline_ui.rs` lines 217-320
- Columns already have fixed widths:
  - Drag handle: 20px (line 219)
  - Visibility checkbox: 20px (line 240)
  - Name: 150px (line 255)
  - Opacity slider: 60px (line 264)
  - Blend mode: 90px (line 283)
  - Speed: 50px (line 304)

### Problem
- No visual column separators
- `item_spacing = 6.0` (line 214) creates gaps but no lines
- Unlike Attribute Editor which has vertical separators

### Proposed Fix
Add thin vertical separator lines between columns using `painter.vline()`:

```rust
// After each column, draw separator
let sep_x = row_ui.cursor().min.x;
painter.vline(sep_x, row_y..row_y + config.layer_height,
              Stroke::new(1.0, Color32::from_gray(50)));
```

**Effort**: LOW (1-2 hours)

---

## Issue #2: Remove Reorder Box

### Current State
- **File**: `timeline_ui.rs` lines 217-226
- Drag handle "≡" in outline (left panel)
- Also has drag'n'drop on canvas (right panel) via `egui_dnd`

### Problem
- Redundant: DnD works on canvas layer bars
- Takes 20px horizontal space
- Users expect drag on layer bars, not separate handle

### Proposed Fix
Remove lines 217-226 (drag handle allocation):

```rust
// REMOVE THIS BLOCK:
// Fixed-width drag handle (20px)
row_ui.allocate_ui_with_layout(
    egui::Vec2::new(20.0, config.layer_height),
    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    |ui| {
        handle.ui(ui, |ui| {
            ui.label("≡");
        });
    },
);
```

**Note**: Keep `egui_dnd` wrapper for DnD functionality, just remove visible handle.

**Effort**: LOW (30 minutes)

---

## Issue #3: Layers Disappear When Moved Outside Timeline (HIGH PRIORITY)

### Current State
- User reports: "layer disappears completely when moved left/right beyond timeline"
- Happens with 3+ layers, only affects upper layers

### Root Cause Analysis

**File**: `comp.rs` lines 1666-1699 (`compute_layer_rows`)

The auto-layout algorithm:
1. Iterates layers in `child_order` (top to bottom in UI = index 0, 1, 2...)
2. For each layer, finds first row without time overlap
3. Uses `full_bar_start()` and `full_bar_end()` for overlap detection

**The Bug**: When a layer is moved far left (negative `in`) or far right:
- Its time range no longer overlaps with other layers
- It gets assigned row 0 (first available)
- Other layers that WERE in row 0 now have a conflict
- Auto-layout recalculates and shifts layers

**Example**:
```
Before:                      After moving Layer 0 to frame -100:
Row 0: [Layer 0: 0-50]      Row 0: [Layer 0: -100 to -50]  (no conflict!)
Row 0: [Layer 1: 60-100]    Row 0: [Layer 1: 60-100]       (still row 0)
Row 0: [Layer 2: 110-150]   Row 0: [Layer 2: 110-150]      (still row 0)

Result: All 3 layers in row 0 - they all render at same Y position!
```

**Why they "disappear"**: Layers are drawn on top of each other at same Y coordinate. Only the last one is visible.

### Solution

Two options:

**Option A: Preserve layer order regardless of time position** (Recommended)
- Modify `compute_layer_rows()` to use display order, not time-based layout
- Each layer gets its sequential row: layer 0 → row 0, layer 1 → row 1, etc.
- Time overlap becomes irrelevant for row assignment

**Option B: Clamp layer position**
- Prevent `in` from going below 0 or beyond reasonable bounds
- Add validation in `MoveAndReorderLayerEvent` handler

**Recommended**: Option A - matches After Effects behavior where layer stacking order is explicit.

**File to modify**: `comp.rs:1666-1699`

```rust
// BEFORE: Auto-layout based on time overlap
pub fn compute_layer_rows(&self, child_order: &[usize]) -> HashMap<usize, usize> {
    // ... complex overlap detection ...
}

// AFTER: Simple sequential rows
pub fn compute_layer_rows(&self, child_order: &[usize]) -> HashMap<usize, usize> {
    child_order.iter()
        .enumerate()
        .map(|(row, &idx)| (idx, row))
        .collect()
}
```

**Effort**: MEDIUM (2-3 hours including testing)

---

## Issue #4: Viewport Dirty Refresh Mechanism

### Current State
**Already Implemented!** The mechanism exists and works:

1. **Dirty flags**: `attrs.rs` lines 72-73, 320-326
   - `AtomicBool dirty` in `Attrs` struct
   - Set on any `attrs.set()` call

2. **Event cascade**: `comp.rs` lines 1140-1150
   - `emit_attrs_changed()` emits `AttrsChangedEvent`
   - Called by `set_child_attr()`, `set_child_attrs()`, etc.

3. **Cache invalidation**: `main.rs` lines 323-340
   - Handles `AttrsChangedEvent`
   - Calls `cache.clear_comp(uuid)` and `invalidate_cascade()`

4. **Viewport update**: `main.rs` lines 1012-1030
   - Centralized dirty check in update loop
   - Sets `displayed_frame = None` to force re-render
   - Calls `enqueue_frame_loads_around_playhead(10)`

### Status
**No fix needed** - mechanism is complete. The recent SlideLayerEvent fix (using `set_child_attrs()` instead of direct `attrs.set()`) ensures proper cache invalidation.

---

## Additional Issues Found

### Issue #5: No bounds checking on pan_offset

**File**: `timeline_ui.rs` lines 765-777
```rust
let new_pan = initial_pan_offset - delta_frames;
state.pan_offset = new_pan;  // No clamping!
```

**Impact**: Can pan infinitely into negative territory, layers become invisible.

**Fix**: Add clamping to reasonable range (e.g., -1000 to comp duration + buffer).

**Effort**: LOW

---

### Issue #6: Outline/Canvas scroll sync

**File**: `timeline_ui.rs` lines 174-182
```rust
ui.add_space(20.0 + status_bar_height + 4.0 + 24.0);  // Hard-coded 24.0
```

**Impact**: If canvas layout changes, outline may desync vertically.

**Fix**: Calculate offset dynamically based on actual ruler height.

**Effort**: LOW

---

### Issue #7: geom_cache created every frame

**File**: `timeline_ui.rs` lines 581-582
```rust
let mut geom_cache: HashMap<usize, LayerGeom> = HashMap::with_capacity(child_order.len());
```

**Impact**: Minor performance - creates new HashMap each frame.

**Fix**: Store in `TimelineState` and reuse.

**Effort**: LOW

---

### Issue #8: Missing layer count limit

**File**: `compute_layer_rows()` has infinite loop potential
```rust
loop {
    // ... find row ...
    row += 1;  // No upper bound!
}
```

**Impact**: With many overlapping layers, could create thousands of rows.

**Fix**: Add max_rows limit (e.g., 100).

**Effort**: LOW

---

## Recommended Fix Order

| Priority | Issue | Effort | Impact |
|----------|-------|--------|--------|
| 1 | #3 Layer disappearing | 2-3h | HIGH - usability breaking |
| 2 | #5 Pan bounds | 30min | MEDIUM - usability |
| 3 | #2 Remove reorder box | 30min | LOW - cleanup |
| 4 | #1 Column separators | 1-2h | LOW - visual polish |
| 5 | #6-8 Minor issues | 1-2h | LOW |

**Total estimated effort**: 5-8 hours

---

## Files to Modify

| File | Changes |
|------|---------|
| `comp.rs` | Lines 1666-1699: Simplify `compute_layer_rows()` |
| `timeline_ui.rs` | Lines 217-226: Remove drag handle |
| `timeline_ui.rs` | Lines 765-777: Add pan_offset clamping |
| `timeline_ui.rs` | Add column separators after each control |

---

## Approval Required

This plan requires approval before implementation.

Awaiting confirmation to proceed with Issue #3 (layer disappearing) as highest priority.

---

*Report generated by Claude Code - December 6, 2025*
