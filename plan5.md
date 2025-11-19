# План рефакторинга Playa - Фаза 5

## Анализ текущего состояния

### Текущая архитектура

**Структуры данных:**
- `Clip`: image sequence (frames + pattern + attrs)
- `Layer`: ссылка на MediaSource (source_uuid + attrs: start/end/play_start/play_end/opacity)
- `Comp`: контейнер слоёв (layers: Vec<Layer> + cache + attrs)
- `Project`: HashMap<uuid, MediaSource> где MediaSource = Clip | Comp
- `MediaSource`: enum-обёртка над Clip/Comp

**UI компоненты:**
- main.rs: монолитный PlayaApp с множеством флагов
- ui.rs: render функции (project window, help)
- timeline.rs: timeline с egui_dnd
- viewport.rs: OpenGL рендеринг
- ui_encode.rs: диалог энкодера

**Messaging:**
- EventBus для AppEvent (уже есть)
- CompEventSender для CompEvent (уже есть)
- Частичная реализация event-driven архитектуры

### Проблемы и несоответствия требованиям

#### 1. Layer - избыточная абстракция
- **Проблема**: Layer - это просто (uuid, attrs), никакой реальной функциональности
- **Решение**: Удалить Layer, Comp должен быть и контейнером и слоем

#### 2. Comp не может загружать файлы
- **Проблема**: Comp и Clip - разные сущности, дублирование функциональности
- **Решение**: Comp с mode: CompMode { Layer, File }

#### 3. Нет parent-child системы
- **Проблема**: Нет иерархии для nested compositions
- **Решение**: parent: Option<uuid>, children: Vec<uuid>

#### 4. Недостающие трансформации
- **Проблема**: Нет position, rotate, scale, pivot, speed
- **Решение**: Добавить Vec3 атрибуты и transformation matrix

#### 5. Timeline без унифицированного времени
- **Проблема**: Нет единого mapping timeline_space -> frame_space с учётом speed/retiming
- **Решение**: Унифицированная функция time mapping для всех comps

#### 6. Нет настоящего dock layout
- **Проблема**: SidePanel + CentralPanel, нет гибкости
- **Решение**: egui_dock для полноценных доков

#### 7. Много TODO в коде
- 15+ не реализованных функций в main.rs
- Нужна полная реализация через EventBus

---

## Детальный пошаговый план

### ФАЗА 1: Подготовка - Dependency Management

**Цель**: Обновить зависимости и подготовить окружение

#### Шаг 1.1: Обновить Cargo.toml
```toml
[dependencies]
egui_dock = "0.14"  # добавить если нет
# egui_taffy не нужен - egui уже имеет встроенный layout
# egui_dnd = "0.14" # уже есть
```

**ВАЖНО**: egui_taffy не нужен - egui имеет встроенный flex/grid layout через ui.horizontal(), ui.vertical(), egui::Layout

#### Шаг 1.2: Создать структуру каталогов
```
src/
├── app.rs              # Главное приложение PlayaApp
├── widgets/
│   ├── mod.rs
│   ├── project.rs      # Project window widget
│   ├── timeline.rs     # Timeline widget (перенести из timeline.rs)
│   ├── viewport.rs     # Viewport widget (перенести из viewport.rs)
│   └── ae.rs           # Attribute Editor widget (новый)
├── dialogs/
│   ├── mod.rs
│   ├── prefs.rs        # Preferences dialog (из prefs.rs)
│   ├── encoder.rs      # Encoder dialog (из ui_encode.rs)
│   └── hotkeys.rs      # Hotkeys dialog (новый)
├── entities/
│   ├── mod.rs          # Переработать
│   ├── comp.rs         # Переработанный Comp (объединить с Clip)
│   ├── project.rs      # Без изменений
│   └── layer.rs        # УДАЛИТЬ
└── main.rs             # Только bootstrap
```

---

### ФАЗА 2: Переработка Entity системы

