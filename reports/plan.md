# PLAYA Refactoring Plan

## Overview

Три основных изменения для улучшения архитектуры:
1. Убрать дублирование Project
2. Добавить AttrValue::Json
3. Добавить comp2local/local2comp методы

---

## Phase 1: Убрать дублирование Project

### Проблема
Сейчас Project существует в двух местах:
- `PlayaApp.project` - сериализуется
- `PlayaApp.player.project` - runtime

Это создаёт sync issues и путаницу.

### Выбранное решение: Вариант A

**PlayaApp владеет Project, Player получает `&mut Project` при вызовах.**

Почему этот вариант:
- Явные зависимости через сигнатуры методов
- Нет overhead от RwLock/Mutex
- Borrow checker помогает находить ошибки
- Сериализация остаётся простой (PlayaApp.project)
- Чистый Rust-идиоматичный подход

Рассмотренные альтернативы:
- ~~B: Player владеет~~ - сложная сериализация
- ~~C: Arc<RwLock>~~ - overhead, скрытые зависимости
- ~~D: Custom serialization~~ - сложно
- ~~E: Global singleton~~ - не идиоматично для Rust

### Изменения

#### 1.1 player.rs - убрать ownership

```rust
// БЫЛО:
pub struct Player {
    pub project: Project,  // owns
    pub active_comp: Option<Uuid>,
    // ...
}

// СТАНЕТ:
pub struct Player {
    pub active_comp: Option<Uuid>,
    pub is_playing: bool,
    pub fps_base: f32,
    pub fps_play: f32,
    pub loop_enabled: bool,
    pub play_direction: f32,
    pub last_frame_time: Option<Instant>,
}
```

#### 1.2 player.rs - изменить методы

Все методы которые используют `self.project` получат `project: &mut Project`:

```rust
// БЫЛО:
impl Player {
    pub fn get_current_frame(&mut self) -> Option<Frame> {
        let comp = self.project.get_comp(self.active_comp?)?;
        // ...
    }
}

// СТАНЕТ:
impl Player {
    pub fn get_current_frame(&self, project: &Project) -> Option<Frame> {
        let comp = project.get_comp(self.active_comp?)?;
        // ...
    }

    pub fn set_frame(&mut self, frame: i32, project: &mut Project) {
        // ...
    }
}
```

#### 1.3 main.rs - обновить вызовы

```rust
// БЫЛО:
self.player.get_current_frame()

// СТАНЕТ:
self.player.get_current_frame(&self.project)
```

#### 1.4 Список методов Player для изменения:

| Метод | Сигнатура |
|-------|-----------|
| `new()` | убрать project из параметров |
| `get_current_frame()` | `(&self, project: &Project)` |
| `set_active_comp()` | `(&mut self, uuid, project: &mut Project)` |
| `set_frame()` | `(&mut self, frame, project: &mut Project)` |
| `play_range()` | `(&self, project: &Project)` |
| `set_play_range()` | `(&mut self, start, end, project: &mut Project)` |
| `total_frames()` | `(&self, project: &Project)` |
| `current_frame()` | `(&self, project: &Project)` |
| `update()` | `(&mut self, project: &mut Project)` |
| `advance_frame()` | `(&mut self, project: &mut Project)` |
| `to_start()` | `(&mut self, project: &mut Project)` |
| `to_end()` | `(&mut self, project: &mut Project)` |
| `step()` | `(&mut self, count, project: &mut Project)` |

#### 1.5 Удалить helper методы в Player:

- `active_comp()` - заменить на прямой вызов `project.get_comp()`
- `modify_active_comp()` - заменить на `project.set_comp()`

---

## Phase 2: AttrValue::Json

### Проблема
Нет способа хранить вложенные структуры (HashMap, Vec) в Attrs.

### Решение
Добавить `AttrValue::Json(String)` + helper методы.

### Изменения

