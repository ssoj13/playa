# Timeline + Layer Bounds Audit

Repo: `playa` (Rust workspace). Engine: `playa-engine`, UI: `playa-ui`, App: `playa-app`.
Aim of audit: AE-style behavior — comp bounds derived from layer in/out, no stored
`comp.duration`. Reality below.

## Current architecture

```
Project
└── media: HashMap<Uuid, Arc<NodeKind>>          # all nodes (FileNode, CompNode, ...)
    └── CompNode { attrs, layers: Vec<Layer> }
         attrs (PERSISTED, written by rebound):
             A_IN  ("in"),  A_OUT ("out"),       # <-- comp bounds, STORED
             A_TRIM_IN, A_TRIM_OUT,               # play area offsets
             A_FRAME (playhead, non-DAG),
             A_FPS, A_WIDTH, A_HEIGHT,
             A_SPEED  (declared but unused on Comp)
         layers: per-layer
             attrs:
                 A_IN ("in")    = layer start in PARENT timeline
                 A_OUT ("out")  = stored = start+duration  (kept stale, see B5)
                 A_SRC_LEN      = source length cache
                 A_TRIM_IN/OUT  = OFFSETS in SOURCE frames (scaled by speed)
                 A_SPEED        = playback speed
                 width/height + transform/effects

Player (singleton state on app)
    active_comp: Uuid
    play_range(project) -> comp.play_range(true) -> comp.work_area()
                            -> (_in()+trim_in, _out()-trim_out)
    set_frame: clamps to [comp._in() .. comp._out()]   # <-- stored bounds
    advance_frame: clamps to play_range (= work_area)
    on_activate: comp.rebound() recomputes _in/_out from layers
```

So **two sources of truth coexist**: the stored `A_IN`/`A_OUT` on `CompNode` and the
"derived" set computed from layers in `bounds()`/`bounds_internal()`. They are kept
in sync only through `rebound()` calls scattered across mutation sites.

## Where bounds live (inventory)

| File:line | Field / Fn | Stored or Computed | Source of truth? |
|---|---|---|---|
| crates/playa-engine/src/entities/comp_node.rs:296-309 | `CompNode::new` writes `A_IN`, `A_OUT`, `A_FPS`, `A_FRAME`, dim | Stored (initial) | yes |
| crates/playa-engine/src/entities/comp_node.rs:386-422 | `CompNode::bounds(use_trim, selection_only, media)` | Computed from layers (dynamic src_len) | "true" answer, not used for clamp |
| crates/playa-engine/src/entities/comp_node.rs:447-470 | `CompNode::bounds_internal` (no media) | Computed from layers (stored src_len) | secondary, used by rebound |
| crates/playa-engine/src/entities/comp_node.rs:474-504 | `rebound()` writes `A_IN/A_OUT` from `bounds_internal(true)`, also resets W/H | Stored ← Computed snapshot | what rest of code reads |
| crates/playa-engine/src/entities/comp_node.rs:353-361 | `play_range()` = `work_area()` = `(_in()+trim_in, _out()-trim_out)` | Stored derivation | drives playback |
| crates/playa-engine/src/entities/node.rs:158-203 | `Node` trait defaults: `play_range` from `attrs.layer_start/end()`, `_in/_out` from `A_IN/A_OUT` | Stored | trait base |
| crates/playa-engine/src/core/player.rs:219-235 | `total_frames`, `play_range` go via `n.play_range(true)` | Stored derivation | playback engine |
| crates/playa-engine/src/core/player.rs:482-496 | `set_frame` clamps to `comp._in()..=comp._out()` | Stored | scrubbing clamp |
| crates/playa-engine/src/core/player.rs:238-268 | `set_play_range` clamps to `comp._in()..=comp._out()` | Stored | trim work-area clamp |
| crates/playa-app/src/main_events.rs:455-467 (ResetPlayRange / ResetCompPlayArea around 1313) | resets play area to `comp._in()/_out()` | Stored | reset path |
| crates/playa-ui/src/widgets/timeline/timeline_ui.rs:600-620 | `extended_min/max = (layers ∪ comp_bounds) ± margin` | Computed | for canvas drawing only |
| crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1248-1270 | overlay shading uses `comp.play_range(true)` | Stored derivation | viz |
| crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:191-196 | playhead from `comp.frame()` | Stored | viz |
| crates/playa-app/src/server/api.rs:82-90 | `CompSnapshot { duration, in_frame, out_frame }` over RPC | Stored | external API |
| crates/playa-engine/src/entities/project.rs:440-486 | media import sets layer in/out and creates comp with `A_OUT = duration` | Stored | initial import path |

