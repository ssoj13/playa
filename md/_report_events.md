# Playa Event System Audit Report

**Audited files:** 13 source files (main_events.rs read in full across multiple chunks)
**Date:** 2026-03-18

---

## 1. Critical Issues

### 1.1 `ApiCommand::Play` double-emits `TogglePlayPauseEvent`
**File:** `src/app/api.rs`, lines 90–95

```rust
event_bus.emit(TogglePlayPauseEvent);          // always emitted
if !player.is_playing() {
    event_bus.emit(TogglePlayPauseEvent);      // emitted again if paused
}
```

The first emit is deferred. Because `is_playing()` reads state **before** the first event is processed, it always reflects the pre-event state. When the player is paused: emits twice → toggles back to paused. The API `play` command effectively does nothing on first call if player was paused.

Fix: emit once only. If the intent is "ensure playing", use `PlayEvent` / check after processing, or emit a dedicated `EnsurePlayingEvent`.

---

### 1.2 `SetFrameEvent` causes double preload per scrub tick
**File:** `src/main_events.rs`, lines ~243–265; `src/app/events.rs`

`SetFrameEvent` handler:
1. Calls `comp.set_frame(e.0)` inside `modify_comp()` → `modify_comp()` emits `CurrentFrameChangedEvent` (deferred).
2. Handler also sets `result.enqueue_frames = true` directly.

Then `CurrentFrameChangedEvent` handler in `app/events.rs` calls `enqueue_frame_loads_around_playhead()` again.

Result: every scrub event triggers two separate preload enqueue operations in the same logical action. During fast scrubbing this floods the preload queue.

Fix: remove `result.enqueue_frames = true` from `SetFrameEvent` handler and rely solely on `CurrentFrameChangedEvent` to drive preloading.

---

### 1.3 Silent error discards with `let _ =`
Multiple locations silently swallow errors with no user feedback:

| File | Line(s) | Operation |
|------|---------|-----------|
| `src/app/run.rs` | ~147 | `let _ = self.load_sequences(dropped)` — drag-and-drop load |
| `src/app/events.rs` | ~244 | `let _ = self.load_sequences(paths)` — `AddClipEvent` load |
| `src/app/api.rs` | ~114 | `let _ = self.load_sequences(...)` — API load |
| `src/runner.rs` | ~202 | `let _ = app.load_sequences(all_files)` — CLI load |
| `src/core/player.rs` | attrs remove | `let _ = self.attrs.remove("active_comp")` |

All `load_sequences` silences are especially bad: the user drags files onto the app and gets no indication of failure. Errors should propagate to the status bar or log at minimum.

---

### 1.4 `deferred_load_sequences` overwrites instead of accumulates
**File:** `src/app/events.rs`, lines ~165–167

```rust
deferred_load_sequences = Some(paths);
```

If two `AddClipEvent`s arrive in the same frame (e.g. rapid API calls), only the last batch's paths are kept. The first batch is silently dropped.

Fix: accumulate — `deferred_load_sequences.get_or_insert_with(Vec::new).extend(paths)`.

---

### 1.5 EventBus queue evicts silently on overflow
**File:** `src/core/event_bus.rs`

When the deferred queue exceeds 1000 events, 500 events are evicted with only a `warn!()`. There is no mechanism for callers to detect this loss. In high-throughput scenarios (rapid scrubbing, API spam) events driving UI updates or preloads are dropped without recovery.

This is not just a performance issue — evicted `CurrentFrameChangedEvent` or `AttrsChangedEvent` means the viewport can get stuck showing a stale frame with no recovery path short of triggering another event.

---

## 2. Performance Improvements

### 2.1 Dock state serialized to JSON twice per frame
**File:** `src/app/run.rs`, lines ~189–203

```rust
let before = serde_json::to_string(&dock_state).ok();
// ... render ...
let after = serde_json::to_string(&dock_state).ok();
if before != after { ... }
```

Full JSON serialization of the entire dock tree happens twice every frame for change detection. Dock state changes are user-driven and rare, but this runs at 24–60+ fps unconditionally.

Fix: use a dirty flag on dock state mutations (tab drag, split, close), or compare a cheaper hash/version counter. Alternatively use egui's `Response::changed()` on the `DockArea`.

---

### 2.2 Full egui style clone every frame for font size
**File:** `src/app/run.rs`, lines ~78–82

```rust
let mut style = (*ctx.style()).clone();
style.text_styles.insert(...font_size...);
ctx.set_style(style);
```

Clones and re-applies the full style struct every frame. Font size does not change unless the user modifies it in settings.

Fix: cache the last applied font size, apply `ctx.set_style()` only when it changes.

