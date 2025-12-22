# Hover Selection Feature

Layer under cursor highlights in Select mode (Q).

## Tasks

### 1. State - CompNode.hovered_layer
- [x] Add `hovered_layer: Option<Uuid>` to CompNode (skip serialization)
- [ ] Clear on mode change / comp switch

### 2. Viewport - hover detection
- [x] In `viewport_ui.rs` render(), when `ToolMode::Select`:
  - Check `ctx.input(|i| i.pointer.hover_pos())`
  - If cursor over viewport - call `pick::pick_layer_at()`
  - Update `comp.hovered_layer` via event or direct mutation
- [x] Throttle not needed - raycast O(layers), typically <10 layers

### 3. Timeline - highlight
- [x] In `timeline_ui.rs` layer bar rendering:
  - If `layer.uuid() == comp.hovered_layer` - draw highlight border/glow
  - Orange border (2px) for hovered, blue for selected

### 4. Viewport outline
- [x] Draw transformed bbox of hovered layer in viewport
- [x] Forward transform: layer corners -> comp space -> screen
- [x] Orange stroke style (2px)

### 5. Settings toggle
- [x] Add `HoverPrefs` to `ProjectPrefs` (viewport_highlight, timeline_highlight)
- [x] Toggle in Settings panel (F12) -> Hover category
- [x] SetHoverPrefsEvent for event-driven updates

## Files to modify

```
src/entities/comp_node.rs        - hovered_layer field
src/widgets/viewport/viewport_ui.rs - hover detection loop
src/widgets/timeline/timeline_ui.rs - highlight rendering
src/widgets/viewport/pick.rs     - already done
src/core/settings.rs             - toggle (phase 2)
```

## Implementation order

1. CompNode.hovered_layer field
2. Viewport hover detection
3. Timeline highlight
4. Test & iterate
5. (Phase 2) Settings toggle
6. (Phase 2) Viewport outline
