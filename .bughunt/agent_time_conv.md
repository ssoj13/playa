# Time & Coordinate Conversion Audit

Scope: playa workspace (playa-app, playa-engine, playa-events, playa-io, playa-ui, playa-py, src/).
playa-ffmpeg is the vendored FFmpeg binding crate; bindings (SMPTE/timecode enum aliases, Rational, rescale, color spaces) are pass-through to libav and OUT OF SCOPE — only call sites in app code matter. xtask is build tooling, no time logic.

Quick verdict: there is NO unified time/coordinate module. Conversions are scattered, frame is universally `i32`, fps is `f32` (never `Rational`), seconds appear only as encode/jog cosmetics. `speed` is `f32` divisor smeared across 6+ sites doing the same math with inconsistent rounding. SMPTE drop-frame timecode does not exist in app code at all (only ffmpeg binding aliases). Audio sample-rate conversion not present (project is video/image only).

## Inventory

| File:line | Function | Input → Output | Notes |
|---|---|---|---|
| crates/playa-engine/src/entities/space.rs:44 | `image_to_frame(p, size)` | image px (TL, Y-down) → frame px (centered, Y-up) | f32, no rounding |
| crates/playa-engine/src/entities/space.rs:54 | `frame_to_image(p, size)` | frame → image | f32 |
| crates/playa-engine/src/entities/space.rs:65 | `object_to_src(p, src_size)` | object → source px | identical math to frame_to_image |
| crates/playa-engine/src/entities/space.rs:79 | `to_math_rot(deg)` | CW° → CCW rad | unit/sign convert |
| crates/playa-engine/src/entities/space.rs:85 | `from_math_rot(rad)` | CCW rad → CW° | inverse |
| crates/playa-engine/src/entities/transform.rs:315 | `sample_bilinear` | sub-pixel sample | uses `floor` then frac, image-space |
| crates/playa-engine/src/core/player.rs:46-48 | `FPS_PRESETS` `[f32]` | jog presets | hard-coded integers; NO 23.976 / 29.97 |
| crates/playa-engine/src/core/player.rs:337-364 | `Player::update` | wall-time elapsed → maybe advance frame | `1.0 / fps_play()` (f32), uses `Instant`, single-frame advance per tick |
| crates/playa-engine/src/entities/node.rs:188-208 | `Node::fps/_in/_out/frame_count` | attrs → i32/f32 | trait defaults |
| crates/playa-engine/src/entities/attrs.rs:655-665 | `Attrs::layer_start` | (in, trim_in, speed) → i32 parent frame | `(trim_in as f64 / speed as f64).round()`, f64 intermediate, speed clamped 0.1..4.0 |
| crates/playa-engine/src/entities/attrs.rs:670-685 | `Attrs::layer_end` | (in, src_len, trim_in/out, speed) → i32 | `.round()` for visible_timeline, speed clamp 0.1..4.0 |
| crates/playa-engine/src/entities/attrs.rs:694-703 | `Attrs::full_bar_end` | (in, src_len, speed) → i32 | uses `.ceil()` (vs `.round()` in layer_end — divergent) |
| crates/playa-engine/src/entities/comp_node.rs:204-214 | `Layer::end` | (start, src_len, speed) → i32 | `((src_len as f32 / speed) as i32) - 1` — NO rounding (truncates), f32, speed via `.abs().max(0.001)` (different clamp than attrs.rs) |
| crates/playa-engine/src/entities/comp_node.rs:218-230 | `Layer::work_area` | (start, end, trim_in/out, speed) → (i32,i32) | `(trim_in as f32 / speed) as i32` truncates, f32 |
| crates/playa-engine/src/entities/comp_node.rs:239-249 | `Layer::parent_to_local` | parent frame → src local frame | `(offset as f32 * speed) as i32` truncates, f32 |
| crates/playa-engine/src/entities/comp_node.rs:765-778 | `Comp::get_layer_end` | (layer, media) → i32 | duplicate of `Layer::end` but reads dynamic src_len from media; same f32 truncation |
| crates/playa-engine/src/entities/comp_node.rs:781-800 | `Comp::get_layer_work_area` | (layer, media) → (i32,i32) | duplicate of `work_area` with dynamic src_len |
| crates/playa-engine/src/entities/comp_node.rs:846-880 | `trim_layers` | delta → trim_in/out adj | `(delta as f32 * speed).round() as i32` |
| crates/playa-engine/src/entities/comp_node.rs:1089-1125 | `set_layer_play_start/_end` | new_play_start/end → trim_in/out src frames | `((delta) as f32 * speed) as i32` truncates (no `.round()`) |
| crates/playa-engine/src/entities/comp_node.rs:1255-1259 | render path | parent_frame → source_frame | `(source_in + local_frame).clamp(...)` |
| crates/playa-engine/src/entities/project.rs:766 | `create_comp` default | `(fps * 5.0) as i32` end | f32 → i32 truncation, "5 seconds" assumption |
| crates/playa-engine/src/core/player.rs:46 | `FPS_PRESETS` | constants | `1,2,4,8,12,24,30,60,120,240,480,960` — no NTSC fractional rates anywhere |
| crates/playa-app/src/server/api.rs:266-275 | http SetFps | `f32 parse` | accepts 0.001..960; mismatch with prefs comment "0.001 and 960" but check is `> 0.0 && <= 960.0` — 0.0001 passes |
| crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:152-158 | `frame_to_screen_x` | (frame f32, rect, ppf, zoom) → screen x f32 | linear, no rounding |
| crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:161-168 | `screen_x_to_frame` | screen x → frame f32 | linear, callers `.round() as i32` or `.floor() as i32` (inconsistent) |
| crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:233-234 | visible range | `pan_offset.floor() / .ceil()` | ok |
| crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:357-358 | `row_to_y` | row idx → y px | f32 |
| crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1015,1090,1129,1172 | drag delta | px → frames | `(delta_x / (config.pixels_per_frame * state.zoom)).round() as i32` (4 copy-pastes) |
| crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1187,1198 | slide tool | `(delta_frames as f32 * speed).round() as i32`, `(src_len as f32 / speed).ceil() as i32` | `.ceil` vs `.round` inconsistent w/ attrs.rs |
| crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1289-1304 | drop preview | `screen_x_to_frame(...).round()` then second copy `.floor()` | TWO conversions of same input on adjacent lines (`raw_drop_frame` rounded, `drop_frame = raw_drop_frame` then a parallel `.floor()` call at 1301) |
| crates/playa-ui/src/widgets/viewport/viewport.rs:547-555 | `mouse_to_normalized` | mouse_x → 0..1 | f32 |
| crates/playa-ui/src/widgets/viewport/viewport.rs:557-561 | `normalized_to_pixel` | 0..1 → px | inverse pair |
| crates/playa-ui/src/widgets/viewport/viewport.rs:563-570 | `normalized_to_frame` | 0..1 → i32 | `(clamped * (total_frames - 1) as f32).round() as i32` — inclusive end logic |
| crates/playa-ui/src/widgets/viewport/viewport.rs:323-330, viewport_ui.rs:362-369 | `fit(local_x, ...)` scrub | local_x → frame f32 → i32 | `.round()`; viewport_ui repeats the formula |
| crates/playa-ui/src/widgets/viewport/gizmo.rs (8 hits), pick.rs (2 hits), transform.rs (8 hits) | uses `image_to_frame`/`frame_to_image`/`object_to_src` | model/inverse model | f32 cumulative; all happens per-pixel |
| crates/playa-io/src/video/ffmpeg_imp.rs:39-65 | media metadata | stream rational → `f64` fps | `fps_rational.numerator() as f64 / .denominator() as f64`; `frame_count = (duration_secs * fps).round() as usize` |
| crates/playa-io/src/video/ffmpeg_imp.rs:118-131 | `frame_to_pts` | frame# → pts | uses ffmpeg `av_rescale_q(frame_num, frame_tb, stream_tb)` — only correct rational arithmetic in the codebase |
| crates/playa-ui/src/dialogs/encode/encode.rs:1297-1316 | `fps_to_rational(fps: f32)` | f32 fps → (num, den) | NTSC table 23.976/29.97/47.952/59.94/119.88 → 24000/1001 etc with `±0.01` tolerance; else `*1000` scale fallback |
| crates/playa-ui/src/dialogs/encode/encode.rs:1480-1487 | encoder time_base | reciprocal of fps_to_rational | `set_time_base(den/num)` |
| crates/playa-ui/src/dialogs/encode/encode.rs:1716-1860 | encode loop | `pts: i64`, `+= 1` per frame, time_base = 1/fps | OK at integer fps + NTSC; relies on `rescale_ts` |
| crates/playa-engine/src/entities/effects/blur.rs:115 | `(radius * 2.0).ceil() as i32` | px geom | spatial only |
| crates/playa-engine/src/entities/text_node.rs:256 | `(max_x.ceil(), max_y.ceil())` | text bbox | spatial only |

