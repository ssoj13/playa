# Codebase Structure

**Analysis Date:** 2026-05-09

Playa is a Cargo workspace with five library crates plus an `xtask` automation crate.
The root crate `playa` is a thin aggregator that exposes a binary (`src/main.rs`) and a
library (`src/lib.rs`) that re-exports the workspace surfaces. `crates/playa-py` is a
separate, excluded workspace for the Python bindings.

## Top-level layout

```
playa/
├── Cargo.toml              # workspace manifest + thin `playa` package; excludes playa-py
├── Cargo.lock
├── build.rs                # minimal — only `cargo:rerun-if-changed=build.rs`
├── bootstrap.py            # vcpkg + MSVC env helper → `cargo xtask <cmd>`
├── start.cmd               # Windows convenience launcher
├── apple_cert.sh           # macOS code-signing helper script
├── apple_reqs.sh           # macOS prerequisites installer
├── lin-reqs.sh             # Linux prerequisites installer
├── icon.png                # packaging icon (NSIS / .app / Linux)
├── cliff.toml              # git-cliff changelog config
├── README.md, AGENTS.md, CHANGELOG.md, DEVELOP.md, TODO.md, RUST.md, CLAUDE.md   # developer docs
├── .cargo/config.toml      # local cargo settings
├── .github/                # CI + PR templates
├── .planning/              # generated planning docs (this file lives here)
├── src/                    # root crate sources (binary + re-export lib)
│   ├── main.rs             # binary entry: init ffmpeg, parse args, configure logging, call run_app
│   ├── lib.rs              # re-exports playa-engine / playa-events / playa-ui / playa-app surfaces
│   └── README.md           # src-level notes
└── crates/
    ├── playa-app/          # desktop host: PlayaApp + main_events + runner + cli + server + shell + config
    ├── playa-engine/       # cache, playback, entities, loaders dispatch, compositor, gpu render
    ├── playa-events/       # typed events + EventBus (the single cross-layer messaging path)
    ├── playa-io/           # media decoders: EXR, generic image, FFmpeg video, WebCodecs scaffold
    ├── playa-ui/           # egui widgets, dialogs, help, menu composition
    ├── xtask/              # build automation: changelog, tags, build wrapper, wipe, deploy
    └── playa-py/           # Python bindings (separate workspace, excluded by root Cargo.toml)
```

### Notable repo-root files

| Path | Purpose |
|------|---------|
| `Cargo.toml` | Workspace + thin aggregator package `playa` (binary + lib re-exports). |
| `build.rs` | Only `println!("cargo:rerun-if-changed=build.rs");` — natives go through vcpkg + crate deps. |
| `bootstrap.py` | Sets `VCPKG_ROOT`, merges MSVC env on Windows, then forwards to `cargo xtask` (or `cargo build -p playa --features` when `-f` is given). |
| `start.cmd` | Quick Windows launcher for the built binary. |
| `apple_cert.sh`, `apple_reqs.sh`, `lin-reqs.sh` | Platform-specific dev environment setup. |
| `icon.png` | App icon used by `package.metadata.packager`. |
| `cliff.toml` | git-cliff template for `cargo xtask changelog`. |
| `AGENTS.md` | Authoritative architectural guide (events, nodes, cache, workers, compositor, persistence, REST). |
| `CHANGELOG.md`, `DEVELOP.md`, `TODO.md`, `RUST.md`, `CLAUDE.md` | Developer docs / agent rules. |
| `.cargo/config.toml` | Cargo runner / linker overrides. |

## Crate: `playa-app` (desktop host / orchestration)

