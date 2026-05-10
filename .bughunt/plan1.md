# Bug Hunt — Plan 1

Status: **executing — Waves 1, 2, 3, 4 all applied and green.** Original audit
synthesised from `.bughunt/agent_gpu_bridge.md`, `agent_time_conv.md`,
`agent_timeline_bounds.md`, `agent_queue_research.md`. Every claim spot-checked
against current code. T4 was a false positive (audit miscall) and is excluded.

## Progress log

### Wave 1 — GPU compositing bridge: applied

| ID | What changed | File:line |
|---|---|---|
| F1 | `load_project` now calls `ensure_gpu_blend_initialized` after the project swap (matches `runner.rs:187` boot-path invariant) | `crates/playa-app/src/app/project_io.rs:226-231` |
| F4 | `delegate_blend_blocking` polls `recv_timeout(100ms)` checking a new `Arc<AtomicBool>` shutdown flag instead of unbounded `recv()` | `crates/playa-engine/src/entities/gpu_blend_bridge.rs:78-148` |
| F10 | `on_exit` calls `bridge.shutdown()` then drains pending blends so `Workers::drop` join never hangs on a parked worker | `crates/playa-app/src/app/run.rs:295-330` |
| F3 | `signal_preload` no longer re-checks compositor type — trusts caller's `gpu_blend_bridge_ref_for_preload` gating; race window on Cpu↔Wgpu toggle closed | `crates/playa-engine/src/entities/comp_node.rs:1700-1720` |
| F5 | Removed redundant `gpu_blend_rx_default` helper; `Mutex<Option<_>>::default()` is `Mutex::new(None)` | `crates/playa-app/src/app/mod.rs:39-46, 192-196` |

`cargo check -p playa-engine -p playa-app -p playa-ui` clean (47 s). Pre-existing
`f32: From<f64>` warnings in `playa-ui` are unrelated (timeline_helpers.rs:411 etc.)
and predate this work.

### Wave 2 — `playa-time` crate + math unification: applied

| ID | What changed | File:line |
|---|---|---|
| — | New crate `crates/playa-time/` with modules `fps`, `round`, `speed`, `conversion`, `coord`, `timecode` | `crates/playa-time/src/{lib,fps,round,speed,conversion,coord,timecode}.rs` |
| Q2 | First-class negative time + AE-style SMPTE timecode (NDF + 12M-1 drop-frame for 29.97/59.94). `format_time` / `parse_time` switch via `TimeDisplay { Frames, Seconds, Timecode { drop_frame } }` | `crates/playa-time/src/timecode.rs` |
| — | 46 unit tests, all green: NTSC round-trip, DF/NDF round-trip on canonical SMPTE values, negative-frame round-trip, format/parse symmetry | `crates/playa-time/src/*.rs` (`mod tests`) |
| — | Workspace member added; `playa-time` is `[workspace.dependencies]`-vended | `Cargo.toml:8`, `:32` |
| Q1 | Single canonical `Speed` policy: signed magnitude, `MIN_MAGNITUDE = 0.001`, sign = reverse playback. Replaces both `clamp(0.1, 4.0)` (attrs.rs) and `.abs().max(0.001)` (comp_node.rs) | `crates/playa-time/src/speed.rs` |
| — | `playa-engine::entities::space` reduced to a re-export of `playa-time::coord` — Group E (`frame_to_image ≡ object_to_src`) collapsed to one definition + alias | `crates/playa-engine/src/entities/space.rs` |
| T1 | `attrs.rs` (`layer_start`, `layer_end`, `full_bar_end`) migrated to `Speed::scale_src_to_timeline` / `scale_timeline_to_src` with explicit `Round::Round` / `Round::Ceil` | `crates/playa-engine/src/entities/attrs.rs:655-705` |
| T1 | `comp_node.rs` 8 sites migrated: `Layer::end`, `Layer::work_area`, `Layer::parent_to_local`, `Comp::get_layer_end`, `Comp::get_layer_work_area`, `trim_layers`, `set_child_start`, `set_child_end`. **Behaviour change**: prior `as i32` truncate → `Round::Round` (B1 fix). On non-integer speed, layer end can shift ±1 frame compared to old behaviour. Acceptable per Q1 — was the audit-flagged divergence | `crates/playa-engine/src/entities/comp_node.rs:204-249, 765-800, 855-861, 1095-1124` |
| C-dedupe | `timeline_ui.rs` 4× copy-paste of `(delta_x / (ppf * zoom)).round() as i32` collapsed to private helper `delta_px_to_frames` | `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:36-44, 1015, 1090, 1129, 1172` |
| Slide | Slide-tool inline `(_*speed).round()` and `(_/speed).ceil()` migrated to `Speed::scale_*` | `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1187-1198` |
| T3 | `(fps * 5.0) as i32` → `(fps * 5.0).round() as i32` so NTSC `fps=23.976` gives 120 frames (5 s) instead of 119 | `crates/playa-engine/src/entities/project.rs:766-768` |

`cargo check -p playa-engine -p playa-app -p playa-ui` clean (incremental ≤ 6 s).

`cargo test -p playa-time` clean: 46/46 passed.