Functions named `*_to_seconds*`, `*_to_timecode*`, `*_to_ms*`, `frames_to_seconds`, `seconds_to_frames`, `timecode_to_*` — **DO NOT EXIST** in app code. Time is everywhere expressed as i32 frame index. Wall-clock seconds appear only inside `Player::update` to gate the next-frame advance, never persisted, never displayed in TC form.

## Duplicates / Near-duplicates

Group A — "src_len divided by speed minus 1":
- attrs.rs:670-685 `Attrs::layer_end` (f64, `.round()`)
- attrs.rs:694-703 `Attrs::full_bar_end` (f64, `.ceil()`)
- comp_node.rs:213 `Layer::end` (f32, truncates, no rounding)
- comp_node.rs:777 `Comp::get_layer_end` (f32, truncates)
- timeline_ui.rs:1198 `(src_len as f32 / speed).ceil()` (slide tool)
Five sites computing the same quantity, three different rounding modes (`round`, `ceil`, truncate), two different precisions (f32 vs f64), two different speed-clamp policies (`clamp(0.1,4.0)` vs `.abs().max(0.001)`).

Group B — "trim_in scaled to timeline frames":
- attrs.rs:661 `(trim_in as f64 / speed as f64).round()`
- comp_node.rs:227 `(trim_in as f32 / speed) as i32` (truncate)
- comp_node.rs:796 (Comp::get_layer_work_area) — same as 227
Three call sites, two different roundings.

