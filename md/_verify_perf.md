# Performance Verification Report
Generated: 2026-03-18

---

## Part 1: Verified Claims (PERF-01 through PERF-05)

### PERF-01 — CONFIRMED
**Claim:** `blend_with_dim` clones the full pixel buffer once per layer.
**Location:** `src/entities/compositor.rs` lines 327, 332, 337
**Evidence:** Inside the per-layer loop, `let mut blended = curr.clone()` appears at each match arm for F32, F16, and U8 pixel buffer variants. For a 1920×1080 F32 frame this is ~8 MB allocated and copied per composited layer above the base on every compose call.

---

### PERF-02 — CONFIRMED (non-openexr path)
**Claim:** EXR header reads perform a full image decode.
**Location:** `src/entities/loader.rs` lines 131–141 (`header_exr` non-openexr), line 327 (`header_generic`)
**Evidence:** Both `header_exr` (without the `openexr` feature) and `header_generic` call `reader.decode()` — a full image decode — just to read image dimensions and channel count. The `openexr`-feature path at lines 98–116 correctly opens `RgbaInputFile` and reads only the header object, so that path is not affected.

---

### PERF-03 — CONFIRMED (with nuance)
**Claim:** `apply_blend` dispatches on `BlendMode` three times per pixel.
**Location:** `src/entities/compositor.rs` — `blend_f32`, `blend_f16`, `blend_u8` inner loops
**Evidence:** `apply_blend(b, t, mode)` is called three times per pixel (R, G, B channels) with the same `mode` value. The function is `#[inline]`, which may allow the compiler to hoist the match in optimized builds, but in debug builds and potentially in opt-level < 3 the match is re-evaluated 3× per pixel. The real fix is a channel-triplet variant that accepts `(rb, gb, bb, rt, gt, bt)` to match once per pixel.

---

### PERF-04 — CONFIRMED
**Claim:** LRU cache uses `shift_remove` (O(n)) on every cache hit.
**Location:** `src/core/global_cache.rs` lines 150–153
**Evidence:** The code calls `lru.shift_remove(&key)` then `lru.insert(key)` on every cache hit to promote the key to the back. `IndexSet::shift_remove` is O(n) because it physically shifts all subsequent entries. The comment at line 150 explicitly acknowledges this. `swap_remove` is O(1) but changes ordering; the correct fix is either a purpose-built LRU structure or using `swap_remove` with index-based ordering tracking.

---

### PERF-05 — CONFIRMED
**Claim:** `serde_json::to_string` called twice per frame to detect dock state changes.
**Location:** `src/app/run.rs` lines 189 and 203
**Evidence:** `serde_json::to_string(&dock_state)` is called before and after `DockArea::new(...).show(...)` every frame, and the two strings are compared with `!=`. Full JSON serialization of the entire dock tree per frame. Should be replaced with a dirty-flag or change counter on `DockState`.

---

## Part 2: New Issues Not in Original Report

### NEW-01 — transform.rs: Per-pixel invariant recomputed for every output pixel
**Location:** `src/entities/transform.rs` — `transform_frame_with_camera`, closure `transform_point`, lines 494, 512
**Category:** Loop invariant not hoisted
**Detail:** `(plane_normal - Vec3::Z).length_squared() > 1e-6` is computed on every output pixel call. `plane_normal` is fixed for the entire frame. The tilt-check boolean and all derived constants (tilt axis, tilt angle, rotation matrices) should be computed once before the `par_iter` closure. At 1920×1080 this is 2,073,600 redundant `length_squared` calls plus conditional branching per frame.

---

### NEW-02 — transform.rs: `camera_info` match and tilt check inside per-pixel closure
**Location:** `src/entities/transform.rs` — `transform_frame_with_camera` per-pixel closure
**Category:** Loop invariant not hoisted
**Detail:** The `camera_info` match (selecting perspective vs orthographic projection parameters) and `layer_is_tilted` check are both frame-invariant but evaluated inside the Rayon parallel per-pixel closure. These should be resolved to concrete values before the `par_iter` call and captured by value in the closure.

---