**Цель**: Объединить Comp и Clip, удалить Layer, добавить parent-child

#### Шаг 2.1: Определить новый CompMode

**src/entities/comp.rs**:
```rust
/// Режим работы Comp: слой или файловый источник
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum CompMode {
    /// Layer mode: композирует children comps
    Layer,
    /// File mode: загружает image sequence с диска
    File,
}
```

#### Шаг 2.2: Расширить Comp структуру

Добавить в Comp:
```rust
pub struct Comp {
    // ... существующие поля ...

    /// Режим работы: Layer или File
    pub mode: CompMode,

    /// Для режима File: маска файлов (e.g. "/path/seq.*.exr")
    pub file_mask: Option<String>,

    /// Для режима File: первый кадр последовательности
    pub file_start: Option<usize>,

    /// Для режима File: последний кадр последовательности
    pub file_end: Option<usize>,

    /// Parent composition UUID (if nested)
    pub parent: Option<String>,

    /// Children compositions UUIDs
    pub children: Vec<String>,

    /// Transform attributes (новые Vec3 attrs):
    /// - "position" (Vec3): x, y, z position
    /// - "rotation" (Vec3): euler angles
    /// - "scale" (Vec3): scale factors
    /// - "pivot" (Vec3): pivot point
    /// - "transparency" (Float): alpha
    /// - "layer_mode" (Str): "normal", "screen", "add", "subtract", "multiply", "divide"
    /// - "speed" (Float): playback speed multiplier

    /// Хэш композиции (для cache invalidation)
    /// В режиме Layer: аккумулированный хэш всех children
    /// В режиме File: хэш file_mask + file_start/end
    #[serde(skip)]
    comp_hash: u64,
}
```

#### Шаг 2.3: Реализовать загрузку файлов в Comp

Перенести логику из `Clip` в `Comp::load_from_files()`:
```rust
impl Comp {
    /// Загрузить image sequence в режиме File
    pub fn load_from_files(&mut self, pattern: &str) -> Result<()> {
        // Логика из Clip::init_from_glob() / init_from_file()
        // Заполнить file_mask, file_start, file_end
        // Создать Frame::new_unloaded() для каждого кадра
    }

    /// Получить кадр в режиме File
    fn get_frame_file_mode(&self, frame_idx: usize) -> Option<Frame> {
        // Логика загрузки из файла
    }

    /// Получить кадр в режиме Layer
    fn get_frame_layer_mode(&self, frame_idx: usize, project: &Project) -> Option<Frame> {
        // Рекурсивная композиция children
    }
}
```

#### Шаг 2.4: Удалить Layer

1. Удалить `src/entities/layer.rs`
2. Изменить `Comp::layers: Vec<Layer>` на `Comp::children: Vec<String>` (UUIDs)
3. Обновить все функции работы с layers

#### Шаг 2.5: Реализовать parent-child management

```rust
impl Comp {
    /// Добавить child comp
    pub fn add_child(&mut self, child_uuid: String) {
        if !self.children.contains(&child_uuid) {
            self.children.push(child_uuid);
            self.invalidate_cache();
        }
    }

    /// Удалить child comp
    pub fn remove_child(&mut self, child_uuid: &str) {
        self.children.retain(|uuid| uuid != child_uuid);
        self.invalidate_cache();
    }

    /// Установить parent
    pub fn set_parent(&mut self, parent_uuid: Option<String>) {
        self.parent = parent_uuid;
    }
}
```

#### Шаг 2.6: Переработать compute_hash

```rust
impl Comp {
    fn compute_comp_hash(&self, project: &Project) -> u64 {
        let mut hasher = DefaultHasher::new();

        match self.mode {
            CompMode::File => {
                // Хэш file_mask, file_start, file_end
                self.file_mask.hash(&mut hasher);
                self.file_start.hash(&mut hasher);
                self.file_end.hash(&mut hasher);
            }
            CompMode::Layer => {
                // Рекурсивно хэшировать всё дерево children
                self.children.len().hash(&mut hasher);
                for child_uuid in &self.children {
                    if let Some(child) = project.get_comp(child_uuid) {
                        let child_hash = child.compute_comp_hash(project);
                        child_hash.hash(&mut hasher);
                    }
                }
            }
        }

        // Хэш transform attrs
        // position, rotation, scale, pivot, transparency, layer_mode, speed

        hasher.finish()
    }
}
```