Group C — "delta_px → delta_frames":
- timeline_ui.rs:1015, 1090, 1129, 1172 — exact same expression copy-pasted 4×: `(delta_x / (config.pixels_per_frame * state.zoom)).round() as i32`

Group D — "frame ↔ screen-x":
- timeline_helpers.rs:152/161 (canonical pair)
- viewport.rs:323-330 + viewport_ui.rs:362-369 — second `fit(...)` formula in two places, separate from timeline (different mapping: maps mouse to play_range, not pan/zoom)

Group E — "image-space ↔ frame-space":
- space.rs:54 `frame_to_image` and space.rs:65 `object_to_src` are line-for-line identical (same `(x + w/2, h/2 - y)`); should be one function.

Group F — "speed clamp":
- attrs.rs:659/675/697 → `.clamp(0.1, 4.0)`
- comp_node.rs:212/226/246/776/791 → `.abs().max(0.001)`
Two divergent invariants for the same attribute. UI slider in timeline_ui.rs:481 enforces `0.1..=4.0`; comp_node code allows down to 0.001 silently.

## Inconsistencies

1. **Rounding mode**: `.round()`, `.floor()`, `.ceil()`, and bare cast (truncate) are mixed across the equivalent calculation `src_len / speed`. Going through the same edit produces different visible bar lengths depending on which function the caller landed on.
2. **Precision**: attrs.rs uses f64 intermediates (good), comp_node.rs uses f32 (bad for src_len > ~16M frames; not realistic but inconsistent).
3. **Speed safe range**: `clamp(0.1, 4.0)` vs `.abs().max(0.001)` — two policies, three orders of magnitude apart at the low end.
4. **fps source mixup site (HIGH)**: in `playa-engine::entities::project.rs::create_comp` (line 766), `(fps * 5.0) as i32` produces 119 frames for fps=23.976 (truncation), 149 for fps=29.97 — silent NTSC truncation.
5. **fps representation**: app uses `f32` everywhere (`Player::fps_play`, `CompNode::fps`, `Layer.fps_base`, `EncoderSettings.fps`). Only `playa-io::video::ffmpeg_imp` keeps the original `Rational` and converts via `as f64 / as f64` — that loses exactness on the way into `attrs::set("fps", AttrValue::Float(meta.fps as f32))` (file_node.rs:48 / dispatch.rs:74). 24000/1001 enters as `0x1.7f9d54p+4` f32 (~23.97602), matches NTSC table at `±0.01` tolerance — works by luck, not by design.
6. **Timeline scrub rounding** (`screen_x_to_frame(...).round()`, helpers.rs:345) vs **drop preview** doing `.round()` and `.floor()` of the same `screen_x_to_frame` on adjacent lines (timeline_ui.rs:1289-1304) — the second `.floor()` likely overshoots by 1 frame near boundaries.
7. **`frame_count` semantics**: trait default in `node.rs:206` is `out - in + 1` (inclusive); but `Player::update` advances by 1 in a half-open loop with explicit `play_end` clamp. Project total frames computed via `total_frames(&Project)` (player.rs:219) returns `play_end - play_start + 1` — consistent — but UI label `format!("{}f", frame_count)` (project_ui.rs:206) shows the inclusive count; encoder uses `play_range.1 - play_range.0 + 1` (encode.rs:1337). All consistent on inclusive-end. Good.
8. **`A_OUT` defaulting**: `_out()` falls back to `A_SRC_LEN` if missing (node.rs:181-184); but `frame_count = out - in + 1` then becomes `src_len + 1` if `_in = 0`, off-by-one against expected `src_len`. Worth a check.

