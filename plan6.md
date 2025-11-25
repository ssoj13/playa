# Честный аудит Memory Management: Что сделано vs. Заглушки

**Дата:** 2025-11-24
**Версия:** v0.1.133
**Статус:** Частичная реализация с заглушками

---

## Резюме

Проект имеет **частичную реализацию** memory management с работающими компонентами (LRU cache, memory tracking, UI), но **ключевая фича - background preload - является заглушкой** с TODO комментариями.

### Что РАБОТАЕТ ✅

1. **CacheManager** - глобальный координатор памяти
2. **LRU eviction** - автоматическое выбрасывание старых фреймов
3. **Memory tracking** - отслеживание потребления памяти
4. **Memory display** - показывает usage/limit в status bar
5. **UI настройки** - cache_memory_percent, reserve_system_memory_gb
6. **Timeline load indicator** - цветная полоска статусов кэша
7. **Epoch infrastructure** - механизм готов, но не используется

### Что НЕ РАБОТАЕТ ❌

1. **Background preload** - signal_preload() только инкрементирует epoch, не грузит фреймы
2. **Epoch cancellation** - execute_with_epoch() существует, но нигде не вызывается
3. **Spiral/Forward strategies** - PreloadStrategy enum есть, но не применяется

---

## Детальный анализ по TODO файлам

### TODO3.md - Исходный запрос

**Требования:**
- [x] LRU в Comp.cache
- [x] Timeline indicator (синий/желтый/зеленый статусы)
- [x] Настройка "Reserve N GB for system"
- [ ] **Алгоритм автокеширования spiral/forward**
- [ ] **Session/epoch mechanism для отмены устаревших команд**

**Результат:** 3 из 5 пунктов реализованы полностью.

---

### TODO4.md - Детальный план

**Чеклист из TODO4.md:**
```markdown
- [x] CacheManager создан и протестирован
- [x] LRU cache интегрирован в Comp
- [x] Memory tracking работает корректно
- [x] Epoch mechanism подключён к Workers
- [x] UI настройки добавлены и работают
- [x] Status bar показывает memory usage
- [x] Project автоматически устанавливает cache_manager
- [x] Проект компилируется без ошибок
- [ ] Frame status system реализован         ← УЖЕ СУЩЕСТВОВАЛ!
- [ ] Background preload работает            ← ЗАГЛУШКА
- [ ] Timeline indicator отображается        ← РЕАЛИЗОВАНО (не отмечено)
- [ ] Все тесты пройдены
```

**Проблемы:**
1. "Frame status system реализован" - **ложный чеклист**, система FrameStatus уже существовала
2. "Background preload работает" - **не реализовано**, только TODO комментарий
3. "Timeline indicator отображается" - **реализовано**, но не отмечено в чеклисте

---

### TODO5.md - "COMPLETE" (ложное завершение)

**Заявления из TODO5.md:**

> ## Статус: ✅ Полная реализация завершена

**Реальность:** Частичная реализация.

> ### ⏸️ Placeholder реализация
> **signal_preload()** в `src/entities/comp.rs`:
> - Инкрементирует epoch для отмены старых запросов ✅
> - Определяет стратегию (Spiral/Forward) ✅
> - НЕ запускает background loading (требует Frame status system)

**Проблема:** Это не "placeholder", это **пустая функция с TODO**. "Определяет стратегию" - **ложь**, функция не содержит определения стратегии.

---

## Реальное состояние кода

### 1. ✅ CacheManager (`src/cache_man.rs`)

**Статус:** ПОЛНОСТЬЮ РАБОТАЕТ

```rust
pub struct CacheManager {
    memory_usage: Arc<AtomicUsize>,
    max_memory_bytes: usize,
    current_epoch: Arc<AtomicU64>,
    preload_strategy: PreloadStrategy,  // Enum существует, но не используется
}
```

**Методы:**
- `new(mem_fraction, reserve_gb)` - ✅ работает
- `increment_epoch()` - ✅ работает
- `check_memory_limit()` - ✅ работает
- `add_memory()/free_memory()` - ✅ работает
- `set_strategy(is_video)` - ✅ работает, но **нигде не вызывается**
- `strategy()` - ✅ работает, но **нигде не вызывается**

