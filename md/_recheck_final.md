# Bug Hunt Cross-Verification Report

Date: 2026-03-18
Verifier: Claude Opus 4.6 (independent cross-check)

---

## 1. BUG-02: API Play double-emit

**CONFIRMED -- actually WORSE than reported**

File: `src/app/api.rs` lines 90-94
```rust
ApiCommand::Play => {
    self.event_bus.emit(TogglePlayPauseEvent);        // line 91: UNCONDITIONAL
    if !self.player.is_playing() {                     // line 92
        self.event_bus.emit(TogglePlayPauseEvent);     // line 93: second emit
    }
}
```

The report said "double-emit." The reality is worse:

- `emit()` (event_bus.rs:109-133) queues events for deferred processing. The TogglePlayPauseEvent handler (main_events.rs:226-237) runs later when events are polled, NOT synchronously during emit.
- Therefore `self.player.is_playing()` at line 92 reflects the state BEFORE the first event is processed.
- **When player is STOPPED**: line 91 queues toggle #1, line 92 sees `!is_playing() == true`, line 93 queues toggle #2. Two toggles cancel each other out. **The Play command does NOTHING when the player is stopped** -- the exact scenario where it should work.
- **When player is ALREADY playing**: line 91 queues toggle #1 (will pause), line 92 sees `!is_playing() == false`, skips. Net: one toggle = pauses the player. **The Play command PAUSES when already playing** -- the opposite of what it should do.

The logic is completely inverted. This is a P0 bug for anyone using the API.

---

## 2. ENC-01: FPS truncation in encoder

**CONFIRMED**

File: `src/dialogs/encode/encode.rs` line 1252
```rust
let fps_num = settings.fps as i32;                                           // line 1252
encoder.set_frame_rate(Some(ffmpeg::util::rational::Rational::new(fps_num, 1))); // line 1253
encoder.set_time_base(ffmpeg::util::rational::Rational::new(1, fps_num));        // line 1254
```

`settings.fps` is `f32` (defined at line 38: `pub fps: f32`). Cast `as i32` truncates:
- 23.976 fps -> 23 (NTSC film)
- 29.97 fps -> 29 (NTSC video)
- 59.94 fps -> 59 (NTSC HD)

The Rational type supports fractional rates (e.g., `Rational::new(24000, 1001)` for 23.976). The truncation loses this entirely. GOP size at line 1258 is also affected: `(fps_num * 10).max(1)`.

The slider at encode_ui.rs:414 allows any value from 1.0 to 960.0 in f32, including non-integer rates.

---

## 3. SEC-01: Server binds 0.0.0.0

**CONFIRMED**

File: `src/server/api.rs` line 201
```rust
let addr = format!("0.0.0.0:{}", self.port);
```

Binds to all network interfaces. On a corporate/public network, the API server is exposed to anyone who can reach the machine. No authentication visible in the handler code. Should be `127.0.0.1` for local-only access.

---

## 4. PERF-01: blend_with_dim clones per layer

**CONFIRMED**

File: `src/entities/compositor.rs` lines 326-339
```rust
(PixelBuffer::F32(curr), PixelBuffer::F32(layer)) => {
    let mut blended = curr.clone();     // line 327: FULL BUFFER CLONE
    blend_rows!(blend_f32, curr, layer, blended);
    result = Frame::from_f32_buffer_with_status(blended, width, height, min_status);
}
(PixelBuffer::F16(curr), PixelBuffer::F16(layer)) => {
    let mut blended = curr.clone();     // line 332: FULL BUFFER CLONE
    ...
}
(PixelBuffer::U8(curr), PixelBuffer::U8(layer)) => {
    let mut blended = curr.clone();     // line 337: FULL BUFFER CLONE
    ...
}
```

This is inside a `for` loop iterating over layers (line 346 closes it). Each layer compositing step clones the entire result buffer. For a 4K F32 image: 3840 * 2160 * 4 * 4 bytes = ~126 MB per clone. With N layers, that's N clones.

The blend operation writes into `blended` using `out_slice` (blend_rows macro), so theoretically the clone could be replaced with an allocation + direct write, or the result buffer could be reused with a swap pattern. The current `curr` comes from `result_buffer` which is re-created each iteration via `Frame::from_*_buffer_with_status`.

---

## 5. NEW-01: transform per-pixel invariant (tilt check)

**NUANCE -- technically correct but likely optimized away**

File: `src/entities/transform.rs` lines 489-536 (closure), called at lines 554, 580 (per-pixel)

The `transform_point` closure is defined at line 489 and called per pixel inside `par_chunks_mut` + `for x` loops. Inside the closure:
```rust
let layer_is_tilted = (plane_normal - Vec3::Z).length_squared() > 1e-6;  // line 494 and 512
```

