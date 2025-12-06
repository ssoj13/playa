# Plan 2: Code Quality Issues - Analysis & Solutions (v2)

## Overview
Task 3 from task2.md - 5 HIGH priority UI/UX issues requiring fixes.

---

## Issue 1: Timeline Outline Controls Not Aligned

### Current State
File: `src/widgets/timeline/timeline_ui.rs:129-294` (`render_outline`)

Controls render in `row_ui` with this order:
1. Handle (drag grip)
2. Visibility checkbox
3. Layer name (Label) - **PROBLEM: variable width**
4. Opacity slider
5. Blend mode combobox
6. Speed drag value

### Solution: Use `egui::Grid` (simplest)
Grid automatically aligns columns across rows:

```rust
egui::Grid::new("outline_grid")
    .num_columns(6)
    .spacing([6.0, 0.0])
    .striped(true)
    .show(ui, |ui| {
        for (idx, (child_uuid, attrs)) in comp.children.iter().enumerate() {
            // Col 1: Handle
            ui.label("≡");

            // Col 2: Visibility
            let mut visible = attrs.get_bool("visible").unwrap_or(true);
            ui.checkbox(&mut visible, "");

            // Col 3: Name (truncated)
            let name = attrs.get_str("name").unwrap_or("?");
            ui.add(egui::Label::new(name).truncate());

            // Col 4: Opacity
            let mut opacity = attrs.get_float("opacity").unwrap_or(1.0);
            ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false));

            // Col 5: Blend mode
            // ...combobox...

            // Col 6: Speed
            // ...drag value...

            ui.end_row();
        }
    });
```

### Implementation
- [ ] Replace `row_ui` horizontal layout with `egui::Grid`
- [ ] Use `.min_col_width()` for name column if needed
- [ ] Keep DnD handle outside grid or integrate via response

---

## Issue 2: Attribute Editor Shift/Ctrl Modifiers

### Current State
File: `src/widgets/ae/ae_ui.rs:192-292` (`render_value_editor`)

DragValue widgets use fixed speed (0.1 for float, 1.0 for int) without modifier support.

### Requirement
- Default: normal speed (0.1/1.0)
- Shift held: 5x speed (coarse, ~5% steps)
- Ctrl held: 0.1x speed (fine, ~1% steps)

### Solution
```rust
fn render_value_editor(ui: &mut Ui, key: &str, value: &mut AttrValue, mixed: bool) -> bool {
    let modifiers = ui.input(|i| i.modifiers);
    let speed_mult = if modifiers.shift {
        5.0  // coarse
    } else if modifiers.ctrl {
        0.1  // fine
    } else {
        1.0
    };

    // Apply to Float:
    (_, AttrValue::Float(v)) => {
        scope_changed |= ui.add(
            egui::DragValue::new(v).speed(0.1 * speed_mult)
        ).changed();
    }
    // ... same for Int, UInt, Vec3, Vec4 ...
}
```

### Implementation
- [ ] Add `speed_mult` calculation at function start
- [ ] Apply multiplier to all DragValue instances

---

## Issue 3: Local trim_in/trim_out + Unified Setters

### Current State

**Comp-level** setters exist (`set_in`, `set_out`, `set_trim_in`, `set_trim_out`) with smart sync.

**Layer attrs** use absolute parent coordinates:
- `in/out` - layer position in parent timeline (absolute frames)
- `trim_in/trim_out` - **also absolute frames in parent**

### Problem
Current: layer compB (0..100) placed at frame 20 in compA:
- `in=20, out=120` (position)
- `trim_in=20, trim_out=120` (no trim = same as in/out)
- If trim_in=40 → layer plays from parent frame 40

This is confusing because trim values are tied to parent timeline.

### Proposed Change: Local Trim Coordinates
Store `trim_in/trim_out` as **offsets from layer start in local frames**:

```
Layer placed at frame 20, duration 100:
  in=20, out=120 (absolute position - unchanged)
  trim_in=0, trim_out=0  (means: no trim, play full range)

If trim_in=20:
  → skip first 20 local frames
  → effective start = in + trim_in = 20 + 20 = frame 40 in parent

If trim_in=-10:
  → extend 10 frames before layer start (if source allows)
  → effective start = 20 + (-10) = frame 10 in parent
```

