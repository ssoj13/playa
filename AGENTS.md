# Playa Guide

Architecture guide for developers and AI assistants. Compiled from facts —
from module rustdocs and code tracing — not from rumors or the old README.

> Version: **0.1.142** · Rust **edition 2024** · `target/release/playa[.exe]`
> EXR backend: **vfx-exr** (pure Rust, all compressions including DWAA/DWAB/HTJ2K).
> Video: **playa-ffmpeg 8.0** (statically linked FFmpeg).

---

## Project Layout

### Workspace

```
playa/
├── Cargo.toml          # workspace = ["xtask"], excludes crates/playa-py
├── build.rs            # minimal, only cargo:rerun-if-changed
├── bootstrap.ps1/.sh   # wrappers → cargo xtask
├── xtask/              # build automation (release/post/pre, lib_discovery)
├── crates/
│   ├── playa-py/       # Python bindings (PyO3, separate workspace)
│   └── vfx-rs/         # path-dep for vfx-exr (EXR I/O), vfx-jph and others
├── src/                # application code (see below)
└── md/                 # documentation (this file, DEVELOP, DIAGRAMS, TODO, WGPU)
```

### `src/` — module map

```
src/
├── main.rs             # binary: ffmpeg::init → cli → log → run_app
├── lib.rs              # lib crate: pub mod core/app/entities/widgets/...
├── runner.rs           # eframe::run_native + state restoration
├── cli.rs              # clap::Args + version with backend info
├── config.rs           # PathConfig (CLI/ENV/dirs-next), theme colors, defaults
├── help.rs             # F1 overlay
├── shell.rs            # OS integration (drag-drop)
├── ui.rs               # composition of top menu/panels
├── utils.rs            # sequence parsing, media extensions
├── main_events.rs      # central event handler (AppEventContext)
│
├── app/                # PlayaApp + eframe::App impl
│   ├── mod.rs          # PlayaApp struct (47+ fields), Default, build_dock_state
│   ├── run.rs          # eframe::App::update() — main frame
│   ├── events.rs       # handle_events: poll EventBus → handle_app_event
│   ├── tabs.rs         # DockTabs: TabViewer for egui_dock
│   ├── layout.rs       # named layouts, sync_dock_tabs_visibility
│   ├── project_io.rs   # save/load Project, load_sequences
│   └── api.rs          # REST server start, ApiCommand handling
│
├── core/               # the "engine" — UI-independent
│   ├── event_bus.rs        # EventBus (sub/emit/poll), Event, BoxedEvent
│   ├── player.rs           # Player: state in Attrs, JKL shuttle, FPS presets
│   ├── player_events.rs    # SetFrame, TogglePlayPause, JogForward/Backward, ...
│   ├── workers.rs          # Workers: work-stealing deques + epoch
│   ├── cache_man.rs        # CacheManager: memory limit + epoch + dirty_repaint
│   ├── global_cache.rs     # GlobalFrameCache: nested HashMap + lru::LruCache
│   ├── debounced_preloader.rs  # 500ms delay before full preload
│   └── layout_events.rs    # ResetLayout, LayoutSelected/Created/Deleted/...
│
├── entities/           # data model + business logic
│   ├── attrs.rs            # Attrs: HashMap<&str, AttrValue> + dirty + schema
│   ├── attr_schemas.rs     # FILE/COMP/LAYER/CAMERA/TEXT/PLAYER/PROJECT_SCHEMA
│   ├── keys.rs             # const keys: A_IN, A_OUT, A_FRAME, A_SPEED, ...
│   ├── traits.rs           # FrameCache, WorkerPool, CacheStrategy (DI inversion)
│   ├── node.rs             # trait Node + ComputeContext
│   ├── node_kind.rs        # enum NodeKind { File, Comp, Camera, Text }
│   ├── file_node.rs        # FileNode: file_mask, file_start, file_end
│   ├── comp_node.rs        # CompNode + Layer: composition with effects
│   ├── camera_node.rs      # CameraNode: fov, near/far, POI vs rotation
│   ├── text_node.rs        # TextNode: cosmic-text, shared FontSystem
│   ├── comp_events.rs      # CurrentFrameChanged/LayersChanged/AttrsChanged
│   ├── frame.rs            # Frame, PixelBuffer (U8/F16/F32), FrameStatus
│   ├── compositor.rs       # CpuCompositor, BlendMode, apply_blend
│   ├── gpu_compositor.rs   # GpuCompositor (OpenGL FBO + GLSL)
│   ├── transform.rs        # 3D affine + sample_bilinear (rayon par_chunks)
│   ├── space.rs            # image_to_frame / frame_to_image / object_to_src
│   ├── loader.rs           # extension classification → exr/video/generic
│   ├── loader_video.rs     # FFmpeg metadata and frame decoding
│   ├── project.rs          # Project: media pool + active + modify_comp pattern
│   └── effects/            # blur.rs, brightness.rs, hsv.rs (+ Effect, EffectType)
│
├── widgets/            # reusable egui widgets (communicate via EventBus)
│   ├── viewport/           # OpenGL viewport: renderer/shaders/gizmo/pick/tool
│   ├── timeline/           # timeline with layers, work area, bookmarks
│   ├── project/            # media panel (Project)
│   ├── node_editor/        # composition graph (egui-snarl)
│   ├── ae/                 # Attribute Editor (universal for Attrs)
│   ├── status/             # status bar (FPS, file name, cache, render time)
│   ├── file_dialogs.rs     # rfd wrappers
│   └── actions.rs          # ActionQueue { hovered, events: Vec<BoxedEvent> }
│
├── dialogs/            # modal dialogs
│   ├── prefs/              # AppSettings, HotkeyHandler, prefs_events
│   └── encode/             # EncodeDialog: H.264/265, ProRes, NVENC/QSV/AMF
│
└── server/             # REST API
    ├── mod.rs              # re-exports + endpoint enumeration
    └── api.rs              # ApiServer, ApiCommand, SharedApiState (rouille)
```