`plane_normal` is captured from the outer scope (computed once at line 470). The subtraction, length_squared, and comparison are all on constant values within the closure's lifetime. The result is the same for every pixel.

**However**: this closure runs in a rayon `par_chunks_mut` context. LLVM can likely hoist this computation since `plane_normal` is a captured immutable reference and `Vec3::Z` is a constant. The branch predictor will also learn the pattern after the first call.

Verdict: Correct observation, but **LOW impact**. A few float ops per pixel is negligible compared to the matrix multiplications and texture sampling that follow. Hoisting `layer_is_tilted` before the closure would be cleaner but is not a performance bottleneck.

---

## 6. NEW-03: renderer F16 .to_vec() unnecessary

**CHALLENGED -- the .to_vec() is necessary**

File: `src/widgets/viewport/renderer.rs` lines 314-328
```rust
PixelBuffer::F16(_) => {
    // Convert F16 -> u16 -> bytes (reuse scratch buffer)
    if let PixelBuffer::F16(src) = pixel_buffer {
        self.f16_scratch.clear();
        self.f16_scratch.extend(src.iter().map(|f| f.to_bits()));
    }
    let bytes_u8: Vec<u8> =
        bytemuck::cast_slice(self.f16_scratch.as_slice()).to_vec();  // line 320-321
    owned_bytes = Some(bytes_u8);
    (
        owned_bytes.as_ref().unwrap().as_slice(),  // line 324: borrows owned_bytes
        glow::RGBA16F as i32,
        glow::RGBA,
        glow::HALF_FLOAT,
    )
}
```

The comment at line 304 explains: "For F16 we create an owned byte buffer to avoid borrowing self across calls." The problem is:
- `bytemuck::cast_slice(self.f16_scratch.as_slice())` borrows `self.f16_scratch` (and therefore `self`)
- The returned `pixels_bytes` must outlive the match, but `self` methods are called later in the same `unsafe` block (texture upload at line 336+)
- The `.to_vec()` breaks the borrow on `self` by creating an owned copy

This IS a real allocation, but it's **architecturally necessary** given the current borrow structure. Eliminating it would require restructuring the F16 path to separate the conversion from the upload, or using raw pointers.

---

## 7. ENC-04: UI freeze on encode stop

**CONFIRMED**

File: `src/dialogs/encode/encode_ui.rs` lines 785-826
```rust
fn stop_encoding_internal(&mut self) {               // line 785
    self.cancel_flag.store(true, Ordering::Relaxed);  // line 786
    ...
    if let Some(handle) = self.encode_thread.take() { // line 792
        let timeout = Duration::from_secs(2);          // line 796
        let start = Instant::now();                    // line 797
        loop {                                         // line 799
            if handle.is_finished() { ... break; }     // line 800
            if start.elapsed() > timeout {             // line 811
                self.orphan_handles.push(handle);      // line 814
                break;                                 // line 815
            }
            std::thread::sleep(Duration::from_millis(100)); // line 818: BLOCKS UI
        }
    }
    ...
}
```

Called from `stop_encoding()` (line 322) which is called from UI button handlers (lines 674, 683). This is a synchronous sleep loop on the UI thread, polling every 100ms for up to 2 seconds. During this time the UI is completely frozen.

The orphan handle pattern (line 814) shows awareness of the issue -- if the thread doesn't finish in 2s, it's stored for later cleanup. But the initial 2-second blocking window is real.

**Additional concern**: `stop_encoding_and_close()` at line 773 and `stop_encoding_keep_window()` at line 779 both call `stop_encoding_internal()`, so ANY stop path freezes the UI.

---

## 8. BUG-08: HSV default value = 2.0

**CHALLENGED -- report misread the function signature**

File: `src/entities/effects/mod.rs` line 146
```rust
AttrDef::with_ui_order("value", AttrType::Float, FX, &["0", "2", "0.01"], 2.0),
```

The `with_ui_order` signature (attrs.rs:117-124):
```rust
pub const fn with_ui_order(
    name: &'static str,
    attr_type: AttrType,
    flags: AttrFlags,
    ui_options: &'static [&'static str],  // <- &["0", "2", "0.01"] = slider min/max/step
    order: f32,                            // <- 2.0 = DISPLAY ORDER, not default value
) -> Self
```

The `2.0` is the **display order** in the Attribute Editor (lower = higher in list). The three HSV attrs have orders 0.0, 1.0, 2.0 to sort them hue/sat/value.

The **actual default** is set in `Effect::new()` at line 202:
```rust
EffectType::AdjustHSV => {
    attrs.set("hue_shift", AttrValue::Float(0.0));
    attrs.set("saturation", AttrValue::Float(1.0));
    attrs.set("value", AttrValue::Float(1.0));        // line 202: default is 1.0
}
```

