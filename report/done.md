# Frame Space Implementation - DONE

Date: 2025-12-19

---

## Changes Made

### 1. space.rs - New coordinate system

Added frame space (centered, Y-up):
```rust
/// Image (0,0) = top-left -> Frame (-w/2, h/2)
/// Image (w/2, h/2) = center -> Frame (0, 0)
pub fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2

pub fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2
```

Removed legacy comp space functions:
- `comp_to_viewport()` - REMOVED
- `viewport_to_comp()` - REMOVED  
- `image_to_comp()` - REMOVED
- `comp_to_image()` - REMOVED

### 2. transform.rs - Use frame space

All 3 pixel format cases (F32, F16, U8) now use:
```rust
let frame_pt = space::image_to_frame(dst_pt, comp_size);
let frame_pt3 = Vec3::new(frame_pt.x, frame_pt.y, 0.0);
let obj_pt3 = inv.transform_point3(frame_pt3);
```

Also fixed inverse transform (from earlier):
- Rotation angles now negated for proper inverse
- Matrix order fixed: `S^(-1) * R^(-1)` not `R * S^(-1)`

### 3. comp_node.rs - Default position (0,0,0)

Layer position is now in frame space:
- `position = (0, 0, 0)` = layer CENTERED
- No more `(comp_w/2, comp_h/2)` hack

### 4. gizmo.rs - No conversion needed

Frame space == viewport space, so:
- Removed `comp_to_viewport()` call
- Removed `viewport_to_comp()` call
- Position passed directly to gizmo
- Removed `space` import

---

## New Coordinate Pipeline

```
Screen pixel (image space: top-left origin, Y-down)
    |
    |  space::image_to_frame()
    v
Frame space (centered origin, Y-up)
    |
    |  inverse model transform (position, rotation, scale, pivot)
    v
Object space (layer center origin)
    |
    |  space::object_to_src()
    v
Source pixel (for texture sampling)
```

---

## Key Semantic Change

**BEFORE**: `position = (360, 288)` meant centered in 720x576 comp
**AFTER**: `position = (0, 0, 0)` means centered (any comp size)

This is simpler and matches how After Effects works conceptually.

---

## BUILD: SUCCESS (no warnings)

Test now - quarter shift bug should be fixed!
