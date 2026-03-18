# Compositor & Frame Pipeline Audit

Audit date: 2026-03-18  
Files examined (all read in full):
- `src/entities/compositor.rs` (350 lines)
- `src/entities/comp_node.rs` (1585 lines)
- `src/entities/frame.rs` (1348 lines)
- `src/entities/loader.rs` (369 lines)
- `src/entities/loader_video.rs` (204 lines)
- `src/entities/transform.rs` (668 lines)
- `src/entities/gpu_compositor.rs` (892 lines)
- `src/entities/effects/mod.rs` (248 lines)
- `src/entities/effects/blur.rs` (247 lines)
- `src/entities/effects/brightness.rs` (166 lines)
- `src/entities/effects/hsv.rs` (239 lines)
- `src/core/workers.rs` (234 lines)
- `src/core/cache_man.rs` (223 lines)
- `src/core/global_cache.rs` (714 lines)

---

## Critical Issues

**compositor.rs:327-337** — `blend_with_dim` clones the entire pixel buffer once per layer iteration. Each call to `blend_with_dim` with N layers allocates N full-resolution intermediate frames. For a 1920x1080 F32 comp with 10 layers, this is ~800 MB of temporary allocations. The intent is to find `dim` first, but `dim` is passed in as a parameter. The clone is entirely unnecessary; the existing buffer should be passed directly into `blend()`.

**loader.rs:131-163** — `header_exr` (non-openexr feature path) fully decodes the entire EXR image via `reader.decode()` at line 163 just to read width/height metadata. A 4K EXR decode takes 100-500ms on the CPU. This is called from every preload scheduling check and blocks the worker thread without doing useful work.

**loader.rs:317-342** — `header_generic` also fully decodes the image via `reader.decode()` at line 317 to get metadata. For large PNGs/TIFFs this can be hundreds of milliseconds. The `image` crate has no separate metadata reader for all formats, but for formats with headers (PNG, TIFF, EXR) a proper bounds check should be done without full decode.

**loader_video.rs:48** — FPS denominator is never checked for zero before use on line 48: `fps_rational.denominator()`. If a corrupted or malformed video has denominator=0, this produces NaN/infinity for `fps`, propagating silently into frame count computation and potentially causing integer overflow when cast to usize on line 49.

**loader_video.rs:97-98** — Thread count is set via unsafe raw pointer dereference: `(*decoder.as_mut_ptr()).thread_count = 0`. This bypasses the safe API and may write to freed memory if the decoder object is moved or the pointer is stale. The value 0 means "auto" but also ignores any application-level thread budget, potentially over-subscribing the CPU alongside the rayon pool and worker pool.

**gpu_compositor.rs:264** (`ensure_initialized`) — Partial initialization is possible. If shader compilation succeeds but VAO creation fails on line 290, `blend_program` is set but `vao` is `None`. Subsequent calls to `blend_textures` will return `Err("VAO not initialized")` but `blend_program` remains set. If `ensure_initialized` is called again, it will skip initialization entirely because `blend_program.is_some()` is the guard. The GPU compositor will silently fail on every frame, always falling back to CPU, with no way to recover without restarting.

**global_cache.rs:384** — `set_status` result is silently discarded with `let _ =` in the dehydrate path of `clear_comp`. `set_status` returns `Result<usize, FrameError>`. Discarding this means a status transition failure (e.g., lock poisoned) is invisible, leaving frames in an incorrect state and potentially causing the UI to show stale content permanently.

**comp_node.rs:1174** — `source_frames.insert(0, base)` performs an O(n) Vec insert at position 0 on every compose call. This shifts every subsequent element. For a comp with 20 layers this is 20 shift operations, each potentially moving 20 elements. The base frame should be pushed last and the blending loop reversed, or the Vec should be constructed in reverse.

**workers.rs:79** — Comment says "LIFO for cache locality" but `Worker::new_fifo()` is called. FIFO means older tasks execute first, which actively hurts cache locality for preload — tasks enqueued nearest the current frame position should execute first (LIFO). The worker queues have no practical value anyway because `execute` always pushes to the global `injector` and never to per-worker queues.

---

## Performance Improvements

**compositor.rs:314-345** — `blend_with_dim` clones the entire buffer for every layer (`let mut blended = curr.clone()` at lines 327, 332, 337) rather than blending in-place or returning a single allocated output buffer. Replace with a single output allocation matching `dim` and blend directly into it.

