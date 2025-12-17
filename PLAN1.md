# PLAN1: Интеграция puffin профайлера

## Цель
Встроить профайлер как отдельный таб рядом с Timeline/NodeEditor для анализа производительности.

## Статус
Feature flag `profiler` уже объявлен в Cargo.toml! Нужно добавить зависимости и код.

---

## 1. Зависимости (Cargo.toml)

```toml
[features]
profiler = ["dep:puffin", "dep:puffin_egui"]

[dependencies]
puffin = { version = "0.19", optional = true }
puffin_egui = { version = "0.30", optional = true }
```

---

## 2. Изменения в main.rs

### 2.1 DockTab enum (строка ~53)
```rust
enum DockTab {
    Viewport,
    Timeline,
    Project,
    Attributes,
    NodeEditor,
    #[cfg(feature = "profiler")]
    Profiler,
}
```

### 2.2 build_dock_state() (строка ~1025)
```rust
#[cfg(feature = "profiler")]
let tabs = vec![DockTab::Timeline, DockTab::NodeEditor, DockTab::Profiler];
#[cfg(not(feature = "profiler"))]
let tabs = vec![DockTab::Timeline, DockTab::NodeEditor];

let [viewport, _timeline] = dock_state.main_surface_mut().split_below(
    NodeIndex::root(),
    0.65,
    tabs,
);
```

### 2.3 TabViewer impl
```rust
fn title(&mut self, tab: &mut DockTab) -> egui::WidgetText {
    match tab {
        // ...existing...
        #[cfg(feature = "profiler")]
        DockTab::Profiler => "Profiler".into(),
    }
}

fn ui(&mut self, ui: &mut egui::Ui, tab: &mut DockTab) {
    match tab {
        // ...existing...
        #[cfg(feature = "profiler")]
        DockTab::Profiler => self.app.render_profiler_tab(ui),
    }
}
```

### 2.4 render_profiler_tab()
```rust
#[cfg(feature = "profiler")]
fn render_profiler_tab(&mut self, ui: &mut egui::Ui) {
    puffin_egui::profiler_ui(ui);
}
```

### 2.5 Инициализация в main()
```rust
#[cfg(feature = "profiler")]
puffin::set_scopes_on(true);
```

### 2.6 new_frame() в update()
```rust
fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    #[cfg(feature = "profiler")]
    puffin::GlobalProfiler::lock().new_frame();
    // ...rest...
}
```

---

## 3. Инструментация (все с #[cfg(feature = "profiler")])

### 3.1 Макрос-хелпер (src/lib.rs или src/utils/mod.rs)
```rust
#[cfg(feature = "profiler")]
macro_rules! profile_fn {
    () => { puffin::profile_function!(); };
    ($name:expr) => { puffin::profile_scope!($name); };
}

#[cfg(not(feature = "profiler"))]
macro_rules! profile_fn {
    () => {};
    ($name:expr) => {};
}
```

### 3.2 Ключевые места для инструментации

| Файл | Функция | Scope name |
|------|---------|------------|
| main.rs | handle_events() | "events" |
| main.rs | render_viewport_tab() | "viewport" |
| main.rs | render_timeline_tab() | "timeline" |
| core/player.rs | update() | "player_update" |
| core/player.rs | get_current_frame() | "get_frame" |
| core/debounced_preloader.rs | tick() | "preloader_tick" |
| core/debounced_preloader.rs | schedule() | "preloader_schedule" |
| entities/loader.rs | load_frame() | "load_frame" |
| entities/compositor.rs | compose() | "compose" |
| entities/gpu_compositor.rs | compose() | "gpu_compose" |
| widgets/viewport/renderer.rs | upload_texture() | "texture_upload" |
| widgets/viewport/renderer.rs | render() | "gl_render" |
| core/workers.rs | worker loop | "worker_job" |
| dialogs/encode.rs | encode_frame() | "encode_frame" |

---

## 4. Файлы для изменения

| Файл | Изменения |
|------|-----------|
| Cargo.toml | Добавить puffin зависимости (optional) |
| src/lib.rs | profile_fn! макрос |
| src/main.rs | DockTab::Profiler, init, new_frame |
| src/core/player.rs | profile_fn!() |
| src/core/debounced_preloader.rs | profile_fn!() |
| src/core/workers.rs | profile_fn!() |
| src/entities/loader.rs | profile_fn!() |
| src/entities/compositor.rs | profile_fn!() |
| src/widgets/viewport/renderer.rs | profile_fn!() |
| src/dialogs/encode.rs | profile_fn!() |

---

## 5. Сборка и использование

```powershell
# Debug с профайлером
cargo run --features profiler

# Release с профайлером
cargo run --release --features profiler

# Без профайлера (production)
cargo build --release
```

---

## 6. Ожидаемый результат

- Таб "Profiler" появляется только при `--features profiler`
- Flamegraph в реальном времени
- Можно найти bottleneck в: preload, compositor, encoder, workers

---

## Порядок реализации

1. Cargo.toml - добавить optional зависимости
2. src/lib.rs - profile_fn! макрос
3. src/main.rs - DockTab, init, new_frame, render_profiler_tab
4. Build & test базовый profiler
5. Добавить profile_fn!() во все ключевые функции
6. Анализ - запустить тяжёлый сценарий, найти bottleneck