---

### 2.3 `ctx.options_mut()` called every frame
**File:** `src/app/run.rs`, line ~100–102

`ctx.options_mut(|opts| opts.max_passes = ...)` acquires an internal lock and writes every frame. This value is constant at runtime.

Fix: set it once during initialization.

---

### 2.4 Theme applied unconditionally every frame
**File:** `src/app/run.rs`, lines ~71–75

`apply_theme()` (or equivalent) runs every frame with no dirty-flag guard.

Fix: track last applied theme variant in `AppSettings`, skip if unchanged.

---

### 2.5 Player state stored in `Attrs` key-value map
**File:** `src/core/player.rs`

Every access to `fps_base`, `fps_play`, `is_playing`, `loop_enabled`, `play_direction`, `active_comp` performs a string key lookup through the `Attrs` map. This runs inside `update()` called every frame during playback.

While individual lookups are cheap, the cumulative overhead during playback (multiple `get_*` calls per frame) is unnecessary allocation and indirection compared to typed struct fields.

Fix: migrate player state to typed `struct` fields. Keep `Attrs` only for persistent/serialized attributes that truly need generic storage.

---

### 2.6 Repeated lock acquisition in `player.update()` hot path
**File:** `src/core/player.rs`

`update()` calls `total_frames()` and inside `advance_frame()` calls `play_range()`, each acquiring a `read()` lock on `project.media`. These are separate lock acquisitions on the same call path within a single frame tick. The read lock should be taken once per frame update.

---

## 3. Code Deduplication

### 3.1 `EventBus` and `EventEmitter` are byte-for-byte duplicates
**File:** `src/core/event_bus.rs`

`EventEmitter` has 4 methods: `emit()`, `subscribe()`, `unsubscribe_all()`, `poll()` — all delegating to an inner `Arc<EventBus>` with identical logic to calling `EventBus` directly. There is no value in the wrapper struct except hiding the `Arc`. The duplication means any bug fix or change to `EventBus::emit()` must be replicated in `EventEmitter::emit()`.

Fix: make `EventEmitter` a thin newtype `struct EventEmitter(Arc<EventBus>)` with `Deref<Target = EventBus>`, eliminating all method duplication.

---

### 3.2 `AttrsChangedEvent` handling duplicated in main loop and derived loop
**File:** `src/app/events.rs`, lines ~67–84 and ~214–226

Identical handling logic copy-pasted for the main event poll loop and the derived events loop. Any change must be made in two places.

Fix: extract to a helper `fn handle_attrs_changed(...)` called from both loops.

---

### 3.3 `RemoveMediaEvent` and `RemoveSelectedMediaEvent` near-identical handlers
**File:** `src/main_events.rs`, lines ~414–443

Both handlers: find the removed comp(s), check if `active_comp` was among them, pick the next active comp, stop playback, switch active comp. The only difference is how the set of removed UUIDs is determined (explicit UUID vs. selected media list).

Fix: extract `fn handle_media_removal(removed: &[Uuid], ...)`.

---

### 3.4 `AlignLayersStartEvent` and `AlignLayersEndEvent` are structurally identical
**File:** `src/main_events.rs`, lines ~955–993

Both handlers iterate `layer_selection`, compute delta to `current_frame`, and call `move_child`. The only difference is which bound (`play_start` vs `play_end`) is used.

Fix: extract `fn align_layers_to_frame(comp, anchor: Bound)`.

---

### 3.5 `SetLayerPlayStartEvent` and `SetLayerPlayEndEvent` are structurally identical
**File:** `src/main_events.rs`

Same pattern: get dragged UUID, compute delta, call `trim_layers` with `A_IN` or `A_OUT`. Single helper parameterized on the trim anchor.

---

### 3.6 Playlist loading in `runner.rs` duplicates `load_project()` in `project_io.rs`
**File:** `src/runner.rs`, lines ~206–242 vs `src/app/project_io.rs`

The runner's playlist loading block reconstructs nearly identical logic to `load_project()`: deserialize JSON, rebuild compositor, attach emitter, set player state. If `load_project()` is updated, the runner path will silently diverge.

Fix: unify via `load_project()` with an optional path override, or extract shared initialization logic.

---

### 3.7 Timeline fit variants (`TimelineFitEvent`, `TimelineFitWorkAreaEvent`) duplicate zoom calc
**File:** `src/main_events.rs`, lines ~600–650

Three timeline fit handlers (`TimelineFitAllEvent`-equivalent, `TimelineFitEvent`, `TimelineFitWorkAreaEvent`) all compute:
```rust
let pixels_per_frame = canvas_width / duration as f32;
let zoom = (pixels_per_frame / default_ppf).clamp(0.1, 20.0);
timeline_state.zoom = zoom;
timeline_state.pan_offset = min_frame as f32;
```
verbatim.

