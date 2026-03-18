# Playa Bug Hunt Report v2 (Expanded & Verified)

**Date:** 2026-03-18
**Version:** 0.1.142 | **Branch:** dev
**Scope:** Full codebase — 60+ source files, 14,000+ lines
**Method:** 9 parallel analysis agents (3 initial + 3 deep + 3 verification)

---

## Executive Summary

| Category | Count | Notes |
|----------|-------|-------|
| Verified Bugs | 12 | 1 denied after code review (NaN blend) |
| New Bugs (encode/server) | 10 | FPS truncation, path traversal, UI freeze |
| Performance Issues | 21+16 new | Buffer clones, O(n) LRU, per-pixel invariants |
| Code Deduplication | 14+7 new | RGBA strip ×8, F16→F32 ×6 |
| Architecture Issues | 8 | God function, dual state, legacy layout |
| Dead/Unused Code | 9+6 new | ExrBitDepth, TiffBitDepth, EncodeStage::Error |
| Security | 3 | Path traversal, no auth, no input validation |
| **Total findings** | **~100** | |

### Verification Results
- **BUG-01 (NaN blend): DENIED** — no division by alpha exists in compositor
- **BUG-02 (API Play): CONFIRMED** — stutter when already playing (not "broken when paused")
- **BUG-03 (NodeKind fps): CONFIRMED with nuance** — same result today, but shadows trait
- **All other bugs: CONFIRMED** with exact code references

---

## 1. VERIFIED Critical Bugs

### BUG-02: ApiCommand::Play stutter when already playing
**File:** `src/app/api.rs:90-95` | **Status:** CONFIRMED
```rust
ApiCommand::Play => {
    self.event_bus.emit(TogglePlayPauseEvent);      // unconditional
    if !self.player.is_playing() {                   // checks AFTER first toggle
        self.event_bus.emit(TogglePlayPauseEvent);   // fires again
    }
}
```
When already playing: stops → checks (now paused) → restarts = visible stutter.
When paused: starts (correct), condition false, no double-emit.
**Fix:** `if !self.player.is_playing() { self.event_bus.emit(TogglePlayPauseEvent); }`

### BUG-03: NodeKind::fps()/\_in()/\_out()/frame() shadow enum\_dispatch
**File:** `src/entities/node_kind.rs:162-207` | **Status:** CONFIRMED (code smell, not behavioral today)
Camera/Text hardcode 24.0, but since they don't set `A_FPS` in constructors, trait default returns 24.0 too. Will break if `A_FPS` is ever set on these node types.
**Fix:** Delete all 4 methods + `is_file_mode()`. Consider adding `A_FPS` to Camera/Text constructors.

### BUG-04: FPS denominator zero-check missing
**File:** `src/entities/loader_video.rs:43-49` | **Status:** CONFIRMED
```rust
let fps = fps_rational.numerator() as f64 / fps_rational.denominator() as f64;
let duration_secs = duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64;
```
Both `fps_rational.denominator()` and `time_base.denominator()` can be 0. Panics in debug, UB in release (`NaN as usize`).
**Fix:** Guard both with `if denom == 0 { return Err(...) }`.

### BUG-05: GPU compositor partial init leaks GL resources
**File:** `src/entities/gpu_compositor.rs:264-326` | **Status:** CONFIRMED
Guard: `blend_program.is_some() && vao.is_some() && fbo.is_some()`. If FBO fails after program+VAO succeed, next call re-creates program+VAO, leaking old ones on GPU.
**Fix:** Use `initialized: bool` flag. On failure, cleanup all already-created resources.

### BUG-06: deferred\_load\_sequences overwrites (+ 5 more deferred fields)
**File:** `src/app/events.rs:159-174` | **Status:** CONFIRMED
`deferred_load_sequences = Some(paths)` — overwrites, not extends. **Same pattern for `load_project`, `save_project`, `new_comp`, `new_camera`, `new_text`.**
**Fix:** `.get_or_insert_with(Vec::new).extend(paths)` for sequences. For others, queue instead of Option.

### BUG-07: ApiCommand::SetFps bypasses event bus
**File:** `src/app/api.rs:107-109` | **Status:** CONFIRMED
Only calls `player.set_fps_base(fps)` — doesn't update comp attrs, doesn't emit events, doesn't invalidate cache, won't be saved to project.
**Fix:** Call `adjust_fps_base()` or emit same event as keyboard path.

