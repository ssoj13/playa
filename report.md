# Playa - Research Report

## 1. Профилирование производительности UI (Preload тормоза)

### Проблема
UI тормозит на preload операциях. Нужно понять где bottleneck.

### Опции профилирования для Rust

#### A. **Tracy Profiler** (Рекомендуется)
- **Что это**: Профайлер реального времени с flamegraph, GPU profiling, memory tracking
- **Плюсы**: Визуализация в реальном времени, минимальный overhead, поддержка egui
- **Минусы**: Требует запуска Tracy сервера отдельно
- **Интеграция**:
  ```toml
  [dependencies]
  tracy-client = "0.17"
  ```
  ```rust
  tracy_client::span!("frame_load");
  ```

#### B. **puffin + puffin_egui** (Хороший вариант для egui)
- **Что это**: Встроенный профайлер прямо в egui окно
- **Плюсы**: Интегрируется прямо в UI, не нужен внешний софт
- **Минусы**: Меньше фич чем Tracy
- **Интеграция**:
  ```toml
  puffin = "0.19"
  puffin_egui = "0.30"
  ```

#### C. **cargo flamegraph** (Простой вариант)
- **Что это**: CLI тул, генерит SVG flamegraph
- **Плюсы**: Просто запустить, не требует изменения кода
- **Минусы**: Только постфактум анализ, нет real-time
- **Команда**: `cargo flamegraph --bin playa`

#### D. **Tracing + chrome tracing**
- **Что это**: Экспорт в Chrome DevTools формат
- **Интеграция**: `tracing-chrome` crate

### План тестирования

1. **Baseline**: Замерить время от запуска до первого отрисованного фрейма
2. **Instrumentация**:
   - `DebouncedPreloader::schedule/tick` - время debounce
   - `ViewportRenderer::upload_texture` - время GPU upload
   - `loader.rs` загрузка файлов
   - `compositor.rs` композитинг слоёв
3. **Метрики**:
   - Frame time (должен быть <16ms для 60fps)
   - Texture upload time
   - File I/O time (OpenEXR, JPEG, etc)
   - Cache hit/miss ratio

### Рекомендация
**puffin + puffin_egui** - проще всего интегрировать в существующий egui код, результат видно сразу в окне приложения.

---

## 2. Манипуляторы (Move/Rotate/Scale Gizmos)

### Требования
- Move: XYZ стрелки
- Rotate: XYZ круги
- Scale: XYZ оси с квадратами на концах
- Hover highlight
- Drag interaction
- Обновление атрибутов слоя

### Опции рендеринга

#### A. **OpenGL Overlay поверх egui** (Рекомендуется)
- **Как**: Отдельный render pass после egui, в том же glow контексте
- **Плюсы**: Полный контроль, быстро, можно делать depth testing
- **Минусы**: Нужно писать geometry самому
- **Реализация**:
  ```rust
  // В viewport.rs после egui render
  fn render_gizmo(&self, gl: &glow::Context, transform: &Transform) {
      // Рисуем манипулятор поверх
  }
  ```

#### B. **egui_gizmo crate**
- **Что это**: Готовая реализация 3D gizmo для egui
- **Плюсы**: Уже написано, стандартный API
- **Минусы**: Может не подойти для 2D compositing use case
- **Crate**: `egui-gizmo = "0.18"`

#### C. **egui Painter API (2D)**
- **Что это**: Рисование через egui shapes
- **Плюсы**: Простая интеграция, работает везде
- **Минусы**: Нет depth, только 2D

### Layer Picking (ID Buffer)

#### Реализация через OpenGL back buffer:
```rust
// 1. Создать offscreen FBO с текстурой R32UI
// 2. Рендерить каждый слой с уникальным ID в fragment shader
// 3. glReadPixels под курсором мыши
// 4. ID -> Layer UUID lookup

// Fragment shader:
// out uint layer_id;
// uniform uint u_layer_id;
// void main() { layer_id = u_layer_id; }
```

#### Альтернатива - Ray casting:
- Проще для 2D случая
- Проверяем пересечение луча мыши с bounding box каждого слоя
- Нет дополнительных GPU ресурсов

### Архитектура манипулятора

```
GizmoMode: Move | Rotate | Scale | Combined
GizmoAxis: X | Y | Z | XY | XZ | YZ | All
GizmoState:
  - hovered_axis: Option<GizmoAxis>
  - dragging: Option<(GizmoAxis, Vec2 start_pos, Transform start_transform)>

// Events:
- MouseMove -> update hover
- MouseDown on axis -> start drag
- MouseDrag -> compute delta, update layer transform attrs
- MouseUp -> commit transform
```

### Рекомендация
1. **OpenGL overlay** для gizmo рендеринга (полный контроль)
2. **Ray casting** для picking (проще чем ID buffer для 2D)
3. Использовать существующий `Transform` в `src/entities/transform.rs`

---

## 3. Python API

### Опции

#### A. **PyO3** (Рекомендуется)
- **Что это**: Rust <-> Python биндинги
- **Плюсы**: Зрелый, хорошая документация, async support
- **Минусы**: Нужен отдельный .pyd/.so модуль
- **Пример**:
  ```rust
  use pyo3::prelude::*;

  #[pyclass]
  struct Player {
      inner: Arc<Mutex<playa::Player>>
  }

  #[pymethods]
  impl Player {
      fn play(&self) { ... }
      fn stop(&self) { ... }
  }

  #[pymodule]
  fn playa(m: &Bound<'_, PyModule>) -> PyResult<()> {
      m.add_class::<Player>()?;
      Ok(())
  }
  ```

