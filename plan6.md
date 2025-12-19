# Plan 6: Unified Audit + Roadmap (Plan4 + Plan5 + Updates)

Date: 2025-12-19
Scope: single source of truth for audit findings, fixes, and 3D perspective roadmap.

---

## 0) Goals
- Consolidate Plan4 (3D perspective) and Plan5 (audit) into one plan.
- Add new requirements: AttrValue Map/Set, previous comp history queue.
- Keep backward compatibility for saved projects.

---

## 1) Plan5 Audit Verification (Confirmed / Partial / Not Confirmed)

### Dead code and unused APIs
- PreloadStrategy (src/core/cache_man.rs, re-exported in src/core/mod.rs): CONFIRMED unused in repo. NOTE: public API surface; removal is a breaking change.
- PreloadFrameEvent (src/core/player_events.rs): CONFIRMED unused.
- EventBus::unsubscribe_all/clear/has_subscribers/queue_len (src/core/event_bus.rs): CONFIRMED only used in tests.
- GlobalFrameCache::clear_frame/comp_count/has_comp/is_empty (src/core/global_cache.rs): CONFIRMED only used in tests (comp_count) or unused.
- CacheManager::mem_usage_fraction (src/core/cache_man.rs): CONFIRMED unused.
- DebouncedPreloader::cancel/is_pending/pending_comp (src/core/debounced_preloader.rs): CONFIRMED only used in tests.
- Player::selected_seq_idx/set_selected_seq_idx (src/core/player.rs): CONFIRMED unused.

### #[allow(dead_code)] review
- GpuCompositor.texture_cache (src/entities/gpu_compositor.rs): CONFIRMED unused. OK to keep as TODO or remove.
- ViewportState::is_point_over_image/screen_to_image (src/widgets/viewport/viewport.rs): CONFIRMED unused.
- EncodeStage::Error (src/dialogs/encode/encode.rs): PLAN5 INCORRECT NAME. Variant is used in encode_ui.rs; allow(dead_code) is unnecessary, not dead code.

### Code duplication
- EventBus emit logic in EventBus::emit/emit_boxed + EventEmitter::emit/emit_boxed: CONFIRMED duplication.
- create_image_dialog in viewport_ui.rs and project_ui.rs: CONFIRMED duplication.
- Sampling helpers in transform.rs (sample_f32/f16/u8): CONFIRMED duplication.
- Blend helpers in compositor.rs (blend_f32/f16/u8): CONFIRMED duplication.

### Potential bugs
- Video frame decode O(n) from start (loader_video.rs): CONFIRMED. Every frame request decodes from frame 0, so random access is O(n) per request.
- Camera/Text timing ignored in NodeKind::_in/_out/frame (node_kind.rs): CONFIRMED. Latent but incorrect if any code uses NodeKind timing for Camera/Text.
- Player::step i32::MIN overflow via unsigned_abs cast (player.rs): CONFIRMED bug.
- GlobalFrameCache get() LRU update race (global_cache.rs): CONFIRMED low impact (stale LRU entry possible).
- Workers epoch load Ordering::Relaxed (workers.rs): NOT CONFIRMED as a bug. Epoch is a cancellation token; Relaxed is sufficient unless it must synchronize other memory.

### Performance notes
- ViewportState clone each frame (viewport_ui.rs): CONFIRMED, likely minor.
- Timeline clones in drag handling and selection copies (timeline_ui.rs): CONFIRMED, moderate if selection large.
- Per-frame HashMap allocation for geom_cache (timeline_ui.rs): CONFIRMED.
- GPU compositor not used in worker compose path (compositor.rs + comp_node.rs): CONFIRMED and documented as WIP, not a regression.

### Architecture notes
- Inconsistent event dispatch patterns across widgets: CONFIRMED.
- Y-axis inversions scattered in viewport/gizmo: CONFIRMED.
- Node editor read-only: CONFIRMED, documented limitation.

### TODO comments (from plan5)
- src/lib.rs: clippy complex signatures - valid tech debt.
- src/entities/compositor.rs: GPU compositing guide - WIP docs.
- src/entities/gpu_compositor.rs: texture caching + canvas-sized blending - future optimizations.

---

## 2) Additional Issues / Requirements (New)

1) EventBus callbacks invoked under read lock
   - EventBus::emit/emit_boxed and EventEmitter equivalents hold the subscribers read lock while invoking callbacks.
   - If a callback tries to subscribe/unsubscribe, it can deadlock (write lock vs read lock).
   - Fix: clone callback list, drop lock, then invoke callbacks.

2) Project UI O(n^2) order lookups + JSON parse per row
   - project_ui.rs calls project.order() for each row, and order() parses JSON every call.
   - Cache order once per render and precompute an index map.

3) AttrValue missing HashMap/HashSet (requested)
   - Add AttrValue::HashMap(HashMap<String, AttrValue>) and AttrValue::HashSet(HashSet<AttrValue>).
   - Extend AttrType with Map/Set and add getters/setters.
   - Add helpers for UUID list/set to avoid JSON for common state.

4) previous_comp should be history list/queue (requested)
   - Replace previous_comp Option with previous_comp_history Vec<Uuid> (or VecDeque).
   - On activation (double-click, U, etc.), push prior comp, dedupe adjacent, cap length.
   - U key pops/uses most recent entry; stable no-op if empty.

5) GlobalFrameCache capacity/memory checks are pre-insert only
   - insert() checks limits before insert; large frames can push cache over limit until next insert.
   - len() is O(n) and used in loop, can be expensive.
   - Fix: track frame count in CacheStats or pre-evict based on frame_size; check after insert if needed.