## Drop-frame / SMPTE compliance

**Verdict: not implemented.** No app-level timecode formatter exists. SMPTE/Timecode strings in the codebase are exclusively libav binding enum aliases (color transfer / packet side data / frame side data). There is no `frame_to_timecode`, no `format_tc`, no DF/NDF flag, no 24h wraparound, no negative-time handling. Status bar shows raw frame index (`{}f  {}fps`).

NTSC rates ARE recognised at encode time (`fps_to_rational` table in encode.rs:1297) but only as encoder time_base — playback engine still computes frame durations in `f32` seconds (`1.0 / fps_play`).

## Coordinate transforms

| From | To | Function | File:line | Type |
|---|---|---|---|---|
| screen px | timeline frame (f32) | `screen_x_to_frame` | timeline_helpers.rs:161 | f32 linear |
| timeline frame (f32) | screen px | `frame_to_screen_x` | timeline_helpers.rs:152 | f32 linear |
| mouse local x | normalized 0..1 | `Scrubber::mouse_to_normalized` | viewport.rs:547 | f32 |
| normalized 0..1 | screen px | `Scrubber::normalized_to_pixel` | viewport.rs:557 | f32 |
| normalized 0..1 | frame i32 | `Scrubber::normalized_to_frame` | viewport.rs:563 | round |
| local x | frame i32 | `fit(local_x, ...).round()` | viewport.rs:323, viewport_ui.rs:362 | duplicated formula |
| image px | frame px (centered, Y-up) | `image_to_frame` | space.rs:44 | f32 |
| frame px | image px | `frame_to_image` | space.rs:54 | f32 |
| object px | source px | `object_to_src` | space.rs:65 | f32 (= frame_to_image) |
| world | screen | model/view/projection matrices | viewport.rs:360-379 | f32 4x4 |
| sub-pixel | bilinear sample | `sample_bilinear` | transform.rs:315 | floor + frac |
| pick.rs | screen → world | gizmo hit test | viewport/pick.rs | f32 |