---

## Architectural principles

### 1. Event-driven, no direct calls between widgets

Widgets **don't call each other** and don't reach into `PlayaApp` directly.
Instead they emit typed events into the `EventBus` (`src/core/event_bus.rs`).

```text
        emit::<E>(event)
              │
        ┌─────┴─────┐
        ▼           ▼
  immediate     deferred queue
  callbacks      (VecDeque, max 1000)
                       │
                       ▼
                 main loop poll() → handle_app_event(ctx, event)
```

**Why**:
- A widget knows nothing about the receiver — the cache can be cleared without knowing the widget.
- `egui` re-renders the UI every frame, callbacks are hard to wire in → deferred handling.
- Polling in `update()` atomically applies state changes in a batch before the next render.

**`downcast_event` pitfall**: the blanket impl `impl<T> Event for T` means
`Box<dyn Event>` itself implements `Event`. Writing `event.as_any()` lets the
method resolver pick the impl on `Box` instead of the inner type. That is why
`event_bus::downcast_event()` uses **`(**event).as_any()`** — to force routing
through the vtable. Don't simplify it.

**Event categories** (live next to the widgets and entities they belong to):

| File | Events |
|------|---------|
| `core/player_events.rs` | `SetFrameEvent`, `TogglePlayPauseEvent`, `Step{F,B}*`, `Jump*`, `Jog{F,B}` |
| `core/layout_events.rs` | `ResetLayout`, `LayoutSelected/Created/Deleted/Updated/Renamed` |
| `entities/comp_events.rs` | `CurrentFrameChangedEvent`, `LayersChangedEvent`, `AttrsChangedEvent` |
| `widgets/project/project_events.rs` | `AddClip(s)`, `AddFolder`, `AddComp/Camera/Text`, `RemoveMedia`, `ClearCache` |
| `widgets/timeline/timeline_events.rs` | `Timeline{Zoom,Pan,Snap,LockWorkArea}*`, `TimelineFitEvent`, ... |
| `widgets/viewport/viewport_events.rs` | `FitViewportEvent`, `Viewport100Event`, `ViewportRefreshEvent` |
| `widgets/viewport/tool.rs` | `SetToolEvent(ToolMode)` |
| `dialogs/prefs/prefs_events.rs` | `SetGizmoPrefsEvent`, hotkey windows |

