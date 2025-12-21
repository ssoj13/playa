# Plan 10: Trim Layers Outwards (Hold First/Last Frame)

## Problem
Currently layers can only be trimmed inward (shortened). We cannot extend layer beyond source length.

## Goal
Allow extending layer beyond source boundaries:
- Extend left (before source start) -> hold first frame
- Extend right (after source end) -> hold last frame

## Current State

### Trim values are clamped to 0
Multiple places use `.max(0)` preventing negative trim:
- `comp_node.rs:696` - trim_in adjustment
- `comp_node.rs:702` - trim_out adjustment  
- `comp_node.rs:901` - set_child_start
- `comp_node.rs:916` - set_child_end

### Frame calculation flow
```
parent_to_local(frame_idx) -> local_frame
source_frame = source_in + local_frame
source_node.compute(source_frame, ctx)
```

No clamping of source_frame to valid range.

## Implementation Plan

### Step 1: Allow negative trim values
Remove `.max(0)` constraints in:
- `comp_node.rs:696, 702` - trim layer adjustments
- `comp_node.rs:901, 916` - set_child_start/end

### Step 2: Clamp source_frame at render time
In `comp_node.rs` compose loop (~line 1042-1047):
```rust
let local_frame = layer.parent_to_local(frame_idx);
let source_in = source_node.attrs().get_i32(A_IN).unwrap_or(0);
let source_out = source_node.attrs().get_i32(A_OUT).unwrap_or(0);

// Clamp to source range (hold first/last frame)
let source_frame = (source_in + local_frame).clamp(source_in, source_out);
```

### Step 3: Update work_area calculation
In `Layer::work_area()` (line 191-198):
- Currently: `(start + trim_in_scaled, end - trim_out_scaled)`
- Allow negative trim_in -> play_start < layer.start
- Allow negative trim_out -> play_end > layer.end

### Step 4: Timeline UI
- Visual indication of extended regions (different color/pattern)
- Dragging beyond source bounds should work smoothly

### Step 5: Update layer_start/layer_end in attrs.rs
- `layer_start()` and `layer_end()` may need adjustment
- Remove `.max(1)` constraints if they exist

## Key Files
- `src/entities/comp_node.rs` - main logic
- `src/entities/attrs.rs` - layer_start/layer_end helpers
- `src/widgets/timeline/timeline_canvas.rs` - UI drag handling

## Testing
1. Create layer with short source (e.g. 10 frames)
2. Try to extend left beyond frame 0 -> should show frame 0
3. Try to extend right beyond frame 9 -> should show frame 9
4. Verify 3D/2D modes work correctly
5. Verify speed != 1.0 works correctly