---

### ФАЗА 3: Timeline с унифицированным временем

**Цель**: Единая функция mapping времени для всех вложенных comps

#### Шаг 3.1: Реализовать time mapping

```rust
impl Comp {
    /// Маппинг из глобального frame в локальный frame с учётом:
    /// - play_start/play_end родительского comp
    /// - speed текущего comp
    /// - рекурсивный вызов для вложенных comps
    pub fn map_global_to_local(
        &self,
        global_frame: usize,
        project: &Project
    ) -> Option<usize> {
        // 1. Проверить что global_frame в пределах play_range
        let (play_start, play_end) = self.play_range();
        if global_frame < play_start || global_frame > play_end {
            return None;
        }

        // 2. Вычислить offset от play_start
        let offset = global_frame - play_start;

        // 3. Применить speed multiplier
        let speed = self.attrs.get_float("speed").unwrap_or(1.0);
        let local_frame = ((offset as f32) * speed) as usize;

        // 4. Добавить play_start offset
        let play_start_offset = self.attrs.get_i32("play_start").unwrap_or(0);
        let final_frame = (local_frame as i32 + play_start_offset).max(0) as usize;

        Some(final_frame)
    }
}
```

#### Шаг 3.2: Timeline widget с zoom/pan

**src/widgets/timeline.rs**:
```rust
pub struct TimelineWidget {
    /// Zoom level (pixels per frame)
    zoom: f32,

    /// Horizontal pan offset (in frames)
    pan_offset: f32,

    /// Selected comp UUID
    selected_comp: Option<String>,
}

impl TimelineWidget {
    /// Mapping: screen_x -> frame_number
    fn screen_to_frame(&self, screen_x: f32, timeline_rect: Rect) -> usize {
        let offset_x = screen_x - timeline_rect.min.x;
        let frame = (offset_x / self.zoom) + self.pan_offset;
        frame.max(0.0) as usize
    }

    /// Mapping: frame_number -> screen_x
    fn frame_to_screen(&self, frame: usize, timeline_rect: Rect) -> f32 {
        let frame_offset = (frame as f32) - self.pan_offset;
        timeline_rect.min.x + (frame_offset * self.zoom)
    }

    pub fn ui(&mut self, ui: &mut Ui, project: &mut Project) {
        // Render time ruler
        // Render nested comps (recursive)
        // Handle drag-and-drop from project window
        // Handle zoom (mouse wheel)
        // Handle pan (middle mouse drag)
    }
}
```

---

### ФАЗА 4: Интеграция egui_dock

**Цель**: Гибкий dock layout для всех окон

#### Шаг 4.1: Создать DockState

**src/app.rs**:
```rust
use egui_dock::{DockArea, DockState, NodeIndex, Style, TabViewer};

pub struct PlayaApp {
    // ... другие поля ...

    /// Dock layout state
    dock_state: DockState<PanelType>,
}

/// Типы панелей в dock layout
#[derive(Debug, Clone, PartialEq)]
pub enum PanelType {
    Viewport,
    Timeline,
    Project,
    AttributeEditor,
    // Будущие панели: NodeEditor, Console, etc.
}

impl TabViewer for PlayaApp {
    type Tab = PanelType;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            PanelType::Viewport => "Viewport".into(),
            PanelType::Timeline => "Timeline".into(),
            PanelType::Project => "Project".into(),
            PanelType::AttributeEditor => "Attributes".into(),
        }
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            PanelType::Viewport => self.render_viewport(ui),
            PanelType::Timeline => self.render_timeline(ui),
            PanelType::Project => self.render_project(ui),
            PanelType::AttributeEditor => self.render_ae(ui),
        }
    }
}
```

