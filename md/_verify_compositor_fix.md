# PERF-01 Verification: Compositor blend_with_dim buffer reuse

**Date:** 2026-03-18
**Reviewer:** Claude Opus 4.6 (analytical verification, no build)
**Files reviewed:**
- `src/entities/frame.rs` (lines 42-46, 110-127, 267-330, 800-815, 896-1008)
- `src/entities/compositor.rs` (full file, 380 lines)

---

## Check 1: frame.rs -- into_pixel_buffer method

**Location:** `frame.rs:809-815`

```rust
pub(crate) fn into_pixel_buffer(self) -> PixelBuffer {
    let data_arc = Arc::try_unwrap(self.data)
        .expect("into_pixel_buffer: Frame Arc is shared");
    let data = data_arc.into_inner().expect("into_pixel_buffer: Mutex poisoned");
    Arc::try_unwrap(data.buffer)
        .expect("into_pixel_buffer: PixelBuffer Arc is shared")
}
```

### Verdict: CRITICAL BUG -- Potential panic

| Criterion | Status | Notes |
|-----------|--------|-------|
| Consumes self (takes ownership) | PASS | `fn into_pixel_buffer(self)` -- moves Frame |
| Arc::try_unwrap handles Ok case | PASS | Unwraps the inner value |
| Arc::try_unwrap handles Err case | **FAIL** | Uses `.expect()` -- panics on Err |
| Returns PixelBuffer | PASS | Correct return type |
| No panics possible | **FAIL** | Two `.expect()` calls can panic |

**Root cause:** The calling code in `compositor.rs:288-306` does:

```rust
let mut iter = frames.iter();                              // borrows frames
let (base_frame, _, _, _) = iter.next().unwrap();          // &Frame into vec
let mut result = base_frame.clone();                       // Arc::clone -- refcount=2
result.crop(width, height, CropAlign::LeftTop);            // mutates through Mutex
let mut curr = match result.into_pixel_buffer() { ... };   // tries try_unwrap
```

`Frame` uses `#[derive(Clone)]` (line 123), which generates `Arc::clone` for
`data: Arc<Mutex<FrameData>>`. After clone, both `result.data` and `base_frame.data`
(still alive in the `frames` vec) point to the same Arc with **refcount = 2**.

`crop()` takes `&self` and writes through the Mutex; it does NOT create a new
`Arc<Mutex<FrameData>>`, only replaces the inner `data.buffer`.

When `result.into_pixel_buffer(self)` executes `Arc::try_unwrap(self.data)`:
- refcount = 2 (result + base_frame in frames vec)
- `try_unwrap` returns `Err`
- `.expect()` **panics**

The comment on line 295 ("succeeds because result is the sole Arc owner") is **incorrect**.

**Suggested fix:** Replace `.expect()` with fallback to deep-clone:

```rust
pub(crate) fn into_pixel_buffer(self) -> PixelBuffer {
    let data = match Arc::try_unwrap(self.data) {
        Ok(mutex) => mutex.into_inner().expect("into_pixel_buffer: Mutex poisoned"),
        Err(arc) => arc.lock().unwrap().clone(),
    };
    match Arc::try_unwrap(data.buffer) {
        Ok(buf) => buf,
        Err(arc) => arc.as_ref().clone(),
    }
}
```

Or alternatively, restructure compositor to use `into_iter()` so the base frame
is consumed (not borrowed), making it the sole Arc owner.

---

## Check 2: compositor.rs -- blend_with_dim double-buffer pattern

**Location:** `compositor.rs:252-379`

### 2a. Initial buffer extraction

```rust
let mut curr = match result.into_pixel_buffer() {   // line 306 -- ONCE
    PixelBuffer::F32(v) => Buf::F32(v),
    PixelBuffer::F16(v) => Buf::F16(v),
    PixelBuffer::U8(v)  => Buf::U8(v),
};
```

| Criterion | Status |
|-----------|--------|
| `into_pixel_buffer()` called once | PASS |
| All three pixel formats handled | PASS |

