# Today's task:

## Prereqs: 
  - Answer in Russian in chat, write English code and .md files.
  - MANDATORY: Use filesystem MCP to work with files, memory MCP to remember, log things and create relations and github MCP or "gh" tool if needed. 
  - Use sub-agents and work in parallel.
  - В каталоге C:\projects\projects.rust\playa.old находится старый код в котором всё работало. Консультируйся с ним, но учти что там старая ненужная логика, а мы используем новую
  - Наша новая логика в reports/arch.md: complete module separation with EventBus and such.

## Workflow:
  - Check the app, try to spot some illogical places, errors, mistakes, unused and dead code and such things.
  - Find unused code and try to figure out why it was created. I think you haven't finished the big refactoring and lost pieces by the way.
  - Do not guess, you have to be sure and produce production-grade decisions and problem solutions. Consult context7 MCP use fetch MCP to search internet.
  - Create a comprehensive dataflow for human and for yourself to help you understand the logic.
  - Do not try to simplify things or take shortcuts or remove functionality, we need just the best practices: fast, compact and elegant, powerful code.
  - If you feel task is complex - ask questions, then just split it into sub-tasks, create a plan and follow it updating that plan on each step (setting checkboxes on what's done).
  - Create comprehensive report so you could "survive" after context compactification, re-read it and continue without losing details. Offer pro-grade solutions.

## Specific task: 
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


15. когда драгаешь слой направо так чтобы он пересекал границы окна таймлайна - он вообще исчезает, даже место под него, но в левой панели контролы по-прежнему есть. кнопка [ выводит его обратно. Почему?


 


## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 