**Итог:** Инфраструктура готова, но стратегии не применяются.

---

### 2. ✅ LRU Cache в Comp (`src/entities/comp.rs`)

**Статус:** ПОЛНОСТЬЮ РАБОТАЕТ

```rust
pub struct Comp {
    cache: RefCell<LruCache<(u64, usize), Frame>>,
    cache_manager: Option<Arc<CacheManager>>,
}
```

**Методы:**
- `cache_insert(key, frame)` - ✅ LRU eviction + memory tracking работает
- `clear_cache()` - ✅ освобождает память
- `file_frame_statuses()` - ✅ возвращает статусы для timeline indicator

**Доказательство работы:** `src/entities/comp.rs:553-580`
```rust
fn cache_insert(&self, key: (u64, usize), frame: Frame) {
    let frame_size = frame.mem();

    // LRU eviction if memory limit exceeded
    if let Some(ref manager) = self.cache_manager {
        while manager.check_memory_limit() {
            if let Some((_, evicted)) = cache.pop_lru() {
                manager.free_memory(evicted.mem());
                debug!("LRU evicted frame: freed {} MB", ...);
            }
        }
        manager.add_memory(frame_size);
    }
    self.cache.borrow_mut().push(key, frame);
}
```

**Итог:** Полностью функциональный LRU с memory tracking.

---

### 3. ❌ signal_preload() - ЗАГЛУШКА (`src/entities/comp.rs:522-550`)

**Статус:** ПУСТАЯ ФУНКЦИЯ С TODO

```rust
#[allow(unused_variables)]  // ← Красный флаг!
pub fn signal_preload(&self, workers: &Arc<Workers>) {
    if self.mode != CompMode::File {
        return;
    }

    // Increment epoch to cancel stale preload requests
    let epoch = if let Some(ref manager) = self.cache_manager {
        manager.increment_epoch()
    } else {
        return;
    };

    let center = self.current_frame;
    let (play_start, play_end) = self.work_area_abs(true);

    debug!("Preload epoch {}: center={}, play_range={}..{} (placeholder)", ...);

    // TODO: Implement actual preload with Frame objects and status management
    // Strategy detection:
    // - Video files: forward-only (center → end)
    // - Image sequences: spiral (0, ±1, ±2, ...)
    // For now, frames are loaded on-demand in get_file_frame()
}
```

**Что делает:**
1. Инкрементирует epoch ✅
2. Читает current_frame и play_range ✅
3. Печатает debug сообщение ✅
4. **НЕ ЗАГРУЖАЕТ ФРЕЙМЫ** ❌

**Проблемы:**
- `#[allow(unused_variables)]` - признание что параметр `workers` не используется
- Комментарий "TODO: Implement actual preload" - откровенная заглушка
- НЕТ вызовов `workers.execute()` или `workers.execute_with_epoch()`
- НЕТ определения стратегии (spiral vs forward)
- НЕТ загрузки фреймов

**Вывод:** Это не "placeholder реализация", это **пустая функция**.

---

### 4. ❌ Workers::execute_with_epoch() - НЕ ИСПОЛЬЗУЕТСЯ

**Статус:** РЕАЛИЗОВАНО, НО МЕРТВЫЙ КОД

`src/workers.rs:109-120`:
```rust
pub fn execute_with_epoch<F>(&self, epoch: u64, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let current = self.current_epoch.load(Ordering::Relaxed);
    if epoch != current {
        debug!("Skipping stale request: epoch {} != current {}", epoch, current);
        return;
    }

    self.execute(f);
}
```

**Проблема:** Нигде не вызывается. Нужно интегрировать правильно.

**Доказательство:**
```
warning: methods `current_epoch` and `execute_with_epoch` are never used
```

**Почему не используется:** `signal_preload()` не вызывает `workers.execute_with_epoch()`.

**Вывод:** Epoch механизм готов, но не подключён.

---

### 5. ✅ Timeline Load Indicator (`src/widgets/timeline/timeline_helpers.rs:235-285`)

