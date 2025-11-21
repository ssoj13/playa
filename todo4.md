- Status recap
  - EventBus expanded for timeline operations (jump edges, reorder/move/trim layers, comp play-range abs).
  - Timeline tab split into SidePanel (outline) + CentralPanel (canvas) using shared TimelineState; outline now called with a dispatch closure in ui.rs.
  - render_canvas signature already accepts a dispatch closure (WIP), but still returns TimelineAction internally.
  - TimelineViewMode added to state, unused.
  - Code formatted; build/tests not run.

- Remaining refactor to finish EventBus-first timeline renderers
  1) render_outline: replace all `action = ...` with `dispatch(...)`; remove TimelineAction return type and the local action variable.
  2) render_canvas: same—switch to dispatch, remove action variable/return value.
  3) ui.rs: call render_canvas with a closure; stop matching on returned action; remove the bridge once both renders dispatch directly.
  4) Consider using TimelineViewMode (Split/CanvasOnly/OutlineOnly) once API is settled.

- After refactor
  - cargo fmt
  - Build/run to catch compile/runtime errors.
