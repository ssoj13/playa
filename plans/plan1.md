# Playa — Bug Hunt & Architecture Report (Codex)

Date: 2025-12-18

Scope: code audit for illogical places, unfinished refactors, dead/unused code, dedup opportunities, dataflow mapping, and production-grade improvement plan. No functionality was removed in this audit.

## Executive Summary

### Critical correctness issues (fix first)
1) **Cache corruption risk via `Frame` shallow clones + in-place crop in CPU compositing**
- `Frame` is `#[derive(Clone)]` but clone is **shallow** (`Arc<Mutex<FrameData>>`), so clones share pixels and metadata.
- `CpuCompositor::blend_with_dim` clones the first frame and then calls `Frame::crop(&self, ...)`, which mutates the shared `FrameData` in-place.
- This can mutate cached frames and break both correctness and cache memory accounting.
- Evidence:
  - `src/entities/frame.rs:123` (`Frame { data: Arc<Mutex<FrameData>> }`)
  - `src/entities/frame.rs:883` (`pub fn crop(&self, ...)` mutates internal buffer)
  - `src/entities/compositor.rs:282` (`result.crop(...)` on `result = base_frame.clone()`)

2) **`cache_strategy` setting is not applied after deserialization / rebuild**
- `Project::rebuild_with_manager` recreates `GlobalFrameCache` with `CacheStrategy::All` unconditionally.
- On startup restore and project/playlist load, cache strategy will effectively reset to `All` unless user opens Settings and changes strategy.
- Evidence:
  - `src/entities/project.rs:405` (`pub fn rebuild_with_manager`)
  - `src/entities/project.rs:417` (hard-coded `CacheStrategy::All`)

### High-value architecture inconsistencies (next)
- **GPU compositor backend is not wired into the actual composition pipeline**:
  - Settings UI selects `CompositorBackend`, and `main.rs` switches `Project.compositor` (`src/main.rs:842`).
  - Actual composition happens in workers via `CompNode::compose_internal`, which always uses a thread-local CPU compositor (`THREAD_COMPOSITOR`).
  - Viewport fetches frames using `Project::compute_frame` which is cache-only (`src/entities/project.rs:466`).
  - Net effect: compositor backend selection currently has **no effect** on the produced comp frames.
  - Evidence:
    - `src/main.rs:842` (`update_compositor_backend` sets `Project::set_compositor`)
    - `src/entities/project.rs:428` (`set_compositor` exists)
    - `src/entities/comp_node.rs:951` (`THREAD_COMPOSITOR.with(|comp| comp.borrow_mut().blend_with_dim(...))`)
    - `src/entities/project.rs:466` (`compute_frame` is cache read-only)

- **Legacy stubs / unused pathways remain from a larger refactor**:
  - `CompNode` includes explicit legacy API stubs:
    - `src/entities/comp_node.rs:1165` (`// --- Stubs for legacy API ---`)
    - `src/entities/comp_node.rs:1169` (`set_event_emitter` no-op)
    - `src/entities/comp_node.rs:1174` (`emit_attrs_changed` marks dirty only)
  - `CompEventEmitter` is propagated through several signatures but is no longer meaningfully used.
  - `PreloadStrategy` and `PreloadFrameEvent` exist but are unused:
    - `src/core/cache_man.rs:15` (`PreloadStrategy`)
    - `src/core/player_events.rs:76` (`PreloadFrameEvent`)

### Hygiene baseline
- `cargo test -p playa` passes.
- `cargo clippy -p playa --all-targets -- -D warnings` fails with many actionable clippy issues.
- `cargo clippy --workspace --all-targets -- -D warnings` fails (also in `xtask`).

## Verified Runtime Dataflow (Current Code)

### A) Event processing & invalidation (main thread)

```
User input (egui) / widget actions
  -> EventBus.emit[_boxed]()
     - immediate callbacks
     - enqueues BoxedEvent

eframe::App::update()
  -> player.update() (playback)
     -> emits SetFrameEvent(new_frame)
  -> handle_events():
       poll() events
       - Comp events (high priority):
           CurrentFrameChangedEvent -> enqueue preloads
           LayersChangedEvent       -> epoch++ ; clear cache range/comp
           AttrsChangedEvent        -> epoch++ ; clear comp cache ; enqueue current frame only ; debounce full preload ; emit ViewportRefreshEvent
           ViewportRefreshEvent     -> viewport_state.request_refresh()
           ClearCacheEvent          -> epoch++ ; clear_all ; ViewportRefreshEvent
       - delegate remaining to main_events::handle_app_event(...)
         -> deferred actions executed after loop
       - derived-events loop (up to 10 iterations):
         processes AttrsChangedEvent/ViewportRefreshEvent emitted during handling in the same frame
  -> DebouncedPreloader.tick(): if elapsed -> enqueue preloads
  -> CacheManager.take_dirty(): if cache changed -> request_repaint
  -> render panels
  -> handle_keyboard_input (after hover states)
```

