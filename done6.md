# Session 6: Project Selection Persistence & Status Bar Enhancements

## Дата: 2025-11-21

## Обзор

В этой сессии завершена реализация персистентного выделения элементов в Project панели и добавлена информация о диапазонах воспроизведения в status bar. Основное внимание было уделено правильной архитектуре через EventBus.

---

## Проблема 1: Персистентное выделение не восстанавливалось корректно

### Симптомы
После перезапуска приложения выделенный элемент в Project панели визуально подсвечивался, но comp не становился активным (не загружались кадры, timeline не обновлялся).

### Причина
При десериализации состояния приложения поле `selected_media_uuid` восстанавливалось, но:
1. Не вызывался `player.set_active_comp()` для активации comp
2. Не запускалась загрузка кадров вокруг playhead
3. Логика активации дублировалась в разных местах (клик в UI vs десериализация)

### Решение

#### Шаг 1: Единая функция для выделения элемента

Создана функция `select_item()` в `PlayaApp`, которая инкапсулирует всю логику выбора:

```rust
/// Select and activate media item (comp/clip) by UUID
fn select_item(&mut self, uuid: String) {
    self.selected_media_uuid = Some(uuid.clone());
    self.player.set_active_comp(uuid.clone());
    // Trigger frame loading around new current_frame
    self.enqueue_frame_loads_around_playhead(10);
}
```

**Почему именно так:**
- Единая точка ответственности (Single Responsibility Principle)
- Гарантирует, что при выделении элемента всегда выполняются все необходимые действия
- Предотвращает дублирование кода и возможные рассинхронизации

#### Шаг 2: Использование EventBus для кликов

В `render_project_tab()` при клике отправляется событие через EventBus:

```rust
// Handle selection from click via EventBus
if let Some(uuid) = project_actions.selected_uuid {
    self.event_bus.send(events::AppEvent::SelectMedia(uuid));
}
```

В `handle_event()` добавлена обработка события:

```rust
AppEvent::SelectMedia(uuid) => {
    // Select and activate media item (comp/clip)
    self.select_item(uuid);
}
```

**Почему именно так:**
- Соблюдается единая архитектура приложения - все UI-взаимодействия идут через EventBus
- Событие `SelectMedia` уже существовало в `AppEvent` (было помечено TODO)
- Обработчики событий централизованы в `handle_event()`
- Легко добавить дополнительные обработчики (логирование, аналитика, отмена и т.д.)

#### Шаг 3: Прямой вызов при десериализации

В блоке загрузки состояния (main.rs, строка ~1700):

```rust
// Restore selected media item (activate if exists)
if let Some(selected_uuid) = app.selected_media_uuid.clone() {
    if app.player.project.media.contains_key(&selected_uuid) {
        app.select_item(selected_uuid.clone());
        info!("Restored selected media: {}", selected_uuid);
    }
}
```

**Почему прямой вызов, а не через EventBus:**
- Десериализация происходит в блоке инициализации приложения (closure в eframe::run_native)
- EventBus обрабатывается в `update()`, который еще не вызван
- Это разовая операция восстановления состояния, а не реакция на UI-действие
- Прямой вызов гарантирует синхронное выполнение до первого кадра отрисовки

### Файлы изменены
- `src/main.rs`:
  - Добавлена функция `select_item()` (строка 1175)
  - Обновлена `render_project_tab()` для отправки SelectMedia (строка 1193)
  - Обновлена обработка SelectMedia в `handle_event()` (строка 640)
  - Добавлено восстановление выделения при десериализации (строка 1699)

---

## Проблема 2: Отсутствие информации о диапазонах в status bar

### Требование
Пользователь запросил отображение в status bar информации о текущем comp/clip:
```
<start | play_start <current_frame> play_end | end>
```

Например: `<0 | 50 <75> 100 | 149>`