**compositor.rs:251-312** — `apply_blend` is called once per pixel channel and contains a `match mode` branch per call. For a 1920x1080 F32 image this is 8.3 million `match mode` evaluations per layer. The blend mode is fixed for the entire frame; hoist the match outside the pixel loop and pass a function pointer or use a single closure that captures the blended operation.

**comp_node.rs:1168-1178** — `promote_frame` is called in a loop that iterates all source frames even when they are already in the target format. The early-return path on line 1476-1478 (`frame.clone()` when formats match) still clones the `Arc<Mutex<FrameData>>` and increments the ref count unnecessarily. Accept `&Frame` and return `Cow<Frame>` or an explicit enum to distinguish cloned vs borrowed.

**transform.rs:538-632** — Three near-identical pixel dispatch blocks (F32/F16/U8) each contain the full rayon parallel loop. The per-pixel transform logic is identical across formats; only the sample function and output write differ. Extract the common rayon dispatch into a generic function parameterized on `SampleFn` and `WriteFn` to reduce code to a single loop body.

**blur.rs:62-120** — `to_f32_buffer` is called at the start of every blur operation, converting the entire source image to f32 regardless of whether it is already f32. For F32 frames this is a complete unnecessary copy of the pixel buffer. Check the source format first and skip conversion for F32 inputs.

**blur.rs:62-240** — The blur creates 3 full-resolution f32 buffers: source conversion, horizontal pass temp, and vertical pass result. A separable Gaussian on a 1920x1080 image allocates ~24 MB per blur call. Consider reusing the source buffer as a scratch pad when the source is f32, and allocating a single temp buffer shared across effects.

**blur.rs:146-165** — `convolve_horizontal` and `convolve_vertical` are identical except for axis indexing. This is 120 lines of duplicated code. Factor into a single `convolve_axis` function with an enum or bool for direction.

**blur.rs:124-135** — `gaussian_kernel()` computes `norm = 1.0 / (sigma * SQRT_2PI)` and multiplies each weight by it, then immediately divides every element by the sum to normalize. The `norm` multiplication is redundant because normalization by sum cancels it. Remove the `norm` multiplication; just normalize by sum.

**hsv.rs:33-130** — `apply` has three near-identical pixel loops for U8/F16/F32. The only differences are: input decode (u8/f16→f32), clamp behavior for value, and output encode. Factor into a shared inner loop operating on f32 slices with format-specific decode/encode functions.

**loader.rs:168-235** — `load_exr` (openexr feature path) opens the file twice: once to detect pixel type (line 168-185) and once to actually read data in `load_exr_half` or `load_exr_float`. This doubles I/O and metadata parsing. Open once, read pixel type from the already-open file, then pass the open handle to the load functions.

**loader.rs:213-230** — `load_exr` (non-openexr path) decodes the EXR as `rgba32f` first, then immediately converts to f16 when the source pixel type is HALF. The intermediate f32 allocation is `width * height * 4 * 4` bytes — for 4K EXR that is 128 MB of temporary allocation that is immediately discarded. Decode directly as f16 from the outset.

**global_cache.rs:152** — LRU update on every cache hit uses `shift_remove` which is O(n) in the IndexSet because it must shift all subsequent elements to preserve insertion order. For a 1000-frame cache at 24fps playback, this is ~24000 O(n) shifts per second. Use a generation counter or a doubly-linked list for true O(1) LRU. Alternatively, since playback is sequential, reconsider whether LRU is even the right eviction policy — LFU or window-based eviction would be cheaper.

**global_cache.rs:263-272** — `enforce_limits` calls `self.len()` on every iteration of the capacity loop at line 268, which acquires the `lru_order` Mutex on every iteration. Cache the length and update it as frames are evicted.

**loader_video.rs:15-68** — `VideoMetadata::from_file` creates a full decoder context and opens the codec just to read width/height. For metadata-only queries, codec parameters are available directly from `AVStream.codecpar` without creating a decoder. This is 5-10x faster for metadata.

**comp_node.rs:974-1197** — `compose_internal` calls `is_dirty` at the start of every `compute` call, which recursively traverses the source node graph. For a deep comp graph with 50 nodes, this is 50 recursive calls on the UI thread before any frame work begins. Since dirty flags are per-node booleans, a single propagation pass when attributes change (marking all dependents dirty) would make the check O(1) per node at render time.

---

## Code Deduplication

