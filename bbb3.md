# 3D Gizmo Support: PARTIAL

## What Was Implemented

### 1. Camera VP in Gizmo
- `get_camera_vp()` - gets view-projection from active camera
- `build_gizmo_matrices()` - uses camera VP when available, ortho fallback for 2D
- Gizmo now matches perspective camera view

### 2. Full 3D Transform
- `layer_to_gizmo_transform()` - passes all 3 rotation components (was only Z)
- Rotation order: ZYX (matches compositor)
- Conversion: CW+ degrees → CCW+ radians via `space::to_math_rot()`

### 3. 3D Gizmo Modes Enabled
```rust
Move:   TranslateX/Y/Z + XY/XZ/YZ planes + View
Rotate: RotateX/Y/Z (full 3D rotation)
Scale:  ScaleX/Y/Z + Uniform
```

### 4. Transform Event Updated
- `build_transform_event()` now takes all 3 components for rotation/scale
- Was: `[old_rot[0], old_rot[1], gizmo_rot[2]]` (only Z from gizmo)
- Now: `gizmo_rot` (all three from gizmo)

---

## Files Changed

| File | Function | Change |
|------|----------|--------|
| `gizmo.rs` | `get_camera_vp()` | NEW - gets camera VP matrix |
| `gizmo.rs` | `build_gizmo_matrices()` | Added `camera_vp` param, perspective support |
| `gizmo.rs` | `render()` | Gets camera VP and passes to matrices |
| `gizmo.rs` | `to_gizmo_modes()` | Added TranslateZ, RotateX/Y, ScaleZ |
| `gizmo.rs` | `layer_to_gizmo_transform()` | Full 3D rotation (was only Z) |
| `gizmo.rs` | `gizmo_to_layer_transform()` | ZYX euler order, all 3 components |
| `gizmo.rs` | `build_transform_event()` | Takes full rotation/scale from gizmo |

---

## Test Cases

- [ ] No camera → gizmo works as before (2D ortho)
- [ ] Perspective camera → gizmo in perspective view
- [ ] Layer at Z=-500 → gizmo appears smaller
- [ ] Rotate layer X/Y → gizmo axes tilted
- [ ] Drag RotateX/Y handles → rotation updates
- [ ] Drag TranslateZ → Z position updates
- [ ] Drag ScaleZ → Z scale updates

---

## Notes

- Gizmo library: `transform-gizmo-egui`
- Camera VP passed as "view" with identity "proj" (combined VP works for gizmo)
- Viewport zoom/pan applied on top of camera VP
