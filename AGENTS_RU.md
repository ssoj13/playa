# Playa Guide

Архитектурный гид для разработчиков и AI-ассистентов. Составлен по факту,
по rustdocs модулей и трассировке кода — не по слухам и не по старому README.

> Версия: **0.1.142** · Rust **edition 2024** · `target/release/playa[.exe]`
> EXR-бэкенд: **vfx-exr** (pure Rust, все компрессии включая DWAA/DWAB/HTJ2K).
> Видео: **playa-ffmpeg 8.0** (статически слинкованный FFmpeg).

---

## Project Layout

### Workspace

```
playa/
├── Cargo.toml          # workspace members: `.` + crates/playa-* + crates/xtask; `crates/playa-py` — в exclude
├── build.rs            # минимальный, только cargo:rerun-if-changed
├── bootstrap.py        # единый кросс-платформенный скрипт → `cargo xtask` (build, test, …)
├── AGENTS.md, AGENTS_RU.md, README.md
├── DEVELOP.md, CHANGELOG.md, TODO.md …
├── crates/
│   ├── playa-app/      # PlayaApp + main_events + runner + cli + server + shell + config
│   ├── playa-engine/
│   ├── playa-events/
│   ├── playa-io/
│   ├── playa-ui/
│   ├── xtask/          # утилиты сборки релиза (changelog, теги, wipe, …)
│   └── playa-py/       # PyO3 — отдельный workspace (`[workspace.exclude]`, maturin)
├── src/                # `main.rs`; `lib.rs` реэкспортирует crate API `playa::`
└── …
```

### Корень `src/` и крейты

**Распределение:** `crates/playa-engine` (`core`, `entities`, дефолты, утилиты), **`crates/playa-app`**
(`app/`, **`main_events`**, **`runner`**, **`cli`**, **`server/`**, **`shell`**, **`config`**),
**`crates/playa-io`**, **`crates/playa-events`**, **`crates/playa-ui`**
(`widgets/`, **`dialogs/`**, **`help`**, композиция UI). Корневой пакет **`playa`** — тонкая обёртка:
реэкспорты публичного API `playa::` (GUI и Python-биндинги).

```
src/
├── main.rs             # бинарник: init FFmpeg → CLI → лог → run_app
├── lib.rs              # реэкспортирует основные интерфейсы playa_* 
└── README.md           # кратко про структуру верхнего уровня

(`crates/playa-app/src` — бывший «монолит»: app/, server/, runner, cli, shell, …)
```

---

## Архитектурные принципы

### 1. Event-Driven, без прямых вызовов между виджетами

Виджеты **не вызывают друг друга** и не лезут в `PlayaApp` напрямую. Вместо
этого они эмитят типизованные события в `EventBus` (`playa_engine::core::event_bus`).

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

**Почему так**:
- Виджет ничего не знает про получателя — кэш можно очистить, не зная виджета.
- `egui` рендерит UI каждый кадр, сложно встроить колбэки → отложенная обработка.
- Поллинг в `update()` атомарно меняет состояние батчем перед следующим рендером.

**Ловушка с `downcast_event`**: бланкетная импла `impl<T> Event for T` означает, что
`Box<dyn Event>` сам реализует `Event`. Если писать `event.as_any()`, резолвер
методов может выбрать импл на `Box`, а не внутренний тип. Поэтому в
`event_bus::downcast_event()` используется **`(**event).as_any()`**, чтобы
принудительно пройти через vtable. Не упрощать.

**Категории событий** (живут рядом с виджетами и сущностями):

| Файл | События |
|------|---------|
| `core/player_events.rs` | `SetFrameEvent`, `TogglePlayPauseEvent`, `Step{F,B}*`, `Jump*`, `Jog{F,B}` |
| `core/layout_events.rs` | `ResetLayout`, `LayoutSelected/Created/Deleted/Updated/Renamed` |
| `entities/comp_events.rs` | `CurrentFrameChangedEvent`, `LayersChangedEvent`, `AttrsChangedEvent` |
| `widgets/project/project_events.rs` | `AddClip(s)`, `AddFolder`, `AddComp/Camera/Text`, `RemoveMedia`, `ClearCache` |
| `widgets/timeline/timeline_events.rs` | `Timeline{Zoom,Pan,Snap,LockWorkArea}*`, `TimelineFitEvent`, ... |
| `widgets/viewport/viewport_events.rs` | `FitViewportEvent`, `Viewport100Event`, `ViewportRefreshEvent` |
| `widgets/viewport/tool.rs` | `SetToolEvent(ToolMode)` |
| `dialogs/prefs/prefs_events.rs` | `SetGizmoPrefsEvent`, hotkey-окна |

