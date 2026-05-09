# Architecture

**Analysis Date:** 2026-05-09

Playa is an image-sequence player (EXR / PNG / JPEG / TIFF / video) written in Rust 2024.
The repository is a Cargo workspace; the binary `playa` is a thin aggregator that wires together
five library crates plus an `xtask` build helper. The Python bindings crate `playa-py` is
deliberately excluded from the workspace.

## Pattern Overview

**Overall:** layered, event-driven desktop application with a node-graph engine.

**Key Characteristics:**
- Strict layering through Cargo crate boundaries (compiler-enforced).
- All cross-layer communication goes through one typed `EventBus` — widgets never call each other or `PlayaApp` directly.
- Node graph dispatched via `enum_dispatch` (no `Box<dyn Node>`).
- Schema-aware attribute system: changing a `FLAG_DAG` attribute auto-invalidates the cache.
- Work-stealing thread pool (no Tokio) with epoch-based cancellation.
- LRU global frame cache with a memory budget.
- CPU compositor on workers; GPU compositor confined to the UI thread via a bridge.
- Persistence via `eframe::APP_KEY` (auto-saved JSON) plus separate `Project` JSON for playlists.

## Crate Layering

```
                ┌────────────────────────────────────────────┐
                │  playa  (root crate)                       │
                │  src/main.rs   src/lib.rs (re-exports)     │
                └──────────────┬─────────────────────────────┘
                               │ depends on
                               ▼
                ┌────────────────────────────────────────────┐
                │  playa-app    (host / orchestration)       │
                │  PlayaApp · main_events · runner · cli ·   │
                │  config · server (REST) · shell            │
                └──────┬─────────────────┬─────────────┬─────┘
                       │                 │             │
                       ▼                 ▼             │
   ┌────────────────────────────┐  ┌──────────────┐   │
   │  playa-ui   (egui widgets) │  │ playa-engine │   │
   │  widgets · dialogs · ui    │  │ core /       │   │
   │  help                      │  │ entities /   │   │
   └────────┬───────────────────┘  │ defaults /   │   │
            │                      │ utils /      │   │
            │                      │ render_gpu   │   │
            ▼                      └──┬───────────┘   │
        ┌──────────────┐               │              │
        │ playa-events │◀──────────────┴──────────────┘
        │ (typed evts  │   used by every layer
        │  + EventBus) │
        └──────┬───────┘
               │
               ▼
        ┌────────────────────────────────────────────────┐
        │  playa-io   (loaders: EXR, image crate, ffmpeg)│
        │  feature-gated: `exr`, `ffmpeg`, `webcodecs`   │
        └────────────────────────────────────────────────┘
```

- `playa-events` is leaf-most: no engine/UI/app deps, just `serde`, `uuid`, `log`. Everyone depends on it.
- `playa-engine` depends on `playa-io` (loaders) and `playa-events`.
- `playa-ui` depends on `playa-engine`, `playa-io`, `playa-events` and the egui stack (`eframe`, `egui_dock`, `egui-snarl`, `egui_dnd`, `transform-gizmo-egui`, etc.).
- `playa-app` depends on all four library crates; it is the only place that owns `PlayaApp` and runs `eframe::run_native`.
- The root crate `playa` only re-exports public surfaces (`src/lib.rs`) and exposes `playa::run_app` for both the binary and the Python bindings.

## Layers

**`playa-events` (cross-cutting messaging):**
- Purpose: typed event structs + `EventBus` (deferred queue, immediate callbacks, blanket `impl<T> Event for T`).
- Location: `crates/playa-events/src/`
- Key files: `bus.rs` (EventBus, EventEmitter, downcast), one module per domain (`player.rs`, `comp.rs`, `timeline.rs`, `viewport.rs`, `viewport_tool.rs`, `node_editor.rs`, `prefs.rs`, `project_media.rs`, `layout.rs`).
- Note: `event_bus::downcast_event` lives in `crates/playa-engine/src/core/event_bus.rs` and uses `(**event).as_any()` to defeat the blanket `impl<T> Event for T` shadowing on `Box<dyn Event>`.