### NEW-03 — renderer.rs: F16 upload allocates Vec<u8> every frame
**Location:** `src/widgets/viewport/renderer.rs` lines 320–322
**Category:** Unnecessary heap allocation
**Detail:** `bytemuck::cast_slice(self.f16_scratch.as_slice()).to_vec()` creates a new `Vec<u8>` by copying data that is already a contiguous byte slice. `bytemuck::cast_slice` returns `&[u8]` directly — the `.to_vec()` call is unnecessary and allocates ~4 MB per frame for a 1920×1080 F16 texture. Pass the `&[u8]` slice directly to `gl.tex_image_2d`.

---

### NEW-04 — renderer.rs: `get_uniform_location` called 7 times per frame
**Location:** `src/widgets/viewport/renderer.rs` lines 500, 508, 516, 528, 533, 536, 541
**Category:** Missing cache
**Detail:** `gl.get_uniform_location(program, "...")` is called for 7 uniforms every frame. Uniform locations are stable for the lifetime of a linked shader program and should be cached in the renderer struct after shader compilation (or after first use with a `Option<UniformLocation>` field). This is 7 GL calls per frame that are entirely avoidable.

---

### NEW-06 — comp_node.rs: `source_frames.insert(0, ...)` O(n) shift
**Location:** `src/entities/comp_node.rs` line 1174
**Category:** O(n) insert at front of Vec
**Detail:** The black base frame is inserted at index 0 of `source_frames` after all other frames have been pushed. This shifts every existing element one position. Since the base is always first and is known before the loop, it should either be pushed last and the compositing order reversed, or the Vec should be pre-allocated with the base at index 0 before the loop.

---

### NEW-07 — comp_node.rs: `is_identity` called twice with identical arguments
**Location:** `src/entities/comp_node.rs` lines 1131 and 1144
**Category:** Redundant computation
**Detail:** `is_identity(pos, rot_rad, scl, pvt)` is called at line 1131 (to skip transform) and again at line 1144 (same condition). The result should be stored in a `let identity = is_identity(...)` binding before the conditional block.

---

### NEW-08 — comp_node.rs: `cache_frame_statuses` acquires a lock per frame index
**Location:** `src/entities/comp_node.rs` lines 922–928
**Category:** Excessive lock acquisition
**Detail:** `cache.get_status(comp_uuid, frame_idx)` is called in a tight loop for every frame in the comp's range. Each call acquires a read lock on the global cache. For a 1000-frame comp this is 1000 lock acquisitions per status refresh. A batch `get_statuses(comp_uuid, range)` API would reduce this to a single lock.

---

### NEW-09 — run.rs: Style cloned and re-applied every frame unconditionally
**Location:** `src/app/run.rs` lines 78–82
**Category:** Unnecessary work per frame
**Detail:** `(*ctx.style()).clone()` + `ctx.set_style(style)` is called every frame regardless of whether `font_size` has changed. Should guard with a dirty flag or compare the current font size before rebuilding and re-applying the style.

---

### NEW-10 — run.rs: `ctx.set_visuals(...)` called every frame
**Location:** `src/app/run.rs` lines 71–75
**Category:** Unnecessary work per frame
**Detail:** `ctx.set_visuals(...)` is called unconditionally every frame even if `dark_mode` has not changed. Should be applied only once at startup and on mode change.

---

### NEW-11 — timeline_ui.rs: `format!` allocates String per layer per frame for egui ID
**Location:** `src/widgets/timeline/timeline_ui.rs` line 418
**Category:** Unnecessary heap allocation
**Detail:** `format!("blend_outline_{}", child_uuid)` allocates a `String` for every visible layer on every timeline render frame to create an egui ComboBox ID. `egui::Id::new("blend_outline").with(child_uuid)` is allocation-free and should be used instead.

---

### NEW-13 — compositor.rs: `result.buffer()` (Arc clone) inside per-layer loop
**Location:** `src/entities/compositor.rs` line 298 (inside per-layer loop in `blend_with_dim`)
**Category:** Unnecessary Arc clone in hot path
**Detail:** `result.buffer()` clones an `Arc` pointer inside the per-layer loop on every iteration. While an Arc clone is cheap (~2 atomic ops), it is still avoidable — the buffer reference can be extracted once before the loop.

---