### 2. Project не принадлежит Player'у

`Player` хранит **только воспроизведение** в собственных `Attrs`. `Project`
живёт в `PlayaApp` (единственный источник правды). Методы плеера, которым
нужен проект, принимают `&mut Project` параметром.

**Почему**: раньше Player владел Project'ом, и возникала дупликация — UI и
плеер расходились. Теперь оба смотрят на один экземпляр; невозможно по ошибке
править копию.

**Ключи Player.attrs**: `active_comp`, `previous_comp_history`, `is_playing`,
`fps_base` (постоянный), `fps_play` (временный для J/L шаттла), `loop_enabled`,
`play_direction` (1.0/-1.0), `selected_seq_idx`.

### 3. Node-граф через `enum_dispatch`

```rust
#[enum_dispatch(Node)]
pub enum NodeKind { File(FileNode), Comp(CompNode), Camera(CameraNode), Text(TextNode) }
```

`Node` — общий трейт (uuid, attrs, inputs, compute, is_dirty, preload, _in/_out/fps/dim/...).
`enum_dispatch` генерирует ноль-стоимостный диспатч (без `Box<dyn Node>`).
`is_renderable()` возвращает `false` для `Camera` (не даёт пикселей).

`Project.media: Arc<RwLock<HashMap<Uuid, Arc<NodeKind>>>>` — внутренние `Arc`
позволяют воркерам **сделать снапшот** (клонировать `HashMap` арков за
микросекунды) и сразу отпустить лок, пока тяжёлая `compute()` бежит 50–500 мс.
UI никогда не блокируется чтением воркера.

### 4. Attrs со схемой → автоинвалидация кэша

`Attrs` — общий контейнер для Frame, Layer, Comp, Camera, Project. У каждого
типа есть `*_SCHEMA` в `attr_schemas.rs`, описывающая флаги атрибутов:

| Флаг | Эффект |
|------|--------|
| `FLAG_DAG`     | Изменение → `dirty=true` → инвалидация кэша рендера |
| `FLAG_DISPLAY` | Показывать в Attribute Editor |
| `FLAG_KEYABLE` | Можно анимировать ключами |
| `FLAG_READONLY`| Только чтение (вычисленное) |
| `FLAG_INTERNAL`| Скрытое, не показывать пользователю |

```text
opacity (DAG)        → set() → schema.is_dag()=true  → dirty=true → инвалидация
frame   (не DAG)     → set() → schema.is_dag()=false → dirty не трогается
node_pos в редакторе → set() → не DAG               → кэш не сбрасывается
```

**Зачем**: можно двигать playhead и selection без сноса кэша. А «опасные»
изменения (трансформ, opacity, blend_mode) автоматически шлют `AttrsChangedEvent`.

### 5. `project.modify_comp(uuid, |comp| ...)` — единственный способ правки

```rust
project.modify_comp(uuid, |comp| {
    comp.set_child_attrs(layer, &values);   // attrs.set() → dirty=true
});
// modify_comp проверяет is_dirty() и эмитит AttrsChangedEvent
// → handler в main_events.rs:
//     1. cache_manager.increment_epoch()  — отменяет старые задачи воркеров
//     2. global_cache.clear_comp(uuid)    — выбрасывает кадры из кэша
//     3. preloader перезапускает загрузку
```

Любая прямая правка `comp.layers.push/insert/remove` или `layer.attrs.set` **не
проходит через геттеры** и обязательна вручную: `comp.attrs.mark_dirty()` —
иначе UI покажет старый кадр.

`modify_comp()` использует `event_emitter: Option<EventEmitter>` (помечено
`#[serde(skip)]`). После десериализации **обязательно** вызвать
`project.set_event_emitter(event_bus.emitter())` — иначе тихая поломка кэша.

### 6. Work-stealing воркеры с эпохами

