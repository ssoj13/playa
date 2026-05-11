# TODO

## Coord-system centralization

### Status (2026-05-10)

**DONE** — phases A, B, plus partial C (algebra-checked tests act as
type-safety proxy):

- Phase A audit at `.bughunt/coord_audit.md` (historical record of
  pre-fix state).
- 11 helpers in new `playa-coord` crate (extracted from playa-time):
  `image_to_frame` / `frame_to_image` / `object_to_src` /
  `image_to_natural` / `natural_to_frame` / `frame_to_ndc` /
  `frame_to_viewport` / `viewport_to_screen` / `flip_y` /
  `object_to_src_affine` / `screen_ndc_from_frame_ndc` (+ their
  inverses). All re-exported via `playa_engine::entities::space`.
- 22 round-trip + corner + algebra-identity tests in playa-coord.
- Migration of inline Y/center/zoom-pan math at sites S1-S6, S5, S9
  (viewport.rs, viewport_ui.rs, transform.rs, gizmo.rs, coords.rs).
- New space added: `ImageNatural` (bottom-left, Y-up) — the
  user-natural Cartesian addressing. Helpers ready; UI/storage
  migration deferred to next pass.

Coord systems now (11) — see `playa-coord` crate-level doc.

Single source of truth: `playa-coord` crate (sibling of playa-time).
playa-engine and playa-ui pull through `entities::space` re-export.
Both crates unchanged at the call sites; only the source pivoted.

### Remaining

- **S7 wgpu render path image-in-corner bug** — static analysis of
  the pipeline (renderer.rs quad VBO + UV mapping + ortho_rh proj +
  wgpu Y-up NDC) shows a self-consistent upright-image chain.
  Bug must be in dynamic state at frame 33 (model/view/proj
  recompute, texture upload format mismatch, or specific src-frame
  attrs). Needs the sanity scene below + bisect 30ffc8f..6c551ce
  to localize.
- **S8 layer_blend.wgsl** — design decision: kill GPU compositor
  matrix path (currently degraded per `comp_node.rs:1380-1387`
  comment), OR finish wiring it. Not a coord bug; documentation +
  decision debt.
- **ImageNatural promotion to UI / storage** — layer attrs
  pos/pivot, status-bar readouts, picker tooltips currently in
  frame coords. Migration to natural is a UX+serialization pass
  ("совместимость не нужна" already greenlit by user). Touch:
  layer attrs (de)serialization, every UI display of a coord,
  scene-file format docs.
- **Phase C — newtype wrappers** [optional]: distinct types per
  space (`struct ImagePos(Vec2)`, `struct FramePos(Vec2)`, etc.)
  so the compiler rejects accidental mixing. Algebra-identity
  tests in playa-coord cover the same drift class but at runtime
  only.

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

### Bisect path (when scene exists)
`git checkout 30ffc8f` (commit before Wave 7-pre big merge), build,
test same scene. If clean → diff `30ffc8f..6c551ce` for coord /
speed / round migration in `comp_node.rs` (221 LOC changed),
`attrs.rs` (45 LOC), `keys.rs` (12 LOC). User hint: cache/time
also disagrees with display ("время и отображение разъезжается") —
possibly Speed::scale_timeline_to_src + Round::Round off-by-one
mis-keying the cache.

---

## Original TODO

1. Explore timecode support
2. Take EDL or OpenTimelineIO as input
3. Explore OCIO/OIIO integration
4. Explore Shotgrid integration
5. Explore headless operations: core without GUI, Python API only
6. Python API via RustPython - expose all major classes, widgets, dialogs, and core functionality
