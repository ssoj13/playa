# Camera System: DONE

## What Was Fixed

1. **Attribute Conflict** (Option A implemented)
   - Removed TRANSFORM from CAMERA_SCHEMA
   - Camera position/rotation now comes from Layer, not CameraNode
   - CameraNode only has lens/projection attrs (fov, near/far, ortho_scale, poi, dof)

2. **Perspective Projection**
   - Added ray-plane intersection for proper perspective unprojection
   - `unproject_to_plane()` casts ray from camera through NDC point, intersects layer plane

3. **Rotation Sign Convention Bug** (THE BIG ONE)
   - `build_model_matrix()` and `layer_plane_normal()` weren't negating angles
   - `build_inverse_transform()` expected negated angles (CW+ → CCW+ for glam)
   - Result: shear distortion on any rotation
   - Fix: negate angles in forward transforms to match documented CW+ convention

---

## What's Left

### Must Have
- [ ] Test camera Z position affects perspective (should zoom in/out)
- [ ] Test FOV behaves correctly (57° ≈ AE default)
- [ ] Test rotation X/Y/Z on layers with perspective camera

### Nice to Have
- [ ] DOF (depth of field) - attrs exist but not implemented
- [ ] Point of Interest mode testing
- [ ] Multiple cameras switching

### Known Limitations
- CPU compositor is slow for perspective (ray-plane per pixel)
- GPU rendering would be much faster but requires OpenGL pipeline

---

## Files Changed (with comments explaining "why")

| File | Change | Comment Location |
|------|--------|------------------|
| `attr_schemas.rs` | CAMERA_SCHEMA without TRANSFORM | "WHY NO TRANSFORM" block |
| `camera_node.rs` | view_matrix takes pos/rot args | "Architecture: Why pos/rot are arguments" |
| `comp_node.rs` | active_camera returns layer transform | "Return value" section in docstring |
| `transform.rs` | perspective unproject + rotation fix | Module docstring + "Why ray-plane intersection" |
| `node_kind.rs` | add_child_layer with initial_position | "initial_position parameter" docstring |
| `main_events.rs` | camera layers get Z=-1000 | "WHY CAMERA GETS Z=-1000" block |

---

## The Rotation Bug Explained

```
User convention: CW+ (clockwise positive looking down axis)
glam convention: CCW+ (counter-clockwise positive, math standard)

To convert: glam_angle = -user_angle

build_model_matrix was passing +angle (wrong)
build_inverse_transform expected -angle for forward (correct assumption)
layer_plane_normal was passing +angle (wrong)

Result: forward and inverse used OPPOSITE rotation directions → shear

Fix: negate angles in build_model_matrix and layer_plane_normal
```
