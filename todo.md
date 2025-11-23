# TODO1: Отчет по проекту (каталог src)

## Обзор структуры
- `main.rs`: точка входа, инициализирует `PlayaApp`, UI-композицию доков, навигацию, горячие клавиши, загрузку секвенций/плейлиста, диспетчер событий, рендер в egui.
- `cli.rs` / `config.rs`: аргументы командной строки, конфигурация путей и настроек.
- `dialogs/`: UI-диалоги (pref/encode) и обработчики хоткеев/настроек.
- `entities/`: модели данных (`Project`, `Comp`, `Frame`, загрузка медиа/видео, атрибуты, сериализация).
- `events.rs`: шина событий и перечисления AppEvent/HotkeyWindow.
- `player.rs`: воспроизведение (JKL, play range, активный comp, current frame, loop/FPS логика).
- `ui.rs`: сборка UI доков, отрисовка viewport/timeline/project/status и диалогов.
- `widgets/`: подвиджеты (viewport, timeline, project, status, AE UI). В `status` и `timeline` выделены состояния/рендер.
- `workers.rs`: пул фоновых задач (фрейм загрузки/кодирование).
- `utils.rs` и пустая `utils/`: общие утилиты.

## Найденные проблемы и недочеты
1) Незавершенные функции/хоткеи (TODO) в `main.rs` (~580, 658, 702, 708, 967–981):
   - Нет реализации шагов по кадрам, переключения клипов, удаления медиа, полноэкрана, удаления выбранного слоя и drag&drop (start/move/drop/cancel). Это оставляет UI без ключевых действий.
2) Разрыв хоткеев/фокуса (`dialogs/prefs/input_handler.rs` + `main.rs`):
   - Бинды только для части событий (нет J/K/L, стрелок, B/N, Fullscreen и др.), при этом `handle_input` в `main.rs` полагается на `HotkeyHandler`. Основные AppEvent недоступны с клавиатуры.
   - `focused_window` в `HotkeyHandler` не синхронизируется с фокусом UI, кроме ручной установки в `main.rs` (определение фокуса крайне упрощено, возможны ложные срабатывания).
3) Управление плейлистом/проектом в `main.rs`:
   - `load_project` не проверяет совместимость версии/формата; нет валидации путей/существования файлов перед установкой активного comp.
   - При загрузке последовательностей `Comp::detect_from_paths` на успехе активируется только первый comp, но не обновляется `selected_media_uuid`, что ломает логику UI выбора.
4) UI-состояние доков:
   - `dock_state` сериализуется, но поля `DockTab` и runtime состояния (`viewport_hovered`, `timeline_hovered`, `project_hovered`) не пересчитываются при десериализации; возможен рассинхрон хоткеев и маршрутизации фокуса после рестора.
5) Память/воркеры (Default в `main.rs`):
   - Кол-во воркеров фиксируется 75% CPU без учета I/O bound задач; нет лимита на очередь/обработку исключений при панике в воркере.
6) Пустая директория `src/utils/`: вероятно забытые утилиты или мусор — стоит удалить или заполнить.
7) Диалог настроек/хоткеев (`dialogs/prefs/input_handler.rs`):
   - `remove_binding` не очищает по модификаторам; хранится строковый ключ, но нет нормализации кейсов (`egui::Key` формат "A"/"Space"/"ArrowRight"), что делает бинды хрупкими.

## Замеченные риски/улучшения
- Отсутствуют тесты на ключевые модули (player, events, comp detection). Нужны базовые unit-тесты для play range и переключения comp.
- Много runtime unwrap/get без обработки ошибок в `player.rs` и `entities/*` — возможны паники при кривых проектах.
- Работа с файловыми путями зависит от `PathConfig::from_env_and_cli(None)` без проверки доступности директории/создания; неясно, как обрабатываются несуществующие пути.
- Drag&drop и remove-layer события объявлены, но не реализованы в UI и player — вероятно несостыковка с `events::EventBus`.

## Проверка доступа к FS (MCP filesystem)
- `list_directory`, `directory_tree`, `read_text_file`, `get_file_info`, `create_directory`, `write_file` успешно использованы.
- Дополнительно доступны: `edit_file`, `move_file`, `read_multiple_files`, `list_directory_with_sizes`, `read_media_file`, `search_files` (не требовались для отчета).



# TODO2:
  - src/widgets/timeline/timeline_ui.rs: unused import `find_free_row_for_new_layer`; unused var `has_overlap` (assigned but unread). Use helpers/logic or drop/underscore.
  - src/dialogs/prefs/input_handler.rs: HotkeyHandler::remove_binding unused. Either wire up, mark with #[allow(dead_code)], or remove.
  - src/entities/mod.rs: traits ProjectUI/TimelineUI/AttributeEditorUI/NodeUI unused. Decide whether to integrate or prune/allow.
  + Fixed: src/entities/attrs.rs: unused methods get/remove/iter_mut/contains/len. (Now wired: AE UI uses iter_mut/len/is_empty/iter; child play range uses get_mut/remove; hash uses get). Remaining: adjust if more API exposure needed.
  + Fixed: src/entities/comp.rs: setters/hierarchy helpers now wired via AppEvent handlers (add layer, FPS change, remove layer). Remaining: ensure reorder/move path good, review parent semantics for shared children.
  - src/entities/compositor.rs: CompositorType::blend and CpuCompositor::blend unused. Hook into compositor pipeline or remove/allow.
  - src/entities/project.rs: set_compositor/get_comp/remove_media unused. Use for project management or prune/allow.
  - src/events.rs: many AppEvent variants, HotkeyWindow::AttributeEditor, CompEvent::TimelineChanged, EventBus::sender/drain unused. Either add handlers/usages or drop/allow.
  - src/player.rs: reset_play_range and toggle_play_pause unused. Call from UI or remove/allow.
  - src/widgets/project/project.rs: PlaylistActions alias unused. Use or remove.
  - src/widgets/timeline/timeline.rs: LayerGeom.visible_start/end unused. Use or remove.
  - src/widgets/timeline/timeline_helpers.rs: draw_playhead and find_free_row_for_new_layer unused. Call in timeline rendering/placement or drop/allow.
  - src/widgets/viewport/renderer.rs: shader_error unused. Surface shader errors in UI/logging or remove/allow.
  - Note: `cargo fix --bin "playa" -p playa` can auto-drop some unused imports, but manual decisions needed for API surface vs pruning.


We're USING JUST START.CMD TO BUILD AND TEST!
Use MCP to work with files and github if needed and for everything else.
Use sub-agents and work in parallel.