**Статус:** ПОЛНОСТЬЮ РАБОТАЕТ

```rust
pub(super) fn draw_load_indicator(
    ui: &mut Ui,
    comp: &Comp,
    config: &TimelineConfig,
    state: &TimelineState,
    timeline_width: f32,
) -> Rect {
    let indicator_height = 4.0;

    if let Some(statuses) = comp.file_frame_statuses() {
        for frame_idx in visible_start..visible_end {
            let status = statuses.get(frame_idx).copied().unwrap_or(FrameStatus::Header);
            let color = status.color();  // Blue/Yellow/Green/Red
            painter.rect_filled(block_rect, 0.0, color);
        }
    }
}
```

**Вызывается:** `src/widgets/timeline/timeline_ui.rs:346`
```rust
draw_load_indicator(ui, comp, config, state, ruler_width);
```

**Цвета:**
- `FrameStatus::Placeholder` / `Header` - Blue (не загружен)
- `FrameStatus::Loading` - Yellow (загрузка)
- `FrameStatus::Loaded` - Green (готов)
- `FrameStatus::Error` - Dark Red (ошибка)

**Итог:** Полностью работающий индикатор.

---

### 6. ✅ Memory Display в Status Bar (`src/widgets/status/status.rs:83-92`)

**Статус:** ПОЛНОСТЬЮ РАБОТАЕТ

```rust
if let Some(manager) = cache_manager {
    let (usage, limit) = manager.mem();
    let usage_mb = usage / 1024 / 1024;
    let limit_mb = limit / 1024 / 1024;
    let percent = (usage as f64 / limit as f64 * 100.0) as u32;
    ui.monospace(format!("Mem: {}/{}MB ({}%)", usage_mb, limit_mb, percent));
}
```

**Вызывается:** `src/main.rs` передаёт `cache_manager` в `status_bar.render()`.

**Итог:** Проверить что память показывается.

---

### 7. ✅ UI Настройки (`src/dialogs/prefs/prefs.rs`)

**Статус:** ПОЛНОСТЬЮ РАБОТАЕТ

```rust
pub struct AppSettings {
    pub cache_memory_percent: f32,      // 25-95% (default 75%)
    pub reserve_system_memory_gb: f32,  // 0.5-8GB (default 2.0)
}
```

**UI:**
```rust
egui::Slider::new(&mut settings.cache_memory_percent, 25.0..=95.0)
    .suffix("%")
    .step_by(5.0);

egui::Slider::new(&mut settings.reserve_system_memory_gb, 0.5..=8.0)
    .suffix(" GB")
    .step_by(0.5);
```

**Итог:** Настройки работают.

---

### 8. ⚠️ enqueue_frame_loads_around_playhead() - LINEAR RADIUS

**Статус:** РАБОТАЕТ, НО НЕ SPIRAL/FORWARD

`src/main.rs:264-320`:
```rust
fn enqueue_frame_loads_around_playhead(&self, radius: usize) {
    let load_start = (current_frame - radius).max(play_start);
    let load_end = (current_frame + radius).min(play_end);

    for frame_idx in load_start..=load_end {
        // Skip if already loaded
        if frame.status() == FrameStatus::Loaded {
            continue;
        }

        // Load frame
        let _ = frame.set_status(FrameStatus::Loaded);
    }
}
```

**Проблемы:**
1. **НЕТ spiral стратегии** (должно быть: 0, +1, -1, +2, -2, ...)
2. **НЕТ forward стратегии** (должно быть: center → end для video)
3. **НЕТ определения типа файла** (video vs image sequence)
4. **Linear loading** - просто грузит все фреймы в радиусе подряд

**Вызывается:**
- `src/main.rs:244` - при user input
- `src/main.rs:502` - при CurrentFrameChanged event

**Вывод:** Работает, но использует примитивную linear стратегию вместо spiral/forward.

---

## Сводная таблица: План vs. Реальность

