cdx report
==========

- Node editor positions are persisted per layer instance using `node_pos` in attrs; root uses `Comp.attrs`, children use `Layer.attrs`. Tree traversal now carries `instance_uuid` and `source_uuid` so multiple instances of the same comp no longer fight over positions; ancestors-based cycle guard prevents runaway recursion.
- `render_node_editor` now returns a hover flag; `render_node_editor_tab` uses it to set `node_editor_hovered`, so A/F/L hotkeys route to the node editor instead of the timeline when the mouse is over the graph.
- Hover detection simplified to `ui.rect_contains_pointer(ui.max_rect())` inside the node editor; hotkey focus calculation updated accordingly.
