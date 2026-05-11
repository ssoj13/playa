# Coord-system audit (Phase A)

Date: 2026-05-10. Scope: every site that performs Y-flip, center-offset,
zoom/pan, or NDC mapping between the 9 spaces enumerated in TODO.md.

## TL;DR

- **Canonical helpers exist for only 2 of the ~5 actually needed
  conversions** (image↔frame, object→src). The other three
  (frame↔viewport-with-zoom-pan, viewport↔screen-Y-flip,
  frame↔NDC) are reimplemented inline at every call site.
- **2 parallel implementations of the full image→screen chain**:
  `viewport.rs::image_to_screen` (point math) and
  `gizmo.rs::build_gizmo_matrices` (4×4 matrix). Drift is the
  default state; the 6ca7a96 fix to one does not propagate.
- **playa-ui has its own coord helpers** in
  `widgets/viewport/coords.rs` (`flip_y_vec2`, `screen_to_viewport_centered`,
  `screen_delta_to_viewport`). Not re-exported from playa-time.
- **wgpu image render path** (`viewport_image.wgsl` + `get_view_matrix` +
  `get_projection_matrix`) does NOT Y-flip anywhere. Egui screen is
  Y-down, wgpu NDC is Y-up. If the model matrix passes image-space
  vertices unflipped, the rendered image is upside-down OR depending
  on UV orientation, in the wrong corner. **Strong candidate for the
  "image renders in bottom-right corner" bug.**

## 1. Canonical helpers (single source of truth)

`crates/playa-time/src/coord.rs`:

| Function | Direction | Math |
|---|---|---|
| `image_to_frame(p, size)` | image (Y-down, TL) → frame (Y-up, center) | `(x - w/2, h/2 - y)` |
| `frame_to_image(p, size)` | frame → image | `(x + w/2, h/2 - y)` |
| `object_to_src(p, src_size)` | object → src image | alias of `frame_to_image` |
| `to_math_rot(deg)` | CW° → CCW rad | `-deg.to_radians()` |
| `from_math_rot(rad)` | CCW rad → CW° | `-rad.to_degrees()` |

Re-exported by `playa-engine::entities::space` and `playa-time::lib`.
**NOT re-exported by playa-ui** — UI call sites import `space::*` from
playa-engine.

### Helpers that DON'T exist but should

1. `frame_to_viewport(p, zoom, pan) -> Vec2`
   = `Vec2::new(p.x * zoom + pan.x, p.y * zoom + pan.y)`
2. `viewport_to_screen(p, viewport_size) -> Vec2`
   = `Vec2::new(p.x + vp.x/2, vp.y/2 - p.y)`
3. `frame_to_ndc(p, comp_size) -> Vec2`
   = `Vec2::new(p.x / (comp.x/2), p.y / (comp.y/2))`
4. `viewport_size_y_up_to_clip_y_up_proj(vp_size) -> Mat4`
   — explicit name for the orthographic projection so callers don't
   reinvent ortho_rh.
5. Affine2 form of object_to_src: `object_to_src_affine(src_size) -> Affine2`
   so transform.rs L295-296 can stop reconstructing it.

## 2. Call sites using canonical helpers (✓ GOOD)

| File:line | What it does |
|---|---|
| `playa-engine/src/entities/transform.rs:506,528,553` | `image_to_frame(dst_pt, comp_size)` in remap! |
| `playa-engine/src/entities/transform.rs:508,530,555` | `object_to_src(obj_pt, src_size)` in remap! |
| `playa-ui/src/widgets/viewport/pick.rs:85` | `image_to_frame` for hit testing |
| `playa-ui/src/widgets/viewport/gizmo.rs:455-457,496-498` | `to_math_rot`/`from_math_rot` for rotation |

## 3. Inline Y-math sites (✗ SUSPECT)

### S1 — viewport.rs:236-253 `image_to_screen`
Inline reproduction of `image_to_frame` + frame→viewport + viewport→screen.