### 2b. Output buffer allocation

```rust
let mut out = match &curr {                          // lines 311-315 -- ONCE
    Buf::F32(_) => Buf::F32(vec![0.0f32; canvas_pixels]),
    Buf::F16(_) => Buf::F16(vec![half::f16::ZERO; canvas_pixels]),
    Buf::U8(_)  => Buf::U8(vec![0u8; canvas_pixels]),
};
```

| Criterion | Status |
|-----------|--------|
| ONE allocation before loop | PASS |
| Sized to canvas_pixels (w*h*4) | PASS |
| Matches pixel format of curr | PASS |

### 2c. Loop body: copy + blend + swap

```rust
// Example for F32 arm (lines 350-354):
o.copy_from_slice(c);                   // 1. copy curr -> out (full canvas)
blend_rows!(blend_f32, c, layer, o);    // 2. blend overlap region into out
std::mem::swap(c, o);                   // 3. swap: curr = blended, out = old
```

| Criterion | Status | Notes |
|-----------|--------|-------|
| copy_from_slice (no allocation) | PASS | memcpy, reuses existing Vec |
| blend_rows reads from `c` (bottom) | PASS | `base_slice = &$curr[...]` |
| blend_rows reads from `layer` (top) | PASS | `layer_slice = &$layer[...]` |
| blend_rows writes to `o` (output) | PASS | `out_slice = &mut $out[...]` |
| swap (no allocation) | PASS | `std::mem::swap` is O(1) pointer swap |
| No clone in loop | PASS | Zero heap allocations per iteration |
| No aliasing between c and o | PASS | Separate Vecs, disjoint memory |
| F32/F16/U8 arms all correct | PASS | Identical pattern in all three |
| Format mismatch handled | PASS | Wildcard arm logs warning, skips layer |

### 2d. blend_rows! macro correctness

```rust
macro_rules! blend_rows {
    ($blend_fn:ident, $curr:expr, $layer:expr, $out:expr) => {{
        let base_stride = width * 4;
        let layer_stride = lw * 4;
        for y in 0..overlap_h {
            let b_off = y * base_stride;
            let l_off = y * layer_stride;
            let base_slice = &$curr[b_off..b_off + overlap_w * 4];
            let layer_slice = &$layer[l_off..l_off + overlap_w * 4];
            let out_slice = &mut $out[b_off..b_off + overlap_w * 4];
            Self::$blend_fn(base_slice, layer_slice, *opacity, mode, out_slice);
        }
    }};
}
```

| Criterion | Status | Notes |
|-----------|--------|-------|
| bottom = curr (read-only) | PASS | `&$curr[...]` immutable borrow |
| top = layer (read-only) | PASS | `&$layer[...]` immutable borrow |
| out = out (write target) | PASS | `&mut $out[...]` mutable borrow |
| Stride calculation correct | PASS | base uses canvas width, layer uses lw |
| Row offsets correct | PASS | b_off uses base_stride, l_off uses layer_stride |
| Only overlap region blended | PASS | `overlap_w * 4` elements per row |

### 2e. Post-loop frame creation

```rust
let result = match curr {
    Buf::F32(v) => Frame::from_f32_buffer_with_status(v, width, height, min_status),
    Buf::F16(v) => Frame::from_f16_buffer_with_status(v, width, height, min_status),
    Buf::U8(v)  => Frame::from_u8_buffer_with_status(v, width, height, min_status),
};
```

| Criterion | Status | Notes |
|-----------|--------|-------|
| Frame created from `curr` (last blend result) | PASS | `curr` holds accumulated result after all swaps |
| min_status propagated | PASS | Computed on line 272-282 from all input frames |
| All three formats handled | PASS | |

### 2f. Edge cases

| Edge case | Status | Notes |
|-----------|--------|-------|
| Single layer (no loop iterations) | PASS | curr = cropped base, returned directly |
| Empty frames | PASS | Early return `None` on line 265-268 |
| Zero overlap (layer fully outside canvas) | PASS | `continue` on line 329 |
| Format mismatch between layers | PASS | Wildcard arm skips with warning |

