# Plan: 3D Gizmo Support

## Current State: 2D Only

Gizmo uses orthographic projection and ignores:
- Camera view-projection (perspective)
- Layer Z position
- Layer X/Y rotation (only Z rotation works)

```rust
// gizmo.rs:244 - always ortho, no camera
let proj = DMat4::orthographic_rh(-w/2, w/2, -h/2, h/2, -1000.0, 1000.0);

// gizmo.rs - transform ignores Z
Transform { translation: [x, y, 0.0], ... }
```

---

## Goal

Gizmo should match what renderer shows:
- With perspective camera → gizmo in perspective
- Layer at Z=-500 → gizmo appears smaller/farther
- Layer rotated in 3D → gizmo axes follow

---

## Implementation Plan

### 1. Pass Camera VP to Gizmo

**File:** `gizmo.rs`

```rust
fn build_gizmo_matrices(
    viewport_state: &ViewportState,
    clip_rect: egui::Rect,
    camera_vp: Option<Mat4>,  // NEW
) -> (mint::RowMatrix4<f64>, mint::RowMatrix4<f64>) {

    let (view, proj) = if let Some(vp) = camera_vp {
        // 3D mode: use camera matrices
        // Split VP back to V and P, or pass separately
        todo!()
    } else {
        // 2D mode: ortho as before
        let view = DMat4::from_scale_rotation_translation(...);
        let proj = DMat4::orthographic_rh(...);
        (view, proj)
    };

    (to_row_matrix(view), to_row_matrix(proj))
}
```

### 2. Use Full 3D Transform

**File:** `gizmo.rs` in `collect_transforms()`

```rust
// BEFORE:
let translation = mint::Vector3 {
    x: pos[0] as f64,
    y: pos[1] as f64,
    z: 0.0,  // Z ignored!
};

// AFTER:
let translation = mint::Vector3 {
    x: pos[0] as f64,
    y: pos[1] as f64,
    z: pos[2] as f64,  // Use actual Z
};

// Also pass rotation X/Y:
let rotation = euler_to_quat(rot[0], rot[1], rot[2]);
```

### 3. Get Camera from Comp

**File:** `gizmo.rs` in `render()`

```rust
// Get active camera for current frame
let camera_vp = project.with_comp(comp_uuid, |comp| {
    let media = project.media.read().unwrap();
    comp.active_camera(frame_idx, &media)
        .map(|(cam, pos, rot)| {
            let aspect = viewport_state.image_size.x / viewport_state.image_size.y;
            cam.view_projection_matrix(pos, rot, aspect, comp.height() as f32)
        })
}).flatten();

let (view, proj) = build_gizmo_matrices(viewport_state, clip_rect, camera_vp);
```

### 4. Handle Perspective Interaction

When dragging gizmo in perspective mode, movement should follow the perspective ray, not screen plane.

`transform-gizmo-egui` should handle this if we pass correct VP matrices.

---

## Files to Modify

| File | Change |
|------|--------|
| `gizmo.rs` | Pass camera VP, use full 3D transform |
| `viewport.rs` | Maybe expose camera VP getter |

---

## Risks

- `transform-gizmo-egui` may not handle perspective well
- Interaction math might need adjustment
- Performance: computing camera VP per frame

---

## Priority

MEDIUM. Camera rendering works, this is UX polish for 3D workflows.

---

## Test Cases

1. No camera → gizmo works as before (2D ortho)
2. Ortho camera → gizmo matches layer positions
3. Perspective camera → gizmo shrinks with distance
4. Layer rotated X/Y → gizmo axes tilted
5. Drag in perspective → movement follows 3D correctly