**`playa-io` (media decoding):**
- Purpose: format dispatch and decoding behind cargo features.
- Location: `crates/playa-io/src/`
- Features: `exr` (vfx-rs path-deps), `ffmpeg` (playa-ffmpeg static), `webcodecs` (Wasm scaffold).
- Key files: `dispatch.rs` (header_attrs / decode_raster), `exr_layered.rs`, `video/{ffmpeg_imp,stub}.rs`, `source_image/{native,stub}.rs`, `pixel.rs`, `media.rs`, `error.rs`.
- Exposes `init_ffmpeg()` called once from `src/main.rs`.

**`playa-engine` (data + compute):**
- Purpose: cache, playback state, node graph, attributes, compositor, loader dispatch.
- Location: `crates/playa-engine/src/`
- Two main subsystems (dependency-inverted):
  - `entities/` — domain types: `Project`, `CompNode`, `FileNode`, `CameraNode`, `TextNode`, `Layer`, `Frame`, `Attrs`, `attr_schemas`, `effects/{blur,brightness,hsv}`, `compositor`, `gpu_blend_bridge`, `loader`, `space`, `transform`, `node`, `node_kind`, `traits` (FrameCache / WorkerPool / CacheStrategy / ComputeContext).
  - `core/` — infrastructure that implements those traits: `global_cache`, `cache_man` (memory budget + epoch), `workers`, `event_bus`, `player`, `debounced_preloader`, `player_events`, `layout_events`.
- `defaults.rs` — startup defaults; `utils.rs` — small helpers; `render_gpu/wgpu_compositor.rs` — wgpu-backed GPU compositor.
- Depends on: nothing UI-related. Tests can run without egui.

**`playa-ui` (egui presentation):**
- Purpose: widgets, dialogs, main menu, help overlay. No application state — only widget state.
- Location: `crates/playa-ui/src/`
- Key trees:
  - `widgets/viewport/` — viewport (gizmo, picker, coords, GL renderer, shaders, tool).
  - `widgets/timeline/` — After-Effects-style timeline (state, helpers, ui, events).
  - `widgets/project/` — project panel (list of clips/comps + DnD source).
  - `widgets/node_editor/` — egui-snarl graph view of comps.
  - `widgets/ae/` — Attribute Editor (generic property editor).
  - `widgets/status/` — status bar + cache progress bar.
  - `widgets/{actions,dnd,file_dialogs}.rs` — cross-widget drag payloads, action enums, native file pickers.
  - `dialogs/encode/` — encoder dialog (gated to native; wasm stub).
  - `dialogs/prefs/` — preferences (settings, hotkey handler, prefs events).
  - `ui.rs` — main menu/composition root for widgets; `help.rs` — help overlay.

**`playa-app` (orchestration / shell):**
- Purpose: composes everything, owns `PlayaApp`, runs main loop, routes events, hosts REST API.
- Location: `crates/playa-app/src/`
- Key files:
  - `app/mod.rs` — `PlayaApp` struct + `DockTab` + dock builders.
  - `app/run.rs` — implements `eframe::App::update` (the main loop steps).
  - `app/events.rs` — `handle_events`, keyboard input, action handlers.
  - `app/api.rs` — wiring `SharedApiState` ↔ main loop.
  - `app/project_io.rs` — load/save project JSON, drag-drop ingest.
  - `app/tabs.rs` — `DockTabs` impl for `egui_dock::TabViewer`.
  - `app/layout.rs` — layout save/load handlers.
  - `main_events.rs` — central `handle_app_event` (project mutations, cache invalidation).
  - `runner.rs` — `run_app(args)`: builds eframe options, persistence path, spawns ffmpeg init.
  - `cli.rs` — `clap` argument struct.
  - `config.rs` — `PathConfig`, `config_file` / `data_file` helpers, portable-mode detection.
  - `server/{mod,api}.rs` — rouille HTTP server, `ApiCommand` mpsc, `SharedApiState`.
  - `shell.rs` — desktop integration helpers (open path, etc.).

## Event-driven core

Widgets and dialogs do not call each other or `PlayaApp` directly. They emit typed events into `EventBus`:

```text
emit::<E>(event)
       │
       ├── immediate callbacks (registered subscribers)
       └── deferred VecDeque<BoxedEvent> (capped at 1000)
                          │
                          ▼
              PlayaApp::update → handle_events
                          │
                          ▼
              main_events::handle_app_event(ctx, event)
```