### 2. Project does not belong to Player

`Player` holds **only playback state** in its own `Attrs`. `Project` lives
in `PlayaApp` (the single source of truth). Player methods that need the
project take `&mut Project` as a parameter.

**Why**: previously Player owned the Project, which caused duplication —
the UI and the player could drift apart. Now both look at the same instance;
it's impossible to mutate a copy by accident.

**Player.attrs keys**: `active_comp`, `previous_comp_history`, `is_playing`,
`fps_base` (constant), `fps_play` (temporary, for J/L shuttle), `loop_enabled`,
`play_direction` (1.0/-1.0), `selected_seq_idx`.

### 3. Node graph via `enum_dispatch`

```rust
#[enum_dispatch(Node)]
pub enum NodeKind { File(FileNode), Comp(CompNode), Camera(CameraNode), Text(TextNode) }
```

`Node` — the shared trait (uuid, attrs, inputs, compute, is_dirty, preload, _in/_out/fps/dim/...).
`enum_dispatch` generates zero-cost dispatch (no `Box<dyn Node>`).
`is_renderable()` returns `false` for `Camera` (it produces no pixels).

`Project.media: Arc<RwLock<HashMap<Uuid, Arc<NodeKind>>>>` — the inner `Arc`s
let workers **take a snapshot** (clone the `HashMap` of arcs in microseconds)
and release the lock immediately while a heavy `compute()` runs for 50–500 ms.
The UI is never blocked by a worker reading.

### 4. Schema-aware Attrs → automatic cache invalidation

`Attrs` is a shared container for Frame, Layer, Comp, Camera, Project. Each
type has a `*_SCHEMA` in `attr_schemas.rs` describing attribute flags:

| Flag | Effect |
|------|--------|
| `FLAG_DAG`     | Change → `dirty=true` → render cache invalidation |
| `FLAG_DISPLAY` | Show in Attribute Editor |
| `FLAG_KEYABLE` | Can be animated with keyframes |
| `FLAG_READONLY`| Read-only (computed) |
| `FLAG_INTERNAL`| Hidden, don't show to the user |

```text
opacity (DAG)        → set() → schema.is_dag()=true  → dirty=true → invalidation
frame   (non-DAG)    → set() → schema.is_dag()=false → dirty untouched
node_pos in editor   → set() → not DAG               → cache not flushed
```

**Why**: you can move the playhead and selection without trashing the cache.
"Dangerous" changes (transform, opacity, blend_mode) automatically dispatch
`AttrsChangedEvent`.

### 5. `project.modify_comp(uuid, |comp| ...)` — the only way to mutate

```rust
project.modify_comp(uuid, |comp| {
    comp.set_child_attrs(layer, &values);   // attrs.set() → dirty=true
});
// modify_comp checks is_dirty() and emits AttrsChangedEvent
// → handler in main_events.rs:
//     1. cache_manager.increment_epoch()  — invalidates old worker tasks
//     2. global_cache.clear_comp(uuid)    — drops frames from the cache
//     3. preloader restarts loading
```

Any direct mutation of `comp.layers.push/insert/remove` or `layer.attrs.set`
**bypasses the setters** and requires manual `comp.attrs.mark_dirty()` —
otherwise the UI keeps showing a stale frame.

`modify_comp()` uses `event_emitter: Option<EventEmitter>` (marked
`#[serde(skip)]`). After deserialization you **must** call
`project.set_event_emitter(event_bus.emitter())` — otherwise the cache
silently desyncs.

### 6. Work-stealing workers with epochs

`Workers` (`src/core/workers.rs`) — a thread pool with **per-worker FIFO deques**
plus a global `Injector`:

```text
Worker loop:
  1. own deque pop()         (FIFO — oldest first, so requests don't starve)
  2. injector.steal()         (global queue)
  3. steal from other workers (work stealing)
  4. shutdown? → exit
  5. sleep 1ms (no spin-burning CPU)
```

Pool size: `num_cpus::get() * 3 / 4` (we leave 25% for the UI).

**Epochs** (`Arc<AtomicU64>` shared with `CacheManager`): on UI scrub the
`current_epoch` is bumped quickly. Before composing/loading, a worker compares
its own epoch to the current one — **if stale, it skips the work**. Without
this, dragging the playhead from 0 to 500 would force the workers to load 500
frames nobody needs.

### 7. LRU cache with memory tracking

```
GlobalFrameCache:
  cache: RwLock<HashMap<Uuid, HashMap<i32, Frame>>>   ← per-comp sub-maps
  lru_order: Mutex<lru::LruCache<CacheKey, ()>>       ← O(1) get/put/pop_lru
  cache_manager: Arc<CacheManager>                    ← memory budget
  strategy: All | LastOnly                            ← All=keep everything in work area
```

- **O(1) clear_comp**: drop the outer `Uuid` key — the inner map goes to
  drop, and the LRU evicts come through normal pushes.
- **`dehydrate=true`**: marks `Loaded → Expired`, pixels stay (fast).
  `false`: removes from the cache entirely (frees memory).
- **Memory budget**: `CacheManager::new(0.75, 2.0)` — 75% of
  `sysinfo::available_memory()` minus a 2 GB system reserve. The limit is
  atomic; you can change it without rebuilding the cache.
- **`dirty_repaint: AtomicBool`**: a worker sets `true` after `insert`;
  the main loop's `take_dirty()` → `ctx.request_repaint()`. Otherwise egui
  would sleep until the cursor moved.

### 8. `DebouncedPreloader` — 500 ms before a full preload

While attributes are being changed quickly (e.g. an opacity slider) the cache
would thrash: clear cache → load 50 frames → clear → load again.
`DebouncedPreloader` holds `(comp_uuid, trigger_time)`; `tick()` returns
`Some(uuid)` only if ≥ 500 ms have elapsed since `schedule()`. Until then
**only the current frame** is loaded.

### 9. Dependency inversion: `core` depends on `entities`

`entities/traits.rs` defines the interfaces (`FrameCache`, `WorkerPool`,
`CacheStrategy`) that **the entities themselves** expect from infrastructure.
Concrete implementations (`GlobalFrameCache`, `Workers`) live in `core/`.
Dependency graph:

```
app  ──→ widgets, dialogs, server, main_events
         │      │       │        │
         ▼      ▼       ▼        ▼
         core ──→ entities (via trait objects in ComputeContext)
```

`ComputeContext` carries `&dyn FrameCache`, `Option<&dyn WorkerPool>` — a node
doesn't know the real types and is testable in isolation.

---

## Data flow: from click to pixels

```text
1. User scrub      — drag on the timeline
2. SetFrameEvent   — emit, both immediately and into the queue
3. main_events::handle_app_event
   → project.modify_comp(active, |c| c.set_frame(target))
   → set_frame mutates a non-DAG attr → NOT dirty
   → modify_comp emits CurrentFrameChangedEvent (frame changed)
4. handle_events catches CurrentFrameChangedEvent:
   → enqueue_frame_loads_around_playhead(preload_radius)
5. cache_manager.increment_epoch()      — old worker tasks become stale
6. workers.execute_with_epoch(epoch, job)
   → if worker_epoch != current_epoch → skip
   → else compose_internal(comp, frame, ctx)
7. compose_internal:
   → for each layer (layers.iter().rev() — bottom-up):
       a) source_node = ctx.media[source_uuid]
       b) source_frame = source_node.compute(layer_frame, ctx) (recursive)
       c) for fx in layer.effects: source_frame = fx.apply(source_frame)
       d) transform::apply (rayon par_chunks_mut, sample_bilinear)
       e) push (frame, opacity, blend_mode, inv_matrix) into a Vec
   → CpuCompositor.blend_with_dim(frames, dim) — Porter-Duff in blend_f32
   → unify formats: blend_u8/blend_f16 decode to f32, delegate, encode back
8. global_cache.insert(comp, frame, result)
   → cache_manager.track_memory(size); if over the limit → evict LRU
   → mark_dirty() → main loop will call ctx.request_repaint()
9. ViewportRenderer.render(frame):
   → if pixel_format changed → recompile shader
   → glTexSubImage2D via PBO (double-buffered for async upload)
   → glDrawArrays via u_model * u_view * u_projection
```