```
crates/playa-app/
├── Cargo.toml              # depends on engine, events, ui, eframe, egui_dock, rouille, rfd, dirs-next, num_cpus, scanseq, regex
└── src/
    ├── lib.rs              # crate root: pub mod app/cli/config/main_events/runner/server/shell + pub use run_app
    ├── runner.rs           # run_app(args): builds eframe NativeOptions, sets persistence_path, spawns app
    ├── cli.rs              # `clap` Args struct (--file, --playlist, --frame, --autoplay, --loop, --range, -v, --log, --config-dir)
    ├── config.rs           # PathConfig, config_file/data_file helpers, portable-mode detection (dirs-next)
    ├── main_events.rs      # central handle_app_event: project mutations, cache invalidation, preloader scheduling
    ├── shell.rs            # OS shell helpers (open path, reveal in explorer)
    ├── app/
    │   ├── mod.rs          # PlayaApp struct, DockTab enum, default_dock_state, build_dock_state, gpu_blend bridge wiring
    │   ├── run.rs          # impl eframe::App for PlayaApp — the 14-step main loop in update()
    │   ├── events.rs       # handle_events (EventBus poll), keyboard input, action handlers
    │   ├── tabs.rs         # DockTabs (egui_dock::TabViewer impl) — renders each DockTab
    │   ├── api.rs          # update_api_state, ApiCommand drain, screenshot servicing
    │   ├── project_io.rs   # project JSON load/save, drag-drop ingestion, playlist parsing
    │   ├── layout.rs       # named-layout save/load handlers
    │   └── README.md       # app-module notes
    └── server/
        ├── mod.rs          # rouille HTTP thread, ApiCommand mpsc, SharedApiState, ApiServer::start
        └── api.rs          # endpoint handlers (status, player, comp, cache, frame/N, fps/N, screenshot, exit, ...)
```

**Imports across crate boundaries:** `playa-app` imports `playa_engine::{core, entities}`, `playa_ui::{widgets, dialogs}`, `playa_events`, `playa_io` (only `init_ffmpeg`).

## Crate: `playa-engine` (data + compute)

```
crates/playa-engine/
├── Cargo.toml              # crossbeam, enum_dispatch, glam, half, lru, rayon, sysinfo, wgpu, cosmic-text, glob, lazy_static
└── src/
    ├── lib.rs              # pub mod core, defaults, entities, render_gpu, utils
    ├── defaults.rs         # startup defaults (paths, settings, sample assets)
    ├── utils.rs            # small shared helpers
    ├── core/
    │   ├── mod.rs          # re-exports CacheManager, EventBus, GlobalFrameCache, Player, Workers, DebouncedPreloader
    │   ├── global_cache.rs # GlobalFrameCache: per-comp HashMap<i32,Frame> + lru::LruCache + dirty_repaint
    │   ├── cache_man.rs    # CacheManager: memory budget (sysinfo), atomic epoch, take_dirty
    │   ├── workers.rs      # Workers thread pool: per-worker FIFO deque + crossbeam Injector + epoch cancellation
    │   ├── event_bus.rs    # EventBus, EventEmitter, downcast_event (with the (**event).as_any() fix)
    │   ├── player.rs       # Player: playback state in its own Attrs (active_comp, is_playing, fps_play, loop, direction)
    │   ├── debounced_preloader.rs # 500 ms debounce window before full radius preload
    │   ├── player_events.rs       # SetFrameEvent, TogglePlayPauseEvent, Step{F,B}*, Jump*, Jog{F,B}
    │   └── layout_events.rs       # ResetLayout, LayoutSelected/Created/Deleted/Updated/Renamed
    ├── entities/
    │   ├── mod.rs          # re-exports Project, CompNode (alias Comp), Frame, Attrs, Node, NodeKind, …
    │   ├── project.rs      # Project: media: Arc<RwLock<HashMap<Uuid, Arc<NodeKind>>>>; modify_comp; to_json/from_json
    │   ├── node.rs         # Node trait + ComputeContext (carries &dyn FrameCache, Option<&dyn WorkerPool>, GpuBlendBridge)
    │   ├── node_kind.rs    # #[enum_dispatch(Node)] enum NodeKind { File, Comp, Camera, Text }
    │   ├── file_node.rs    # FileNode: image/EXR/video source; uses playa_io::loader through entities::loader
    │   ├── comp_node.rs    # CompNode: layers + compose_internal; THREAD_COMPOSITOR thread_local
    │   ├── camera_node.rs  # CameraNode: non-renderable (is_renderable=false)
    │   ├── text_node.rs    # TextNode: text overlay via cosmic-text
    │   ├── attrs.rs        # Attrs: Arc-shared bag; set() consults schema for FLAG_DAG dirtying
    │   ├── attr_schemas.rs # *_SCHEMA tables: COMP, LAYER, FRAME, CAMERA, PROJECT, FX_BLUR, FX_BC, FX_HSV
    │   ├── keys.rs         # canonical attribute key constants
    │   ├── traits.rs       # FrameCache, WorkerPool, CacheStrategy, CacheStatsSnapshot (dependency-inversion targets)
    │   ├── compositor.rs   # CompositorType enum (Cpu / Wgpu); blend / blend_with_dim API
    │   ├── gpu_blend_bridge.rs # GpuBlendBridge + GpuBlendRequest + GpuBlendReport; gpu_blend_arc_pair()
    │   ├── effects/
    │   │   ├── mod.rs      # Effect, EffectType enum, schema(), apply()
    │   │   ├── blur.rs     # GaussianBlur (separable; one convolve_axis function for H/V)
    │   │   ├── brightness.rs # BrightnessContrast
    │   │   └── hsv.rs      # AdjustHSV (single rgb→hsv→adj→rgb path, used by all bit depths)
    │   ├── loader.rs       # classify_ext + dispatch to playa_io header_*/load_*; FrameStatus FSM uses this
    │   ├── frame.rs        # Frame + FrameStatus (Placeholder/Header/Loading/Loaded/Expired/Error); try_claim_for_loading
    │   ├── space.rs        # IMAGE/FRAME/OBJECT spaces; CW+ ↔ CCW+ rotation conversion
    │   ├── transform.rs    # affine + sample_bilinear<T>; ray-plane intersection for perspective
    │   └── comp_events.rs  # CurrentFrameChangedEvent, LayersChangedEvent, AttrsChangedEvent
    └── render_gpu/
        ├── mod.rs          # GPU compositor module entry
        └── wgpu_compositor.rs # GpuCompositor (UI-thread only; binds to current wgpu device/queue)
```

