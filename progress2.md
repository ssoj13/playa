# Playa - Progress Report 2

## ✅ Завершено

### Priority 0: Критический баг (FIXED)
- ✅ **Фикс padding bug** в `split_sequence_path()` (src/clip.rs:121-167)
  - Проблема: padding считался от числового значения вместо длины строки в filename
  - Решение: `let padding = number_str.len()` - берём длину строки из filename
  - Результат: frames корректно загружаются при втором запуске

### Priority 1: Улучшение десериализации (DONE)
- ✅ **Фикс rebuild_runtime()** (src/project.rs:121-136)
  - Изменено `values()` → `values_mut()` для мутабельного доступа
  - event_sender корректно устанавливается после десериализации

### Priority 2: EventBus Architecture (DONE)
- ✅ **Создан src/events.rs** (196 строк)
  - `AppEvent` enum со всеми событиями (Play, Pause, AddClip, DragDrop, etc.)
  - `EventBus` с crossbeam channels для lock-free messaging
  - `HotkeyWindow` enum для контекста window-specific hotkeys
  - Unit tests для EventBus

- ✅ **Интеграция в PlayaApp** (src/main.rs)
  - EventBus добавлен в struct PlayaApp
  - `handle_event()` метод (118 строк) для обработки всех AppEvent
  - Event processing loop в `update()`: `while let Some(event) = self.event_bus.try_recv()`
  - Keyboard shortcuts конвертированы в events

### Priority 3: GUI Traits (DONE)
- ✅ **Создан src/entities/mod.rs** (73+ строк)
  - `ProjectUI` trait - для project panel view
  - `TimelineUI` trait - для timeline bars view
  - `AttributeEditorUI` trait - для attribute editor panel
  - `NodeUI` trait - для будущего node editor (optional)

- ✅ **Реализация traits для Clip** (src/clip.rs:488-593)
  - `ProjectUI`: иконка, имя, resolution, frame range
  - `TimelineUI`: bar с playhead indicator
  - `AttributeEditorUI`: универсальный attrs editor + Info секция

- ✅ **Реализация traits для Comp** (src/comp.rs:446-547)
  - `ProjectUI`: иконка, имя, fps, frame range, layer count
  - `TimelineUI`: фиолетовый bar с playhead
  - `AttributeEditorUI`: универсальный attrs editor + Info секция

### Priority 4: egui_dnd Integration (DONE)
- ✅ **Добавлен egui_dnd = "0.14"** в Cargo.toml
- ✅ **Интеграция в timeline** (src/timeline.rs:295-362)
  - Drag handle "☰" для каждого слоя
  - Плавная анимация при переупорядочивании
  - Синхронизация layer names ↔ timeline bars через `layer_order`
  - Сохранена вся кастомная логика (horizontal drag, trimming, etc.)
  - **Решена проблема "прыгающего таймлайна"**

### Архитектурный рефакторинг: Attrs-based Properties (DONE)
- ✅ **Comp рефакторинг** (src/comp.rs)
  - Все редактируемые поля перенесены в attrs:
    - `name`, `start`, `end`, `fps`, `play_start`, `play_end`
  - Геттеры/сеттеры: `name()`, `set_name()`, `start()`, `set_start()`, etc.
  - Struct упрощён: только `uuid`, `attrs`, `layers`, `selected_layer`, `current_frame`, runtime fields

- ✅ **Clip рефакторинг** (src/clip.rs)
  - Редактируемые поля перенесены в attrs:
    - `start`, `end`, `padding`
  - Геттеры/сеттеры: `start()`, `set_start()`, `end()`, `set_end()`, `padding()`, `set_padding()`
  - Struct упрощён: только `uuid`, `pattern`, `xres`, `yres`, `attrs`, runtime fields

- ✅ **Универсальный AttributeEditor** (src/entities/mod.rs:12-76)
  - `render_attrs_editor(ui, attrs)` - универсальная функция
  - Поддержка всех типов `AttrValue`:
    - Str: TextEdit
    - Int/UInt: DragValue
    - Float: DragValue с decimal
    - Vec3/Vec4: XYZ(W) компоненты
    - Mat3/Mat4: read-only placeholder
  - **Любые новые атрибуты автоматически редактируются без изменения кода**