**compositor.rs:118-218** — `blend_f32`, `blend_f16`, and `blend_u8` are three functions of ~30 lines each that differ only in the element type (f32, f16, u8) and the alpha computation (f16/f32 are identical; u8 scales by 255). They share the same `apply_blend` call structure, the same 4-channel loop stride, and the same Porter-Duff formula. Suggested fix: a single generic function parameterized on a `BlendPixel` trait, or a macro. The f16 path converts to f32 per-pixel anyway, so f16 and f32 could share one implementation.

**transform.rs:289-397** — `sample_f32`, `sample_f16`, `sample_u8` are three 36-line functions that are identical except for the element type decode. All three implement bilinear interpolation with the same coordinate clamp, the same `tl/tr/bl/br` grid sampling, and the same lerp formula. `sample_f16` and `sample_u8` both convert to f32 internally and return `[f32; 4]`. Suggested fix: a single `sample_generic<T: SampleDecode>(buf: &[T], ...) -> [f32; 4]` with a `SampleDecode` trait for element→f32 conversion.

**transform.rs:538-632** — Three nearly identical rayon dispatch blocks for F32/F16/U8 output. The only differences are: output buffer type, call to `sample_*`, and write-back conversion. The boilerplate accounts for ~90 lines of 3-way repetition. Refactor into a macro or a generic dispatch function.

**loader.rs:31-56** — Extension detection (lower-case, match against known image extensions) is duplicated between `header()` (lines 31-41) and `load()` (lines 46-56). Both blocks independently lowercase the extension and match the same set of strings. Extract into a single `classify_path(path) -> FileType` helper.

**loader.rs:149-155 and 336-342** — Channel-count detection logic from `image::ColorType` is duplicated between `header_exr` (no-openexr path) and `header_generic`. Identical match arms counting channels from `ColorType`. Extract into `color_type_channels(ct: ColorType) -> u32`.

**frame.rs:720-746 and 763-780** — The "create green placeholder buffer" pattern (allocate zero buffer, set G=100, A=255 per pixel) is duplicated verbatim in the `Loaded→Header` and `Error→Header` transitions of `set_status`. Extract into a `make_placeholder_buffer(width, height) -> Vec<u8>` helper.

**hsv.rs:46-127** — Three per-format pixel loops in `apply` that differ only in decode/encode. The actual HSV math (`rgb_to_hsv`, adjustments, `hsv_to_rgb`) is repeated identically three times. Factor the math into an inner function and call it from a single format-dispatch wrapper.

**comp_node.rs:1271-1289 and top of `compute`** — `is_dirty` recursively checks source nodes, while `compute` also checks `is_dirty` before composing. The dirty traversal is performed at least twice per `compute` call when a dirty node is encountered. Cache the dirty result or propagate dirty upward on write.

---

## Logic Issues

**compositor.rs:163** — F32 Porter-Duff alpha compositing: `result[i+3] = bottom[i+3] * inv_alpha + top_alpha`. This is correct. However, the RGB blending at line 157 uses premultiplied form only for Normal mode: `result[i] = (bottom[i] * bottom_alpha * inv_alpha + top[i] * top_alpha) / result_alpha`. When `result_alpha` is near-zero (transparent area), this division approaches 0/0. There is no epsilon guard — if `result_alpha < epsilon`, the division should be skipped and the output forced to 0. Currently this produces NaN in transparent pixels for the F32 path. NaN propagates through subsequent blend operations silently.

**compositor.rs:157-160** — The premultiplied blend formula is only used for `BlendMode::Normal`. Other blend modes (`Screen`, `Multiply`, etc.) operate on straight alpha values and do not apply alpha compositing at the pixel level before accumulation. This means that blending a partially transparent layer with Screen mode gives incorrect results — the alpha of the top layer is not properly composited into the result alpha. The blend modes need alpha-aware compositing, not just channel math.

**hsv.rs:154** — `rgb_to_hsv` hue computation: `60.0 * (((g - b) / delta) % 6.0)`. The `%` operator on f32 is the remainder, not the mathematical modulo. For negative values this gives a negative result (e.g., `(-0.1) % 6.0 = -0.1`). The code then applies `if h < 0.0 { h + 360.0 }` at line 164, which only adds 360 once. If `(g - b) / delta` results in a value in [-6, -5), the raw hue is in [-360, -300) after multiplying by 60, and `h + 360.0` correctly wraps it. However if the f32 remainder produces an out-of-range intermediate, the final hue could still be negative or > 360. More robust: use `rem_euclid` here as is done for `h_new` in `apply`.

