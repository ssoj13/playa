# Playa - TODO List (Phase 4)

**Дата:** 2025-11-18
**Статус:** После завершения архитектурного рефакторинга

---

## ✅ Завершено в предыдущих фазах

- EventBus architecture (events.rs)
- GUI Traits (ProjectUI, TimelineUI, AttributeEditorUI)
- Attrs-based properties для Clip/Comp
- egui_dnd интеграция для timeline
- Реорганизация модулей в entities/
- Универсальный AttributeEditor
- Сборка проекта работает

---

## 📋 High Priority

### 1. Завершить EventBus handlers

#### 1.1 Playback control
- [ ] **StepForward** - шаг вперёд на 1 фрейм
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика: `comp.set_current_frame(comp.current_frame + 1)`
  - Нужно учитывать границы play_range

- [ ] **StepBackward** - шаг назад на 1 фрейм
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика: `comp.set_current_frame(comp.current_frame - 1)`
  - Нужно учитывать границы play_range

#### 1.2 Media management
- [ ] **RemoveMedia(uuid)** - удаление клипов/компов
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика:
    - Удалить из `project.media`
    - Удалить из `project.clips_order` или `project.comps_order`
    - Обновить UI
  - Проверить: не используется ли в активном comp

- [ ] **SelectMedia(uuid)** - выбор медиа в project panel
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика:
    - Установить selected state в UI
    - Показать в AttributeEditor
  - Нужно добавить поле для tracking выбранного media

#### 1.3 Timeline interaction
- [ ] **SelectLayer(idx)** - выбор слоя в timeline
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика:
    - `comp.set_selected_layer(Some(idx))`
    - Обновить highlight в timeline
    - Показать layer attrs в AttributeEditor

---

## 📋 Medium Priority

### 2. Project → Timeline Drag-and-Drop

Завершить реализацию drag-and-drop из Project panel в Timeline:

- [ ] **DragStart { media_uuid }**
  - Файл: `src/main.rs`, метод `handle_event()`
  - Логика: Store drag state в egui context
  - UI feedback: cursor change, ghost preview

- [ ] **DragMove { mouse_pos }**
  - Обновить позицию ghost preview
  - Рассчитать target frame на timeline

- [ ] **DragDrop { target_comp, frame }**
  - Добавить media как layer в target_comp на указанный frame
  - Логика: `comp.add_layer(media_uuid, frame, &project)`
  - Clear drag state

- [ ] **DragCancel**
  - Clear drag state
  - Restore cursor

**Связанные файлы:**
- `src/ui.rs` - Project panel (source)
- `src/timeline.rs` - Timeline (target)
- `src/main.rs` - Event handlers

---

### 3. Per-Window Hotkey Handling

Реализовать контекстно-зависимые хоткеи:

- [ ] **Focus tracking**
  - Определять какое окно (panel) активно
  - HotkeyWindow enum уже есть: Global, Viewport, Timeline, Project, AttributeEditor

- [ ] **Window-specific dispatch**
  - Space (Global) → Play/Pause
  - Arrow keys (Timeline focused) → Navigate frames
  - Arrow keys (Project focused) → Navigate items
  - Delete (Timeline focused) → Delete selected layer
  - Delete (Project focused) → Delete selected media
  - Enter (Project focused) → Activate/open media

**Реализация:**
- Добавить `active_window: HotkeyWindow` в PlayaApp
- Update на каждый UI response (`.has_focus()`)
- Dispatch в `handle_event()` на основе context

---

## 📋 Low Priority (опционально)

### 4. egui_taffy - Адаптивные layouts

**Зачем:** Flexible/responsive layout system

- [ ] Добавить `egui_taffy` в Cargo.toml
- [ ] Переписать main layout с использованием taffy
- [ ] Adaptive panel sizes (min/max constraints)

**Заметка:** Текущий layout на egui работает приемлемо, это для улучшения UX

---

### 5. egui_dock - Workspace management

**Зачем:** Dockable panels, tabs, workspace persistence

- [ ] Добавить `egui_dock` в Cargo.toml
- [ ] Реализовать DockState для panels
- [ ] Tab system для multiple comps/viewports
- [ ] Save/restore workspace layout

**Заметка:** Из original plan, пока не критично

---

## 🧪 Тестирование

После реализации каждого handler:

- [ ] Проверить compilation (`.\bootstrap.ps1 build`)
- [ ] Ручное тестирование функциональности
- [ ] Проверить edge cases (empty project, boundary conditions)
- [ ] Проверить сериализация/десериализация (save/load project)

---

## 📝 Заметки

### EventBus Flow
Все UI взаимодействия должны:
1. Отправлять event в EventBus: `self.event_bus.send(AppEvent::...)`
2. Обрабатываться в `handle_event()`
3. Изменять state (player, project, comp)
4. UI автоматически обновляется на следующем frame

### GUI Traits
Каждая entity (Clip, Comp, Project) реализует:
- `ProjectUI::project_ui()` - отображение в project panel
- `TimelineUI::timeline_ui()` - отображение в timeline
- `AttributeEditorUI::ae_ui()` - отображение в attribute editor

### Attrs System
Все editable properties хранятся в `attrs: Attrs`:
- Использовать геттеры: `comp.name()`, `comp.start()`, `comp.fps()`
- Использовать сеттеры: `comp.set_name()`, `comp.set_start()`
- НЕ обращаться к полям напрямую

---

## 🎯 Рекомендуемый порядок выполнения

1. **StepForward/Backward** (простые, быстро)
2. **SelectLayer** (нужен для UI feedback)
3. **SelectMedia** (аналогично)
4. **RemoveMedia** (чуть сложнее, нужны проверки)
5. **DragDrop flow** (самое сложное, но важное)
6. **Per-window hotkeys** (улучшение UX)
7. **egui_taffy/dock** (опционально, если время есть)

---

**Последнее обновление:** 2025-11-18
**Следующий шаг:** Выбрать задачу из High Priority для реализации