### BUG-08: HSV default value = 2.0 (doubles brightness)
**File:** `src/entities/effects/mod.rs:145-146` | **Status:** CONFIRMED
```rust
// value: 0.0 (black) to 2.0 (overbright), 1.0 = no change
AttrDef::with_ui_order("value", AttrType::Float, FX, &["0", "2", "0.01"], 2.0),
//                                                              default ^^^
```
Comment says "1.0 = no change", default is 2.0. Saturation correctly defaults to 1.0.
**Fix:** Change `2.0` → `1.0`.

### BUG-09: SetFrameEvent double preload
**File:** `src/main_events.rs:243-265` + `src/app/events.rs:42-45,177` | **Status:** CONFIRMED
1. `modify_comp(set_frame)` → emits `CurrentFrameChangedEvent` → immediate `enqueue_frame_loads_around_playhead()`
2. `result.enqueue_frames = true` → deferred `enqueue_frame_loads_around_playhead()`
**Fix:** Remove `result.enqueue_frames = true` from SetFrameEvent handler.

### BUG-10: Dehydrate skips Loading frames → stale data accepted
**File:** `src/core/global_cache.rs:373-389` | **Status:** PARTIALLY CONFIRMED
Dehydrate only marks `Loaded` → `Expired`. Worker finishes `Loading` frame with old data → becomes `Loaded` → accepted as fresh. Not a crash but correctness issue.
**Fix:** Epoch check on worker completion — if epoch mismatched, discard result.

### BUG-11: CameraNode use\_poi default mismatch
**File:** `src/entities/camera_node.rs:50,108-109` | **Status:** CONFIRMED
Constructor: `false`. Getter: `unwrap_or(true)`. Legacy saves without key get wrong default.
**Fix:** Getter → `unwrap_or(false)`.

### BUG-12: contains\_comp() doesn't check node type
**File:** `src/entities/project.rs:649-652` | **Status:** CONFIRMED
**Fix:** `self.with_comp(uuid, |_| ()).is_some()`

### BUG-13: Float truncation in video frame count
**File:** `src/entities/loader_video.rs:48-49` | **Status:** CONFIRMED
**Fix:** `.round() as usize`

---

## 2. NEW Bugs (Encode, Server, Misc)

### ENC-01: FPS precision truncation in video encoder
**File:** `src/dialogs/encode/encode.rs:~1256` | **Severity:** HIGH
`settings.fps as i32` truncates 23.976→23, 29.97→29. All timestamps drift. Audio/video sync broken.
**Fix:** Use `Rational::approximate(settings.fps as f64)` for frame rate.

### ENC-02: `octx.stream(0).unwrap()` — hardcoded stream index
**File:** `src/dialogs/encode/encode.rs:~1392` | **Severity:** MEDIUM
If muxer renumbers streams, panic. **Fix:** Store stream index from `add_stream()`.

### ENC-03: `sws_ctx.as_mut().unwrap()` in hot encoding loop
**File:** `src/dialogs/encode/encode.rs:~1483,1494` | **Severity:** MEDIUM
**Fix:** Use `?` or `ok_or_else`.

### ENC-04: UI freezes 2 seconds on encode stop
**File:** `src/dialogs/encode/encode_ui.rs` | **Severity:** HIGH
`stop_encoding_internal()` polls `is_finished()` with `sleep(100ms)` on the UI thread. Blocks egui for up to 2 seconds.
**Fix:** Channel-based notification or background thread for join.

### ENC-05: cleanup\_orphan\_handles drops without join()
**File:** `src/dialogs/encode/encode_ui.rs` | **Severity:** MEDIUM
Thread result (including panic info) silently discarded. **Fix:** Explicitly `join()` before removing.

### ENC-06: EXR/TIFF/TGA compression settings silently ignored
**Files:** `encode.rs` — `write_exr_frame`, `write_tiff_frame`, `write_tga_frame`
All three have `let _ = settings` / `// TODO:`. User selects compression in UI but output is always uncompressed/default.
**Fix:** Use lower-level APIs that expose compression (available in both `exr` and `tiff` crates).

### ENC-07: ExrBitDepth/TiffBitDepth per-format fields are dead code
**File:** `src/dialogs/encode/encode.rs` | **Severity:** LOW
Fields defined, shown in UI, but `write_*_frame` uses top-level `OutputBitDepth` instead.
**Fix:** Wire per-format bit depth to writers, or remove dead fields.

