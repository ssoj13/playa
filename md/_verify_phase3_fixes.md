# Phase 3 Verification Report

Date: 2026-03-18

## 1. DUP-03: EventEmitter Deref newtype

**File:** `src/core/event_bus.rs`
**Status: PASS**

- `EventEmitter` is a thin newtype: `pub struct EventEmitter(Arc<EventBus>);` (line 217)
- `Deref<Target = EventBus>` implemented (lines 229-234)
- No duplicated methods on `EventEmitter` -- only `new()` and `inner()` remain (lines 219-227)
- All `emit`, `subscribe`, `poll`, etc. are accessed via Deref to `EventBus`
- `EventBus` retains all methods: `subscribe`, `emit`, `emit_boxed`, `poll`, `emitter`, `unsubscribe_all`, `clear`, `has_subscribers`, `queue_len`
- `CompEventEmitter` still works: wraps `Option<EventEmitter>`, delegates `emit` through inner emitter (lines 245-268)
- Test `test_emitter_handle` confirms `emitter.emit()` works through Deref (line 357)

## 2. DUP-04: handle_attrs_changed helper extracted

**File:** `src/app/events.rs`
**Status: PASS**

- Helper method: `fn handle_attrs_changed(&mut self, comp_uuid: uuid::Uuid)` (line 257)
- Docstring: "Shared by the main event loop and the derived-events loop." (lines 254-256)
- Main event loop calls: `self.handle_attrs_changed(e.0)` (line 68)
- Derived events loop calls: `self.handle_attrs_changed(e.0)` (line 202)
- Logic preserved: increment epoch -> clear cache -> enqueue current frame -> schedule debounced preload -> emit ViewportRefreshEvent (lines 258-272)
- No duplication -- both call sites delegate to the shared helper

## 3. DUP-08: fit_timeline_to_range extracted

**File:** `src/main_events.rs`
**Status: PASS**

- Helper function: `fn fit_timeline_to_range(timeline_state, canvas_width, min_frame, max_frame)` (lines 207-218)
- `DEFAULT_PPF` is a named constant: `const DEFAULT_PPF: f32 = 2.0;` (line 215)
- Three call sites use the helper:
  - `TimelineFitAllEvent` handler (line 616)
  - `TimelineFitEvent` handler (line 628)
  - `TimelineFitWorkAreaEvent` handler (line 640)
- Logic preserved: `duration = (max - min + 1).max(1)`, `ppf = width / duration`, `zoom = (ppf / DEFAULT_PPF).clamp(0.1, 20.0)`, `pan_offset = min_frame`

## 4. DEAD-03: Legacy constants deleted from keys.rs

**File:** `src/entities/keys.rs`
**Status: PASS**

- `COMP_NORMAL` -- NOT present (grep: 0 matches)
- `COMP_FILE` -- NOT present (grep: 0 matches)
- `A_MODE` -- NOT present (grep: 0 matches)
- File contains only active constants (79 lines total)

## 5. DEAD-04: ae_ui collect_changes param removed

**File:** `src/widgets/ae/ae_ui.rs`
**Status: PASS**

- `render_impl` signature (lines 92-99): takes `ui`, `attrs`, `state`, `display_name`, `mixed_keys`, `changed_out`
- `collect_changes: bool` parameter -- NOT present (grep: 0 matches in entire file)

## 6. DEAD-05: StatusBar::update no-op cleaned

**File:** `src/widgets/status/status.rs`
**Status: PASS**

- `update` method (line 22): `pub fn update(&mut self, _ctx: &egui::Context) {}`
- Empty body, no `let _ = ctx;` -- uses `_ctx` prefix convention instead
- Clean one-liner

## 7. DEAD-02: space.rs src_to_object deleted

**File:** `src/entities/space.rs`
**Status: PASS**

- `src_to_object` -- NOT present (grep: 0 matches)
- `object_to_src` still exists (line 65)
- File contains: `image_to_frame`, `frame_to_image`, `object_to_src`, `to_math_rot`, `from_math_rot` (87 lines total)

---

**All 7 checks PASSED. No issues found.**
