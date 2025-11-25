# План рефакторинга кэширования v7

> **Стратегия:** Агрессивная миграция без dual mode. Локальный `Comp::cache` - dead code, вырезаем нахрен сразу.

> **⚠️ ВАЖНО: Сборка проекта ТОЛЬКО через `start.cmd`!** Прямой `cargo build` не работает. См. BUILD.md

## Текущее состояние

### Архитектура кэша
**Comp (src/entities/comp.rs:143)**
```rust
cache: RefCell<LruCache<(u64, usize), Frame>>
```
- Каждый Comp имеет собственный LRU кэш
- Ключ: `(comp_hash, frame_idx)` где:
  - `comp_hash` = хэш от mode, file_mask, children, children_attrs
  - `frame_idx` = номер фрейма
- Методы: `cache_insert()`, `get_file_frame()`, `get_layer_frame()`

**Attrs (src/entities/attrs.rs:47)**
```rust
pub struct Attrs {
    map: HashMap<String, AttrValue>,
}
```
- Простой HashMap без dirty флага
- Методы: `set()`, `get()`, `hash_all()`

**CacheManager (src/cache_man.rs:27)**
```rust
pub struct CacheManager {
    memory_usage: Arc<AtomicUsize>,
    max_memory_bytes: usize,
    current_epoch: Arc<AtomicU64>,
}
```
- Глобальный трекер памяти
- Epoch для отмены stale requests
- Методы: `add_memory()`, `free_memory()`, `check_memory_limit()`

**PlayaApp (src/main.rs:91-95)**
```rust
cache_manager: Arc<CacheManager>,
workers: Arc<Workers>,
```
- Владеет глобальным CacheManager
- Раздает Arc<CacheManager> каждому Comp

### Проблемы текущей реализации

1. **Фрагментированный кэш**
   - Каждый Comp имеет свой LruCache
   - Нет единого view на все закэшированные фреймы
   - Сложно реализовать глобальные стратегии eviction

2. **Нет dirty tracking**
   - Attrs не знает когда его изменили
   - Приходится пересчитывать comp_hash каждый раз
   - Невозможно инвалидировать кэш по изменению атрибутов

3. **Дублирование кэша**
   - Если два Comp'а ссылаются на один File comp - кэш дублируется
   - Waste памяти

---

## Целевая архитектура

### Структуры

#### 1. GlobalFrameCache (новый файл src/global_cache.rs)
```rust
pub struct GlobalFrameCache {
    /// Глобальный LRU кэш: (comp_uuid, frame) -> Frame
    cache: Arc<Mutex<LruCache<(String, i32), Frame>>>,
    /// Менеджер памяти
    cache_manager: Arc<CacheManager>,
    /// Стратегия кэширования
    strategy: CacheStrategy,
}

pub enum CacheStrategy {
    /// Кэшировать только последний фрейм (минимальная память)
    LastOnly,
    /// Кэшировать все фреймы в work area (макс производительность)
    All,
}

impl GlobalFrameCache {
    pub fn new(manager: Arc<CacheManager>, strategy: CacheStrategy) -> Self;

    /// Получить фрейм из кэша
    pub fn get(&self, comp_uuid: &str, frame: i32) -> Option<Frame>;

    /// Вставить фрейм с LRU eviction
    pub fn insert(&self, comp_uuid: &str, frame: i32, frame_data: Frame);

    /// Очистить кэш для конкретного comp
    pub fn clear_comp(&self, comp_uuid: &str);

    /// Очистить весь кэш
    pub fn clear_all(&self);
}
```

#### 2. Attrs с dirty tracking (src/entities/attrs.rs)
```rust
pub struct Attrs {
    map: HashMap<String, AttrValue>,
    dirty: bool,  // <-- НОВОЕ
}

impl Attrs {
    pub fn set(&mut self, key: impl Into<String>, value: AttrValue) {
        self.map.insert(key.into(), value);
        self.dirty = true;  // <-- Помечаем как грязный
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}
```