`Workers` (`crates/playa-engine/src/core/workers.rs`) — пул потоков с **per-worker FIFO деками**
+ глобальным `Injector`:

```text
Worker loop:
  1. own deque pop()         (FIFO — старое первым, чтобы не голодали запросы)
  2. injector.steal()         (глобальная очередь)
  3. steal у других воркеров (work stealing)
  4. shutdown? → exit
  5. sleep 1ms (без spin-burn CPU)
```

Размер пула: `num_cpus::get() * 3 / 4` (25% оставляем UI).

**Эпохи** (`Arc<AtomicU64>` шарится с `CacheManager`): при скрабинге UI
быстро инкрементит `current_epoch`. Воркер перед компоновкой/загрузкой
сравнивает свой эпох с текущим — **если устарел, пропускает работу**.
Без этого пользователь, протащив playhead с 0 на 500, заставил бы воркеры
выкачать 500 ненужных кадров.

### 7. LRU-кэш с трекингом памяти

```
GlobalFrameCache:
  cache: RwLock<HashMap<Uuid, HashMap<i32, Frame>>>   ← per-comp подмапы
  lru_order: Mutex<lru::LruCache<CacheKey, ()>>       ← O(1) get/put/pop_lru
  cache_manager: Arc<CacheManager>                    ← бюджет памяти
  strategy: All | LastOnly                            ← All=держим всё в work area
```

- **O(1) clear_comp**: убрать внешний ключ `Uuid` — внутренняя мапа отправлена в
  drop, LRU эвикты считаются обычным push'ом.
- **`dehydrate=true`**: метит `Loaded → Expired`, пиксели остаются (быстро).
  `false`: полностью убирает из кэша (освобождает память).
- **Бюджет памяти**: `CacheManager::new(0.75, 2.0)` — 75% от
  `sysinfo::available_memory()` минус 2 ГБ резерва системе. Лимит атомарный,
  можно менять без перестройки кэша.
- **`dirty_repaint: AtomicBool`**: воркер ставит `true` после `insert`, главный
  цикл `take_dirty()` → `ctx.request_repaint()`. Иначе egui спал бы пока курсор
  не дёрнут.

### 8. `DebouncedPreloader` — 500 мс перед полным preload

При быстрой правке атрибутов (например, slider opacity) cache бы топтался: сбрось
кэш → загрузи 50 кадров → опять сбрось → опять загрузи. `DebouncedPreloader`
держит `(comp_uuid, trigger_time)`; `tick()` возвращает `Some(uuid)` только если
с момента `schedule()` прошло ≥ 500 мс. До этого грузится **только текущий кадр**.

### 9. Dependency inversion: `core` зависит от `entities`

`entities/traits.rs` определяет интерфейсы (`FrameCache`, `WorkerPool`,
`CacheStrategy`), которые **сами entity** ожидают от инфраструктуры. Конкретные
реализации (`GlobalFrameCache`, `Workers`) живут в `core/`. Граф зависимостей:

```
app  ──→ widgets, dialogs, server, main_events
         │      │       │        │
         ▼      ▼       ▼        ▼
         core ──→ entities (через trait-объекты в ComputeContext)
```

`ComputeContext` несёт `&dyn FrameCache`, `Option<&dyn WorkerPool>` — нода не
знает реальные типы и тестируема в изоляции.

---

## Поток данных: от клика к пикселям

```text
1. User scrub      — drag по таймлайну
2. SetFrameEvent   — emit, сразу + в очередь
3. main_events::handle_app_event
   → project.modify_comp(active, |c| c.set_frame(target))
   → set_frame правит non-DAG attr → НЕ dirty
   → modify_comp эмитит CurrentFrameChangedEvent (frame изменился)
4. handle_events ловит CurrentFrameChangedEvent:
   → enqueue_frame_loads_around_playhead(preload_radius)
5. cache_manager.increment_epoch()      — старые задачи воркеров протухли
6. workers.execute_with_epoch(epoch, job)
   → если worker_epoch != current_epoch → skip
   → else compose_internal(comp, frame, ctx)
7. compose_internal:
   → для каждого layer (layers.iter().rev() — снизу-вверх):
       a) source_node = ctx.media[source_uuid]
       b) source_frame = source_node.compute(layer_frame, ctx) (рекурсивно)
       c) for fx in layer.effects: source_frame = fx.apply(source_frame)
       d) transform::apply (rayon par_chunks_mut, sample_bilinear)
       e) push (frame, opacity, blend_mode, inv_matrix) в Vec
   → CpuCompositor.blend_with_dim(frames, dim) — Porter-Duff в blend_f32
   → unify formats: blend_u8/blend_f16 декодируют в f32, делегируют, кодируют
8. global_cache.insert(comp, frame, result)
   → cache_manager.track_memory(size); если за лимитом → evict LRU
   → mark_dirty() → главный цикл вызовет ctx.request_repaint()
9. ViewportRenderer.render(frame):
   → если pixel_format поменялся → recompile shader
   → glTexSubImage2D через PBO (двойной буфер для асинхронной загрузки)
   → glDrawArrays через u_model * u_view * u_projection
```

