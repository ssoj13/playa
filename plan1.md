# Task 3: Code Quality Issues - Completion Report

## Summary

All HIGH, MEDIUM, and LOW priority code quality issues have been fixed, along with specific issues 13-15 and a new feature request.

## Completed Tasks

### HIGH Priority

#### HIGH-1: Cache LayerGeom between draw and interaction passes
**File**: `src/widgets/timeline/timeline_ui.rs`

Added `HashMap<usize, LayerGeom>` cache that stores computed geometry during draw pass and reuses it in interaction pass, eliminating duplicate calculations.

```rust
let mut geom_cache: std::collections::HashMap<usize, super::timeline::LayerGeom> =
    std::collections::HashMap::with_capacity(child_order.len());
// ... in draw pass:
geom_cache.insert(idx, geom);
// ... in interaction pass:
let Some(&geom) = geom_cache.get(&idx) else { continue };
```

#### HIGH-2: Replace .unwrap() on RwLock with .expect()
**Files**: Multiple files across codebase

Bulk replaced all `RwLock.read().unwrap()` and `RwLock.write().unwrap()` calls with `.expect("lock poisoned")` for better panic messages when locks are poisoned.

#### HIGH-3: Fix race condition in enqueue_frame
**Files**: `src/core/global_cache.rs`, `src/entities/frame.rs`, `src/entities/comp.rs`

Added atomic `get_or_insert` method to `GlobalFrameCache` that prevents duplicate frame loading:

```rust
pub fn get_or_insert(&self, comp_uuid: Uuid, frame_idx: i32, make_frame: impl FnOnce() -> Frame) -> (Frame, bool) {
    let mut cache = self.cache.lock().unwrap();
    if let Some(frames) = cache.get(&comp_uuid) {
        if let Some(existing) = frames.get(&frame_idx) {
            return (existing.clone(), false);
        }
    }
    let frame = make_frame();
    // ... insert and return
}
```

Added `Frame::new_composing()` for creating Loading placeholder frames.

### MEDIUM Priority

#### MED-4: Consolidate duplicate edge jump handlers
**File**: `src/main_events.rs`

Created `jump_to_edge(comp, forward)` helper function to eliminate code duplication between `JumpNextEdgeEvent` and `JumpPrevEdgeEvent` handlers.

#### MED-5: Consolidate duplicate FPS handlers
**File**: `src/main_events.rs`

Created `adjust_fps_base(player, project, increase)` helper function to eliminate code duplication between `IncreaseFpsBaseEvent` and `DecreaseFpsBaseEvent` handlers.

#### MED-6: Reuse row layout logic
**Files**: `src/entities/comp.rs`, `src/widgets/timeline/timeline_helpers.rs`

Added `Comp::compute_layer_rows(&self, child_order: &[usize]) -> HashMap<usize, usize>` method that implements the greedy row layout algorithm. Updated `compute_all_layer_rows` in helpers to delegate to this method.

#### MED-7: Fix lock contention in child loop
**File**: `src/entities/comp.rs`

Optimized child source lookup to acquire lock once and collect all needed data, instead of acquiring lock per-child:

```rust
// Before: lock acquired per child
// After: single lock, collect sources
let media = project.media.read().expect("media lock poisoned");
let child_sources: Vec<_> = self.children.iter()
    .filter_map(|(_, attrs)| {
        attrs.get_str("uuid")
            .and_then(|s| Uuid::parse_str(s).ok())
            .and_then(|uuid| media.get(&uuid).cloned())
    })
    .collect();
drop(media); // Release lock before processing
```

### LOW Priority (8-12)

All minor issues fixed:
- Removed unnecessary `.clone()` on Uuid (Copy type)
- Removed dead `_has_overlap` variable
- Fixed double references where single ref sufficed
- Changed `== false` to `!` (more idiomatic)
- Renamed `get_child_edges_near` to `get_child_edges` (removed unused `_current_frame` param)

### Specific Issues

#### ISSUE-13: Fix viewport timeslider clamping
**Files**: `src/widgets/viewport/viewport.rs`, `src/widgets/viewport/viewport_ui.rs`