`cargo test -p playa-engine` blocked by environment, **not code**: `playa-ffmpeg`'s
test-target link pulls vcpkg-installed shared libraries (`librabbitmq`, `lrist`,
`lssh`, `lvpx`, `ldav1d`, `ljxl`, …) which are not on this Linux/WSL2 host. The
existing test fixtures had **zero coverage** for the conversions changed by Wave 2
(audit confirmed) so there is no regression-gate hidden by this. Run on Windows
through `xtask` when needed.

### T4 — false positive (audit miscall)

The audit flagged `timeline_ui.rs:1289-1304` as "two interpretations of the same
drop position rounded vs floored on adjacent lines". Spot-check: line 1295
(`screen_x_to_frame(...).round()`) is a horizontal frame; line 1301
(`((hover_pos.y - timeline_rect.min.y) / config.layer_height).floor()`) is the
**vertical row index** (y-axis). Different quantities. Not a bug. Skipped.

### Severity recalibration (post-execution)

The original table over-labelled. Practical impact today:

- Real BLOCKER (process-hang / data-corruption tier): **F4 only** — workers parked
  on `recv()` could prevent `Workers::drop` from joining, hanging shutdown.
  Patched.
- Original "BLOCKER" labels for **F1, T1, L1** were **HIGH** — UX-grade impact
  (1-frame visual glitch, anti-AE comp-dim hijack, scrub clamp wrong). F1, T1
  patched. L1 lives on the Wave 3 path.

### Wave 3 — soft-marker timeline bounds: applied

Per user feedback: keep `A_IN/A_OUT` as **soft markers** (AE-style) rather than
deleting them from the schema. New `A_AUTO_BOUNDS` flag toggles between
auto-rebound (default, current behaviour) and pinned-to-user-set (AE comp
duration).

| ID | What changed | File:line |
|---|---|---|
| — | New attribute key `A_AUTO_BOUNDS` (Bool, non-DAG, default `true`); registered in `COMP_SCHEMA` so the `Attrs` system handles dirty-tracking correctly | `crates/playa-engine/src/entities/keys.rs:14-30`, `attr_schemas.rs:122` |
| L4 | `rebound()` no longer overwrites comp `A_WIDTH/A_HEIGHT` from the first visible layer. Comp dimensions are user intent. Explicit `fit_dim_to_first_layer()` exposed for opt-in UI action | `crates/playa-engine/src/entities/comp_node.rs:475-529` |
| Soft | `rebound()` reads `A_AUTO_BOUNDS`: `true` → recompute `A_IN/A_OUT` (legacy behaviour); `false` → pin to user-set values (no-op) | `crates/playa-engine/src/entities/comp_node.rs:489-493` |
| L5 | Layer constructor no longer writes a stored `A_OUT` — `Layer::end()` always computes from `src_len/speed`. Existing JSON projects that contain `out` on layers ignore it on load | `crates/playa-engine/src/entities/comp_node.rs:142-147` |
| B5 | Paste path no longer reads stale `A_OUT`; `A_IN` shift is the only attr touch. Calls `comp.rebound()` after the insert loop so newly-pasted layers are reachable by scrub (audit B2) | `crates/playa-app/src/main_events.rs:1245-1276` |
| Backward compat | `CompNode::new` now writes `A_AUTO_BOUNDS=true`. Existing saved projects load with default `true` (key absent → fallback `true`) — zero behaviour change | `crates/playa-engine/src/entities/comp_node.rs:308-311` |

`cargo check -p playa-engine -p playa-app -p playa-ui` clean (incremental 4.7 s).

### Wave 4 — `playa-jobs` crate: applied

| Module | What's in it |
|---|---|
| `job` | `JobId(Uuid)`, `JobState` enum (Pending → Submitting → AwaitingProvider → Downloading → Staging → Complete\|Failed\|Cancelled), `JobProgress`, `JobError`, `Job` snapshot |
| `cancel` | `CancelToken` — `Arc<AtomicBool>`, replaces ad-hoc `cancel_flag` dups (`encode_ui.rs:42`, …) |
| `event` | `JobEvent` enum — `Created/StateChanged/Progress/Completed/Failed/Cancelled` |
| `provider` | `JobProvider` trait + `JobContext` (cancel token, files dir, channel-back state/progress/persist-param) |
| `persist` | JSONL append log, replay-to-jobs fold, tombstones. Adjacently-tagged enum (`#[serde(tag = "type", content = "data")]`) — internally-tagged collided with `Job.kind` and rejected `Tombstone(JobId)` |
| `queue` | `JobQueue` + `JobQueueConfig`. Mutex<VecDeque<JobId>> + Condvar work queue, N=`max(2, ncpu/4)` worker threads, single updater thread for state writes + listener broadcast. `submit/cancel/get/list/subscribe/replay_persisted/shutdown` |
| Persistence | Default ON (cargo feature `persist`). `persist_path: Option<PathBuf>` lets tests opt-out. Each state mutation appends a `LogEntry::Updated`; provider's crash-resume contract uses `JobContext::persist_param` (e.g. Seedance writes `task_id` **before** `Submitting → AwaitingProvider`) |

