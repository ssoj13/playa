# What's Left to Fix

After unifying viewport/gizmo coordinate systems, here's what remains from cdx.md audit:

---

## Completed

| Task | Status |
|------|--------|
| #2 Gizmo/renderer world units | FIXED - both use `zoom + pan` view matrix |
| #7 Stale comments | FIXED - updated in gizmo.rs |
| #4 Rotation helpers | DONE - added to space.rs, used in gizmo.rs |
| Cleanup _comp_size params | DONE - removed from gizmo.rs |

---

## Still To Do

### 1. Unify coordinate helpers (Issue #1)

Three places with overlapping conversions:
- `space.rs` — `image_to_frame`, `frame_to_image` (glam::Vec2)
- `coords.rs` — `screen_to_viewport_centered` (egui::Vec2)
- `viewport.rs` — `image_to_screen`, `screen_to_image` (egui::Vec2)

**Options:**
- A) Move all to `space.rs`, add egui/glam conversion helpers
- B) Keep `coords.rs` for egui-specific UI helpers, have them call `space.rs` internally
- C) Delete `coords.rs`, inline into viewport.rs

**Effort:** Medium

---

### 2. GPU compositor conventions (Issue #5)

`build_inverse_matrix_3x3()` in transform.rs builds comp->src mapping. Currently unused (GPU path inactive). When enabled, verify it matches CPU path conventions.

**Effort:** Low (verification when GPU path enabled)

---

### 3. CameraNode integration (Issue #6)

**Status:** CameraNode is FULLY IMPLEMENTED but not wired into compositor.

`camera_node.rs` has:
- `view_matrix()` — world -> camera (supports POI or rotation)
- `projection_matrix()` — perspective or orthographic
- `view_projection_matrix()` — combined
- Full AE-like attributes: fov, near/far clip, DOF params

`transform.rs` has `transform_frame_with_camera()` but always receives `None`.

**To integrate:**
1. Add camera selection to comp (active camera UUID)
2. In compositor, get camera's view_projection matrix
3. Pass to `transform_frame_with_camera()` instead of `None`
4. Update viewport to use camera's projection for preview

**Effort:** Medium-High (real 3D compositing)

---

### 4. Use rotation helpers in transform.rs

`transform.rs` still has manual sign handling. Could use `space::to_math_rot()` for consistency.

**Effort:** Low

---

## Summary

Quick wins done. Remaining work is either:
- **Unification** (coords.rs consolidation) — cleanup
- **Feature work** (CameraNode integration) — new capability
- **Future-proofing** (GPU compositor) — when needed