NDC space is implicit (the GL projection in viewport.rs builds an ortho matrix from `image_size`/zoom/pan); not exposed via named functions. No explicit `screen_to_ndc` / `ndc_to_clip` helpers.

Cumulative drift risk is real: image→frame→object→model→view→projection happens entirely in f32, with no tests anywhere covering round-trip identity. Pivot/anchor handling for rotation (CW+ user vs CCW+ math via to_math_rot) is the one explicit unit-system boundary that's documented.

## Tests

61 `#[test]` annotations exist. Conversion-relevant subset:
- `playa-engine/src/entities/comp_node.rs:1800-1839` — only checks `_in/_out/fps`, layer creation, `start()`/`end()` for one trivial case (start=10, src_len=50, no speed/trim).
- `playa-engine/src/entities/transform.rs:589, 629` — bilinear sample tests.
- `playa-engine/src/entities/file_node.rs:502-575` — fps default, file detection.
- `playa-engine/src/entities/project.rs:1095-1145` — comp creation; uses `fps * 5.0` end formula but doesn't assert frame count vs NTSC.

**No test covers**:
- `layer_start`/`layer_end` with speed != 1.0
- `parent_to_local` round-trip
- `screen_x_to_frame ∘ frame_to_screen_x = id`
- `image_to_frame ∘ frame_to_image = id`
- NTSC fps surviving f32 conversion
- Rounding mode equivalence between attrs.rs and comp_node.rs
- Negative trim values (the comments in comp_node.rs:866-875 say "negative trim_in = extend before source start (hold first frame)" — semantics not asserted)

## Bugs found

### B1 (HIGH): Three rounding modes produce divergent layer_end / full_bar_end / Layer::end for same input
- `crates/playa-engine/src/entities/attrs.rs:679-681` — `Attrs::layer_end` uses `.round()`
- `crates/playa-engine/src/entities/attrs.rs:699-701` — `Attrs::full_bar_end` uses `.ceil()`
- `crates/playa-engine/src/entities/comp_node.rs:213` — `Layer::end` truncates (no rounding)
- `crates/playa-engine/src/entities/comp_node.rs:777` — `Comp::get_layer_end` truncates
- Evidence: speed=1.5, src_len=10. attrs.layer_end full_bar = `(10/1.5).round() = 7`, attrs.full_bar_end = `(10/1.5).ceil() = 7` (lucky), Layer::end = `(10/1.5) as i32 = 6`. After +1/-1 offsets the visible bar in the timeline differs from "full bar" by 1 frame depending on which getter the caller used.
- Why: callers pick whichever is convenient. Drop-preview uses `Comp::get_layer_end`, drag-trim uses `Layer::end`, status bar reads `attrs.layer_end`. Visible artifacts: layer extends 1 frame past playhead end, last frame "duplicates" or "drops" depending on speed value.
- Class-of-bug check: the same pattern appears for `trim_in / speed` in 5 sites with two different rounding modes (Group B above) — same off-by-one for the start side.
- Proposed fix: single `frames_at_speed(src_frames: i32, speed: f32) -> i32` in unified crate, fixed rounding rule (recommend `.round()` for "visible duration in timeline frames" because `.ceil()` over-extends and truncate under-extends; document explicitly).

