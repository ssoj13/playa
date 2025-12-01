# PLAYA Architecture Review - Claude Opus Assessment

## Executive Summary

Изучил arch.md (целевая архитектура), clod1.md/qwn.md (предыдущие анализы) и исходный код.
Текущая реализация **работоспособна**, но есть **архитектурная непоследовательность** которая создаёт технический долг.

---

## 1. Attrs System - ЧАСТИЧНО РЕАЛИЗОВАНО

### Текущее состояние (attrs.rs):
```rust
pub struct Attrs {
    map: HashMap<String, AttrValue>,
    dirty: AtomicBool,  // ✓ Thread-safe dirty tracking
}
```

**Что работает:**
- `dirty: AtomicBool` - атомарный флаг изменений ✓
- `hash_all()` / `hash_filtered()` - хэширование для cache invalidation ✓
- `remove()` (в arch.md назван `del`) ✓
- Thread-safe через AtomicBool ✓

**Чего не хватает согласно arch.md:**
- `AttrValue::Json(String)` - для вложенных структур (HashMap, Vec)
- `Arc<Atomic*>` типы - для thread-safe shared primitives
- Текущие типы ограничены: Bool, Str, Int, UInt, Float, Vec3/4, Mat3/4

### Оценка: 7/10
Базовая функциональность есть. Json тип критичен для сериализации вложенных children.

---

## 2. Comp Structure - ХОРОШО РЕАЛИЗОВАНО

### Текущее (comp.rs):
```rust
pub struct Comp {
    pub uuid: Uuid,
    pub mode: CompMode,           // Layer | File - dual-mode ✓
    pub attrs: Attrs,             // Persistent attributes ✓
    pub children: Vec<Uuid>,      // Instance UUIDs
    pub children_attrs: HashMap<Uuid, Attrs>,  // Per-child attrs
    // ...
}
```

**Что работает хорошо:**
- Dual-mode (File/Layer) - элегантное решение ✓
- Рекурсивная композиция через `compose()` ✓
- Thread-local CPU compositor для background threads ✓
- Global cache integration ✓
- Work area (play_start/play_end) ✓

**Расхождения с arch.md:**
| arch.md | Текущий код | Комментарий |
|---------|-------------|-------------|
| `in/out` | `start/end` | Naming difference |
| `trim_in/trim_out` | `play_start/play_end` | Naming difference |
| `Vec<Tuple(uuid, attrs)>` | `Vec<Uuid>` + `HashMap<Uuid, Attrs>` | HashMap гибче |
| `comp2local()` / `local2comp()` | inline в `compose()` | Нужно выделить |

**Что добавить:**
```rust
// Явные методы конверсии времени (сейчас интегрированы в compose)
pub fn comp2local(&self, child_uuid: Uuid, comp_frame: i32) -> i32
pub fn local2comp(&self, child_uuid: Uuid, local_frame: i32) -> i32
```

### Оценка: 8/10
Хорошая реализация. Нужно выделить time conversion methods.

---

## 3. Project Structure - ПРОБЛЕМНОЕ МЕСТО

### Текущее (project.rs):
```rust
pub struct Project {
    pub attrs: Attrs,                                    // Минимально используется!
    pub media: Arc<RwLock<HashMap<Uuid, Comp>>>,         // Direct field
    pub comps_order: Vec<Uuid>,                          // Direct field  
    pub selection: Vec<Uuid>,                            // Direct field
    pub active: Option<Uuid>,                            // Direct field
    pub compositor: RefCell<CompositorType>,             // Direct field
    // ...
}
```

**ПРОБЛЕМА:** arch.md требует ВСЕ поля в `Project.attr`, но текущий код хранит их напрямую.

### arch.md Target:
```rust
struct Project {
    attr: Attrs {
        media: Arc<RwLock<HashMap<Uuid, Comp>>>,
        order: Vec<Uuid>,
        selection: Vec<Uuid>,
        active: Option<Uuid>,
        // ...
    }
}
```

**Последствия текущего подхода:**
- Сериализация работает через serde, не через Attrs
- Нет единой системы dirty tracking для Project
- Две параллельные системы состояния