### Benefits
1. **Intuitive**: trim values are relative to the clip, not parent
2. **Portable**: moving layer doesn't require recalculating trim
3. **Extensible**: negative trim = extend beyond source bounds

### Affected Code

| File | Changes |
|------|---------|
| `comp.rs:comp2local()` | Account for trim offset |
| `comp.rs:resolve_source_frame()` | Add trim to local frame |
| `comp.rs:child_play_start/end()` | Convert local trim to absolute |
| `comp.rs:set_layer_play_start/end()` | Store as local offset |
| `comp.rs:play_range()` | Use converted trim values |
| `comp.rs:add_layer()` | Initialize trim_in=0, trim_out=0 |
| `comp.rs:move_layer()` | No change to trim (local coords) |
| `timeline_ui.rs:LayerGeom` | Convert local→absolute for display |
| `timeline_ui.rs:AdjustPlayStart/End` | Set local trim values |

### New Conversion Logic

```rust
impl Comp {
    /// Get layer's effective play start in parent coordinates
    pub fn layer_play_start_abs(&self, child_uuid: Uuid) -> Option<i32> {
        let attrs = self.children_attrs_get(&child_uuid)?;
        let in_abs = attrs.get_i32("in").unwrap_or(0);
        let trim_in_local = attrs.get_i32("trim_in").unwrap_or(0);
        // Effective start = position + local trim offset
        Some(in_abs + trim_in_local)
    }

    /// Get layer's effective play end in parent coordinates
    pub fn layer_play_end_abs(&self, child_uuid: Uuid) -> Option<i32> {
        let attrs = self.children_attrs_get(&child_uuid)?;
        let out_abs = attrs.get_i32("out").unwrap_or(0);
        let trim_out_local = attrs.get_i32("trim_out").unwrap_or(0);
        // If trim_out=0, use full duration; else apply offset from end
        Some(out_abs + trim_out_local)
    }

    /// Set layer trim start in local frames
    pub fn set_layer_trim_in(&mut self, layer_uuid: Uuid, local_trim: i32) {
        if let Some(attrs) = self.children_attrs_get_mut(&layer_uuid) {
            attrs.set("trim_in", AttrValue::Int(local_trim));
        }
        // ... emit event ...
    }
}
```

### Migration
Existing projects with absolute trim values need migration:
```rust
// On load, if old format detected:
let old_trim_in = attrs.get_i32("trim_in").unwrap_or(in_abs);
let new_trim_in = old_trim_in - in_abs;  // Convert to local
attrs.set("trim_in", AttrValue::Int(new_trim_in));
```

### Implementation Steps
- [ ] Add `layer_play_start_abs()` / `layer_play_end_abs()` methods
- [ ] Change `add_layer()` to set trim_in=0, trim_out=0
- [ ] Update `comp2local()` to apply trim offset
- [ ] Update timeline_ui.rs LayerGeom to use new methods
- [ ] Update drag handlers (AdjustPlayStart/End) for local coords
- [ ] Add migration for existing projects
- [ ] Update all Alt+[/] handlers
- [ ] Test negative trim values

---

## Issue 4: Add Loop Checkbox + Move View Buttons to Toolbar

### Current State
File: `src/widgets/timeline/timeline_ui.rs:72-126` (`render_toolbar`)

Current toolbar:
```
[<<] [>] [||] [>>] | Zoom: [====] [Reset] [Fit] [x] Snap [x] Lock
```

