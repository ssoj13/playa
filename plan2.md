# План исправления багов и рефакторинга архитектуры Playa

## ПРИОРИТЕТ 0: Критический баг с padding (СРОЧНО)

### Проблема
При десериализации `Clip` генерируются неправильные пути к кадрам из-за некорректного `padding`.

**Корень проблемы:**
- `src/clip.rs:383-386` и `436` вычисляют padding как `number.to_string().len()`
- Для кадра `kz.0000.tif` number=0 → `"0".len()` = 1, но реально padding=4
- При восстановлении генерируются пути `kz.0.tif` вместо `kz.0000.tif`
- Ошибка: "The system cannot find the file specified"

**Файл:** `src/clip.rs`
**Строки:** 383-386, 436

### Решение

1. **Парсить padding из имени файла, а не из числа**

   ```rust
   // БЫЛО (НЕПРАВИЛЬНО):
   self.padding = number.to_string().len();

   // ДОЛЖНО БЫТЬ:
   // Извлечь padding из реального имени файла
   // Например для "kz.0000.tif" найти "0000" и взять len() = 4
   ```

2. **Реализация:**
   - В `split_sequence_path()` парсить число как строку и вычислять padding
   - Использовать regex для извлечения последовательности цифр из имени файла
   - Сохранять длину этой последовательности как padding

3. **Места изменений:**
   - `src/clip.rs:383-386` - `init_from_glob()`
   - `src/clip.rs:436` - `init_from_file()`
   - `src/clip.rs:121-164` - `split_sequence_path()` - добавить вывод реального padding

### Шаги выполнения:

#### Шаг 1: Исправить split_sequence_path()
```rust
// В split_sequence_path() вернуть tuple: (prefix, ext, number, padding)
// где padding = длина цифровой последовательности в имени файла

fn split_sequence_path(path: &Path) -> Result<(String, String, usize, usize), FrameError> {
    // ... существующий код парсинга ...

    // Найти последовательность цифр в stem
    let re = Regex::new(r"(\d+)$")?;
    let padding = if let Some(caps) = re.captures(stem_str) {
        caps.get(1).map(|m| m.as_str().len()).unwrap_or(4)
    } else {
        4 // default
    };

    Ok((prefix, ext.to_string(), number, padding))
}
```

#### Шаг 2: Обновить init_from_glob()
```rust
fn init_from_glob(&mut self, pattern: &str) -> Result<(), FrameError> {
    let paths = glob_paths(pattern)?;

    // Парсим ПЕРВЫЙ файл для определения padding
    let first_path = paths.first().ok_or(...)?;
    let (prefix, ext, first_num, padding) = split_sequence_path(first_path)?;

    self.padding = padding;  // ✅ Правильный padding из файла

    // ... остальной код ...
}
```

#### Шаг 3: Обновить init_from_file()
```rust
fn init_from_file(&mut self, file_path: &str) -> Result<(), FrameError> {
    let path = Path::new(file_path);
    let (prefix, ext, number, padding) = split_sequence_path(path)?;

    self.padding = padding;  // ✅ Правильный padding

    // ... остальной код ...
}
```

#### Шаг 4: Тестирование
1. Удалить кеш: `C:\Users\joss1\AppData\Roaming\playa\playa.json`
2. Запустить приложение, загрузить клип
3. Проверить что padding правильный в логах
4. Закрыть, открыть снова - проверить что кадры загружаются

---

## ПРИОРИТЕТ 1: Улучшение десериализации

### Цель
Унифицировать инициализацию при создании и при десериализации.

### Проблемы:
1. `Clip::new()` делает одно, `Deserialize::deserialize()` делает другое
2. UUID зависит от pattern (но это не критично если padding починить)
3. Event sender для Comp не устанавливается в `rebuild_runtime()`

### Решение:

#### 1.1 Общая функция инициализации для Clip

```rust
impl Clip {
    /// Общая инициализация из метаданных (используется и в new(), и в deserialize)
    fn init_common(&mut self) -> Result<(), FrameError> {
        // Устанавливаем метаданные в attrs
        self.attrs.set("pattern", AttrValue::Str(self.pattern.clone()));
        self.attrs.set("xres", AttrValue::UInt(self.xres as u32));
        self.attrs.set("yres", AttrValue::UInt(self.yres as u32));
        self.attrs.set("start", AttrValue::UInt(self.start as u32));
        self.attrs.set("end", AttrValue::UInt(self.end as u32));

        Ok(())
    }
}
```