**global_cache.rs:384** — In `clear_comp` dehydrate mode: `let _ = frame.set_status(FrameStatus::Expired)`. The `set_status(Expired)` call falls into the catch-all `_ =>` arm of the match in `frame.rs:792-795`, which directly sets `status = new_status`. But this bypasses all the state transition guards. Frames in `Loading` state should not be set to `Expired` since a background thread holds an implicit "claim" on them. Setting them to `Expired` while a worker is writing into them creates a time-of-check/time-of-use window where the completed load sets status back to `Loaded`, then a racing dehydrate marks it `Expired` again — leaving the frame in Expired state with valid pixel data, never to be recomposed.

**workers.rs:62** — Each worker adds its own stealer to the `stealers` vec: `stealers.push(worker.stealer())`. Then each worker attempts to steal from all stealers including its own. Self-stealing from a crossbeam deque is a no-op (the deque is empty from its own perspective by the time the worker checks), but it wastes a call per idle loop. Index `i` should be excluded from the steal list for worker `i`.

**workers.rs:106** — Shutdown is checked AFTER `thread::sleep(Duration::from_millis(1))`. Every worker always sleeps 1ms before checking if it should exit. On shutdown, all N worker threads add 1ms of latency before joining. With 8 workers, `Workers::drop` takes at least 8ms. More importantly, the task injection check at line 102 (`while let Some(task) = self.injector.steal()...`) runs before the sleep, meaning the sleep is only reached when the queue is empty — but the shutdown check is after the sleep, not after the steal loop. So between "queue empty" and "sleep" a new task may arrive, be processed, and the sleep still happens.

**loader_video.rs:49** — Frame count: `(duration_secs * fps) as usize`. Floating-point multiplication of `duration_secs * fps` may produce a value like `23.9999` instead of `24` due to floating-point rounding. Truncating to usize gives one fewer frame than expected. Use `(duration_secs * fps).round() as usize` instead.

**comp_node.rs:192** — `Layer::end()`: `(src_len as f32 / speed) as i32`. If `speed` is very small (e.g., 0.01) and `src_len` is large (e.g., 86400 frames), the f32 intermediate overflows. f32 has only 24 bits of mantissa; values above ~16 million are rounded. Use integer arithmetic: `(src_len as i64 * 100 / (speed * 100.0) as i64) as i32` or work entirely in f64.

**comp_node.rs:1385** — Preload spiral: `let max_offset = radius.min(play_end - play_start)`. When `play_end - play_start` overflows (if play_end < play_start, checked on line 1326, but the subtraction itself occurs on negative i32 values), this is safe due to the earlier guard. However, `radius` is also i32 and could be negative if passed in incorrectly, making `min` return a negative value. The loop `for offset in 0..=max_offset` would then produce an empty range silently rather than an error — this is benign but the intent is obscured.

**gpu_compositor.rs:753** — `blend` clones the entire `frames` Vec before passing to `blend_impl` on line 753: `self.blend_impl(frames.clone())`. Each element is `(Frame, f32, BlendMode, [f32; 9])`. Frame is `Arc<Mutex<FrameData>>` so cloning is cheap (Arc refcount), but the Vec allocation and element copy still happen on every blend call. Pass by reference into `blend_impl` and only fall back to CPU compositor if truly needed, accepting the Vec by value only in the CPU path.

**gpu_compositor.rs:833** — In `blend_impl`, the first result texture (the bottom layer, `guard.textures[0]`) is never deleted inside the loop because the guard only deletes `result_texture` when `i > 1`. After the loop, `drop(guard)` deletes all textures. But `result_texture` at the end of the loop is `new_result` from the last iteration, which is also in `guard.textures`. The final `download_texture_to_frame` reads from `result_texture`, then `drop(guard)` deletes it. This means the GPU texture is deleted immediately after readback, which is correct, but if `download_texture_to_frame` fails and returns `Err`, the guard still drops and deletes the result — so there is no retry path available.

**frame.rs:519** — `try_claim_for_loading` is called BEFORE format detection (`ext` computation on lines 530-534). If claiming succeeds but the extension is unsupported (line 543), the frame is left in `Loading` state (set inside `try_claim_for_loading`) and the error path at line 553 sets it to `Error`. This is correct, but the comment at line 518 says "prevents duplicate loads" — it also prevents retrying load on unsupported formats without going through `set_status(Header)` first, since `try_claim_for_loading` blocks when status is `Error`.

---

## Recommendations

**Priority 1 — Correctness fixes (ship-blockers)**

