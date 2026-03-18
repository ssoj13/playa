# Final Sweep — Missed Issues Report

Date: 2026-03-18
Scope: debounced_preloader, player_events, comp_events, viewport_events, traits, actions, mod.rs files

---

## CRITICAL BUGS

### 1. `ZoomViewportEvent` — wrong semantics in handler (doc/impl mismatch)

File: `src/widgets/viewport/viewport_events.rs` line 4
File: `src/main_events.rs` line 671

The doc comment says "Set zoom level directly" but the handler does:
```rust
viewport_state.zoom *= e.0;
```
This is a **multiply**, not a set. If a caller emits `ZoomViewportEvent(2.0)` expecting to set zoom to 200%, they will instead double the current zoom. The semantics are contradictory and undefined.

Additionally: `ZoomViewportEvent` is **never emitted anywhere in the codebase** (confirmed by full grep). The handler exists, the event is defined, but no code path produces it. This is a dead code path with a logic bug in the handler waiting to be triggered.

**Fix:** Either change the doc to "multiply zoom by factor" and rename the event to `ScaleViewportEvent`, or change the handler to `=` assignment instead of `*=`.

---

### 2. `idx_to_uuid(...).unwrap_or_default()` — silent UUID=0 corruption

Files: `src/main_events.rs` lines 789, 814, 829

Pattern repeated in `MoveAndReorderLayerEvent`, `SetLayerPlayStartEvent`, `SetLayerPlayEndEvent` handlers:
```rust
let dragged_uuid = comp.idx_to_uuid(e.layer_idx).unwrap_or_default();
```

`Uuid::default()` is `Uuid::nil()` (all zeros). If `layer_idx` is out of bounds (stale event from a race between layer removal and drag completion), `dragged_uuid` becomes `Uuid::nil()`. The subsequent lookups (`child_in`, `child_start`, `child_end`) will then silently return `None`/0, and the delta computation will produce incorrect values that get applied to all selected layers via the multi-selection path.

The `SlideLayerEvent` handler at line 846 correctly uses `if let Some(uuid) = comp.idx_to_uuid(...)` and bails out. The three handlers above should do the same.

**Fix:** Replace `unwrap_or_default()` with early return on `None`, same pattern as SlideLayerEvent.

---

## ORPHANED EVENTS (defined, never emitted)

### 3. `PreloadFrameEvent` — fully orphaned

File: `src/core/player_events.rs` lines 73-79

Defined with a comp_uuid + frame_idx, intended for "request to preload a specific frame". Never emitted anywhere. Never handled anywhere. The doc comment says "Sent when a frame is needed but not yet loaded (e.g., during composition)" — but the actual preloading is done via `enqueue_frame_loads_around_playhead()` directly, bypassing this event entirely.

Either implement this event into the preload pipeline, or delete it.

### 4. `ZoomViewportEvent` — never emitted (already noted in #1 above)

File: `src/widgets/viewport/viewport_events.rs` line 18

Zero emit sites found in entire codebase. Dead.

### 5. `ResetViewportEvent` — never emitted

File: `src/widgets/viewport/viewport_events.rs` line 21

Handler exists in `main_events.rs` line 674 (`viewport_state.reset()`). No hotkey binding, no UI button, no emit site found anywhere. The reset functionality is unreachable at runtime.

---

## SEMANTIC / DESIGN ISSUES

### 6. `debounced_preloader.tick()` discards the returned `comp_uuid`

File: `src/app/run.rs` line 132

```rust
if let Some(_comp_uuid) = self.debounced_preloader.tick() {
    self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
}
```

The returned UUID is suppressed with `_comp_uuid`. `enqueue_frame_loads_around_playhead` then independently calls `self.player.active_comp()` to decide what to load. This works correctly only if the active comp hasn't changed between when `schedule()` was called and when `tick()` fires (500ms later).

If the user switches to a different comp within the debounce window, the wrong comp will get the full preload triggered. The scheduled comp_uuid is ignored entirely — the debouncer stores it but no code uses it.

This is a design gap, not a crash, but it means the debounce protection is comp-agnostic when it should be comp-specific.

### 7. `DebouncedPreloader.set_delay()` called every frame

File: `src/app/run.rs` line 131

```rust
self.debounced_preloader.set_delay(self.settings.preload_delay_ms);
```

Called unconditionally in the hot update loop. Harmless (just overwrites a Duration), but wasteful. Should be called only when settings change, or at app init.

### 8. `LayerAttributesChangedEvent` vs `SetLayerAttrsEvent` — two parallel attribute change paths

Files: `src/entities/comp_events.rs` lines 148-166

Two events do similar jobs:
- `LayerAttributesChangedEvent` — carries typed fields (visible, solo, opacity, blend_mode, speed). Used by Timeline outline panel.
- `SetLayerAttrsEvent` — carries generic key-value `Vec<(String, AttrValue)>`. Used by Attribute Editor and Node Editor.