---

## Check 3: Behavior preservation (rendering result identical)

### Trace through 3-layer example (A=base, B, C)

**After init:**
- `curr` = A pixels (cropped to canvas)
- `out` = zeroed buffer

**Iteration 1 (B):**
1. `out = copy(curr)` -- out now contains A
2. `blend_rows` -- overwrites overlap region of out with blend(A, B)
3. `swap` -- curr = {blend(A,B) in overlap, A elsewhere}, out = old A

**Iteration 2 (C):**
1. `out = copy(curr)` -- out now contains blend(A,B)
2. `blend_rows` -- overwrites overlap region of out with blend(blend(A,B), C)
3. `swap` -- curr = final result

**Final:** Frame from curr = blend(blend(A, B), C) -- correct bottom-to-top compositing.

### Correctness argument

The blend functions read `bottom` from `c` and write result to `o`. After
`copy_from_slice`, `o` and `c` contain identical data. Blend only writes to the
overlap region of `o`. Non-overlap pixels retain `c`'s values (from the copy).
After swap, `curr` contains the fully correct accumulated buffer.

This is **mathematically identical** to the old pattern of cloning per iteration:
the only difference is that the clone allocation is replaced by a memcpy into a
pre-existing buffer.

| Criterion | Status |
|-----------|--------|
| blend_rows writes blended output into `out` | PASS |
| swap makes `curr` = blended result for next iteration | PASS |
| Final frame uses last `curr` (final blend result) | PASS |
| Non-overlap pixels preserved correctly | PASS |
| No aliasing between read (curr) and write (out) | PASS |

---

## Overall Assessment

| Check | Verdict |
|-------|---------|
| Check 1: into_pixel_buffer | **FAIL** -- will panic due to shared Arc (refcount=2) |
| Check 2: blend_with_dim double-buffer | **PASS** -- correct pattern, zero in-loop allocations |
| Check 3: Behavior preservation | **PASS** -- rendering result is identical |

### Summary

The double-buffer swap optimization in `blend_with_dim` is **correctly implemented**
and produces identical rendering output with zero per-layer heap allocations. The
`blend_rows!` macro passes correct arguments, there are no aliasing issues, and all
edge cases are handled.

**However, there is a critical bug in `into_pixel_buffer`**: it uses `.expect()` on
`Arc::try_unwrap` but the Frame's outer `Arc<Mutex<FrameData>>` has refcount 2
(shared between `result` and the original in `frames` vec). This will **panic at
runtime** on line 811.

### Recommended fix (pick one):

**Option A** -- Make `into_pixel_buffer` fallback-safe (minimal change):
```rust
pub(crate) fn into_pixel_buffer(self) -> PixelBuffer {
    let data = match Arc::try_unwrap(self.data) {
        Ok(mutex) => mutex.into_inner().expect("Mutex poisoned"),
        Err(arc) => {
            let guard = arc.lock().unwrap();
            FrameData { buffer: Arc::clone(&guard.buffer), ..guard.clone() }
            // or just: guard.clone()
        }
    };
    match Arc::try_unwrap(data.buffer) {
        Ok(buf) => buf,
        Err(arc) => arc.as_ref().clone(),
    }
}
```

**Option B** -- Fix compositor to use `into_iter()` (zero-cost, no fallback needed):
```rust
let mut iter = frames.into_iter();   // consume the vec
let (base_frame, _, _, _) = iter.next().unwrap();
let mut result = base_frame;         // already owned, no clone needed
result.crop(width, height, CropAlign::LeftTop);
// Now result.data Arc refcount = 1 (base_frame was moved out of vec)
```
This would require changing `frames` from `iter()` to `into_iter()`, and the
subsequent `for` loop already destructures by reference so it would need adjustment
to take owned values. Layer frames are only borrowed (via `buffer()`), so they could
still be consumed without issue.

Option B is preferred as it eliminates both the clone AND the panic risk, and it
avoids holding the entire `frames` vec alive during blending.
