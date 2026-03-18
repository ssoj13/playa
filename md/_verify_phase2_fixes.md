# Phase 2 Fixes Verification Report

Date: 2026-03-18

---

## 1. DEAD-07: `_saved_spacing` removed

**File:** `src/widgets/timeline/timeline_ui.rs`

**Verification:**
- Grep for `_saved_spacing` returns 0 matches. The dead variable is fully removed.
- `ui.spacing_mut().item_spacing.y = 0.0;` still present at lines 309 and 623 (the intentional override remains).

**Result: PASS**

---

## 2. NEW-11: `format!` replaced with `egui::Id`

**File:** `src/widgets/timeline/timeline_ui.rs`

**Verification:**
- Line 418: `egui::ComboBox::from_id_salt(egui::Id::new("blend_outline").with(child_uuid))`
- Uses `egui::Id::new().with()` instead of `format!("blend_outline_{}", ...)`.
- `from_id_salt` accepts `impl Hash`, and `egui::Id` implements `Hash`, so this compiles correctly.

**Result: PASS**

---

## 3. ARCH-05: Workers comment fixed

**File:** `src/core/workers.rs`

**Verification:**
- Line 61: `Worker::new_fifo()` -- creates a FIFO worker.
- Line 79: Comment reads `// 1. Try own queue first (FIFO: older tasks execute first)`.
- Comment correctly says FIFO, matching the `new_fifo()` call. No stale LIFO references found.

**Result: PASS**

---

## 4. Global cache O(n) comments fixed

**File:** `src/core/global_cache.rs`

**Verification:**
- Line 8-9: Module doc correctly says `O(1) clear_comp()` and `O(1) lookup` (these are HashMap operations, correct).
- Line 86-88: Struct doc correctly says `O(1) clear_comp()`, `O(1) lookup`, `O(n) LRU eviction via IndexSet (shift_remove ...)`.
- Line 94: Field comment says `shift_remove is O(n)` -- correct.
- Line 147: `O(n) due to shift_remove` -- correct.
- Line 150: `shift_remove is O(n) but we need it for LRU ordering; swap_remove would be O(1) but breaks order` -- correct.
- Line 236: `O(1) hash lookup + O(n) shift` -- correct, accurately describes both phases.
- **Line 325: `O(1) lookup via hash` on a `shift_remove` call -- INACCURATE.** The `shift_remove` on line 327 is O(n), not O(1). The comment only mentions the lookup cost but omits the shift cost, which is misleading. Should say `O(1) hash lookup + O(n) shift` like line 236 does.

**Result: PARTIAL PASS** -- One remaining inaccurate comment at line 325.

---

## 5. ENC-01: FPS rational conversion

**File:** `src/dialogs/encode/encode.rs`

**Verification:**
- `fps_to_rational` function exists at line 1099.
- Handles NTSC rates: 23.976 (24000/1001), 29.97 (30000/1001), 47.952, 59.94 (60000/1001), 119.88 (120000/1001). Tolerance: 0.01.
- Falls back to integer rationals for whole fps values, then 1000x scale for arbitrary rates.
- Line 1275-1277: `set_frame_rate` uses `Rational::new(fps_num, fps_den)` -- correct rational, not `fps as i32`.
- Line 1277: `set_time_base` uses `Rational::new(fps_den, fps_num)` -- correctly inverted.
- Line 1281: GOP size uses `(settings.fps.round() as i32 * 10).max(1)` -- 10-second keyframe interval, clamped to minimum 1. Reasonable.

**Result: PASS**

---

## 6. ENC-04: UI freeze removed

**File:** `src/dialogs/encode/encode_ui.rs`

**Verification:**
- `stop_encoding_internal()` at line 785:
  - Sets `cancel_flag` (line 786).
  - Calls `cleanup_orphan_handles()` to reap previously finished orphans (line 789).
  - Pushes current handle to `orphan_handles` (line 794-795) -- no blocking join.
  - Resets state: `reset_encoding_state()`, clears progress, creates fresh cancel_flag (lines 799-801).
  - **No `thread::sleep` calls anywhere in the file** (grep returns 0 matches).
  - **No polling loop** -- the function is entirely non-blocking.

- `cleanup_orphan_handles()` at line 805:
  - Uses `retain()` with `is_finished()` to reap only completed threads.
  - Non-blocking: never calls `.join()`.

- **Leak concern:** `cleanup_orphan_handles()` is only called from within `stop_encoding_internal()` (line 789), NOT from the render loop. The comment on line 793 says "will reap it on the next UI tick" but this is inaccurate -- orphans are only reaped on the next stop/cancel operation.
  - **Mitigation:** The `Drop` implementation (line 1289-1296) joins all orphan handles when the dialog is destroyed, so there is no permanent thread leak.
  - **Minor issue:** If the user starts/stops encoding many times without closing the dialog, finished threads accumulate until the next stop or dialog close. Not a practical problem (encode operations are infrequent), but the comment is misleading.

**Result: PASS** (with minor note about orphan cleanup timing)

---

## Summary

| Fix | Status | Notes |
|-----|--------|-------|
| DEAD-07: `_saved_spacing` | **PASS** | Fully removed, override preserved |
| NEW-11: `format!` -> `egui::Id` | **PASS** | Correct usage of `Id::new().with()` |
| ARCH-05: Workers FIFO comment | **PASS** | Comment matches `new_fifo()` call |
| Global cache O(n) comments | **PARTIAL PASS** | Line 325 still says O(1) for shift_remove |
| ENC-01: FPS rational | **PASS** | NTSC rates, rational frame_rate/time_base, sane GOP |
| ENC-04: UI freeze | **PASS** | Non-blocking, no sleep, Drop cleans up |

### Remaining Issues

1. **global_cache.rs line 325:** Comment says `O(1) lookup via hash` but the `shift_remove` on line 327 is actually O(n). Should be `O(1) hash lookup + O(n) shift` to match the style used at line 236.

2. **encode_ui.rs line 793:** Comment says `cleanup_orphan_handles() will reap it on the next UI tick` but cleanup is not called from the render loop -- only from the next `stop_encoding_internal()` call or on `Drop`. Comment is misleading but the behavior is functionally safe.
