# Investigation: timeline preload, cache filling, and camera vs plain 2D

Date: 2026-05-11 · Repo: `playa`

This document separates **verified facts in the codebase** from **hypotheses** about user-visible symptoms (“wrong segment cached”, “~20 frames vs ~220”, “adding a camera fixes the picture”).

---

## 1. Summary

| Topic | Verdict |
|--------|---------|
| Preload not running on scrub / frame changes | **Was a wiring bug**: `CurrentFrameChangedEvent` had no emitter. **Fixed:** `Project::modify_comp` now emits it when `frame` changes (see §5). |
| “Only current frame loads when scrubbing” | **Was expected** before the fix; after fix, `handle_events` should receive `CurrentFrameChangedEvent` for every playhead change that goes through `modify_comp` (Player + UI paths). |
| `LastOnly` cache strategy | Can erase prefetched neighbors whenever **any** frame is inserted — worsens “nothing stays cached” if preload ever runs. Default prefs use `All`. |
| Epoch bump on large seeks | Confirmed in `SetFrameEvent`; cancels queued worker jobs whose captured epoch is stale (by design). Less relevant until preload is actually enqueued. |
| Camera “fixes” 2D look | **No single proven bug** from this pass; code paths differ (`camera_path` + VP inverse vs `build_inverse_canvas_to_src_3x3`). Worth targeted repro + pixel comparison. `has_3d_content()` is unused — intended “2D unless camera” policy is **not** enforced here. |

---

## 2. Facts: preload pipeline

### 2.1 Where full-radius preload is supposed to run

- `PlayaApp::enqueue_frame_loads_around_playhead` → `CompNode::signal_preload` → `CompNode::preload` (spiral around `center = comp.frame()`, capped by `work_area()`).  
  Files: `crates/playa-app/src/app/project_io.rs`, `crates/playa-engine/src/entities/comp_node.rs`.

### 2.2 Handler that calls preload on “frame changed”

```43:49:C:/projects/projects.rust.cg/playa/crates/playa-app/src/app/events.rs
            if let Some(e) = downcast_event::<CurrentFrameChangedEvent>(&event) {
                trace!(
                    "Comp {} frame changed: {} → {}",
                    e.comp_uuid, e.old_frame, e.new_frame
                );
                self.enqueue_frame_loads_around_playhead(self.settings.playback.preload_radius);
                continue;
            }
```

### 2.3 Emitter for `CurrentFrameChangedEvent` (post-fix)

- `Project::modify_comp` (`crates/playa-engine/src/entities/project.rs`): after the closure, if `comp.frame()` changed vs before, emits `CurrentFrameChangedEvent { comp_uuid, old_frame, new_frame }` via `project.event_emitter` (same handle as `EventBus::emitter()`).

### 2.4 ~~Nothing emits `CurrentFrameChangedEvent`~~ (historical)

Previously workspace-wide search showed **no** `emit(...)` — that motivated this fix.

### 2.5 `SetFrameEvent` path (epoch + playhead)

```351:376:C:/projects/projects.rust.cg/playa/crates/playa-app/src/main_events.rs
    if let Some(e) = downcast_event::<SetFrameEvent>(event) {
        trace!("SetFrame: moving to frame {}", e.0);
        if let Some(comp_uuid) = player.active_comp() {
            let old_frame = project
                .with_comp(comp_uuid, |comp| comp.frame())
                .unwrap_or(e.0);
            let distance = (e.0 - old_frame).abs();

            if distance > 1 {
                if let Some(manager) = project.cache_manager() {
                    manager.increment_epoch();
                    ...
                }
            }

            project.modify_comp(comp_uuid, |comp| {
                comp.set_frame(e.0);
            });
            // Preload: `Project::modify_comp` emits `CurrentFrameChangedEvent` when frame changes.
        }
        return Some(result);
    }
```

Playhead-only updates do **not** mark the comp DAG-dirty, so **`AttrsChangedEvent` does not fire** for scrub — preload relies on **`CurrentFrameChangedEvent`** from `modify_comp`.

### 2.6 When preload runs

- Every **`modify_comp` that changes `comp.frame()`** → **`CurrentFrameChangedEvent`** → `enqueue_frame_loads_around_playhead(preload_radius)`. Covers **Player** (`step`, `set_frame`, playback advance), **UI** (`SetFrameEvent`), **bookmarks**, **jump-to-edge**, **project dive** `target_frame`, **CLI `--frame`**, etc., as long as the project has `event_emitter` set.
- After loading sequences: extra `enqueue_frame_loads_around_playhead` once in `load_sequences`. (`project_io.rs`)
- Attribute-driven invalidation: `handle_attrs_changed` → `enqueue_current_frame_only()` + debounced full radius. (`events.rs`, `run.rs`)

### 2.7 Stale narrative docs (still TODO)

- `diagram_flow.md`, `docs/AGENTS.md`: should explicitly say **`CurrentFrameChangedEvent` is emitted from `modify_comp`**, not from a separate mythical hook.

---

## 3. Hypotheses tied to user symptoms

### 3.1 “Slider moves over 200 frames but only ~20 ever cache”

