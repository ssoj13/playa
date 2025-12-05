# Project Window Analysis Report

## Overview

Analysis of the Project panel dataflow, event handling, and potential issues.

**Files analyzed:**
- `src/widgets/project/project.rs` - ProjectActions struct
- `src/widgets/project/project_ui.rs` - UI rendering
- `src/entities/project.rs` - Project entity
- `src/project_events.rs` - Event definitions
- `src/main_events.rs` - Event handlers
- `src/event_bus.rs` - EventBus implementation
- `src/bin/project.rs` - Standalone test binary

---

## Dataflow Diagram

```
                              PROJECT WINDOW DATAFLOW
    
    +-------------------+
    |   User Actions    |
    | (click, drag,     |
    |  buttons)         |
    +--------+----------+
             |
             v
    +-------------------+
    | project_ui.rs     |
    | render()          |
    +--------+----------+
             |
             | Creates events via actions.send(EventType)
             v
    +-------------------+
    | ProjectActions    |
    | {                 |
    |   hovered: bool,  |
    |   events: Vec<    |
    |     BoxedEvent>   |
    | }                 |
    +--------+----------+
             |
             | Returned to caller (main.rs or bin/project.rs)
             v
    +-------------------+
    | Main Event Loop   |
    | for evt in        |
    |   actions.events  |
    |   event_bus.      |
    |     emit_boxed()  |
    +--------+----------+
             |
             | Events queued in EventBus
             v
    +-------------------+
    | EventBus.poll()   |
    | Returns all       |
    | queued events     |
    +--------+----------+
             |
             v
    +-------------------+
    | handle_app_event()|
    | main_events.rs    |
    +--------+----------+
             |
             | Returns EventResult with deferred actions:
             | - load_project: Option<PathBuf>
             | - save_project: Option<PathBuf>
             | - load_sequences: Option<Vec<PathBuf>>
             | - new_comp: Option<(String, f32)>
             | - quick_save: bool
             | - show_open_dialog: bool
             v
    +-------------------+
    | Deferred Actions  |
    | Handler           |
    | (file I/O, etc)   |
    +-------------------+
```

---

## Event Flow Details

### 1. Button Actions

| Button | Event | Handler Action |
|--------|-------|----------------|
| Save | `SaveProjectEvent(PathBuf)` | `result.save_project = path` -> `project.to_json()` |
| Load | `LoadProjectEvent(PathBuf)` | `result.load_project = path` -> `Project::from_json()` |
| Add Clip | `AddClipsEvent(Vec<PathBuf>)` | `result.load_sequences = paths` -> `Comp::detect_from_paths()` |
| Add Comp | `AddCompEvent{name, fps}` | `result.new_comp = (name, fps)` -> `project.create_comp()` |
| Clear All | `ClearAllMediaEvent` | Clears media HashMap and order |
| Delete (X) | `RemoveMediaEvent(Uuid)` | `project.remove_media_with_cleanup()` |

### 2. Selection Events

| Action | Event | Handler |
|--------|-------|---------|
| Single click | `ProjectSelectionChangedEvent{selection, anchor}` | `project.set_selection()` |
| Double click | `ProjectSelectionChangedEvent` + `ProjectActiveChangedEvent(Uuid)` | set_selection + `player.set_active_comp()` |

### 3. Drag Events

```
drag_started() -> GlobalDragState::ProjectItem { source_uuid, duration }
                  (stored in egui context data)
                  
dragged()     -> cursor icon = Grabbing
hovered()     -> cursor icon = Grab

Timeline receives drop -> AddLayerEvent { comp_uuid, source_uuid, start_frame, target_row }
```

---

## Issues Found

### CRITICAL: Potential Deadlock

**Location:** `project_ui.rs:87`

```rust
let media = project.media.read().unwrap();
for comp_uuid in &all_comps {
    let comp = match media.get(comp_uuid) { ... };
    // ... renders entire list while holding read lock
}
```

**Problem:** Read lock held for entire render loop. If any immediate event callback (via EventBus.subscribe) tries to get write lock on `project.media`, deadlock occurs.

**Risk:** Medium-High. Currently no immediate subscribers modify media, but architecture allows it.

---