**Imports across crate boundaries:** `playa-engine` imports `playa_io` (with `exr`+`ffmpeg` features on native) and `playa_events`. It does **not** import `playa_ui` or `playa_app`.

## Crate: `playa-events` (typed events + bus)

```
crates/playa-events/
├── Cargo.toml              # log, serde, serde_json, uuid — minimal
└── src/
    ├── lib.rs              # pub mod bus/comp/layout/node_editor/player/prefs/project_media/timeline/viewport/viewport_tool
    ├── bus.rs              # EventBus, EventEmitter, CompEventEmitter, BoxedEvent, downcast_event, blanket Event impl
    ├── player.rs           # mirrored player events for cross-layer dispatch
    ├── comp.rs             # comp/layer events (CurrentFrameChanged, LayersChanged, AttrsChanged)
    ├── timeline.rs         # Timeline{Zoom,Pan,Snap,LockWorkArea}*, TimelineFitEvent, …
    ├── viewport.rs         # FitViewportEvent, Viewport100Event, ViewportRefreshEvent
    ├── viewport_tool.rs    # SetToolEvent + ToolMode enum (Pan, Zoom, Select, …)
    ├── node_editor.rs      # node-graph editor events
    ├── project_media.rs    # AddClip(s), AddFolder, AddComp/Camera/Text, RemoveMedia, ClearCache
    ├── prefs.rs            # CompositorBackend, CompositorBackendChangedEvent, GizmoPrefs, hotkey events
    └── layout.rs           # LayoutSelected/Created/Deleted/Updated/Renamed, ResetLayout
```

**Imports across crate boundaries:** none — this is the leaf crate, depended upon by every other.

## Crate: `playa-io` (media decoding)

