# Анализ проекта Playa: реализация, баги, оптимизации

## Обзор проекта

Playa - это плеер для последовательностей изображений (EXR, PNG, JPEG, TIFF, MP4) на чистом Rust с поддержкой OpenEXR и FFmpeg.

**Текущая версия:** 0.1.133  
**Язык:** Rust 2024 Edition  
**Ключевые зависимости:** eframe/egui, image, lru, sysinfo, crossbeam, etc.

## Структура проекта

```
src/
├── cache_man.rs          # Глобальный менеджер кэша с LRU и epoch
├── cli.rs                # Аргументы командной строки
├── config.rs             # Конфигурация
├── dialogs/
│   ├── prefs/            # Настройки приложения
│   └── encode/           # Диалог кодирования
├── entities/             # Основные структуры данных
│   ├── comp.rs           # Composition (главный кэш)
│   ├── frame.rs          # Кадр изображения
│   ├── project.rs        # Проект с коллекцией comps
│   └── ...
├── widgets/              # UI компоненты
│   ├── timeline/         # Таймлайн с индикатором загрузки
│   ├── viewport/         # Просмотр изображений
│   ├── status/           # Статус бар
│   └── ...
├── workers.rs            # Пул воркеров для фоновой загрузки
├── main.rs               # Точка входа
└── ...
```

## Анализ TODO файлов

### todo3.md: Проблемы и план
- **Проблема:** Отсутствие управления памятью, риск переполнения
- **Требования:** LRU в Comp.cache, timeline indicator, автокеширование с стратегиями Spiral/Forward, epoch механизм для отмены запросов

### todo4.md: Детальный план реализации
- **Фазы:** Foundation (CacheManager, LRU, настройки), Core (preload логика), UI (timeline indicator)
- **Код:** Полные сниппеты для всех компонентов

### todo5.md: Отчёт о завершении реализации
- **Статус:** ✅ Реализация завершена, проект компилируется
- **Реализовано:** CacheManager, LRU, memory tracking, timeline indicator, UI настройки
- **Placeholder:** signal_preload не запускает background loading (нужен Frame status system)

## Состояние реализации

### ✅ Полностью реализовано

1. **CacheManager (`src/cache_man.rs`)**
   - Глобальный учёт памяти across всех Comp
   - Автоопределение лимита: `mem_fraction` (75%) от available - `reserve_gb` (2GB)
   - Epoch механизм для отмены stale запросов
   - Методы: `new()`, `increment_epoch()`, `check_memory_limit()`, `add_memory()`, `free_memory()`

2. **LRU Cache в Comp (`src/entities/comp.rs`)**
   - Замена HashMap на `LruCache<(u64, usize), Frame>`
   - Memory-aware eviction в `cache_insert()`
   - Освобождение памяти при eviction

3. **Memory Tracking**
   - `Arc<AtomicUsize>` для thread-safe учёта
   - Отображение в status bar: `Mem: usage/limit MB (percent%)`
   - Live update при изменении настроек

4. **UI настройки (`src/dialogs/prefs/prefs.rs`)**
   - `cache_memory_percent`: 25-95% (default 75%)
   - `reserve_system_memory_gb`: 0.5-8GB (default 2.0)
   - Слайдеры с шагом 5% и 0.5GB

5. **Timeline Load Indicator (`src/widgets/timeline/timeline_helpers.rs`)**
   - Цветная полоска под ruler: Blue (незагружен), Yellow (загрузка), Green (загружен), Red (ошибка)
   - Высота 4px, синхронизирован с pan/zoom
   - Использует `comp.cache_frame_statuses()`

6. **Интеграция в main.rs**
   - Создание CacheManager при старте
   - Передача в Player и Workers
   - Live update лимита из настроек

### ⚠️ Частично реализовано / Placeholder

1. **signal_preload() в Comp**
   - ✅ Инкрементирует epoch
   - ✅ Определяет стратегию Spiral/Forward
   - ❌ НЕ запускает background loading (нужен thread-safe Frame status)
   - Текущий код: debug логи + return

2. **Background Preload**
   - ✅ Workers с `execute_with_epoch()` для cancellable задач
   - ❌ Нет вызова workers в signal_preload (из-за RefCell<Comp>)
   - Проблема: Comp не Sync (RefCell), нельзя Arc<Comp> в workers

3. **Epoch механизм**
   - ✅ Shared `Arc<AtomicU64>` между CacheManager и Workers
   - ✅ `execute_with_epoch()` проверяет epoch перед выполнением
   - ❌ `increment_epoch()` не вызывается (signal_preload использует `current_epoch()`)

### ❌ Не реализовано / Отсутствует

1. **Frame Status System**
   - Нужен для thread-safe status transitions: Placeholder → Loading → Loaded
   - Текущий Frame имеет статус, но не атомарный

2. **Full Background Preload**
   - Spiral: 0, +1, -1, +2, -2, ...
   - Forward: center → end
   - Отмена при новом scrub (epoch mismatch)

3. **Дополнительные оптимизации**
   - Batch eviction (выбрасывать несколько фреймов за раз)
   - Predictive preload (учитывать direction)
   - Priority-based loading

## Баги и проблемы

### 🚨 Критические проблемы

1. **signal_preload не инкрементирует epoch**
   ```rust
   // В todo4: должен increment_epoch()
   // В коде: manager.current_epoch() - без инкремента
   ```
   **Последствие:** Старые запросы не отменяются при fast scrubbing