`rebound()` call sites (engine only): `on_activate` (380), `add_layer` (551),
`remove_layer` (559), `move_layers` (839), `trim_layers` (887), and three
add-helper paths around 1085/1106/1127 in comp_node.rs.

## Layer in/out fields

| File:line | field | semantics |
|---|---|---|
| comp_node.rs:143-148, 199-201 | `A_IN` ("in") | Layer **start** in parent timeline (== full bar start, "in point") |
| comp_node.rs:144, 204-214 | `A_OUT` ("out") | **Stored value** = `start + duration` from constructor; never re-written when src_len/speed change. Layer.end() ignores it and computes from `src_len/speed`. So `A_OUT` field is dead/misleading — see B5 |
| comp_node.rs:145 | `A_SRC_LEN` ("src_len") | Cache of source length in source frames |
| comp_node.rs:218-230 | `A_TRIM_IN`, `A_TRIM_OUT` | OFFSETS in source frames; scaled by speed → parent frames |
| comp_node.rs:152-153 | `A_SPEED` | Playback speed; clamped 0.001 lower bound |
| comp_node.rs:204-214 | `Layer::end()` | `start + (src_len / speed) - 1` (uses STORED src_len) |
| comp_node.rs:780-800 | `CompNode::get_layer_end / get_layer_work_area` | uses **dynamic** src_len from media — preferred path |
| attrs.rs:687-702 (`layer_start/layer_end`) | trait-level `_in/_out` for non-comp nodes | similar formula |

## Bugs / inconsistencies

### B1 — BLOCKER: dual source of truth for comp bounds
- comp_node.rs:296-309 store `A_IN/A_OUT` at construction; rebound (474) overwrites them; everything downstream (player.set_frame:482-496, set_play_range:248-255, ResetPlayRange in main_events, all UI ruler math) reads the **stored** values.
- Meanwhile `CompNode::bounds()` (386) is the only function that returns the live "derived from layers" answer, and it is used **only** for zoom-to-fit (main_events.rs:690, 702). The clamp/playback path **never** consults it.
- Class: dual-state divergence. Whenever any mutation forgets `rebound()` (or rebound runs with stale `src_len`, see B6), the stored `_in/_out` lies and the playable range is wrong.
- Fix: make `_in/_out` for `CompNode` *computed* (override the trait default), drop the `A_IN/A_OUT` field on `CompNode` from the persisted schema, and recompute lazily / cache invalidated by layer mutations.

### B2 — HIGH: `set_frame` clamps to *stored* `_in/_out`, scrubbing breaks if rebound stale
- player.rs:482-496: `clamped = frame.clamp(comp._in(), comp._out())`.
- If a layer was added/moved without `rebound()` (e.g. direct `comp.layers.push`/`insert` paths — and there are several in main_events paste at line ~1300, and `add_child_layer` mutations that bypass `add_layer`), the playhead can no longer reach the new content.
- Class: clamp uses stored field instead of derived bound.
- Fix: clamp against `bounds(use_trim=false, false, &media)` (the layer-derived range), not against stored `A_IN/A_OUT`.

### B3 — HIGH: empty comp returns `(0, 100)` instead of empty / sane sentinel
- comp_node.rs:392-394 and :448-450 and :465-468 hard-code `(0, 100)` when `layers.is_empty()`.
- player.rs:339, 369-376 then treat `total_frames>0` as "playable", and Player.set_frame clamps to `[_in.._out]` = stored, but the *bounds* function is only consulted for zoom-to-fit.
- Effect: empty comp pretends to be 101 frames long; user gets a non-empty timeline ruler with no content. Inconsistent with AE (empty comp should display the user-set comp duration if a "design size" exists, or 0 frames).
- Class: magic-number fallback hiding an undefined state.
- Fix: split semantics — "design length" (user intent) vs "playable bounds" (derived). Empty → playable = (0,0) or `None`; design length kept separately.

### B4 — HIGH: `rebound()` clobbers comp width/height from "first visible layer"
- comp_node.rs:483-487. Every `add_layer/remove_layer/move_layers/trim_layers/on_activate` overrides comp width/height with the earliest layer's (`get_first_size`).
- Combined with `rebound()` rewriting `A_IN/A_OUT`, this means **the user can't have a comp with size or duration independent of layers** — adding a tiny layer at frame 0 shrinks the entire comp resolution. Direct contradiction with AE model where comp dimensions are user-defined and stable.
- Class: aggressive auto-derive overwriting user intent.
- Fix: separate `comp.design_dim` (user) from `comp.bounds_dim` (derived); rebound should only refresh derived, never write user fields.