```
crates/playa-io/
├── Cargo.toml              # features: default=["exr","ffmpeg"]; "webcodecs"; deps: image, half, log, playa-ffmpeg, vfx-{exr,io,core}
└── src/
    ├── lib.rs              # pub mod dispatch/error/exr_layered/media/pixel/source_image/video/webcodecs; init_ffmpeg
    ├── dispatch.rs         # classify_ext → header_attrs / decode_raster; AttrKv tuple type
    ├── error.rs            # IoError enum
    ├── pixel.rs            # DecodedRaster, RawPixelBuffer, RawPixelFormat (U8/F16/F32 etc.)
    ├── media.rs            # high-level media classification helpers
    ├── exr_layered.rs      # vfx-exr / vfx-io path-dep wrappers (feature = "exr")
    ├── source_image/
    │   ├── mod.rs          # SourceImage abstraction + pick_display_layer
    │   ├── native.rs       # native (image crate + EXR) implementation
    │   └── stub.rs         # wasm fallback
    ├── video/
    │   ├── mod.rs          # VideoMetadata, decode_frame, get_video_dimensions
    │   ├── ffmpeg_imp.rs   # playa-ffmpeg 8.0 backend (feature = "ffmpeg")
    │   └── stub.rs         # non-ffmpeg fallback
    └── webcodecs.rs        # browser WebCodecs scaffold (feature = "webcodecs")
```

**Imports across crate boundaries:** none from the workspace — only external crates (`image`, `playa-ffmpeg`, `vfx-{exr,io,core}`).

## Crate: `playa-ui` (egui presentation)

```
crates/playa-ui/
├── Cargo.toml              # eframe, egui_dock, egui-snarl, egui_dnd, egui_extras, egui_ltreeview, transform-gizmo-egui, rfd, glam, half, image
└── src/
    ├── lib.rs              # pub mod dialogs/help/ui/widgets
    ├── ui.rs               # main menu + composition root for widget rendering
    ├── help.rs             # help overlay window
    ├── widgets/
    │   ├── mod.rs          # pub mod actions, ae, dnd, file_dialogs, node_editor, project, status, timeline, viewport
    │   ├── actions.rs      # action enums emitted via dispatch closures
    │   ├── dnd.rs          # GlobalDragState — single drag payload across project↔timeline
    │   ├── file_dialogs.rs # rfd-based native file pickers
    │   ├── viewport/
    │   │   ├── mod.rs              # re-exports ViewportRenderer, Shaders, ViewportState, render
    │   │   ├── viewport.rs         # ViewportState, ViewportMode, ViewportRenderState
    │   │   ├── viewport_ui.rs      # render() — main viewport panel
    │   │   ├── renderer.rs         # ViewportRenderer (GL-backed via egui paint callback)
    │   │   ├── shaders.rs          # Shaders manager (loads `shaders/` next to binary)
    │   │   ├── coords.rs           # screen↔image↔frame conversions
    │   │   ├── pick.rs             # picking under cursor
    │   │   ├── gizmo.rs            # transform-gizmo-egui integration; GizmoState
    │   │   ├── tool.rs             # ToolMode handling (re-exports SetToolEvent)
    │   │   └── viewport_events.rs  # FitViewport, Viewport100, ViewportRefresh
    │   ├── timeline/
    │   │   ├── mod.rs              # re-exports TimelineState, TimelineActions, TimelineConfig, TimelineViewMode, ClipboardLayer, render_*
    │   │   ├── timeline.rs         # state + actions (no rendering)
    │   │   ├── timeline_ui.rs      # render_canvas, render_outline, render_toolbar
    │   │   ├── timeline_helpers.rs # drawing primitives co-located with UI code
    │   │   └── timeline_events.rs  # TimelineZoom/Pan/Snap/LockWorkArea*, TimelineFitEvent
    │   ├── project/
    │   │   ├── mod.rs              # re-exports ProjectActions, render
    │   │   ├── project.rs          # ProjectActions
    │   │   ├── project_ui.rs       # render() — list of clips/comps with DnD source
    │   │   └── project_events.rs   # AddClip(s), AddFolder, AddComp/Camera/Text, RemoveMedia, ClearCache
    │   ├── node_editor/
    │   │   ├── mod.rs              # re-exports CompNode (snarl wrapper), NodeEditorState, render_node_editor
    │   │   ├── node_graph.rs       # egui-snarl integration over Project's comp hierarchy
    │   │   └── node_events.rs
    │   ├── ae/
    │   │   ├── mod.rs              # re-exports AttributesState, EffectAction, render, render_effects, render_with_mixed
    │   │   └── ae_ui.rs            # generic Attribute Editor for Comp/Layer/Camera/Text/Effect
    │   └── status/
    │       ├── mod.rs              # re-exports StatusBar
    │       ├── status.rs           # StatusBar widget
    │       └── progress_bar.rs     # cache progress bar
    └── dialogs/
        ├── mod.rs                  # cfg-gated encode (native) vs encode_stub_wasm (wasm32)
        ├── encode/
        │   ├── mod.rs              # re-exports
        │   ├── encode.rs           # EncodeDialog state + job dispatch
        │   └── encode_ui.rs        # encoder dialog UI
        ├── encode_stub_wasm.rs     # wasm32 stand-in for the encoder
        └── prefs/
            ├── mod.rs              # re-exports input_handler, prefs
            ├── prefs.rs            # AppSettings (theme, font, gizmo, REST port, compositor backend, …)
            ├── input_handler.rs    # HotkeyHandler: (HotkeyWindow, Key) → EventFactory
            └── prefs_events.rs     # SetGizmoPrefsEvent, hotkey window enum, prefs change events
```

