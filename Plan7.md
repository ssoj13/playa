# Plan 7: Coordinate Space Unification (Y‑up)

Date: 2025-12-19
Scope: convert all transform logic to a single Y‑up ground truth; fix gizmo/pivot consistency.

---

## Goals
- Use ONE consistent coordinate system across gizmo, transform, and rendering.
- Make pivot=0,0 sit at layer center and keep gizmo anchored to pivot.
- Remove ad‑hoc Y‑flips and divergent codepaths.

---

## Definitions (single source of truth)
- **comp space**: origin = left‑bottom, +Y up, units = pixels.
- **layer/object space**: origin = center of layer, +Y up, units = pixels.
- **pivot**: offset from layer center in layer space (default 0,0).
- **position**: pivot position in comp space (so pivot=0 ⇒ position = layer center).

---

## Step 0 — Inventory (no code change)
- List all current Y‑flips and transform helpers:
  - `src/widgets/viewport/coords.rs`
  - `src/widgets/viewport/gizmo.rs`
  - `src/widgets/viewport/viewport.rs`
  - `src/widgets/viewport/viewport_ui.rs`
  - `src/entities/transform.rs`
  - `src/entities/comp_node.rs`
- Confirm where width/height are sourced for transforms (layer attrs vs source frame).

---

## Step 1 — Add core conversion helpers (new module)
Create a single “ground truth” module, e.g. `src/entities/space.rs` (or extend `transform.rs`), with:

1) **comp <-> viewport** (viewport is centered Y‑up):
- `comp_to_viewport(p, comp_size)` → `p - (w/2,h/2)`
- `viewport_to_comp(p, comp_size)` → `p + (w/2,h/2)`

2) **layer pivot helpers**:
- `layer_pivot_in_comp(position, pivot, src_size)`
- `position_from_pivot(pivot_pos, pivot, src_size, position_z)`

3) **layer/object <-> source image pixels** (image is top‑left Y‑down):
- `object_to_src(p, src_size)` → `p + (w/2,h/2)` with Y flip to image space
- `src_to_object(p, src_size)` → inverse of above

4) **comp <-> image** (comp Y‑up ↔ image Y‑down):
- `comp_to_image(p, comp_size)` and `image_to_comp(p, comp_size)`

All future code must use these helpers — no inline Y‑flips.

---

## Step 2 — Transform math (rendering path)
Update `src/entities/transform.rs` and `transform_frame()` to use the new spaces:
- Define transform in **layer space** (center origin, Y‑up):
  - `comp_pos = position + R*S*(object_pos - pivot)`
- Inverse for sampling:
  - `object_pos = R^-1*S^-1*(comp_pos - position) + pivot`
- Convert `object_pos` → source pixel coords with `object_to_src` before sampling.
- Replace any use of `src_center` with layer/object conversions.

Update `build_inverse_matrix_3x3` to match the new math (GPU path still WIP but keep consistent).

---

## Step 3 — Gizmo (viewport) + RMB drag
Update `src/widgets/viewport/gizmo.rs` and RMB drag in `src/widgets/viewport/viewport_ui.rs`:
- Gizmo translation = `comp_to_viewport(layer_pivot_in_comp(position, pivot, src_size), comp_size)`
- On drag/move: `position = position_from_pivot(viewport_to_comp(gizmo_pos), pivot, src_size)`
- All conversions must go through the new helper module (no manual flips).

---

## Step 4 — Viewport pan/zoom input
Ensure pan/zoom uses comp Y‑up consistently:
- Use conversion helpers for cursor deltas and any screen↔comp math.
- Remove ad‑hoc `y = -y` in viewport code where possible.

---

## Step 5 — Attrs + UI semantics
- Treat `position`/`pivot`/`rotation` as Y‑up in attrs.
- Update comments and UI labels/tooltips (AE‑style Y‑down text must be removed).
- Make sure default pivot (0,0) means “center”.

---

## Step 6 — Breakage policy (explicit)
- No backward compatibility: old project files will interpret values under new Y‑up rules.
- Document this in plan and (optionally) a short note in README or CHANGELOG if needed.

---

## Step 7 — Validation
Manual checks:
1) New layer with pivot (0,0): gizmo centered on the layer.
2) Move tool: gizmo and layer move together, no drift.
3) Scale: layer scales around pivot; pivot stays fixed.
4) Rotate: rotation direction matches chosen convention.
5) Pivot offsets move gizmo predictably.

---

## Decisions
1) **Rotation sign**: clockwise (right) = positive (user‑friendly, AE‑style).
2) **RMB drag**: must match gizmo rotation sign (clockwise positive).
3) **Comp size source**: use `comp.dim()` as ground truth for comp<->viewport conversions.
