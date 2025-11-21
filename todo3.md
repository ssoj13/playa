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