#### B. **Embedded Python** (Python внутри Playa)
- **Через**: `pyo3` с feature `auto-initialize`
- **Плюсы**: Скрипты прямо в приложении
- **Минусы**: Увеличивает размер бинарника

#### C. **IPC + JSON-RPC**
- **Как**: Отдельный Python процесс общается через socket/pipe
- **Плюсы**: Изоляция, можно использовать любой Python
- **Минусы**: Latency, сложнее отладка

### Предлагаемая объектная модель

```python
import playa

# Singleton или создание инстанса
app = playa.app()  # или playa.get_instance()

# Project
prj = app.project
prj.save("path.playa")
prj.load("path.playa")

# Media / Clips
clip = prj.import_clip("/path/to/sequence.####.exr")
folder = prj.new_folder("Renders")
folder.add(clip)

# Compositions
comp = prj.new_comp("Main", width=1920, height=1080, fps=24)
comp.add_layer(clip, in_point=0, duration=100)

# Timeline
timeline = comp.timeline
timeline.frame = 50
timeline.play()
timeline.stop()
timeline.play_range = (10, 90)

# Viewport
viewport = app.viewport
viewport.zoom = 1.0
viewport.pan = (0, 0)
viewport.fit()

# Layers
for layer in comp.layers:
    layer.transform.position = (100, 200)
    layer.transform.rotation = 45
    layer.transform.scale = (1.5, 1.5)
    layer.opacity = 0.8

# Node Editor (если есть)
node_editor = app.node_editor
# ...

# Events / Callbacks
@app.on("frame_changed")
def on_frame(frame: int):
    print(f"Frame: {frame}")
```

### Рекомендация
**PyO3** с отдельным `playa-python` crate в workspace. Это даст:
- `pip install playa` для пользователей
- Scripting внутри приложения через embedded Python (опционально)

---

## 4. Web Server / REST API

### Опции

#### A. **Axum** (Рекомендуется)
- **Что это**: Async web framework от Tokio team
- **Плюсы**: Быстрый, ergonomic, хорошая экосистема
- **Минусы**: Async runtime (Tokio) нужен
- **Пример**:
  ```rust
  async fn play() -> impl IntoResponse {
      // send command to player
      Json({"status": "playing"})
  }

  let app = Router::new()
      .route("/api/play", post(play))
      .route("/api/stop", post(stop))
      .route("/api/frame/:n", post(set_frame));
  ```

#### B. **Actix-web**
- **Плюсы**: Очень быстрый, mature
- **Минусы**: Свой runtime, сложнее интегрировать с egui

#### C. **Warp**
- **Плюсы**: Functional style, composable
- **Минусы**: Менее популярен сейчас

#### D. **Tiny-http** (Минималистичный)
- **Плюсы**: Без async, простой
- **Минусы**: Меньше фич, блокирующий

### Предлагаемые REST endpoints

```
POST /api/player/play
POST /api/player/stop
POST /api/player/frame         { "frame": 50 }
GET  /api/player/status        -> { "playing": true, "frame": 50, "fps": 24 }

POST /api/project/open         { "path": "..." }
POST /api/project/save
GET  /api/project/info         -> { "name": "...", "comps": [...] }

POST /api/comp/{id}/activate
GET  /api/comp/{id}/layers     -> [{ "id": "...", "name": "...", "transform": {...} }]
PUT  /api/comp/{id}/layer/{lid}/transform  { "position": [x, y], "rotation": 45 }

GET  /api/viewport/frame.png   -> текущий кадр как PNG
GET  /api/viewport/frame.exr   -> текущий кадр как EXR (HDR)

# WebSocket для real-time updates
WS  /api/ws                    -> { "event": "frame_changed", "frame": 51 }
```

### Архитектура интеграции

```
┌─────────────┐     channel      ┌─────────────┐
│   Web Server │ <-------------> │  PlayaApp   │
│   (Tokio)    │   Command/Event │  (egui)     │
└─────────────┘                  └─────────────┘

// Общение через crossbeam channel:
enum WebCommand {
    Play,
    Stop,
    SetFrame(i32),
    GetStatus(oneshot::Sender<Status>),
}

// В update() проверяем команды от web server
while let Ok(cmd) = web_rx.try_recv() {
    match cmd { ... }
}
```

### Рекомендация
**Axum** + **tokio** в отдельном треде. Общение через `crossbeam-channel` (уже есть в зависимостях).

---

## Приоритеты реализации

| # | Задача | Сложность | Ценность | Рекомендация |
|---|--------|-----------|----------|--------------|
| 1 | Профилирование | Низкая | Высокая | **Первым** - найти bottleneck |
| 2 | Web API | Средняя | Высокая | **Вторым** - быстрый результат |
| 3 | Python API | Высокая | Высокая | **Третьим** - требует дизайна |
| 4 | Gizmos | Высокая | Средняя | **Последним** - много работы |

---

## Следующие шаги

1. **Профилирование**: Добавить `puffin` и найти где тратится время
2. **Web API**: Начать с простого `/api/player/play|stop|frame`
3. **Python**: Создать `playa-python` crate с базовым API
4. **Gizmos**: Прототип Move gizmo на OpenGL overlay
