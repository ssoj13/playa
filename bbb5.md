# Attribute Conflict: position on Layer vs CameraNode

## Problem

Both Layer and CameraNode have `position/rotation/scale` via TRANSFORM group:

```rust
// attr_schemas.rs
const TRANSFORM: &[AttrDef] = &[
    AttrDef::new("position", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("rotation", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("scale", AttrType::Vec3, DAG_DISP_KEY),
    AttrDef::new("pivot", AttrType::Vec3, DAG_DISP_KEY),
];

LAYER_SCHEMA  = [IDENTITY, LAYER_SPECIFIC, TIMING, OPACITY, TRANSFORM, NODE_POS]
CAMERA_SCHEMA = [IDENTITY, TRANSFORM, CAMERA_SPECIFIC, TIMING, OPACITY]
                         ^^^^^^^^^
```

When Camera added as layer:
```
Layer (has position/rotation/scale from LAYER_SCHEMA)
  └─ source_uuid -> CameraNode (has position/rotation/scale from CAMERA_SCHEMA)
```

**Which position is used?** Currently `active_camera()` returns `&CameraNode` and code uses `cam.position()` — ignoring layer transform.

---

## Current Behavior

```rust
// comp_node.rs:984
let camera = self.active_camera(frame_idx, ctx.media);
let view_projection = camera.map(|cam| {
    cam.view_projection_matrix(aspect, height)  // uses cam.position()
});
```

CameraNode.position is used. Layer.position on camera layer is ignored.

---

## Options

### Option A: Layer owns transform (AE-style) ✓ RECOMMENDED

Camera is spatial object in comp. Use layer's transform, not node's.

**Changes:**
1. Remove TRANSFORM from CAMERA_SCHEMA:
   ```rust
   CAMERA_SCHEMA = [IDENTITY, CAMERA_SPECIFIC, TIMING, OPACITY]
   ```

2. Update `active_camera()` to return layer transform:
   ```rust
   pub fn active_camera(...) -> Option<(CameraConfig, [f32;3], [f32;3], [f32;3])>
   //                                   ^lens/proj    ^pos    ^rot    ^scale
   ```

3. Build view matrix from layer transform in compositor.

**Pros:**
- Consistent: all layer types use layer.position
- No duplicate attrs
- Animation on layer works naturally

**Cons:**
- Refactor needed

---

### Option B: Node owns transform (current)

Camera position lives in CameraNode. Layer is just a timing container.

**Pros:**
- Already implemented
- Camera is self-contained

**Cons:**
- Layer.position on camera layer is dead code
- Confusing: two position attrs, only one works
- Inspector would show both

---

### Option C: Combine transforms

`final_transform = layer_transform * camera_transform`

**Cons:**
- Confusing mental model
- Not how AE works
- Unnecessary complexity

---

## Recommendation: Option A

1. Remove TRANSFORM from CAMERA_SCHEMA
2. Camera only has: `projection_type, fov, near_clip, far_clip, ortho_scale, poi, use_poi, dof_*`
3. Position/rotation/scale come from the Layer
4. Update `active_camera()` + `compose_internal()` to read layer attrs

Same applies to future LightNode, NullNode — they're spatial objects, position comes from Layer.

---

## Implementation Plan

1. **CAMERA_SCHEMA** — remove TRANSFORM include
2. **CameraNode** — remove position/rotation/scale getters (keep only lens/projection)
3. **camera_node.rs** — `view_matrix()` takes position/rotation as args
4. **comp_node.rs** — `active_camera()` returns layer transform too
5. **compose_internal()** — build camera view from layer attrs

---

## Files to Touch

- `src/entities/attr_schemas.rs` — CAMERA_SCHEMA
- `src/entities/camera_node.rs` — remove transform, update view_matrix signature
- `src/entities/comp_node.rs` — active_camera + compose_internal

---

## Status: Option A Implemented ✓