#### Шаг 4.2: Настроить default layout

```rust
impl PlayaApp {
    fn create_default_dock_layout() -> DockState<PanelType> {
        let mut dock_state = DockState::new(vec![PanelType::Viewport]);

        // Split viewport vertically, add timeline below
        let [_viewport, timeline] = dock_state.main_surface_mut()
            .split_below(NodeIndex::root(), 0.7, vec![PanelType::Timeline]);

        // Split viewport horizontally, add project on right
        let [_viewport, _project] = dock_state.main_surface_mut()
            .split_right(NodeIndex::root(), 0.75, vec![PanelType::Project]);

        // Add attribute editor as tab with project
        dock_state.main_surface_mut()
            .push_to_focused_leaf(PanelType::AttributeEditor);

        dock_state
    }
}
```

---

### ФАЗА 5: Event-driven architecture

**Цель**: Все операции через EventBus, реализовать все TODO

#### Шаг 5.1: Расширить AppEvent

**src/events.rs**:
```rust
pub enum AppEvent {
    // ... существующие события ...

    // Новые события для всех TODO операций
    StepForward,
    StepBackward,
    StepForwardLarge,   // +25 frames
    StepBackwardLarge,  // -25 frames

    JumpToFrame(usize),
    JumpToStart,
    JumpToEnd,

    // Layer operations
    AddLayer { comp_uuid: String, source_uuid: String, start_frame: usize },
    RemoveLayer { comp_uuid: String, layer_idx: usize },
    MoveLayer { comp_uuid: String, layer_idx: usize, new_start: usize },

    // Hotkeys with window context
    Hotkey { key: String, window: HotkeyWindow, pressed: bool },
}
```

#### Шаг 5.2: Создать HotkeyHandler

**src/dialogs/hotkeys.rs**:
```rust
pub struct HotkeyHandler {
    /// Bindings per window: (window, key) -> AppEvent
    bindings: HashMap<(HotkeyWindow, String), AppEvent>,

    /// Currently focused window
    focused_window: HotkeyWindow,
}

impl HotkeyHandler {
    pub fn new() -> Self {
        let mut bindings = HashMap::new();

        // Default bindings
        bindings.insert((HotkeyWindow::Global, "Space".into()), AppEvent::TogglePlayPause);
        bindings.insert((HotkeyWindow::Global, "K".into()), AppEvent::Stop);
        // ... все остальные hotkeys

        bindings.insert((HotkeyWindow::Timeline, "Delete".into()), AppEvent::RemoveSelectedLayer);
        // ... timeline-specific hotkeys

        Self {
            bindings,
            focused_window: HotkeyWindow::Global,
        }
    }

    pub fn handle_key(&self, key: &str, window: HotkeyWindow) -> Option<AppEvent> {
        self.bindings.get(&(window.clone(), key.to_string())).cloned()
            .or_else(|| self.bindings.get(&(HotkeyWindow::Global, key.to_string())).cloned())
    }
}
```

#### Шаг 5.3: Реализовать все TODO операции

В `PlayaApp::handle_events()`:
```rust
impl PlayaApp {
    fn handle_events(&mut self) {
        while let Some(event) = self.event_bus.try_recv() {
            match event {
                AppEvent::StepForward => {
                    if let Some(comp) = self.get_active_comp_mut() {
                        let new_frame = (comp.current_frame + 1).min(comp.end());
                        comp.set_current_frame(new_frame);
                    }
                }
                AppEvent::StepBackward => {
                    if let Some(comp) = self.get_active_comp_mut() {
                        let new_frame = comp.current_frame.saturating_sub(1).max(comp.start());
                        comp.set_current_frame(new_frame);
                    }
                }
                // ... реализовать все остальные события
            }
        }
    }
}
```

---

### ФАЗА 6: UI Widgets как модули