**Imports across crate boundaries:** `playa-ui` imports `playa_engine`, `playa_io`, `playa_events`. It is consumed by `playa-app` only.

## Crate: `xtask` (build automation)

```
crates/xtask/
├── Cargo.toml              # anyhow, clap, fs_extra, indicatif; on Windows also vcv-rs (vcvars merge)
└── src/
    ├── main.rs             # `cargo xtask` CLI: build, test, deploy, changelog, tag-dev, tag-rel, pr, wipe, wipe-wf
    ├── env_setup.rs        # MSVC env helpers (Windows); vcpkg triplet detection
    └── release.rs          # changelog (git-cliff), tagging, GitHub Actions workflow management
```

## Crate (excluded from workspace): `playa-py`

```
crates/playa-py/
├── Cargo.toml              # PyO3 bindings (separate workspace; built via maturin)
├── pyproject.toml
├── README.md
└── src/lib.rs              # exposes playa::run_app to Python
```

Excluded by `[workspace] exclude = ["crates/playa-py"]` in the root `Cargo.toml`.

## Where to add new code

| Concern | Location |
|---------|----------|
| New `NodeKind` variant | `crates/playa-engine/src/entities/<foo>_node.rs` + variant in `node_kind.rs` + schema in `attr_schemas.rs`. |
| New event | `crates/playa-events/src/<domain>.rs` (mirror in `crates/playa-engine/src/<core|entities>/<x>_events.rs` only when handled there). |
| New effect | `crates/playa-engine/src/entities/effects/<foo>.rs` + variant in `EffectType` + schema `FX_FOO_SCHEMA`. |
| New widget | `crates/playa-ui/src/widgets/<foo>/{mod,foo,foo_ui,foo_events}.rs`. |
| New dialog | `crates/playa-ui/src/dialogs/<foo>/{mod,foo,foo_ui}.rs`. |
| New REST endpoint | `crates/playa-app/src/server/api.rs` (handler) + `ApiCommand` variant in `server/mod.rs`. |
| New CLI flag | `crates/playa-app/src/cli.rs::Args` + handling in `crates/playa-app/src/runner.rs`. |
| New loader / file format | `crates/playa-io/src/dispatch.rs` (extension classification) + decoder module under `crates/playa-io/src/`. |

## Naming conventions

- Files: `snake_case.rs`. Submodule directories use `snake_case/` with a `mod.rs` re-exporting public surface.
- Event modules: `<domain>_events.rs` (engine/UI side) or one file per domain in `playa-events/`.
- Schema constants: `UPPER_SNAKE_CASE` (e.g. `COMP_SCHEMA`, `FX_BLUR_SCHEMA`) in `attr_schemas.rs`.
- Re-exports concentrate at `mod.rs` to keep external import paths short and stable.

## Generated / non-source directories

| Directory | Purpose | Committed |
|-----------|---------|-----------|
| `target/` | Cargo build output | No |
| `.planning/` | Generated planning / mapping documents (this file) | Per project policy |
| `shaders/` (next to built binary) | Runtime-loaded shader sources picked up by `Shaders::load_shader_directory` | Yes (in repo) |
| `crates/playa-py/target/` | Maturin/PyO3 build output | No |

---

*Structure analysis: 2026-05-09. Tree derived from `Cargo.toml` workspace + actual `src/` listings.*
