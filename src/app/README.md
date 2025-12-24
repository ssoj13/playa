# App Module Structure

The `app/` module contains the main `PlayaApp` application logic, organized by responsibility.

## Module Overview

```
src/app/
  mod.rs        - PlayaApp struct, DockTab enum, Default impl
  events.rs     - Event handling (handle_events, hotkeys, effect actions)
  api.rs        - REST API server (start, update state, handle commands)
  project_io.rs - Project/sequence loading and saving
  layout.rs     - Dock layout management (save/load/reset, named layouts)
  tabs.rs       - Tab rendering (render_*_tab) + DockTabs TabViewer
  run.rs        - eframe::App impl (update loop, save, on_exit)
```

## Dataflow

```
                          +------------------+
                          |   main.rs        |
                          |  (entry point)   |
                          +--------+---------+
                                   |
                                   v
                          +------------------+
                          |   PlayaApp       |
                          |  (app/mod.rs)    |
                          +--------+---------+
                                   |
         +------------+------------+------------+------------+
         |            |            |            |            |
         v            v            v            v            v
    +--------+   +--------+   +--------+   +--------+   +--------+
    | events |   |  api   |   |project |   | layout |   |  tabs  |
    |   .rs  |   |   .rs  |   | _io.rs |   |   .rs  |   |   .rs  |
    +--------+   +--------+   +--------+   +--------+   +--------+
         |            |            |            |            |
         v            v            v            v            v
    +----------------------------------------------------------------+
    |                        EventBus                                 |
    |   (decoupled event-driven communication between components)     |
    +----------------------------------------------------------------+
```

## Event Flow

```
User Action
    |
    v
+-------------------+     +------------------+
| UI Widget Events  | --> |   EventBus       |
| (viewport, timeline)    |   .emit(Event)   |
+-------------------+     +--------+---------+
                                   |
                                   v
                          +------------------+
                          | handle_events()  |
                          | (events.rs)      |
                          +--------+---------+
                                   |
              +--------------------+--------------------+
              |                    |                    |
              v                    v                    v
      +-------------+      +-------------+      +-------------+
      | SetFrame    |      | Attrs       |      | Viewport    |
      | Event       |      | Changed     |      | Refresh     |
      +-------------+      +-------------+      +-------------+
              |                    |                    |
              v                    v                    v
      +-------------+      +-------------+      +-------------+
      | load frame  |      | invalidate  |      | request     |
      | from cache  |      | cache       |      | repaint     |
      +-------------+      +-------------+      +-------------+
```

## Key Components

### PlayaApp (mod.rs)
- Main application state struct
- Contains all UI state, player, project, cache manager
- `Default::default()` creates initial state with workers, event bus

### Events (events.rs)
- `handle_events()` - main event dispatcher
- `handle_keyboard_input()` - hotkey routing via HotkeyHandler
- `handle_effect_actions()` - layer effects modifications

### API (api.rs)
- REST API for remote control (playback, screenshot, etc.)
- `start_api_server()` - lazy init on first frame
- `update_api_state()` - snapshot current state for clients
- `handle_api_commands()` - process incoming commands

### Project IO (project_io.rs)
- `load_sequences()` - load files/folders into project
- `save_project()` / `load_project()` - JSON serialization
- `enqueue_*` - frame preloading logic

### Layout (layout.rs)
- Dock panel visibility sync
- Save/load layout to project attrs
- Named layouts (create, apply, delete, rename)

### Tabs (tabs.rs)
- `render_*_tab()` methods for each dock panel
- `DockTabs` wrapper for egui_dock TabViewer

### Run (run.rs)
- `impl eframe::App for PlayaApp`
- `update()` - main frame loop
- `save()` - persist to storage
- `on_exit()` - cleanup