Implemented simple linear interpolation (`fit` function) to map mouse X position directly to frame range:

```rust
fn fit(value: f32, old_min: f32, old_max: f32, new_min: f32, new_max: f32) -> f32 {
    if (old_max - old_min).abs() < f32::EPSILON { return new_min; }
    let t = (value - old_min) / (old_max - old_min);
    new_min + t * (new_max - new_min)
}
```

Updated `handle_scrubbing` to use `fit(mouse_x, image_left, image_right, trim_in, trim_out)`.

#### ISSUE-15: Fix layer disappearing when dragged past timeline edge
**File**: `src/widgets/timeline/timeline_ui.rs`

Changed `hover_pos()` to `latest_pos()` in drag handling code. `latest_pos()` tracks pointer position even when cursor moves outside the window, preventing the layer ghost preview from disappearing during drag.

```rust
// Before: hover_pos() returns None outside window
if let Some(current_pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
// After: latest_pos() tracks position even outside window
if let Some(current_pos) = ui.ctx().input(|i| i.pointer.latest_pos()) {
```

### New Feature

#### Double-click empty area in Project panel to Add Clip
**File**: `src/widgets/project/project_ui.rs`

Added double-click handling on the project panel background to open the same file dialog as the "Add Clip" button:

```rust
let panel_response = ui.interact(
    panel_rect,
    ui.id().with("project_panel"),
    egui::Sense::click(),  // Changed from hover() to click()
);
// ... at end of function:
if panel_response.double_clicked() {
    if let Some(paths) = create_image_dialog("Add Media Files").pick_files() {
        if !paths.is_empty() {
            actions.send(AddClipsEvent(paths));
        }
    }
}
```

#### ISSUE-14: Diagonal hatching for file comp bars
**Files**: `src/widgets/timeline/timeline.rs`, `src/widgets/timeline/timeline_ui.rs`, `src/ui.rs`

Implemented pre-generated texture approach for maximum GPU efficiency:

1. Created 16x16 diagonal pattern texture with semi-transparent white lines
2. Texture created once lazily, stored in `TimelineState.hatch_texture`
3. Pattern tiled over file comp bars using UV coordinates

```rust
// Create hatch pattern texture (16x16, diagonal lines)
fn create_hatch_texture(ctx: &egui::Context) -> egui::TextureHandle {
    const SIZE: usize = 16;
    const LINE_WIDTH: usize = 2;
    const SPACING: usize = 6;
    // ... generate diagonal pattern pixels
    ctx.load_texture("hatch_pattern", image, TextureOptions { wrap_mode: Repeat, .. })
}

// In drawing code - check if source is file comp
let is_source_file = attrs.get_str("uuid")
    .and_then(|s| Uuid::parse_str(s).ok())
    .and_then(|source_uuid| project.get_comp(source_uuid))
    .map(|source| source.is_file_mode())
    .unwrap_or(false);

if is_source_file {
    let hatch_id = state.get_hatch_texture(ui.ctx());
    painter.image(hatch_id, visible_bar_rect, uv, Color32::WHITE);
}
```

## All Tasks Completed

## Files Modified

1. `src/core/global_cache.rs` - atomic get_or_insert
2. `src/entities/frame.rs` - Frame::new_composing()
3. `src/entities/comp.rs` - compute_layer_rows, get_child_edges, enqueue_frame, lock optimization
4. `src/main_events.rs` - consolidated handlers
5. `src/widgets/timeline/timeline.rs` - hatch_texture field, create_hatch_texture()
6. `src/widgets/timeline/timeline_ui.rs` - LayerGeom caching, latest_pos() fix, hatch pattern drawing
7. `src/widgets/timeline/timeline_helpers.rs` - delegate to Comp::compute_layer_rows
8. `src/widgets/viewport/viewport.rs` - fit() function, simplified scrubbing
9. `src/widgets/viewport/viewport_ui.rs` - pass play_end to handle_scrubbing
10. `src/widgets/project/project_ui.rs` - double-click to add clip
11. `src/ui.rs` - pass project to render_canvas