### NEW-14 — loader.rs: EXR file opened twice per load (openexr path)
**Location:** `src/entities/loader.rs` lines 168 and 185
**Category:** Redundant file I/O
**Detail:** On the `openexr` feature path, `load_exr` opens the EXR file at line 168 to detect the pixel type (half vs float), drops the handle at line 185, then `load_exr_half` or `load_exr_float` opens the same file again. The pixel type information (or the open file handle) should be passed through to avoid two separate filesystem opens of the same file.

---

### NEW-15 — global_cache.rs: `LastOnly` insert strategy scans entire LRU queue
**Location:** `src/core/global_cache.rs` lines 223–224
**Category:** O(n) scan in hot path
**Detail:** `insert()` with the `LastOnly` caching strategy calls `clear_comp(comp_uuid)` which performs `lru.retain(...)` — an O(n) scan of the entire LRU queue — on every single frame insert under that strategy. If `LastOnly` is used for compositor output frames this runs every compose call.

---

### NEW-16 — global_cache.rs: `enforce_limits()` acquires Mutex once per loop iteration
**Location:** `src/core/global_cache.rs` lines 268–272
**Category:** Excessive lock acquisition
**Detail:** The eviction loop in `enforce_limits()` calls `self.len()` on each iteration. `len()` acquires the `Mutex<IndexSet>` lock each call. The length should be read once before the loop (or the loop should hold the lock for its entire duration) to avoid repeated lock/unlock cycles during eviction.

---

### NEW-17 — run.rs: `ctx.options_mut` called every frame to set a constant
**Location:** `src/app/run.rs` lines 100–102
**Category:** Unnecessary work per frame
**Detail:** `ctx.options_mut(|opts| { opts.max_passes = NonZeroUsize::new(2).unwrap(); })` is called every frame. `NonZeroUsize::new(2).unwrap()` is a compile-time constant, and `max_passes` does not change. This should be set once during app initialization (e.g., in the `App::new` constructor or the first-frame setup block).

---

### NEW-18 — comp_node.rs: Frame cloned before `apply_all` effects unnecessarily
**Location:** `src/entities/comp_node.rs` line 1113
**Category:** Unnecessary clone
**Detail:** `super::effects::apply_all(frame.clone(), &layer.effects)` clones the frame before passing it to effects. The return value of `apply_all` replaces `frame` immediately after. If `apply_all` takes ownership, the clone is unnecessary — pass `frame` directly and rebind: `frame = super::effects::apply_all(frame, &layer.effects)`.

---

## Summary Table

| ID | File | Line(s) | Category | Severity |
|----|------|---------|----------|----------|
| PERF-01 | compositor.rs | 327,332,337 | Clone in hot loop | High |
| PERF-02 | loader.rs | 131, 327 | Full decode for header | Medium |
| PERF-03 | compositor.rs | blend inner loops | Match 3x per pixel | Medium |
| PERF-04 | global_cache.rs | 150–153 | O(n) LRU shift_remove | High |
| PERF-05 | run.rs | 189, 203 | JSON serialize per frame | Medium |
| NEW-01 | transform.rs | 494, 512 | Invariant in pixel loop | High |
| NEW-02 | transform.rs | pixel closure | Invariant in pixel loop | Medium |
| NEW-03 | renderer.rs | 320–322 | Heap alloc per frame | High |
| NEW-04 | renderer.rs | 500–541 | GL calls per frame | Medium |
| NEW-06 | comp_node.rs | 1174 | O(n) Vec front insert | Low |
| NEW-07 | comp_node.rs | 1131, 1144 | Redundant fn call | Low |
| NEW-08 | comp_node.rs | 922–928 | 1000 lock acquires | Medium |
| NEW-09 | run.rs | 78–82 | Style rebuild per frame | Low |
| NEW-10 | run.rs | 71–75 | Visuals set per frame | Low |
| NEW-11 | timeline_ui.rs | 418 | String alloc per layer | Low |
| NEW-13 | compositor.rs | 298 | Arc clone in layer loop | Low |
| NEW-14 | loader.rs | 168, 185 | Double file open | Low |
| NEW-15 | global_cache.rs | 223–224 | O(n) retain on insert | High |
| NEW-16 | global_cache.rs | 268–272 | Mutex per loop iter | Low |
| NEW-17 | run.rs | 100–102 | options_mut per frame | Low |
| NEW-18 | comp_node.rs | 1113 | Unnecessary clone | Low |