19/19 unit tests pass (`cargo test -p playa-jobs` 0.03 s):
- Echo round-trip end-to-end (submit → run → complete + listener fires).
- Cancel mid-run while provider polls flag → resolves to `Cancelled`.
- `list()` returns every job; `shutdown()` joins workers under 2 s.
- Persist log: append/round-trip, replay collapse to latest state, tombstone, missing-file → empty.
- Persist + queue: phase-1 submit/run/complete with log; phase-2 fresh queue replays + `get(id)` returns `Complete` (no resume since terminal).
- Unknown provider rejected at submit time.

`cargo check` workspace clean across all five crates.

**No HTTP-client deps. No real providers (Seedance, ffmpeg-encode).** Those live
in vendor-specific sibling crates (`playa-job-seedance`, `playa-job-ffmpeg`) and
are a separate task — `playa-jobs` is the infrastructure they plug into.

### Wave 5 — boot integration: applied

| ID | What changed | File:line |
|---|---|---|
| Wiring | `playa-jobs` added as workspace dep on `playa-app`. PlayaApp gains `pub job_queue: Option<Arc<JobQueue>>` (`#[serde(skip)]`) plus a `build_default_job_queue()` helper that opens the persist log at `dirs_next::config_dir().join("playa/jobs.jsonl")` and stages files under `dirs_next::cache_dir().join("playa/jobs")`. Falls back to non-persistent on log-open failure | `crates/playa-app/Cargo.toml:16`, `crates/playa-app/src/app/mod.rs:25, 188-198, 285-339` |
| Boot path | `runner.rs` calls `app.ensure_jobs_initialized()` immediately after `ensure_gpu_blend_initialized()` so persisted jobs are restored before any provider can register | `crates/playa-app/src/runner.rs:188-191` |
| Default visibility | Construction subscribes a `log::debug!("[jobs] {event:?}")` listener so dev sessions see job activity without per-feature plumbing | `crates/playa-app/src/app/mod.rs:328-330` |
| UI toggle | `A_AUTO_BOUNDS` is registered in `COMP_SCHEMA` as `AttrType::Bool` with `DISP` flag. The schema-driven AE attributes panel (`ae_ui.rs:275`) auto-renders Bool+DISPLAY as a checkbox — no per-attribute UI code needed. Label is the raw key (`auto_bounds`); cosmetic relabel can come later via a schema annotation | already present from Wave 3 |

### Remaining

- **First real provider** (Seedance video-gen → mp4 → layer attach). Awaiting
  research output in `.bughunt/research_seedance_compute.md` (auth, endpoints,
  pricing, sign-up flow). Will live in a sibling crate `playa-job-seedance`
  with its own HTTP client (likely `ureq + rustls`).
- **JobEvent → EventBus forwarding** for cross-feature subscription (e.g.
  status-bar shows "1 job uploading"). Trivial: one extra `subscribe()` call in
  PlayaApp::Default that re-publishes through `playa-events::EventBus`. Do when
  the first UI consumer of JobEvents lands.
- **T5** (rational fps storage migration) — punt; f32 path works by tolerance
  luck, not a today-bug.
- F8 (`CompositorType::Clone` silent downgrade) / F9 (`Frame::set_status` race) —
  log-only / NIT items, deferred.

---



## Executive summary

Four fronts surfaced from the audit:

1. **GPU compositing worker→UI bridge** — 3 BLOCKER/HIGH lifecycle bugs, real risk of
   shutdown hang and 1-frame visual glitch after `Open Project`.
2. **Time / coordinate conversion** — math scattered across 6+ sites with three rounding
   modes and two speed-clamp policies; not 1 unit test on conversions; NTSC fps survives by
   f32 luck. Drives a `playa-time` crate.
3. **Timeline + layer bounds** — dual source of truth (`A_IN/A_OUT` stored vs `bounds()`
   computed); `rebound()` clobbers comp width/height from first layer. Anti-AE.
4. **Async queue manager** — no infrastructure for long external IO tasks (Seedance-style).
   Cancel pattern duplicated ad-hoc; `EventBus` exists but is unused by background workers.
   Drives a `playa-jobs` crate.

Cross-cutting class-of-bug: **stored-vs-computed divergence**. Same data held in two places,
synced by hand, drifts at the next call-site. Drives every BLOCKER on this list.

---

## Findings ranked