#### 3. Comp без локального кэша (src/entities/comp.rs)
```rust
pub struct Comp {
    pub uuid: String,
    pub attrs: Attrs,
    // ...

    // УДАЛЯЕМ:
    // cache: RefCell<LruCache<(u64, usize), Frame>>,

    // ДОБАВЛЯЕМ:
    /// Ссылка на глобальный кэш (опционально, для обратной совместимости)
    #[serde(skip)]
    global_cache: Option<Arc<GlobalFrameCache>>,
}

impl Comp {
    pub fn get_frame(&self, frame_idx: i32, project: &Project) -> Option<Frame> {
        // 1. Вычислить ключ: (self.uuid, frame_idx)
        let cache_key = (self.uuid.clone(), frame_idx);

        // 2. Проверить dirty или отсутствие в кэше
        let needs_recompose = self.attrs.is_dirty()
            || !project.global_cache.contains(&cache_key);

        if needs_recompose {
            // 3. Compose/load фрейм
            let frame = match self.mode {
                CompMode::File => self.compose_file_frame(frame_idx),
                CompMode::Layer => self.compose_layer_frame(frame_idx, project),
            }?;

            // 4. Вставить в глобальный кэш
            project.global_cache.insert(&self.uuid, frame_idx, frame.clone());

            // 5. Очистить dirty флаг
            self.attrs.clear_dirty();

            Some(frame)
        } else {
            // 6. Вернуть из кэша
            project.global_cache.get(&self.uuid, frame_idx)
        }
    }
}
```

#### 4. Project с глобальным кэшем (src/entities/project.rs)
```rust
pub struct Project {
    pub media: HashMap<String, Comp>,
    // ...

    /// Глобальный кэш фреймов
    #[serde(skip)]
    pub global_cache: Arc<GlobalFrameCache>,
}
```

---

## План миграции

### Фаза 1: Создание инфраструктуры + вырезание старого кэша

**1.1. Создать GlobalFrameCache** (новый файл) ✅ DONE
- [x] Создать src/global_cache.rs
- [x] Реализовать GlobalFrameCache с LRU
- [x] Реализовать CacheStrategy::LastOnly и ::All
- [x] Добавить тесты
- [x] Добавить #[derive(Debug)]

**1.2. Добавить dirty tracking в Attrs** ✅ DONE
- [x] Добавить поле `dirty: bool` в Attrs
- [x] Обновить `set()` для установки dirty
- [x] Добавить `is_dirty()`, `clear_dirty()`, `mark_dirty()`
- [x] Убедиться что serde skip dirty (не сохранять в JSON)

**1.3. Подключить GlobalFrameCache к Project** ✅ DONE
- [x] Добавить поле `global_cache: Arc<GlobalFrameCache>` в Project
- [x] Инициализировать в Project::new()
- [x] Передать через rebuild_with_manager()
- [x] Добавить import в project.rs

**1.4. ВЫРЕЗАТЬ старый локальный кэш из Comp (src/entities/comp.rs)** ✅ DONE
- [x] Удалить поле `cache: RefCell<LruCache>` (строка 143)
- [x] Удалить инициализацию в new() (строка 192)
- [x] Удалить методы:
  - [x] `cache_insert()` (строка 730-757)
  - [x] `clear_cache()` (строка 515-520)
  - [x] `invalidate()` (строка 1711) - заменено на dirty
  - [x] `cache_frame_statuses()` (строка 491) - переписано на возврат всех Header
- [x] Удалить `compute_comp_hash()` (строка 760) - заменено на dirty tracking
- [x] Очистить imports: `lru::LruCache`, `NonZeroUsize`, `RefCell`
- [x] Удалить использование cache в:
  - [x] `enqueue_load()` (строка 669-714) - preloading disabled (TODO)
  - [x] `get_file_frame()` (строка 893) - использует global_cache
  - [x] `get_layer_frame()` (строка 922) - использует global_cache
- [x] Обновить тесты (строки 2088, 2104, 2192)
- [x] Заменить все clear_cache() на attrs.mark_dirty()

**1.5. Переписать get_frame() на global_cache** ✅ DONE
- [x] Обновить `get_file_frame()` для использования global_cache
- [x] Обновить `get_layer_frame()` для использования global_cache
- [x] Использовать `attrs.is_dirty()` вместо hash comparison
- [x] Обновить `enqueue_load()` - отключен preloading (TODO для будущего)
- [x] Все тесты проходят успешно (24 passed, 0 failed)

### Фаза 2: Оптимизация

**2.1. Реализовать стратегии кэширования**
- [ ] Добавить настройку в AppSettings
- [ ] Реализовать переключение между LastOnly и All
- [ ] UI для выбора стратегии в Settings

**2.2. Оптимизировать memory tracking**
- [ ] Точный подсчет памяти фреймов
- [ ] Корректный LRU eviction при достижении лимита
- [ ] Логирование eviction events

**2.3. Nested comps optimization**
- [ ] Убедиться что child comps не дублируют кэш
- [ ] Реализовать cascade invalidation при dirty

---

## Детали реализации

### Ключи кэша

**Было:**
```rust
(comp_hash: u64, frame_idx: usize)
```
- comp_hash менялся при изменении children_attrs
- Приходилось пересчитывать каждый раз

**Будет:**
```rust
(comp_uuid: String, frame: i32)
```
- Стабильный ключ
- Инвалидация через dirty флаг

### Invalidation strategy

