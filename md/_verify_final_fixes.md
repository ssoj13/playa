# Final Fixes Verification Report

Date: 2026-03-18
Reviewer: Claude Opus 4.6 (code review, no build)

---

## 1. PERF-04: LRU cache now uses lru::LruCache

**File:** `src/core/global_cache.rs`

| Check | Status | Details |
|-------|--------|---------|
| `use lru::LruCache` (not indexmap) | PASS | Line 16: `use lru::LruCache;` |
| Field type `Arc<Mutex<LruCache<CacheKey, ()>>>` | PASS | Line 95: `lru_order: Arc<Mutex<LruCache<CacheKey, ()>>>` |
| Constructor uses `LruCache::unbounded()` | PASS | Line 124: `LruCache::unbounded()` |
| Cache hit uses `lru.get(&key)` (O(1) promote) | PASS | Line 151: `lru.get(&key);` |
| Insert uses `lru.put(key, ())` | PASS | Lines 197, 245: `lru.put(CacheKey { ... }, ());` |
| Eviction uses `lru.pop_lru()` | PASS | Line 281: `let key = match lru.pop_lru() {` |
| No `shift_remove` calls remain | PASS | grep returns 0 matches |
| No `.retain()` calls remain | PASS | grep returns 0 matches; replaced with collect+pop pattern (lines 409-416, 464-471) |
| CacheKey implements Hash + Eq | PASS | Line 70: `#[derive(Clone, Copy, PartialEq, Eq, Debug)]` + line 76: `impl Hash for CacheKey` |

**Verdict: PASS**

---

## 2. idx_to_uuid fix -- no more unwrap_or_default on UUID lookups

**File:** `src/main_events.rs`

| Check | Status | Details |
|-------|--------|---------|
| `unwrap_or_default` count | **PARTIAL** | 2 matches remain (lines 1022, 1203) -- see analysis below |
| `idx_to_uuid` uses `if let Some(uuid)` | PASS | All 4 occurrences (lines 814, 840, 856, 874) use `if let Some(...)` |
| MoveAndReorderLayerEvent | PASS | Line 814: `if let Some(dragged_uuid) = comp.idx_to_uuid(e.layer_idx)` |
| SetLayerPlayStartEvent | PASS | Line 840: `if let Some(dragged_uuid) = comp.idx_to_uuid(e.layer_idx)` |
| SetLayerPlayEndEvent | PASS | Line 856: `if let Some(dragged_uuid) = comp.idx_to_uuid(e.layer_idx)` |

**Remaining `unwrap_or_default` analysis:**

- **Line 1022:** `project.with_comp(e.comp_uuid, |comp| { ... }).unwrap_or_default();`
  This is on `Option<Vec<...>>` -- returns empty Vec if comp not found. **NOT related to idx_to_uuid.** This is safe and idiomatic for `Option<Vec<T>>`.

- **Line 1203:** `comp.attrs.get_map("bookmarks").cloned().unwrap_or_default();`
  This is on `Option<HashMap<...>>` -- returns empty HashMap if no bookmarks key. **NOT related to idx_to_uuid.** This is safe and idiomatic for `Option<HashMap<K,V>>`.

**Verdict: PASS** -- The idx_to_uuid fix is correct. The two remaining `unwrap_or_default` calls are on collection types (Vec, HashMap), not on UUID lookups, and are safe/idiomatic.

---

## 3. BUG-05: GPU compositor partial init fixed

**File:** `src/entities/gpu_compositor.rs`

| Check | Status | Details |
|-------|--------|---------|
| `cleanup_gl_resources()` method exists | PASS | Line 265: `fn cleanup_gl_resources(&mut self)` |
| Cleans up all 4 resources | PASS | Lines 267-278: program.take(), vao.take(), vbo.take(), fbo.take() with proper GL delete calls |
| `ensure_initialized()` calls cleanup before re-init | PASS | Line 290: `self.cleanup_gl_resources();` |
| Guard checks all 4 resources | PASS | Line 285: `self.blend_program.is_some() && self.vao.is_some() && self.vbo.is_some() && self.fbo.is_some()` |
| Returns Ok(()) only if ALL 4 present | PASS | Line 286: early return only when all 4 are Some |
| Partial state triggers cleanup + retry | PASS | If any resource is None (partial), falls through to cleanup + full re-init |

**Verdict: PASS**

---

## 4. DUP-05: handle_media_removal helper

**File:** `src/main_events.rs`

| Check | Status | Details |
|-------|--------|---------|
| Helper function exists | PASS | Line 80: `fn handle_media_removal(removed_uuids: &[Uuid], project, player, node_editor_state)` |
| RemoveMediaEvent calls it | PASS | Line 476: `handle_media_removal(&[e.0], project, player, node_editor_state);` |
| RemoveSelectedMediaEvent calls it | PASS | Line 481: `handle_media_removal(&selection, project, player, node_editor_state);` |
| Logic correct (del comps, reset active) | PASS | Lines 86-101: iterates removed_uuids, deletes comps, picks new active if needed |

**Verdict: PASS**

---

## 5. DUP-06: align_layers_to_frame helper

**File:** `src/main_events.rs`

| Check | Status | Details |
|-------|--------|---------|
| Helper function exists | PASS | Line 106: `fn align_layers_to_frame(comp: &mut Comp, use_start: bool)` |
| AlignLayersStartEvent calls it | PASS | Line 967: `align_layers_to_frame(comp, true)` |
| AlignLayersEndEvent calls it | PASS | Line 971: `align_layers_to_frame(comp, false)` |
| Correct logic (iterates selected layers, computes delta) | PASS | Lines 107-118: gets current_frame, iterates selected, computes bound based on use_start, applies delta |

**Verdict: PASS**

---

## 6. Compositor into_pixel_buffer safe

### Frame::into_pixel_buffer

**File:** `src/entities/frame.rs`

| Check | Status | Details |
|-------|--------|---------|
| Uses `match Arc::try_unwrap` | PASS | Line 809: `match Arc::try_unwrap(self.data)` + Line 812: `match Arc::try_unwrap(data.buffer)` |
| Clone fallback (no panic) | PASS | Line 814: `Err(arc) => (*arc).clone()` and Line 819: `(*data.buffer).clone()` |
| No `.expect()` | PASS | No `.expect()` in the function body |
| No bare `.unwrap()` | PASS | Uses `unwrap_or_else(|e| e.into_inner())` for poison recovery on Mutex (lines 811, 818) -- safe |

**Verdict: PASS**

### CpuCompositor::blend_with_dim

**File:** `src/entities/compositor.rs`

| Check | Status | Details |
|-------|--------|---------|
| Uses `crop_copy` | PASS | Line 290: `base_frame.crop_copy(width, height, CropAlign::LeftTop)` |
| Uses `into_pixel_buffer` | PASS | Line 304: `base_cropped.into_pixel_buffer()` |
| Double-buffer (ping-pong) swap pattern | PASS | Lines 296-313: `Buf` enum with curr/out, lines 349-361: `std::mem::swap(c, o)` in each blend iteration |
| No panic-inducing calls in blend loop | PASS | All buffer access is through sliced ranges with computed overlap dimensions |

**Verdict: PASS**

---

## Summary

| Fix | Status |
|-----|--------|
| PERF-04: LRU cache migration | **PASS** |
| idx_to_uuid unwrap fix | **PASS** |
| BUG-05: GPU partial init guard | **PASS** |
| DUP-05: handle_media_removal | **PASS** |
| DUP-06: align_layers_to_frame | **PASS** |
| into_pixel_buffer safety | **PASS** |

**All 6 fixes verified correct via static code analysis.**
