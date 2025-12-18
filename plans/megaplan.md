# Playa — Mega Plan (Merged + Re-verified)

Date: 2025-12-18

This document merges and deduplicates the plans/reports found in `./plans`:
- `plans/plan1.md` (Bug Hunt & Architecture Report)
- `plans/plan_cdx.md` (Approval checklist)
- `plans/claude_plan.md` (3D transforms + REST API exploration)
- `plans/rest.md` (REST API implementation plan)

It also re-validates each *claimed problem* against the current codebase and classifies it as:
- **Confirmed bug** (reproducible/active mismatch)
- **Confirmed tech debt** (real inconsistency/dead code, but not breaking)
- **Latent risk** (could become a bug if/when certain codepaths are wired)
- **Not a problem** (claim does not hold)

---

## 0) Executive Summary

### Highest-impact confirmed issues
1. **Cache strategy is not applied after rebuild/load** (**Confirmed bug**)
   - `Project::rebuild_with_manager` always creates `GlobalFrameCache` with `CacheStrategy::All`, ignoring persisted `AppSettings.cache_strategy`.

2. **GPU compositor backend selection does not affect comp rendering** (**Confirmed product/architecture mismatch**)
   - UI exposes CPU/GPU compositing backend, and `main.rs` switches `Project.compositor`, but actual composition in workers uses `THREAD_COMPOSITOR` (CPU) and never consults `Project.compositor`.

3. **Unused/legacy plumbing remains from a refactor** (**Confirmed tech debt**)
   - Legacy stubs (e.g., `CompNode::emit_attrs_changed`) are still referenced by UI code in at least one path.
   - Several events/types/settings are defined or emitted but not handled.

### Largest new-feature proposals included in merged sources
- Add a **local REST API server** for remote control (play/pause/seek/status/media listing).
- Extend transform UX toward **true 3D** (XYZ rotation/translation, camera-driven perspective) — large scope, requires architectural decisions.

---

## 1) Current Verified Runtime Dataflow (Short)

### 1.1 Event-driven app loop
- Widgets + input emit events into `EventBus`.
- `PlayaApp::update()` drains queued events via `handle_events()`, then runs a derived-events loop to process `AttrsChangedEvent` emitted during the same frame.
- Cache invalidation:
  - `AttrsChangedEvent` → epoch++ → clear comp cache → enqueue current frame only → debounce full preload → `ViewportRefreshEvent`.

### 1.2 Preload and compute
- `enqueue_frame_loads_around_playhead()` calls `CompNode::signal_preload()`.
- Preload enqueues worker jobs; each job snapshots `Project.media` (clone of `HashMap<Uuid, Arc<NodeKind>>`) and runs `CompNode::compute()`.
- `FileNode::compute()` performs synchronous disk decode on a worker thread and inserts results into `GlobalFrameCache`.

### 1.3 Viewport frame acquisition
- Viewport reads from cache only (`Project::compute_frame` → `GlobalFrameCache::get`).
- UI repaint is triggered by `CacheManager::take_dirty()` when workers insert frames.

---

## 2) Issue Registry (Validated)

> Evidence references are file paths + key identifiers. Line numbers may drift; use search in file.

