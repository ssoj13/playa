# Phase 1 Fixes Verification Report

Date: 2026-03-18

---

## 1. BUG-02: API Play -- only emit when not playing

**File:** `src/app/api.rs` lines 90-94

**Code:**
```rust
ApiCommand::Play => {
    if !self.player.is_playing() {
        self.event_bus.emit(TogglePlayPauseEvent);
    }
}
```

**Verdict: PASS**
- TogglePlayPauseEvent is guarded by `!self.player.is_playing()`.
- No unconditional emit. Pause handler (lines 95-98) is symmetrically correct with `is_playing()`.

---

## 2. BUG-07: API SetFps -- updates comp attrs too

**File:** `src/app/api.rs` lines 106-111

**Code:**
```rust
ApiCommand::SetFps(fps) => {
    self.player.set_fps_base(fps);
    if let Some(comp_uuid) = self.player.active_comp() {
        self.project.modify_comp(comp_uuid, |comp| comp.set_fps(fps));
    }
}
```

**Verdict: PASS**
- Calls `set_fps_base` on the player.
- Calls `modify_comp(...set_fps...)` to persist FPS into comp attrs.
- Correctly guards with `active_comp()` check.

---

## 3. BUG-03: NodeKind shadow methods deleted

**File:** `src/entities/node_kind.rs`

**Searched for:** `fn fps()`, `fn _in()`, `fn _out()`, `fn frame()`, `fn is_file_mode()` -- **zero matches** in `impl NodeKind`.

**Remaining methods confirmed:** `is_file`, `is_comp`, `is_camera`, `is_text`, `is_renderable`, `is_listed`, `add_child_layer`, `as_file`, `as_file_mut`, `as_comp`, `as_comp_mut`, `as_camera`, `as_camera_mut`, `as_text`, `as_text_mut`, `file_mask`, `set_event_emitter`.

**Verdict: PASS**
- Shadow methods are fully removed. No `fps()`, `_in()`, `_out()`, `frame()`, `is_file_mode()` exist in `impl NodeKind`.

---

## 4. BUG-04 + BUG-13: loader_video safety

**File:** `src/entities/loader_video.rs` lines 46-55

**Code:**
```rust
if fps_rational.denominator() == 0 || time_base.denominator() == 0 {
    return Err(FrameError::LoadError(
        "Invalid video metadata: zero denominator in fps or time_base".to_string(),
    ));
}

let duration_secs =
    duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64;
let fps = fps_rational.numerator() as f64 / fps_rational.denominator() as f64;
let frame_count = (duration_secs * fps).round() as usize;
```

**Verdict: PASS**
- Zero-check for both `fps_rational.denominator()` and `time_base.denominator()` BEFORE any division (line 46).
- `frame_count` uses `.round() as usize` (line 55), not truncation.

---

## 5. BUG-06: deferred_load_sequences accumulates

**File:** `src/app/events.rs` line 166

**Code:**
```rust
deferred_load_sequences.get_or_insert_with(Vec::new).extend(paths);
```

**Verdict: PASS**
- Uses `.get_or_insert_with(Vec::new).extend(paths)` -- accumulates into existing vec.
- NOT `= Some(paths)` which would overwrite.

---

## 6. BUG-09: SetFrameEvent no double preload

**File:** `src/main_events.rs` lines 243-267

**Code (key section):**
```rust
if let Some(e) = downcast_event::<SetFrameEvent>(event) {
    // ... distance calculation + epoch increment ...
    project.modify_comp(comp_uuid, |comp| {
        comp.set_frame(e.0);
    });
    // enqueue_frames is intentionally omitted: CurrentFrameChangedEvent
    // emitted by modify_comp handles preloading to avoid double-preload.
    return Some(result);
}
```

**Verdict: PASS**
- No `result.enqueue_frames = true` anywhere in the SetFrameEvent handler.
- Explicit comment explains the intentional omission (line 263-264).
- Preloading is delegated to `CurrentFrameChangedEvent` emitted by `modify_comp`.

---

## 7. BUG-11: CameraNode use_poi

**File:** `src/entities/camera_node.rs` lines 108-109

**Code:**
```rust
pub fn use_poi(&self) -> bool {
    self.attrs.get_bool("use_poi").unwrap_or(false)
}
```

**Verdict: PASS**
- Uses `unwrap_or(false)` -- defaults to rotation mode when attr is missing.

---

## 8. BUG-12: contains_comp type check

**File:** `src/entities/project.rs` lines 650-652

**Code:**
```rust
pub fn contains_comp(&self, uuid: Uuid) -> bool {
    self.with_comp(uuid, |_| ()).is_some()
}
```

**Verdict: PASS**
- Uses `with_comp` (which internally checks `NodeKind::Comp`) to verify the node is actually a comp.
- NOT `contains_node` (which would return true for any node type).
- Caller in `src/core/player.rs:294` uses `contains_comp` correctly.

---

## 9. SEC-01+02: Server security

**File:** `src/server/api.rs`

**Bind address (line 201):**
```rust
let addr = format!("127.0.0.1:{}", self.port);
```

**FPS validation (lines 252-262):**
```rust
if let Ok(fps) = fps_str.parse::<f32>() {
    if fps.is_finite() && fps > 0.0 && fps <= 960.0 {
        return Self::send_command(tx, ApiCommand::SetFps(fps))...;
    }
    return Response::json(&ApiResponse::err("FPS must be between 0.001 and 960"))
        .with_status_code(400)...;
}
```

**Verdict: PASS**
- SEC-01: Server binds to `127.0.0.1` (localhost only), not `0.0.0.0`.
- SEC-02: FPS validated with `is_finite() && > 0.0 && <= 960.0`. Rejects NaN, Inf, zero, negative, and absurdly high values. Returns 400 with error message on invalid input.

---

## 10. BUG-08 revert: HSV value order param + Effect::new default

**File:** `src/entities/effects/mod.rs`

**HSV schema (line 146):**
```rust
AttrDef::with_ui_order("value", AttrType::Float, FX, &["0", "2", "0.01"], 2.0),
```

**Effect::new for AdjustHSV (lines 199-203):**
```rust
EffectType::AdjustHSV => {
    attrs.set("hue_shift", AttrValue::Float(0.0));
    attrs.set("saturation", AttrValue::Float(1.0));
    attrs.set("value", AttrValue::Float(1.0));
}
```

**Verdict: PASS**
- HSV "value" line has `2.0` as the last argument to `with_ui_order` (the `ui_order` param).
- Effect::new sets "value" default to `1.0` (no change), which is the correct identity value for HSV value multiplier.

---

## Summary

| # | Bug ID | Check | Result |
|---|--------|-------|--------|
| 1 | BUG-02 | Play guard | **PASS** |
| 2 | BUG-07 | SetFps dual update | **PASS** |
| 3 | BUG-03 | Shadow methods removed | **PASS** |
| 4 | BUG-04+13 | Video zero-check + round | **PASS** |
| 5 | BUG-06 | deferred_load accumulate | **PASS** |
| 6 | BUG-09 | No double preload | **PASS** |
| 7 | BUG-11 | use_poi unwrap_or(false) | **PASS** |
| 8 | BUG-12 | contains_comp via with_comp | **PASS** |
| 9 | SEC-01+02 | Localhost bind + FPS validation | **PASS** |
| 10 | BUG-08 | HSV value order=2.0, default=1.0 | **PASS** |

**All 10 checks: PASS**