Fix: `fn fit_timeline_to_range(state, canvas_width, min_frame, max_frame)`.

---

## 4. Architecture Issues

### 4.1 `handle_app_event()` god function with 16 parameters
**File:** `src/main_events.rs`

Signature takes: `event`, `project`, `player`, `settings`, `comp_event_emitter`, `timeline_state`, `node_editor_state`, `viewport_state`, `cache_manager`, `debounced_preloader`, `event_bus`, `encoder`, `screenshot_manager`, `status`, `layout_state`, `modal_state` (16 parameters, approximate count).

This function handles every event in the application. It is 1232 lines. Any new event handler adds more parameters or touches shared mutable state without any invariant enforcement.

Fix: group parameters into a `AppEventContext<'_>` struct. This alone doesn't reduce coupling but removes the parameter explosion and enables passing context through sub-handlers without signature changes. Long term: domain-split into `PlaybackEventHandler`, `LayerEventHandler`, etc., each owning their relevant context slice.

---

### 4.2 Dual source of truth for loop state
**File:** `src/main_events.rs`, `ToggleLoopEvent` handler

`ToggleLoopEvent` writes to both `settings.loop_enabled` AND `player.set_loop_enabled()`. The player and settings each maintain their own loop flag. On startup, which source wins depends on initialization order. A mismatch results in the UI showing the wrong state.

Fix: single source of truth — either read from `settings.loop_enabled` at playback time and remove the player field, or persist the player field and remove it from settings.

---

### 4.3 Two parallel layout serialization systems
**File:** `src/app/layout.rs`, `src/core/layout_events.rs`

**System A** (legacy): `SaveLayoutEvent`/`LoadLayoutEvent` → `save_layout_to_attrs` / `load_layout_from_attrs` → stores layout in project attrs. Marked "legacy, kept for compatibility" in `layout_events.rs` but still subscribed and active.

**System B** (current): `capture_current_layout` / `apply_layout` → stores in `AppSettings` named layouts.

Both serialize the same `dock_state` to different stores. On project load, which layout wins is non-deterministic if both stores have data. The legacy system silently uses `serde_json::to_string(...).unwrap_or_default()` — serialization failure produces an empty string that will be mistaken for "no saved layout."

Fix: complete the migration — remove `SaveLayoutEvent`/`LoadLayoutEvent` handlers and the `save_layout_to_attrs`/`load_layout_from_attrs` functions. Add a one-time migration on project load that moves legacy attrs layout data into AppSettings if present.

---

### 4.4 `ApiCommand::SetFps` bypasses the event bus
**File:** `src/app/api.rs`, lines ~107–109

```rust
self.player.set_fps_base(fps);  // direct call
```

The rest of FPS changes go through `IncreaseFPSBaseEvent`/`DecreaseFPSBaseEvent` which call `adjust_fps_base()`, which also updates the active comp's FPS attribute. The direct API path does NOT update the comp FPS, so the active comp and player can have diverged FPS state after an API `setfps` call.

Fix: `ApiCommand::SetFps` should emit the same event that the keyboard shortcut emits, or call the same `adjust_fps_base()` path.

---

### 4.5 `AppSettings::default()` instantiated twice in `PlayaApp::default()`
**File:** `src/app/mod.rs`, lines ~196–198

`AppSettings::default()` is called for the `settings` field, then immediately called again to read `settings.cache_strategy` for project initialization. The second instance is used for one field and discarded.

Fix: read `cache_strategy` from the already-constructed `settings`.

---

### 4.6 `HoverLayerEvent` may trigger spurious cache invalidation
**File:** `src/main_events.rs`

`HoverLayerEvent` calls `modify_comp()`. If the comp happens to be dirty at the moment of hover (e.g., from a prior operation not yet flushed), `modify_comp()` will emit `AttrsChangedEvent`, which increments the cache epoch, invalidating the frame cache. Hover state carries no renderable data and should not be routable through `modify_comp()`.

Fix: store hover state outside the comp (e.g., in `timeline_state`) and bypass `modify_comp()` entirely for hover.

---

### 4.7 `load_project()` does not emit `ViewportRefreshEvent`
**File:** `src/app/project_io.rs`

After loading a project, the viewport refresh relies on `mark_dirty()` propagating an `AttrsChangedEvent` on the next frame tick. There is no explicit `ViewportRefreshEvent` emitted. If the comp is not dirty (loaded clean state), the first frame may display nothing until user interaction triggers a re-render.