**Missing**: Loop checkbox (only hotkey `` ` `` works)
**Separate row**: View: [Split] [Canvas] [Outline]

### Solution
Add Loop checkbox next to Lock, then View selector:

```rust
pub fn render_toolbar(ui: &mut Ui, state: &mut TimelineState, player: &Player, dispatch: ...) {
    ui.horizontal(|ui| {
        // Transport buttons...
        // Zoom controls...

        if ui.checkbox(&mut state.snap_enabled, "Snap").changed() {
            dispatch(Box::new(TimelineSnapChangedEvent(state.snap_enabled)));
        }
        if ui.checkbox(&mut state.lock_work_area, "Lock").changed() {
            dispatch(Box::new(TimelineLockWorkAreaChangedEvent(state.lock_work_area)));
        }

        // NEW: Loop checkbox
        let mut loop_enabled = player.loop_enabled();
        if ui.checkbox(&mut loop_enabled, "Loop").changed() {
            dispatch(Box::new(SetLoopEvent(loop_enabled)));
        }

        ui.separator();

        // View selector (moved from ui.rs)
        for (label, mode) in [
            ("Split", TimelineViewMode::Split),
            ("Canvas", TimelineViewMode::CanvasOnly),
            ("Outline", TimelineViewMode::OutlineOnly),
        ] {
            if ui.selectable_label(state.view_mode == mode, label).clicked() {
                state.view_mode = mode;
            }
        }
    });
}
```

### Implementation
- [ ] Add `player: &Player` parameter to `render_toolbar`
- [ ] Add Loop checkbox with `SetLoopEvent`
- [ ] Move View selector from ui.rs into toolbar
- [ ] Remove extra `ui.horizontal` block and spacing from ui.rs
- [ ] Update call site in ui.rs

---

## Issue 5: Multi-Layer Control Propagation

### Current State
`LayerAttributesChangedEvent` has single `layer_uuid: Uuid`.

### Solution: Extend existing event
```rust
// comp_events.rs - modify existing event:
#[derive(Clone, Debug)]
pub struct LayerAttributesChangedEvent {
    pub comp_uuid: Uuid,
    pub layer_uuids: Vec<Uuid>,  // Changed from single Uuid
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub speed: f32,
}
```

### Timeline UI Change
```rust
if dirty {
    let targets = if comp.layer_selection.contains(child_uuid) {
        comp.layer_selection.clone()  // All selected
    } else {
        vec![*child_uuid]  // Just this one
    };

    dispatch(Box::new(LayerAttributesChangedEvent {
        comp_uuid: comp_id,
        layer_uuids: targets,
        visible, opacity, blend_mode, speed,
    }));
}
```

### Handler Update
```rust
// main.rs handler:
fn handle_layer_attrs_changed(&mut self, evt: LayerAttributesChangedEvent) {
    if let Some(comp) = self.project.get_comp_mut(evt.comp_uuid) {
        for layer_uuid in &evt.layer_uuids {
            comp.set_child_attr(layer_uuid, "visible", AttrValue::Bool(evt.visible));
            comp.set_child_attr(layer_uuid, "opacity", AttrValue::Float(evt.opacity));
            // ...
        }
    }
}
```

### Implementation
- [ ] Change `layer_uuid` to `layer_uuids: Vec<Uuid>` in event
- [ ] Update timeline_ui.rs to collect selected layers
- [ ] Update handler to iterate over layer_uuids

---

## Summary of Changes

| File | Changes |
|------|---------|
| `timeline_ui.rs` | 1. Grid layout for outline<br>2. Loop checkbox + View selector in toolbar<br>3. Multi-layer events |
| `ae_ui.rs` | Shift/Ctrl modifier speed |
| `comp.rs` | Local trim coords + `layer_play_start/end_abs()` methods |
| `comp_events.rs` | Change `layer_uuid` → `layer_uuids: Vec<Uuid>` |
| `ui.rs` | Remove View selector block (moved to toolbar) |
| `main.rs` | Update event handler for Vec<Uuid> |

---

## Estimated Complexity

| Issue | Complexity | Risk |
|-------|------------|------|
| 1. Grid layout | Low | Low |
| 2. Shift/Ctrl modifiers | Low | Low |
| 3. Local trim coords | **High** | Medium - affects rendering, preload, drag |
| 4. Loop + View buttons | Low | Low |
| 5. Multi-layer Vec<Uuid> | Low | Low |

---

## Recommended Order

1. **Issue 2** (Shift/Ctrl) - isolated, quick win
2. **Issue 4** (Loop + View buttons) - simple UI change
3. **Issue 5** (Vec<Uuid>) - minor event change
4. **Issue 1** (Grid) - UI refactor
5. **Issue 3** (Local trim) - most complex, save for last

---

## Awaiting Approval

Confirm:
1. Is the local trim coordinate system clear?
2. Should migration be automatic or require manual project re-save?
3. Ready to start implementation?
