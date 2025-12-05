# Playa Comprehensive Audit Plan

## Executive Summary

This document outlines the findings from a thorough code audit of the Playa application - a cross-platform image sequence and video player built with Rust + egui. The audit identified **47 issues** across 6 categories with varying severity levels.

---

## Issue Summary by Severity

| Severity | Count | Description |
|----------|-------|-------------|
| **CRITICAL** | 2 | Workers never shutdown, GpuCompositor::Clone unsafe |
| **HIGH** | 5 | Text input hotkey filtering, orphan encode thread, missing path validation, duplicate code |
| **MEDIUM** | 15 | Dead code, missing validation, incomplete implementations |
| **LOW** | 25 | Code organization, hardcoded values, unused code |

---

## PHASE 1: Critical Fixes (Immediate)

### 1.1 Workers Never Shutdown [CRITICAL]
**File:** `src/workers.rs:189-195`

**Problem:** The `shutdown` AtomicBool is never set to `true`. Workers spin forever after Drop.

**Fix:**
```rust
impl Drop for Workers {
    fn drop(&mut self) {
        debug!("Workers shutting down...");
        self.shutdown.store(true, Ordering::SeqCst);
        // Optionally join handles
    }
}
```

### 1.2 GpuCompositor::Clone Unsafe [CRITICAL]
**File:** `src/entities/gpu_compositor.rs:201`

**Problem:** `#[derive(Clone)]` on GpuCompositor doesn't properly handle OpenGL resources (FBO, VAO, VBO, Program). Cloning creates dangling handles.

**Fix Options:**
- A) Remove `Clone` derive entirely
- B) Implement proper Clone with Arc<> for shared OpenGL resources
- C) Use interior mutability pattern

**Recommended:** Option A - remove Clone, use `Arc<GpuCompositor>` where sharing is needed.

---

## PHASE 2: High Priority Fixes

### 2.1 Hotkeys Fire During Text Input [HIGH]
**File:** `src/main.rs:441-443`

**Problem:** When `ctx.wants_keyboard_input()` returns true, hotkeys still process.

**Fix:** Add early return in `handle_keyboard_input()`:
```rust
if ctx.wants_keyboard_input() {
    return; // Don't process hotkeys during text input
}
```

### 2.2 Orphan Encode Thread [HIGH]
**File:** `src/dialogs/encode/encode_ui.rs:519`

**Problem:** Encode thread can remain orphaned after timeout. No `Drop` implementation.

**Fix:**
```rust
impl Drop for EncodeDialog {
    fn drop(&mut self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        // Wait for thread with timeout
    }
}
```

### 2.3 Duplicate set_strategy() Call [HIGH]
**File:** `src/main.rs:1040-1047`

**Problem:** Copy-paste bug - `set_strategy()` called twice.

**Fix:** Remove lines 1045-1047.

### 2.4 Arc::get_mut Always Fails [HIGH]
**File:** `src/main.rs:961-963`

**Problem:** `Arc::get_mut()` returns `None` when other refs exist (always the case).

**Fix:** Redesign `CacheManager::set_memory_limit()` to use interior mutability or message passing.

### 2.5 Missing Output Path Validation [HIGH]
**File:** `src/dialogs/encode/encode_ui.rs:325-340`

**Problem:** No validation of output path before encoding.

**Fix:** Add validation for:
- Non-empty path
- Valid extension
- Writable directory
- Sufficient disk space (optional)

---

## PHASE 3: Dead Code Removal

### 3.1 Completely Unused Events
| File | Event | Line | Action |
|------|-------|------|--------|
| `player_events.rs` | `PreviousClipEvent` | 52 | DELETE |
| `player_events.rs` | `NextClipEvent` | 55 | DELETE |
| `encode_events.rs` | ALL 4 EVENTS | 6-21 | DELETE FILE or integrate with EventBus |