### ISSUE 1: Redundant comps_order() Call Inside Loop

**Location:** `project_ui.rs:93`

```rust
for comp_uuid in &all_comps {
    // ...
    let comps_order = project.comps_order();  // <-- Called EVERY iteration!
    let clicked_idx = match comps_order.iter().position(|u| u == comp_uuid) { ... };
```

**Problem:** `comps_order()` deserializes JSON from attrs on every call. Called N times for N items.

**Fix:** Use `all_comps` which already has the same data:
```rust
let clicked_idx = match all_comps.iter().position(|u| u == comp_uuid) { ... };
```

---

### ISSUE 2: Double Selection Event on Double-Click

**Location:** `project_ui.rs:240-268`

```rust
if response.clicked() {
    // ... sends ProjectSelectionChangedEvent
}
if response.double_clicked() {
    // ... sends ANOTHER ProjectSelectionChangedEvent  <-- Duplicate!
    // ... sends ProjectActiveChangedEvent
}
```

**Problem:** egui fires both `clicked()` and `double_clicked()` for double-click. Selection event sent twice.

**Fix:** Use `else if` or check for double_click first:
```rust
if response.double_clicked() {
    // selection + activation
} else if response.clicked() {
    // selection only
}
```

---

### ISSUE 3: Unused Event Type

**Location:** `project_events.rs:9`

```rust
#[derive(Clone, Debug)]
pub struct AddClipEvent(pub PathBuf);  // <-- Never used anywhere
```

**Problem:** Dead code. Only `AddClipsEvent(Vec<PathBuf>)` is used.

**Fix:** Remove `AddClipEvent` or use it for single-file drops.

---

### ISSUE 4: GlobalDragState Not Cleared on Drag Cancel

**Location:** `project_ui.rs:271-283`

```rust
if response.drag_started() {
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("global_drag_state"), GlobalDragState::ProjectItem { ... });
    });
}
```

**Problem:** If drag is cancelled (ESC, click elsewhere), the temp data remains until overwritten.

**Fix:** Clear on drag_stopped() or at start of next frame if no drag active.

---

### ISSUE 5: selection_anchor Runtime-Only Field

**Location:** `entities/project.rs:39`

```rust
#[serde(skip)]
#[serde(default)]
pub selection_anchor: Option<usize>,
```

**Problem:** Not persisted. After project save/load, shift-click range selection breaks because anchor is lost.

**Decision needed:** Should anchor be persisted? Or is it acceptable to reset on load?

---

## Architecture Notes

### Comparison: Old vs New Code

| Aspect | Old (playa.old) | New (playa) |
|--------|-----------------|-------------|
| UUID type | `String` | `uuid::Uuid` |
| Actions struct | Direct fields (`load_sequence`, `save_project`, etc.) | Events only (`Vec<BoxedEvent>`) |
| Project access | `player.project` | Separate `&Project` parameter |
| Event system | `Vec<AppEvent>` enum | `EventBus` with typed events |

**Benefits of new architecture:**
- Type-safe events (no enum matching)
- Decoupled components via EventBus
- Cleaner separation of concerns
- Easier to add new event types

**Tradeoffs:**
- More boilerplate (event structs)
- Runtime type checking via `downcast_event`
- Slightly more complex event flow

---

## Recommendations

### Priority 1 (Should Fix)

1. **Fix redundant comps_order() call** - Easy performance win
2. **Fix double selection event** - Causes unnecessary redraws

### Priority 2 (Good to Fix)

3. **Remove unused AddClipEvent** - Dead code cleanup
4. **Add drag state cleanup** - Better UX

### Priority 3 (Consider)

5. **Review read lock scope** - Consider cloning data before render to release lock earlier
6. **Document selection_anchor behavior** - Clarify if reset-on-load is intentional

---

## Test Checklist

- [ ] Single click selects item
- [ ] Shift+click extends selection
- [ ] Ctrl+click toggles selection
- [ ] Double-click activates item
- [ ] Drag to timeline creates layer
- [ ] Delete button removes item
- [ ] Save/Load preserves selection
- [ ] Clear All empties everything
- [ ] No flickering on selection change

---

*Report generated: 2024-12-05*
*Files reviewed: 7*
*Issues found: 5*
