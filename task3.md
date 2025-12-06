# Task 3: Code Quality Issues

## HIGH Priority

1. **LayerGeom computed twice per layer** (`timeline_ui.rs:471-629`)
   - `LayerGeom::calc()` called in draw pass, then again in interaction pass
   - Fix: cache results from draw pass in Vec, reuse in interaction

2. **Poisoned lock panics** (`main_events.rs:287`, `comp.rs:782`)
   - `.unwrap()` on RwLock can panic if lock is poisoned
   - Fix: use `.expect("context")` or proper error handling

3. **Race condition in enqueue_frame** (`comp.rs:954-962`)
   - Check for existing status and insert are not atomic
   - Two threads could both pass check and both insert

## MEDIUM Priority

4. **Duplicate edge jump handlers** (`main_events.rs:141-170`)
   - `JumpToPrevEdgeEvent` and `JumpToNextEdgeEvent` nearly identical
   - Fix: single helper with direction parameter

5. **Duplicate FPS handlers** (`main_events.rs:182-197`)
   - `IncreaseFPSBaseEvent` / `DecreaseFPSBaseEvent` identical except sign
   - Fix: single handler with delta

6. **Duplicate row layout logic** (`comp.rs:1550-1598`)
   - `find_insert_position_for_row()` duplicates `compute_all_layer_rows()`
   - Fix: reuse existing helper

7. **Lock contention in loop** (`comp.rs:782-783`)
   - `project.media.read().unwrap()` acquired per iteration in child loop
   - Fix: get all needed data in one read

## LOW Priority

8. **Unnecessary clone** (`timeline_ui.rs:924`)
   - `source_uuid.clone()` - Uuid is Copy, clone unnecessary

9. **Dead variable** (`timeline_ui.rs:872`)
   - `_has_overlap` computed but never used

10. **Inconsistent reference** (`main_events.rs:89`)
    - `downcast_event::<...>(&event)` - extra `&` unnecessary

11. **Confusing double negation** (`comp.rs:1293`)
    - `attrs.get_bool("visible").unwrap_or(true) == false`
    - Fix: `!attrs.get_bool("visible").unwrap_or(true)`

12. **Unused parameter** (`comp.rs:1858`)
    - `get_child_edges_near()` takes `_from_frame` but never uses it


13. Сейчас viewport on-screen timeslider глючит: если мышь выходит за пределы кадра, это убирает кадр с экрана и и слайдер перестаёт работать. Посмотри как можно починить или заклампить минимальный и максимальный кадры даже если мышь за пределами


14. изучить вопрос добавления на бары file comp диагональной крупной штриховки полосками, сложно ли?