**Primary hypothesis (strong, mostly addressed):** spiral preload was not requested on scrub because **`CurrentFrameChangedEvent` was never emitted**. With `modify_comp` emitting it on frame changes, scrub should enqueue preload unless **`event_emitter` is unset** (tests / misconfigured project).

**Secondary:** If user enables **`CacheStrategy::LastOnly`**, every `GlobalFrameCache::insert` clears **other** frames for that comp (`global_cache.rs` ~242–244). That aggressively keeps only the **last inserted** index — bad for “prefetch a window”. Default in prefs is `All`.

**Tertiary:** `preload_radius` persisted as a small positive value (slider 10–500) caps spiral radius; combined with no re-enqueue on scrub, behavior looks random.

### 3.2 “Wrong part of the clip on the timeline”

Not fully traced in this pass; code facts worth checking in a follow-up:

- Layer timeline → source frame: `Layer::parent_to_local` + clamp to source `_in.._out` in `compose_internal` (~1321–1326 in `comp_node.rs`).
- Comp `compute()` returns `None` if `frame_idx` outside **comp** `work_area()` (~1615–1618).

If UI playhead and comp `work_area()` disagree (trim / auto-bounds / `_in`/`_out`), users see “timeline says here but image is hold/black/wrong”.

---

## 4. Camera vs “plain 2D”

### 4.1 What the code does

- **Active camera:** topmost visible layer whose source is a `CameraNode`; VP matrix from `CameraNode::view_projection_matrix` with layer transform (`active_camera`, ~642–675 `comp_node.rs`).
- **Compose:** if camera present and layer not XY-tilted, compositor gets `camera_path` (VP inverse + layer inverse). Without camera, non-tilted layers use `transform::build_inverse_canvas_to_src_3x3` (~1407–1454).
- **CPU compositor:** matrix-aware path samples via `canvas_to_src_cpu` — camera branch vs 2D `inv_matrix` branch (`compositor.rs` ~576–613).

### 4.2 Intended product rule vs code

You described: *3D only if there is a camera layer, and only for layers below it.*

Facts:

- There is a helper `has_3d_content()` (`comp_node.rs` ~677+) but it is **never called** — no enforcement of “flat comp unless camera”.
- Layer ordering always uses **Z** (+ AE index tie-break) for painter’s algorithm (~1277–1297), independent of camera presence.

So “why does adding a camera fix the image?” is **not explained by a single obvious guard**; plausible causes:

1. **Different sampling math** (`camera_path` ray-plane vs 2D `inv_matrix`) hides an off-by-one or framing mismatch when footage resolution ≠ comp size.
2. **Ordering / depth** changes perceived stacking once VP is applied.
3. **Psychological / incidental:** camera layer forces a bounds / playback tweak elsewhere — needs repro project.

**Recommendation:** capture one comp where “no camera = wrong, add camera = good”, dump layer transforms, comp dim vs footage dim, and compare outputs of `build_inverse_canvas_to_src_3x3` vs camera ortho defaults.

---

## 5. Recommended fixes (engineering)

### Implemented (2026-05-11)

- `Project::modify_comp` now compares `comp.frame()` before/after the closure and emits **`CurrentFrameChangedEvent`** when it changed (same `EventEmitter` / queue as other app events).
- Emission order: **`AttrsChangedEvent` first**, then **`CurrentFrameChangedEvent`**, so `increment_epoch` / cache clear run before preload schedules jobs (avoids stale epoch on combined dirty + playhead updates).
- Comments updated in `main_events.rs`, `runner.rs` (removed redundant `enqueue_current_frame_only` after CLI `--frame`), `run.rs`, `player.rs`.

### Remaining cleanup

1. **Repair root docs** (`AGENTS.md`, `diagram_flow.md`) so the data-flow diagrams match `modify_comp` + `CurrentFrameChangedEvent`.

2. **Revisit `LastOnly` + preload**: document that “prefetch window” and LastOnly fight each other; consider not clearing on insert when prefetching, or special-case.

3. **Epoch + preload:** verify behavior when multiple large seeks land in one UI frame (epoch vs enqueue ordering).

4. **Camera / 2D parity**: add regression tests (fixed comp size, fixed bitmap pattern) comparing camera ortho vs no-camera path for identity transforms.

---

## 6. Files referenced by this investigation

| Area | Path |
|------|------|
| **`modify_comp` + frame notify** | `crates/playa-engine/src/entities/project.rs` |
| Scrub / epoch comment | `crates/playa-app/src/main_events.rs` |
| Preload handler | `crates/playa-app/src/app/events.rs` |
| Preload spiral | `crates/playa-engine/src/entities/comp_node.rs` (`preload`, `signal_preload`) |
| Preload API | `crates/playa-app/src/app/project_io.rs` |
| Debounced full preload | `crates/playa-app/src/app/run.rs`, `debounced_preloader.rs` |
| LastOnly clearing | `crates/playa-engine/src/core/global_cache.rs` |
| Camera vs 2D matrices | `crates/playa-engine/src/entities/comp_node.rs`, `transform.rs`, `compositor.rs` |

---

*End of FINDINGS.*
