# Playa Codebase Review (Architecture/Logic)

## Key Findings
- **Unimplemented event handlers – playback/timeline controls are no-ops** (`src/main.rs`:569-618, 841-873, 890-902): `AppEvent::{StepForward,StepBackward,StepForwardLarge,StepBackwardLarge,PreviousClip,NextClip}`, `RemoveSelectedLayer`, and all drag events return without logic. Timeline toolbar and future hotkeys that emit these events do nothing, so expected navigation/editing actions silently fail.
- **Loop/FPS toggles mutate settings only, not the runtime player** (`src/main.rs`:696-705, 788-795): `ToggleLoop`, `IncreaseFPS`, `DecreaseFPS` only flip `AppSettings` values. Playback actually reads `player.loop_enabled` / `player.fps_base`, so EventBus callers will see no effect; state can also diverge between UI-persisted settings and live playback.
- **Hotkey system is effectively dead code** (`src/dialogs/prefs/hotkeys.rs`, `src/main.rs`:94-101, 214-222, 925-936): `HotkeyHandler` is constructed and stored but never consulted; `AppEvent::Hotkey{Pressed,Released}` handlers are TODOs. Any attempt to route keyboard shortcuts through this system will be ignored, and `focused_window` is unused.
- **Worker count overrides ignored** (`src/main.rs`:1663-1668): CLI flag `--workers` / `AppSettings.workers_override` are read into `_workers` and stored in `applied_workers`, but the worker pool was already created in `PlayaApp::default` with a fixed `(num_cpus*3/4)` size and never rebuilt. User-specified worker counts are silently ignored.
- **Timeline action result discarded** (`src/main.rs`:1284): `render_timeline_panel` returns `TimelineActions` (hover/interaction metadata) but caller drops it. Any future additions to `TimelineActions` will be lost until the result is handled.
- **Remove media event stub** (`src/main.rs`:640-643): `AppEvent::RemoveMedia` is defined but unimplemented and never emitted. If wired, it would silently fail to delete items.

## Observations
- EventBus is only partially integrated: many controls bypass it and talk to `Player` directly, leaving duplicate code paths and inconsistent behavior between direct hotkeys and event-driven callers.
- Drag-and-drop + selection events have enum coverage but no logic, signaling unfinished features that currently do nothing when triggered.