### B2 (HIGH): Speed-clamp inconsistency: `clamp(0.1,4.0)` vs `.abs().max(0.001)`
- `crates/playa-engine/src/entities/attrs.rs:659, 675, 697` — `clamp(0.1, 4.0)`
- `crates/playa-engine/src/entities/comp_node.rs:212, 226, 246, 776, 791` — `.abs().max(0.001)`
- UI: `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:481` slider `0.1..=4.0`
- Evidence: user sets `speed=0.05` via API/save-file (no UI clamp on load): attrs.rs treats it as 0.1 → layer_end = `(src_len/0.1).round() = 10*src_len`. comp_node.rs treats it as 0.05 → Layer::end = `(src_len/0.05) as i32 = 20*src_len`. Same layer reports two different end frames depending on which method called.
- Why: deserialised speed bypasses UI slider; no central validator.
- Class-of-bug: `.abs()` in comp_node.rs hides negative speed (user feature for reverse playback?) but attrs.rs doesn't `.abs()` — passing `speed=-1.0` makes attrs return negative offset (`trim_in / -1.0 = -trim_in`), comp_node returns positive — divergent.
- Proposed fix: validate speed once at attribute set time; add `attrs::set_speed(f32) -> Result<...>` that clamps to a single canonical range (or document negative speed as "reverse", store sign, use `.abs()` consistently). Eliminate ad-hoc clamps at read sites.

### B3 (MED): `playa-engine/src/entities/project.rs:766` truncates NTSC fps in default duration
- `let end = (fps * 5.0) as i32;`
- Evidence: fps = 23.976 → `5 * 23.976 = 119.88 → as i32 = 119` (4.96s); fps = 29.97 → `149` (4.97s). Default "5 second" comp is short by ~1 frame.
- Why: cast truncates, not rounds.
- Class-of-bug check: every f32 → i32 cast in conversion code suffers similar truncation (Layer::end, parent_to_local, get_layer_work_area). 12 sites grep'd by pattern `as f32 / speed) as i32`. All systematically lose up to 1 frame.
- Proposed fix: `(fps * 5.0).round() as i32`; add `frames_for_seconds(secs: f32, fps: f32) -> i32` helper.

### B4 (MED): `screen_x_to_frame` rounded vs floored in adjacent lines
- `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1289-1301`
- Evidence:
  ```
  let raw_drop_frame = screen_x_to_frame(...) ... .round() as i32;  // line 1295
  let drop_frame = raw_drop_frame;                                   // line 1304
  ...
  .floor() as i32;                                                   // line 1301
  ```
  Two interpretations of the same drop position computed on adjacent lines, used for different fields of the same DropPayload. Off-by-one snap target.
- Why: copy-paste during refactor; no test pinned the expected snap behaviour.
- Class-of-bug check: timeline_helpers.rs:233-234 also mixes `.floor()` (visible_start) and `.ceil()` (visible_end) — that one is intentional (range bracket).
- Proposed fix: one rule for "drop snaps to nearest frame" (`.round()`); store once.

### B5 (MED): NTSC fps survives only by f32 tolerance luck
- `crates/playa-io/src/video/ffmpeg_imp.rs:50` reads `f64 = num/den`, then **caller** stores as `f32` (file_node.rs:48 `AttrValue::Float(meta.fps as f32)`).
- `fps_to_rational` (encode.rs:1306) tolerance is `±0.01`. The f32 of 24000/1001 is `23.976025` — within 0.01 of 23.976, so it round-trips. But the f32 of 30000/1001 is `29.970032`, also within 0.01. Currently OK.
- Evidence: `1.0 / 23.976` (f32) used for frame duration in Player::update is `0.04170769` — over 1 hour drifts ~0.6 frame from true 1001/24000. Player advances frame on `elapsed >= frame_duration` so this drift accumulates as visible playhead-vs-wallclock skew over long playback. Acceptable for preview (Instant skew not visible to user) but breaks any "playhead = wallclock" assumption.
- Why: lossy conversion; no Rational type retained.
- Proposed fix: store fps as `(num: u32, den: u32)` rational; expose `fps_f32()` for UI display only. Changes serialisation format — design migration.

