# Playa Widgets Audit Report
**Date:** 2025-12-04  
**Scope:** All widgets in `src/widgets/`

---

## Executive Summary

Analysis of 17 widget files across 5 widget modules. Found **11 issues** total:
- **2 CRITICAL** - Duplicate code, unused functionality
- **4 HIGH** - Dead code, missing handlers
- **3 MEDIUM** - Hardcoded values, incomplete implementations
- **2 LOW** - Minor improvements

---

## 1. CRITICAL ISSUES

### 1.1 Duplicate ProgressBar Implementation
**Files:**
- `C:\projects\projects.rust\playa\src\widgets\timeline\progress_bar.rs` (88 lines)
- `C:\projects\projects.rust\playa\src\widgets\status\progress_bar.rs` (88 lines)

**Issue:** Two identical ProgressBar implementations exist in different modules. Both files are 100% identical with the same struct definition and render logic.

**Impact:** Code duplication, maintenance overhead, risk of divergent behavior if one is updated but not the other.

**Recommendation:** Delete one copy and re-export from a single location. The `status` module already exports `ProgressBar`, so `timeline/progress_bar.rs` should be removed.

### 1.2 ProgressBar is Never Used Anywhere
**Files:** Both progress_bar.rs files

**Issue:** grep for `ProgressBar` returns 0 results outside the definition files. The widget is defined but never instantiated or rendered anywhere in the codebase.

**Impact:** Dead code bloat.

**Recommendation:** Either remove entirely or integrate into StatusBar where it was likely intended.

---

## 2. HIGH SEVERITY ISSUES

### 2.1 Viewport Dead Code - Marked with #[allow(dead_code)]
**File:** `C:\projects\projects.rust\playa\src\widgets\viewport\viewport.rs`
**Lines:** 165-192 and 193-300

```rust
#[allow(dead_code)]
pub fn is_point_over_image(&self, screen_pos: egui::Vec2) -> bool { ... }

#[allow(dead_code)]
pub fn screen_to_image(&self, screen_pos: egui::Vec2) -> Option<egui::Vec2> { ... }
```

**Issue:** Two public methods explicitly marked as dead code. These are coordinate conversion utilities that could be useful but are not currently called.

**Impact:** Code that may have been written for future features but never used.

**Recommendation:** Either integrate these methods into viewport event handling (e.g., for pixel inspection, color picker) or remove them.

### 2.2 StatusBar::update() is No-Op
**File:** `C:\projects\projects.rust\playa\src\widgets\status\status.rs`
**Lines:** 23-25

```rust
pub fn update(&mut self, ctx: &egui::Context) {
    let _ = ctx;
}
```

**Issue:** The update method accepts Context but does nothing with it. The method signature suggests it should read messages from a channel, but the implementation is empty.

**Impact:** StatusBar message updates are broken. `self.current_message` can never be updated after construction.

**Recommendation:** Either implement the message reading logic or remove the method if messages will be passed via render() directly.

### 2.3 timeline_helpers Functions Not Called
**File:** `C:\projects\projects.rust\playa\src\widgets\timeline\timeline_helpers.rs`

**Unused functions (grep returns 0 external calls):**
- `find_free_row_for_new_layer()` (lines 385-431)
- `draw_playhead()` (lines 199-227)

**Issue:** These helper functions are defined but never called from timeline_ui.rs or anywhere else.

**Impact:** Dead code that creates false impression of functionality.

### 2.4 TimelineViewMode Enum Likely Unused
**File:** `C:\projects\projects.rust\playa\src\widgets\timeline\timeline.rs`

**Issue:** `TimelineViewMode` enum is exported but grepping shows limited usage beyond the definition.

**Impact:** Potential incomplete feature implementation for different timeline display modes.

---

## 3. MEDIUM SEVERITY ISSUES

### 3.1 Hardcoded UI Values in timeline_helpers.rs
**File:** `C:\projects\projects.rust\playa\src\widgets\timeline\timeline_helpers.rs`

