# Plan 6: Remaining Work

Date: 2025-12-19
Scope: only outstanding work; no backward compatibility required.

---

## Constraints
- No backward compatibility: remove JSON migration paths for project/player attrs.
- Single ground truth: avoid parallel codepaths for the same behavior.

---

## P0 - Correctness and user-visible behavior
1) Fix `Player::step` i32::MIN overflow.
2) Make `NodeKind::_in/_out/frame` use node attrs for Camera/Text timing.
3) Video random access: seek by timestamp/PTS (or build a frame index) instead of decode-from-start.

---

## P1 - Performance
1) `loader_video`: request RGBA directly from the scaler (no manual RGB24->RGBA loop).
2) `GlobalFrameCache`: track counts/size and evict post-insert to honor limits; avoid O(n) len scans.
3) `EventBus`: switch queue storage from `Vec` to `VecDeque`/ring buffer to avoid O(n) drains.
4) `Timeline`: keep `geom_cache` in state to avoid per-frame HashMap allocations.
5) Reduce per-frame clones in viewport/timeline (prefer references or small value copies).

---

## P2 - Cleanup and consistency
1) Remove JSON compatibility migrations in `Project`/`Player` attrs (order/selection/active/prefs/selected_seq_idx/previous_comp).
2) Consolidate event dispatch helpers + file dialog helpers to avoid duplicate codepaths.
3) Centralize coordinate system/Y inversion rules and document once.

---

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
