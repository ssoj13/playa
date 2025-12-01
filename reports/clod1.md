# Plan: arch.md vs Current Code Analysis

## Summary

arch.md describes target architecture. Current code partially implements it with several gaps and inconsistencies.

---

## 1. Attrs System (entities/attrs.rs)

### arch.md Target:
```rust
struct Attrs {
    get/set/del(String, Val): getter/setter triggering dirty flag
    _hash(): hidden function returning hash of all nested attributes
    _data: HashMap<String, AttrValue>,
    dirty: AtomicBool,
}

enum AttrValue {
    Bool, Str, Int, UInt, Float, Vec3, Vec4, Mat3, Mat4,
    Arc<Atomic*> types,  // for thread-safe shared state
    Json(String),        // nested JSON serialization
}
```

### Current Code:
- `dirty: AtomicBool` - implemented
- `get/set` with dirty tracking - implemented
- Missing: `_hash()` function for cache invalidation
- Missing: `Arc<Atomic*>` types for thread-safe shared primitives
- Missing: `Json(String)` type for nested structures
- Missing: `del()` method

### Action Items:
- [ ] Add `hash_all()` method to Attrs for cache key generation
- [ ] Add `AttrValue::Json(String)` variant
- [ ] Add `AttrValue::AtomicInt(Arc<AtomicI32>)` etc. variants
- [ ] Add `Attrs::del(key)` method

---

## 2. Node Trait (entities/node.rs)

### arch.md Target:
```rust
trait Node {
    attr: Attrs,           // persistent attributes
    data: Attrs,           // transient runtime data (not serialized)
    compute(&self, ctx),   // processing method
}
```

### Current Code:
- **Node trait does not exist**
- Comp/Project/Player contain attrs directly without common interface
- No `data: Attrs` for transient runtime state

### Action Items:
- [ ] Create `Node` trait with `attr()` and `data()` accessors
- [ ] Add `data: Attrs` field to Comp for transient state
- [ ] Consider if Project/Player should implement Node

---

## 3. Comp Structure (entities/comp.rs)

### arch.md Target:
```rust
struct Comp {
    attr: Attrs {
        uuid, name, comp_type,
        frame,                    // current playhead
        width, height, fps,
        in, out,                  // comp bounds
        trim_in, trim_out,        // play range (work area)
        dirty: Arc<AtomicBool>,
        children: Vec<Tuple(src_uuid, children_attr)>,
        compose_solo, compose_mute, compose_opacity, compose_blend_mode,
        selection: Vec<Uuid>,
    }

    fn comp2local(layer_num, frame_num) -> i32  // time conversion
    fn local2comp(layer_num, local_frame_num) -> i32
}
```

### Current Code:
```rust
struct Comp {
    uuid: Uuid,
    mode: CompMode,
    attrs: Attrs,                           // contains name, start, end, fps, play_start, play_end
    children: Vec<Uuid>,                    // instance UUIDs
    children_attrs: HashMap<Uuid, Attrs>,   // per-child attributes
    // ...
}
```

### Differences:
1. **children storage**: arch.md uses `Vec<Tuple>`, current uses `Vec<Uuid>` + `HashMap<Uuid, Attrs>`
   - Current approach is more flexible but less consistent with arch.md
2. **Naming**: arch.md uses `in/out/trim_in/trim_out`, current uses `start/end/play_start/play_end`
3. **compose_* attributes**: arch.md expects per-comp compositing attrs, current has them in children_attrs
4. **comp2local/local2comp**: partially implemented but not as explicit methods

### Action Items:
- [ ] Rename: `start->in`, `end->out`, `play_start->trim_in`, `play_end->trim_out` (or keep and document)
- [ ] Add explicit `comp2local()` and `local2comp()` methods
- [ ] Move compositing attrs (solo, mute, opacity, blend_mode) to proper location
- [ ] Document children storage decision (current HashMap approach vs arch.md Vec<Tuple>)

---

## 4. Project Structure (entities/project.rs)

### arch.md Target:
```rust
struct Project {
    attr: Attrs {
        media: Arc<RwLock<HashMap<Uuid, Comp>>>,
        order: Vec<Uuid>,
        selection: Vec<Uuid>,
        active: Option<Uuid>,
        cache_manager, global_cache, compositor,
    }
}
```

### Current Code:
```rust
struct Project {
    attrs: Attrs,                                    // minimal usage
    media: Arc<RwLock<HashMap<Uuid, Comp>>>,         // direct field
    comps_order: Vec<Uuid>,                          // direct field
    selection: Vec<Uuid>,                            // direct field
    active: Option<Uuid>,                            // direct field
    compositor: RefCell<CompositorType>,             // direct field
    cache_manager: Option<Arc<CacheManager>>,        // direct field
    global_cache: Option<Arc<GlobalFrameCache>>,     // direct field
}
```