Где:
- `start` / `end` - полный диапазон композиции
- `play_start` / `play_end` - диапазон воспроизведения (work area)
- `current_frame` - текущий кадр

### Решение

Добавлен новый блок в `StatusBar::render()` после отображения FPS:

```rust
// Comp/Clip range info: <start | play_start <current_frame> play_end | end>
if let Some(comp_uuid) = &player.active_comp {
    if let Some(comp) = player.project.media.get(comp_uuid) {
        ui.separator();
        let start = comp.start();
        let end = comp.end();
        let (play_start, play_end) = comp.play_range();
        let current = comp.current_frame;
        ui.monospace(format!(
            "<{} | {} <{}> {} | {}>",
            start, play_start, current, play_end, end
        ));
    }
}
```

**Почему именно так:**
- Используется существующий API `Comp`: `start()`, `end()`, `play_range()`, `current_frame`
- Отображается только для активного comp (если нет активного - ничего не показывается)
- Моноширинный шрифт (`ui.monospace`) для выравнивания цифр
- Информация обновляется автоматически при каждом перерисовке (egui immediate mode)

### Ошибка при компиляции

**Проблема:**
```
error[E0599]: no method named `current_frame` found for reference `&Comp`
   --> src\widgets\status\status.rs:99:44
    |
 99 |                         let current = comp.current_frame();
    |                                            ^^^^^^^^^^^^^-- help: remove the arguments
```

**Причина:**
`current_frame` - это публичное поле структуры `Comp`, а не метод.

**Решение:**
```rust
let current = comp.current_frame;  // Поле, а не метод
```

**Как избежать в будущем:**
- При работе с незнакомым API сначала проверить определение структуры
- Использовать IDE с автодополнением (rust-analyzer)
- Если поле публичное - доступ через `.field`, если приватное - через getter `.field()`

### Файлы изменены
- `src/widgets/status/status.rs`:
  - Добавлен блок отображения диапазонов после FPS (строка 92)

---

## Архитектурные решения

### 1. EventBus как центральная шина событий

**Принцип:**
Все UI-взаимодействия (клики, hotkeys, actions) должны отправлять события через EventBus, а не вызывать методы напрямую.

**Преимущества:**
- **Разделение ответственности**: UI виджеты только отправляют события, не знают о бизнес-логике
- **Централизация обработки**: вся логика в `handle_event()`, легко отслеживать
- **Расширяемость**: легко добавить middleware (логирование, аналитика,undo/redo)
- **Тестируемость**: можно тестировать отправку событий независимо от обработки

**Когда можно вызывать методы напрямую:**
- Инициализация / десериализация (до запуска event loop)
- Вспомогательные утилиты (форматирование, вычисления)
- Internal helpers внутри одного модуля

### 2. Единая функция для сложных операций

**Проблема:**
Операция "выбрать элемент" требует:
1. Сохранить UUID в `selected_media_uuid`
2. Активировать comp через `player.set_active_comp()`
3. Запустить загрузку кадров `enqueue_frame_loads_around_playhead()`

**Решение:**
Инкапсулировать в `select_item()` - единая точка входа для выделения.

**Альтернативы (почему НЕ использованы):**
- ❌ Дублировать код в каждом месте вызова - риск забыть один из шагов
- ❌ Сделать SelectMedia событие самодостаточным - нарушает SRP, событие не должно знать о загрузке кадров
- ✅ Централизованный метод, вызываемый из обработчика события

### 3. Immediate Mode UI с минимальным состоянием

**Принцип egui:**
Каждый кадр UI пересчитывается с нуля на основе текущего состояния.

**Status bar:**
- Не хранит текущие значения диапазонов
- Каждый кадр читает `comp.start()`, `comp.end()`, `comp.current_frame`
- Автоматически отображает актуальные данные

**Когда нужно кешировать:**
- Тяжелые вычисления (в данном случае - просто чтение полей)
- Асинхронные операции
- Данные, которые нужно сохранять между кадрами

