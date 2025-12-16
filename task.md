1. Почему так сильно тормозит интерфейс на прелоаде? Давай сделаем flamegraph или какой-то профайлинг производительности?
  - Дай опции и план тестирования.
Вот информация к размышлению:

# Investigation: Timeline Scrubbing UI Jank
- Symptom: UI stutters when scrubbing the playhead on the timeline; EventBus decoupling is in place, but frame jumps feel blocked.
  Findings
  - `SetFrameEvent` handling writes playhead via `project.modify_comp` (`src/main_events.rs:242-249`). This takes a write lock on `project.media`.
  - `modify_comp` grabs the global `media` write lock before invoking the closure (`src/entities/project.rs:427-449`). Any playhead change blocks on this lock.
  - Preload/compute jobs hold a read lock on the same `media` for the entire compute:
    - In worker enqueued closures, they read-lock `media` and run `comp.compute` under that guard (`src/entities/comp_node.rs:1135-1152`).
    - `signal_preload` also read-locks `media` to build `ComputeContext` (`src/entities/comp_node.rs:1228-1238`).
  - With workers holding long-lived read locks while composing frames, the UI thread cannot acquire the write lock to update the playhead, causing jank during scrubbing. EventBus isn’t the bottleneck; the shared `RwLock` on the media pool is.
  - Recommendations?

2. изучить вопрос добавления tools - move, rotate, scale, комбинированного манипулятора как в Maya.
  - Его надо рисовать поверх картинки оверлеем.
  - Как это делать быстро?
  - Можно рисовать OpenGL?
  - у манипулятора должны быть: Move: XYZ arrows, Rotate: XYZ Circles, Scale: XYZ axis with boxes at ends (like arrows just with boxes at the end).
  - Мышь должна уметь подсвечивать эти элементы при наведении и кликать на эти элементы и делать drag. При этом обновляются соответствующие атрибуты на выбранном слое в timeline. Всё как в Maya или Houdini.
  - Можно ещё изучить вопрос экранного layer picking на основе какого-нибудь opengl back buffer в который мы будем рендерить какие-то хэши превращённые в индексы.
  - Дай опции и лучшие практики на эту тему. Что лучше всего сделать?

3. Изучить вопрос создания Python API и обьектной модели отражающей структуру приложения: Player/Project/Media/Timeline/Viewport/Node_editor с соответствующими командами для каждого модуля.
  - import playa
  - plr = playa.new_player()
  - prj = plr.new_project()
  - p.add_clip(...)
  - p.add_folder(...)
  - p.add_comp(...)
  - playa.player.timeline.go(0)
  - playa.player.timeline.play()

4. Дай отчёт, запиши в report.md

