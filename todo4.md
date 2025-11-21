------------------------------------------------------------
#1:

- Goal: Split Timeline tab into two panes (splitter) with full EventBus wiring; Zoom/Pan only in right pane.

- Layout: One Timeline tab housing a horizontal split:
  - Left (outline): toolbar (transport/zoom controls, checkboxes), layer list with DnD, no horizontal scroll/zoom; collapsible.
  - Right (canvas): ruler + status strip + bars + playhead + drag/trim; own horizontal scroll/zoom.

- EventBus-first comms (no direct return values):
  - From outline → core/canvas: timeline zoom set/reset, pan set, toggle snap/frame numbers, layer select, layer reorder, layer move/start/end trim, add layer drop.
  - From canvas → core/outline: layer reorder/select, move/trim results, playhead set, drag/drop add layer.
  - App handles these events to mutate Comp/TimelineState and triggers repaint; keeps state single-source.

- Refactor steps:
  1) Extract pure render helpers: `render_timeline_outline(ui, comp, state, bus_sender)` and `render_timeline_canvas(ui, comp, state, bus_sender)`.
  2) Replace `TimelineAction` returns with EventBus emits (new AppEvents: `TimelineZoomChanged(f32)`, `TimelinePanChanged(f32)`, `TimelineLayerReordered{from,to}`, `TimelineLayerMoved{idx,new_start}`, `TimelineLayerTrim{idx,start,end}`, `TimelineSetFrame(i32)`, etc.).
  3) Split layout inside Timeline tab with a splitter: left fixed/collapsible width, right flex; horizontal scroll/zoom only on right.
  4) Ensure shared `TimelineState` is single instance passed to both; no duplicate pan/zoom values.
  5) Keep egui-dnd in outline; on reorder emit event; update comp in App when handling bus events.
  6) Preserve status strip/playhead logic in canvas; outline stays light.
  7) Test: run app, hide/show outline, verify zoom/pan only affect canvas; DnD reorder/trim/move events propagate through EventBus; playhead scrubbing works.

------------------------------------------------------------

#2:
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


------------------------------------------------------------