---

## Coordinate spaces

```
+──────────────+   +──────────────────────+   +──────────────+
│ IMAGE        │   │ FRAME (= Viewport)   │   │ OBJECT       │
│ origin: TL   │   │ origin: CENTER       │   │ origin:      │
│ +Y down      │   │ +Y up                │   │  layer center│
│              │   │                      │   │ +Y up        │
│ pixels       │   │ pixels               │   │ pixels       │
+──────────────+   +──────────────────────+   +──────────────+
   loader               position              for rotation/scale
   textures             gizmo                 around pivot
```

```
Screen pixel ──image_to_frame──▶ Frame ──inv model──▶ Object ──object_to_src──▶ Source pixel
```

**Rotations**: order is ZYX (like After Effects). The user-facing convention
is clockwise = "+" (`CW+`); `glam` uses the mathematical one (`CCW+`),
so **angles are inverted** when calling `glam::Quat::from_euler` —
see `space::to_math_rot` / `from_math_rot`.

**Perspective projection**: the CPU compositor does inverse mapping — "for
each output pixel, find the source pixel". With perspective you can't just
multiply by the inverse MVP, so we use **ray–plane intersection**: a ray from
the camera through the pixel intersects the layer plane in world space
(`transform::unproject_to_plane`). Orthographic uses the fast path through
the inverse affine matrix.

---

## Loaders

| Type | Backend | Extensions |
|------|---------|-----------|
| EXR | `vfx-exr` (path-dep, pure Rust) | `.exr` — all compressions including DWAA/DWAB/HTJ2K |
| Generic | `image` 0.25 | `.png .jpg .jpeg .tif .tiff .tga .hdr` |
| Video | `playa-ffmpeg` 8.0 (static) | `.mp4 .mov .avi .mkv` |

`loader::classify_ext` dispatches to `header_*` and `load_*`. `header_*`
reads only the header (for FileNode when added to the project); the full
decode is deferred until a worker requests a frame.

**Video metadata**: `VideoMetadata::from_file` guards `denom != 0`
(BUG-04 fix), `frame_count = (duration_secs * fps).round()` (BUG-13 fix —
`as usize` was losing half of the last frame).

**Frame status FSM**:

```
Placeholder ─┐
Header  ───── try_claim ───▶ Loading ──── success ──▶ Loaded
                              │              │
                              │              └── dehydrate ──▶ Expired ──▶ Loading
                              │
                              └── failure ──▶ Error
```

`try_claim_for_loading()` atomically performs `Header → Loading` so two
workers don't load the same file (TOCTOU race).

---

## Layer effects

```rust
Layer {
    attrs: Attrs,
    effects: Vec<Effect>,   // applied in order BEFORE transform/blend
}
```

| Type | Parameters | Notes |
|------|-----------|-------|
| `GaussianBlur` | `radius: 0–100` | Separable: `convolve_axis(true)` H, `convolve_axis(false)` V — single function, axis is a parameter |
| `BrightnessContrast` | `brightness: -1..1`, `contrast: -1..1` | Per pixel |
| `AdjustHSV` | `hue_shift: -180..180`, `saturation: 0..2`, `value: 0..2` | Extracted into `adjust_hsv()` — the only rgb→hsv→adj→rgb path |

**DRY principle in blend/transform/effects**: U8/F16/F32 branches do not
duplicate business logic — they decode to f32, delegate to the shared f32
function, then encode back. Same for `transform::sample_bilinear<T>(decode: impl Fn(T) → f32)`
with a rayon macro for the parallel arms.