### B5 — HIGH: layer's `A_OUT` attr is stored but stale, never read
- Constructor writes `A_OUT = start + duration` (comp_node.rs:144); but `Layer::end()` (204-214) recomputes from `src_len/speed`, ignoring `A_OUT`.
- Move/trim paths only update `A_IN`, `A_TRIM_IN/OUT` — they never refresh `A_OUT`. `move_layers` (820-844) writes only `A_IN`.
- Effect: any external reader of `layer.attrs[A_OUT]` (paste path in main_events.rs ~1247 reads exactly this for offset math!) sees stale data.
- Paste path (main_events.rs:1248-1264) explicitly reads `A_OUT` then writes it back after offset — divergent representation between paste and runtime.
- Class: attribute-vs-method drift; same field encoded twice with different ground truth.
- Fix: drop `A_OUT` from `Layer` schema (compute via `Layer::end()` only), or make `A_OUT` the source and remove the recompute from `src_len/speed`. Pick one.

### B6 — MED: `bounds_internal` uses STORED `src_len` while `bounds` uses DYNAMIC src_len from media
- comp_node.rs:447-470 (internal, called by `rebound`) uses `Layer::work_area()` / `Layer::end()` → stored src_len.
- comp_node.rs:386-422 (public `bounds`) uses `get_layer_work_area`/`get_layer_end` → media's `play_frame_count`.
- Therefore `rebound()` writes `A_IN/A_OUT` based on possibly stale per-layer `src_len`, while UI fit/zoom-to-fit reports the truthful range. They will diverge whenever a source comp's duration changes (nested comps re-trimmed) without re-importing the layer.
- Class: two formulas for the same thing.
- Fix: `rebound()` should accept `&media` and use the dynamic path (or the public `bounds()`). The "no-media" version should not exist.

### B7 — MED: `set_play_range` ignores layer-derived bounds
- player.rs:238-268 clamps work-area to `comp._in()..=comp._out()` — i.e. the stored field. Cannot set work area beyond stored bounds even if layers extend further.
- If user moves/extends layer beyond current `_out` and rebound hasn't propagated (e.g. mid-drag), play range can't reach the new content. Symptom: scrubbing OK, but set in/out (B/N keys around main_events:118-200) silently clamps.
- Class: same as B2, different call site.
- Fix: clamp against derived bounds.

### B8 — MED: ResetCompPlayArea / ResetPlayRange use stored `_in/_out` not layer-derived
- main_events.rs:455-467 and :1313-1320: `comp._in()/_out()`. After scenes that bypass rebound, "Reset" can resize work area to a phantom range.
- Class: stored vs derived again; pervasive.

### B9 — MED: No support for negative comp time at the engine level
- `bounds_internal` empty fallback `(0, 100)` and `set_frame` clamp at `comp._in()` allow negative if `_in` < 0. UI half-supports it: timeline_ui.rs:599 comment says "allows negative starts", and the dragging path (timeline_ui.rs:1016) "Allow negative values". Status strip path (1579-1600) does `state.pan_offset.max(comp_start as f32) as i32` — that's fine for negatives.
- BUT: `total_frames` (player.rs:219-227) computes `(end - start + 1).max(0)` — handles negatives, OK.
- BUT: `Player::step` loop math (player.rs:514+) uses `range_size = play_end - play_start + 1` which is fine if play_start < 0, but the `clamp(i32::MIN, i32::MAX)` on i64 intermediate (511) is OK.
- BUT: file_node.rs:325-339 builds FileNode with `min_frame as i32`/`max_frame as i32` — this is filename frame numbers, can be 0/positive only in practice. Layer start in parent timeline can be negative; nothing forbids it.
- Net: negative comp time is *almost* supported, but no test, and several APIs (CompSnapshot.duration in api.rs:82-90) assume `duration = frame_count = out - in + 1`, which silently underflows if `_out < _in`. Player's `update()` early-returns when `total_frames == 0`, masking that case.
- Class: incidental support, not contract; will rot.
- Fix: explicit decision (allow vs clip), then enforce.

### B10 — LOW: Loop range = work_area, but UI label says "play_range"
- player.rs:374 `play_range(project)` returns work_area (trim-applied). Loop wraps over work_area (player.rs:399-432, step:506). Encode/render goes through the same `comp.play_range(true)` (encode.rs:1336, 2804). Internally consistent — but naming "play_range" is overloaded with both "trimmed work area" and "comp full bounds" depending on context. Refactor target.

### B11 — LOW: Margin in UI uses i32 for canvas frames; potential overflow at extreme zoom-out
- timeline_ui.rs:617 `(ui.available_width() / (config.pixels_per_frame * state.zoom)).ceil() as i32 + 100`. If `pixels_per_frame * zoom` is near 0.0 (zoom clamped to 0.1 at 1480/1485, ppf default 2.0, so min effective ~0.2 → 5*width frames), reasonable. Not a bug today, only a future trap if zoom range is widened.