Both have handlers in `main_events.rs` (lines 887 and 907). This is not a bug per se, but it means the same attribute (e.g. opacity) can arrive via two different paths. If the handlers diverge in their post-processing (e.g. one emits `AttrsChangedEvent` and the other doesn't), there will be cache invalidation asymmetry. Worth auditing both handlers to confirm they both trigger invalidation.

### 9. `viewport/mod.rs` — re-exports only `ViewportRefreshEvent`, not the other viewport events

File: `src/widgets/viewport/mod.rs` line 19

Only `ViewportRefreshEvent` is re-exported from the viewport module. `ZoomViewportEvent`, `ResetViewportEvent`, `FitViewportEvent`, `Viewport100Event` are accessible only via the full path `crate::widgets::viewport::viewport_events::*`. This is inconsistent — callers that need these (hotkey handler, any future API) must use the verbose path. If `ZoomViewportEvent` or `ResetViewportEvent` ever get wired up, the missing re-export will cause confusion.

---

## MOD.RS RE-EXPORT AUDIT

### `src/core/mod.rs`
- `player_events` module is declared `pub mod` but **not re-exported** at the convenience level. All consumers import directly from `crate::core::player_events::*`. Consistent, not a bug.
- `layout_events` module: same pattern. Fine.

### `src/entities/mod.rs`
- `comp_events` module is declared `pub mod comp_events` but **no types from it are re-exported**. All consumers use full paths like `crate::entities::comp_events::SetBookmarkEvent`. Consistent, not a bug, but worth noting as intentional design.
- `NodeLayer` alias: `pub use comp_node::Layer as NodeLayer` — the comment on line 33 says "Layer is now only in comp_node.rs" which is correct, but `NodeLayer` is an awkward name (mixing "Node" and "Layer" concepts). Not a bug.

### `src/widgets/mod.rs`
- No re-exports at all, only module declarations. Fine.

### `src/dialogs/mod.rs`
- Only two modules: `encode` and `prefs`. Fine.

---

## TRAITS AUDIT (`src/entities/traits.rs`)

### 10. `FrameCache::is_empty()` default impl is correct but redundant with `len()`

Line 57: `fn is_empty(&self) -> bool { self.len() == 0 }`

Standard Rust pattern, nothing wrong.

### 11. No `clear()` method on `FrameCache` trait

The trait has no `clear` method. Cache clearing is done through the concrete `GlobalFrameCache` directly (bypassing the trait abstraction). This means code that holds only a `dyn FrameCache` cannot clear the cache. The trait is incomplete for all intended use cases. Any future code using the trait interface for cache management will be unable to clear it.

### 12. `WorkerPool` trait has only one method — no cancel/shutdown

`execute_with_epoch` is the only method. There is no way to cancel pending work or check the current epoch through the trait interface. Epoch management (`increment_epoch`, `current_epoch`) is done on the concrete `CacheManager` type only, not through any trait. The epoch-based cancellation is architectural: it works, but the trait boundary doesn't encapsulate the full contract.

---

## ACTIONS.RS AUDIT (`src/widgets/actions.rs`)

### 13. `ActionQueue.hovered` field — semantically overloaded

`hovered: bool` is a plain public field. It serves as an input-routing signal (which panel has mouse focus for hotkey dispatch). It is set by every widget's render function (`actions.hovered = response.hovered()`) and read by `app/tabs.rs` to populate `self.project_hovered / timeline_hovered / viewport_hovered`. This is correct but fragile — it requires that every new widget remembers to set it. There is no enforcement in the type system.

### 14. `ActionQueue::new()` is identical to `Default::default()` — redundant

Line 13: `pub fn new() -> Self { Self::default() }` with a `#[derive(Default)]`. The `new()` method adds nothing. Standard Clippy warning `clippy::new_without_default` inverted — here `new` delegates to default, which is fine, but could just be removed in favor of callers using `ActionQueue::default()` directly.

---

## SUMMARY TABLE

| # | Severity | File | Issue |
|---|----------|------|-------|
| 1 | HIGH | main_events.rs:671, viewport_events.rs:4 | ZoomViewportEvent multiply vs set mismatch + never emitted |
| 2 | HIGH | main_events.rs:789,814,829 | idx_to_uuid unwrap_or_default silently uses Uuid::nil on OOB |
| 3 | MEDIUM | player_events.rs:76 | PreloadFrameEvent defined, never emitted, never handled |
| 4 | MEDIUM | viewport_events.rs:21 | ResetViewportEvent has handler, never emitted |
| 5 | MEDIUM | entities/traits.rs | FrameCache trait missing clear() method |
| 6 | LOW | app/run.rs:132 | debounced tick() ignores comp_uuid, preloads wrong comp on switch |
| 7 | LOW | app/run.rs:131 | set_delay called every frame unnecessarily |
| 8 | LOW | comp_events.rs:148-166 | Two parallel attribute change event paths, handler parity unverified |
| 9 | LOW | widgets/viewport/mod.rs | Viewport events not re-exported consistently |
| 10 | LOW | widgets/actions.rs | ActionQueue.hovered enforcement gap |