---

## Compositing: CPU vs GPU

| Component | Where | Status |
|-----------|-------|--------|
| `CpuCompositor` | works everywhere, including in workers | main path |
| `GpuCompositor` | OpenGL FBO + GLSL, 10–50× faster | **viewport-only**, not used in `compose_internal` |

The `CompositorType::blend()` interface takes `Vec<(Frame, opacity, BlendMode, [f32; 9])>`
with 3×3 matrices (column-major for GL) — the API is unified. However
`compose_internal` runs in workers where no GL context is available (the
context belongs to the eframe main thread). So real GPU compositing is only
used for viewport effects today, and layers are blended on CPU. A migration
plan is outlined in the header of `compositor.rs`.

`BlendMode`: Normal · Screen · Add · Subtract · Multiply · Divide · Difference · Overlay
(`apply_blend()` is the single place with the Porter–Duff formulas).

---

## Main loop (`PlayaApp::update`)

```
1. exit_requested?               → Close viewport
2. start_api_server()            (lazy: on first frame, if enabled)
3. update_compositor_backend(gl) (CPU↔GPU per Settings)
4. apply theme/font              (last_applied_* guards)
5. handle_events()               poll EventBus → handle_app_event
6. process player.update()       (advances frame by wall-clock)
7. handle dropped files          (drag-drop)
8. DockArea.show(ctx, &mut DockTabs(self))
9. handle_keyboard_input()       (HotkeyHandler by focused window)
10. process API commands         (mpsc::Receiver<ApiCommand>)
11. update_api_state()           (writes SharedApiState under RwLock)
12. handle pending screenshots   (PNG via glReadPixels or from current frame)
13. cache_manager.take_dirty()   → ctx.request_repaint() if a load happened
```

**Hotkey routing** — `HotkeyHandler` stores `(HotkeyWindow, key) → EventFactory`.
We first look up by the focused window (Viewport / Timeline / Project /
NodeEditor / Settings / Encode / Hotkeys), then fall back to `Global`.
This lets `Delete` in Project remove media, while in Timeline it removes a layer.

---

## Persistence

- Window: `eframe` saves position/size itself (`persist_window: true`),
  `persistence_path` is set in `runner.rs` via `config::config_file("playa.json")`.
- App state: `eframe` serializes `PlayaApp` into the same JSON via
  `eframe::APP_KEY` (`#[serde(default)]`, runtime-only fields are
  `#[serde(skip)]`).
- Project: `Project::to_json` / `Project::from_json` — a separate on-disk
  format for "playlists"; `--playlist <FILE>` loads at startup.
- Shaders: `shaders/` next to the binary is picked up by
  `Shaders::load_shader_directory`.

**Platform paths** (via `dirs-next`):

| OS | config | data |
|----|--------|------|
| Linux | `~/.config/playa/` | `~/.local/share/playa/` |
| macOS | `~/Library/Application Support/playa/` | same |
| Windows | `%APPDATA%\playa\` | same |

Override: CLI `--config-dir`, the `PLAYA_CONFIG_DIR` ENV, or a local
directory (if it already contains `playa.json`/`playa.log` — "portable" mode).

---

## REST API

```
┌──────────────────────┐  mpsc::Sender<ApiCommand>  ┌───────────────────┐
│ rouille HTTP thread  │ ──────────────────────────▶│ Main thread       │
│ POST /api/player/play│                             │ poll → emit       │
│ POST /api/.../frame/N│                             │ project.modify... │
└──────────────────────┘                             └───────────────────┘
        │                                                     │
        │  Arc<RwLock<SharedApiState>>                        │
        │ ◀──────────────── snapshot ─────────────────────────│
        │                                       writes every frame