### B6 (MED): Negative time / negative-frame handling unspecified
- `crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:236-241` does manual sign branch for visible-start alignment — proves negative frames CAN appear on the ruler.
- `Player::set_frame` (player.rs:482-491) clamps to `(comp_start, comp_end)` only; nothing prevents `comp_start = -100`.
- `Layer::parent_to_local` (comp_node.rs:247-248): `(parent_frame - start) as f32 * speed` — negative offsets become negative source frames, then `(source_in + local_frame).clamp(source_in, source_out)` (comp_node.rs:1259) clamps. So accidentally negative `parent_frame - start` returns `source_in` for all such frames. No assert, silent.
- Class-of-bug: there is no documented invariant for what negative-frame means in the timeline. Either disallow at attr-set time or document.

### B7 (LOW): Cumulative f32 drift in nested coordinate transforms
- All viewport math (gizmo, pick, transform.rs uses `image_to_frame`/`frame_to_image`/`object_to_src`) is f32. With image sizes >= 8K and zoom < 0.001, the pixel mantissa runs out (~7 decimals).
- Evidence: no test, no measurement; symptomatic only at extreme zoom out / 8K+.
- Class-of-bug: matches typical egui app — egui itself uses f32 throughout. Not worth fixing standalone, but a unified module should at least mark hot-paths and avoid double conversions.

### B8 (LOW): `Scrubber::normalized_to_frame` uses `(total_frames - 1)`
- `crates/playa-ui/src/widgets/viewport/viewport.rs:566` — `(clamped * (total_frames - 1) as f32).round() as i32`. Maps 0..1 to 0..(total_frames-1). Inclusive convention. Consistent w/ encoder loop (`frame_idx in play_range.0..=play_range.1`). OK but the `-1` is undocumented; future caller may change to `total_frames` and shift all scrub-frame mappings by one.

### B9 (LOW): No 24h wraparound, no overflow guard for `pts: i64` increment
- encode.rs:1716,1860 — `pts += 1` per encoded frame. `i64` will not overflow in practice (>290 billion years at 1 fps). Mention only because no SMPTE TC layer to worry about wrap.

## Recommendation for unified crate

Crate name: `playa-time` (or `playa-coords` if coordinate transforms folded in — recommended, because they share the rounding/precision discipline and integer-vs-float boundary).

### Public API surface (proposal)

