# Plan 5: Task4.md Implementation Report

## Completed Tasks

### Task 1: Timeline Name Column Alignment
**Status:** COMPLETED

**Problem:** ComboBox for blend mode wasn't wrapped in `allocate_ui_with_layout`, causing column misalignment when layer names had different lengths.

**Solution:** Wrapped the ComboBox in a fixed-width container (90px) matching other columns.

**File changed:** `src/widgets/timeline/timeline_ui.rs:281-297`

### Task 2: Click Below Layers Clears Selection
**Status:** COMPLETED (from previous session)

### Task 3: Layer Stroke Width 1px
**Status:** COMPLETED (from previous session)

### Task 4: Trim Slide Mode
**Status:** COMPLETED

**Problem:** Need slide tool for layers - when dragging in trim zones (grey areas between full bar and visible bar), move in/out together while compensating trims to keep visible content in the same visual position.

**Solution Architecture:**

1. **GlobalDragState::SlidingLayer** (`timeline.rs:184-193`)
   ```rust
   SlidingLayer {
       layer_idx: usize,
       initial_in: i32,
       initial_out: i32,
       initial_trim_in: i32,
       initial_trim_out: i32,
       speed: f32,
       drag_start_x: f32,
   }
   ```

2. **LayerTool::Slide** (`timeline_helpers.rs:12-13`)
   - Added new tool variant with `ResizeColumn` cursor

3. **detect_layer_tool_with_geom()** (`timeline_helpers.rs:96-133`)
   - New geometry-aware tool detection function
   - Detects Slide when clicking in trim zones (areas between full_bar_rect and visible_bar_rect)
   - Falls back to standard Move/AdjustPlayStart/AdjustPlayEnd for visible bar

4. **SlideLayerEvent** (`comp_events.rs:201-216`)
   ```rust
   pub struct SlideLayerEvent {
       pub comp_uuid: Uuid,
       pub layer_idx: usize,
       pub new_in: i32,
       pub new_out: i32,
       pub new_trim_in: i32,
       pub new_trim_out: i32,
   }
   ```

5. **Drag handling** (`timeline_ui.rs:934-989`)
   - Visual feedback: orange ghost bar showing new position
   - Slide formula: `trim_delta = delta_frames * speed`
   - `new_trim_in = initial_trim_in - trim_delta`
   - `new_trim_out = initial_trim_out - trim_delta`

6. **Event handler** (`main_events.rs:581-596`)
   - Updates all four attributes atomically: in, out, trim_in, trim_out

**Mathematical Proof:**
- `layer_start = in + trim_in/speed`
- `layer_end = in + ((out-in) + trim_out)/speed`

When sliding by `delta`:
- `new_in = in + delta`, `new_out = out + delta`
- `new_trim_in = trim_in - delta*speed`, `new_trim_out = trim_out - delta*speed`

Verification:
- `new_layer_start = (in+delta) + (trim_in - delta*speed)/speed = in + trim_in/speed = layer_start` (unchanged)
- `new_layer_end = (in+delta) + (((out+delta)-(in+delta)) + (trim_out - delta*speed))/speed`
  `= (in+delta) + ((out-in) + trim_out - delta*speed)/speed`
  `= in + delta + (out-in+trim_out)/speed - delta = layer_end` (unchanged)

## Files Modified

| File | Changes |
|------|---------|
| `src/widgets/timeline/timeline.rs` | Added `GlobalDragState::SlidingLayer` |
| `src/widgets/timeline/timeline_helpers.rs` | Added `LayerTool::Slide`, `detect_layer_tool_with_geom()` |
| `src/widgets/timeline/timeline_ui.rs` | Updated tool detection, added SlidingLayer drag handling, fixed ComboBox alignment |
| `src/entities/comp_events.rs` | Added `SlideLayerEvent` |
| `src/main_events.rs` | Added `SlideLayerEvent` handler |

## Testing Checklist

- [ ] Click on trim zone (grey area) activates slide cursor
- [ ] Dragging in trim zone shows orange ghost bar
- [ ] Layer in/out change but visible content stays in same position
- [ ] Works with different speed values
- [ ] Columns in outline panel are properly aligned
- [ ] Other drag tools still work (Move, TrimLeft, TrimRight)

## Build Status
```
cargo build - SUCCESS
```