| ID | Title | Status | Evidence (primary) | Why it matters | Fix direction |
|---:|------|--------|-------------------|----------------|--------------|
| P0-1 | `cache_strategy` ignored after rebuild/load | **Confirmed bug** | `src/entities/project.rs` (`rebuild_with_manager` creates cache with `CacheStrategy::All`) | Persisted setting is misleading; memory usage/perf profile differs from user choice | Pass strategy into rebuild or apply `global_cache.set_strategy(settings.cache_strategy)` immediately after rebuild |
| P0-2 | GPU compositor backend is a no-op for comp output | **Confirmed mismatch** | `src/main.rs` (`update_compositor_backend`), `src/entities/comp_node.rs` (`THREAD_COMPOSITOR`), `src/entities/project.rs` (`compute_frame` is cache-only) | UI claims a feature that is not actually used for composition | Either wire compositor selection into comp blend pipeline (large) or mark as experimental + ensure UI is honest (small) |
| P1-1 | `CompositorBackendChangedEvent` emitted but not handled | **Confirmed tech debt** | `src/dialogs/prefs/prefs.rs` (emits event), no handler in `main.rs`/`main_events.rs` | Event is redundant (backend switching happens per-frame anyway) and confuses dataflow | Remove event or handle it as the single switching mechanism |
| P1-2 | Unused event/type: `PreloadFrameEvent` | **Confirmed tech debt** | `src/core/player_events.rs` | Dead type indicates half-finished preload architecture | Remove or implement; prefer one source of truth for preload requests |
| P1-3 | Unused type: `PreloadStrategy` | **Confirmed tech debt** | `src/core/cache_man.rs` | Implies planned forward/spiral selection, but code doesn’t use it | Remove or wire into preload logic (video vs image sequences) |
| P1-4 | Legacy stubs remain (`CompNode::emit_attrs_changed`, etc.) | **Confirmed tech debt** | `src/entities/comp_node.rs` (`// --- Stubs for legacy API ---`) | Confuses current dirty/event model and can cause missed invalidation if relied on | Replace stub usage in UI with current canonical path (`Project::modify_comp` / `SetLayerAttrsEvent`) |
| P1-5 | `Project::rebuild_runtime` takes unused `CompEventEmitter` | **Confirmed tech debt** | `src/entities/project.rs` (`rebuild_runtime`) | Dead parameter suggests lost refactor pieces | Remove parameter or reintroduce real purpose |
| P1-6 | `src/README.md` documents `src/bin/*` that doesn’t exist | **Confirmed docs drift** | `src/README.md`, repo tree | Misleads contributors; `src/shell.rs` looks unused because binaries are missing | Either restore debug binaries or update docs and move `shell.rs` behind examples/feature |
| P2-1 | Frame clone + in-place mutation could corrupt cached frames | **Latent risk (not active today)** | `src/entities/frame.rs` (`Frame` is `Arc<Mutex<FrameData>>`), `src/entities/frame.rs` (`crop` mutates), `src/entities/compositor.rs` (calls `crop`) | Today the hot path appears safe because the crop is a no-op (dimensions match). But this is a footgun for future compositor wiring | Harden by avoiding in-place mutations on shared clones; use `crop_copy` where needed; document clone semantics |
| P2-2 | `crossbeam-channel` dependency appears unused | **Confirmed tech debt** | `Cargo.toml` + no `crossbeam_channel` usage in repo | Extra dependency; could be used for REST server implementation | Either remove or adopt it as the canonical channel for REST thread commands |
| F-1 | CameraNode exists but not integrated into comp rendering | **Confirmed feature stub** | `src/entities/camera_node.rs` + no camera usage in `CompNode::compose_internal` | Users can create camera nodes, but they don’t affect output | Decide whether to hide/mark experimental or implement camera-driven pipeline |

---

## 3) Dedup: One Source of Truth Principles

### 3.1 One canonical invalidation path
- Keep `AttrsChangedEvent` as the single trigger for epoch bump + cache invalidation + preload.
- Consolidate handler logic into one function used by both main and derived event loops.

### 3.2 One canonical settings application path
- Apply settings in one place each frame (or in one event-driven handler) to avoid “only when Settings window is open” effects.

### 3.3 One canonical composition backend decision
- Either:
  - (A) backend choice affects comp output → must be part of `CompNode` compute/blend, or
  - (B) backend choice is viewport-only → rename setting and scope it explicitly.

---

## 4) Roadmap (Workstreams)

### Workstream A — Correctness & Persistence (P0)

**A1. Fix cache strategy after rebuild/load**
- Apply `AppSettings.cache_strategy` immediately after any `Project::rebuild_with_manager` call.
- Acceptance criteria:
  - If settings say `LastOnly`, after restart/load the cache uses `LastOnly` without opening Settings.
  - Add a small unit test around `rebuild_with_manager`/`set_strategy` (if feasible).

