# Project Window Analysis Report - Part 2

## Debug Session: Why Buttons Don't Work

### Build Issue
Both `cargo run --release --bin playa` and `cargo run --bin project` fail with:
```
error: failed to run custom build command for `ffmpeg-sys-next v8.0.1`
```
This is an environment/toolchain issue with ffmpeg-sys-next build script, not a code issue.

---

## Static Code Analysis

### 1. Event Flow Architecture (New vs Old)

**Old architecture (playa.old):**
```rust
// project.rs
pub struct ProjectActions {
    pub new_comp: bool,
    pub save_project: Option<PathBuf>,
    pub load_project: Option<PathBuf>,
    // ... direct fields
}

// project_ui.rs
if ui.button("Add Comp").clicked() {
    actions.new_comp = true;  // Direct field assignment
}
```

**New architecture (current):**
```rust
// project.rs
pub struct ProjectActions {
    pub hovered: bool,
    pub events: Vec<BoxedEvent>,  // EventBus pattern
}

// project_ui.rs
if ui.button("Add Comp").clicked() {
    actions.send(AddCompEvent { ... });  // Event dispatched
}
```

The new architecture is cleaner and follows the EventBus pattern from `arch.md`, but introduces a **frame delay**.

### 2. Frame Timing Issue

**Order of operations in `update()`:**
```
1. handle_events()     [line 941]  - processes events from PREVIOUS frame
2. player.update()     [line 1004]
3. handle_events()     [line 1007] - processes CurrentFrameChanged etc.
4. DockArea render     [line 1057] - calls render_project_tab
   └─ project_ui::render -> events added to EventBus
5. handle_keyboard     [line 1069]
6. NEXT FRAME...
```

**Critical observation:** Events from UI (step 4) are added to EventBus AFTER `handle_events()` calls (steps 1,3). They will be processed on the NEXT frame.

### 3. Repaint Trigger Problem

```rust
// main.rs:1023-1025
if self.player.is_playing() {
    ctx.request_repaint();
}
```

**Problem:** `request_repaint()` is only called when player is playing! If player is paused, egui relies on automatic repaint from user interaction.

**However:** egui should automatically repaint when UI elements are clicked. This is standard egui behavior.

### 4. Logging Level

```rust
// main.rs:1174-1179
let log_level = match args.verbosity {
    0 => log::LevelFilter::Warn,   // DEFAULT - won't show info!
    1 => log::LevelFilter::Info,
    2 => log::LevelFilter::Debug,
    _ => log::LevelFilter::Trace,
};
```

**All my debug logging uses `log::info!`** which is NOT visible by default!

**To see logs, run with:** `playa.exe -v`

---

## Verification Checklist

When you can run the app, verify these steps:

1. [ ] **Run with `-v` flag:** `playa.exe -v` to see info-level logs
2. [ ] **Click "Add Comp" button**
3. [ ] **Check for log:** `[PROJECT_UI] Add Comp button clicked!`
4. [ ] **Check for log:** `[MAIN] Project UI generated X events`
5. [ ] **Check for log:** `[MAIN] Emitting event: ...AddCompEvent`
6. [ ] **Check for log (next frame):** `[MAIN_EVENTS] AddCompEvent received`
7. [ ] **Check for log:** `Created new comp: <uuid>`

If step 3 appears but not step 4 - problem is in `render_project_tab`
If step 4 appears but not step 6 - problem is in event processing timing
If step 6 appears but comp doesn't appear - problem is in UI rendering

---

## Issues Found (from previous analysis)

### ISSUE 1: Redundant `comps_order()` call in loop
**File:** `src/widgets/project/project_ui.rs:93-94`
```rust
for comp_uuid in &all_comps {
    let comps_order = project.comps_order();  // Called EVERY iteration!
```
**Impact:** Performance (N calls instead of 1)
**Fix:** Move outside loop

### ISSUE 2: Double selection event on double-click
**File:** `src/widgets/project/project_ui.rs:240-268`
```rust
if response.clicked() {
    // Sends ProjectSelectionChangedEvent
}
if response.double_clicked() {
    // Sends ANOTHER ProjectSelectionChangedEvent (duplicate!)
    // Then sends ProjectActiveChangedEvent
}
```
**Impact:** Two identical selection events on double-click
**Fix:** Use `else if` or skip selection in double-click

### ISSUE 3: Unused `AddClipEvent`
**File:** `src/project_events.rs:9`
```rust
pub struct AddClipEvent(pub std::path::PathBuf);  // Single file
```
vs `AddClipsEvent` (multiple files) - `AddClipEvent` appears unused.

### ISSUE 4: GlobalDragState not cleared on cancel
**File:** `src/widgets/project/project_ui.rs:272-284`
Drag state is set on `drag_started()` but never cleared if drag is cancelled.