**Цель**: Каждый виджет - независимый модуль с trait-based интерфейсом

#### Шаг 6.1: Project Widget

**src/widgets/project.rs**:
```rust
pub struct ProjectWidget {
    // State
}

impl ProjectWidget {
    pub fn ui(&mut self, ui: &mut Ui, project: &mut Project, event_bus: &EventBus) {
        ui.heading("Project");

        // Buttons
        ui.horizontal(|ui| {
            if ui.button("Add Clip").clicked() {
                // Send event to EventBus
            }
            if ui.button("Add Comp").clicked() {
                event_bus.send(AppEvent::AddComp {
                    name: "New Comp".into(),
                    fps: 24.0
                });
            }
        });

        // List all clips and comps using ProjectUI trait
        for (uuid, source) in &project.media {
            let response = source.project_ui(ui);

            // Handle drag-and-drop
            if response.drag_started() {
                // ...
            }
        }
    }
}
```

#### Шаг 6.2: Attribute Editor Widget

**src/widgets/ae.rs**:
```rust
pub struct AttributeEditorWidget {
    selected_entity: Option<String>,
}

impl AttributeEditorWidget {
    pub fn ui(&mut self, ui: &mut Ui, project: &mut Project) {
        ui.heading("Attributes");

        if let Some(uuid) = &self.selected_entity {
            if let Some(source) = project.media.get_mut(uuid) {
                // Use AttributeEditorUI trait
                source.ae_ui(ui);
            }
        } else {
            ui.label("No selection");
        }
    }
}
```

---

### ФАЗА 7: Сериализация с новыми структурами

**Цель**: Backward compatibility не нужна, новый формат JSON

#### Шаг 7.1: Обновить Project::to_json / from_json

Всё уже сериализуется через serde, просто убедиться что:
- CompMode сериализуется корректно
- parent/children сохраняются
- file_mask, file_start, file_end сохраняются для File mode

#### Шаг 7.2: Migration helper (опционально)

Если нужна миграция старых проектов:
```rust
pub fn migrate_old_project(old_json: &str) -> Result<Project> {
    // Парсинг старого формата
    // Конвертация в новый формат
    // Clip -> Comp в режиме File
    // Layer -> children references
}
```

---

## Порядок реализации (оптимальный)

### Этап 1: Foundation (1-2 дня)
1. ✅ Фаза 1 - Dependency management и структура каталогов
2. ✅ Фаза 5, Шаг 5.1 - Расширить AppEvent
3. ✅ Фаза 5, Шаг 5.2 - HotkeyHandler

### Этап 2: Entity System (2-3 дня)
4. ✅ Фаза 2, Шаг 2.1 - CompMode enum
5. ✅ Фаза 2, Шаг 2.2 - Расширить Comp
6. ✅ Фаза 2, Шаг 2.3 - Загрузка файлов в Comp
7. ✅ Фаза 2, Шаг 2.4 - Удалить Layer
8. ✅ Фаза 2, Шаг 2.5 - Parent-child management
9. ✅ Фаза 2, Шаг 2.6 - Переработать compute_hash

### Этап 3: Timeline (1-2 дня)
10. ✅ Фаза 3, Шаг 3.1 - Time mapping функция
11. ✅ Фаза 3, Шаг 3.2 - Timeline widget с zoom/pan

### Этап 4: UI Architecture (2-3 дня)
12. ✅ Фаза 6 - UI widgets как модули
13. ✅ Фаза 4 - egui_dock integration
14. ✅ Фаза 5, Шаг 5.3 - Реализовать все TODO операции

### Этап 5: Testing & Polish (1 день)
15. ✅ Тестирование всех операций
16. ✅ Фаза 7 - Сериализация
17. ✅ Исправление багов

---

## Слабые места и риски

### 1. Memory Management
**Риск**: Рекурсивная композиция может привести к копированию Frame
**Решение**:
- Frame уже использует Arc<Vec<u8>> внутри - копирование дешёвое
- Cache на каждом уровне иерархии предотвращает пересчёт
- Проверить с profiler что нет избыточного клонирования