#### 1.2 UUID через uuid::Uuid вместо хеша pattern

```rust
// БЫЛО:
fn gen_clip_uuid(pattern: &str, start: usize, end: usize) -> String {
    format!("clip:{}:{}:{}", pattern, start, end)  // ❌ Зависит от pattern
}

// ДОЛЖНО БЫТЬ:
// В Clip::new():
uuid: uuid::Uuid::new_v4().to_string(),  // ✅ Стабильный UUID

// При десериализации UUID просто восстанавливается из JSON
```

**НО:** Это сломает существующие проекты! Layer.source_uuid не найдёт клипы!

**Решение:**
- Добавить migration в `Project::from_json()`
- Создать map старых UUID → новых UUID
- Обновить все source_uuid в Layer'ах

#### 1.3 Исправить rebuild_runtime()

```rust
// src/project.rs:121-135
pub fn rebuild_runtime(&mut self, event_sender: Option<CompEventSender>) {
    self.compositor = CompositorType::default();

    // Установить event_sender для всех Comp
    if let Some(sender) = event_sender {
        for source in self.media.values_mut() {  // ✅ values_mut() вместо values()
            if let Some(comp) = source.as_comp_mut() {
                comp.clear_cache();
                comp.set_event_sender(sender.clone());  // ✅ Работает
            }
        }
    }
}
```

---

## ПРИОРИТЕТ 2: Архитектура - Event Bus

### Цель
Разделить логику на независимые модули с коммуникацией через события.

### Структура:

```
src/
├── app.rs          // Главное приложение
├── events.rs       // AppEvent enum, EventBus
├── entities/       // Бизнес-логика БЕЗ GUI
│   ├── mod.rs
│   ├── project.rs
│   ├── clip.rs
│   ├── comp.rs
│   └── node.rs     // Future: базовый Node trait
├── widgets/        // GUI компоненты
│   ├── mod.rs
│   ├── viewport.rs
│   ├── timeline.rs
│   ├── project_panel.rs
│   └── attr_editor.rs
├── dialogs/
│   ├── prefs.rs
│   └── encoder.rs
└── main.rs
```

### Шаги:

#### 2.1 Создать events.rs с AppEvent

```rust
// src/events.rs
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Playback
    Play,
    Pause,
    Stop,
    SetFrame(usize),
    StepForward,
    StepBackward,

    // Project
    AddClip(PathBuf),
    AddComp { name: String, fps: f32 },
    RemoveMedia(String),  // uuid

    // Timeline
    DragStart { media_uuid: String },
    DragMove { mouse_pos: (f32, f32) },
    DragDrop { target_comp: String, frame: usize },

    // Selection
    SelectMedia(String),  // uuid
    DeselectAll,

    // UI
    TogglePlaylist,
    ToggleHelp,
    ZoomViewport(f32),
}

pub struct EventBus {
    tx: crossbeam::channel::Sender<AppEvent>,
    rx: crossbeam::channel::Receiver<AppEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam::channel::unbounded();
        Self { tx, rx }
    }

    pub fn send(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    pub fn try_recv(&self) -> Option<AppEvent> {
        self.rx.try_recv().ok()
    }

    pub fn sender(&self) -> crossbeam::channel::Sender<AppEvent> {
        self.tx.clone()
    }
}
```

#### 2.2 Добавить crossbeam-channel в Cargo.toml

```toml
[dependencies]
crossbeam = "0.8"
```

#### 2.3 Интегрировать EventBus в PlayaApp

```rust
// src/main.rs
struct PlayaApp {
    event_bus: Arc<EventBus>,
    player: Player,
    // ... остальные поля
}

impl PlayaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Обработать все события из очереди
        while let Some(event) = self.event_bus.try_recv() {
            self.handle_event(event);
        }

        // ... рендер UI
    }

    fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Play => self.player.play(),
            AppEvent::Pause => self.player.pause(),
            AppEvent::SetFrame(f) => {
                if let Some(comp_uuid) = &self.player.active_comp {
                    if let Some(comp) = self.player.project.get_comp_mut(comp_uuid) {
                        comp.set_frame(f);
                    }
                }
            },
            AppEvent::AddClip(path) => {
                // ...
            },
            // ... остальные события
        }
    }
}
```