```

Bound to `127.0.0.1:port` (loopback only). FPS validation in the handler:
`is_finite() && > 0.0 && <= 960.0`. Endpoints:
`status / player / comp / cache / health / play / pause / stop / frame/N /
fps/N / toggle-loop / project/load / event / next / prev / screenshot / exit`.

**Screenshots**: `Screenshot { viewport_only: bool, response: crossbeam::Sender }`.
If viewport_only — `glReadPixels` via `frame.read_pixels()` after the render;
otherwise the current `Frame` is serialized to PNG.

---

## Layouts

`AppSettings.layouts: HashMap<String, Layout>` — named layouts (dock splits,
timeline state, viewport state). Events live in `core/layout_events.rs`:
`LayoutSelected/Created/Deleted/Updated/Renamed`. The old
`SaveLayoutEvent`/`LoadLayoutEvent` were removed — they were replaced by
a structured schema with auto-generated names ("Layout 2", "Layout 3", ...).

`build_dock_state(show_project, show_attributes, split_pos)` rebuilds the
egui_dock tree with configurable visibility for the Project/Attributes panels.

---

## Build pipeline

`bootstrap.ps1` / `bootstrap.sh` → `cargo xtask`. The thin `build.rs` does
nothing — dependency management and DLL/so copying are delegated to xtask.

```
cargo xtask build [--release|--debug]   # build + post-build copy
cargo xtask post                        # copy native libs
cargo xtask verify                      # check dependencies
cargo xtask deploy [--install-dir P]    # install to system
cargo xtask pre                         # Linux: patch OpenEXR headers (for GCC 11+)
cargo xtask tag-dev/tag-rel/pr/changelog
```

**vcpkg for FFmpeg** — required. Triplets: `x64-windows-static-md-release`,
`x64-linux-release`, `arm64-osx-release`, `x64-osx-release`. ENV: `VCPKG_ROOT`,
`VCPKGRS_TRIPLET`, `PKG_CONFIG_PATH`. Details — in `md/DEVELOP.md`.

**Release profile**: `strip = false`, `lto = false`, `codegen-units = 1`
is commented out — optimized for link speed, not binary size.

**Windows specifics**: static build with no DLLs (triplet `static-md`).
**macOS**: signed with Developer ID `Y8PQ7YASU9`, notarization disabled in metadata.

---

## CLI

```
playa [OPTIONS] [FILE]
  -f, --file FILE          extra files (multiple)
  -p, --playlist FILE      playlist (Project::from_json)
  -F, --fullscreen
      --frame N            start frame
  -a, --autoplay
  -o, --loop 0|1           default 1
      --start N --end N    play range
      --range S E          shorthand
  -l, --log [FILE]         log to file
  -v..-vvv                 warn/info/debug/trace
  -c, --config-dir DIR     override platform paths