---

## Рекомендации на будущее

### 1. Проверка типов перед использованием API

**Правило:**
Перед вызовом метода/поля убедиться в его сигнатуре.

**Как:**
```rust
// Смотрим в определение структуры
pub struct Comp {
    pub current_frame: i32,  // Поле - доступ без ()
    // ...
}

// Использование
let frame = comp.current_frame;  // ✅ Правильно
let frame = comp.current_frame();  // ❌ Ошибка
```

### 2. EventBus для всех UI-взаимодействий

**Правило:**
Виджеты должны возвращать Actions/Events, которые обрабатываются через EventBus.

**Паттерн:**
```rust
// Виджет
pub fn render(...) -> WidgetActions {
    let mut actions = WidgetActions::new();
    if ui.button("Click").clicked() {
        actions.some_action = Some(data);
    }
    actions
}

// Контейнер
fn render_widget_tab(&mut self, ui: &mut egui::Ui) {
    let actions = widget::render(ui, &mut self.data);
    if let Some(data) = actions.some_action {
        self.event_bus.send(AppEvent::SomeEvent(data));
    }
}

// Обработка
fn handle_event(&mut self, event: AppEvent) {
    match event {
        AppEvent::SomeEvent(data) => {
            self.do_something(data);
        }
    }
}
```

### 3. Персистентные данные через serde

**Что сохранять:**
- Состояние UI (выделение, раскрытые панели, zoom)
- Настройки пользователя
- Последний открытый проект

**Что НЕ сохранять:**
- Runtime данные (Arc, Mutex, каналы) - пометить `#[serde(skip)]`
- Кешированные вычисления
- Временные буферы

**Паттерн:**
```rust
#[derive(Serialize, Deserialize)]
struct AppState {
    pub selected_uuid: Option<String>,  // Сохраняется
    #[serde(skip)]
    pub event_sender: Sender<Event>,     // Пропускается
}
```

### 4. Синхронизация состояния при десериализации

**Проблема:**
После загрузки сериализованного состояния может потребоваться:
- Восстановить runtime ссылки (Arc, каналы)
- Активировать выбранные элементы
- Запустить фоновые задачи

**Решение:**
```rust
// 1. Десериализация
let mut app: PlayaApp = storage
    .and_then(|s| s.get_string(eframe::APP_KEY))
    .and_then(|json| serde_json::from_str(&json).ok())
    .unwrap_or_default();

// 2. Восстановление runtime (rebuild_runtime)
app.player.project.rebuild_runtime(Some(app.comp_event_sender.clone()));

// 3. Синхронизация состояния (select_item)
if let Some(uuid) = app.selected_media_uuid.clone() {
    if app.player.project.media.contains_key(&uuid) {
        app.select_item(uuid);
    }
}
```

**Порядок важен:**
1. Сначала rebuild_runtime (восстановить Arc/каналы)
2. Потом синхронизация состояния (активация может использовать каналы)

---

## Итоги

### Что сделано
1. ✅ Добавлена единая функция `select_item()` для выделения элементов
2. ✅ Реализована обработка `SelectMedia` через EventBus
3. ✅ Восстановление выделения при загрузке состояния работает корректно
4. ✅ Status bar отображает диапазоны: `<start | play_start <current_frame> play_end | end>`
5. ✅ Вся архитектура согласована с принципами EventBus

### Файлы изменены
```
src/main.rs                      - select_item(), обработка SelectMedia, десериализация
src/widgets/status/status.rs     - отображение диапазонов
```

### Следующие шаги
- [ ] Добавить hotkeys для навигации по элементам Project панели
- [ ] Реализовать drag-and-drop между элементами
- [ ] Добавить контекстное меню для элементов (правая кнопка мыши)
- [ ] Рефакторинг других виджетов для использования EventBus (если есть прямые вызовы)

### Время сессии
~40 минут (поиск проблемы, рефакторинг, тестирование)
