# Plan 1 — Bug Hunt Implementation Plan v2 (Expanded)

**Date:** 2026-03-18
**Based on:** REPORT.md v2 (~100 findings, 9 analysis agents, verification pass)
**Status:** AWAITING APPROVAL

---

## Phase 1 — Critical Bugs + Security (Est: 2-3 sessions)

### 1.1 API fixes
- [ ] `api.rs:90-95` — Fix Play: `if !self.player.is_playing() { emit(...) }`
- [ ] `api.rs:107-109` — SetFps: route through `adjust_fps_base()` or emit event
- [ ] `server/api.rs` — Bind to `127.0.0.1` by default instead of `0.0.0.0`
- [ ] `server/api.rs` — Validate FPS input: `fps.clamp(0.001, 960.0)` + reject NaN/Inf
- [ ] `server/api.rs` — Document security implications, consider token auth
- [ ] `server/api.rs:~310` — Replace `.unwrap()` on RwLock with poison recovery

### 1.2 NodeKind enum_dispatch cleanup
- [ ] Delete `fps()`, `_in()`, `_out()`, `frame()` from `impl NodeKind` (lines 162-207)
- [ ] Delete `is_file_mode()` (line 157), replace call sites with `is_file()`
- [ ] Grep all callers to verify trait dispatch works
- [ ] Consider adding `A_FPS` to Camera/Text constructors

### 1.3 Video loader safety
- [ ] `loader_video.rs:43-49` — Guard `fps_rational.denominator()` and `time_base.denominator()` for zero
- [ ] `loader_video.rs:49` — Use `.round() as usize` for frame count

### 1.4 Event system fixes
- [ ] `events.rs:165` — Accumulate `deferred_load_sequences` (`.get_or_insert_with(Vec::new).extend()`)
- [ ] Check same pattern for `load_project`, `save_project`, `new_comp`, `new_camera`, `new_text`
- [ ] `main_events.rs:~250` — Remove `result.enqueue_frames = true` from SetFrameEvent handler

### 1.5 Other critical fixes
- [ ] `effects/mod.rs:146` — HSV default value: `2.0` → `1.0`
- [ ] `camera_node.rs:109` — use_poi getter: `unwrap_or(true)` → `unwrap_or(false)`
- [ ] `project.rs:650` — `contains_comp()`: check node type, not just existence
- [ ] `gpu_compositor.rs:264` — Use `initialized: bool` flag; cleanup partial resources on failure
- [ ] `global_cache.rs:373` — Epoch check on worker completion for dehydrate race

### 1.6 Encode pipeline fixes
- [ ] `encode.rs:~1256` — Use `Rational::approximate(fps)` instead of `fps as i32`
- [ ] `encode.rs:~1392` — Store stream index from `add_stream()`, don't hardcode 0
- [ ] `encode.rs:~1483` — Replace `.unwrap()` with `?` or `ok_or_else` on `sws_ctx`
- [ ] `encode_ui.rs` — Move stop_encoding join to background thread (fix 2s UI freeze)
- [ ] `encode_ui.rs` — Explicitly `join()` in cleanup_orphan_handles before dropping

---

## Phase 2 — Performance: Hot Paths (Est: 2-3 sessions)

### 2.1 Compositor hot path
- [ ] `compositor.rs:327` — Eliminate `curr.clone()` per layer in blend_with_dim → single output buffer
- [ ] `compositor.rs:251` — Hoist `apply_blend` match outside pixel loop → function pointer/closure
- [ ] `compositor.rs:298` — Extract `result.buffer()` Arc before loop

### 2.2 Transform hot path
- [ ] `transform.rs:494,512` — Hoist plane_normal tilt check, camera_info match, all invariants BEFORE `par_iter` closure
- [ ] `comp_node.rs:1131,1144` — Cache `is_identity()` result in local binding
- [ ] `comp_node.rs:1174` — Pre-allocate Vec with base frame at index 0 instead of `insert(0,...)`
- [ ] `comp_node.rs:1113` — Pass frame by move to `apply_all` instead of `.clone()`

### 2.3 Cache hot path
- [ ] `global_cache.rs:150` — Replace `shift_remove` O(n) with O(1) LRU (use `lru` crate or HashMap+linked list)
- [ ] `global_cache.rs:223` — Fix `LastOnly` strategy O(n) retain on every insert
- [ ] `global_cache.rs:268` — Hold mutex for entire eviction loop, not per-iteration
- [ ] `comp_node.rs:922` — Batch `get_statuses(comp_uuid, range)` API with single lock

### 2.4 Renderer hot path
- [ ] `renderer.rs:320` — Remove `.to_vec()` on F16 cast_slice — pass `&[u8]` directly to GL
- [ ] `renderer.rs:500-541` — Cache uniform locations after shader compilation

### 2.5 Per-frame overhead
- [ ] `run.rs:189` — Dirty flag for dock state instead of JSON serialize ×2
- [ ] `run.rs:78` — Cache font size, skip `set_style` when unchanged
- [ ] `run.rs:71` — Track dark_mode, skip `set_visuals` when unchanged
- [ ] `run.rs:100` — Move `options_mut(max_passes)` to init
- [ ] `timeline_ui.rs:418` — Replace `format!` with `egui::Id::new().with()`

### 2.6 Loader I/O
- [ ] `loader.rs:131` — Header-only read for non-openexr EXR (use `exr` crate metadata API)
- [ ] `loader.rs:327` — Header-only read for generic formats where possible
- [ ] `loader.rs:168` — Open EXR once, pass handle to load_exr_half/float