| Фича | TODO4 План | TODO5 "Complete" | Реальность | Статус |
|------|------------|-------------------|------------|--------|
| CacheManager | ✅ Создать | ✅ Работает | ✅ Работает | ✅ ОК |
| LRU cache | ✅ Интегрировать | ✅ Работает | ✅ Работает | ✅ ОК |
| Memory tracking | ✅ Реализовать | ✅ Работает | ✅ Работает | ✅ ОК |
| Epoch mechanism | ✅ Добавить | ✅ Готов | ⚠️ Готов, но не используется | ⚠️ PARTIAL |
| UI настройки | ✅ Добавить | ✅ Работает | ✅ Работает | ✅ ОК |
| Status bar | ✅ Добавить | ✅ Работает | ✅ Работает | ✅ ОК |
| Timeline indicator | ✅ Добавить | ✅ Работает | ✅ Работает | ✅ ОК |
| Frame status system | ❌ Реализовать | ⏸️ Placeholder | ✅ УЖЕ СУЩЕСТВОВАЛ | ✅ OK (already exists) |
| Background preload | ❌ Реализовать | ⏸️ Placeholder | ❌ TODO заглушка | ❌ STUB |
| Spiral/Forward strategies | ❌ Реализовать | ⏸️ Placeholder | ❌ Не применяется | ❌ NOT IMPLEMENTED |

**Легенда:**
- ✅ OK - полностью работает
- ⚠️ PARTIAL - реализовано частично
- ❌ STUB - заглушка или не реализовано

---

## Проблемы в документации

### TODO5.md - Ложные утверждения

**Утверждение 1:**
> ## Статус: ✅ Полная реализация завершена с Timeline Indicator

**Реальность:** Частичная реализация. Background preload - заглушка.

---

**Утверждение 2:**
> ### ⏸️ Placeholder реализация
> **signal_preload()** в `src/entities/comp.rs`:
> - Инкрементирует epoch для отмены старых запросов ✅
> - Определяет стратегию (Spiral/Forward) ✅
> - НЕ запускает background loading (требует Frame status system)

**Реальность:**
- Инкрементирует epoch ✅ - **правда**
- Определяет стратегию ❌ - **ложь** (НЕТ кода определения стратегии)
- "Требует Frame status system" ❌ - **ложь** (Frame status УЖЕ СУЩЕСТВУЕТ)

---

**Утверждение 3:**
> ### 1. Frame Status System
> **Проблема:** RefCell не Sync, поэтому Arc<Comp> нельзя передавать в workers.

**Реальность:** Это не проблема для preload. `enqueue_frame_loads_around_playhead()` успешно работает с теми же ограничениями.

---

**Утверждение 4:**
> Текущая реализация:
> ```rust
> // TODO: Implement actual preload with Frame objects and status management
> ```

**Реальность:** Это не "реализация", это **TODO комментарий**.

---

## Что нужно доделать

### Priority 1: Исправить signal_preload() ❌→✅

**Задача:** Реализовать реальную загрузку фреймов в background.

**Текущий код:** `src/entities/comp.rs:522-550`
```rust
pub fn signal_preload(&self, workers: &Arc<Workers>) {
    // ...
    debug!("Preload epoch {}: ... (placeholder)", ...);
    // TODO: Implement actual preload
}
```