Evidence:
- `src/main.rs:346` (`handle_events`)
- `src/main.rs:486` (derived-events loop comments and implementation)
- `src/main.rs:1460` (debounced preload tick + repaint if cache dirty)

### B) Preloading & composition (workers)

```
enqueue_frame_loads_around_playhead(radius)
  -> active comp_uuid
  -> CompNode::signal_preload(&Workers, &Project, radius)
       builds ComputeContext { cache_arc, media_arc, workers: Some(..), epoch }
       -> CompNode::preload(center, radius, &ctx)
            for frame_idx in spiral around center:
              workers.execute_with_epoch(epoch, job)

worker job:
  -> cache.get_status(comp_uuid, frame_idx)
       if Loaded/Loading => skip
  -> snapshot Project.media: HashMap<Uuid, Arc<NodeKind>>
  -> CompNode::compute(frame_idx, &ComputeContext{ cache, media, workers: None })
       -> compose_internal:
            - for each layer: compute source frames via Node::compute
              - FileNode::compute does disk IO in the worker thread
            - apply CPU transforms when needed
            - blend via THREAD_COMPOSITOR (CPU)
       -> cache.insert(comp_uuid, frame_idx, composed)
       -> clears dirty flags
```

Evidence:
- `src/main.rs:328` (`enqueue_frame_loads_around_playhead`)
- `src/entities/comp_node.rs:1183` (`signal_preload`)
- `src/entities/comp_node.rs:1074` (`preload`)
- `src/entities/comp_node.rs:994` (`compute`)
- `src/entities/comp_node.rs:831` (`compose_internal`)
- `src/entities/file_node.rs:127` (`compute` does load + insert)

### C) Viewport frame selection
- Viewport does **not** compute. It reads current frame from cache:
  - `Player::get_current_frame` -> `Project::compute_frame` -> `GlobalFrameCache::get`.
- If not loaded, the UI keeps requesting repaints until `CacheManager.take_dirty()` flips.

Evidence:
- `src/core/player.rs:220` (calls `project.compute_frame(...)`)
- `src/entities/project.rs:466` (`compute_frame` is cache get)

## Findings (Detailed)

### 1) Critical: `Frame` mutation can corrupt cache (must-fix)

Root cause:
- `Frame` is clone-shared (`Arc<Mutex<FrameData>>`). Any method that mutates `FrameData` changes all clones.
- `Frame::crop(&self, ...)` mutates internal buffer and can change memory usage.

Immediate high-risk call site:
- `CpuCompositor::blend_with_dim` clones the base frame, then crops in-place:
  - `src/entities/compositor.rs:282`

Why this is dangerous in this codebase:
- Cached frames are stored by value in `GlobalFrameCache`. A shallow clone is enough to later mutate what the cache points to.
- Cache memory tracking assumes frames are not resized in-place after insertion.

Minimal production-grade fix:
- Replace in-place crop on a shallow clone with `crop_copy(...)` (copy-on-write semantics).
- Add a regression test ensuring `blend_with_dim` does not mutate its input frames.

### 2) Settings/dataflow mismatch: cache strategy resets on rebuild/load

Root cause:
- `Project::rebuild_with_manager` constructs global cache with `CacheStrategy::All`:
  - `src/entities/project.rs:417`

What currently happens:
- Startup restore / playlist load / project load rebuilds runtime cache as `All`.
- Strategy changes are only applied when the Settings window is shown and edited (`main.rs` compares old/new strategy in the Settings UI codepath).

Production-grade fix:
- Thread `CacheStrategy` through rebuild:
  - `Project::rebuild_with_manager(manager, event_emitter, cache_strategy)`
  - or call `global_cache.set_strategy(app.settings.cache_strategy)` immediately after rebuild in startup and load flows.

### 3) Unfinished integration: compositor backend setting is effectively a no-op

What exists:
- UI option: `AppSettings.compositor_backend` (`src/dialogs/prefs/prefs.rs:89`)
- Switcher: `PlayaApp::update_compositor_backend` (`src/main.rs:842`)
- Storage: `Project::set_compositor` (`src/entities/project.rs:428`)

What is actually used for comp blending:
- `CompNode::compose_internal` uses `THREAD_COMPOSITOR` (CPU) (`src/entities/comp_node.rs:951`).

Verified outcome:
- Toggling compositor backend does not change the computed comp frames in cache.

Production-grade options (no feature removal):
- **Option A (incremental):** keep backend setting but mark it as “experimental / not yet wired to comp output”, and prevent misleading claims in UI.
- **Option B (recommended roadmap):** refactor composition into two steps (collect inputs + blend), and allow a compositor backend to be chosen for viewport rendering using cache-only source frames (no disk IO on main thread). Later optimize preloading to avoid wasted CPU compositing when GPU is selected.

### 4) Dead/legacy pathways and documentation drift

- `CompNode` legacy stubs:
  - `src/entities/comp_node.rs:1165`, `src/entities/comp_node.rs:1169`, `src/entities/comp_node.rs:1174`
