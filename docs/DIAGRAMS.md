# diagram_flow.md — сборник диаграмм Playa

Единая точка для ASCII/`text`-схем. Часть взята из [`docs/AGENTS.md`](docs/AGENTS.md); при расхождении приоритет у живого кода и доков в репозитории.

**Оглавление**

1. [Медиа → слои → композит](#1-медиа--слои--композит)
2. [Структура репозитория](#2-структура-репозитория-workspace)
3. [Карта `src/` (binary + lib)](#3-карта-src-binary--lib)
4. [EventBus: immediate vs deferred](#4-eventbus-immediate-vs-deferred)
5. [Attrs: DAG vs non-DAG](#5-attrs-dag-vs-non-dag)
6. [Цикл воркера + epochs](#6-цикл-воркера--epochs)
7. [GlobalFrameCache](#7-globalframecache)
8. [Слойность core ↔ entities](#8-слойность-playa-app-ui--core--entities)
9. [От скрабба до пикселей](#9-от-скрабба-до-пикселей)
10. [Координатные пространства](#10-координатные-пространства)
11. [FSM кадра FileNode](#11-fsm-кадра-filenode)
12. [Главный цикл `PlayaApp::update`](#12-главный-цикл-playaappupdate)
13. [REST API ↔ главный поток](#13-rest-api--главный-поток)
14. [Модули `playa-app` / поток данных / события](#14-модули-playa-app--поток-данных--события)
15. [prefs: машина состояний окна](#15-playa-prefs--машина-состояний-окна)
16. [jobs: фасад и родственники](#16-playa-jobs--фасад-и-core-соседи)

---

## 1. Медиа → слои → композит

Обзорный поток (`playa-engine`): пул медиа, активная композиция, воркеры, кэш, CPU/GPU blend.

```
                         PLAYA: медиа → слои → композит
                         =================================

  ┌─────────────────────────────────────────────────────────────────────┐
  │  Project                                                             │
  │  media: HashMap<Uuid, Arc<NodeKind>>  wrapped in Arc<RwLock<…>>       │
  │         │                                                            │
  │         ├── FileNode  (path, загрузчик EXR/image/video…)             │
  │         ├── CompNode  (слои + размер + текущий кадр comp)            │
  │         ├── CameraNode, TextNode …                                   │
  └─────────────────────────────────────────────────────────────────────┘
              │
              │  UI / Player держит «активный» comp (uuid в attrs игрока)
              ▼

  ┌─────────────────────────────────────────────────────────────────────┐
  │  CompNode (активная композиция)                                      │
  │                                                                      │
  │   layers[0]  ─────────────►  фон (нижний в стеке пикселей)           │
  │   layers[1]                                                     │    │
  │     …                                                           │ Z  │
  │   layers[N-1] ─────────────►  передний план (верхний)           ▼    │
  │                                                                      │
  │   каждый Layer: ссылка на источник (uuid File/Comp/…) + эффекты      │
  │                 + transform + blend_mode + трим/speed …             │
  └─────────────────────────────────────────────────────────────────────┘


                         ЗАГРУЗКА ИСХОДНИКА (FileNode)
                         ---------------------------

     файл на диске
           │
           ▼
    ┌──────────────┐     try_claim      ┌──────────┐
    │ Placeholder/ │ ─────────────────► │ Loading  │
    │ Header       │    (один воркер)   └────┬─────┘
    └──────────────┘                         │
           │                                 │ decode в воркере
           │                                 ▼
           │                           ┌──────────┐
           └──────────────────────────►│ Loaded   │──► Frame в памяти
                     ошибка           └──────────┘         │
                                             │               │
                                             └───────────────┘
                                   (кэш композиции хранит уже ГОТОВЫЙ кадр comp,
                                    а не сырой файл целиком)


                    ЧТО ДЕЛАЕТ ВОРКЕР НА КАДРЕ Comp
                    ------------------------------

   scrub / смена кадра / AttrsChanged …
           │
           │  CacheManager.increment_epoch()   ◄── отмена «устаревших» задач
           ▼
   Workers::execute_with_epoch  ───►  CompNode::compose_internal(frame_idx, ctx)
                   │
                   │   порядок слоёв при сборке стека:
                   │   итерация  layers.iter().rev()
                   │   (сначала «верхние» источники в список — нижний слой фона
                   │    оказывается снизу при финальном blend)
                   │
                   ├── для каждого Layer:
                   │      • resolve источника по uuid из media pool (snapshot Arc)
                   │      • effects по цепочке на Frame
                   │      • geometry / transform → растр слоя
                   │      • режим смешивания (Normal, Screen, …)
                   │
                   ├── CPU prefs или fallback:
                   │      CpuCompositor на воркере (THREAD_COMPOSITOR)
                   │
                   └── GPU prefs + GpuBlendBridge:
                          воркер отдаёт стек ──► UI-поток drain → GpuCompositor (GL)

                           ▼

              ┌─────────────────────────────────┐
              │ GlobalFrameCache                │
              │   ключ: (comp_uuid, frame_idx)  │
              │   LRU + бюджет памяти           │
              └─────────────────────────────────┘
                           │
                           ▼
                   ViewportRenderer / encode / API …
                   (читают готовый Frame из кэша)


                    PRELOAD / DEBOUNCE (упрощённо)
                    ------------------------------

   DebouncedPreloader + CompNode::preload()
           │
           │  по радиусу вокруг playhead ставит задачи на те же compose/load пути
           │  (источники «рядом по времени» подтягиваются заранее)
           │
           └── пока слайдер двигается часто — только текущий кадр; полный preload
               после паузы (~500 ms)


              LEGEND (поток данных, не поток вызовов)
              --------------------------------------

    [диск] ──► FileNode state machine ──► пиксели слоя ──► blend ──► кэш comp ──► экран
```

**Кратко:** медиа в `Project.media`; активный `CompNode` задаёт слои; воркер композитит в кэш `(comp_uuid, frame)`; GPU-final через `GpuBlendBridge` на UI-поток.

---

## 2. Структура репозитория (workspace)

*Источник: `docs/AGENTS.md`, секция Project Layout.*

```
playa/
├── Cargo.toml          # workspace + thin `lib` aggregator; excludes playa-py
├── build.rs            # minimal, only cargo:rerun-if-changed
├── bootstrap.py        # vcpkg + VS env → `cargo xtask` (build, test, …)
├── crates/
│   ├── playa-app/      # PlayaApp + main_events + runner + cli + server + shell + config
│   ├── playa-engine/
│   ├── playa-events/
│   ├── playa-io/
│   ├── playa-ui/
│   ├── xtask/          # build automation (changelog, tags, build/test wrapper, wipe, deploy)
│   └── playa-py/       # Python bindings — separate workspace (`xtask`/maturin)
├── src/                # `main.rs`; `lib.rs` re-exports engine/ui/app for `playa::` API
├── AGENTS.md, README.md
├── CHANGELOG.md, DEVELOP.md, TODO.md, … # developer docs at repo root
```

---

## 3. Карта `src/` (binary + lib)

*Источник: `docs/AGENTS.md`.*

```
src/
├── main.rs             # binary: playa_io::init_ffmpeg → log → run_app
├── lib.rs              # re-exports playa_engine + playa_events + playa_ui + playa_app surfaces
└── README.md           # src-level notes only

(crates/playa-app/src mirrors the former monolith: app/, server/, runner, cli, shell, …)
```

---

## 4. EventBus: immediate vs deferred

*Источник: `docs/AGENTS.md`.*

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

---

## 5. Attrs: DAG vs non-DAG

*Источник: `docs/AGENTS.md`.*

```text
opacity (DAG)        → set() → schema.is_dag()=true  → dirty=true → invalidation
frame   (non-DAG)    → set() → schema.is_dag()=false → dirty untouched
node_pos in editor   → set() → not DAG               → cache not flushed
```

---

## 6. Цикл воркера + epochs

*Источник: `docs/AGENTS.md`.*

```text
Worker loop:
  1. own deque pop()         (FIFO — oldest first, so requests don't starve)
  2. injector.steal()         (global queue)
  3. steal from other workers (work stealing)
  4. shutdown? → exit
  5. sleep 1ms (no spin-burning CPU)
```

---

## 7. GlobalFrameCache

*Источник: `docs/AGENTS.md`.*

```
GlobalFrameCache:
  cache: RwLock<HashMap<Uuid, HashMap<i32, Frame>>>   ← per-comp sub-maps
  lru_order: Mutex<lru::LruCache<CacheKey, ()>>       ← O(1) get/put/pop_lru
  cache_manager: Arc<CacheManager>                    ← memory budget
  strategy: All | LastOnly                            ← All=keep everything in work area
```

---

## 8. Слойность playa-app/ui → core → entities

*Источник: `docs/AGENTS.md`.*

```
playa-app (+ playa-ui)  ──→  orchestration / EventBus handlers / PlayaApp state
                                      │
                                      ▼
                             playa-engine: core ──→ entities (via ComputeContext traits)
```

---

## 9. От скрабба до пикселей

*Источник: `docs/AGENTS.md`, секция «Data flow: from click to pixels».*

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
   → build `Vec<(Frame, opacity, BlendMode, inv_matrix)>` (same as before)
   → **Cpu prefs** (`ComputeContext.gpu_blend_bridge == None`): `THREAD_COMPOSITOR` /
     `CpuCompositor::blend_with_dim` on the worker
   → **Gpu prefs** + bridge wired: `GpuBlendBridge::delegate_blend_blocking` — stacks are blended
     on the **UI thread** when `PlayaApp::drain_gpu_blend_queue` runs `GpuBlendBridge::drain_into_compositor`
     against `project.compositor` (after `update_compositor_backend` / GL sync)
   → encode / blocking `get_frame`: always **no bridge** — Cpu compositor on that thread
8. global_cache.insert(comp, frame, result)
   → cache_manager.track_memory(size); if over the limit → evict LRU
   → mark_dirty() → main loop will call ctx.request_repaint()
9. ViewportRenderer.render(frame):
   → if pixel_format changed → recompile shader
   → glTexSubImage2D via PBO (double-buffered for async upload)
   → glDrawArrays via u_model * u_view * u_projection
```

---

## 10. Координатные пространства

*Источник: `docs/AGENTS.md`.*

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

---

## 11. FSM кадра FileNode

*Источник: `docs/AGENTS.md`.*

```
Placeholder ─┐
Header  ───── try_claim ───▶ Loading ──── success ──▶ Loaded
                              │              │
                              │              └── dehydrate ──▶ Expired ──▶ Loading
                              │
                              └── failure ──▶ Error
```

---

## 12. Главный цикл `PlayaApp::update`

*Источник: `docs/AGENTS.md`.*

```
1. exit_requested?               → Close viewport
2. start_api_server()            (lazy: on first frame, if enabled)
3. update_compositor_backend(gl) (CPU↔GPU per Settings — (re)binds `GpuCompositor` to current GL when Gpu)
4. drain_gpu_blend_queue(ctx)    unblocks workers blocked in `GpuBlendBridge::delegate_blend_blocking` (Gpu path)
5. apply theme/font              (last_applied_* guards)
6. handle_events()               poll EventBus → handle_app_event
7. process player.update()       (advances frame by wall-clock)
8. handle dropped files          (drag-drop)
9. DockArea.show(ctx, &mut DockTabs(self))
10. handle_keyboard_input()       (HotkeyHandler by focused window)
11. process API commands         (mpsc::Receiver<ApiCommand>)
12. update_api_state()          (writes SharedApiState under RwLock)
13. handle pending screenshots   (PNG via glReadPixels or from current frame)
14. cache_manager.take_dirty()   → ctx.request_repaint() if a load happened
```

---

## 13. REST API ↔ главный поток

*Источник: `docs/AGENTS.md`.*

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

---

## 14. Модули `playa-app` / поток данных / события

*Источник: `crates/playa-app/src/app/README.md`.*

### Обзор модулей

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

### Поток данных (от entry к EventBus)

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

### Поток событий (упрощённо)

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

---

## 15. playa-prefs: машина состояний окна

*Источник: `crates/playa-prefs/README.md`.*

```
                          open_with(state)
                          ──────────────→
                  ┌──── working_copy = state.clone() ────┐
                  │     last_applied = state.clone()     │
                  │                                       │
       Closed ────┴─→  Open ──Apply──→  Open (dirty=false, working=state)
                       │ │
                       │ └──OK──→  Closed (working_copy committed back)
                       │
                       └─Cancel─→  Closed (working_copy discarded)
```

---

## 16. playa-jobs: фасад и core-соседи

*Источники: `crates/playa-jobs/README.md`, `crates/playa-jobs-core/README.md`.*

### Фасад `playa-jobs`

```
                  ┌─→ playa-jobs-core     (always)
                  ├─→ playa-jobs-ui       (cfg(ui))
playa-jobs (facade)
                  ├─→ playa-prefs         (cfg(prefs))
                  ├─→ playa-job-seedance  (cfg(seedance))
                  └─→ playa-job-inpaint   (cfg(inpaint))
```

### Ядро и провайдеры

```
playa-jobs-core ──┬── playa-job-seedance     (Seedance i2v/t2v provider)
                  ├── playa-job-inpaint      (Flux Pro v1.1 inpaint provider)
                  ├── playa-jobs-ui          (egui panel + submit dialog)
                  └── playa-jobs             (facade — one dep for hosts)

playa-jobs-core ──> playa-events             (EventBus types)
```