**A2. Normalize “apply settings”**
- Create a single helper (e.g., `PlayaApp::apply_runtime_settings(...)`) which:
  - applies compositor backend (if kept)
  - applies cache strategy
  - applies memory limit
- Acceptance criteria:
  - No setting depends on “Settings window opened” to take effect.

### Workstream B — Compositing Backend Truthfulness (P0/P1)

**B1. Decide scope of GPU compositor setting**
- Option B1a (small, immediate): make UI copy accurate: “GPU compositor is currently viewport-only / experimental / not wired to comp output”.
- Option B1b (recommended long-term): wire compositor selection into the blend step.

**B2. If wiring compositor selection: pick a feasible architecture**
- Workers currently have no GL context → GPU compositing cannot happen in worker threads.
- Two realistic approaches:
  1) **Viewport-only GPU composition**:
     - Workers preload only FileNode frames.
     - Main thread composes current frame on-demand using GPU compositor (requires cache-only inputs).
     - Encoding continues using CPU (or uses a separate offline pipeline).
  2) **CPU stays for workers; GPU only accelerates preview blend**:
     - Keep current CPU composition in cache for playback.
     - Use GPU compositor only when user toggles an “accelerated preview” option.

Acceptance criteria (for any “wired” interpretation):
- Changing backend changes actual blending path for at least one visible feature (multi-layer comp), not just logs.

### Workstream C — Legacy Cleanup (P1)

**C1. Remove or rewire unused events and stubs**
- Remove `CompositorBackendChangedEvent` or route backend switching through it.
- Remove or implement `PreloadFrameEvent`/`PreloadStrategy`.
- Replace AE codepaths that call legacy stubs (`emit_attrs_changed`) with canonical event-driven updates.

**C2. Docs alignment**
- Update `src/README.md` to match actual repo layout OR restore the missing debug binaries.

### Workstream D — REST API Server (New Feature)

Goal: local HTTP control interface without blocking UI.

**Design constraints (to preserve current architecture):**
- REST thread must not mutate app state directly.
- REST requests should enqueue commands/events, processed by main thread.

**Minimal recommended design (deduped from `claude_plan.md` + `rest.md`):**
- Add `src/core/rest/` module:
  - `commands.rs`: `RestCommand` enum (Play/Pause/Stop/SetFrame/Status/etc.)
  - `server.rs`: blocking HTTP server loop
- Communication:
  - use a channel (either `crossbeam-channel` or `std::sync::mpsc`) to send `RestCommand` to main thread.
  - main thread converts commands to EventBus events (`TogglePlayPauseEvent`, `SetFrameEvent`, …) to keep one codepath.

**Suggested v1 endpoints (keep small):**
- `POST /api/v1/play`
- `POST /api/v1/pause`
- `POST /api/v1/toggle`
- `POST /api/v1/stop`
- `POST /api/v1/frame?frame=<i32>`
- `POST /api/v1/step?count=<i32>`
- `GET  /api/v1/status`
- `GET  /api/v1/media` (read-only listing)

**Security defaults:**
- Disabled by default.
- Bind to `127.0.0.1` by default.
- Remote access requires explicit opt-in (and ideally an API key).

Acceptance criteria:
- Server thread does not block UI.
- Status endpoint reflects real state.
- Commands go through EventBus, not direct mutation.

### Workstream E — 3D Transforms & Camera (New Feature, Large)

This is a major feature area; current code supports only 2D transforms in practice:
- Gizmo tools operate on Translate X/Y, Rotate Z, Scale X/Y.
- CPU transform function uses `rotation_z` only.
- `CameraNode` exists but is not used by `CompNode::compose_internal`.

**Validated current capabilities:**
- Layer attributes already store `Vec3` for position/rotation/scale/pivot.
- `CameraNode` provides view/projection matrices.

**Key decision required before implementation:**
- What does “3D” mean for Playa?
  - true perspective projection with camera + depth ordering?
  - or just extra channels (Z, rotX/rotY) for future GPU sampling?