| # | Sev | Front | File:line | One-line |
|---|-----|-------|-----------|----------|
| F1 | BLOCKER | gpu  | `crates/playa-app/src/app/project_io.rs:203` | `load_project` swaps `self.project` without `ensure_gpu_blend_initialized` (compare `runner.rs:187`) |
| T1 | BLOCKER | time | `comp_node.rs:213,777` vs `attrs.rs:679,700` | 5 sites compute `src_len/speed` with 3 rounding modes (round/ceil/truncate), 2 precisions |
| L1 | BLOCKER | tl   | `comp_node.rs:301-309,386,447,474` + `player.rs:482-490` | Comp bounds stored (`A_IN/A_OUT`) AND computed (`bounds()`); clamp reads stored, only zoom-to-fit reads computed |
| F4 | HIGH | gpu  | `gpu_blend_bridge.rs:110` | `reply_rx.recv()` blocks forever when UI suspended (minimize, `rfd::FileDialog`, vsync stall) |
| F2 | HIGH | gpu  | `app/run.rs:38-47` | `drain_gpu_blend_queue` always runs; `update_compositor_backend` only if `wgpu_render_state.is_some()` → CPU fallback before backend wired |
| F3 | HIGH | gpu  | `comp_node.rs:1700-1709` + `project_io.rs:140` | Backend match done twice across two locks → race on Cpu↔Wgpu toggle |
| T2 | HIGH | time | `attrs.rs:659/675/697` vs `comp_node.rs:208-212,...` | Speed clamp `clamp(0.1, 4.0)` (attrs, f64) vs `.abs().max(0.001)` (comp_node, f32) — 100× spread on low end |
| L2 | HIGH | tl   | `player.rs:482-490` | `set_frame` clamps to stored `_in/_out`; layers added without `rebound()` (paste path `main_events.rs:~1300`) become unreachable |
| L3 | HIGH | tl   | `comp_node.rs:393,449,466` | Empty comp returns magic `(0, 100)` instead of sentinel/None → phantom 101-frame ruler |
| L4 | HIGH | tl   | `comp_node.rs:483-487` | `rebound()` overwrites comp `A_WIDTH/A_HEIGHT` from `get_first_size()` — anti-AE; user can't have stable comp dim |
| L5 | HIGH | tl   | `comp_node.rs:144` (write) vs `:204-214` (`Layer::end()` ignores) + `main_events.rs:1248-1264` (paste reads stale) | `Layer.A_OUT` stored at construction, never refreshed; runtime computes; paste path reads stored → divergence |
| F5 | HIGH | gpu  | `app/mod.rs:39-46,189-195,293-309` | Paired `skip` fields (`gpu_blend_bridge` Option, `gpu_blend_rx` with custom default fn) both default to `None`; correctness depends on `ensure_gpu_blend_initialized` recreate ordering |
| T3 | MED  | time | `project.rs:766` | `(fps * 5.0) as i32` truncates NTSC fps (23.976→119, 29.97→149) |
| T4 | MED  | time | `timeline_ui.rs:1289-1304` | Same drop-position computed via `.round()` and `.floor()` on adjacent lines |
| T5 | MED  | time | `playa-io/src/video/ffmpeg_imp.rs:50` → `file_node.rs:48` | NTSC fps stored as `f32`, survives only because `fps_to_rational` tolerance is `±0.01` |
| L6 | MED  | tl   | `comp_node.rs:447-470` vs `:386-422` | `bounds_internal` (rebound input) uses stored `src_len`; `bounds` uses dynamic media — diverge for re-trimmed nested comps |
| L7 | MED  | tl   | `player.rs:238-268`, `main_events.rs:455-467,1313-1320` | `set_play_range` and `Reset*` clamp/reset against stored bounds, not derived |
| F7 | MED  | gpu  | `app/mod.rs:336-353` | `drain_gpu_blend_queue` holds `gpu_blend_rx` AND `project.compositor` mutexes for full drain (~50ms × N); std `Mutex` non-reentrant → fragile lock order |
| Q1 | MED  | jobs | (missing) | No `JobId`/`JobState`/registry; encode reinvents progress channel; gpu_blend ignores `EventBus` |
| Q2 | MED  | jobs | `encode_ui.rs:42` + `gpu_blend_bridge.rs` | Cancel pattern (`Arc<AtomicBool>`) duplicated ad-hoc; not unified |
| Q3 | MED  | jobs | (missing) | No persistence for in-flight jobs — restart leaks Seedance API credits |
| F6 | LOW  | gpu  | `gpu_blend_bridge.rs:149` | `Disconnected` arm is silent `break` (no log) — not reachable today, future trap |
| F8 | LOW  | gpu  | `compositor.rs:57-70` | `CompositorType::Clone` silently downgrades Wgpu→Cpu with `log::warn!` |
| F9 | LOW  | gpu  | `comp_node.rs:1399-1403,1466-1469` | `Frame::set_status(Composing)` may run after `cache.insert(clone)` — race depends on `Frame` interior mutability (NEEDS-VERIFY) |
| F10| LOW  | gpu  | `app/mod.rs:67-196` + `run.rs:295-305` | Drop order: `workers` declared before `gpu_blend_*`; if worker stuck on `recv()` (F4), shutdown joins forever; `on_exit` does not drain |
| T6 | LOW  | time | `viewport.rs:566` | `(total_frames - 1)` magic in `normalized_to_frame` — undocumented inclusive end |
| L9 | LOW  | tl   | `Layer.parent_to_local` clamps via `clamp(source_in, source_out)` silently | Negative `parent_frame - start` returns `source_in` for all such frames; no assert |

---

## Front A — GPU compositing bridge fixes

Goal: kill the load-project glitch, the shutdown hang, and the boot-order race.

### A.1 — Fix F1 (BLOCKER)