### ISSUE 5: `selection_anchor` not persisted
Runtime-only field, lost on save/load. May cause unexpected shift-click behavior after reload.

### POTENTIAL DEADLOCK
**File:** `src/widgets/project/project_ui.rs:88`
```rust
let media = project.media.read().unwrap();
```
Read lock held for entire ScrollArea rendering. If any button handler tries to write - deadlock.

---

## Recommendations

### Immediate (for debugging)

1. **Add unconditional request_repaint after UI events:**
```rust
// In render_project_tab, after emitting events:
if !project_actions.events.is_empty() {
    ui.ctx().request_repaint();  // Force repaint for next frame
}
```

2. **Run with verbose logging:**
```
playa.exe -v  # or -vv for debug level
```

### Architectural

3. **Process UI events within same frame:**
Add third `handle_events()` call AFTER DockArea rendering:
```rust
// After DockArea::show_inside()
self.handle_events();  // Process UI-generated events immediately
```

4. **Or switch to direct action execution for simple buttons:**
Keep EventBus for cross-component communication but handle simple actions directly:
```rust
if ui.button("Add Comp").clicked() {
    // Direct execution instead of event
    let uuid = project.create_comp(...);
    player.set_active_comp(Some(uuid), project);
}
```

---

## Code Added for Debugging

**project_ui.rs:53:**
```rust
log::info!("[PROJECT_UI] Add Comp button clicked!");
```

**main.rs:630-635:**
```rust
if !project_actions.events.is_empty() {
    log::info!("[MAIN] Project UI generated {} events", project_actions.events.len());
}
for evt in project_actions.events {
    log::info!("[MAIN] Emitting event: {}", evt.type_name());
    self.event_bus.emit_boxed(evt);
}
```

**main_events.rs:212:**
```rust
log::info!("[MAIN_EVENTS] AddCompEvent received: name={}, fps={}", e.name, e.fps);
```

---

## Dataflow Diagram (Updated)

```
┌──────────────────────────────────────────────────────────────────┐
│                         FRAME N                                   │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. handle_events()  ◄─── processes events from frame N-1        │
│         │                                                         │
│         ▼                                                         │
│  2. player.update()                                               │
│         │                                                         │
│         ▼                                                         │
│  3. handle_events()  ◄─── processes CurrentFrameChanged etc.     │
│         │                                                         │
│         ▼                                                         │
│  4. DockArea::show_inside()                                       │
│         │                                                         │
│         ├─► render_project_tab()                                  │
│         │       │                                                 │
│         │       ├─► project_ui::render()                         │
│         │       │       │                                         │
│         │       │       ├─► Button clicked?                      │
│         │       │       │       │                                 │
│         │       │       │       ▼                                │
│         │       │       │   actions.send(AddCompEvent)           │
│         │       │       │                                         │
│         │       │       └─► return ProjectActions                │
│         │       │                                                 │
│         │       └─► event_bus.emit_boxed(evt)  ───┐              │
│         │                                          │              │
│         │                                          │              │
│  5. handle_keyboard_input()                        │              │
│                                                    │              │
└────────────────────────────────────────────────────│──────────────┘
                                                     │
                                                     ▼
┌──────────────────────────────────────────────────────────────────┐
│                         FRAME N+1                                 │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. handle_events()  ◄─── NOW processes AddCompEvent             │
│         │                                                         │
│         ├─► result.new_comp = Some((name, fps))                  │
│         │                                                         │
│         ▼                                                         │
│     (after event loop)                                            │
│         │                                                         │
│         ├─► project.create_comp()                                │
│         │       │                                                 │
│         │       ├─► Comp::new()                                  │
│         │       ├─► project.add_comp()                           │
│         │       │       │                                         │
│         │       │       ├─► media.write().insert()               │
│         │       │       └─► comps_order.push()                   │
│         │       │                                                 │
│         │       └─► return uuid                                  │
│         │                                                         │
│         └─► player.set_active_comp(uuid)                         │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

**Key insight:** There is a 1-frame delay between button click and action execution. This is normal for EventBus pattern, BUT requires proper repaint scheduling.

---

## Next Steps

1. Fix ffmpeg-sys-next build issue (environment setup)
2. Run app with `-v` flag and test buttons
3. If events are being processed correctly but UI doesn't update - add `request_repaint()`
4. If events are not being processed - investigate EventBus queue
5. Consider adding same-frame event processing for better responsiveness

---

## Status

- [x] Dataflow analysis complete
- [x] Code issues identified  
- [ ] Root cause confirmed (requires running app)
- [ ] Fix implemented
- [ ] Fix verified
