# Session 7: Viewport не обновляется при изменении слоёв

## Дата: 2025-11-21

## Обзор

Анализ проблемы: viewport не обновляется при перемещении/изменении слоёв в timeline, хотя текущий кадр остаётся тем же.

---

## ❌ ПРОБЛЕМА: Viewport не перезагружает текстуру при LayersChanged

### Симптомы
При перемещении слоя в timeline (drag), изменении trim points, или любом изменении атрибутов слоя - viewport продолжает показывать старый композит.

### Причина

**Viewport загружает текстуру только при смене current_frame:**

`src/main.rs:1286`:
```rust
let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame());
```

**Когда слой перемещается:**
1. ✅ `move_child()` изменяет атрибуты start/end в `children_attrs`
2. ✅ `clear_cache()` вызывается - кеш композита очищается
3. ✅ `compute_comp_hash()` возвращает **новый хэш** (start/end хэшируются правильно!)
4. ✅ `LayersChanged` событие эмитится
5. ❌ **НО обработчик LayersChanged ПУСТОЙ** (main.rs:481-484)
6. ❌ `self.displayed_frame` остаётся прежним
7. ❌ `texture_needs_upload = false` → viewport НЕ обновляется

### Proof of Concept

```rust
// User перемещает слой в timeline
comp.move_child(0, 50); // start: 10→50
comp.clear_cache();     // Кеш очищен
comp.event_sender.emit(CompEvent::LayersChanged { comp_uuid });

// В main.rs обработчик LayersChanged:
events::CompEvent::LayersChanged { comp_uuid } => {
    debug!("Comp {} layers changed", comp_uuid);
    // НИЧЕГО НЕ ДЕЛАЕТ! ❌
}

// Viewport рендерится каждый кадр UI (60 FPS):
let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame());
// displayed_frame = Some(75), current_frame = 75
// → texture_needs_upload = false ❌
// → Viewport показывает СТАРЫЙ композит!
```

---

## ✅ РЕШЕНИЕ: Сбросить displayed_frame при LayersChanged

**Файл:** `src/main.rs`

**Функция:** `handle_comp_events()` (строка ~481)

**Изменение:**

```rust
events::CompEvent::LayersChanged { comp_uuid } => {
    debug!("Comp {} layers changed", comp_uuid);
    // Force viewport texture re-upload since composition changed
    self.displayed_frame = None;
    // Future: invalidate timeline cache, rebuild layer UI
}
```

### Как это работает

1. Слой перемещается → `LayersChanged` эмитится
2. Обработчик устанавливает `self.displayed_frame = None`
3. При следующем рендере viewport:
   ```rust
   let texture_needs_upload = self.displayed_frame != Some(self.player.current_frame());
   // displayed_frame = None, current_frame = 75
   // → texture_needs_upload = true ✅
   ```
4. `self.frame = self.player.get_current_frame()` вызывается
5. `comp.get_frame(75, project)` вычисляет **новый хэш** (с обновлённым start/end)
6. Кеш по старому хэшу пуст → `compose()` вызывается
7. Новый композит создаётся и кешируется
8. Viewport загружает новую текстуру и отображает правильный результат

---

## ✅ Что работает правильно

### 1. Хэширование атрибутов

**НЕТ проблемы с хэшированием!** Все атрибуты **уже хэшируются** в `compute_comp_hash()`:

`src/entities/comp.rs:317-328`:
```rust
if let Some(attrs) = self.children_attrs.get(child_uuid) {
    attrs.get_u32("start").unwrap_or(0).hash(&mut hasher);     // ✅
    attrs.get_u32("end").unwrap_or(0).hash(&mut hasher);       // ✅
    attrs.get_i32("play_start").unwrap_or(0).hash(&mut hasher); // ✅
    attrs.get_i32("play_end").unwrap_or(0).hash(&mut hasher);   // ✅
    attrs.get_bool("visible").unwrap_or(true).hash(&mut hasher);
    let opacity_bits = attrs.get_float("opacity").unwrap_or(1.0).to_bits();
    opacity_bits.hash(&mut hasher);
    if let Some(blend) = attrs.get_str("blend_mode") {
        blend.hash(&mut hasher);
    }
    let speed_bits = attrs.get_float("speed").unwrap_or(1.0).to_bits();
    speed_bits.hash(&mut hasher);
}
```

### 2. Cache Invalidation

При изменении слоёв **вызывается** очистка кеша:

```rust
// src/entities/comp.rs

pub fn move_child(&mut self, child_idx: usize, new_start: i32) {
    // ... update attrs ...
    self.clear_cache(); // ✅ Вызывается
    self.event_sender.emit(CompEvent::LayersChanged);
}

pub fn set_child_play_start(&mut self, child_idx: usize, new_play_start: i32) {
    // ... update attrs ...
    self.clear_cache(); // ✅ Вызывается
    self.event_sender.emit(CompEvent::LayersChanged);
}
```

### 3. Event Flow

**События корректно эмитятся и обрабатываются:**

```
User перемещает слой → MoveLayer event
  ↓
Comp::move_child() - изменяет start/end атрибуты
  ↓
Comp::clear_cache() - очищает HashMap кеша
  ↓
CompEvent::LayersChanged эмитится
  ↓
Main::handle_comp_event() ТЕПЕРЬ сбрасывает displayed_frame ✅
  ↓
Viewport::render() → texture_needs_upload = true
  ↓
Comp::get_frame() → новый хэш → cache miss → compose() вызывается
  ↓
Новый композит отображается ✅
```

---

## Архитектурные заметки

### Почему не вызывать get_frame() напрямую в LayersChanged?

**Вариант 1 (ВЫБРАН):** Сброс displayed_frame
```rust
events::CompEvent::LayersChanged { .. } => {
    self.displayed_frame = None;  // Форсируем перезагрузку при следующем рендере
}
```

**Преимущества:**
- Простота: одна строка кода
- Безопасность: не создаёт лишних зависимостей
- Эффективность: viewport сам решает когда перезагружать
- Соответствует immediate mode GUI паттерну egui

**Вариант 2 (НЕ ВЫБРАН):** Прямой вызов get_frame()
```rust
events::CompEvent::LayersChanged { comp_uuid } => {
    self.frame = self.player.get_current_frame();
    self.displayed_frame = Some(self.player.current_frame());
}
```

**Проблемы:**
- Дублирование логики (та же логика есть в render_viewport_tab)
- Потенциальная рассинхронизация
- Нарушает принцип единственной ответственности

### Cache Key Structure

Кеширование работает правильно:

```rust
type CacheKey = (u64, usize); // (comp_hash, frame_idx)
type FrameCache = HashMap<CacheKey, Frame>;
```

**Пример работы:**
```
// До перемещения слоя:
hash_before = compute_comp_hash() // = 0xABCD1234 (start=10, end=100)
cache[(0xABCD1234, 75)] = Frame { /* старый композит */ }

// После перемещения слоя:
comp.move_child(0, 50) // start: 10→50, end: 100→140
hash_after = compute_comp_hash()  // = 0xDEADBEEF (start=50, end=140) ✅ НОВЫЙ!

// При get_frame(75):
cache_key = (0xDEADBEEF, 75)  // НОВЫЙ ключ!
cache.get(cache_key) → None    // Cache MISS
compose(75) → создаёт новый композит ✅
cache[(0xDEADBEEF, 75)] = Frame { /* новый композит */ }
```

---

## Связанные файлы

### Изменения применены в:
- `src/main.rs:481-486` - обработчик LayersChanged (добавлен сброс displayed_frame)

### Зависимости (без изменений):
- `src/entities/comp.rs:317-328` - хэширование атрибутов (уже работает правильно)
- `src/entities/comp.rs:686,714,742,754,764,815` - эмиты LayersChanged
- `src/events.rs:209` - определение CompEvent::LayersChanged
- `src/main.rs:1286-1292` - логика texture_needs_upload
- `src/widgets/viewport/viewport_ui.rs:101-113` - проверка needs_upload

---

## Тестирование

### Ручное тестирование

- [x] Перемещение слоя в timeline → viewport обновляется
- [x] Trim слоя (левый/правый край) → viewport обновляется
- [x] Изменение opacity → viewport обновляется
- [x] Playback → композиты кешируются и переиспользуются корректно

### Результаты

**РАБОТАЕТ!** Viewport теперь корректно обновляется при любых изменениях слоёв.

---

## Следующие шаги

- [x] ~~Применить изменения в `compute_comp_hash()`~~ (НЕ ТРЕБУЕТСЯ - уже работает!)
- [x] Применить изменения в обработчике `LayersChanged`
- [x] Собрать проект через START.CMD
- [x] Протестировать перемещение/trim слоёв

### Время сессии
~60 минут (диагностика, исправление ошибочного анализа, правильное решение)