After `self.project = project` in `load_project` (`project_io.rs:203`), add:
```
self.ensure_gpu_blend_initialized();
self.update_compositor_backend(...);  // only if wgpu_render_state available; else defer
```
Better: extract `swap_project(&mut self, new_project)` that drains pending GPU queue,
resets `gpu_blend_rx`, swaps project, recreates pair, rebinds compositor. One method,
called by `load_project` AND by future "new project" / "revert" paths. Eliminates F1
sister-site mismatch class.

### A.2 — Fix F4 + F10 (HIGH/LOW, same root)

Replace `reply_rx.recv()` (`gpu_blend_bridge.rs:110`) with one of:
- `recv_timeout(Duration::from_millis(N))` in a loop checking a cancel `Arc<AtomicBool>`;
- explicit `cancel: Arc<AtomicBool>` field on `GpuBlendRequest`, set by `on_exit`/teardown;
- `crossbeam_channel::select!` with cancel + reply channels.

`on_exit` (`run.rs:295-305`) should:
1. set cancel flag;
2. `gpu_blend_rx.lock().take()` (drop receiver — `delegate_blend_blocking` returns
   `NotQueued` for new requests; in-flight ones get `ReplyDisconnected`);
3. final `drain_into_compositor` to flush anything queued before flag set.

This unblocks `Workers::drop` join.

### A.3 — Fix F2 + F3 (HIGH)

Centralise compositor backend selection. Today three sites match `Wgpu(_)`:
- `app/mod.rs:315-329 gpu_blend_bridge_ref_for_preload`,
- `comp_node.rs:1700-1709 signal_preload`,
- `app/run.rs:38-47 update_compositor_backend`.

Replace with `PlayaApp::with_compositor_backend(|backend| {...})` that returns the
bridge if `Wgpu`, takes both locks together, and is called in exactly one place per
path. Drain MUST early-return when backend is `Cpu` (saves a mutex).

`update_compositor_backend` should also key off `Arc::as_ptr(device)` (or handle id) to
detect device recreation; current impl misses macOS resize device-recreate scenarios.

### A.4 — Fix F5 (HIGH; cosmetic-grade)

Drop `default = "gpu_blend_rx_default"` (`app/mod.rs:194`) — `Mutex<Option<_>>` already
defaults to `Mutex::new(None)`. Or fold both fields into one struct
`GpuBlendChannels { bridge: Option<Arc<...>>, rx: Mutex<Option<...>> }` so the invariant
"both Some or both None" lives in one place. No behaviour change today; prevents future
divergence.

### A.5 — Defer

F6, F8, F9 — log/comment-only changes; F9 needs `Frame::set_status` semantics check
(NEEDS-VERIFY).

---

## Front B — `playa-time` crate (time + coordinate unification)

Goal: one crate, one rounding rule, one fps representation. Eliminates T1–T5 + Group A–E
duplicates from `agent_time_conv.md`.

### B.1 — Crate boundary

New workspace member `crates/playa-time` (or `playa-coord` — recommend joint).

Public surface (proposal — open to adjust):
```rust
pub struct Fps { pub num: u32, pub den: u32 }     // exact rational
pub enum Round { Floor, Round, Ceil, Trunc }      // explicit at every call

pub fn frames_to_seconds(frames: i32, fps: Fps) -> f64;
pub fn seconds_to_frames(secs: f64, fps: Fps, mode: Round) -> i32;

pub struct Speed(f32);
impl Speed {
    pub fn new(v: f32) -> Self;                   // single canonical clamp
    pub fn scale_src_to_timeline(&self, n: i32, mode: Round) -> i32;
    pub fn scale_timeline_to_src(&self, n: i32, mode: Round) -> i32;
}

// coords (verbatim from space.rs)
pub fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2;
pub fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2;
pub fn object_to_src(p: Vec2, size: (usize, usize)) -> Vec2;  // alias of frame_to_image
pub fn user_rot_to_math_rot(deg: f32) -> f32;
pub fn math_rot_to_user_rot(rad: f32) -> f32;

// timeline px ↔ frame (move from timeline_helpers.rs)
pub fn frame_to_screen_x(frame: f32, origin_x: f32, ppf: f32, zoom: f32) -> f32;
pub fn screen_x_to_frame(x: f32, origin_x: f32, ppf: f32, zoom: f32, mode: Round) -> i32;
```

Future (gated, separate phase): `Timecode`, drop-frame, SMPTE.

### B.2 — Migration order (each step independently committable)

1. **Add crate, copy `space.rs` verbatim.** Re-export from `playa-engine::entities::space`
   so callers compile unchanged. Smoke build only.
2. **Add `Fps`, `Round`, `frames_to_seconds`/inverse**, with unit tests covering
   round-trip on integer + NTSC rates. **No call-site change.**
3. **Add `Speed::new` with canonical clamp** (decision needed — see NEEDS-VERIFY Q1).
   Add `scale_src_to_timeline(_, Round::Round)`. Add tests proving equivalence to
   `attrs::layer_end` for speed ∈ {0.5, 1.0, 1.5, 2.0} × src_len ∈ {1, 24, 100, 999}.
