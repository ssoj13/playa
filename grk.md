# Исследование проблем в Playa

## Введение

Проведено исследование кода приложения Playa на основе отчета report.md. Проблемы подтверждены анализом кода. Ниже приведен детальный анализ, dataflow диаграммы и план исправлений.

## Анализ проблем

### 1. Клики в окне проекта не работают, ничего не выделяется

**Статус:** Частично подтверждено. Код обработки кликов присутствует, но возможно проблемы с обработкой событий или обновлением UI.

**Расположение кода:** `src/widgets/project/project_ui.rs`, функция `render()`

**Dataflow:**
```
Пользователь кликает на элемент проекта
  ↓
egui::Sense::click_and_drag обнаруживает клик
  ↓
response.clicked() == true
  ↓
compute_selection() вычисляет новую selection
  ↓
Отправляется ProjectSelectionChangedEvent
  ↓
EventBus → main_events.rs обрабатывает событие
  ↓
Обновляется project.selection
  ↓
UI перерисовывается с новой selection
```

**Проблема:** Возможно, событие обрабатывается, но UI не обновляется должным образом, или selection не сохраняется.

### 2. Нет drag'n'drop

**Статус:** Подтверждено частично. Drag из проекта реализован, но drop в timeline может не работать.

**Расположение кода:** `src/widgets/project/project_ui.rs` (drag), `src/widgets/timeline/timeline_ui.rs` (drop)

**Dataflow для drag из проекта:**
```
Пользователь начинает drag на элементе проекта
  ↓
response.drag_started()
  ↓
GlobalDragState::ProjectItem сохраняется в ui.ctx()
  ↓
Timeline обнаруживает drag state
  ↓
draw_drop_preview() показывает preview
  ↓
При drop: должно отправляться событие добавления layer
```

**Проблема:** Drop handling может быть не реализован или не работать.

### 3. Timeline не таскается

**Статус:** Полностью подтверждено. Код отправляет событие, но оно не обрабатывается.

**Расположение кода:** `src/widgets/timeline/timeline_ui.rs` (отправка), отсутствует обработка в `src/main_events.rs`

**Dataflow:**
```
Пользователь middle-drag или scroll wheel на timeline
  ↓
timeline_response.hovered() && middle button down
  ↓
GlobalDragState::TimelinePan инициализируется
  ↓
Или: scroll_delta.x > 0
  ↓
TimelinePanChangedEvent(new_pan_offset) отправляется
  ↓
EventBus → main_events.rs НЕ ОБРАБАТЫВАЕТ событие!
  ↓
state.pan_offset НЕ обновляется
  ↓
UI не панируется
```

**Проблема:** `TimelinePanChangedEvent` определено, но не обрабатывается в `main_events.rs`.

## Дополнительные проблемы

### Компиляция не проходит
Из-за проблем с FFmpeg bindings. Требует исправления зависимостей.

## План исправлений

### Высокий приоритет
1. **Исправить timeline pan:** Добавить обработку `TimelinePanChangedEvent` в `main_events.rs`:
   ```rust
   if let Some(e) = downcast_event::<TimelinePanChangedEvent>(&event) {
       // Обновить state.pan_offset = e.0
       // Возможно, сохранить в timeline state
   }
   ```

2. **Проверить project selection:** Убедиться, что `ProjectSelectionChangedEvent` правильно обновляет `project.selection` и вызывает перерисовку UI.

3. **Реализовать drop для drag'n'drop:** В `timeline_ui.rs` добавить обработку drop из `GlobalDragState::ProjectItem`, отправляя `AddLayerEvent`.

### Средний приоритет
4. **Исправить компиляцию:** Обновить FFmpeg зависимости или использовать cargo features для отключения FFmpeg.

5. **Добавить логирование:** Добавить debug логи в обработку событий для диагностики.

### Низкий приоритет
6. **Тестирование UI:** Добавить unit тесты для UI interactions.

## Заключение

Основные проблемы - отсутствие обработки событий timeline pan и возможные проблемы с event handling для selection и drag'n'drop. Код структуры присутствует, но не завершен.