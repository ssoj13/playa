# Timeline Bug Hunt Report - Session 2

## Summary
All 5 timeline bugs from task.md have been identified and fixed.

---

## Bug 1: Layer controls not aligned horizontally (FIXED)

**Problem:** Different layer name lengths caused controls (checkbox, opacity slider, blend combo, speed) to be misaligned.

**Root Cause:** `render_outline` in `timeline_ui.rs` used dynamic width for name column.

**Fix:** Added fixed 150px width to name column:
```rust
// timeline_ui.rs:246-253
row_ui.allocate_ui_with_layout(
    egui::Vec2::new(150.0, config.layer_height),
    egui::Layout::left_to_right(egui::Align::Center),
    |ui| {
        ui.set_min_width(150.0);
        ui.add(egui::Label::new(child_name).truncate());
    },
);
```

---

## Bug 2: Unnecessary reorder drag handle (FIXED)

**Problem:** Left side had a drag handle for reordering, but drag'n'drop is available in the canvas area.

**Fix:** Removed drag handle rendering, only consuming the DnD handle without display:
```rust
// timeline_ui.rs:217-218
// Consume DnD handle without rendering (reorder via canvas DnD)
let _ = handle;
```

---

## Bug 3: Layers disappearing when dragged beyond timeline (FIXED)

**Problem:** When moving one layer, another layer would disappear. User reported: "moving second layer causes FIRST layer to disappear."

**Root Cause:** `compute_layer_rows` function was doing "smart packing" - placing non-overlapping layers on the same visual row. This caused background rectangles to overlap and hide layers.

**Fix:** Completely removed smart row packing. Now `row = layer_index` (simple 1:1 mapping):
```rust
// timeline_ui.rs:540-544
// Simple row assignment: each layer gets its own row based on index
// No "smart" packing - layer order in children = visual row order
let child_order_inner: Vec<usize> = (0..comp.children.len()).collect();
let num_layers = comp.children.len();
let total_height_inner = (num_layers.max(1) as f32) * config.layer_height;
```

Row backgrounds now drawn by index:
```rust
// First pass: draw row backgrounds (alternating colors)
for row in 0..num_layers {
    // ...
}
// Second pass: draw layer bars
for idx in ... {
    let row = idx;  // Simple: row = layer index
}
```

**Cleanup:** Removed unused `compute_all_layer_rows` from `timeline_helpers.rs`.

---

## Bug 4: Viewport not updating when cache status changes (FIXED)

**Problem:** App started, cache filled (visible in status strip), but viewport showed "Loading frame N" until user moved the time cursor.

**Root Cause:** `texture_needs_upload` in `main.rs` only checked if frame NUMBER changed, not if frame STATUS changed from Loading to Loaded.

**Fix:** Added `frame_loading` check to force refresh while frame is Loading/Header:
```rust
// main.rs: render_viewport_tab()
let frame_changed = self.displayed_frame != Some(self.player.current_frame(&self.project));
let frame_loading = self.frame.as_ref()
    .map(|f| matches!(f.status(), FrameStatus::Header | FrameStatus::Loading))
    .unwrap_or(false);
let texture_needs_upload = frame_changed || frame_loading;

if texture_needs_upload {
    self.frame = self.player.get_current_frame(&self.project);
    if !frame_loading {
        self.displayed_frame = Some(self.player.current_frame(&self.project));
    }
}
```

Combined with existing `ui.ctx().request_repaint()` in `viewport_ui.rs:157` for Loading frames, this creates a polling loop until frame is loaded.

---

## Bug 5: Strange timeline panel offsets (FIXED)

**Problem:** User described panels as "rendered with crazy offsets" in Split mode. The outline and canvas panels didn't align properly.

**Root Cause:** `SidePanel` and `CentralPanel` have different default frames/margins in egui. A magic number `24.0` was hardcoded to compensate:
```rust
// OLD: timeline_ui.rs:182
ui.add_space(20.0 + status_bar_height + 4.0 + 24.0);
// Comment: "Extra 24.0 accounts for panel frame differences between SidePanel and CentralPanel"
```

**Fix:** Set `Frame::NONE` on both panels to remove default margins, then removed the magic offset:

```rust
// ui.rs:119-123 (SidePanel)
let outline_response = egui::SidePanel::left("timeline_outline")
    .resizable(true)
    .min_width(100.0)
    .default_width(saved_width)
    .frame(egui::Frame::NONE)  // Remove default frame
    .show_inside(ui, |ui| { ... });

// ui.rs:159-161 (CentralPanel)
egui::CentralPanel::default()
    .frame(egui::Frame::NONE)  // Remove default frame
    .show_inside(ui, |ui| { ... });
```

Also applied to CanvasOnly and OutlineOnly modes for consistency.

Timeline offset now clean:
```rust
// timeline_ui.rs:182
ui.add_space(20.0 + status_bar_height + 4.0);  // No more magic number
```

---

## Files Modified

| File | Changes |
|------|---------|
| `src/ui.rs` | Added `Frame::NONE` to all timeline panels (Split, CanvasOnly, OutlineOnly modes) |
| `src/widgets/timeline/timeline_ui.rs` | Fixed name column width (150px), removed drag handle, simplified row layout (row=index), removed 24.0 magic offset |
| `src/widgets/timeline/timeline_helpers.rs` | Removed dead code `compute_all_layer_rows` |
| `src/main.rs` | Added frame_loading check to viewport update logic |

---

## Remaining Notes

1. **Dead code in comp.rs:** `compute_layer_rows` and `find_insert_position_for_row` are still present but now serve no purpose since row = index. Consider removing them in a future cleanup.

2. **Preload strategies:** User asked about cache behavior (sometimes only frames around cursor are green, sometimes whole timeline). This is due to different preload strategies:
   - `Spiral` strategy (for images): preloads bidirectionally from current frame
   - `Forward` strategy (for video): preloads only forward from current frame
   Strategy is chosen automatically based on source type.

---

## Build Status

All changes compile without warnings or errors.