- Event categories live next to the domain: `core/player_events.rs`, `core/layout_events.rs`, `entities/comp_events.rs` in `playa-engine`; `widgets/<X>/<X>_events.rs` and `dialogs/prefs/prefs_events.rs` in `playa-ui`; mirrors live in `playa-events/src/*` for the shared bus.
- Downcast via `playa_engine::core::event_bus::downcast_event::<E>(&event)`. Always uses `(**event).as_any()` — see surprises in `AGENTS.md`.

## Node graph (enum_dispatch)

```rust
#[enum_dispatch(Node)]
pub enum NodeKind { File(FileNode), Comp(CompNode), Camera(CameraNode), Text(TextNode) }
```

- Trait: `crates/playa-engine/src/entities/node.rs` (`Node`, `ComputeContext`).
- Variants: `entities/{file_node,comp_node,camera_node,text_node}.rs`.
- `Project.media: Arc<RwLock<HashMap<Uuid, Arc<NodeKind>>>>` — outer `Arc` lets workers snapshot the map cheaply (microseconds) and release the lock before a heavy `compute()` runs.
- `is_renderable()` returns false for cameras (they produce no pixels).

## Attrs and schema-driven invalidation

- `Attrs` (`entities/attrs.rs`) is the shared bag for Frame, Layer, Comp, Camera, Project, Effect.
- Each entity has a schema in `entities/attr_schemas.rs` listing flags `FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE | FLAG_READONLY | FLAG_INTERNAL`.
- A `set()` on a `FLAG_DAG` attr → `dirty=true`; non-DAG attrs (e.g. `frame`, node-editor positions) leave the cache untouched.
- The **only** sanctioned mutation entry-point is `Project::modify_comp(uuid, |comp| ...)`. After the closure returns, it inspects `is_dirty()` and emits `AttrsChangedEvent`. The handler in `playa-app` (`main_events.rs`) then:
  1. `cache_manager.increment_epoch()` — invalidates in-flight worker tasks.
  2. `global_cache.clear_comp(uuid)`.
  3. `DebouncedPreloader::schedule(uuid)`.

## Work-stealing workers with epochs

- Pool: `crates/playa-engine/src/core/workers.rs`. Per-worker FIFO deque + global `crossbeam::Injector`. Pool size: `num_cpus::get() * 3 / 4`.
- Loop: own deque pop → injector steal → steal from peers → shutdown? → `thread::sleep(1ms)`.
- Epochs: `Arc<AtomicU64>` shared with `CacheManager`. Workers compare task epoch against `current_epoch`; if stale, the task is dropped before any decode/composite work happens. Scrubbing therefore never floods the pool.
- Workers never touch OpenGL/wgpu directly; for GPU compositing they enqueue stacks through `GpuBlendBridge`.

## LRU cache with memory budget

- `GlobalFrameCache` (`core/global_cache.rs`):
  - `RwLock<HashMap<Uuid, HashMap<i32, Frame>>>` — per-comp sub-maps for O(1) `clear_comp`.
  - `Mutex<lru::LruCache<CacheKey, ()>>` for O(1) eviction order.
  - `Arc<CacheManager>` for memory tracking + epoch (`core/cache_man.rs`).
  - `CacheStrategy` (defined in `entities/traits.rs`): `All` (keep everything in work area) or `LastOnly`.
- `CacheManager::new(0.75, 2.0)` — 75% of `sysinfo::available_memory()` minus 2 GB system reserve.
- `dirty_repaint: AtomicBool` is set by workers after every `insert`; the main loop's `cache_manager.take_dirty()` triggers `ctx.request_repaint()` so egui doesn't sleep until the next mouse move.

## DebouncedPreloader

- `core/debounced_preloader.rs`. Holds `(comp_uuid, trigger_time)`; `tick()` returns `Some(uuid)` only after ≥ 500 ms of quiet. Prevents thrash during slider drags — only the current frame is loaded during the debounce window; the radius preload kicks in after silence.

## Dependency inversion (`ComputeContext`)

- `entities/traits.rs` declares `FrameCache`, `WorkerPool`, `CacheStrategy`, `CacheStatsSnapshot`.
- `core/global_cache.rs` and `core/workers.rs` implement those traits.
- `entities/node.rs::ComputeContext` carries `&dyn FrameCache`, `Option<&dyn WorkerPool>`, plus `Option<&GpuBlendBridge>`. Nodes never know the concrete types — tests can mock them.

