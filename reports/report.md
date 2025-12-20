# Coordinate System Analysis & Fix

Date: 2025-12-19

---

## PROBLEM

We have a mess of coordinate systems and the quarter-shift bug persists.

---

## WHAT USER WANTS (from task.md)

### Two coordinate systems:

1. **Comp space**: origin bottom-left, +X right, +Y up
   - Used for final render output
   - Matches standard image coordinates (with Y flip)

2. **Frame space**: origin CENTER, extends in all directions
   - Negative values: left and down
   - Positive values: right and up
   - `position = (0,0,0)` = layer is CENTERED
   - `pivot = (0,0,0)` = pivot at layer CENTER
   - Gizmo appears at pivot point

---

## CURRENT STATE (broken)

### Too many coordinate spaces:
- Image space (top-left, Y-down) - standard images
- Comp space (bottom-left, Y-up) - internal
- Viewport space (center, Y-up) - OpenGL
- Object space (layer center, Y-up) - transforms
- Frame space - NOT IMPLEMENTED but desired!

### Current layer position semantic:
- `position = (comp_w/2, comp_h/2)` = centered
- Default set in `add_child_layer()`
- This is COMP space, not FRAME space

### The bug:
Transform code mixes coordinate systems:
```rust
// In transform_frame_with_camera:
let comp_pt = space::image_to_comp(dst_pt, comp_size);  // 0..w, 0..h
let obj_pt3 = inv.transform_point3(comp_pt3);           // expects centered coords?
```

The inverse transform subtracts position, but position is in comp space (360, 288 for center),
while the math expects frame space (0, 0 for center).

---

## SOLUTION: Simplify to Frame Space

### New semantic:
- **Position is in FRAME SPACE** (center origin)
- `position = (0, 0, 0)` = layer centered in comp
- `position = (100, 0, 0)` = layer 100px to the right of center
- `position = (-100, -50, 0)` = layer 100px left, 50px down from center

### New default:
```rust
// In add_child_layer():
layer.attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));  // centered!
```

### Transform pipeline (simplified):
```
Screen pixel (image space, top-left origin, Y-down)
    |
    v  image_to_frame(): flip Y, center origin
Frame space (center origin, Y-up)
    |
    v  inverse model transform (position, rotation, scale, pivot)
Object space (layer center)
    |
    v  object_to_src(): convert to source image coords
Source pixel (for sampling)
```

### New helper functions needed:
```rust
/// Image space -> Frame space (centered, Y-up)
fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x - w * 0.5, h * 0.5 - p.y)
}

/// Frame space -> Image space
fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x + w * 0.5, h * 0.5 - p.y)
}
```

### Inverse transform (simplified):
Forward: `frame = position + R * S * (object - pivot)`
Inverse: `object = pivot + S^(-1) * R^(-1) * (frame - position)`

With position in frame space, `position = (0,0)` means no translation needed for centered layer.

---

## MIGRATION

### Files to change:

1. **space.rs**: Add `image_to_frame()`, `frame_to_image()`
2. **transform.rs**: Use `image_to_frame()` instead of `image_to_comp()`
3. **comp_node.rs**: Change default position to `(0, 0, 0)`
4. **gizmo.rs**: Use frame space for gizmo position
5. **viewport tools**: Use frame space for drag deltas

### Gizmo position:
```rust
// OLD (comp space):
let gizmo_pos = space::comp_to_viewport(layer_position, comp_size);

// NEW (frame space - position IS viewport position):
let gizmo_pos = layer_position;  // already centered!
```

---

## WHY THIS IS SIMPLER

1. **Position (0,0,0) = centered** - intuitive
2. **Frame space = Viewport space** - no conversion needed for gizmo
3. **One less coordinate system** - Frame replaces Comp for positions
4. **Transform math is cleaner** - position directly adds to frame coords

---

## STEP-BY-STEP FIX

### Step 1: Add frame space helpers to space.rs
```rust
pub fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2
pub fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2
```

### Step 2: Update transform.rs
Replace `image_to_comp()` with `image_to_frame()` in transform_frame_with_camera

### Step 3: Update comp_node.rs
Default position = `[0.0, 0.0, 0.0]` (already centered)

### Step 4: Update gizmo.rs
Remove `comp_to_viewport()` conversion - position is already in frame/viewport space

### Step 5: Test
- New layer should appear centered
- Moving layer should work correctly
- Rotation/scale should work around pivot

---

## SUMMARY

**Root cause**: Position stored in comp space (bottom-left origin) but transform expects frame space (center origin).

**Fix**: Make position use frame space. `(0,0,0)` = centered. This matches viewport/gizmo coords naturally.