#### 2.4 UI компоненты посылают события

```rust
// Пример: кнопка Play
if ui.button("▶ Play").clicked() {
    self.event_bus.send(AppEvent::Play);
}

// Пример: таймслайдер
if ui.add(egui::Slider::new(&mut frame, 0..=max)).changed() {
    self.event_bus.send(AppEvent::SetFrame(frame));
}

// Пример: drag-and-drop
if ui.input(|i| i.pointer.any_pressed()) {
    self.event_bus.send(AppEvent::DragStart { media_uuid: clip.uuid.clone() });
}
```

---

## ПРИОРИТЕТ 3: GUI Traits для Entities

### Цель
Отделить бизнес-логику от GUI, используя трейты.

### Трейты:

```rust
// src/entities/mod.rs
use egui::{Ui, Response};

/// Виджет для окна проекта (имя + метаданные)
pub trait ProjectUI {
    fn project_ui(&self, ui: &mut Ui) -> Response;
}

/// Виджет для таймлайна (бар клипа/компа)
pub trait TimelineUI {
    fn timeline_ui(&self, ui: &mut Ui, bar_rect: egui::Rect, current_frame: usize) -> Response;
}

/// Виджет для Attribute Editor (все атрибуты)
pub trait AttributeEditorUI {
    fn ae_ui(&mut self, ui: &mut Ui);
}
```

### Реализация для Clip:

```rust
// src/entities/clip.rs
impl ProjectUI for Clip {
    fn project_ui(&self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label(&self.uuid);
            ui.label(format!("{}x{}", self.xres, self.yres));
            ui.label(format!("{}-{}", self.start, self.end));
        }).response
    }
}

impl TimelineUI for Clip {
    fn timeline_ui(&self, ui: &mut Ui, bar_rect: egui::Rect, current_frame: usize) -> Response {
        let painter = ui.painter();
        painter.rect_filled(bar_rect, 0.0, egui::Color32::BLUE);

        // Подсветить текущий кадр
        if current_frame >= self.start && current_frame <= self.end {
            // ...
        }

        ui.interact(bar_rect, ui.id().with(&self.uuid), egui::Sense::click_and_drag())
    }
}

impl AttributeEditorUI for Clip {
    fn ae_ui(&mut self, ui: &mut Ui) {
        ui.heading("Clip Attributes");
        ui.label(format!("Pattern: {}", self.pattern));
        ui.add(egui::Slider::new(&mut self.start, 0..=10000).text("Start"));
        ui.add(egui::Slider::new(&mut self.end, 0..=10000).text("End"));
        // ... остальные атрибуты
    }
}
```

---

## ПРИОРИТЕТ 4: egui библиотеки

### 4.1 egui_dnd - Drag and Drop

```toml
[dependencies]
egui_dnd = "0.9"
```

**Использование:**
```rust
use egui_dnd::dnd;

// В timeline.rs
dnd(ui, "timeline_dnd").show_vec(&mut self.layers, |ui, layer, handle, state| {
    handle.ui(ui, |ui| {
        ui.label("≡");  // drag handle
    });

    // Рендер layer бара
    layer.timeline_ui(ui, ...);
});
```

**Преимущества:**
- Решает проблему прыгающего таймлайна
- Встроенный снэппинг
- Визуализация drag процесса

### 4.2 egui_taffy - Layouts

```toml
[dependencies]
egui_taffy = "0.1"
```

**Использование:**
```rust
use egui_taffy::{Flex, item, FlexAlignContent};

// Адаптивный layout для главного окна
Flex::horizontal().show(ui, |flex| {
    // Левая панель - проект
    flex.add(item().grow(0.3), |ui| {
        project_panel(ui);
    });

    // Центр - viewport + timeline
    flex.add_flex(item().grow(0.5), Flex::vertical(), |flex| {
        flex.add(item().grow(1.0), |ui| {
            viewport(ui);
        });
        flex.add(item().height(150.0), |ui| {
            timeline(ui);
        });
    });

    // Правая панель - attribute editor
    flex.add(item().grow(0.2), |ui| {
        attr_editor(ui);
    });
});
```