### Рекомендация:
Либо:
1. Мигрировать ВСЕ в Attrs (требует AttrValue::Json и расширения типов)
2. Задокументировать что Project использует serde напрямую (компромисс)

### Оценка: 5/10
Непоследовательность с arch.md. Работает, но архитектурный долг.

---

## 4. Player Structure - АНАЛОГИЧНАЯ ПРОБЛЕМА

### Текущее (player.rs):
```rust
pub struct Player {
    pub project: Project,         // OWNS Project (arch.md хочет &Project)
    pub active_comp: Option<Uuid>,
    pub is_playing: bool,
    pub fps_base: f32,
    // ... все поля напрямую
}
```

**Нет `attrs: Attrs`** - всё напрямую.

**Проблема ownership:** Player владеет Project целиком. arch.md предлагает ссылку.
Текущий подход работает, но создаёт copy-on-modify паттерн (`modify_active_comp`).

### Оценка: 6/10
JKL работает отлично. Архитектура ownership спорная.

---

## 5. EventBus - ХОРОШО РЕАЛИЗОВАНО

### Текущее (events.rs):
```rust
pub enum AppEvent { /* 50+ вариантов */ }
pub enum CompEvent { CurrentFrameChanged, LayersChanged, TimelineChanged, AttrsChanged }

pub struct EventBus {
    tx: crossbeam::channel::Sender<AppEvent>,
    rx: crossbeam::channel::Receiver<AppEvent>,
}
```

**Что работает:**
- Crossbeam unbounded channels ✓
- Non-blocking send/receive ✓
- Drain для batch processing ✓
- CompEventSender для comp-level events ✓

**arch.md хочет больше типов:**
- `ProjectEvent`, `PlayEvent`, `IOEvent`, `KeyEvent`, `MouseEvent`

**Моя оценка:** Текущий единый `AppEvent` проще и достаточен. Разбиение добавит complexity без явной выгоды.

### Оценка: 9/10
Отлично работает. Не надо разбивать на много типов.

---

## 6. Main App (PlayaApp) - МОНОЛИТ

### Текущее (main.rs):
```rust
struct PlayaApp {
    player: Player,
    viewport_renderer: Arc<Mutex<ViewportRenderer>>,
    viewport_state: ViewportState,
    timeline_state: TimelineState,
    settings: AppSettings,
    project: Project,  // ДУБЛИКАТ! Player тоже имеет project
    cache_manager: Arc<CacheManager>,
    workers: Arc<Workers>,
    event_bus: EventBus,
    // ... ещё 20+ полей
}
```

**КРИТИЧЕСКАЯ ПРОБЛЕМА:** Два Project:
1. `PlayaApp.project` - persisted
2. `PlayaApp.player.project` - runtime

Это создаёт sync issues и путаницу.

**arch.md предлагает:**
```
PlayaApp
├── Player (playback engine)
├── EventBus (async events)
├── Attrs (global state)
├── Workers (thread pool)
├── GlobalFrameCache
├── Shaders
├── TimelineState
└── ViewportState
```

### Рекомендация:
Убрать дублирование Project. Один источник истины.

### Оценка: 4/10
Монолит с дублированием. Главная архитектурная проблема.

---

## 7. Node Trait - НЕ РЕАЛИЗОВАНО

arch.md предлагает:
```rust
trait Node {
    fn attr(&self) -> &Attrs;
    fn data(&self) -> &Attrs;  // transient runtime data
    fn compute(&self, ctx: &Context);
}
```

**Текущее состояние:** Нет такого trait. Comp/Project/Player не унифицированы.

**Моя оценка:** Это может быть over-engineering. Comp уже работает, добавление trait усложнит код без явной выгоды.

### Рекомендация: SKIP
Не реализовывать Node trait. Сфокусироваться на реальных проблемах.

---

## 8. Cache System - ХОРОШО

### Текущее:
```rust
// global_cache.rs
pub struct GlobalFrameCache {
    cache: RwLock<LruCache<(Uuid, i32), Frame>>,
    manager: Arc<CacheManager>,
    strategy: CacheStrategy,
}
```

