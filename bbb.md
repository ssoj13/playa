# Plan6 / Plan7 Status

Date: 2025-12-19

---

## Plan7 - Y-up Coordinate Unification

### Done

| Step | Description | Status |
|------|-------------|--------|
| 1 | `space.rs` with conversions comp↔viewport, object↔src, comp↔image | Done |
| 2 | `transform.rs` uses Y-up and new helpers | Done |
| 3 | Gizmo uses `comp_to_viewport()`, position centered correctly | Done |
| 5 | Default position = center of comp when creating layer | Done |
| 6 | Breakage policy (old projects will break) | Accepted |

### Remaining

| Step | Description | Status |
|------|-------------|--------|
| 3 | **RMB drag tool** — verify it uses same helpers | TODO |
| 4 | **Viewport pan/zoom** — remove ad-hoc Y-flips if any | TODO |
| 5 | Attrs/UI labels — remove AE-style Y-down mentions | TODO |
| 7 | **Validation** — manual tests (scale, rotate, pivot) | TODO |

---

## Plan6 - 3D Perspective Roadmap (not started)

- Phase 1: Camera integration (`active_camera`, `aspect()`)
- Phase 2: 3D transform math (Mat4)
- Phase 3: GPU compositor (mat4 MVP)
- Phase 4: compose_internal 3D routing
- Phase 5: UI for 3D

---

## Fix Applied Today (2025-12-19)

**Problem**: Gizmo appeared in bottom-left corner instead of center when selecting a layer.

**Root cause**: Semantic mismatch between gizmo and compositor:
- Gizmo used `comp_to_viewport(position, comp_size)` correctly
- But layer default `position = (0, 0, 0)` meant left-bottom of comp
- OpenGL renderer always centers the image quad at (0,0) viewport
- Result: image centered, gizmo in corner

**Solution**:
1. Layer `position` now means "absolute position of layer center in comp space"
2. Default position = `(comp_w/2, comp_h/2)` — center of comp
3. `comp_to_viewport((360, 288), (720, 576))` = `(0, 0)` — gizmo at viewport center
4. Compositor and gizmo now use same semantics

**Files changed**:
- `src/entities/comp_node.rs` — `add_child_layer()` sets position to comp center
- `src/widgets/viewport/gizmo.rs` — documented coordinate system
- `src/entities/space.rs` — documented coordinate spaces

---

## Plan: Tool Unification + Complete Plan7

Date: 2025-12-19

### Current state:
- **Q** = Select (does nothing on viewport)
- **W/E/R** = Move/Rotate/Scale (RMB drag transforms)
- **Scrubber** = LMB drag on viewport (not tied to tool)

### Goal:
- **Q** = Select → RMB = timeline scrubbing
- **W/E/R** = Move/Rotate/Scale → RMB = transform (as now)
- All tools work uniformly via RMB

### Steps:

**Step 1: Scrubber on RMB in Select mode**
- In `viewport_ui.rs`: scrubbing only when tool=Select and RMB drag
- Use same latch logic as `right_drag_tool_event()`

**Step 2: Remove LMB scrubbing**
- Remove current LMB scrubbing completely (confirmed: all via RMB)

**Step 3: Verify RMB drag tool**
- Ensure `right_drag_tool_event()` uses `space::` helpers
- Remove ad-hoc Y-flips if any

**Step 4: Verify pan/zoom**
- Check `handle_pan()` and `handle_zoom()` use `coords::` helpers

**Step 5: Validation**
- Manual tests: move, rotate, scale, scrub, pan, zoom

---

## Implementation Progress

### Step 1: Scrubber on RMB in Select mode - DONE
- Modified `right_drag_tool_event()` to handle Select tool
- RMB drag in Select mode now does timeline scrubbing
- Uses same latch logic as transform tools

### Step 2: Remove LMB scrubbing - DONE
- Removed `handle_scrubbing()` call from render()
- All scrubbing now via RMB in Select mode

### Files changed:
- `src/widgets/viewport/viewport_ui.rs` - unified RMB handler
- `src/widgets/viewport/viewport.rs` - made `fit()` public

### Ready for testing:
- Q (Select) + RMB drag = scrub timeline
- W (Move) + RMB drag = translate layer
- E (Rotate) + RMB drag = rotate layer
- R (Scale) + RMB drag = scale layer