### 2.7 Encode performance
- [ ] `encode.rs` — SwsContext RGB48: use `bytemuck::cast_slice` instead of byte-by-byte
- [ ] `encode.rs` — Use `Cow<Frame>` to avoid clone when dimensions match
- [ ] `encode.rs` — Use `SWS_FAST_BILINEAR` for same-dimension format conversion

---

## Phase 3 — Deduplication (Est: 3-4 sessions)

### 3.1 Pixel format unification (~488 lines saved)
- [ ] Design `PixelAccessor` / `SampleDecode` trait for element→f32 conversion
- [ ] Unify `blend_f32`/`blend_f16`/`blend_u8` → single generic
- [ ] Unify `sample_f32`/`sample_f16`/`sample_u8` → single generic
- [ ] Unify 3× rayon dispatch in transform.rs → single generic
- [ ] Unify 3× format loops in hsv.rs → single inner function
- [ ] Merge `convolve_horizontal`/`convolve_vertical` → `convolve_axis(direction)`

### 3.2 Encode helpers (~200 lines saved)
- [ ] Extract `fn strip_alpha_u8(rgba: &[u8]) -> Vec<u8>` — used 8+ times
- [ ] Extract `fn f16_to_f32_buf(data: &[f16]) -> Vec<f32>` — used 6+ times
- [ ] Extract `fn frame_to_rgba8(frame: &Frame) -> Vec<u8>` — used 3+ times
- [ ] Merge `render_h264_settings` / `render_h265_settings` → `render_h26x_settings`
- [ ] Consolidate `encode_comp` / `encode_sequence_from_comp` names

### 3.3 Event system (~150 lines saved)
- [ ] `EventEmitter` → `struct EventEmitter(Arc<EventBus>)` with `Deref<Target=EventBus>`
- [ ] Extract `handle_attrs_changed()` helper (main + derived loop)
- [ ] Extract `handle_media_removal(removed: &[Uuid])` (Remove + RemoveSelected)
- [ ] Extract `align_layers_to_frame(comp, anchor: Bound)` (Start + End)
- [ ] Parameterize `SetLayerPlayStart/End` on trim anchor
- [ ] Extract `fit_timeline_to_range(state, width, min, max)` (3 handlers)
- [ ] Unify playlist loading with `load_project()`

### 3.4 Misc helpers
- [ ] `loader.rs` — Extract `classify_path(path) -> FileType` for extension detection
- [ ] `frame.rs` — Extract `make_placeholder_buffer(w, h) -> Vec<u8>`
- [ ] `config.rs` — Unify `get_config_dir`/`get_data_dir` → `get_app_dir(base)`
- [ ] `project.rs` — Derive `Serialize` on `ProjectPrefs`, replace `prefs_to_map`/`prefs_from_map`

---

## Phase 4 — Architecture (Est: 2-3 sessions)

- [ ] Create `AppEventContext<'_>` struct for `handle_app_event()` 16 params
- [ ] Single source of truth for loop state (remove from Player or AppSettings)
- [ ] Complete layout migration: remove legacy SaveLayout/LoadLayout, add one-time migration
- [ ] Move hover state to `timeline_state`, bypass `modify_comp()`
- [ ] Emit `ViewportRefreshEvent` at end of `load_project()`
- [ ] Unify multi-node attrs edit through `AttrsChangedEvent`
- [ ] EventBus overflow: reject or backpressure instead of silent eviction
- [ ] Fix workers.rs: FIFO comment, exclude self from steal list
- [ ] Node trait: add `attach_schema()` method, collapse manual match
- [ ] Player: migrate hot-path state from Attrs to typed struct fields

---

## Phase 5 — Dead Code Cleanup (Est: 1 session)

- [ ] Delete `is_file_mode()`, replace callers with `is_file()`
- [ ] Delete `src_to_object()` from space.rs (or add clear TODO)
- [ ] Delete `COMP_NORMAL`, `COMP_FILE`, `A_MODE` from keys.rs (verify no callers first)
- [ ] Remove `collect_changes: bool` from ae_ui.rs, delete false branch
- [ ] Remove `StatusBar::update()` no-op (or implement)
- [ ] Fix play icon toggle in timeline_ui.rs
- [ ] Remove `_saved_spacing` assignment or implement restore
- [ ] Remove `EncodeStage::Error` (never emitted) or wire it up
- [ ] Wire `ExrBitDepth`/`TiffBitDepth` to writers or remove from UI
- [ ] Remove/implement `render_general_settings` stub
- [ ] Move `HotkeyWindow` from prefs_events to input_handler
- [ ] Fix unreachable duplicate key handler branch in input_handler.rs
- [ ] Wire EXR/TIFF/TGA compression settings to writers (TODOs)

---

## Execution Notes

- Build with `start.cmd` after each phase to verify no regressions
- Phase 1.2 (NodeKind) is highest-risk — grep ALL callers of `.fps()`, `._in()`, `._out()`, `.frame()` on NodeKind before deleting
- Phase 3.1 (pixel format unification) is largest refactor — implement on ONE effect (e.g., blur) first, verify, then propagate
- Do NOT remove any features — only consolidate and deduplicate
- Security fixes (Phase 1.1) should ship first if server is used in production
- All `let _ =` on `load_sequences` should log errors to status bar
