# CDX Audit Review

Review of issues described in `cdx.md` against actual codebase.

**Date:** 2025-12-19  
**Reviewer:** Claude Code  
**Verdict:** 6 of 7 issues confirmed as real

---

## Issue 1: Multiple coordinate systems without single owner

**Status:** CONFIRMED

Evidence:
- `space.rs` — defines `image_to_frame`, `frame_to_image`, `object_to_src` (uses `glam::Vec2`)
- `coords.rs` — defines `screen_to_viewport_centered`, `flip_y_vec2` (uses `egui::Vec2`)
- `viewport.rs` `ViewportState` — has `image_to_screen`, `screen_to_image` (uses `egui::Vec2`)

Three separate modules with coordinate conversions, using different vector types, not unified.

---

## Issue 2: Gizmo and renderer use different world units

**Status:** CONFIRMED (Critical)

Evidence:

**Renderer** (`renderer.rs:193-201`):
```rust
let vertices: [f32; 16] = [
    -0.5, -0.5,  0.0, 1.0,  // normalized quad
     0.5, -0.5,  1.0, 1.0,
     ...
];
```

**Viewport view matrix** (`viewport.rs:305-313`):
```rust
let aspect_corrected_zoom_x = self.zoom * self.image_size.x;
let aspect_corrected_zoom_y = self.zoom * self.image_size.y;
// scales normalized -0.5..0.5 to pixels
```

**Gizmo** (`gizmo.rs:207-216`):
```rust
let view = DMat4::from_scale_rotation_translation(
    DVec3::splat(viewport_state.zoom as f64),  // just zoom, no image_size
    ...
    DVec3::new(viewport_state.pan.x, viewport_state.pan.y, 0.0),
);
```

Renderer: `normalized * image_size * zoom = pixels`  
Gizmo: `pixels * zoom`

When `comp_size != image_size` or zoom != 1, positions desync.

---

## Issue 3: Default position semantics

**Status:** PARTIALLY CONFIRMED

Evidence (`comp_node.rs:141-144`):
```rust
// Transform in frame space (origin = center, Y-up)
// Position (0,0,0) = layer centered in comp
attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
```

Intent is correct — `position=(0,0,0)` should center layer. However, due to Issue #2 (unit mismatch), gizmo may visually appear misaligned. The root cause is Issue #2, not the default value itself.

---

## Issue 4: Rotation sign conversions not centralized

**Status:** CONFIRMED

Evidence:

**transform.rs:65-67:**
```rust
// Our convention is CW+ (clockwise positive), glam uses CCW+ (math convention).
// Forward rotation in our convention = negative in glam.
```

**gizmo.rs:244-247:**
```rust
let rotation_quat = DQuat::from_euler(
    glam::EulerRot::XYZ,
    0.0, 0.0,
    -((rotation[2] as f64).to_radians()),  // sign flip here
);
```

**gizmo.rs:261:**
```rust
(-(euler.2 as f32)).to_degrees(),  // another sign flip
```

Sign conversions scattered across two files without centralized helpers like `to_math_rotation()` / `from_math_rotation()`.

---

## Issue 5: GPU compositor matrix conventions fragile

**Status:** POTENTIAL (not yet active)

`build_inverse_matrix_3x3` exists in `transform.rs:128` and builds comp->src mapping with Y-flip. GPU compositor path is currently inactive, so cannot verify in practice. Will surface when GPU path is enabled.

---

## Issue 6: 3D path not integrated

**Status:** CONFIRMED

Evidence (`transform.rs:319-322`):
```rust
pub fn transform_frame_with_camera(
    ...
    view_projection: Option<Mat4>,  // always None in current usage
) -> Frame {
```

And in `transform_frame` (`transform.rs:307`):
```rust
transform_frame_with_camera(src, canvas, position, rotation, scale, pivot, None)
```

`CameraNode` exists but its matrices are never passed to layer transforms.

---

## Issue 7: Stale/misleading comments

**Status:** CONFIRMED

**gizmo.rs:12:**
```rust
//! Frame space == viewport space, so NO conversion needed for gizmo!
```

**gizmo.rs:237:**
```rust
// Frame space == viewport space, so NO conversion needed!
```

This is no longer true — renderer uses normalized space (-0.5..0.5) scaled by `image_size * zoom`, while gizmo operates in raw pixel coordinates with just `zoom` scaling.

---

## Summary

| Issue | Status | Severity |
|-------|--------|----------|
| 1. Multiple coord systems | CONFIRMED | Medium |
| 2. Different world units | CONFIRMED | **Critical** |
| 3. Default position | PARTIAL | Low (symptom of #2) |
| 4. Rotation sign scattered | CONFIRMED | Medium |
| 5. GPU conventions | POTENTIAL | Low (inactive) |
| 6. 3D not integrated | CONFIRMED | Low (feature gap) |
| 7. Stale comments | CONFIRMED | Low |

## Recommendation

The cdx.md analysis is accurate. **Issue #2 is the root cause** of visual desync between gizmo and rendered image.

**Recommended fix: Option A from cdx.md**

Change renderer quad to pixel space instead of normalized:
- Quad vertices: `(-w/2, -h/2) .. (w/2, h/2)` in pixels
- View matrix: just `scale(zoom) + translate(pan)` — same as gizmo
- Result: renderer and gizmo share identical coordinate system

This eliminates the unit mismatch and makes Issues #3 and #7 resolve naturally.

Secondary fixes:
1. Centralize rotation sign helpers in `space.rs`
2. Unify `ViewportState::image_to_screen` with `space.rs` helpers
3. Update stale comments after coordinate unification