Fix: emit `ViewportRefreshEvent` at the end of `load_project()`.

---

### 4.8 `render_multi_node_attributes()` bypasses event bus
**File:** `src/app/tabs.rs`

Multi-node attribute changes apply via direct `modify_node()` calls and then call `invalidate_and_refresh()` (increment epoch + `enqueue_current_frame_only()`). The single-node path routes through the event system via `AttrsChangedEvent`. The two paths have diverged: the multi-node path does not fire `AttrsChangedEvent`, meaning any subscriber listening for attribute changes (e.g., a future undo system) will not be notified for multi-selection edits.

Fix: unify through `AttrsChangedEvent` emission, or document the bypass as intentional with a comment explaining why.

---

### 4.9 `ae_focus` (Vec<Uuid>) cloned every frame in attributes tab
**File:** `src/app/tabs.rs`, line ~183

`ae_focus.clone()` on every frame render of the attributes tab. `Vec<Uuid>` clone is cheap in isolation but unnecessary if the selection hasn't changed.

Fix: compare length/content before cloning, or store as `Arc<Vec<Uuid>>` and clone the Arc.

---

### 4.10 Derived events loop `ViewportRefreshEvent` re-entrance
**File:** `src/app/events.rs`

`AttrsChangedEvent` handler emits `ViewportRefreshEvent`. If `AttrsChangedEvent` arrives during the derived events loop (iteration 2–10), `ViewportRefreshEvent` goes back into the queue and consumes an additional derived loop iteration. With 10 maximum iterations, a chain of `AttrsChanged → ViewportRefresh → (re-queued)` can consume half the derived loop budget.

Fix: handle `ViewportRefreshEvent` in the main loop only and skip re-queuing it during derived iterations, or use a simple boolean flag `needs_viewport_refresh` rather than an event for this purpose.

---

### 4.11 `move_child()` return value silently discarded in layer alignment handlers
**File:** `src/main_events.rs`, lines ~955–993 (`AlignLayersStartEvent`, `AlignLayersEndEvent`, `MoveLayerEvent`)

```rust
let _ = comp.move_child(layer_idx, layer_in + delta);
```

`move_child()` returns a `Result`. The error is silently discarded. If the operation fails (e.g., invalid index), the layer is not moved and the user gets no feedback.

Fix: log the error or propagate via `result.error_message`.

---

## 5. Recommendations

**Priority 1 — Fix immediately (correctness bugs):**
1. Fix `ApiCommand::Play` double-emit (§1.1) — breaks the API play command.
2. Fix `deferred_load_sequences` overwrite (§1.4) — silently drops file batches.
3. Fix `ApiCommand::SetFps` bypassing `adjust_fps_base()` (§4.4) — diverges player/comp FPS.
4. Replace `let _ =` on all `load_sequences()` calls (§1.3) — user gets no error feedback.

**Priority 2 — Performance (frame budget):**
5. Eliminate dual `serde_json::to_string` dock state comparison per frame (§2.1). Use a dirty flag.
6. Cache last applied font size, skip `ctx.set_style()` when unchanged (§2.2).
7. Remove double preload in `SetFrameEvent` handler (§1.2).
8. Move `ctx.options_mut(max_passes)` to initialization (§2.3).

**Priority 3 — Deduplication:**
9. Make `EventEmitter` a newtype with `Deref<Target = EventBus>` (§3.1).
10. Extract `handle_attrs_changed()` helper to eliminate copy-paste in `app/events.rs` (§3.2).
11. Extract `fit_timeline_to_range()` for the three identical timeline fit handlers (§3.7).
12. Extract `handle_media_removal()` for `RemoveMediaEvent`/`RemoveSelectedMediaEvent` (§3.3).

**Priority 4 — Architecture:**
13. Group `handle_app_event()` parameters into `AppEventContext<'_>` struct (§4.1).
14. Choose single source of truth for loop state — remove it from either `Player` or `AppSettings` (§4.2).
15. Complete layout system migration: remove legacy `SaveLayoutEvent`/`LoadLayoutEvent` (§4.3).
16. Route `HoverLayerEvent` through `timeline_state` instead of `modify_comp()` (§4.6).
17. Emit `ViewportRefreshEvent` at end of `load_project()` (§4.7).
18. Unify single-node and multi-node attribute edit paths through `AttrsChangedEvent` (§4.8).
19. Address EventBus queue overflow: consider a bounded strategy that rejects (not silently evicts) or back-pressures (§1.5).
20. Add per-subscriber handles to `EventBus` for precise `unsubscribe()` without clearing all subscribers of a type (currently only `unsubscribe_all<E>()` exists).