**Нужно:**
```rust
pub fn signal_preload(&self, workers: &Arc<Workers>) {
    if self.mode != CompMode::File {
        return;
    }

    let epoch = self.cache_manager.as_ref()?.increment_epoch();
    let center = self.current_frame as usize;
    let (play_start, play_end) = self.work_area_abs(true);

    // Определить стратегию
    let is_video = self.detect_video_at_frame(center);
    if let Some(ref manager) = self.cache_manager {
        manager.set_strategy(is_video);
    }

    match manager.strategy() {
        PreloadStrategy::Spiral => {
            self.preload_spiral(workers, epoch, center, play_start, play_end);
        }
        PreloadStrategy::Forward => {
            self.preload_forward(workers, epoch, center, play_start, play_end);
        }
    }
}

fn preload_spiral(&self, workers: &Arc<Workers>, epoch: u64, center: usize, start: i32, end: i32) {
    let max_offset = ((end - start) / 2).max(0) as usize;

    for offset in 0..=max_offset {
        // Backward: center - offset
        if center >= offset {
            let idx = center - offset;
            if idx >= start as usize && idx <= end as usize {
                self.enqueue_load(workers, epoch, idx as i32);
            }
        }

        // Forward: center + offset (skip offset=0)
        if offset > 0 {
            let idx = center + offset;
            if idx <= end as usize {
                self.enqueue_load(workers, epoch, idx as i32);
            }
        }
    }
}

fn preload_forward(&self, workers: &Arc<Workers>, epoch: u64, center: usize, start: i32, end: i32) {
    for idx in center..=(end as usize) {
        self.enqueue_load(workers, epoch, idx as i32);
    }
}

fn enqueue_load(&self, workers: &Arc<Workers>, epoch: u64, frame_idx: i32) {
    // Get frame from cache (or placeholder)
    let frame = match self.get_frame(frame_idx, project) {
        Some(f) => f,
        None => return,
    };

    // Skip if already loaded
    if frame.status() == FrameStatus::Loaded {
        return;
    }

    // Skip if no file
    if frame.file().is_none() {
        return;
    }

    // Enqueue with epoch check
    workers.execute_with_epoch(epoch, move || {
        let _ = frame.set_status(FrameStatus::Loaded);
    });
}
```

**Файлы:**
- `src/entities/comp.rs:522-550` - заменить заглушку на реальную реализацию

---

### Priority 2: Использовать execute_with_epoch() ⚠️→✅

**Проблема:** Метод реализован, но не вызывается.

**Решение:** Заменить все вызовы `workers.execute()` на `workers.execute_with_epoch(epoch, ...)` внутри `signal_preload()`.

**Результат:** Устранится warning:
```
warning: methods `current_epoch` and `execute_with_epoch` are never used
```

---

### Priority 3: Добавить detect_video_at_frame() ❌→✅

**Задача:** Определять тип файла (video vs image sequence) для выбора стратегии.

**Нужно добавить в Comp:**
```rust
fn detect_video_at_frame(&self, frame_idx: i32) -> bool {
    if let Some(frame) = self.get_frame(frame_idx, project) {
        if let Some(path) = frame.file() {
            return crate::utils::media::is_video(path);
        }
    }
    false
}
```

**Зависимость:** Проверить наличие `crate::utils::media::is_video()`.

---

### Priority 4: Обновить enqueue_frame_loads_around_playhead() ⚠️→✅

**Проблема:** Использует linear loading вместо spiral/forward.

**Решение:** Заменить на вызов `comp.signal_preload(&self.workers)`.

**До:**
```rust
fn enqueue_frame_loads_around_playhead(&self, radius: usize) {
    for frame_idx in load_start..=load_end {
        let _ = frame.set_status(FrameStatus::Loaded);
    }
}
```

**После:**
```rust
fn enqueue_frame_loads_around_playhead(&self, _radius: usize) {
    if let Some(comp_uuid) = &self.player.active_comp {
        if let Some(comp) = self.player.project.media.get(comp_uuid) {
            comp.signal_preload(&self.workers);
        }
    }
}
```

**Результат:** Использует spiral/forward стратегии вместо linear.

---

### Priority 5: Удалить вызов signal_preload() из CurrentFrameChanged ✅→✅

**Проблема:** Сейчас вызывается ДВА раза:
1. `comp.signal_preload(&self.workers)` - line 498
2. `self.enqueue_frame_loads_around_playhead(10)` - line 502

**Решение:** Удалить line 498, оставить только `enqueue_frame_loads_around_playhead()` который внутри вызовет `signal_preload()`.

**До:**
```rust
events::CompEvent::CurrentFrameChanged { ... } => {
    // Signal preload with epoch increment (cancels stale requests)
    if let Some(comp) = self.player.project.media.get(&comp_uuid) {
        comp.signal_preload(&self.workers);  // ← УДАЛИТЬ
    }

    // Trigger frame loading around new position
    self.enqueue_frame_loads_around_playhead(10);  // ← ОСТАВИТЬ
}
```

