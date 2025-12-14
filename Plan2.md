# Plan2: Research & Implementation Plan for task1.md

## Task 1: Timeline Ruler Numbers Density

### Problem
At certain zoom levels, frame numbers on the timeline overlap and become unreadable.

### Current Implementation
`timeline_helpers.rs:160` - `draw_frame_ruler()`:
```rust
let frame_step = if effective_ppf > 10.0 { 1 }
    else if effective_ppf > 2.0 { 5 }
    else if effective_ppf > 0.5 { 10 }
    else { 50 };

let label_step = if effective_ppf > 50.0 { 10 }
    else if effective_ppf > 20.0 { 5 }
    else { (frame_step * 2).max(frame_step) };
```

### Solution
Improve thresholds and add more granular steps:
1. Calculate text width for current font (9pt monospace, ~6px per digit)
2. Base `label_step` on actual text width vs available space
3. Add intermediate zoom levels (1, 2, 5, 10, 25, 50, 100, 250, 500...)
4. Consider showing timecode (MM:SS:FF) at very low zooms instead of raw frames

### Implementation
```rust
// Calculate min spacing between labels (text width + padding)
let digit_width = 6.0; // ~6px per digit at 9pt monospace
let max_label_digits = (total_frames as f32).log10().ceil() as usize + 1;
let min_label_spacing = (max_label_digits as f32 * digit_width) + 20.0; // +20px padding

// Determine label_step based on actual pixel density
let frames_per_label_space = min_label_spacing / effective_ppf;
let label_step = nice_step(frames_per_label_space.ceil() as i32);

fn nice_step(min: i32) -> i32 {
    const STEPS: [i32; 12] = [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000];
    STEPS.iter().find(|&&s| s >= min).copied().unwrap_or(min)
}
```

---

## Task 2: Node Not Created on Layer Drop

### Problem
When dropping a layer in timeline, [OUT] node gets a new input but no node is created.

### Current Flow
1. `timeline_ui.rs:1089` - dispatches `AddLayerEvent`
2. `main_events.rs:559` - handles `AddLayerEvent`:
   - Calls `comp.add_child_layer()` which adds layer to `layers` vec
   - Does NOT call `node_editor_state.mark_dirty()`

### Root Cause
Node editor `needs_rebuild` flag is not set when layers are added/removed.

### Solution
After any layer modification, mark node editor as dirty:
```rust
// In main_events.rs after AddLayerEvent handling:
node_editor_state.mark_dirty();
```

### Affected Events to Fix
- `AddLayerEvent` - add layer
- `RemoveLayerEvent` - remove layer  
- `RemoveSelectedLayerEvent` - remove selected layers
- `ReorderLayerEvent` - reorder layers
- `PasteLayerEvent` - paste layers
- `DuplicateLayerEvent` - duplicate layers

---

## Task 3: Solo Checkbox for Layers

### Problem
Need "Solo" feature like After Effects - when any layer has Solo enabled, only Solo layers render.

### Implementation Plan

#### 1. Add Solo Attribute
`keys.rs`:
```rust
pub const A_SOLO: &str = "solo";
```

`comp_node.rs` Layer:
```rust
pub fn is_solo(&self) -> bool {
    self.attrs.get_bool(A_SOLO).unwrap_or(false)
}
```

#### 2. Check Solo in Compose
`comp_node.rs` `compose_internal()`:
```rust
// Before layer loop - check if ANY layer has solo
let has_solo = self.layers.iter().any(|l| l.is_solo());

// In layer loop:
if has_solo && !layer.is_solo() {
    continue; // Skip non-solo when solo mode active
}
```

#### 3. Add UI Checkbox
`timeline_ui.rs` - add "S" button next to visibility eye icon in layer row.

#### 4. Add Event
```rust
pub struct LayerSoloChangedEvent {
    pub comp_uuid: Uuid,
    pub layer_uuid: Uuid,
    pub solo: bool,
}
```

---

