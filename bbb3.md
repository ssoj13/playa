# Coordinate System Cleanup — Final Status

## Completed

| Task | Status | Notes |
|------|--------|-------|
| #2 Gizmo/renderer world units | FIXED | Both use `zoom + pan` view matrix |
| #7 Stale comments | FIXED | Updated in gizmo.rs |
| #4 Rotation helpers | DONE | Added to space.rs, used in gizmo.rs |
| Cleanup _comp_size params | DONE | Removed from gizmo.rs |
| transform.rs rotation | OK | Already correct — inverse math handles sign |
| coords.rs consolidation | KEEP | Stays in widgets/ — UI layer, not domain |

---

## Architecture After Cleanup

```
entities/space.rs          — Domain: frame/image/object conversions (glam)
                           — Rotation helpers: to_math_rot(), from_math_rot()

widgets/viewport/coords.rs — UI: egui screen <-> viewport conversions
                           — Stays separate (egui types, widget-specific)

widgets/viewport/viewport.rs — Uses both:
                           — image_to_screen() uses frame space (like space.rs)
                           — handle_zoom/pan use coords.rs for egui input
```

---

## Remaining Work

### 1. CameraNode Integration (Feature)

**Status:** CameraNode is fully implemented, just not wired in.

`camera_node.rs` provides:
- `view_matrix()` — world -> camera (POI or rotation mode)
- `projection_matrix()` — perspective or orthographic
- `view_projection_matrix()` — combined MVP

**To enable 3D:**
1. Add active camera selection to comp
2. Get camera's view_projection matrix in compositor
3. Pass to `transform_frame_with_camera()` instead of `None`
4. Update viewport preview to use camera projection

**Effort:** Medium-High

---

### 2. GPU Compositor (When Enabled)

`build_inverse_matrix_3x3()` in transform.rs is ready but GPU path is inactive.
Verify conventions match CPU path when GPU compositor is enabled.

**Effort:** Low (verification only)

---

## Summary

**Core coordinate issues are resolved.** The codebase now has:
- Unified view matrix between renderer and gizmo
- Centralized rotation convention helpers
- Clean separation: domain (space.rs) vs UI (coords.rs)

Remaining work is feature development (3D camera) not bug fixes.