### 3.2 Events Defined but Never Sent
| File | Event | Line | Action |
|------|-------|------|--------|
| `player_events.rs` | `PlayEvent` | 7 | DELETE (TogglePlayPauseEvent sufficient) |
| `player_events.rs` | `PauseEvent` | 10 | DELETE (TogglePlayPauseEvent sufficient) |
| `comp_events.rs` | `TimelineChangedEvent` | 18 | DELETE handler or implement sending |

### 3.3 Duplicate ProgressBar Files
**Files:**
- `src/widgets/timeline/progress_bar.rs` (88 lines)
- `src/widgets/status/progress_bar.rs` (88 lines)

**Action:** DELETE `timeline/progress_bar.rs`, keep `status/progress_bar.rs`

### 3.4 Unused Entity Code
| File | Item | Line | Action |
|------|------|------|--------|
| `node.rs` | Entire module | ALL | DELETE or implement properly |
| `keys.rs` | `A_LOOP`, `A_PING_PONG` | 62, 64 | DELETE or implement loop playback |
| `keys.rs` | `A_OFFSET_X`, `A_OFFSET_Y` | 56, 58 | DELETE (deprecated) |
| `frame.rs` | `Frame::new_f16()` | 246-248 | DELETE |
| `frame.rs` | `Frame::new_f32()` | 253-255 | DELETE |
| `mod.rs` | `AttributeEditorUI` trait | 41-44 | DELETE |
| `mod.rs` | `NodeUI` trait | 47-50 | DELETE |

### 3.5 Unused in Main/Core
| File | Item | Line | Action |
|------|------|------|--------|
| `main.rs` | `_workers` variable | 1262-1266 | DELETE or wire to worker pool |
| `utils.rs` | `is_image()` function | 48-53 | DELETE |
| `utils.rs` | `IMAGE_EXTS` constant | 16 | DELETE |
| `prefs_events.rs` | `HotkeyWindow::AttributeEditor` | 34 | DELETE or implement |
| `gpu_compositor.rs` | `texture_cache` field | 210, 223 | DELETE or implement caching |

---

## PHASE 4: Code Deduplication

### 4.1 parse_video_path() Duplicated
**Files:**
- `src/entities/frame.rs:42-55`
- `src/entities/loader.rs:306-320`

**Action:** Create `src/utils/media.rs` with single implementation, import in both places.

### 4.2 Tonemapping Logic Duplicated
**File:** `src/entities/frame.rs:1045-1154`

**Problem:** F16 and F32 tonemapping code is nearly identical.

**Action:** Create generic `tonemap<T: FloatPixel>(...)` function.

---

## PHASE 5: Missing Implementations

### 5.1 Essential Hotkeys Missing
| Action | Suggested Key | Priority |
|--------|---------------|----------|
| Save Project | Ctrl+S | HIGH |
| Open Project | Ctrl+O | HIGH |
| Undo | Ctrl+Z | MEDIUM |
| Redo | Ctrl+Y | MEDIUM |
| Copy | Ctrl+C | MEDIUM |
| Paste | Ctrl+V | MEDIUM |
| Select All | Ctrl+A | LOW |

### 5.2 UI Components Incomplete
| Component | Issue | Action |
|-----------|-------|--------|
| Tonemapping mode | No UI control | Add ComboBox in encode dialog |
| Container selection | Auto-set only | Add manual MP4/MOV choice |
| General settings | Empty category | Populate or remove from TreeView |
| Attribute Editor | No keyboard nav | Add Up/Down/Enter/Delete |
| Project Panel | No context menu | Add right-click menu |

### 5.3 Validation Missing
| Location | Validation Needed |
|----------|-------------------|
| `Frame::new()` | Check width/height > 0 |
| `encode_ui.rs` | Output path validation |
| `prefs.rs` | Settings bounds on load |
| FPS slider | Round to integer or support fractional |

---

## PHASE 6: Legacy Migration Cleanup

### 6.1 Comp Legacy Fields
**File:** `src/entities/comp.rs:83-97`

**Problem:** Both legacy fields AND attrs-based storage exist for same data.