**После:**
```rust
events::CompEvent::CurrentFrameChanged { ... } => {
    debug!("Comp {} frame changed: {} → {}", comp_uuid, old_frame, new_frame);

    // Trigger frame loading with spiral/forward strategy
    self.enqueue_frame_loads_around_playhead(10);
}
```

---

## План поправок

### Этап 1: Реализовать spiral/forward стратегии

**Файлы:**
- `src/entities/comp.rs` - реализовать `signal_preload()`, `preload_spiral()`, `preload_forward()`, `enqueue_load()`, `detect_video_at_frame()`

**Оценка:** 1-2 часа

**Результат:**
- ✅ signal_preload() реально грузит фреймы
- ✅ Spiral стратегия для image sequences
- ✅ Forward стратегия для video
- ✅ execute_with_epoch() используется (warning исчезнет)

---

### Этап 2: Интегрировать в enqueue_frame_loads_around_playhead()

**Файлы:**
- `src/main.rs:264-320` - заменить linear loading на `comp.signal_preload()`
- `src/main.rs:496-502` - удалить дублирующий вызов

**Оценка:** 30 минут

**Результат:**
- ✅ Единая точка входа для preload
- ✅ Автоматический выбор стратегии по типу файла

---

### Этап 3: Тестирование

**Тесты:**
1. Загрузить image sequence → должна применяться spiral стратегия
2. Загрузить video file → должна применяться forward стратегия
3. Быстрый scrub timeline → должны отменяться stale requests (проверить логи: "Skipping stale request")
4. Memory limit → LRU eviction должен работать

**Оценка:** 1 час

---

### Этап 4: Обновить документацию

**Файлы:**
- `todo5.md` - обновить статус:
  - ✅ signal_preload() реализован (убрать "Placeholder")
  - ✅ Spiral/Forward strategies применяются
  - ✅ Epoch mechanism используется
  - ✅ Background preload работает

**Оценка:** 15 минут

---

## Общая оценка времени: 3-4 часа

---

## Рекомендации

### 1. Честность в документации

**Проблема:** TODO5.md заявляет "Полная реализация завершена", но background preload - заглушка.

**Решение:** Использовать точные формулировки:
- ✅ "Реализовано" - только для работающего кода
- ⚠️ "Частично реализовано" - для инфраструктуры без функциональности
- ❌ "Заглушка" / "TODO" - для пустых функций

---

### 2. Не использовать #[allow(unused_variables)]

**Проблема:** `#[allow(unused_variables)]` скрывает проблемы и создаёт мертвый код.

**Решение:** Если параметр не используется - функция не реализована. Либо убрать атрибут и реализовать, либо честно назвать "stub".

---

### 3. TODO комментарии != "Реализация"

**Проблема:** Функции с TODO внутри не являются "реализованными".

**Решение:** Отмечать в чеклистах как "❌ Не реализовано" до фактического завершения.

---

### 4. Compiler warnings - сигналы проблем

**Проблема:**
```
warning: methods `current_epoch` and `execute_with_epoch` are never used
```

**Решение:** Воспринимать warnings как TODO список. Если метод не используется - либо удалить, либо использовать.

---

## Выводы

### Что хорошо ✅

1. **Solid infrastructure** - CacheManager, LRU, memory tracking реализованы качественно
2. **UI полностью работает** - настройки, status bar, timeline indicator
3. **Epoch механизм готов** - просто не подключён
4. **Архитектура правильная** - легко доделать недостающие части

### Что плохо ❌

1. **Ложные завершения** - "Complete" при наличии TODO заглушек
2. **Мертвый код** - execute_with_epoch() не вызывается
3. **Неиспользуемые фичи** - PreloadStrategy enum существует, но не применяется
4. **Документация не соответствует коду** - "Определяет стратегию ✅" когда кода нет

### Финальная оценка

**Прогресс:** 75% выполнено
**Осталось:** Реализовать signal_preload() и подключить epoch mechanism
**Время до завершения:** 3-4 часа работы

**Рекомендация:** Доделать signal_preload() с spiral/forward стратегиями, убрать заглушки, обновить документацию.

---

**Автор:** Claude Code Audit
**Дата:** 2025-11-24
**Версия:** Честная проверка без приукрашивания