## CPU vs GPU compositor

| Component | Where it runs | Used when |
|-----------|---------------|-----------|
| `CpuCompositor` (`entities/compositor.rs`, via `THREAD_COMPOSITOR` thread-local in `comp_node.rs`) | any worker thread (and main thread for blocking encode/get_frame) | project prefs = Cpu, OR Gpu prefs but bridge returned `GpuBlendReport::NotQueued` |
| `GpuCompositor` (`render_gpu/wgpu_compositor.rs`) | UI thread only (wgpu device current) | project prefs = Gpu and `GpuBlendBridge` delivered the stack |

`CompNode::compose_internal` always builds `Vec<(Frame, opacity, BlendMode, inv_matrix)>` first. With the bridge wired, it calls `GpuBlendBridge::delegate_blend_blocking`, which:
1. Workers enqueue a `GpuBlendRequest` and block on a oneshot return channel.
2. `PlayaApp::drain_gpu_blend_queue` (called every frame after `update_compositor_backend`) drains the queue into the UI-thread `GpuCompositor`, then signals workers.
3. If enqueue fails (channel closed), the worker falls back to its `THREAD_COMPOSITOR` (Cpu) path.

Blocking encode (`get_frame`) intentionally bypasses the bridge so encode jobs never stall on the UI thread.

## Main loop (`PlayaApp::update` in `crates/playa-app/src/app/run.rs`)

```text
1.  exit_requested?              → ctx.send_viewport_cmd(Close)
2.  start_api_server() (lazy)    once on first frame if enabled
3.  update_compositor_backend()  rebinds GpuCompositor to current wgpu device on switch
4.  drain_gpu_blend_queue(ctx)   unblocks workers waiting on GPU blends
5.  apply theme/font             guarded by last_applied_*
6.  handle_events()              EventBus.poll → main_events::handle_app_event
7.  player.update()              advances frame by wall-clock if playing
8.  drag-drop ingestion          add files/playlist
9.  DockArea::show(ctx, &mut DockTabs(self))
10. handle_keyboard_input()      via HotkeyHandler routed by focused_window
11. drain ApiCommand mpsc        execute REST commands on the main thread
12. update_api_state()           publish SharedApiState snapshot
13. service pending screenshots  (PNG via glReadPixels or current Frame)
14. cache_manager.take_dirty()   request_repaint if a worker delivered pixels
```

## Threading model

- No async runtime. Workers are `std::thread`, queues are `crossbeam::deque::Injector` and `crossbeam_channel`. The HTTP server is `rouille` (synchronous). egui runs on the main thread.
- Synchronisation primitives: `Arc<RwLock<…>>` for the project's media map, `Mutex` for compositor and gpu_blend_rx, `AtomicU64` for epoch, `AtomicBool` for dirty_repaint.
- Rule from `AGENTS.md`: never block the main thread; long work goes to `Workers::execute`.

## Coordinate spaces

```
+──────────────+   +──────────────────────+   +──────────────+
│ IMAGE        │   │ FRAME (= Viewport)   │   │ OBJECT       │
│ origin: TL   │   │ origin: CENTER       │   │ origin:      │
│ +Y down      │   │ +Y up                │   │  layer center│
│ pixels       │   │ pixels               │   │ +Y up        │
+──────────────+   +──────────────────────+   +──────────────+
   loader               position gizmo         rotation/scale pivot

Screen pixel ──image_to_frame──▶ Frame ──inv model──▶ Object ──object_to_src──▶ Source pixel
```

- Rotation order ZYX (After Effects). UI convention is CW+; glam is CCW+. `entities/space.rs::to_math_rot/from_math_rot` invert when calling `glam::Quat::from_euler`.
- Perspective projection: CPU compositor does inverse mapping via ray–plane intersection (`entities/transform.rs::unproject_to_plane`); orthographic uses the inverse affine fast path.

## Frame status FSM (`entities/frame.rs`)

```
Placeholder ─┐
Header  ────  try_claim ──▶ Loading ──── success ──▶ Loaded
                              │            │
                              │            └── dehydrate ──▶ Expired ──▶ Loading
                              └── failure ──▶ Error
```