Changes made:
1. Removed TRANSFORM from CAMERA_SCHEMA
2. CameraNode no longer has position/rotation/scale attrs
3. `view_matrix(pos, rot)` and `view_projection_matrix(pos, rot, aspect, height)` take args
4. `active_camera()` returns `Option<(&CameraNode, [f32;3], [f32;3])>` — camera + layer pos/rot
5. `add_child_layer()` has `initial_position` param — camera layers get `[0, 0, -1000]`

---

# NEW ISSUE: Perspective Projection Broken

## Problem

Camera с `projection_type: perspective` даёт сильно увеличенную картинку.

**Root cause:** `transform.rs:401` использует `transform_point3()` для инверсии MVP:

```rust
let obj_pt3 = inv.transform_point3(frame_pt3);
```

Это работает только для **аффинных** матриц (translate/rotate/scale).
Перспективная проекция требует **perspective divide** (деление на W), которое `transform_point3` не делает.

Правильный pipeline для перспективы:
```
Forward:  P_clip = VP * P_world  →  P_ndc = P_clip.xyz / P_clip.w
Inverse:  Requires ray-plane intersection, not simple matrix inverse
```

## Options

### Option 1: Quick Fix — Disable Perspective

Пока использовать только orthographic камеру. Ortho не требует W-divide.

```rust
// camera_node.rs - force ortho for now
pub fn projection_matrix(&self, aspect: f32, comp_height: f32) -> Mat4 {
    // Always use orthographic until perspective is fixed
    let scale = self.ortho_scale();
    let half_h = (comp_height * 0.5) / scale;
    let half_w = half_h * aspect;
    Mat4::orthographic_rh_gl(-half_w, half_w, -half_h, half_h, near, far)
}
```

**Pros:** Quick, works now
**Cons:** No perspective camera

---

### Option 2: Proper Perspective Unproject

Для каждого screen pixel:
1. Cast ray from camera through pixel
2. Intersect ray with layer's Z plane
3. Get world coordinate
4. Transform to object space

```rust
fn unproject_to_plane(screen: Vec2, inv_vp: Mat4, plane_z: f32) -> Vec3 {
    // Ray origin (camera pos) and direction from screen point
    let near_pt = inv_vp.project_point3(Vec3::new(ndc.x, ndc.y, -1.0));
    let far_pt = inv_vp.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
    let ray_dir = (far_pt - near_pt).normalize();

    // Intersect with z=plane_z
    let t = (plane_z - near_pt.z) / ray_dir.z;
    near_pt + ray_dir * t
}
```

**Pros:** Real perspective works
**Cons:** More complex, need layer Z info

---

### Option 3: GPU Rendering

Move transform to GPU shader where perspective is handled automatically.

**Pros:** Correct, fast
**Cons:** Big refactor, need OpenGL pipeline

---

## Solution: Option 2 Implemented ✓

Реализован ray-plane intersection для перспективной проекции.

### Changes in `transform.rs`:

1. Added `unproject_to_plane()` function:
```rust
fn unproject_to_plane(ndc: Vec2, inv_vp: Mat4, plane_z: f32) -> Option<Vec3> {
    // Cast ray from camera through NDC point
    // Intersect with z=plane_z plane
    // Return world-space point
}
```

2. Updated `transform_frame_with_camera()`:
   - Uses `layer_z = position[2]` for the plane Z coordinate
   - For perspective: calls `unproject_to_plane()` then applies `inv_model`
   - For ortho/no camera: uses direct affine transform as before

### How it works:

```
Screen pixel → NDC → Ray from camera → Intersect Z plane → World point → Object space → Texture sample
```

For each screen pixel:
1. Convert to NDC [-1, 1]
2. Unproject two points (near/far plane) through inverse VP
3. Build ray direction
4. Intersect ray with z=layer_z plane
5. Apply inverse model matrix to get object space coords
6. Sample texture

Build: ✓
Tests: 2/2 passed