1. **NaN in F32 Porter-Duff blend** (`compositor.rs:157`): Guard the RGB division by `result_alpha` with an epsilon check. When `result_alpha < 1e-6`, write `[0.0, 0.0, 0.0, 0.0]` directly.

2. **FPS denominator zero-check** (`loader_video.rs:48`): Add `if fps_rational.denominator() == 0 { return Err(...) }` before computing fps. Propagate as a proper `FrameError::InvalidFormat`.

3. **GPU compositor partial initialization** (`gpu_compositor.rs:264`): Change the init guard from `blend_program.is_some()` to a dedicated `initialized: bool` field that is only set to `true` after ALL resources (program + VAO + VBO + FBO) succeed. On partial failure, clean up any already-created resources before returning `Err`.

4. **Dehydrate vs. Loading race** (`global_cache.rs:384`): Skip `set_status(Expired)` when frame status is `Loading` — a background worker owns that frame. Only dehydrate `Loaded` frames (already done) and skip others explicitly rather than via catch-all.

5. **f32 truncation in frame count** (`loader_video.rs:49`): Change `as usize` to `.round() as usize` or use integer rounding.

6. **Unsafe raw pointer for thread count** (`loader_video.rs:97-98`): Replace with the safe API if available in the ffmpeg-rs binding (check for `set_threading`), or add a `// SAFETY:` comment documenting why the pointer is valid at this point. Evaluate against the application thread budget.

**Priority 2 — Performance (viewer responsiveness)**

7. **Eliminate full-decode in `header_exr` and `header_generic`** (`loader.rs:131, 317`): For EXR, use the `openexr` or `exr` crate header-only read path. For PNG, use `lodepng` or `png` crate chunk reader. For TIFF, read IFD entries only. The goal is metadata in <1ms instead of 100-500ms.

8. **Eliminate buffer clone in `blend_with_dim`** (`compositor.rs:327-337`): Remove the `let mut blended = curr.clone()` line and replace with a single destination buffer allocation for the entire `blend_with_dim` call.

9. **O(n) LRU update on every cache hit** (`global_cache.rs:152`): Replace `shift_remove` + `insert` with a proper O(1) LRU structure. The simplest approach is a `HashMap` + doubly-linked list (the `lru` crate). For sequential playback, consider a clock/generation eviction instead.

10. **Unify three-format pixel loops** (`compositor.rs:118-218`, `transform.rs:289-397`, `hsv.rs:46-127`, `blur.rs:62-240`): Each of these files has 3x the code it needs due to format dispatch at the wrong level. Define a `PixelAccessor` or `SampleDecode` trait and collapse to a single implementation per operation.

11. **Hoist blend mode match out of pixel loop** (`compositor.rs:apply_blend`): Evaluate `BlendMode` once per frame call, not once per pixel per channel. Use a `fn(f32, f32) -> f32` closure or enum dispatch lifted above the pixel loop.

**Priority 3 — Code quality and maintainability**

12. **Workers comment is wrong** (`workers.rs:79`): Fix the "LIFO" comment to say "FIFO" or change to `Worker::new_lifo()` if LIFO is actually desired for cache locality during preload.

13. **Workers self-steal** (`workers.rs:62`): Exclude `stealers[i]` from the steal list for worker thread `i`. This is a minor correctness/clarity fix.

14. **Dead `texture_cache`** (`gpu_compositor.rs`): Either implement the texture cache (store frequently-used textures by UUID to avoid re-upload on every frame) or remove the field and `#[allow(dead_code)]` entirely.

15. **Duplicated extension detection** (`loader.rs:31-56`): Extract `classify_path` helper used by both `header()` and `load()`.

16. **Duplicated placeholder buffer** (`frame.rs:720-746, 763-780`): Extract `make_placeholder_buffer(w, h) -> Vec<u8>` used in two `set_status` arms.

17. **`blend_with_dim` GPU stub** (`gpu_compositor.rs:851-862`): The TODO comment "Implement proper canvas-sized blending" has been there since the GPU compositor was written. The current implementation calls `blend()` which uses first-frame dimensions, then crops — giving wrong results when the canvas is larger than the input frames. Either implement proper canvas-sized blending or document clearly that GPU path does not support canvas resize and always falls back to CPU.

18. **`HSV_ATTRS` default `value` = 2.0** (`effects/mod.rs:146`): The schema default for the `value` attribute is 2.0 (double brightness). The neutral default should be 1.0 (no change). This means any newly added HSV effect silently doubles the brightness until the user notices.