```rust
// fps as exact rational
pub struct Fps { pub num: u32, pub den: u32 }
impl Fps {
    pub const NTSC_24: Self;       // 24000/1001
    pub const NTSC_30: Self;       // 30000/1001
    pub const NTSC_60: Self;       // 60000/1001
    pub fn from_f32_lossy(v: f32) -> Self;   // existing fps_to_rational logic
    pub fn as_f64(&self) -> f64;
    pub fn as_f32(&self) -> f32;
    pub fn ticks_per_frame(&self, time_base: u32) -> i64; // for ffmpeg
}

// time conversions (frame as canonical unit)
pub fn frames_to_seconds(frames: i32, fps: Fps) -> f64;
pub fn seconds_to_frames(secs: f64, fps: Fps, mode: Round) -> i32; // floor/round/ceil explicit
pub fn duration_frames(secs: f64, fps: Fps) -> i32;                 // = seconds_to_frames(round)

pub enum Round { Floor, Round, Ceil, Trunc }

// timecode (if needed later)
pub struct Timecode { /* hh:mm:ss:ff or :; for DF */ }
pub fn frames_to_tc(frames: i32, fps: Fps, drop_frame: bool) -> Timecode;
pub fn tc_to_frames(tc: Timecode, fps: Fps) -> i32;

// speed-aware layer math (replaces all Group A/B duplicates)
pub struct Speed(f32);
impl Speed {
    pub fn new(v: f32) -> Self;        // single source of truth for clamp
    pub const ONE: Self;
    pub fn scale_src_to_timeline(&self, src_frames: i32, mode: Round) -> i32;
    pub fn scale_timeline_to_src(&self, tl_frames: i32, mode: Round) -> i32;
}

// coord transforms (move out of playa-engine::entities::space)
pub fn image_to_frame(p: Vec2, size: UVec2) -> Vec2;
pub fn frame_to_image(p: Vec2, size: UVec2) -> Vec2;
pub fn object_to_src(p: Vec2, src_size: UVec2) -> Vec2;          // alias of frame_to_image
// rotation convention helpers (existing)
pub fn user_rot_to_math_rot(deg: f32) -> f32;
pub fn math_rot_to_user_rot(rad: f32) -> f32;

// timeline px ↔ frame (move from timeline_helpers.rs)
pub struct PixelsPerFrame(f32);
pub fn frame_to_screen_x(frame: f32, origin_x: f32, ppf: PixelsPerFrame, zoom: f32) -> f32;
pub fn screen_x_to_frame(x: f32, origin_x: f32, ppf: PixelsPerFrame, zoom: f32, mode: Round) -> i32;
```

### What stays in callers

- All egui-specific glue (Sense, Rect, Painter) stays in playa-ui.
- `Player::update`, `advance_frame`, `Instant`-based timing stays in playa-engine — unified crate provides only the per-frame-duration helper (`fps.frame_duration_secs() -> f64`).
- ffmpeg `time_base`/`pts`/`rescale_ts` stays in playa-ui::dialogs::encode and playa-io::video::ffmpeg_imp — these use libav rationals correctly already.
- `parent_to_local` stays as a method on `Layer` (it needs layer state) but DELEGATES rounding to `Speed::scale_timeline_to_src(_, Round::Round)`.

### Migration risk

- Low blast radius for the **coordinate** half: `space.rs` is consumed by 3-4 files (gizmo, pick, transform, comp_node), all in playa-engine and playa-ui — a straight `pub use` re-export keeps callers compiling.
- Medium risk for **layer math**: 8 call sites in comp_node.rs, 1 in attrs.rs, 4 in timeline_ui.rs share the math but with divergent rounding. Each switch needs a test pinning the previous behaviour first, then explicit rule choice. **The fact that nobody has noticed the rounding divergence so far suggests few users hit non-integer speeds — start there.**
- High risk for **fps representation change**: serialisation hits AttrValue::Float; needs migration step (read float, store rational; on load detect old format). Recommend phased: ship rational type first, keep f32 storage, only canonicalise reads.
- gitnexus impact NOT run for this audit per "read-only" rule; recommended next step before any refactor: `gitnexus_impact({target: "Layer::end"})`, `Attrs::layer_end`, `frame_to_screen_x`, `image_to_frame`, `Layer::parent_to_local`, `fps_to_rational`.

### Ordering for the actual refactor

1. Add `playa-time` crate, move `space.rs` verbatim into it (rename: `coord.rs`); re-export from playa-engine. Smoke build.
2. Add `Fps`, `Round`, unit-tested `frames_to_seconds` / inverse, `frames_at_speed`. No call-site change yet.
3. Pick ONE rounding rule for `src_len/speed` (recommend round-to-nearest, justify in docstring). Replace 5 sites in attrs.rs/comp_node.rs/timeline_ui.rs in one commit, with test asserting equivalence of all old-getter outputs to the new helper for speed ∈ {0.5, 1.0, 1.5, 2.0} × src_len ∈ {1, 24, 100, 999, 1000}.
4. Unify speed validation. Single `Speed::new` consuming the existing range debate.
5. Optional follow-up: rational fps. Big migration; gate behind an opt-in.