6) Video decode format conversion cost
   - loader_video.rs decodes to RGB24 then expands to RGBA in nested loop.
   - Use ffmpeg scaler to output RGBA directly to avoid extra pass.

7) Video frame indexing ignores timestamps
   - loader_video.rs uses frame counter; for VFR/B-frames, frame index != target PTS.
   - Fix: seek by timestamp (PTS) using stream time_base or track decoded PTS.

8) Event queue eviction cost
   - EventBus uses Vec and drains from front on overflow (O(n)).
   - Consider VecDeque or ring buffer.

---

## 3) Fix Plan (Prioritized)

### P0 - Correctness and user-visible behavior
1) Fix Player::step i32::MIN overflow (player.rs).
2) Improve video frame access:
   - Implement seek-to-PTS or frame index map for random access.
   - Avoid decode-from-start per request.
3) Make NodeKind::_in/_out/frame delegate to node attrs (camera/text timing).
4) EventBus: remove lock while invoking callbacks (clone list first).
5) previous_comp history queue:
   - Replace previous_comp Option with previous_comp_history (Vec/VecDeque).
   - Update set_active_comp to push prior comp; update U key to pop.
   - Add length cap + duplicate suppression.

### P1 - Data model and performance (do it right)
1) Extend AttrValue with HashMap/HashSet:
   - Add AttrValue::HashMap and AttrValue::HashSet.
   - Extend AttrType with Map/Set and schema support where needed.
   - Add typed getters/setters and helpers for UUID list/set.
   - Add migration to convert Json strings -> typed values on load.
2) Move Project/Player attrs off Json where possible:
   - order -> List of Uuid
   - selection -> List of Uuid (keep ordering) and optional cached set for membership
   - active/active_comp -> Uuid (absent key means None)
   - previous_comp_history -> List of Uuid
   - prefs stays Json until typed map support for that struct is defined
3) Cache project.order() once per UI render; avoid repeated parsing in project_ui.rs.
4) Cache timeline geom_cache in TimelineState to avoid per-frame allocations.
5) Reduce per-frame clones in viewport/timeline (prefer references or small copies).
6) GlobalFrameCache: track count or evict based on frame_size; avoid repeated len() scans.
7) loader_video: request RGBA output directly from scaler.
8) Event queue storage: switch Vec -> VecDeque or ring buffer.

### P2 - Cleanup and consistency
1) Remove or cfg(test) gate truly unused functions (after verifying public API expectations).
2) Remove GpuCompositor.texture_cache or implement cache properly.
3) Consolidate event dispatch patterns (single mechanism across widgets).
4) Document coordinate system once and centralize Y inversion.

---

## 4) Plan4: 3D Perspective Projection Roadmap

### 4.1 Current State (Ready)
- CameraNode (src/entities/camera_node.rs): view_matrix(), projection_matrix(aspect), view_projection_matrix(aspect), AE defaults (FOV 39.6, position [0,0,-1000], POI [0,0,0]).
- Layer attrs: position/rotation/scale/pivot already Vec3.
- EventBus uses std::sync (crossbeam only in workers for work-stealing).

### 4.2 Not Ready (Gaps)
- compose_internal is 2D only (uses rot[2], mat3). Camera not referenced.
- No perspective-correct sampling on CPU; no Z ordering.

### 4.3 Architecture
- Camera selection: AE-style, topmost CameraNode layer is active.
- Pipeline per layer: Model (T*R*S*Pivot) -> View -> Projection -> MVP.
- GPU required for perspective-correct sampling; CPU can only approximate.
- Z ordering: painter's algorithm (sort by Z) for v1; depth buffer later.

### 4.4 Implementation Phases
Phase 1: Camera integration
- CompNode::active_camera(media) finds topmost CameraNode layer.
- CompNode::aspect() helper.

Phase 2: 3D transform math
- transform::build_model_matrix(position, rotation, scale, pivot) -> Mat4.
- build_mvp(model, view, projection) and inverse MVP.

Phase 3: GPU compositor
- Shader uses mat4 MVP + inverse for projective sampling.
- Blend API changes from mat3 [f32;9] to mat4 [f32;16].

Phase 4: compose_internal
- Detect 3D comp (camera present or rot X/Y non-zero).
- compose_3d path uses GPU compositor with mat4 transforms.
- Skip rendering camera layers; sort layers by Z.

Phase 5: UI updates
- Ensure XYZ rotation editable in AE panel.
- Keep 2D gizmo for v1; numeric inputs for 3D.
- Add camera creation in UI where needed.

### 4.5 Execution Order
1) build_model_matrix in transform.rs
2) active_camera helper in CompNode
3) 3D detection
4) GPU shader mat4
5) Blend API mat4
6) compose_3d implementation
7) Z sorting
8) Camera layer UI
9) Verify AE XYZ rotation inputs

### 4.6 Testing
- Unit: model matrix, MVP inversion, camera matrices not NaN/Inf.
- Visual: X/Y rotation tilt, camera move perspective, Z sort correctness, transparency.
- Regression: 2D comps unchanged.

### 4.7 Open Decisions
- Euler order: ZYX (AE) or configurable?
- Default camera fallback settings.
- Auto-detect 2D/3D vs per-comp toggle.
- 3D gizmo v1 or later.

### 4.8 Crossbeam vs std::sync (clarification)
- crossbeam::deque used in workers.rs (work-stealing).
- EventBus uses std::sync, not crossbeam-channel.
- For REST/local API: std::sync::mpsc is fine unless crossbeam features needed.

---

## 5) Notes on Removals / Migration
- Dead code removals may be breaking for public APIs.
- AttrValue changes require backward-compatible migration:
  - On load: accept Json and upgrade to typed values.
  - On save: emit new typed format.
- Keep prefs as Json until typed map for ProjectPrefs is defined.