---

## Координатные пространства

```
+──────────────+   +──────────────────────+   +──────────────+
│ IMAGE        │   │ FRAME (= Viewport)   │   │ OBJECT       │
│ origin: TL   │   │ origin: CENTER       │   │ origin:      │
│ +Y down      │   │ +Y up                │   │  layer center│
│              │   │                      │   │ +Y up        │
│ pixels       │   │ pixels               │   │ pixels       │
+──────────────+   +──────────────────────+   +──────────────+
   loader               position              для rotation/scale
   текстуры             gizmo                 вокруг pivot
```

```
Screen pixel ──image_to_frame──▶ Frame ──inv model──▶ Object ──object_to_src──▶ Source pixel
```

**Ротации**: порядок ZYX (как в After Effects). Пользовательская конвенция —
по часовой = «+» (`CW+`); `glam` использует математическую (`CCW+`),
поэтому **углы инвертируются** при вызове `glam::Quat::from_euler` —
см. `space::to_math_rot` / `from_math_rot`.

**Перспективная проекция**: CPU-композитор делает обратное отображение «для
каждого выходного пикселя — найти исходный». При перспективе нельзя просто
умножить на обратную MVP, поэтому используется
**ray–plane intersection**: луч из камеры через пиксель → пересечение с
плоскостью слоя в мировом пространстве (`transform::unproject_to_plane`).
Ортография идёт быстрым путём через обратную аффинную матрицу.

---

## Загрузчики

| Тип | Бэкенд | Расширения |
|-----|--------|-----------|
| EXR | `vfx-exr` (path-dep, pure Rust) | `.exr` — все компрессии включая DWAA/DWAB/HTJ2K |
| Generic | `image` 0.25 | `.png .jpg .jpeg .tif .tiff .tga .hdr` |
| Video | `playa-ffmpeg` 8.0 (статика) | `.mp4 .mov .avi .mkv` |

`loader::classify_ext` диспатчит на `header_*` и `load_*`. `header_*`
читает только заголовок (для FileNode при добавлении в проект),
полный декод откладывается до запроса кадра воркером.

**Видео-метаданные**: `VideoMetadata::from_file` гардом `denom != 0`
(BUG-04 fix), `frame_count = (duration_secs * fps).round()` (BUG-13 fix —
`as usize` теряло половину последнего кадра).

**Frame status FSM**:

```
Placeholder ─┐
Header  ───── try_claim ───▶ Loading ──── success ──▶ Loaded
                              │              │
                              │              └── dehydrate ──▶ Expired ──▶ Loading
                              │
                              └── failure ──▶ Error
```

`try_claim_for_loading()` атомарно делает `Header → Loading`, чтобы два
воркера не качали один и тот же файл (TOCTOU race).

---

## Эффекты слоя

```rust
Layer {
    attrs: Attrs,
    effects: Vec<Effect>,   // применяются по порядку ДО transform/blend
}
```

| Тип | Параметры | Заметки |
|-----|-----------|--------|
| `GaussianBlur` | `radius: 0–100` | Сепарабельный: `convolve_axis(true)` H, `convolve_axis(false)` V — единая функция, ось переключается параметром |
| `BrightnessContrast` | `brightness: -1..1`, `contrast: -1..1` | На пиксель |
| `AdjustHSV` | `hue_shift: -180..180`, `saturation: 0..2`, `value: 0..2` | Вынесена в `adjust_hsv()` — единственный путь rgb→hsv→adj→rgb |