2. **Отсутствие background loading**
   - Frames загружаются только on-demand в `get_file_frame()`
   - Нет preload вокруг cursor
   - Timeline indicator всегда показывает "loaded" для видимых кадров

### ⚠️ Warnings при компиляции (24 warnings)

- Неиспользуемые методы: `increment_epoch`, `current_epoch`, `execute_with_epoch`
- Неиспользуемые поля: `texture_cache`, `selected_seq_idx`
- Неиспользуемые traits: `ProjectUI`, `TimelineUI`, etc.
- Неиспользуемые enum variants: `Play`, `Pause`, etc. в `AppEvent`

**Причина:** Код подготовлен для Phase 2, но не используется.

### 🔍 Потенциальные проблемы

1. **Memory estimation accuracy**
   - `frame.mem()` возвращает buffer size
   - Для HDR (f16/f32) может быть неточно из-за alignment
   - Не учитывает metadata overhead

2. **LRU cache size**
   - Fixed 10000 slots, unlimited virtual capacity
   - При 64MB/frame = 640GB virtual, но memory limit режет

3. **Serialization**
   - `#[serde(skip)]` для runtime полей (cache, cache_manager)
   - `default_cache()` для LruCache
   - Требует `set_cache_manager()` после десериализации

4. **RefCell vs RwLock**
   - Comp использует RefCell (single-threaded)
   - Хорошо для main thread, но блокирует background preload

## Оптимизации и улучшения

### ✅ Уже оптимизировано

1. **Memory-aware LRU**
   - Eviction только при превышении лимита
   - Освобождение памяти сразу

2. **Shared epoch counter**
   - Thread-safe без locks
   - Efficient cancellation

3. **Timeline indicator caching**
   - Не перерисовывается при каждом frame
   - Использует egui memory для cache

### 🚀 Возможные улучшения

1. **Batch eviction**
   ```rust
   // Вместо while loop - evict multiple frames at once
   let mut to_evict = Vec::new();
   while memory_over_limit && to_evict.len() < 10 {
       if let Some((_, frame)) = cache.pop_lru() {
           to_evict.push(frame);
       }
   }
   for frame in to_evict {
       manager.free_memory(frame.mem());
   }
   ```

2. **Predictive preload**
   - Учитывать playback direction
   - Preload больше в направлении движения

3. **Memory stats caching**
   - Cache usage/limit в status bar
   - Update раз в 100ms вместо каждого frame

4. **Frame size estimation**
   - Более точный учёт для compressed formats
   - Include metadata size

## Дедупликация

### ✅ Хорошо дедуплицировано

- CacheManager как single source of truth для памяти
- Shared Workers pool для всех background tasks
- Common FrameStatus enum для всех компонентов
- Unified preload strategies в CacheManager

### ⚠️ Возможная дедупликация

1. **Multiple status displays**
   - Status bar: memory usage
   - Timeline: frame statuses
   - Возможно объединить в один компонент

2. **Cache invalidation logic**
   - Повторяется в разных местах
   - Можно вынести в trait или helper

## Производительность

### 📊 Метрики

- **Compilation:** 2.78s dev build, warnings only
- **Memory:** Configurable limit (default 75% available - 2GB reserve)
- **Cache:** LRU 10000 slots, memory-bounded
- **Workers:** 3/4 CPU cores, work-stealing

### 🎯 Bottlenecks

1. **Single-threaded rendering**
   - Все Comp operations в main thread
   - RefCell blocks parallel access

2. **On-demand loading**
   - Нет preload → stuttering при fast scrub
   - IO blocking main thread

3. **Large EXR files**
   - 4K EXR ~64MB, slow to load/decompress
   - Memory pressure без preload

### 🚀 Performance improvements

1. **Implement Frame status system**
   - Thread-safe status transitions
   - Background loading без blocking UI

2. **Parallel composition**
   - GPU acceleration (уже есть GpuCompositor)
   - Multi-threaded layer blending

3. **Memory pooling**
   - Reuse frame buffers
   - Reduce allocations

## Рекомендации

### Phase 2: Background Preload

1. **Добавить Frame status system**
   ```rust
   pub enum FrameStatus {
       Placeholder,  // Green placeholder
       Loading,      // Yellow, async load in progress
       Loaded,       // Green, ready to display
       Error,        // Red, load failed
   }
   // Make atomic for thread-safe updates
   ```

2. **Refactor Comp for thread-safety**
   - Replace RefCell with RwLock or separate structures
   - Allow Arc<Comp> in workers

3. **Complete signal_preload**
   - Increment epoch
   - Launch background loading with Workers
   - Implement spiral/forward loops

### Code Quality

1. **Remove unused code**
   - Delete dead methods/traits/enums
   - Clean up warnings

2. **Add tests**
   - Unit tests for CacheManager
   - Integration tests for memory limits
   - Performance benchmarks

3. **Documentation**
   - Complete API docs for public methods
   - Architecture decision records

### UX Improvements

1. **Better memory feedback**
   - Progress bar for memory usage
   - Warnings when approaching limit

2. **Preload controls**
   - Manual preload button
   - Preload radius settings

3. **Error handling**
   - Better error messages for failed loads
   - Retry mechanisms

## Заключение

Проект в хорошем состоянии: core memory management реализован, timeline indicator работает, код компилируется без ошибок. Основная недоработка - отсутствие background preload из-за architectural constraints (RefCell vs thread-safety). 

**Рекомендация:** Завершить Phase 2 для полного functionality, затем добавить tests и polish.

**Приоритет:** High - исправить epoch increment в signal_preload, Medium - реализовать background loading, Low - cleanup warnings.