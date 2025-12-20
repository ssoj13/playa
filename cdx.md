# Gizmo + Transform Audit (cdx)

## Scope
Key files reviewed:
- `src/widgets/viewport/gizmo.rs`
- `src/widgets/viewport/viewport.rs`
- `src/widgets/viewport/viewport_ui.rs`
- `src/widgets/viewport/renderer.rs`
- `src/widgets/viewport/coords.rs`
- `src/entities/transform.rs`
- `src/entities/space.rs`
- `src/entities/comp_node.rs`
- `src/entities/camera_node.rs`
- `src/entities/compositor.rs`
- `src/entities/gpu_compositor.rs`
- `src/entities/node.rs`

## What the code does today (short)
2D:
- Frames are composed on CPU. Each layer is pre-transformed in `transform_frame()` and then blended (`CpuCompositor`).
- `transform_frame()` maps destination pixels -> comp (Y-up) -> object -> source (Y-down) using `space.rs`.
- Viewport renderer draws a unit quad (-0.5..0.5) scaled by `image_size * zoom` and panned.
- Gizmo uses transform-gizmo-egui; translation is provided in comp pixels and converted through its view/proj.

3D:
- `CameraNode` defines a view/projection matrix in Y-up, but is not used by compositor or viewport render.
- Gizmo is 3D-capable, but only 2D axes are enabled (Translate/RotateZ/Scale) and only rot.z is used.

## Findings (problems + risks)
1) Multiple coordinate systems without a single runtime owner
- `space.rs` defines comp/object/image conversions (Y-up vs Y-down), `coords.rs` defines screen<->viewport conversions, and `ViewportState` has its own image<->screen math. They are not wired together consistently.
- Result: comp/viewport/image conversions are scattered, easy to desync, and already drifted.

2) Gizmo and renderer use different world units
- Renderer world units are normalized (-0.5..0.5) and scaled by `image_size * zoom` (`ViewportState::get_view_matrix()` / `renderer.rs`).
- Gizmo world units are comp pixels (`space::comp_to_viewport`).
- Even with matching projection, this mix leads to scale mismatch, especially when comp size != image size or with non-1 zoom.
- This also explains “gizmo jumps” and inconsistent placement: the gizmo’s world coordinates do not share the same basis as the rendered image.

3) `position` semantics changed but defaults were not updated
- We now define `position` as pivot position in comp space (origin left-bottom, Y-up).
- New layers still default to `position = (0,0,0)`, which places them at comp origin (bottom-left). That’s why the gizmo spawns in the corner before any interaction.
- This is a UX regression unless the design is “origin = bottom-left” by default.

4) Rotation sign conversions are not centralized
- Attrs store clockwise-positive (user convention). Gizmo uses CCW-positive math. CPU transform uses CW-positive values in the inverse mapping.
- These conversions are spread across `gizmo.rs` and `transform.rs` and not documented as a single invariant.

5) GPU compositor matrix conventions are fragile
- `build_inverse_matrix_3x3()` now outputs a comp->src mapping (Y-up -> Y-down). The GPU shader expects `u_top_transform * canvas_pixel` in Y-down.
- It works only if all comp/image assumptions hold. The GPU path is currently not active in comp, so mismatches will surface later.

6) 3D path is not integrated
- `CameraNode` exists and is instantiable, but its matrices are unused in layer rendering/compositing.
- UI can expose 3D attrs, but layers are still 2D-only (only rot.z applied, no perspective).
- This is a source of confusion and inconsistent expectations in UI.

7) Comments are stale and misleading
- `gizmo.rs` claims “Frame space == viewport space, no conversion needed”, which is no longer true after introducing comp/object/image spaces.

## Proposed fixes (options)
### Option A (preferred): make viewport + gizmo share a single pixel space
Goal: both renderer and gizmo operate in the same pixel-world coordinates.
- Change renderer quad to be in pixel space instead of normalized space. For example, set quad vertices to `(-w/2, -h/2) .. (w/2, h/2)` and drive them with the same view/proj the gizmo uses (pan + zoom).
- Use a single `view_matrix` builder for both renderer and gizmo: `scale = zoom`, `translate = pan`. Projection stays centered on viewport.
- Result: comp pixels == viewport pixels, gizmo translation uses comp pixels directly, no extra conversion.

### Option B: keep renderer normalized, add explicit comp->renderer mapping
Goal: no renderer changes, but gizmo coordinates are converted into renderer world units.
- Define `comp_to_renderer_world(p)` that converts comp pixels to normalized [-0.5..0.5] space using comp size (divide by comp dims, subtract 0.5).
- Gizmo uses that mapping before passing transforms to the gizmo library.
- Keeps renderer intact, but introduces another conversion layer.

## Additional concrete actions
1) Set default layer position to comp center on creation
- In `add_child_layer` (or layer init), set `position = [comp_w/2, comp_h/2, 0]` when creating a layer so pivot=0 means “centered by default”.
- This directly fixes gizmo spawning at bottom-left.

2) Centralize rotation sign conversion
- Add helpers in `space.rs` (or `transform.rs`) like `to_math_rotation()` / `from_math_rotation()` to avoid scattering sign flips.

3) Unify conversion helpers
- Make `ViewportState::image_to_screen` / `screen_to_image` use `space.rs` helpers internally, or remove them in favor of a single set of conversion functions.

4) Clarify and document 3D status
- Either: (a) hide 3D features until integrated, or (b) wire CameraNode into comp rendering and add 3D transforms for layers.

5) Add small correctness tests
- Unit tests for `space.rs` round-trips (comp->image->comp, object->src->object).
- A 2D transform test that places a known pixel at comp center and verifies expected output.

## Summary (what to fix first)
- Pick Option A or B for renderer/gizmo alignment.
- Set default layer position to comp center.
- Replace scattered conversions with `space.rs` as single source of truth.
- Decide whether 3D is “real” or “stub” and adjust UI/implementation accordingly.