**Hardcoded values:**
- Line 113: `ruler_height = 20.0`
- Line 246: `indicator_height = 4.0`
- Line 178: `FontId::monospace(9.0)` - ruler font size
- Line 455: `bar_height = (config.layer_height - 8.0).max(2.0)` - magic number 8.0
- Line 462: `egui::Stroke::new(2.0, ...)` - stroke width

**Impact:** UI cannot be customized via settings, inconsistent with other configurable values in TimelineConfig.

**Recommendation:** Move these to TimelineConfig struct for user customization.

### 3.2 Hardcoded Colors in status.rs
**File:** `C:\projects\projects.rust\playa\src\widgets\status\status.rs`

**Hardcoded values:**
- Line 96: `log::debug!` always runs (even in release) - should use cfg debug check
- Line 100: `log::warn!` on None - may spam logs

**Impact:** Performance and log noise.

### 3.3 Attribute Editor Missing Edit Handlers for Some Types
**File:** `C:\projects\projects.rust\playa\src\widgets\ae\ae_ui.rs`
**Lines:** 228-237

```rust
(_, AttrValue::Mat3(_)) => {
    ui.label("(3x3 matrix - not editable)");
}
(_, AttrValue::Mat4(_)) => {
    ui.label("(4x4 matrix - not editable)");
}
(_, AttrValue::Json(s)) => {
    ui.label(format!("JSON: {} chars", s.len()));
}
(_, AttrValue::List(items)) => {
    ui.label(format!("List: {} items", items.len()));
}
```

**Issue:** These attribute types are display-only with no editing capability. The user cannot modify matrix, JSON, or list values.

**Impact:** Incomplete attribute editor functionality.

**Recommendation:** Add matrix component editors, JSON text area, and list item management.

---

## 4. LOW SEVERITY ISSUES

### 4.1 StatusBar Debug Logging in Render Path
**File:** `C:\projects\projects.rust\playa\src\widgets\status\status.rs`
**Line:** 96

```rust
log::debug!("StatusBar: cache_manager present, usage={}MB, limit={}MB", usage_mb, limit_mb);
```

**Issue:** Debug logging on every render frame even when log level is not debug. Small performance overhead.

**Recommendation:** Remove or gate behind a feature flag.

### 4.2 hsv_to_rgb Could Be External Crate
**File:** `C:\projects\projects.rust\playa\src\widgets\timeline\timeline_helpers.rs`
**Lines:** 481-506

**Issue:** Custom HSV to RGB conversion that duplicates functionality available in color crates.

**Impact:** Minor - works correctly but adds maintenance burden.

---

## 5. EVENT BUS CONNECTIVITY ANALYSIS

### 5.1 All Widgets Properly Connected to EventBus

| Widget | Events Defined | Events Handled in main_events.rs |
|--------|---------------|----------------------------------|
| Viewport | ZoomViewportEvent, ResetViewportEvent, FitViewportEvent, Viewport100Event | YES |
| Timeline | TimelineZoomChangedEvent, TimelinePanChangedEvent, TimelineSnapChangedEvent, TimelineLockWorkAreaChangedEvent, TimelineFitAllEvent, TimelineFitEvent, TimelineResetZoomEvent | YES |
| Project | ProjectSelectionChangedEvent, ProjectActiveChangedEvent, SaveProjectEvent, LoadProjectEvent, AddClipsEvent, AddCompEvent, RemoveMediaEvent, ClearAllMediaEvent | YES |
| Status | SetLoopEvent | YES |
| AE/Attributes | (renders inline, no dedicated events) | N/A |

**Status:** All widget events are properly handled in `main_events.rs`. No orphaned event types found.

### 5.2 Event Flow Verification

1. **Project widget** -> emits events via `ProjectActions.send()` -> EventBus -> `handle_app_event()`
2. **Timeline widget** -> emits events via `TimelineActions.events` -> EventBus -> `handle_app_event()`
3. **Viewport widget** -> emits events via hotkey handler -> EventBus -> `handle_app_event()`
4. **Status widget** -> emits events via `dispatch` closure -> EventBus -> `handle_app_event()`

---

## 6. ORPHANED WIDGETS/COMPONENTS

### 6.1 timeline/progress_bar.rs - Orphaned Module
- Defined in `mod.rs` but never used
- Should be removed (duplicate of status/progress_bar.rs which is also unused)