```rust
let frame = egui::vec2(
    image_pos.x - self.image_size.x * 0.5,        // ← image_to_frame inlined
    self.image_size.y * 0.5 - image_pos.y,
);
let viewport = egui::vec2(
    frame.x * self.zoom + self.pan.x,             // ← frame_to_viewport inlined
    frame.y * self.zoom + self.pan.y,
);
egui::vec2(
    viewport.x + self.viewport_size.x * 0.5,      // ← viewport_to_screen inlined
    self.viewport_size.y * 0.5 - viewport.y,
)
```

**Hypothesised fix** after centralisation:
```rust
let frame = playa_time::coord::image_to_frame(image_pos.into(), image_size_usize);
let viewport = frame_to_viewport(frame, self.zoom, self.pan);
viewport_to_screen(viewport, self.viewport_size)
```

Status: math is currently CORRECT (verified 6ca7a96), but inline ⇒
fragile. Any future tweak risks breaking pick / bracket / gizmo.

### S2 — viewport.rs:259-285 `screen_to_image`
Symmetric inverse of S1. Same fix shape: inverse helpers.

### S3 — viewport_ui.rs:716-717 (2D bracket path)
```rust
let image_x = world_pt.x + viewport_state.image_size.x * 0.5;
let image_y = viewport_state.image_size.y * 0.5 - world_pt.y;
```
This is `frame_to_image` literally inlined.

**Hypothesised fix**:
```rust
let image = playa_time::coord::frame_to_image(world_pt, image_size);
let screen = viewport_state.image_to_screen(image);
```

### S4 — viewport_ui.rs:702-711 (3D bracket path, NDC mode)
```rust
let frame_x = ndc.x * comp_w * 0.5;
let frame_y = ndc.y * comp_h * 0.5;            // ← ndc_to_frame inlined
let vp_x = frame_x * viewport_state.zoom + viewport_state.pan.x;  // ← frame_to_viewport
let vp_y = frame_y * viewport_state.zoom + viewport_state.pan.y;
let screen_x = vp_x + viewport_state.viewport_size.x * 0.5;       // ← viewport_to_screen
let screen_y = viewport_state.viewport_size.y * 0.5 - vp_y;
```
Three inlined conversions back-to-back. The Y-flip on the LAST line
mirrors S1; if S1's Y math ever changes (e.g. wgpu Y-up convention is
swapped), this site won't follow.

### S5 — gizmo.rs:280-393 `build_gizmo_matrices`
**Highest risk site.** Reproduces the entire `image_to_screen` chain
as a 4×4 viewport_transform matrix:

```rust
let scale_x = comp_w * zoom / vp_w;
let scale_y = comp_h * zoom / vp_h;
let trans_x = pan_x * 2.0 / vp_w;
let trans_y = pan_y * 2.0 / vp_h;
```

Comments say "must match `image_to_screen()`" — exactly the
condition that bug-class S5↔S1 drift would violate. There is **no
shared algebra** between the two; they're just commented to agree.

3D path (L329-373) and 2D path (L375-392) ALSO diverge from each
other (3D builds composite NDC matrix, 2D uses ortho_rh). Two
parallel chains for the same task.

**Hypothesised fix**: extract `screen_ndc_from_frame_ndc(zoom, pan,
comp_size, vp_size) -> Mat4` and use in BOTH gizmo and (if/when needed)
viewport image render.

### S6 — transform.rs:292-296 `build_inverse_matrix_3x3`
```rust
let object_to_src = Affine2::from_translation(Vec2::new(src_half.x, src_half.y))
    * Affine2::from_scale(Vec2::new(1.0, -1.0));
```
Inline matrix form of `object_to_src` (which is point-form). Math is
correct; reason for inlining is API mismatch — current helper takes
a point, not a size.

**Hypothesised fix**: add
`object_to_src_affine(src_size) -> Affine2` to playa-time and call
it here. Pure refactor, no semantic change.

### S7 — viewport.rs:378-416 + viewport_image.wgsl (wgpu render path)
- `get_view_matrix` returns `[zoom, 0; 0, zoom; pan.x, pan.y]` —
  no Y-flip.
- `get_projection_matrix` is a standard `ortho_rh` mapping
  `[-w/2..w/2]` × `[-h/2..h/2]` → `[-1,1]²` — no Y-flip.
- `viewport_image.wgsl` does plain `proj * view * model * vec4(position)`
  with no UV flip and no Y inversion.