- Unused events/strategies:
  - `src/core/player_events.rs:76` (`PreloadFrameEvent` unused)
  - `src/core/cache_man.rs:15` (`PreloadStrategy` unused)
  - `src/dialogs/prefs/prefs.rs:50` (`CompositorBackendChangedEvent` emitted but not handled)
- `src/shell.rs` appears unused (no `src/bin/*` present despite `src/README.md` describing it).
- Attribute Editor docs still suggest `Comp::emit_attrs_changed` (`src/widgets/ae/ae_ui.rs` header comments), but the method is a legacy stub.

### 5) Clippy baseline is currently broken (both main crate and xtask)

Repro commands:
- `cargo clippy -p playa --all-targets -- -D warnings`
- `cargo clippy --workspace --all-targets -- -D warnings`

Representative categories (from current output):
- `clippy::question_mark` (`src/core/debounced_preloader.rs`)
- `clippy::explicit_auto_deref` (many schema usages)
- `clippy::type_complexity` (events and gizmo signatures)
- `clippy::should_implement_trait` (tool parsing)
- `dead_code` in tests (event_bus test struct field)
- xtask: `unused_imports`, `manual_flatten`, `needless_bool`, etc.

## Dedup / Single Source of Truth Opportunities

### A) Settings application should be centralized
Currently, some settings are applied:
- every frame (theme/font/compositor backend)
- only when Settings window is shown and edited (cache strategy)

Proposal:
- Add a single `apply_settings(&mut self, ctx, frame)` function called every frame that:
  - applies theme/font (unchanged)
  - applies compositor backend (unchanged)
  - applies cache strategy (new: always keep cache consistent)
  - applies memory limits (already tracked via `applied_mem_fraction`)

### B) Consolidate cache invalidation logic
`AttrsChangedEvent` handling exists twice (main loop + derived loop).

Proposal:
- Introduce `fn handle_attrs_changed(&mut self, comp_uuid: Uuid)` and call it from both loops.

### C) Fix documentation drift as part of refactor
- Update `src/README.md` to match reality (no `src/bin` now).
- Update `widgets/ae` docs to describe the current, correct dirty/event flow.

## Proposed Execution Plan (Staged, Production-Grade)

> Each stage should be a separate PR/commit series for review.

### Stage 0 — Correctness hotfixes (must-do)
- [ ] Fix CPU compositor crop to avoid mutating shared cached frames (`src/entities/compositor.rs:282` -> use `crop_copy`).
- [ ] Add regression test demonstrating the bug and preventing reintroduction.
- [ ] Audit for other in-place mutators applied to cached frames (e.g., `crop`, `tonemap`, etc.) and ensure they operate on detached/copy frames when needed.

### Stage 1 — Settings correctness
- [ ] Apply `AppSettings.cache_strategy` on startup restore and on project/playlist loads.
- [ ] Adjust `Project::rebuild_with_manager` to accept a strategy, or apply `global_cache.set_strategy(...)` immediately after rebuild.

### Stage 2 — Remove legacy stubs safely (no feature removal)
- [ ] Remove unused `CompEventEmitter` plumbing from `Project::rebuild_runtime`, `Project::rebuild_with_manager`, `Project::create_comp` if truly unused, or rewire to a meaningful role.
- [ ] Remove/replace `CompNode::emit_attrs_changed` usage in AE panel; rely on schema-based dirty tracking.
- [ ] Remove or properly handle `CompositorBackendChangedEvent` (currently emitted but ignored).

### Stage 3 — GPU compositor roadmap (optional but aligns with settings UI)
- [ ] Refactor `CompNode::compose_internal` into:
  - `collect_source_frames(...)`
  - `blend_frames(compositor_backend, ...)`
- [ ] Implement viewport-only GPU blending using cache-only source frames (no disk IO on main thread).
- [ ] Later optimization: add `FileNode::preload` and a "load-only" graph warmup to avoid doing CPU comp work when GPU is selected.

### Stage 4 — Clippy hardening
- [ ] Fix `playa` crate clippy to pass with `-D warnings`.
- [ ] Fix `xtask` clippy to pass with `-D warnings`.
- [ ] Decide whether to keep `#![allow(clippy::too_many_arguments)]` in `src/lib.rs:5` or factor types.

## Open Questions (Need Your Direction)
1) Should the “GPU compositor backend” be expected to accelerate:
   - (a) viewport rendering only,
   - (b) background preloading (workers),
   - (c) both viewport and encoding?

2) Is it acceptable for the main thread to do some composition work when GPU backend is enabled (with cache-only inputs), or must all composition remain off-thread?

3) Do you want to restore the missing standalone debug binaries (`src/bin/*`) described in `src/README.md`, or should that documentation be updated and `src/shell.rs` moved behind a feature/examples?

---

If you approve, I will start with Stage 0 + Stage 1 (small, high-impact fixes), then iterate on the remaining stages in separate, reviewable chunks.