**Принцип DRY в blend/transform/effects**: U8/F16/F32 ветки не дублируют
бизнес-логику — декодируют в f32, делегируют общей f32-функции, кодируют
обратно. То же для `transform::sample_bilinear<T>(decode: impl Fn(T) → f32)`
с rayon-макросом для параллельных арм.

---

## Компоновка: CPU vs GPU

| Компонент | Где | Состояние |
|-----------|-----|-----------|
| `CpuCompositor` | работает везде, в т.ч. в воркерах | основной путь |
| `GpuCompositor` | OpenGL FBO + GLSL, 10–50× быстрее | **viewport-only**, не используется в `compose_internal` |

Интерфейс `CompositorType::blend()` принимает `Vec<(Frame, opacity, BlendMode, [f32; 9])>`
с матрицами 3×3 (в column-major для GL) — API единый. Однако `compose_internal`
бежит в воркерах, где GL-контекст недоступен (контекст принадлежит главному
потоку eframe). Поэтому реальное GPU-композитирование пока используется только
для viewport-эффектов, а слои блендятся CPU. План перехода описан в шапке
`compositor.rs`.

`BlendMode`: Normal · Screen · Add · Subtract · Multiply · Divide · Difference · Overlay
(в `apply_blend()` единственное место с Porter–Duff формулами).

---

## Главный цикл (`PlayaApp::update`)

```
1. exit_requested?               → Close viewport
2. start_api_server()            (lazy: на первом кадре, если включено)
3. update_compositor_backend(gl) (CPU↔GPU по Settings)
4. apply theme/font              (guard'ы last_applied_*)
5. handle_events()               poll EventBus → handle_app_event
6. process player.update()       (продвигает frame по wall-clock)
7. handle dropped files          (drag-drop)
8. DockArea.show(ctx, &mut DockTabs(self))
9. handle_keyboard_input()       (HotkeyHandler по сфокусированному окну)
10. process API commands         (mpsc::Receiver<ApiCommand>)
11. update_api_state()           (пишет SharedApiState под RwLock)
12. handle pending screenshots   (PNG из glReadPixels или из current frame)
13. cache_manager.take_dirty()   → ctx.request_repaint() если была загрузка
```

**Hotkey routing** — `HotkeyHandler` хранит `(HotkeyWindow, key) → EventFactory`.
Сначала ищется по сфокусированному окну (Viewport / Timeline / Project /
NodeEditor / Settings / Encode / Hotkeys), потом fallback на `Global`.
Это позволяет, например, `Delete` в Project удалять медиа, а в Timeline —
слой.

---

## Persistence

- Окно: `eframe` сам сохраняет позицию/размер (`persist_window: true`),
  `persistence_path` указан в `runner.rs` через `config::config_file("playa.json")`.
- App state: `eframe` через `eframe::APP_KEY` сериализует `PlayaApp` в тот же
  json (`#[serde(default)]`, runtime-only поля помечены `#[serde(skip)]`).
- Project: `Project::to_json` / `Project::from_json` — отдельный диск-формат для
  «плейлистов»; `--playlist <FILE>` грузит при старте.
- Шейдеры: `shaders/` рядом с бинарником подхватываются `Shaders::load_shader_directory`.

**Платформенные пути** (через `dirs-next`):

| ОС | config | data |
|----|--------|------|
| Linux | `~/.config/playa/` | `~/.local/share/playa/` |
| macOS | `~/Library/Application Support/playa/` | то же |
| Windows | `%APPDATA%\playa\` | то же |

Override: CLI `--config-dir`, `PLAYA_CONFIG_DIR` ENV, либо локальный каталог
(если в нём уже лежат `playa.json`/`playa.log` — режим «portable»).

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
        │                                       writes каждый кадр
```

Биндинг `127.0.0.1:port` (loopback only). FPS-валидация в обработчике:
`is_finite() && > 0.0 && <= 960.0`. Эндпоинты:
`status / player / comp / cache / health / play / pause / stop / frame/N /
fps/N / toggle-loop / project/load / event / next / prev / screenshot / exit`.

