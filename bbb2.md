# Remaining from Plan: Viewport/Gizmo Coordinate Unification

## Status: NOT STARTED

The camera system is done. The original plan was about unifying viewport/gizmo coordinates.

---

## What the Plan Says

### Problem
Renderer and gizmo use different world units:
- **Renderer:** quad `-0.5..0.5` (normalized), view matrix scales by `image_size * zoom`
- **Gizmo:** positions in pixels, view matrix scales by `zoom` only

Result: visual desync between rendered image and gizmo position.

### Solution
Introduce **model matrix** to renderer:
- `model` = scale by image_size (transforms normalized quad to pixel space)
- `view` = scale by zoom + translate by pan (same as gizmo)
- `projection` = orthographic (unchanged)

Both use same view matrix = synchronized.

---

## Files to Modify

1. **viewport.rs**
   - [ ] `get_view_matrix()` — remove image_size scaling, keep only zoom + pan
   - [ ] Add `get_model_matrix()` — scale by image_size
   - [ ] Update `ViewportRenderState` struct
   - [ ] Simplify `image_to_screen()` / `screen_to_image()`

2. **shaders.rs**
   - [ ] Add `u_model` uniform to vertex shader

3. **renderer.rs**
   - [ ] Pass `u_model` uniform in render()

4. **gizmo.rs**
   - [ ] Update stale comments (now actually true after fix)

5. **space.rs**
   - [ ] Add `to_math_rot()` / `from_math_rot()` helpers

---

## Is This Still Needed?

**MAYBE NOT.** The camera system fix (rotation signs) might have fixed the gizmo alignment too.

**Test first:**
1. Add layer to comp
2. Check if gizmo appears centered on layer
3. Move/rotate/scale — does gizmo follow?
4. Zoom/pan — does gizmo stay aligned?

If all works → plan is obsolete.
If gizmo is still off → implement the plan.

---

## Priority

LOW. Camera was the blocker. Gizmo alignment is polish.
