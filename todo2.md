- Build warnings are unused/leftover functions, not deprecated APIs. Decide what to revive vs remove.

- UI traits usage: `ProjectUI`/`TimelineUI`/`AttributeEditorUI`/`NodeUI` should be called by project/timeline/attribute editor/node tree widgets so comps render themselves; ensure `TimelineDragUI` exists for drag.
- `timeline_ui`: clarify/replace `child_ui` usage.
- `viewport/mod.rs`: check if `ViewportActions` is still needed with EventBus.
- `dialogs/prefs/hotkeys.rs`: finish `HotkeyHandler` and hook into prefs UI (`handle_key`/bindings).
- `entities/attrs.rs`: consider restoring helpers (get/remove/iter_mut/contains/len) to dedupe callers.
- `entities/comp.rs`: decide on setter/parent getters; use `FrameStatus::color` for an indicator.
- `loader_video.rs`: use `frame_count` in metadata; ensure loader functionality kept.
- `project.rs`: remove `set_compositor`/`get_comp`/`remove_media`.
- `events.rs`: wire `AppEvent`/`HotkeyWindow`/`CompEvent::TimelineChanged` through EventBus.
- `timeline.rs`: either use or remove `drag_state` fields (display_name/drag_start_pos/initial_end) and helpers `detect_layer_tool`/`draw_playhead`.
- `viewport/renderer.rs`: surface `shader_error` in UI.

Back to .orig references (working code):
- `.orig/src/timeslider.rs`: status strip under timeruler using `FrameStatus::color`; `Cache` (cached_count + loaded_events + sequences_version); sequence background + play range. Current widgets/timeline lacks this—restore.
- `.orig/src/sequence.rs`: supports `*`, printf `%0xd`, padding, gaps; builds frame_path. Current `utils/sequences.rs` is simpler—add printf/padding/gaps support.
- Frame/EXR/Video (`frame.rs`, `exr.rs`, `video.rs`, `convert.rs`): may have loader/tonemap/ffmpeg details; compare with `entities/loader.rs` + viewport.
- Hotkeys/prefs (`.orig/src/prefs.rs`, `ui_encode.rs`): hotkey logic may be missing; matches warnings on stubs.

Plan (actionable):
1) Timeline status bar: restore FrameStatus-based strip + cache next to timeruler; placeholder outside range must work; keep key path computations intact.
2) Sequence parser: add printf-style patterns/padding/gaps and robust frame_path; reintroduce detect_sequence in Comp (Comp == old Sequence+Layer).
3) Loader/viewport audit: compare with .orig for EXR/video options; ensure video loader metadata uses frame_count.
4) Hotkey/prefs: integrate HotkeyHandler into prefs UI; at least shrink warnings, leave TODO where scope is large.
5) Testing: run `./start.cmd` for smoke; targeted `cargo test` when math changes; note UX/perf questions as found.
