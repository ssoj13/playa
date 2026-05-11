# TODO

## Coord-system centralization (HIGH priority — accumulating bugs)

Recurring class of visual bugs: each module does its own Y-flip /
center-offset / scale; on the seam between two modules they disagree
and overlays drift, images render in corners, brackets follow opposite
of pan, etc. Fixed cases this session: `image_to_screen` Y-flip
(commit `6ca7a96`). Probably more lurking — bracket / gizmo / hover
highlight / wgpu compositor handoffs.

Coord systems currently in use (9):
1. Image / source pixel — top-left, Y-down
2. Frame (centered) — center, Y-up [AE convention]
3. Object — layer center, Y-up
4. NDC (wgpu) — center, Y-up
5. UV (texture) — top-left, Y-down
6. Viewport screen — top-left, Y-down (egui pointer)
7. Viewport centered — center, Y-up (pan + zoom storage)
8. Layer params (pos / pivot) — center, Y-up (user-facing AE)
9. Source seq frame index — start-relative, integer

Single source of truth that EXISTS: `playa-time::coord::{
image_to_frame, frame_to_image, object_to_src, to_math_rot,
from_math_rot}`. NOT used everywhere — many sites still inline
their own Y math.

Refactor plan — 4 phases, ~3 focused days total:

### Phase A — Audit + inventory (~half day)
Use filesystem MCP `grep_files` for Y-flip patterns:
`h - y`, `h * 0.5 - p`, `1.0 - uv.y`, `-y`, `flip_y`, manual `0.5 -`,
`* -1.0`. Produce `.bughunt/coord_audit.md` listing every site with:
- file:line
- input space → output space
- through helper (good) or inline (suspect)
- if inline: hypothesised correct version

### Phase B — Centralize all helpers (~day)
Move all Y-flips to `playa-time::coord::` (or new `playa-coord` crate
if we want playa-time clean). Touch sites:
- `playa-ui/src/widgets/viewport/coords.rs` → re-export from playa-time
- `playa-ui/src/widgets/viewport/viewport.rs::image_to_screen` and
  `screen_to_image` → use coord helpers verbatim (already fixed
  this session but still inline; should call helpers explicitly)
- `playa-ui/src/widgets/viewport/viewport_ui.rs:717` (`image_size.y * 0.5 - world_pt.y`) — replace with `frame_to_image` call
- `playa-engine/src/entities/transform.rs::transform_frame_with_camera`
  — verify all inline math (image_to_frame / object_to_src already
  used; verify no leftover inline)
- `playa-engine/src/render_gpu/shaders/layer_blend.wgsl` — document
  the coordinate convention assumed by the matrix passed from
  `build_inverse_matrix_3x3` and the UV→canvas mapping. Decide
  CANONICAL convention for shader: either pre-transformed frame +
  identity matrix (current) or raw frame + non-identity matrix.
  Document and enforce one.
- gizmo / brackets / hover-highlight rendering: each goes through
  `image_to_screen` (now fixed) — verify no inline Y math remains.

### Phase C — Newtype wrappers (~day) [optional but bulletproof]
Distinct types per space:
```rust
struct ImagePos(Vec2);    // top-left, Y-down
struct FramePos(Vec2);    // center, Y-up
struct ScreenPos(Vec2);   // top-left, Y-down, viewport-relative
struct NdcPos(Vec2);      // center, Y-up, clip-space
struct UvPos(Vec2);       // 0..1, Y-down
```
Conversions are the ONLY way to cross types. Compiler error if you
mix. Eliminates entire class of "I forgot which Y" bugs forever.

### Phase D — Test matrix (~half day)
Round-trip + corner-case tests for every pair:
- `image_to_frame(frame_to_image(p, sz), sz) == p`
- For known corners (top-left, top-right, bottom-left, bottom-right,
  center) verify mapping into each other space.

### Triggers for executing this refactor
- Add this to the active phase queue WHEN starting Paint workstream
  (Phases 2-5 of Wave 8 paint comp) — paint adds yet another coord
  layer (brush position on canvas), so centralizing FIRST prevents
  another bug class.
- OR after first user report of new Y-flip bug post-this-session.

### Known bugs probably belonging to the same coord-system class
(not yet fixed; will need bisect or audit):
- "Image renders in bottom-right corner of canvas at frame 33" —
  reproduced on both CPU and GPU backend; same on TGA and EXR
  sources. User hint: cache/time also disagrees with display ("время
  и отображение разъезжается"). Hypothesis: regression in
  `6c551ce` (playa-time extraction). Bisect path:
  `git checkout 30ffc8f` (commit before Wave 7-pre big merge), build,
  test same scene. If clean → diff `30ffc8f..6c551ce` for coord /
  speed / round migration in `comp_node.rs` (221 LOC changed),
  `attrs.rs` (45 LOC), `keys.rs` (12 LOC).
- Cache/time desync reported by user separately — possibly the
  same root cause. Speed::scale_timeline_to_src migrations with
  Round::Round semantics may produce off-by-one frame_idx that
  mis-keys the cache.

### Sanity test scene (build once, reuse for any future coord
audit / bisect / fix)
- 720×576 comp at 30 FPS
- Single layer: solid 720×576 PNG (e.g. red, full opacity)
- Layer attrs: pos=(0,0,0), scale=(1,1,1), rot=(0,0,0), pivot=(0,0,0)
- Expected: red fills canvas, no offset, no crop, no Y-flip
- If fails on this MINIMAL case → coord bug in compose / viewport
- Add same scene with a second 720×576 layer half-transparent on
  top to test compositor stacking
- Add same scene with layer scale=2.0, pos=(360,288) to test
  transform-with-non-identity path (centers a 2× zoom-in)

---

## Original TODO

1. Explore timecode support
2. Take EDL or OpenTimelineIO as input
3. Explore OCIO/OIIO integration
4. Explore Shotgrid integration
5. Explore headless operations: core without GUI, Python API only
6. Python API via RustPython - expose all major classes, widgets, dialogs, and core functionality
