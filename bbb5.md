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