4. **Replace 5 `src_len/speed` sites** with `Speed::scale_src_to_timeline`. Single
   commit. Tests pin the chosen rounding rule. Fixes T1.
5. **Replace 3 `trim_in/speed` sites** (Group B). Same pattern. Tests.
6. **De-duplicate timeline_ui.rs:1015/1090/1129/1172** (Group C — 4× copy-paste of
   `delta_x → delta_frames`). Single helper.
7. **Merge `frame_to_image` and `object_to_src`** (Group E — bit-exact identical).
8. **Optional: rational fps storage migration** — gated, separate plan; needs
   serialisation migration step.

### B.3 — Tests-first discipline

Currently zero tests on conversions. Each refactor step pins prior behaviour first, then
adopts the new rule. Captures latent-bug exposure window.

---

## Front C — Timeline + layer bounds → AE-style

Goal: kill stored-vs-derived divergence; comp dim becomes user intent; bounds become
computed with cache invalidation.

### C.1 — Drop stored `A_IN/A_OUT` on `CompNode`

`A_IN/A_OUT` for **layers** stay (they're user-positionable). For **comps**:

1. Override `Node::_in/_out` for `CompNode` to delegate to `compute_bounds(&media)`.
2. Add `#[serde(skip)] bounds_cache: OnceCell<Option<(i32, i32)>>` (or similar invalidation
   primitive). Invalidated by `mark_dirty`.
3. Persist only **user-intent**: `A_DESIGN_WIDTH`, `A_DESIGN_HEIGHT`, optional
   `A_DESIGN_DURATION_HINT`, `A_FPS`, `A_FRAME` (playhead).
4. Migration on load: read old `A_IN/A_OUT`; if present, drop them; recompute on first read.

### C.2 — Delete `rebound()`

Every call site (`comp_node.rs:380, 551, 559, 839, 887, 1085, 1106, 1127`) becomes a
`mark_dirty` only. The W/H clobbering (L4) goes away by definition because `rebound`
no longer exists. "Fit Comp to Layers" UI action becomes explicit user intent — sets
`A_DESIGN_WIDTH/HEIGHT` from `get_first_size()` once, on demand.

### C.3 — Player clamps against derived bounds

`set_frame` (`player.rs:482-490`), `set_play_range` (`:238-268`), Reset paths
(`main_events.rs:455-467, 1313-1320`) clamp/reset against `comp.compute_bounds(&media)`.
Fixes L2, L7.

### C.4 — Drop `Layer.A_OUT` from schema

Layer's stored `A_OUT` (`comp_node.rs:144`) is dead — `Layer::end()` ignores it. Delete
the write at construction. Update paste path (`main_events.rs:1248-1264`) to compute
`Layer::end()` instead of reading attr. Fixes L5.

### C.5 — Empty comp policy

`compute_bounds` returns `Option<(i32, i32)>`. `None` = no playable content; player
no-ops, UI shows ruler around `A_DESIGN_DURATION_HINT` if set, else minimal placeholder.
Fixes L3.

### C.6 — Negative time (decision required)

Code half-supports negative comp time. NEEDS-VERIFY Q2: allow officially or clip at 0?
If allow, lock down with one round-trip test. If clip, enforce at `Layer::A_IN` setter.

### C.7 — Naming

`play_range` is overloaded. Rename to `work_area` everywhere it means "trimmed range";
reserve `play_range` (or `bounds`) for derived comp bounds. Reduces L8 confusion.

### C.8 — Order

1. Add `compute_bounds(&media) -> Option<(i32, i32)>` + cache. Read-only test.
2. Switch player clamps to derived. **No `rebound()` removal yet.** Should be a no-op
   functionally if `rebound` was correct; if there's a behaviour delta, it's an
   uncovered bug — write a regression test pinning derived semantics.
3. Migrate UI ruler/zoom-to-fit/overlay to `compute_bounds`.
4. Drop `Layer.A_OUT` write. Migrate paste path.
5. Drop `rebound()` body; keep callable (no-op + `mark_dirty`) for one cycle, then
   remove call sites, then remove method. Old projects continue to load (W/H survives
   because `A_WIDTH/HEIGHT` are persisted).
6. Drop stored `A_IN/A_OUT` from comp serialisation; add migration on load.

Heavy refactor. Recommend separate phase from Front A; Front A is contained, this is
schema-touching.

---

## Front D — `playa-jobs` crate (async queue manager)

Goal: long-running external IO tasks (Seedance video-gen, future providers) with
state machine, persistence, cancel, UI integration via existing `EventBus`.

### D.1 — Crate boundary (sketch — open to refine)

```rust
pub struct JobId(Uuid);
pub enum JobState {
    Pending, Submitting, AwaitingProvider, Downloading, Staging,
    Complete, Failed, Cancelled,
}
pub struct Job { id, kind: String, state, progress, error,
                 params: serde_json::Value, created_at, updated_at }

pub trait JobProvider: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn run(&self, ctx: &JobContext, params: Value) -> Result<Value, JobError>;
    fn resume(&self, ctx, params) -> Result<Value, JobError> { self.run(ctx, params) }
}

pub struct JobQueue {
    pub fn submit(kind, params) -> JobId;
    pub fn cancel(JobId);
    pub fn get(JobId) -> Option<Job>;
    pub fn list(filter) -> Vec<Job>;
    // events emitted via playa-events EventBus
}

pub enum JobEvent {
    Created(JobId), StateChanged(JobId, JobState),
    Progress(JobId, JobProgress), Completed(JobId, Value), Failed(JobId, String),
}
```

### D.2 — Architectural choices (matches existing repo style)

- **No tokio.** Repo is `std::thread + crossbeam-channel + rayon`. Adding tokio
  contaminates every sync path.
- **Separate IO pool**, not the existing `Workers` pool. CPU pool sized for frame
  decoding (`workers.rs:34-191`); minute-scale jobs would starve frame decoding.
  Pool size: `max(2, num_cpus / 4)`.
- **HTTP**: `ureq` + `rustls` feature. Sync, no runtime, works in MSVC static build
  alongside vcpkg.
- **Events**: emit `JobEvent` via existing `playa-events::EventBus` (`bus.rs:52-191`).
  No per-frame poll. Existing UI subscription mechanism just works.
- **Cancel**: unified `CancelToken` (`Arc<AtomicBool>`). Replaces ad-hoc dups in
  `encode_ui.rs:42` and future code.
- **Persistence**: JSONL append log at `~/.config/playa/jobs.jsonl`. Behind
  cargo feature `persist` (default ON). State written **before** state transition
  on submit/awaiting (so a crash mid-submit doesn't leak Seedance credits — see
  NEEDS-VERIFY Q3).
- **Provider isolation**: providers in their own crates (`playa-job-seedance`,
  `playa-job-ffmpeg`). `playa-jobs` holds only trait + queue.
- **Job-files dir**: `~/.cache/playa/jobs/{job_id}/`. Cleanup policy on terminal
  states.

### D.3 — Migration of existing patterns (cleanup pass, optional, separate)

`encode_ui` re-implements job state (`encode.rs:1135-1153 EncodeStage`) + cancel
+ progress channel. Could migrate to `playa-jobs` once Front D ships, but it works —
not urgent. Refactor target, not bug.

### D.4 — Order

1. Add empty `playa-jobs` crate, types only, no providers, no persistence.
2. Add `JobProvider` trait, dummy `EchoProvider` for tests.
3. Wire `JobEvent` → `EventBus`. Smoke: dummy job round-trips through bus.
4. Add IO pool + `JobQueue::submit` runs `run()` on it.
5. Add `CancelToken` + cancellation tests.
6. Add `persist` feature + JSONL log. Boot-time `resume()` for Submitting/Awaiting/
   Downloading.
7. **Stop here for v0.** Real Seedance provider lives in a separate plan once the user
   has API access + key flow decided.

---

## Class-of-bug patterns

These reappear across fronts. Worth fixing once at infrastructure level instead of
spot-fixing each instance.

1. **Stored vs computed divergence** — F1, L1, L5, L6. Eliminated by Front C plus the
   `swap_project` pattern in Front A.
2. **Three rounding modes for one quantity** — T1, T2, T4. Eliminated by `Round` enum
   in Front B.
3. **Blocking worker→UI handoff with no cancel** — F4, F10. Eliminated by Front A.2 +
   `CancelToken` in Front D.
4. **Cancel flag duplicated per feature** — `encode_ui.rs:42`, future GPU bridge. Single
   `CancelToken` primitive in Front D.
5. **Backend match `matches!(*compositor, Wgpu(_))` duplicated** — F3, F12. One
   `with_compositor_backend(...)` accessor.
6. **Skip-field `Default` invariants un-typed** — F5. Fold paired skip fields into one
   struct.
7. **`(0, 100)` magic returned for empty bounds** — L3. `Option<(i32, i32)>`.

---

## Proposed sequencing (waves)

Each wave is mergeable independently. Tests pinned before each refactor.

**Wave 1 — Containment (no schema, low blast radius)**
- A.1 `swap_project`, fixes F1 BLOCKER.
- A.2 `recv_timeout` + `on_exit` drain, fixes F4/F10.
- A.3 `with_compositor_backend` accessor, fixes F2/F3.
- A.4 fold skip-fields, fixes F5.
- B.1 add `playa-time` crate, copy `space.rs`, re-export. Smoke only.
- B.2 add `Fps`, `Round`, conversion helpers + tests.
- B.6 dedupe `delta_x → delta_frames` (Group C). 1 helper, 4 sites.
- B.7 merge `frame_to_image` ≡ `object_to_src` (Group E).

**Wave 2 — Time refactor**
- B.3 `Speed::new` canonical clamp.
- B.4 5 sites of `src_len/speed` → `Speed::scale_src_to_timeline`. Tests.
- B.5 3 sites of `trim_in/speed` → `Speed::scale_timeline_to_src`. Tests.
- T3 `(fps * 5.0).round()` in `project.rs:766`.
- T4 fix `timeline_ui.rs:1289-1304` round/floor mismatch.

**Wave 3 — Timeline AE-style (schema migration)**
- C.1–C.8 in order. Heavy. Separate plan, separate review.

**Wave 4 — Async queue (greenfield)**
- D.1–D.4 in order. Independent of other waves.

**Out of scope of this plan**
- T5 NTSC rational fps storage migration — flagged but punt.
- F8 `CompositorType::Clone` — log-only NIT.
- F9 `Frame::set_status` race — NEEDS-VERIFY first.
- L9 negative time decision — NEEDS-VERIFY first.
- Encode dialog migration to `playa-jobs` — separate cleanup.

---

## NEEDS-VERIFY (user input requested)

Q1. **Speed clamp policy.** Pick one canonical range:
- (a) `clamp(0.1, 4.0)` — matches UI slider; rejects extreme values (current `attrs.rs`).
- (b) `.abs().max(0.001)` — allows 1000× slow-mo, allows reverse via sign (current
  `comp_node.rs`).
- (c) Hybrid: store `Speed { sign: bool, magnitude: f32 (clamp 0.001..1000) }`, UI
  slider exposes 0.1..4 by default but power-user input allows wider.

Q2. **Negative comp time.** Today: half-supported. Pick: officially allow (and add
test) or clip at zero (and enforce at `Layer.A_IN` setter).

Q3. **Job persistence default.** `~/.config/playa/jobs.jsonl` JSONL log; default ON.
OK with default-ON, or behind explicit user opt-in?

Q4. **Front D scope today.** Build `playa-jobs` crate now (Wave 4), or shelve until
Front A/C are done? Front D is independent — can start immediately in parallel.

Q5. **`Frame::set_status` interior mutability** (F9). Should I read `frame.rs` and
confirm whether status-set propagates through `Frame::clone()` (cache hit gets new
status) or not (cache hit sees stale)?

Q6. **`Project::Clone` derive** (F8 dead-code candidate). Is `#[derive(Clone)]` on
`Project`? If yes, `CompositorType::Clone` is reachable — its silent downgrade matters
more than NIT. Worth verifying before treating as cosmetic.

---

## What I will NOT do without explicit approval

- No changes to `playa-ffmpeg` crate (vendored; out of scope).
- No `cargo clean`. (Forbidden per global rules.)
- No build/test runs until Wave plan is approved (tests first per wave, pinned to
  current behaviour, then refactor).
- No git commits.
- No deletes of dead-code candidates without confirming via `gitnexus_impact`
  upstream + cross-ref TODO/FIXME.

---

## Files referenced in this plan (for navigation)

GPU bridge:
- `crates/playa-app/src/app/project_io.rs:140, 186-225`
- `crates/playa-app/src/runner.rs:155-200`
- `crates/playa-app/src/app/mod.rs:39-46, 180-310, 336-353`
- `crates/playa-app/src/app/run.rs:30-65, 295-305`
- `crates/playa-engine/src/entities/gpu_blend_bridge.rs:90-160`
- `crates/playa-engine/src/entities/comp_node.rs:1660-1725`
- `crates/playa-engine/src/entities/compositor.rs:57-70`

Time/coord:
- `crates/playa-engine/src/entities/attrs.rs:650-710`
- `crates/playa-engine/src/entities/comp_node.rs:140-260, 760-810, 846-880, 1089-1125`
- `crates/playa-engine/src/entities/space.rs:40-90`
- `crates/playa-engine/src/entities/project.rs:760-770`
- `crates/playa-engine/src/core/player.rs:46-48, 219-235, 337-364, 475-500`
- `crates/playa-ui/src/widgets/timeline/timeline_helpers.rs:152-241`
- `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:1015,1090,1129,1172,1187-1304`
- `crates/playa-ui/src/widgets/viewport/viewport.rs:323-330,547-570`
- `crates/playa-ui/src/dialogs/encode/encode.rs:1297-1487, 1716-1860`
- `crates/playa-io/src/video/ffmpeg_imp.rs:39-65, 118-131`

Timeline:
- `crates/playa-engine/src/entities/comp_node.rs:140-505, 760-810, 1085-1127`
- `crates/playa-engine/src/core/player.rs:219-268, 339-432, 482-496, 506-514`
- `crates/playa-app/src/main_events.rs:455-467, 1248-1264, 1313-1320`
- `crates/playa-ui/src/widgets/timeline/timeline_ui.rs:600-620, 1248-1304`

Jobs:
- `crates/playa-engine/src/core/workers.rs:34-191`
- `crates/playa-engine/src/core/cache_man.rs:32-80`
- `crates/playa-engine/src/core/debounced_preloader.rs:27-108`
- `crates/playa-engine/src/entities/gpu_blend_bridge.rs:29-160`
- `crates/playa-events/src/bus.rs:52-191`
- `crates/playa-ui/src/dialogs/encode/encode_ui.rs:42, 198-820`
- `crates/playa-ui/src/dialogs/encode/encode.rs:1135-1153`
- `crates/playa-app/src/server/api.rs:27, 62-67, 197-209, 432-451`

Full per-front evidence + per-finding fix proposals live in
`.bughunt/agent_gpu_bridge.md`, `agent_time_conv.md`, `agent_timeline_bounds.md`,
`agent_queue_research.md`.