### SEC-01: Path traversal via `/api/project/load`
**File:** `src/server/api.rs` | **Severity:** HIGH (security)
Server binds `0.0.0.0`, accepts arbitrary file paths from JSON body without sanitization. No auth on any endpoint.
**Fix:** Default to `127.0.0.1`. Add token-based auth. Sanitize paths.

### SEC-02: No input validation on API FPS value
**File:** `src/server/api.rs` | **Severity:** MEDIUM
Can set FPS to 0.0, NaN, Inf, negative. **Fix:** `fps.clamp(0.001, 960.0)`.

### SEC-03: RwLock::read().unwrap() in all API handlers
**File:** `src/server/api.rs:~310-325` | **Severity:** MEDIUM
Poisoned lock → panic in HTTP thread → server thread dies.
**Fix:** `unwrap_or_else(|p| p.into_inner())` or return HTTP 503.

---

## 3. Performance Issues (Verified + New)

### Tier 1 — High Impact (Hot Paths)

| ID | File:Lines | Issue | Impact |
|----|-----------|-------|--------|
| PERF-01 | compositor.rs:327,332,337 | `curr.clone()` per layer in blend_with_dim | ~8MB alloc per layer per compose |
| PERF-04 | global_cache.rs:150-153 | `shift_remove` O(n) on every cache hit | 24K O(n) ops/sec at 24fps |
| NEW-01 | transform.rs:494,512 | Per-pixel invariant recomputed (tilt check, plane normal) | 2M redundant ops/frame |
| NEW-03 | renderer.rs:320-322 | `.to_vec()` on already-contiguous F16 slice | ~4MB alloc/frame for F16 |
| NEW-15 | global_cache.rs:223-224 | `LastOnly` strategy O(n) retain on every insert | O(n) per compose call |

### Tier 2 — Medium Impact

| ID | File:Lines | Issue | Impact |
|----|-----------|-------|--------|
| PERF-02 | loader.rs:131,327 | Full decode for metadata (non-openexr) | 100-500ms per header read |
| PERF-03 | compositor.rs:blend loops | apply_blend match 3×/pixel (`#[inline]` may help) | 8.3M matches/layer |
| PERF-05 | run.rs:189,203 | JSON serialize dock state 2×/frame | String alloc+compare at 60fps |
| NEW-02 | transform.rs:pixel closure | camera_info match inside per-pixel closure | Branch per pixel |
| NEW-04 | renderer.rs:500-541 | 7 `get_uniform_location` GL calls per frame | 7 avoidable GL calls/frame |
| NEW-08 | comp_node.rs:922-928 | Lock per frame in cache_frame_statuses | 1000 locks for 1000-frame comp |
| NEW-14 | loader.rs:168,185 | EXR opened twice (openexr path) | Double file I/O |
| ENC-P1 | encode.rs:~1483 | RGB48 byte-by-byte copy instead of slice | 25M tiny copies for 4K |
| ENC-P2 | encode.rs:frame loop | Frame clone per encode frame when dimensions match | Doubles peak encode memory |

### Tier 3 — Low Impact (Still Worth Fixing)

| ID | File:Lines | Issue |
|----|-----------|-------|
| NEW-06 | comp_node.rs:1174 | O(n) `insert(0, base)` on Vec |
| NEW-07 | comp_node.rs:1131,1144 | `is_identity` called twice |
| NEW-09 | run.rs:78-82 | Style clone every frame |
| NEW-10 | run.rs:71-75 | `set_visuals` every frame |
| NEW-11 | timeline_ui.rs:418 | `format!` String alloc per layer per frame |
| NEW-13 | compositor.rs:298 | Arc clone in per-layer loop |
| NEW-16 | global_cache.rs:268-272 | Mutex per eviction loop iteration |
| NEW-17 | run.rs:100-102 | `options_mut` every frame for constant |
| NEW-18 | comp_node.rs:1113 | Frame clone before apply_all effects |
| PERF-11 | player.rs | Attrs string map for hot-path state |

---

## 4. Code Deduplication (Original + New)

### From Compositor/Transform/Effects (DUP-02 expanded):
- `blend_f32`/`blend_f16`/`blend_u8` — same Porter-Duff, different types (~90 lines)
- `sample_f32`/`sample_f16`/`sample_u8` — same bilinear interp (~108 lines)
- 3× rayon dispatch blocks in transform.rs (~90 lines)
- 3× format loops in hsv.rs (~80 lines)
- `convolve_horizontal`/`convolve_vertical` — identical except axis (~120 lines)
**Total:** ~488 lines of triplicated code