**Скриншоты**: `Screenshot { viewport_only: bool, response: crossbeam::Sender }`.
Если viewport_only — `glReadPixels` через `frame.read_pixels()` после рендера;
иначе сериализация текущего `Frame` в PNG.

---

## Layouts

`AppSettings.layouts: HashMap<String, Layout>` — именованные раскладки (dock-сплиты,
timeline state, viewport state). События в `core/layout_events.rs`:
`LayoutSelected/Created/Deleted/Updated/Renamed`. Старые
`SaveLayoutEvent`/`LoadLayoutEvent` удалены — их заменила структурированная схема
с автогенерацией имён («Layout 2», «Layout 3», ...).

`build_dock_state(show_project, show_attributes, split_pos)` пересобирает дерево
egui_dock с конфигурируемой видимостью Project/Attributes панелей.

---

## Build-конвейер

`python bootstrap.py build` (по умолчанию **release**; `-d` / `--debug` — debug) задаёт
`VCPKG_ROOT` / `VCPKGRS_TRIPLET`, при необходимости MSVC-окружение на Windows и вызывает **`cargo xtask build`**.
Тонкий **`build.rs`**; нативные либы — через Cargo + **vcpkg**, см. **`DEVELOP.md`**.

```
python bootstrap.py build
python bootstrap.py build -d
python bootstrap.py test
cargo xtask build [--release|--debug]
cargo xtask test [--debug] [--nocapture]
cargo xtask deploy [--install-dir P]
cargo xtask changelog
cargo xtask tag-dev / tag-rel / pr
cargo xtask wipe
cargo xtask wipe-wf
```

**vcpkg для FFmpeg** — обязательно. Триплеты: `x64-windows-static-md-release`,
`x64-linux-release`, `arm64-osx-release`, `x64-osx-release`. ENV: `VCPKG_ROOT`,
`VCPKGRS_TRIPLET`, `PKG_CONFIG_PATH`. Подробности — в **`DEVELOP.md`**.

**Релизный профиль**: `strip = false`, `lto = false`, `codegen-units = 1`
закомментирован — оптимизировано на скорость линковки, не на размер.

**Особенность Windows**: статическая сборка без DLL (триплет `static-md`).
**macOS**: подписан Developer ID `Y8PQ7YASU9`, нотарификация отключена в metadata.

---

## CLI

```
playa [OPTIONS] [FILE]
  -f, --file FILE          доп. файлы (множественно)
  -p, --playlist FILE      playlist (Project::from_json)
  -F, --fullscreen
      --frame N            стартовый кадр
  -a, --autoplay
  -o, --loop 0|1           default 1
      --start N --end N    play range
      --range S E          shorthand
  -l, --log [FILE]         лог в файл
  -v..-vvv                 warn/info/debug/trace
  -c, --config-dir DIR     override platform paths
```

`--mem` и `--workers` помечены `hide = true` — пережиток старого кэша, в коде
читаются для ENV-fallback'а конфигурации воркеров.

Версия (`-V`):
```
0.1.142
EXR:    vfx-exr (pure Rust, all compressions)
Video:  playa-ffmpeg 8.0 (static)
Target: x86_64-windows
```

---

## Правила работы с кодом

### Rust

- В продакшен-коде **избегать** `unwrap()`/`expect()`. Исключения:
  тесты, восстановление после `PoisonError` (`unwrap_or_else(|e| e.into_inner())`).
- Ошибки распространяем через `Result<_, FrameError>` / `anyhow::Result` + `?`.
- Не глотать ошибки молча. `log::warn!` или `log::error!` минимум.
- `Arc::clone(&x)` вместо `x.clone()` для явности.
- Не плодить зависимости — Cargo.toml уже широкий.
- `serde(skip)` на runtime-полях; **обязательно** восстанавливать после
  десериализации (event_emitter, schemas, cache_manager) — см. **`crates/playa-app/src/runner.rs`**.

### Tokio / Async

В проекте **нет** Tokio. Воркеры — это `std::thread`, очереди — crossbeam,
HTTP — `rouille` (синхронный). Не вводить async-runtime без явной необходимости.
Не блокировать главный поток — `Workers::execute(job)` для тяжёлых задач.

### Edits / Refactors

- Минимальный диффф. Не рефакторить по дороге.
- Имена и стиль — как у соседей.
- Не делать форматирующих-only коммитов.
- Если меняешь `Comp.layers` напрямую — `comp.attrs.mark_dirty()` в той же
  транзакции `modify_comp`.