### 6.2 ViewportScrubber.frozen_bounds() - Likely Unused
**File:** `C:\projects\projects.rust\playa\src\widgets\viewport\viewport.rs`
**Line:** 444

Method `frozen_bounds()` returns cached bounds but no callers found outside the file.

---

## 7. UI STATE SYNCHRONIZATION ISSUES

### 7.1 TimelineState.last_canvas_width Not Always Updated
**File:** Timeline uses `last_canvas_width` for TimelineFitEvent but it's set only when canvas is rendered.

**Issue:** If TimelineFitEvent fires before first render, `last_canvas_width` will be 0 causing division issues.

**Recommendation:** Add validation in event handler:
```rust
if timeline_state.last_canvas_width <= 0.0 {
    return Some(result); // Skip if not yet rendered
}
```

### 7.2 Attributes Panel State Not Saved Between Sessions
**File:** `C:\projects\projects.rust\playa\src\widgets\ae\ae_ui.rs`

**Issue:** `AttributesState` has `#[derive(serde::Serialize, serde::Deserialize)]` but actual persistence is not verified.

**Impact:** Column width preferences may be lost on restart.

---

## 8. MISSING FUNCTIONALITY

### 8.1 No Keyboard Navigation in Project Panel
**File:** `C:\projects\projects.rust\playa\src\widgets\project\project_ui.rs`

**Missing:**
- Arrow Up/Down to navigate between items
- Enter to activate selected item
- Delete key to remove selected items (only X button exists)

### 8.2 No Context Menu in Project Panel
**File:** `C:\projects\projects.rust\playa\src\widgets\project\project_ui.rs`

**Missing:**
- Right-click context menu for Rename, Duplicate, Delete, Properties

### 8.3 Attribute Editor Cannot Create New Attributes
**File:** `C:\projects\projects.rust\playa\src\widgets\ae\ae_ui.rs`

**Issue:** Can only edit existing attributes, no "Add Attribute" button.

---

## 9. RECOMMENDATIONS PRIORITY

### Immediate (P0):
1. Remove duplicate `timeline/progress_bar.rs`
2. Remove or integrate unused `ProgressBar` widget
3. Remove dead code methods in viewport.rs

### Short-term (P1):
4. Implement `StatusBar::update()` or remove it
5. Move hardcoded UI values to TimelineConfig
6. Add keyboard navigation to Project panel

### Medium-term (P2):
7. Add matrix/JSON/list editors to Attributes panel
8. Add context menu to Project panel
9. Review and cleanup unused timeline_helpers functions

---

## 10. FILES ANALYZED

| Path | Lines | Status |
|------|-------|--------|
| widgets/mod.rs | ~20 | OK |
| widgets/viewport/mod.rs | ~10 | OK |
| widgets/viewport/viewport.rs | ~480 | 2 dead code methods |
| widgets/viewport/viewport_ui.rs | ~200 | OK |
| widgets/viewport/viewport_events.rs | ~25 | OK |
| widgets/viewport/renderer.rs | ~300 | OK |
| widgets/viewport/shaders.rs | ~150 | OK |
| widgets/timeline/mod.rs | ~16 | OK |
| widgets/timeline/timeline.rs | ~200 | OK |
| widgets/timeline/timeline_ui.rs | ~600 | OK |
| widgets/timeline/timeline_events.rs | ~23 | OK |
| widgets/timeline/timeline_helpers.rs | ~507 | 2 unused functions |
| widgets/timeline/progress_bar.rs | ~88 | DUPLICATE - remove |
| widgets/project/mod.rs | ~9 | OK |
| widgets/project/project.rs | ~22 | OK |
| widgets/project/project_ui.rs | ~345 | Missing keyboard nav |
| widgets/ae/mod.rs | ~8 | OK |
| widgets/ae/ae_ui.rs | ~255 | Some types non-editable |
| widgets/status/mod.rs | ~5 | OK |
| widgets/status/status.rs | ~153 | update() is no-op |
| widgets/status/progress_bar.rs | ~88 | UNUSED - remove |

**Total lines analyzed:** ~3,500+

---

*End of Report*