### B12 — LOW: `frame_count() = (out - in + 1).max(0)` is *inclusive* but rebound stores `_out` from layer.end() which is *also* inclusive
- Convention is inclusive end across the engine (player.rs:222, file_node.rs:393 `last_frame = frame_count - 1`). OK now, but the two-path encoders (encode.rs:1336-1337) compute `play_range.1 - play_range.0 + 1` — consistent. Worth a comment in `node.rs:206-208`.

## Gap to AE-style behavior

- **Comp bounds NOT stored, computed**: violated. `A_IN/A_OUT` are stored on `CompNode`, written by `rebound()`, used by every clamp. (B1, B2, B6, B7, B8)
- **Hard playable bounds = derived from layers**: only one read path (`bounds()` for zoom-to-fit) consults this; the canonical playback path uses the stored copy. (B1, B2)
- **Per-layer in/out + offset**: layer has `A_IN`, `A_TRIM_IN/OUT` (offset semantics). `A_OUT` exists too but is ignored by runtime — confusing dead field. (B5)
- **Layer added beyond bounds → bounds expand**: works *if* the mutation calls `rebound()`. Direct `comp.layers.push`/`insert` paths (paste, layer-reorder) require manual `mark_dirty()` and **do not** call `rebound()` — comment at comp_node.rs:32-34 documents this trap explicitly. So expansion is opt-in, not invariant.
- **Empty comp**: returns sentinel (0,100) — neither AE-correct nor empty. (B3)
- **Comp width/height**: hijacked by rebound from first layer. AE keeps comp dim user-defined. (B4)
- **Negative time**: half-supported. (B9)
- **Loop / Export ranges**: both go through `comp.play_range(true)` → work_area. Consistent. ✓
- **Per-layer time stretching (speed)**: respected in `Layer::end()`/`work_area`/`get_layer_*`. ✓
- **Drag-extend layer in timeline**: `trim_layers` calls `rebound()`. ✓
- **Source-of-truth divergence between `bounds()` (dynamic src_len) and `bounds_internal()` (stored src_len)**: B6.

## Migration recommendation

Concrete refactor path (in order):

1. **Drop stored `A_IN/A_OUT` on `CompNode` schema.** Override `Node::_in/_out` for `CompNode` to delegate to a cached `compute_bounds()` that returns derived bounds. Persist only user-intent fields: `design_dim`, `design_fps`, optional `design_duration_hint`.

2. **Introduce `CompBoundsCache`** on `CompNode` (interior mutability or computed-on-demand):
   ```rust
   #[serde(skip)]
   bounds_cache: OnceCell<(i32, i32)>   // invalidated by mark_dirty
   ```
   `compute_bounds()` runs the existing `bounds(true, false, media)` logic. Invalidate in every layer mutation (already wired via `mark_dirty`).

3. **Delete `rebound()`**. Width/height of comp becomes a separate user field (`A_DESIGN_WIDTH`, `A_DESIGN_HEIGHT`). Layer transforms still respect comp dim — but comp dim is no longer hijacked (fixes B4). Provide explicit "Fit Comp to Layers" UI action that *intentionally* writes design dim.

4. **Player clamps against derived bounds**, not stored:
   - `set_frame`: clamp to `comp.compute_bounds(&media)`.
   - `set_play_range`: same.
   - `total_frames`: same path it already uses (`play_range(true)`), but `_in/_out` now come from cache → still works.

5. **Drop layer `A_OUT` from schema** (B5). Make `Layer::end()` the only source. Update paste path in main_events.rs (~1248-1264) to compute new in/out from `start()` + `src_len/speed` instead of attr math.

6. **Trim semantics double-check**: keep `A_TRIM_IN/OUT` as offsets-in-source-frames; document that these are clamped to `[−src_len, +src_len]` (negatives = "hold first/last frame", as comment at comp_node.rs:866-880 already implies).

7. **Empty comp policy**: explicit `Option<(i32,i32)>` from `compute_bounds()`. Player treats `None` as "no playable content" (no clamp, no advance, set_frame is a no-op or stores raw value). UI shows ruler around `design_duration_hint` if user set it, else 0..design_duration_default.

8. **Negative time**: add a unit test asserting bounds with `layer.in = -50` works end-to-end (clamp, scrub, ruler). Status strip / file_node frame indexing already handles it; just lock it down with tests.

9. **External API (CompSnapshot)**: change `duration/in_frame/out_frame` to come from derived bounds; mark old serialized projects with a migration that drops persisted `A_IN/A_OUT` on comps and recomputes on load.

10. **Naming**: rename `play_range` → `work_area` everywhere it means trimmed range; reserve `play_range` for "current playable range = derived bounds" if needed at all. Reduces B10 confusion.