**Incremental plan (safe ordering):**
1) UI-only:
   - Ensure Attribute Editor clearly exposes XYZ channels (it already stores Vec3; verify UI editing intent).
   - Add a “3D experimental” toggle and clarify that preview/render is still 2D until pipeline is wired.
2) Render pipeline:
   - Decide how to represent transforms for sampling: 2D affine (mat3) vs projective (mat4).
   - Decide whether to use GPU compositor (GL) for projective sampling.
3) Camera integration:
   - Define how a camera layer is selected (single active camera per comp? nearest above? explicit setting?).

Acceptance criteria (for “real 3D”):
- Camera affects visible output (perspective change).
- Z ordering is defined and stable.

### Workstream F — Clippy / Quality Gate (P1)

- Current state: `cargo clippy -p playa --all-targets -- -D warnings` fails; `xtask` also fails.
- Target:
  - `cargo clippy --workspace --all-targets -- -D warnings` passes.

Recommendation:
- Do this after P0 fixes, but before large refactors.

---

## 5) Proposed Milestones (No Duplication)

### Milestone M0 (P0) — Settings correctness
- Deliverables:
  - cache strategy applied after rebuild/load
  - centralized settings application helper

### Milestone M1 (P0/P1) — Compositing backend truthfulness
- Deliverables:
  - either: UI copy/behavior matches actual usage
  - or: first real wiring of backend into a visible codepath

### Milestone M2 (P1) — Legacy cleanup + docs alignment
- Deliverables:
  - no legacy stub calls from UI paths
  - `src/README.md` matches repo layout (or debug bins restored)

### Milestone M3 (Feature) — REST API v1
- Deliverables:
  - local-only server with play/pause/seek/status/media
  - settings section to enable/disable

### Milestone M4 (Feature) — 3D roadmap decision
- Deliverables:
  - explicit definition of 3D scope
  - chosen technical approach (CPU vs GPU, mat3 vs mat4)

---

## 6) Open Questions (Need answers before “big” work)

1) Should GPU compositing accelerate:
   - preview only,
   - background preloading,
   - encoding,
   - or all of the above?

2) For REST API:
   - local-only is enough, or do you want remote LAN control?
   - do we need an API key from day 1?

3) For 3D:
   - do we need real camera/perspective rendering now, or is it “data plumbing first”?

4) Do you want to restore standalone debug binaries (`src/bin/*`) or remove the doc references and repurpose `src/shell.rs` as an example/feature-gated module?

---

## 7) Notes on Validation (What changed after double-check)

- The “Frame clone + crop → cache corruption” claim is **not currently triggered in the main composition path** because:
  - the crop call in `CpuCompositor::blend_with_dim` is effectively a no-op for the inserted base frame (dimensions already match).
  - there are no other active call sites using CPU compositor with a cached frame as the base.
- It remains a **latent risk** if/when `Project.compositor` is wired into composition or if blend is used elsewhere.

---

## Appendix A — Source Merge Notes (Dedup decisions)

- REST API content: `rest.md` and the REST part of `claude_plan.md` overlapped heavily. This megaplan keeps a single endpoint list and a single architecture.
- 3D transforms content: kept the validated “current state” analysis, but removed file-path assumptions that don’t match the repo (e.g., there is no `src/widgets/attr_editor.rs`; the Attribute Editor is in `src/widgets/ae/ae_ui.rs`).
- `plan_cdx.md` checkboxes are represented as milestones/workstreams here to avoid duplication.

---

## Appendix B — REST Endpoint Inventory (Merged, No Duplicates)

This appendix preserves the superset of unique endpoint ideas from `rest.md` + `claude_plan.md`, grouped by stability.

### B1) Recommended v1 (small, high-value)
- `POST /api/v1/play`
- `POST /api/v1/pause`
- `POST /api/v1/toggle`
- `POST /api/v1/stop`
- `POST /api/v1/frame?frame=<i32>`
- `POST /api/v1/step?count=<i32>`
- `GET  /api/v1/status`
- `GET  /api/v1/media`