And the fallback in the effect implementation (hsv.rs:36):
```rust
let value = attrs.get_float("value").unwrap_or(1.0);  // fallback is also 1.0
```

**There is no bug here.** The default value for HSV "value" is correctly 1.0 (no change).

---

## 9. PERF-04: LRU O(n) shift_remove

**CONFIRMED -- acknowledged in code comments**

File: `src/core/global_cache.rs` lines 147-153
```rust
// Update LRU order: move to back (most recently used) - O(1) with IndexSet  // line 147 (MISLEADING)
let key = CacheKey { comp_uuid, frame_idx };
let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());
// shift_remove is O(n) but we need it for LRU ordering; swap_remove would be O(1) but breaks order
// Alternative: use move_index but IndexSet doesn't have it - just re-insert
lru.shift_remove(&key);                                                     // line 152: O(n)
lru.insert(key);                                                             // line 153: O(1)
```

The comment at line 147 says "O(1) with IndexSet" but the very next comment at line 150 contradicts this: "shift_remove is O(n)". This runs on EVERY cache hit (line 145: `if result.is_some()`).

Additional O(n) usage at:
- Line 238: cache insertion path (shift_remove existing before re-insert)
- Line 283: eviction path (`shift_remove_index(0)` is O(n) for index 0)
- Line 327: invalidation path

For a cache with thousands of entries (typical for frame sequences), each cache hit triggers an O(n) shift on the IndexSet's internal Vec. Consider `indexmap::IndexMap::move_index` (available since indexmap 2.0) or a different LRU structure (linked list, doubly-linked intrusive list).

Note: the comment at line 88 claims "O(1) LRU eviction via IndexSet" which is incorrect.

---

## 10. BUG-06: deferred_load_sequences overwrite

**CONFIRMED**

File: `src/app/events.rs` lines 29, 165-166, 243-244
```rust
let mut deferred_load_sequences: Option<Vec<std::path::PathBuf>> = None;     // line 29

// Inside the event processing loop:
if let Some(paths) = result.load_sequences {
    deferred_load_sequences = Some(paths);                                    // line 166: OVERWRITES
}

// After the loop:
if let Some(paths) = deferred_load_sequences {
    let _ = self.load_sequences(paths);                                       // line 243-244
}
```

This is `= Some(paths)`, not extend. If multiple events in the same frame produce `load_sequences`, only the **last** one survives. The same pattern applies to ALL deferred actions in this block:
- `deferred_load_project` (line 160)
- `deferred_save_project` (line 163)
- `deferred_load_sequences` (line 166)
- `deferred_new_comp` (line 169)
- `deferred_new_camera` (line 172)
- `deferred_new_text` (line 175)

For most of these, having only one per frame may be intentional (you can't load two projects at once). But for `load_sequences`, dropping paths silently is a data-loss bug -- the user drops 3 files, only the last batch fires.

**Practical impact**: depends on whether multiple `load_sequences` events can actually fire in a single frame. If events come from drag-and-drop, the OS typically batches them into one event with all paths in a Vec, so the overwrite may not trigger in practice. But the API at `api.rs:113` fires `LoadSequence` per path, so rapid API calls could trigger this.

---

## Summary Table

| # | Finding | Verdict | Severity | Notes |
|---|---------|---------|----------|-------|
| 1 | BUG-02: API Play double-emit | **CONFIRMED+** | P0 | Worse than reported: Play command is completely broken |
| 2 | ENC-01: FPS truncation | **CONFIRMED** | P1 | 23.976, 29.97 fps impossible |
| 3 | SEC-01: 0.0.0.0 bind | **CONFIRMED** | P2 | No auth on API server |
| 4 | PERF-01: clone per layer | **CONFIRMED** | P2 | ~126MB clone per layer at 4K F32 |
| 5 | NEW-01: tilt check per pixel | **NUANCE** | P3 | True but LLVM likely hoists; negligible cost |
| 6 | NEW-03: F16 .to_vec() | **CHALLENGED** | -- | Necessary for borrow checker; not a bug |
| 7 | ENC-04: UI freeze on stop | **CONFIRMED** | P1 | Up to 2s hard freeze on UI thread |
| 8 | BUG-08: HSV default = 2.0 | **CHALLENGED** | -- | Report misread `order` param as default; actual default is 1.0 |
| 9 | PERF-04: LRU O(n) | **CONFIRMED** | P2 | Every cache hit is O(n); comments self-contradictory |
| 10 | BUG-06: deferred overwrite | **CONFIRMED** | P2 | Silent data loss on multi-event frames |

**Score: 7 CONFIRMED, 1 NUANCE, 2 CHALLENGED**