#### 2.1 attrs.rs - добавить вариант

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int(i32),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    Json(String),  // NEW: JSON-encoded nested data
}
```

#### 2.2 attrs.rs - добавить Hash для Json

```rust
impl std::hash::Hash for AttrValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // ...existing code...
        Json(v) => v.hash(state),
    }
}
```

#### 2.3 attrs.rs - добавить helper методы

```rust
impl Attrs {
    /// Get JSON value and deserialize
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        match self.map.get(key) {
            Some(AttrValue::Json(s)) => serde_json::from_str(s).ok(),
            _ => None,
        }
    }

    /// Serialize and set JSON value
    pub fn set_json<T: serde::Serialize>(&mut self, key: impl Into<String>, value: &T) {
        if let Ok(json) = serde_json::to_string(value) {
            self.map.insert(key.into(), AttrValue::Json(json));
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Get raw JSON string
    pub fn get_json_str(&self, key: &str) -> Option<&str> {
        match self.map.get(key) {
            Some(AttrValue::Json(s)) => Some(s),
            _ => None,
        }
    }
}
```

#### 2.4 Использование

```rust
// Хранение вложенного HashMap
let nested: HashMap<String, i32> = HashMap::new();
attrs.set_json("nested_data", &nested);

// Чтение
let nested: Option<HashMap<String, i32>> = attrs.get_json("nested_data");
```

---

## Phase 3: comp2local / local2comp методы

### Проблема
Time conversion логика разбросана внутри `compose()`, нет явных публичных методов.

### Решение
Выделить в отдельные методы с чёткой документацией.

### Изменения

#### 3.1 comp.rs - добавить методы

```rust
impl Comp {
    /// Convert parent comp frame to child's local frame
    ///
    /// Takes into account:
    /// - child's start position in parent timeline
    /// - child's speed multiplier (TODO: not yet implemented)
    ///
    /// # Arguments
    /// * `child_uuid` - UUID of child layer (instance UUID)
    /// * `comp_frame` - Frame number in parent comp timeline
    ///
    /// # Returns
    /// Local frame number in child's coordinate system, or None if child not found
    pub fn comp2local(&self, child_uuid: Uuid, comp_frame: i32) -> Option<i32> {
        let attrs = self.children_attrs.get(&child_uuid)?;
        let child_start = attrs.get_i32("start").unwrap_or(0);
        let speed = attrs.get_float("speed").unwrap_or(1.0);

        // Offset from child's start position
        let offset = comp_frame - child_start;

        // Apply speed (TODO: implement properly)
        let local_frame = if speed != 0.0 {
            (offset as f32 / speed).round() as i32
        } else {
            offset
        };

        Some(local_frame)
    }

    /// Convert child's local frame to parent comp frame
    ///
    /// Inverse of comp2local().
    ///
    /// # Arguments
    /// * `child_uuid` - UUID of child layer (instance UUID)
    /// * `local_frame` - Frame number in child's local timeline
    ///
    /// # Returns
    /// Frame number in parent comp timeline, or None if child not found
    pub fn local2comp(&self, child_uuid: Uuid, local_frame: i32) -> Option<i32> {
        let attrs = self.children_attrs.get(&child_uuid)?;
        let child_start = attrs.get_i32("start").unwrap_or(0);
        let speed = attrs.get_float("speed").unwrap_or(1.0);

        // Apply speed and add offset
        let comp_frame = child_start + (local_frame as f32 * speed).round() as i32;

        Some(comp_frame)
    }

    /// Get source comp's frame for given parent frame
    ///
    /// Combines comp2local with source comp lookup.
    /// Used in compose() for recursive frame fetching.
    pub fn resolve_source_frame(
        &self,
        child_uuid: Uuid,
        comp_frame: i32,
        project: &Project,
    ) -> Option<(Uuid, i32)> {
        let attrs = self.children_attrs.get(&child_uuid)?;

        // Get source UUID
        let source_uuid_str = attrs.get_str("uuid")?;
        let source_uuid = Uuid::parse_str(source_uuid_str).ok()?;

        // Get source comp to find its start
        let source = project.get_comp(source_uuid)?;

        // Convert to local frame
        let local_frame = self.comp2local(child_uuid, comp_frame)?;

        // Map to source comp's timeline
        let source_frame = source.start() + local_frame;

        Some((source_uuid, source_frame))
    }
}
```

#### 3.2 Обновить compose() использовать новые методы

```rust
// БЫЛО (в compose()):
let offset = frame_idx - child_start;
let source_frame = source.start() + offset;

// СТАНЕТ:
let (source_uuid, source_frame) = self.resolve_source_frame(child_uuid, frame_idx, project)?;
```

---

## Execution Order

### Step 1: Phase 2 (AttrValue::Json)
- Самое простое, не ломает существующий код
- Можно сделать первым как подготовку

### Step 2: Phase 3 (comp2local/local2comp)
- Рефакторинг внутри Comp
- Не затрагивает другие файлы

### Step 3: Phase 1 (Project duplication)
- Самое большое изменение
- Затрагивает main.rs, player.rs
- Много вызовов для обновления

---

## Files Affected

| File | Phase 1 | Phase 2 | Phase 3 |
|------|---------|---------|---------|
| src/player.rs | MAJOR | - | - |
| src/main.rs | MAJOR | - | - |
| src/entities/attrs.rs | - | MAJOR | - |
| src/entities/comp.rs | - | - | MAJOR |
| src/entities/project.rs | minor | - | - |

---

## Testing

После каждой фазы:
1. `cargo build` - компиляция
2. `cargo test` - unit tests
3. Ручной тест - загрузка клипа, playback, timeline

---

## Risks

### Phase 1 (Project)
- **HIGH**: Много изменений в main.rs event handling
- Mitigation: Делать по одному методу, проверять компиляцию

### Phase 2 (Json)
- **LOW**: Аддитивное изменение
- Mitigation: Не ломает существующий код

### Phase 3 (time methods)
- **MEDIUM**: Может сломать time mapping если ошибка в формуле
- Mitigation: Написать unit tests для edge cases

---

## Decision Log

| Date | Decision |
|------|----------|
| 2025-11-30 | Plan created |
| 2025-11-30 | Cache refactored to nested HashMap (done) |
| 2025-11-30 | Вариант A выбран для Project ownership |

---

*Last updated: 2025-11-30*