**Action:** Complete migration, remove legacy fields:
- `uuid` -> `A_UUID`
- `mode` -> `A_MODE`
- `parent` -> `A_PARENT`
- `file_mask` -> `A_FILE_MASK`
- `file_start` -> `A_FILE_START`
- `file_end` -> `A_FILE_END`

### 6.2 CompMode Enum Duplication
**File:** `src/entities/comp.rs:54-64`

**Problem:** Both `CompMode` enum AND `COMP_NORMAL/COMP_FILE` constants exist.

**Action:** Remove one, standardize on the other.

---

## PHASE 7: Code Organization

### 7.1 Split main.rs (~1400 lines)
Extract to separate files:
- `src/dock.rs` - DockTabs, TabViewer impl
- `src/app_init.rs` - App initialization logic
- `src/app_update.rs` - Update loop logic

### 7.2 Move HotkeyWindow Enum
**From:** `src/dialogs/prefs/prefs_events.rs:29-35`
**To:** `src/dialogs/prefs/input_handler.rs` or new `src/hotkeys.rs`

---

## PHASE 8: Hardcoded Values

### 8.1 Timeline Constants
**File:** `src/widgets/timeline/timeline_helpers.rs`

Move to `TimelineConfig`:
- `ruler_height = 20.0`
- `indicator_height = 4.0`
- `FontId::monospace(9.0)`

### 8.2 Encode Constants
**File:** `src/dialogs/encode/encode.rs`

Create `EncodeConfig`:
- GOP size calculation
- Bitrate mappings
- CRF thresholds

---

## Implementation Order

### Week 1: Critical & High Priority
1. [ ] Fix Workers shutdown (CRITICAL)
2. [ ] Fix GpuCompositor::Clone (CRITICAL)
3. [ ] Fix hotkeys during text input (HIGH)
4. [ ] Add EncodeDialog Drop (HIGH)
5. [ ] Remove duplicate set_strategy() (HIGH)
6. [ ] Fix Arc::get_mut issue (HIGH)
7. [ ] Add output path validation (HIGH)

### Week 2: Dead Code Cleanup
8. [ ] Delete unused events (player_events, encode_events)
9. [ ] Delete duplicate progress_bar.rs
10. [ ] Delete unused entity code
11. [ ] Delete unused main/core code

### Week 3: Deduplication & Missing Features
12. [ ] Consolidate parse_video_path()
13. [ ] Consolidate tonemapping logic
14. [ ] Add essential hotkeys (Ctrl+S, Ctrl+O)
15. [ ] Add missing validations

### Week 4: Cleanup & Organization
16. [ ] Complete Comp legacy migration
17. [ ] Split main.rs
18. [ ] Extract hardcoded values to configs

---

## Files to Modify (Summary)

| File | Changes |
|------|---------|
| `src/workers.rs` | Add shutdown signal in Drop |
| `src/entities/gpu_compositor.rs` | Remove Clone or implement properly |
| `src/main.rs` | Multiple fixes, split later |
| `src/dialogs/encode/encode_ui.rs` | Add Drop, validation |
| `src/dialogs/encode/encode_events.rs` | DELETE or integrate |
| `src/player_events.rs` | Remove 4 unused events |
| `src/comp_events.rs` | Remove TimelineChangedEvent |
| `src/entities/node.rs` | DELETE |
| `src/entities/keys.rs` | Remove unused constants |
| `src/entities/frame.rs` | Remove unused methods |
| `src/entities/mod.rs` | Remove unused traits |
| `src/entities/loader.rs` | Use shared parse_video_path |
| `src/entities/comp.rs` | Complete migration |
| `src/widgets/timeline/progress_bar.rs` | DELETE |
| `src/dialogs/prefs/input_handler.rs` | Add essential hotkeys |
| `src/dialogs/prefs/prefs_events.rs` | Remove AttributeEditor |
| `src/utils.rs` | Remove unused functions |

---

## Approval Required

Please review this plan and confirm:
1. Which phases to proceed with
2. Any items to skip or defer
3. Priority adjustments needed

Once approved, I will begin implementation starting with Phase 1 (Critical Fixes).
