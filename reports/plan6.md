# Plan 6: Remaining Work

Date: 2025-12-19
Scope: outstanding work only.

---

## Gizmo / Transform Space Unification (pending)
1) Define two spaces:
   - comp: origin left-bottom, Y up.
   - layer (object): origin center, coordinates in pixels [-w/2..w/2], [-h/2..h/2].
2) Add conversion helpers (single ground truth):
   - comp <-> viewport (centered) conversion using comp size.
   - layer pivot <-> comp conversion using src size + pivot offsets.
3) Update gizmo:
   - translate = comp->viewport(layer pivot).
   - move writes back via viewport->comp->position_from_pivot.
4) Update RMB drag tool to use same conversions (no divergent codepath).
5) Decide canonical storage for attrs:
   - keep existing Y-down storage and only convert for gizmo, or
   - switch attrs to Y-up comp space and update transform/render math.

## 3D Perspective Roadmap (still pending)

### Phase 1: Camera integration
- `CompNode::active_camera` (topmost CameraNode), `CompNode::aspect()` helper.

### Phase 2: 3D transform math
- `transform::build_model_matrix(position, rotation, scale, pivot) -> Mat4`.
- `build_mvp(model, view, projection)` + inverse.

### Phase 3: GPU compositor
- Shader uses `mat4` MVP + inverse for projective sampling.
- Blend API changes from `mat3` to `mat4`.

### Phase 4: compose_internal
- Detect 3D comp and route to GPU path.
- Skip rendering camera layers; sort layers by Z.

### Phase 5: UI
- Ensure XYZ rotation editable in AE panel.
- Add camera creation in UI where needed; numeric 3D controls for v1.

---

## Open decisions
- Euler order (AE ZYX vs configurable).
- 2D/3D auto-detect vs per-comp toggle.
- v1 3D gizmo vs later.