### 2. Cache Invalidation
**Риск**: Рекурсивный compute_hash может быть медленным
**Решение**:
- Кэшировать хэш на каждом уровне
- Инвалидировать только когда attrs изменились
- Использовать dirty flags вместо полного пересчёта

### 3. Timeline Performance
**Риск**: Отрисовка тысяч nested comps может быть медленной
**Решение**:
- Culling: рисовать только видимые в viewport
- LOD: упрощённая отрисовка для далёких/мелких элементов
- Virtualization: использовать egui ScrollArea с виртуализацией

### 4. Backward Compatibility
**Риск**: Старые проекты не загрузятся
**Решение**:
- Как указано в task5: "No compatibility needed, this is WiP"
- Можно добавить migration helper позже если нужно

### 5. egui_taffy Integration
**Риск**: В документации упоминается egui_taffy, но его нет в deps
**Решение**:
- egui_taffy НЕ НУЖЕН - egui имеет встроенный layout
- Использовать ui.horizontal(), ui.vertical(), egui::Layout
- egui_dock уже предоставляет flex layout для доков

---

## Контрольные точки (Checkpoints)

После каждого этапа:
1. ✅ Код компилируется без ошибок
2. ✅ Базовая функциональность работает (load/play/save)
3. ✅ Нет регрессий в производительности
4. ✅ Тесты проходят (если есть)

**Тестовый сценарий**:
1. Создать новый проект
2. Добавить image sequence (Comp в режиме File)
3. Создать composition (Comp в режиме Layer)
4. Добавить image sequence как child в composition
5. Воспроизвести composition
6. Сохранить/загрузить проект
7. Проверить что кэш работает (повторное воспроизведение быстрее)

---

## Выводы и рекомендации

### Что ТОЧНО делать:
✅ Удалить Layer - он действительно не нужен
✅ Объединить Comp и Clip через CompMode
✅ Реализовать parent-child систему
✅ Добавить transform атрибуты (Vec3)
✅ Унифицировать time mapping
✅ Использовать egui_dock для layout
✅ Расширить EventBus для всех операций

### Что НЕ делать:
❌ НЕ добавлять egui_taffy - не нужен, egui уже имеет layout
❌ НЕ пытаться сохранить backward compatibility (указано в task5)
❌ НЕ оптимизировать преждевременно - сначала работающий код

### Порядок приоритетов:
1. **High**: Фаза 2 (Entity system) - фундамент всего
2. **High**: Фаза 3 (Time mapping) - критично для воспроизведения
3. **Medium**: Фаза 4 (egui_dock) - улучшает UX
4. **Medium**: Фаза 5 (EventBus) - завершает архитектуру
5. **Low**: Фаза 6 (UI modules) - можно делать постепенно

### Примерная оценка времени:
- **Минимум** (только критичное): 5-7 дней
- **Оптимально** (все фазы): 10-14 дней
- **С тестированием и polish**: 15-20 дней

---

## Итоговая схема архитектуры

```
PlayaApp (egui_dock)
├── DockState<PanelType>
│   ├── Viewport Widget
│   ├── Timeline Widget (zoom/pan, time mapping)
│   ├── Project Widget (drag-and-drop source)
│   └── Attribute Editor Widget
├── EventBus (crossbeam channel)
│   ├── HotkeyHandler (per-window bindings)
│   └── Event handlers
└── Project
    └── HashMap<UUID, Comp>
        ├── CompMode::File (ex-Clip)
        │   ├── file_mask
        │   ├── file_start/end
        │   └── Frame loading
        └── CompMode::Layer
            ├── children: Vec<UUID>
            ├── parent: Option<UUID>
            ├── Transforms (position, rotate, scale...)
            └── Recursive composition
```

Всё чётко, логично, и без избыточных абстракций. Вперёд! 🚀