`try_claim_for_loading()` atomically transitions `Header → Loading` to prevent two workers from loading the same file (TOCTOU).

## Persistence

- Window position/size — `eframe` (`persist_window: true`); persistence path is `config::config_file("playa.json")` set in `crates/playa-app/src/runner.rs`.
- App state — `eframe::APP_KEY` serialises `PlayaApp` into the same JSON; `#[serde(skip)]` on runtime-only fields, `#[serde(default)]` to tolerate missing keys.
- Project ("playlists") — separate JSON via `Project::to_json` / `Project::from_json`, loadable via `--playlist <FILE>`.
- Shaders — `shaders/` next to the binary, loaded by `widgets/viewport/shaders.rs`.

| OS | config | data |
|----|--------|------|
| Linux | `~/.config/playa/` | `~/.local/share/playa/` |
| macOS | `~/Library/Application Support/playa/` | same |
| Windows | `%APPDATA%\playa\` | same |

Override: `--config-dir`, `PLAYA_CONFIG_DIR`, or "portable" mode (a `playa.json`/`playa.log` already next to the binary).

## REST API

```
┌──────────────────────┐   mpsc::Sender<ApiCommand>   ┌───────────────────┐
│ rouille HTTP thread  │ ──────────────────────────▶  │ Main thread       │
│ POST /api/player/...│                              │ poll → emit       │
│ POST /api/frame/N    │                              │ project.modify... │
└──────────────────────┘                              └───────────────────┘
        ▲                                                     │
        │  Arc<RwLock<SharedApiState>>                        │
        │ ◀──── snapshot written every frame ─────────────────┘
```

Bound to `127.0.0.1:port` (loopback). Files: `crates/playa-app/src/server/{mod,api}.rs`. FPS validation: `is_finite() && > 0.0 && <= 960.0`.

## Error handling

- Production code avoids `unwrap()`/`expect()`. Exceptions: tests and `PoisonError` recovery (`unwrap_or_else(|e| e.into_inner())`).
- Errors flow through `Result<_, FrameError>` / `anyhow::Result` + `?`.
- `log::warn!` / `log::error!` at minimum — never silently swallow.

## Cross-cutting concerns

- **Logging:** `log` + `env_logger`, configured in `src/main.rs` from `-v..-vvv` and `-l/--log`.
- **Validation:** schema flags + per-attr setters (`Attrs::set` consults the schema).
- **Hotkeys:** `dialogs/prefs/input_handler.rs::HotkeyHandler` keyed by `(HotkeyWindow, Key)`; the focused window is consulted first, then `Global`.
- **Drag-and-drop:** `widgets/dnd.rs::GlobalDragState` is the single payload carrier between Project and Timeline.

## Data flow: click → pixels

```text
1. User scrubs the timeline.
2. SetFrameEvent → EventBus (immediate + deferred).
3. main_events::handle_app_event:
       project.modify_comp(active, |c| c.set_frame(target));
       set_frame mutates non-DAG attr → not dirty.
       modify_comp emits CurrentFrameChangedEvent.
4. handle_events catches CurrentFrameChangedEvent:
       enqueue_frame_loads_around_playhead(preload_radius);
5. cache_manager.increment_epoch() → stale tasks become no-ops.
6. workers.execute_with_epoch(epoch, job)
       worker_epoch != current_epoch → skip.
       else compose_internal(comp, frame, ctx).
7. compose_internal:
       Cpu prefs (no bridge): THREAD_COMPOSITOR / CpuCompositor::blend_with_dim on the worker.
       Gpu prefs + bridge: GpuBlendBridge::delegate_blend_blocking → UI thread blends, then signals.
       Encode / blocking get_frame: bridge omitted, Cpu path on that thread.
8. global_cache.insert(comp, frame, result)
       cache_manager.track_memory(); evict LRU if over budget.
       mark_dirty() so the main loop's take_dirty() requests a repaint.
9. ViewportRenderer.render(frame):
       recompile shader if pixel_format changed;
       glTexSubImage2D via PBO (double-buffered);
       glDrawArrays with u_model * u_view * u_projection.
```

---

*Architecture analysis: 2026-05-09. Source of truth: rustdocs across `crates/*/src/**/*.rs` and `AGENTS.md`.*