- Если добавляешь атрибут — описать его в соответствующей `*_SCHEMA` с
  правильными флагами (`DAG` обязателен для всего, что влияет на пиксели).

### Добавление NodeKind

1. `entities/foo_node.rs` со структом + `impl Node`.
2. Вариант в `enum NodeKind`.
3. Схема в `attr_schemas.rs` (композировать общие `IDENTITY`, `TIMING`, `TRANSFORM`).
4. Пометить `is_renderable()` и `is_listed()` если нужно.
5. Если есть `add_child_layer` — обновить `NodeKind::add_child_layer()`.

### Добавление события

1. Структ в правильном `*_events.rs` (рядом со «своим» доменом).
2. Эмит: `event_bus.emit(MyEvent { ... })` или через `ActionQueue`.
3. Обработка: `if let Some(e) = downcast_event::<MyEvent>(&event)` в
   `app/events.rs::handle_events` или `main_events.rs::handle_app_event`.
4. Если событие меняет проект — внутри `project.modify_comp` чтобы
   автоинвалидация сработала.

### Добавление эффекта

1. `entities/effects/foo.rs` с функцией `apply(&Frame, &Effect) → Frame`.
2. Вариант в `EffectType` enum.
3. Схема `FX_FOO_SCHEMA` (поля с `FLAG_DAG | FLAG_DISPLAY | FLAG_KEYABLE`).
4. Match-арм в `effects::schema()` и `effects::apply()`.

---

## Платформа разработки (для AI/контекст)

- **Windows 11**, PowerShell 7+ (`pwsh`). Не `bash`. Вместо `/dev/null` —
  `$null`; экранировать слэши `\` или использовать прямые `/` где принимается.
- **vcpkg** в `C:\vcpkg`, ENV: `$env:VCPKG_ROOT`. При сборке нужен MSVC: **Developer PowerShell for VS**
  или активированный `vcvars64.bat`.
- **Sciter / Flutter** не используются (это к RustDesk относится). Здесь
  один UI — egui/eframe + glow OpenGL.

---

## Сюрпризы и подводные камни

| Где | Что | Почему важно |
|-----|-----|--------------|
| `event_bus::downcast_event` | `(**event).as_any()` обязательно | Бланкетная импла на `Box<dyn Event>` ломает простой `event.as_any()` |
| `project.set_event_emitter` | вызывать после каждой десериализации | `event_emitter` в `#[serde(skip)]` — без восстановления модификации не инвалидируют кэш |
| `compose_internal` rev order | `layers.iter().rev()` | `layers[0]` — фон, `layers[N-1]` — передний; источники в `Vec` собираются снизу-вверх |
| `trim_in/trim_out` | **смещения, не абсолюты** | `work_start = _in + trim_in`, `work_end = _out - trim_out`. Для Layer — в исходных кадрах, потом масштабируются `speed` |
| `enum_dispatch` shadow | методы `fps/_in/_out/frame` **не** дублируем в `impl NodeKind` | Дубликат тенирует трейтовый метод, тесты падают |
| Rotation sign | `space::to_math_rot(deg)` инвертирует | UI — CW+, glam — CCW+ |
| Cache LRU | используем `lru::LruCache`, не свою `IndexSet` | O(1) вместо O(n) `shift_remove` |
| `process_blocking` в воркерах | нет — воркеры это `std::thread::sleep(1ms)` | Никаких async-вложенных рантаймов |
| `THREAD_COMPOSITOR` | `thread_local!` намеренно | Воркер не имеет GL-контекста, разделять `RefCell<Compositor>` через потоки нельзя |
| GPU compositor | пока **viewport-only** | `compose_internal` бежит в воркерах без GL — план миграции в шапке `compositor.rs` |

---

## Структурные диаграммы

Разделы этого файла («поток кадров», LRU-кэш, граф узлов и т.д.) дают структуру понятий.
Подробнее про **сборку** — **[`DEVELOP.md`](DEVELOP.md)** (vcpkg, FFmpeg).

---

*Базис: rustdocs модулей под `crates/*/src/**/*.rs`. Если текст расходится с кодом — верьте исходникам.*