## Task 4: Cache & Preload Behavior

### Current State

#### Preload Mechanism Exists
- `file_node.rs:238` - `preload()` with spiral/forward strategies
- `comp_node.rs:954` - `preload()` delegates to child sources
- `comp_node.rs:1013` - `signal_preload()` public API

#### When Preload is Triggered
- `main.rs:312-314` - on `set_active_comp()`: `comp.signal_preload(...)`
- `main.rs:571` - marks comp dirty (indirect trigger)

#### Problem
When comp is dirty (layer moved, etc.), only current frame is recomposed. Nearby frames are NOT queued for preload.

### Solution

#### 1. Add Preload Radius Parameter
```rust
pub fn signal_preload(
    &mut self,
    workers: &Workers,
    project: &Project,
    center: Option<i32>,
    radius: Option<i32>,  // NEW: how many frames around center
) { ... }
```

#### 2. Trigger Preload on Dirty
In `main.rs` dirty handling loop:
```rust
if comp.is_dirty() {
    // Current behavior: recompose current frame
    // NEW: also queue nearby frames for background preload
    comp.signal_preload(&self.workers, &self.project, Some(frame), Some(PRELOAD_RADIUS));
}
```

#### 3. Configurable Radius
Add to config:
```rust
preload_radius: i32 = 10,  // frames before/after current
```

---

## Task 5: Frame Size Bug in Nested Comps

### Problem
When nesting same FileNode in multiple CompNodes, frame size unexpectedly changes from PAL D1 to FullHD on certain frames.

### Analysis - Dimension Resolution

#### Where Dimensions Come From
1. `comp_node.rs:239` - `dim()` reads A_WIDTH/A_HEIGHT attrs (comp's own size)
2. `compose_internal():820-823` - output dim taken from "earliest" layer's frame:
```rust
let dim = earliest
    .and_then(|(_, idx)| source_frames.get(idx))
    .map(|(f, _, _)| (f.width().max(1), f.height().max(1)))
    .unwrap_or_else(|| self.dim());
```

### Potential Causes

#### Cause A: Earliest Layer Changes
- `earliest` is the layer with smallest `start()` value among visible layers
- If layer visibility/timing changes, a different layer becomes "earliest"
- Different layers may have different source dimensions

#### Cause B: Cache Returns Wrong Frame
- Frame from different comp/source could be returned due to UUID collision
- Check `global_cache.rs` key structure

#### Cause C: Placeholder Frame Dimension Mismatch  
- `placeholder_frame()` uses `self.dim()` which is comp's declared size
- If comp has no layers visible at frame N, placeholder dimensions are used
- But when layers ARE visible, source frame dimensions are used

### Recommended Fix

#### Option 1: Always Use Comp Dimensions (Recommended)
```rust
// In compose_internal():
let dim = self.dim();  // Always use comp's declared dimensions
// Then scale/fit source frames to this dimension
```

This matches After Effects behavior - comp has fixed dimensions.

#### Option 2: Debug Logging
Add detailed logging to identify exactly when/why dimensions change:
```rust
log::debug!(
    "compose frame={}: earliest_layer={:?}, dim={:?}, comp_dim={:?}",
    frame_idx, earliest, dim, self.dim()
);
```

---

## Implementation Order

1. **Task 2** (Node not created) - Quick fix, 1 line per event
2. **Task 1** (Ruler density) - Medium, isolated change
3. **Task 3** (Solo) - Medium, new feature
4. **Task 5** (Dimension bug) - Investigation + fix
5. **Task 4** (Cache/preload) - Larger refactor

## Estimated Effort

| Task | Complexity | Files | Est. Time |
|------|------------|-------|-----------|
| 1. Ruler | Medium | 1 | 30 min |
| 2. Node update | Easy | 1 | 10 min |
| 3. Solo | Medium | 4 | 1 hour |
| 4. Preload | High | 3-4 | 2 hours |
| 5. Dimensions | Medium | 1-2 | 1 hour |