**Работает:**
- LRU eviction ✓
- Epoch-based cancellation ✓
- Memory tracking ✓
- `contains()` / `get()` / `insert()` / `clear_comp()` ✓

**arch.md хочет:**
```
HashMap<UUID:[Frames]> для trivial removal
```

**Моя оценка:** Текущий LRU с composite key `(Uuid, i32)` достаточен.
`clear_comp()` итерирует, но это O(n) операция редко вызываемая.

### Оценка: 8/10
Работает хорошо. Не нужно менять на nested HashMap.

---

## 9. Timeline / Drag-n-Drop - РАБОТАЕТ

Текущее использует `GlobalDragState` в egui temp storage.
arch.md хочет hit-test registry с расстоянием до элементов.

**Моя оценка:** Текущий подход работает. Можно улучшить, но не критично.

### Оценка: 7/10

---

## Priority Matrix

### CRITICAL (делать первым):
1. **Убрать дублирование Project** в PlayaApp/Player
2. **Добавить AttrValue::Json** для вложенных структур

### HIGH (важно, но не блокирует):
3. Выделить `comp2local()` / `local2comp()` как явные методы
4. Документировать архитектурные решения (почему HashMap вместо Vec)

### MEDIUM (улучшения):
5. Рассмотреть миграцию Project полей в Attrs
6. Timeline hit-test improvements

### LOW (nice to have):
7. Node trait (вероятно не нужен)
8. Разбиение AppEvent на подтипы (вероятно не нужно)
9. ~~Nested HashMap для cache~~ DONE

---

## Архитектурные Принципы (подтверждаю из arch.md)

1. **Attrs как универсальная сериализация** - ДА, но не до фанатизма
2. **EventBus для decoupling** - ДА, работает отлично
3. **Dual-mode Comp** - ОТЛИЧНОЕ решение
4. **Global cache с epoch** - РАБОТАЕТ
5. **Workers thread pool** - РАБОТАЕТ

---

## Итоговая Оценка

| Component | Score | Status |
|-----------|-------|--------|
| Attrs | 7/10 | Needs Json type |
| Comp | 8/10 | Good, add time methods |
| Project | 5/10 | Inconsistent with arch |
| Player | 6/10 | Works, ownership issues |
| EventBus | 9/10 | Excellent |
| PlayaApp | 4/10 | Monolith, duplication |
| Cache | 9/10 | Refactored to nested HashMap |
| Timeline | 7/10 | Works |

**Overall: 6.75/10**

Приложение работает, но есть архитектурный долг который будет мешать развитию.
Главная проблема - дублирование Project и непоследовательное использование Attrs.

---

## Конкретный План Действий

### Phase 1: Убрать дублирование (2-3 дня)
1. Решить кто владеет Project: PlayaApp или Player
2. Убрать дублирование, сделать один источник истины
3. Обновить все references

### Phase 2: Attrs расширение (1-2 дня)
1. Добавить `AttrValue::Json(String)`
2. Добавить helper методы `get_json<T>()` / `set_json<T>()`
3. Рассмотреть `AttrValue::List(Vec<AttrValue>)`

### Phase 3: Comp time methods (1 день)
1. Выделить `comp2local()` / `local2comp()` как public методы
2. Документировать time conversion logic

### Phase 4: Documentation (ongoing)
1. Документировать почему HashMap вместо Vec для children_attrs
2. Документировать ownership модель
3. Обновить arch.md с реальным состоянием

---

## CHANGELOG

### 2025-11-30: Cache Refactored

Переделал `global_cache.rs` с LRU на nested HashMap:

```rust
// БЫЛО:
LruCache<(Uuid, i32), Frame>

// СТАЛО:
HashMap<Uuid, HashMap<i32, Frame>>  // nested structure
VecDeque<CacheKey>                   // LRU order tracking
```

**Изменения:**
- `clear_comp()` теперь O(1) на HashMap level (было O(n))
- Добавлены `comp_count()`, `comp_frame_count(uuid)`
- Убрана зависимость `lru` из Cargo.toml
- LRU eviction сохранён через VecDeque

---

*Report generated by Claude Opus 4.5, 2025-11-30*
