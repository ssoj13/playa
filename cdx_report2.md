# План исправлений (playa)

## Цели
- Довести EventBus до рабочей интеграции: управление воспроизведением, таймлайном, drag’n’drop.
- Исключить рассинхрон настроек и рантайма (loop/FPS/workers).
- Реализовать недостающие операции навигации/удаления и очистить мёртвый код.

## Шаги
1) **События воспроизведения и навигации**
   - Реализовать `AppEvent::{StepForward,StepBackward,StepForwardLarge,StepBackwardLarge,PreviousClip,NextClip}` через `Player::step`, `jump_prev_sequence`, `jump_next_sequence`.
   - Связать `TogglePlayPause`, `Stop`, `JumpTo{Start,End}` с `Player` (уже частично есть) и добавить тесты/ручные сценарии.
2) **Play range и таймлайн**
   - Для `SetPlayRange{Start,End}, ResetPlayRange, SetCompPlay{Start,End}, ResetCompPlayArea` убедиться, что работа идёт через `Comp`/`Player` API, обновлять timeline_state при изменениях.
   - Обрабатывать `TimelineActions` из `render_timeline_panel` (пока пусто) — зафиксировать будущие поля для навигации/drag.
3) **Drag & drop / слои**
   - Реализовать `AppEvent::{DragStart,DragMove,DragDrop,DragCancel}` и `RemoveSelectedLayer`.
   - Подключить к `TimelineState.drag_state` и `GlobalDragState`; обновлять `Comp.children`/`children_attrs` и инвалидировать cache.
4) **Удаление медиа**
   - Дописать `AppEvent::RemoveMedia`: удалять из `Project.media` и `comps_order`, перекидывать `active_comp` на первый оставшийся, пересобрать runtime (`rebuild_runtime`), сбросить selection.
5) **Hotkeys**
   - Использовать `HotkeyHandler::handle_input` в `handle_keyboard_input` до прямых if-key блоков; либо удалить handler, либо перенести туда основные биндинги.
   - Реализовать обработку `AppEvent::Hotkey{Pressed,Released}` или убрать события.
6) **Loop/FPS консистентность**
   - `ToggleLoop`, `IncreaseFPS`, `DecreaseFPS` должны менять `player.loop_enabled`/`player.fps_base` и только затем синхронизировать в `settings`.
7) **Workers**
   - Учитывать `args.workers` / `settings.workers_override`: пересоздавать `Workers` перед `PlayaApp` или внедрить `Workers::resize` и вызывать при старте.
8) **Тесты/валидация**
   - Добавить модульные тесты для `EventBus` (уже есть) + новые: `Player::step` bounds/loop, удаление media, hotkey routing.
   - Ручной чеклист: шаги по клавишам (PgUp/PgDn, [, ], F1/F2/F3/F4, loop toggle), drag layer, удаление клипа.

## Риски/важности
- Высокий: пустые event-ветки и рассинхрон настроек → пользовательские действия “молчат”.
- Средний: drag/drop/удаление — вероятность рассинхрона `comps_order` и `active_comp`.
- Низкий: hotkey handler — мёртвый код, но простаивает функционал настройки.