### From Encode Pipeline (NEW):
- RGBA→RGB strip copy-pasted **8+ times** across all write functions
- F16→F32 conversion duplicated in every single writer
- Buffer-to-U8 conversion duplicated in PNG, TIFF, TGA writers
- `render_h264_settings` / `render_h265_settings` near-identical
- `load_from_settings` / `save_to_settings` verbose trace logs (30 lines each)

### From Event System:
- EventBus/EventEmitter 4 methods duplicated
- AttrsChangedEvent handler in main + derived loop
- RemoveMedia/RemoveSelectedMedia near-identical
- AlignLayersStart/End structurally identical
- SetLayerPlayStart/End structurally identical
- 3× timeline fit zoom calc
- Playlist loading duplicates load_project()

---

## 5. Architecture Issues

| ID | Issue | File |
|----|-------|------|
| ARCH-01 | `handle_app_event()` god function: 1232 lines, 16 params | main_events.rs |
| ARCH-02 | Dual source of truth for loop state (Player + AppSettings) | main_events.rs |
| ARCH-03 | Two parallel layout serialization systems (legacy + current) | layout.rs, layout_events.rs |
| ARCH-04 | HoverLayerEvent → modify_comp → spurious cache invalidation | main_events.rs |
| ARCH-05 | Workers: comment says LIFO, uses FIFO; self-steal | workers.rs:62,79 |
| ARCH-06 | Multi-node attrs edit bypasses event bus | tabs.rs |
| ARCH-07 | load_project() missing ViewportRefreshEvent | project_io.rs |
| ARCH-08 | EventBus silently drops 500 events on overflow | event_bus.rs |

---

## 6. Dead / Unused Code

| Item | File | Notes |
|------|------|-------|
| `is_file_mode()` | node_kind.rs:157 | Exact duplicate of `is_file()` |
| `src_to_object()` | space.rs:71-77 | `#[allow(dead_code)]` |
| `COMP_NORMAL`, `COMP_FILE`, `A_MODE` | keys.rs | Legacy constants |
| `collect_changes` param | ae_ui.rs | Always `true` |
| `StatusBar::update()` | status.rs | No-op body |
| Play icon never toggles | timeline_ui.rs:~98 | Always "▶" |
| `_saved_spacing` | timeline_ui.rs:623 | Saved but never restored |
| `EncodeStage::Error` | encode.rs | Never emitted by encoder |
| `ExrBitDepth` field | encode.rs | Defined in UI, never read in writer |
| `TiffBitDepth` field | encode.rs | Defined in UI, never read in writer |
| `render_general_settings` | prefs.rs | Empty stub |
| `HotkeyWindow` in prefs_events | prefs_events.rs | Not a prefs event, belongs in input_handler |
| `encode_comp` / `encode_sequence_from_comp` | encode.rs | One-liner wrapper, consolidate names |
| Duplicate key handler branch | input_handler.rs:~30 | Second branch unreachable (first already matches) |
| `get_shader_names()` returns cloned Strings | shaders.rs | Should return `&str` if used in hot path |

---

## 7. Security

| ID | Issue | Severity | File |
|----|-------|----------|------|
| SEC-01 | API server binds `0.0.0.0` with no auth — can load arbitrary files, exit app, take screenshots | HIGH | server/api.rs |
| SEC-02 | No FPS validation — NaN/Inf/0/negative accepted | MEDIUM | server/api.rs |
| SEC-03 | No frame number validation — arbitrary i32 | LOW | server/api.rs |

---

## Sub-Reports Reference

| Report | Focus | Findings |
|--------|-------|----------|
| `_verify_bugs.md` | Bug verification (13 bugs) | 1 denied, 12 confirmed |
| `_verify_perf.md` | Perf verification (5) + new (16) | All 5 confirmed + 16 new |
| `_report_compositor.md` | Compositor, frame, cache, effects, workers | 9 critical + 11 perf + 8 dedup |
| `_report_events.md` | Event system, app state, player | 5 critical + 6 perf + 7 dedup + 11 arch |
| `_report_ui_entities.md` | UI widgets, node types, project | 4 critical + 6 perf + 5 dedup + 7 dead |
| `_report_encode_server_misc.md` | Encode, server, prefs, shaders, gizmo | 10 critical + 5 perf + 7 dedup + 7 dead + 3 security |

---

*Generated by Claude Code Bug Hunt v2 — 9 analysis agents, 2026-03-18*