**File mode Comp:**
1. attrs.set() -> dirty = true
2. get_frame() -> проверяет dirty
3. Если dirty -> загружает заново -> очищает dirty

**Layer mode Comp:**
1. children_attrs[uuid].set() -> child dirty = true
2. get_frame() -> проверяет свой dirty + детей dirty
3. Если кто-то dirty -> рекомпозит -> очищает все dirty

### Memory management

**CacheManager остаётся без изменений:**
- Отслеживает общую память
- Epoch для workers
- LRU eviction делает GlobalFrameCache

**GlobalFrameCache LRU:**
```rust
while cache_manager.check_memory_limit() {
    if let Some((_key, evicted_frame)) = cache.pop_lru() {
        let size = evicted_frame.mem();
        cache_manager.free_memory(size);
    } else {
        break; // Cache empty
    }
}
```

---

## Риски и митигация

### Риск 1: Breaking changes в Project serialization
**Митигация:**
- global_cache помечен #[serde(skip)]
- При десериализации пересоздаётся в rebuild_with_manager()

### Риск 2: Race conditions в global_cache
**Митигация:**
- Использовать Arc<Mutex<LruCache>>
- Минимизировать время удержания lock
- Возможно использовать DashMap для concurrent access

### Риск 3: Производительность при большом количестве comps
**Митигация:**
- Ключ (String, i32) эффективнее (u64, usize)
- LRU cache быстрый O(1) для get/insert
- Можно добавить метрики для мониторинга

### Риск 4: Дублирование фреймов при nested comps
**Митигация:**
- Использовать comp_uuid в ключе - уникальный для каждого comp
- Если нужна дедупликация - использовать content hash

---

## Метрики успеха

1. **Память:**
   - Снижение использования памяти на 20-30% при nested comps
   - Более предсказуемый memory footprint

2. **Производительность:**
   - Не хуже текущей на простых сценариях
   - Лучше на nested comps (нет дублирования)

3. **Maintainability:**
   - Проще понять где лежит кэш
   - Проще отследить invalidation

---

## Следующие шаги

1. Создать GlobalFrameCache (src/global_cache.rs)
2. Добавить dirty tracking в Attrs
3. Подключить GlobalFrameCache к Project
4. ВЫРЕЗАТЬ весь локальный кэш из Comp (dead code)
5. Переписать get_frame() на global_cache + dirty tracking
6. Тестировать
7. Оптимизация (стратегии, memory tracking)

---

## Альтернативные подходы (рассмотрены и отклонены)

### Вариант A: Content-based cache key
```rust
(content_hash: u64, frame: i32)
```
**Плюсы:** Автоматическая дедупликация
**Минусы:** Дорого считать hash при каждом get_frame()

### Вариант B: Оставить локальные кэши + добавить global index
**Плюсы:** Меньше breaking changes
**Минусы:** Сложность, двойное управление памятью

### Вариант C: Database-like cache с SQL запросами
**Плюсы:** Гибкость
**Минусы:** Overkill для нашего use case

---

## Status Update (2025-11-24)

### ✅ Фаза 1 ЗАВЕРШЕНА

**Все задачи выполнены:**
- [x] GlobalFrameCache создан и протестирован
- [x] Dirty tracking добавлен в Attrs
- [x] GlobalFrameCache подключен к Project
- [x] Локальный Comp::cache полностью вырезан
- [x] get_frame() методы переписаны на global_cache
- [x] Все тесты обновлены и проходят (24 passed, 0 failed)

**Измененные файлы:**
1. `src/global_cache.rs` - НОВЫЙ ФАЙЛ с GlobalFrameCache
2. `src/entities/attrs.rs` - добавлен dirty tracking
3. `src/entities/project.rs` - добавлен global_cache
4. `src/entities/comp.rs` - вырезан локальный cache, переписан get_frame()
5. `src/main.rs` - заменен clear_cache() на attrs.mark_dirty()
6. `src/widgets/timeline/timeline_ui.rs` - удален clear_cache()
7. `src/dialogs/encode/encode.rs` - исправлен тест (добавлен CacheManager)

**Архитектурные изменения:**
- Ключи кэша: `(comp_hash, frame_idx)` → `(comp_uuid: String, frame_idx: i32)`
- Инвалидация: hash comparison → dirty flag tracking
- Владение кэшем: per-Comp локальный → Project-level глобальный
- Preloading: временно отключен (помечен как TODO)

**Результаты тестов:**
```
running 24 tests
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured
```

**Следующие шаги (Фаза 2 - опционально):**
- Реализовать стратегии кэширования (LastOnly vs All) с UI настройкой
- Re-enable preloading с GlobalFrameCache
- Оптимизировать memory tracking для LRU eviction
- Cascade invalidation для nested comps