### 4.3 egui_dock (опционально)

Проверить docs.rs/egui_dock - если нужны сложные workspace splits с вкладками.

Альтернатива: использовать `egui::TopBottomPanel` + `egui::SidePanel` для простых случаев.

---

## ПОРЯДОК ВЫПОЛНЕНИЯ

### Неделя 1: Критические баги
1. ✅ День 1-2: Исправить padding bug (ПРИОРИТЕТ 0)
2. ✅ День 3: Тестировать сохранение/загрузку проектов
3. ✅ День 4: Исправить rebuild_runtime() event_sender
4. ✅ День 5: Code review и тесты

### Неделя 2: EventBus
1. ✅ День 1: Создать events.rs с AppEvent enum
2. ✅ День 2: Интегрировать crossbeam-channel
3. ✅ День 3: Переделать UI на emit events
4. ✅ День 4-5: Тестировать все UI взаимодействия

### Неделя 3: GUI Traits
1. ✅ День 1: Создать трейты ProjectUI, TimelineUI, AttributeEditorUI
2. ✅ День 2-3: Реализовать для Clip и Comp
3. ✅ День 4: Вынести widgets в отдельную папку
4. ✅ День 5: Рефакторинг main.rs

### Неделя 4: egui библиотеки
1. ✅ День 1-2: Интегрировать egui_dnd для timeline
2. ✅ День 3-4: Использовать egui_taffy для layouts
3. ✅ День 5: Полировка UI, фиксы багов

---

## ВАЖНЫЕ ЗАМЕЧАНИЯ

### Не делать сразу:
- ❌ Node editor (noded.rs) - пока не нужен
- ❌ Полная миграция на новую структуру папок - делать постепенно
- ❌ Менять UUID scheme - это сломает существующие проекты (нужна миграция)

### Делать постепенно:
- ✅ Один виджет за раз (начать с timeline)
- ✅ Тестировать после каждого изменения
- ✅ Не менять всё сразу

### Тестирование:
- Запускать после каждого изменения
- Проверять сохранение/загрузку проектов
- Проверять что старые проекты загружаются
- Проверять drag-and-drop
- Проверять keyboard shortcuts

---

## MIGRATION PLAN (для UUID изменений)

Если решим менять UUID на uuid::Uuid::new_v4():

```rust
// src/project.rs
impl Project {
    pub fn migrate_from_v1(&mut self) {
        // Создать map старых UUID → новых UUID
        let mut uuid_map: HashMap<String, String> = HashMap::new();

        // Переименовать все Clip UUID
        let mut new_media = HashMap::new();
        for (old_uuid, source) in self.media.drain() {
            let new_uuid = if old_uuid.starts_with("clip:") {
                uuid::Uuid::new_v4().to_string()
            } else {
                old_uuid.clone()  // Comp UUID не меняем
            };
            uuid_map.insert(old_uuid.clone(), new_uuid.clone());
            new_media.insert(new_uuid, source);
        }
        self.media = new_media;

        // Обновить все source_uuid в Layer
        for source in self.media.values_mut() {
            if let Some(comp) = source.as_comp_mut() {
                for layer in comp.layers.iter_mut() {
                    if let Some(new_uuid) = uuid_map.get(&layer.source_uuid) {
                        layer.source_uuid = new_uuid.clone();
                    }
                }
            }
        }

        // Обновить clips_order
        self.clips_order = self.clips_order.iter()
            .map(|old| uuid_map.get(old).cloned().unwrap_or_else(|| old.clone()))
            .collect();
    }
}
```

---

## ИТОГО

**Критично сейчас:**
- Фикс padding bug - это разблокирует всё остальное

**Важно скоро:**
- EventBus архитектура - упростит разработку дальше
- egui_dnd - решит проблему прыгающего таймлайна

**Можно потом:**
- GUI traits - для чистоты архитектуры
- Модульная структура папок - для масштабируемости
- egui_taffy - для красивых layouts