```

`--mem` and `--workers` are marked `hide = true` — relics of the old cache,
the code reads them as ENV-fallback for worker configuration.

Version (`-V`):
```
0.1.142
EXR:    vfx-exr (pure Rust, all compressions)
Video:  playa-ffmpeg 8.0 (static)
Target: x86_64-windows
```

---

## Coding rules

### Rust

- In production code **avoid** `unwrap()`/`expect()`. Exceptions: tests,
  `PoisonError` recovery (`unwrap_or_else(|e| e.into_inner())`).
- Propagate errors through `Result<_, FrameError>` / `anyhow::Result` + `?`.
- Don't swallow errors silently. `log::warn!` or `log::error!` at minimum.
- `Arc::clone(&x)` instead of `x.clone()` for explicitness.
- Don't grow dependencies — Cargo.toml is already wide.
- `serde(skip)` on runtime fields; **must** be restored after deserialization
  (event_emitter, schemas, cache_manager) — see `runner.rs`.

### Tokio / Async

There is **no** Tokio in the project. Workers are `std::thread`, queues are
crossbeam, HTTP is `rouille` (synchronous). Don't introduce an async runtime
without a clear need. Don't block the main thread — use `Workers::execute(job)`
for heavy tasks.

### Edits / Refactors

- Minimal diff. Don't refactor along the way.
- Names and style — match the neighbors.
- No formatting-only commits.
- If you mutate `Comp.layers` directly — `comp.attrs.mark_dirty()` in the
  same `modify_comp` transaction.
- If you add an attribute — describe it in the relevant `*_SCHEMA` with the
  right flags (`DAG` is mandatory for anything that affects pixels).

### Adding a NodeKind

1. `entities/foo_node.rs` with a struct and `impl Node`.
2. A variant in `enum NodeKind`.
3. A schema in `attr_schemas.rs` (compose shared `IDENTITY`, `TIMING`, `TRANSFORM`).
4. Mark `is_renderable()` and `is_listed()` as needed.
5. If there's an `add_child_layer` — update `NodeKind::add_child_layer()`.

### Adding an event

1. A struct in the right `*_events.rs` (next to its "own" domain).
2. Emit: `event_bus.emit(MyEvent { ... })` or via `ActionQueue`.
3. Handle: `if let Some(e) = downcast_event::<MyEvent>(&event)` in
   `app/events.rs::handle_events` or `main_events.rs::handle_app_event`.
4. If the event mutates the project — do it inside `project.modify_comp`
   so auto-invalidation kicks in.

### Adding an effect

1. `entities/effects/foo.rs` with a function `apply(&Frame, &Effect) → Frame`.
2. A variant in the `EffectType` enum.
3. A schema `FX_FOO_SCHEMA` (fields with `FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE`).
4. Match arms in `effects::schema()` and `effects::apply()`.

---

## Development platform (for AI/context)

- **Windows 11**, PowerShell 7+ (`pwsh`). Not `bash`. Instead of `/dev/null` —
  `$null`; escape `\` or use forward `/` where accepted.
- **vcpkg** in `C:\vcpkg`, ENV: `$env:VCPKG_ROOT`. Before building the
  OpenEXR variant, activate `vcvars64.bat` (via `Developer PowerShell for VS 2022`).
- **Sciter / Flutter** are not used (that belongs to RustDesk). Here the
  UI is a single stack — egui/eframe + glow OpenGL.

---

## Surprises and gotchas

| Where | What | Why it matters |
|-------|------|----------------|
| `event_bus::downcast_event` | `(**event).as_any()` is required | The blanket impl on `Box<dyn Event>` breaks naive `event.as_any()` |
| `project.set_event_emitter` | call after every deserialization | `event_emitter` is `#[serde(skip)]` — without restoring it, mutations don't invalidate the cache |
| `compose_internal` rev order | `layers.iter().rev()` | `layers[0]` is the background, `layers[N-1]` is in front; sources are gathered into a `Vec` bottom-up |
| `trim_in/trim_out` | **offsets, not absolutes** | `work_start = _in + trim_in`, `work_end = _out - trim_out`. For a Layer — in source frames, then scaled by `speed` |
| `enum_dispatch` shadowing | do **not** duplicate `fps/_in/_out/frame` in `impl NodeKind` | Duplicates shadow the trait method, tests fail |
| Rotation sign | `space::to_math_rot(deg)` inverts | UI is CW+, glam is CCW+ |
| Cache LRU | use `lru::LruCache`, not a custom `IndexSet` | O(1) instead of O(n) `shift_remove` |
| `process_blocking` in workers | none — workers are `std::thread::sleep(1ms)` | No async runtimes nested inside |
| `THREAD_COMPOSITOR` | `thread_local!` on purpose | A worker has no GL context, you can't share `RefCell<Compositor>` across threads |
| GPU compositor | currently **viewport-only** | `compose_internal` runs in workers without GL — migration plan in the header of `compositor.rs` |

---

## Structural diagrams

See `md/DIAGRAMS.md` — Mermaid diagrams for the frame pipeline, events,
cache, compositing, and the Node hierarchy.

See `md/WGPU.md` — a report on a possible port to wasm32 + wgpu (blocked
by FFmpeg, rouille, the OpenGL renderer, `sysinfo`).

See `md/DEVELOP.md` — vcpkg, FFmpeg, and cross-platform build details.

---

*Basis: rustdocs of every module in `src/**/*.rs`, verified against the
current code (commit `75240d7`). If this disagrees with reality — the truth
is in the code.*