### B2) Optional v1.1 (still low risk)
- `GET  /api/v1/frame` (read current frame index)
- `GET  /api/v1/project` (basic project info: path/modified)
- `GET  /api/v1/comps` (list comps)
- `GET  /api/v1/comps/{uuid}` (comp details including layers)
- `GET  /api/v1/media/{uuid}` (media/node details)
- `POST /api/v1/jog?direction=<i32>` (JKL-style jog/shuttle control)

### B3) Explicitly future (requires new capabilities)
- `GET /api/v1/frame.png` (render current frame as PNG)
  - Needs a defined “current rendered frame” source (cache vs viewport render) and a PNG encoder path.
- Mutating graph endpoints (layer transforms/visibility/keyframes)
  - Must be mapped to canonical EventBus commands; avoid direct mutation from REST thread.

---

## Appendix C — 3D Transforms & Camera Notes (Merged)

### C1) Verified current state
- Data model already stores `Vec3` for `position`, `rotation`, `scale`, `pivot` (schema supports XYZ).
- Viewport gizmo is effectively 2D today (Translate X/Y, Rotate Z, Scale X/Y).
- CPU transform path uses `rotation_z` (2D affine); 3D rotation channels are currently unused for rendering.
- `CameraNode` provides valid `view_matrix()` / `projection_matrix()` / `view_projection_matrix()` but returns `None` from `Node::compute()` and is not consulted by `CompNode::compose_internal`.

### C2) Camera integration decision points (before any “real 3D”)
- Camera selection rule (single active camera per comp? explicit reference? nearest above?).
- Transform math representation for sampling:
  - keep 2D (`mat3` / `Affine2`) and treat Z as metadata, or
  - projective 3D (`mat4`) + perspective-correct sampling + defined Z ordering.

### C3) Practical incremental path (keeps system stable)
1) UI clarity: expose XYZ channels consistently, but label non-wired channels as “not affecting render yet” unless wired.
2) Decide GPU vs CPU for projective sampling (GPU is the realistic path for perspective resampling).
3) Wire camera only after the sampling path is decided (otherwise “camera” will be metadata-only again).

---

## Appendix D — Verification Ledger (Double-check Log)

This is a minimal “what was checked” log to avoid regressions after context compaction.

1) `cache_strategy` persistence
   - Checked: `src/entities/project.rs` `rebuild_with_manager` creates `GlobalFrameCache` with `CacheStrategy::All`.
   - Conclusion: confirmed bug (setting can be persisted but not applied on rebuild).

2) GPU compositor backend vs actual comp output
   - Checked: `src/entities/comp_node.rs` always blends via `THREAD_COMPOSITOR` (CPU).
   - Checked: `src/entities/project.rs` `compute_frame` is a cache read; it does not run blending.
   - Conclusion: confirmed mismatch (backend selection currently does not change comp frames).

3) “Frame clone + crop corrupts cache”
   - Checked: `CpuCompositor::blend_with_dim` crops `base_frame.clone()`.
   - Checked: `CompNode::compose_internal` inserts a freshly created base frame before blending (`create_base_frame(dim, ...)`), so the cropped clone is not a cached frame and the crop is typically a no-op (dims match).
   - Conclusion: not an active bug today; keep as latent risk if compositor wiring changes.

4) Dead/unused items from partial refactor
   - Checked: `CompositorBackendChangedEvent` exists and is emitted in prefs, but no handler found.
   - Checked: `PreloadFrameEvent` and `PreloadStrategy` exist with no uses beyond definitions/re-exports.
   - Conclusion: confirmed tech debt; remove or complete the design.

5) Legacy stub usage in UI paths
   - Checked: `src/main.rs` calls `comp.emit_attrs_changed()` after Attribute Editor changes.
   - Checked: `CompNode::emit_attrs_changed` is explicitly labeled as a legacy stub.
   - Conclusion: confirmed tech debt; should be replaced with canonical event-driven invalidation.