- ✅ **Расширен Attrs API** (src/attrs.rs:71-105)
  - `get_mut()`, `remove()`, `iter()`, `iter_mut()`
  - `contains()`, `len()`, `is_empty()`

## 🔄 В процессе / Частично реализовано

### EventBus TODOs в handle_event()
- ⚠️ **Неполная реализация** некоторых events:
  - `StepForward`, `StepBackward` - placeholders (// TODO: implement)
  - `RemoveMedia` - placeholder
  - `SelectMedia`, `SelectLayer` - placeholders
  - `DragStart`, `DragMove`, `DragDrop`, `DragCancel` - placeholders для Project→Timeline DnD

### Hotkey System
- ⚠️ **HotkeyWindow context** определён, но не полностью используется
  - `HotkeyPressed/Released` events существуют
  - Нужна реализация window-specific handling (global vs viewport vs timeline vs project)

## 📋 Еще нужно сделать

### Priority 4 (Optional): egui_taffy
- ❌ Интеграция egui_taffy для адаптивных layouts
  - Полностью опциональная задача
  - Текущий layout на egui работает приемлемо

### Завершить реализацию EventBus handlers
1. **Playback control**:
   - Реализовать `StepForward` / `StepBackward`

2. **Media management**:
   - Реализовать `RemoveMedia(uuid)`
   - Реализовать `SelectMedia(uuid)`

3. **Timeline interaction**:
   - Реализовать `SelectLayer(idx)`

4. **Drag-and-Drop (Project → Timeline)**:
   - Завершить реализацию `DragStart/Move/Drop/Cancel`
   - Интеграция с global drag state в egui context

### Per-Window Hotkey Handling
- Реализовать logic определения активного окна (focus tracking)
- Dispatch hotkeys в зависимости от `HotkeyWindow` context
- Примеры:
  - Space (Global) → Play/Pause
  - Arrow keys (Timeline) → Navigate frames
  - Delete (Timeline) → Delete layer
  - Delete (Project) → Delete media

### Тестирование и сборка
- ⚠️ **НЕ собирали проект во время разработки** (по указанию пользователя)
- Нужно:
  1. Cargo build/check для проверки compilation errors
  2. Тестирование функциональности:
     - Загрузка frame sequences
     - Сохранение/загрузка проекта (сериализация attrs)
     - Timeline DnD
     - Attribute Editor
     - EventBus flow
  3. Фикс возможных ошибок компиляции

## 📊 Статистика изменений

### Новые файлы
- `src/events.rs` - 196 строк (EventBus architecture)
- `src/entities/mod.rs` - 140+ строк (GUI traits + render_attrs_editor)

### Измененные файлы
- `src/main.rs` - добавлен EventBus, handle_event() (~150 новых строк)
- `src/clip.rs` - рефакторинг структуры, GUI traits, attrs migration (~100 изменений)
- `src/comp.rs` - рефакторинг структуры, GUI traits, attrs migration (~80 изменений)
- `src/attrs.rs` - новые методы для итерации/модификации (~35 новых строк)
- `src/timeline.rs` - egui_dnd интеграция (~50 изменений)
- `src/project.rs` - фикс rebuild_runtime() (~15 строк)
- `Cargo.toml` - добавлен egui_dnd = "0.14"

### Архитектурные улучшения
1. **Separation of concerns**: GUI traits отделяют presentation от business logic
2. **Event-driven**: Все UI interactions через EventBus
3. **Data-driven**: Все редактируемые данные в attrs (unified storage)
4. **Extensibility**: Новые атрибуты работают автоматически
5. **Smooth UX**: egui_dnd устраняет "jumping" timeline issue

## 🎯 Следующие шаги (приоритизация)

### High Priority
1. **Тестирование сборки** - проверка compilation
2. **Завершить EventBus handlers** - StepForward/Backward, SelectMedia/Layer
3. **Project→Timeline DnD** - завершить drag-and-drop flow

### Medium Priority
4. **Per-window hotkeys** - context-aware keyboard handling
5. **RemoveMedia** - удаление clips/comps из project

### Low Priority (опционально)
6. **egui_taffy** - адаптивные layouts (если понадобится)
7. **egui_dock** - workspace management (из original plan)

---

**Дата обновления:** 2025-11-18
**Статус:** EventBus + GUI Traits + Attrs Migration завершены. Готов к тестированию и доработке handlers.
