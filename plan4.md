# Bug Hunt Report (Hotkeys & Events)

## Summary
- Added widget-scoped event modules for `ae`, `node_editor`, and `project` to align with the existing event layout and deduplicate imports.
- Implemented Node Editor hotkey context: new `HotkeyWindow::NodeEditor`, hover tracking (reset each frame and when tab hidden), and bindings for A/F/L routed through the event bus.
- Node Editor now follows the active composition on load, on project activation changes, and on new comp creation, keeping it in sync with Timeline.
- Node Editor fit/layout events are routed and now center nodes deterministically: fit-all recenters all nodes, fit-selected recenters only the current selection (falls back to all if none); fit no longer triggers rebuilds, eliminating position jitter.
- Persisted node positions per layer (`node_pos` in layer attrs, root in comp attrs). Positions are loaded during rebuild and saved after render without marking comps dirty (clears dirty flags), so layout stays stable across sessions.

## Data Flow (hotkeys & active comp)
- Input → `HotkeyHandler` (context from `determine_focused_window`) →
  - `Timeline` context: F/A → `TimelineFitEvent` / `TimelineResetZoomEvent`.
  - `NodeEditor` context: F/A/L → `NodeEditorFitSelectedEvent` / `NodeEditorFitAllEvent` / `NodeEditorLayoutEvent`.
- `EventBus` → `main_events::handle_app_event` → updates `TimelineState` or `NodeEditorState` flags.
- `render_node_editor` consumes flags → centers nodes (selection-first) and triggers layout → view stays aligned without stale flags → persists `node_pos` into attrs.
- Active comp changes (project load, explicit activation, new comp) → `NodeEditorState::set_comp` to mirror `Project.active_comp` used by Timeline.

## Changes
- Added `ae_events.rs`, `project_events.rs` (re-export core project events), and re-exported node editor events in `mod.rs` files.
- Hotkey context: new `NodeEditor` window, hover tracking reset each frame and when tab hidden, bindings for A/F/L.
- Event handling: `main_events::handle_app_event` now receives `NodeEditorState` and handles node editor fit/layout events; subscribes to `ProjectActiveChangedEvent` for sync.
- Sync on load/new comp: set node editor comp when loading sequences/projects or creating comps.
- Fit handling: center all nodes or selection (uses egui-snarl selection API) without forcing rebuild; fallback to all when selection empty.
- Persistence: `node_pos` stored per layer/root attrs, loaded on rebuild, written after render; dirties are cleared so rendering cache is unaffected.

## Tests
- Not run (not requested). Build via `./start.cmd` succeeds.

## Follow-ups
1) If precise zooming is needed, switch to manipulating `SnarlState` transforms directly when/if the library exposes a public API (current centering is translation-only).
2) Consider persisting last node-editor view per comp if users expect independent pans per comp instead of centering on every fit command.
