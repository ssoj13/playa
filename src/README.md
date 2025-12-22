# Source Code Structure

## Overview

```
src/
├── bin/            # Standalone binary entry points
├── core/           # Core engine (playback, caching, events)
├── dialogs/        # Modal dialogs (preferences, encoder)
├── entities/       # Data models (comp, frame, project)
├── server/         # REST API server for remote control
├── widgets/        # UI widgets (timeline, viewport, status)
│
├── lib.rs          # Library crate entry point
├── main.rs         # Main application entry point
├── main_events.rs  # Application event handlers
├── cli.rs          # Command-line argument parsing
├── config.rs       # Application configuration
├── shell.rs        # OS shell integration (drag-drop, recent files)
├── ui.rs           # UI utility functions
└── utils.rs        # General utilities
```

## Modules

### `core/` - Engine

Core playback engine, independent of UI. Can be used as a library.

| File | Description |
|------|-------------|
| `cache_man.rs` | Global memory manager with LRU eviction limits |
| `event_bus.rs` | Type-erased event system for decoupled communication |
| `global_cache.rs` | Frame cache with nested HashMap (comp_uuid -> frame_idx -> Frame) |
| `player.rs` | Playback state machine (play, pause, stop, seek, loop) |
| `player_events.rs` | Player-related events (SetFrame, Play, Stop, etc.) |
| `project_events.rs` | Project-related events (AddClips, RemoveComp, etc.) |
| `workers.rs` | Work-stealing thread pool for background frame loading |

### `entities/` - Data Models

Core data structures representing the compositing model.

| File | Description |
|------|-------------|
| `attrs.rs` | Generic attribute container (key-value with types) |
| `attr_schemas.rs` | Attribute schema definitions (DAG/display/keyframable flags) |
| `comp.rs` | Composition - timeline with children, work area, caching |
| `comp_node.rs` | CompNode - composition as a node with layers |
| `effects/` | Layer effects system (blur, brightness, HSV adjust) |
| `comp_events.rs` | Comp-related events (dirty flag, child updates) |
| `compositor.rs` | CPU frame blending (blend modes, alpha compositing) |
| `file_node.rs` | FileNode - image/video file source |
| `frame.rs` | Frame buffer (U8/F16/F32), loading, crop, tonemap |
| `gpu_compositor.rs` | GPU-accelerated compositing via wgpu |
| `keys.rs` | Attribute key constants (A_IN, A_SPEED, etc.) |
| `loader.rs` | Image format loaders (PNG, EXR, JPEG, etc.) |
| `loader_video.rs` | Video frame extraction via FFmpeg |
| `node.rs` | Node trait + NodeKind enum (enum_dispatch) |
| `camera_node.rs` | CameraNode - pan/zoom/rotate transform |
| `text_node.rs` | TextNode - rasterized text via cosmic-text |
| `project.rs` | Project container (media library, active comp, settings) |

### `widgets/` - UI Components

Reusable egui widgets for the application interface.

| Folder | Description |
|--------|-------------|
| `viewport/` | Image display, pan/zoom, shader preview, scrubber |
| `timeline/` | Timeline editor, layers, work area, keyframes |
| `status/` | Status bar, memory usage, cache stats |
| `project/` | Project/playlist panel |
| `ae/` | After Effects-style attribute editor + effects UI |

### `dialogs/` - Modal Windows

Modal dialog windows for specific tasks.

| Folder | Description |
|--------|-------------|
| `prefs/` | Preferences dialog (cache, playback, shortcuts) |
| `encode/` | Export/encode dialog (FFmpeg integration) |

### `server/` - REST API

HTTP REST API server for remote control of playa.

| File | Description |
|------|-------------|
| `mod.rs` | Module entry, re-exports, architecture docs |
| `api.rs` | HTTP handlers, ApiServer, ApiCommand, SharedApiState |

**Architecture:**
- Runs in background thread (rouille sync HTTP server)
- Commands sent to main thread via `mpsc::channel`
- State snapshots (`SharedApiState`) updated by main thread each frame
- CORS enabled for browser access

**Endpoints:**
- `GET /api/status` - full status (player/comp/cache)
- `GET /api/player` - player state only
- `POST /api/player/play|pause|stop` - control playback
- `POST /api/player/frame/{n}` - seek to frame
- `POST /api/player/fps/{n}` - set FPS

**Settings:** Enable/disable and port in Settings -> Web Server

### `bin/` - Standalone Binaries

Development/debug binaries for testing individual components.

| File | Description |
|------|-------------|
| `viewport.rs` | Standalone viewport window |
| `timeline.rs` | Standalone timeline window |
| `project.rs` | Standalone project panel |
| `encoder.rs` | Standalone encoder dialog |
| `prefs.rs` | Standalone preferences dialog |
| `attributes.rs` | Standalone attributes editor |

## Architecture Notes

### Node System (enum_dispatch)

All compositing elements implement the `Node` trait via `enum_dispatch` for zero-cost polymorphism:

```rust
#[enum_dispatch(Node)]
pub enum NodeKind {
    FileNode,    // Image/video source
    CompNode,    // Composition with layers
    CameraNode,  // Pan/zoom/rotate transform
    TextNode,    // Rasterized text
}
```

**Node trait provides:**
- `compute(frame, ctx)` - render frame at given time
- `attrs()` / `attrs_mut()` - attribute access
- `play_range()` - visible frame range after trims
- `bounds()` - spatial bounding box
- `is_dirty()` / `mark_dirty()` - cache invalidation

**Attribute schemas** (`attr_schemas.rs`) define per-attribute flags:
- `DAG` - changes invalidate render cache
- `DISP` - show in Attribute Editor
- `KEY` - keyframable

Non-DAG attributes (e.g. `node_pos` for Node Editor positions) don't trigger recompute.

### Event-Driven Communication

Components communicate via `EventBus` with typed events:
- Widgets emit events (e.g., `SetFrameEvent`)
- `main_events.rs` handles events and updates state
- Avoids tight coupling between UI and logic

### Frame Loading Pipeline

1. `Player::get_current_frame()` checks `GlobalFrameCache`
2. Cache miss → `Comp::get_frame()` creates placeholder
3. `Workers` load frame in background with epoch check
4. Loaded frame inserted into cache
5. Next render picks up cached frame

### Memory Management

- `CacheManager` tracks global memory usage
- `GlobalFrameCache` evicts LRU frames when limit exceeded
- Epoch mechanism cancels stale preload requests during scrubbing
