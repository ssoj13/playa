# Plan 4: Code Quality Improvements - Task 3

## Status: COMPLETED (awaiting approval)

## Summary

Implemented 6 code quality improvements from task3.md:

| # | Task | Status |
|---|------|--------|
| 1 | Timeline control alignment | DONE |
| 2 | Attribute Editor modifier keys | PENDING (awaiting clarification) |
| 3 | Ctrl-D layer duplication | DONE |
| 4 | Ctrl-C/Ctrl-V clipboard | DONE |
| 5 | Click empty area clears selection | DONE |
| 6 | Tooltips checkbox in Preferences | DONE |

---

## Completed Changes

### Task 1: Timeline Control Alignment
**Files modified:**
- `src/widgets/timeline/timeline_ui.rs`

**Changes:**
- Added fixed width (60px) for opacity slider using `allocate_ui_with_layout`
- Added fixed width (50px) for speed DragValue
- Layer name already had 120px width
- Blend mode combobox already had 80px width

**Result:** All columns now align vertically regardless of layer name length.

---

### Task 3: Ctrl-D Layer Duplication
**Files modified:**
- `src/entities/comp_events.rs` - Added `DuplicateLayersEvent`
- `src/dialogs/prefs/input_handler.rs` - Added hotkey binding
- `src/main.rs` - Added event routing with comp_uuid
- `src/main_events.rs` - Added event handler

**Behavior:**
- Duplicates all selected layers
- Inserts copies ABOVE originals (same index)
- Generates unique names via `project.gen_name()`
- Selects duplicated layers

---

### Task 4: Ctrl-C/Ctrl-V Clipboard
**Files modified:**
- `src/entities/comp_events.rs` - Added `CopyLayersEvent`, `PasteLayersEvent`
- `src/widgets/timeline/timeline.rs` - Added `ClipboardLayer` struct
- `src/widgets/timeline/mod.rs` - Exported `ClipboardLayer`
- `src/dialogs/prefs/input_handler.rs` - Added hotkey bindings
- `src/main.rs` - Added event routing
- `src/main_events.rs` - Added event handlers

**ClipboardLayer struct:**
```rust
pub struct ClipboardLayer {
    pub source_uuid: Uuid,
    pub attrs: Attrs,
    pub original_start: i32,
}
```

**Behavior:**
- Ctrl-C: Copies selected layers to `timeline_state.clipboard`
- Ctrl-V: Pastes at playhead position
- Preserves relative layer positions (offset from first layer)
- Generates unique names
- Works across different Comps (clipboard is global in TimelineState)

---

### Task 5: Click Empty Area Clears Selection
**Files modified:**
- `src/widgets/timeline/timeline_ui.rs`

**Changes:**
Added after outline DnD scroll area:
```rust
let remaining_height = ui.available_height();
if remaining_height > 0.0 {
    let (empty_rect, empty_response) = ui.allocate_exact_size(...);
    if empty_response.clicked() && primary_clicked {
        // Clear selection
        dispatch(CompSelectionChangedEvent { selection: vec![], anchor: None });
    }
}
```

**Behavior:**
- Left click on empty area below layers clears selection
- Visual hover feedback (subtle highlight)
- Middle click still pans (not affected)

---

### Task 6: Tooltips Checkbox in Preferences
**Files modified:**
- `src/dialogs/prefs/prefs.rs` - Added `show_tooltips: bool` to `AppSettings`
- `src/widgets/timeline/timeline_ui.rs` - Modified `render_toolbar()` signature
- `src/ui.rs` - Updated `render_timeline_panel()` signature and call
- `src/main.rs` - Passed `settings.show_tooltips` to render
- `src/bin/timeline.rs` - Updated standalone timeline

**Tooltips added to:**
- **Snap** - "Snap to frame edges when dragging layers"
- **Lock** - "Lock work area markers (B/N keys)"
- **Loop** - "Loop playback within work area (` key)"

**Settings UI:** Checkbox in Preferences > UI > Appearance section.

---

## Pending: Task 2 - Attribute Editor Modifier Keys

**Current behavior (ae_ui.rs:198-205):**
```rust
let speed_mult = if modifiers.shift {
    5.0   // Shift = 5x faster (coarse)
} else if modifiers.ctrl {
    0.1   // Ctrl = 0.1x slower (fine)
} else {
    1.0
};
```

**Question:** You requested "Shift=5%, Ctrl=1%". Current logic makes:
- Shift = FASTER drag (5x multiplier)
- Ctrl = SLOWER drag (0.1x multiplier)

Is this the intended behavior or should it be inverted/changed?

---

## Build Status

```
cargo check -> OK (no errors, no warnings)
```

---

## Files Changed Summary

| File | Changes |
|------|---------|
| `src/entities/comp_events.rs` | +3 new events |
| `src/widgets/timeline/timeline.rs` | +ClipboardLayer struct, +clipboard field |
| `src/widgets/timeline/timeline_ui.rs` | Fixed widths, empty click handler, tooltips |
| `src/widgets/timeline/mod.rs` | Export ClipboardLayer |
| `src/dialogs/prefs/prefs.rs` | +show_tooltips setting |
| `src/dialogs/prefs/input_handler.rs` | +3 hotkey bindings |
| `src/ui.rs` | +show_tooltips param |
| `src/main.rs` | Event routing |
| `src/main_events.rs` | +3 event handlers |
| `src/bin/timeline.rs` | Updated call |

---

## Testing Recommendations

1. **Timeline alignment**: Open comp with layers of different name lengths, verify columns align
2. **Ctrl-D**: Select layer(s), press Ctrl-D, verify duplicate appears above with new name
3. **Ctrl-C/V**: Copy layers in one comp, navigate to another, paste at playhead
4. **Empty click**: Click below last layer, verify selection clears
5. **Tooltips**: Enable in Preferences, hover over Snap/Lock/Loop, verify 2s delay tooltip appears