### Differences:
- arch.md wants ALL fields in `Project.attr`
- Current code has attrs but stores core fields directly

### Action Items:
- [ ] Decide: migrate all fields to Attrs OR keep current structure with documentation
- [ ] If migrating: need type-safe Attrs accessors for complex types (Arc, RefCell)

---

## 5. Player Structure (player.rs)

### arch.md Target:
```rust
struct Player {
    attr: Attrs {
        project: &Project,
        active_comp: Option<Uuid>,
        is_playing: bool,
        fps_base, fps_play,
        loop_enabled: bool,
        play_direction: f32,
        last_frame_time: Option<Instant>,
    }
}
```

### Current Code:
```rust
struct Player {
    pub project: Project,              // owned, not reference
    pub active_comp: Option<Uuid>,
    pub is_playing: bool,
    pub fps_base: f32,
    pub fps_play: f32,
    pub loop_enabled: bool,
    pub play_direction: f32,
    pub last_frame_time: Option<Instant>,
    pub selected_seq_idx: Option<usize>,
}
```

### Differences:
- No `attrs: Attrs` field
- `project` is owned, arch.md suggests reference
- Extra `selected_seq_idx` field not in arch.md

### Action Items:
- [ ] Add `attrs: Attrs` and migrate appropriate fields
- [ ] Consider if Player should own Project or hold reference
- [ ] Document `selected_seq_idx` purpose or remove if unused

---

## 6. EventBus (events.rs)

### arch.md Target:
```
Events: AppEvent, ProjectEvent, CompEvent, PlayEvent, IOEvent, KeyEvent, MouseEvent
```

### Current Code:
- `AppEvent` - comprehensive enum with all events
- `CompEvent` - separate enum for comp-level events
- No: ProjectEvent, PlayEvent, IOEvent, KeyEvent, MouseEvent

### Differences:
- arch.md wants specialized event types
- Current code consolidates into AppEvent + CompEvent

### Action Items:
- [ ] Evaluate if event splitting provides value
- [ ] Current approach may be simpler and sufficient
- [ ] Add HotkeyWindow enum usage (defined but potentially unused)

---

## 7. Timeline Architecture (widgets/timeline/)

### arch.md Target:
```
- Механизм Drag'n'drop with hit-testing
- Словарь с clip edges, clip bounds
- При движении мыши постоянно проверяем где она находится
- Элементы сортируются по дальности от мыши
```

### Current Code:
- `GlobalDragState` in egui temp storage
- Direct egui interaction handling
- No explicit hit-test registry

### Action Items:
- [ ] Review if current drag implementation is sufficient
- [ ] Consider adding hit-test registry for precision edge grabbing
- [ ] Document current drag architecture

---

## 8. Attribute Editor (widgets/ae/)

### arch.md Target:
```
Редактор атрибутов строит UI на основе типа атрибута:
- текстовые поля, цифры, слайдер, color picker, combobox
```

### Current Code:
- Need to verify widgets/ae/ implementation
- Check if it dynamically builds UI from AttrValue types

### Action Items:
- [ ] Verify ae_ui.rs matches arch.md description
- [ ] Add missing widget types if needed

---

## 9. Cache System

### arch.md Target:
```
GlobalFrameCache: HashMap<UUID:[Frames]> for trivial removal
```

### Current Code:
```rust
LruCache<(Uuid, i32), Frame>  // (comp_uuid, frame_idx) -> Frame
```

### Differences:
- arch.md suggests nested HashMap for easy comp removal
- Current uses flat LRU with composite key
- Current has `clear_comp(uuid)` that iterates and removes

### Action Items:
- [ ] Evaluate if nested HashMap improves removal performance
- [ ] Current approach with `clear_comp()` may be sufficient

---

## 10. EventBus Isolation

### arch.md Target:
```
Все части изолированы друг от друга и общаются исключительно через EventBus.
Они настолько изолированы что должны существовать вообще отдельно, как панель без основного приложения.
```

### Current Code:
- EventBus exists and is used
- Components may still have direct references to shared state

### Action Items:
- [ ] Audit component isolation
- [ ] Identify direct state access that should go through EventBus
- [ ] Document which components are fully isolated

---

## Priority Order

### High Priority (Architecture Foundation):
1. Attrs enhancements (hash_all, Json type, del method)
2. Comp time conversion methods (comp2local, local2comp)
3. Document children storage decision

### Medium Priority (Consistency):
4. Decide on Attrs-everywhere vs direct fields pattern
5. Timeline hit-test improvements
6. Event type organization

### Low Priority (Nice to Have):
7. Node trait (may be over-engineering)
8. Project/Player Attrs migration
9. Cache nested HashMap (current works fine)

---

## Notes

- arch.md is a design document, not all details must be implemented literally
- Some current code deviations may be intentional improvements
- Key principle: **Attrs as universal serialization** for persistence
- Key principle: **EventBus for decoupling** UI from logic