- Image quad model matrix (viewport.rs:370-376) is
  `[image_size.x, 0; 0, image_size.y]` — pure scale, no flip.

**Combined effect**: the rendered image's vertex Y goes from image
space (Y-down) → unmodified through model+view+proj → wgpu NDC (Y-up).
This means **the image is upside-down on screen** UNLESS the UVs
compensate or the texture itself is uploaded flipped.

The TODO references "image renders in bottom-right corner of canvas at
frame 33". Combined with `0c1d804 revert(compositor): wgpu shader UV
— fix was on incorrect mental model`, the issue is likely here:
inconsistent assumptions about which layer in the chain owns the
Y-flip. **This is the strongest candidate for the open bug.**

Recommend: trace the image-quad vertex/UV generation path for
`viewport_image` pipeline; document the canonical answer to "where
does Y-flip happen for the wgpu image render"; add a comment in
`get_view_matrix` linking to that doc.

### S8 — layer_blend.wgsl:83-91 (compositor handoff)
Comment is correct as far as it goes ("canvas_pixel in top-left Y-down,
pairs with IDENTITY_TRANSFORM"). But:
- The wgsl REQUIRES the matrix produced by `build_inverse_matrix_3x3`
  (which already does object→src Y-flip via L296), or the IDENTITY
  matrix when the layer is identity-transformed.
- comp_node.rs:1380-1387 comment says "GPU path still uses Z-only
  rotation until shader is updated" — implying the GPU path is
  intentionally degraded. Decision needed: kill the GPU path, OR
  finish wiring the matrix.

Not a Y-flip bug, but a documentation + decision debt that lives in
the same coord-system class.

### S9 — playa-ui/src/widgets/viewport/coords.rs (whole file)
```rust
pub fn flip_y_vec2(v: egui::Vec2) -> egui::Vec2 {
    egui::vec2(v.x, -v.y)
}
pub fn screen_to_viewport_centered(...)
pub fn screen_delta_to_viewport(...)
```
Helpers exist but live in playa-ui, NOT in playa-time. They handle
egui-specific `Vec2` types so promoting needs a Vec2/glam wrapper.

**Hypothesised fix**: promote to playa-time as
`fn flip_y(p: glam::Vec2) -> glam::Vec2`, add egui-compat wrapper in
playa-ui that just calls into it.

## 4. Patterns NOT found (false-alarm clears)

- `flip_y` outside `coords.rs` — none.
- `1.0 - uv.y` in any wgsl — none. UV-flip is not currently the
  problem; the issue is the geometry-side Y direction.
- Ad-hoc `Vec2::new(x, -y)` outside legitimate sites — none in
  prod code (only test data and the `(1.0, -1.0)` Affine2 scale at S6).

## 5. Priority for Phase B

1. **S7** — wgpu render path. Likely contains the open
   "image-in-corner" bug. Investigate first.
2. **S5** — gizmo matrix. Highest drift risk. Extract shared chain.
3. **S1, S2, S3, S4** — viewport.rs + viewport_ui.rs inline math.
   Mechanical refactor once helpers from §1 exist.
4. **S6, S9** — small, low-risk additions to playa-time.
5. **S8** — design decision (kill GPU compositor or finish it),
   not a coord bug.

## 6. Files touched / read

```
crates/playa-time/src/coord.rs                         (canonical)
crates/playa-time/src/lib.rs                           (re-export)
crates/playa-engine/src/entities/space.rs              (re-export)
crates/playa-engine/src/entities/transform.rs          (consumers + S6)
crates/playa-engine/src/entities/comp_node.rs          (compositor wiring)
crates/playa-engine/src/render_gpu/shaders/layer_blend.wgsl (S8)
crates/playa-ui/src/widgets/viewport/coords.rs         (S9)
crates/playa-ui/src/widgets/viewport/viewport.rs       (S1, S2, S7)
crates/playa-ui/src/widgets/viewport/viewport_ui.rs    (S3, S4)
crates/playa-ui/src/widgets/viewport/gizmo.rs          (S5)
crates/playa-ui/src/widgets/viewport/pick.rs           (consumer)
crates/playa-ui/src/widgets/viewport/wgsl/viewport_image.wgsl (S7)
```
