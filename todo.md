Набор ворнингов от сборки — это не «устаревшие API», а вывалившиеся/отложенные функции. Надо решить, что привести в порядок:

- timeline_ui: что за child_ui, Обьясни?
- viewport/mod.rs: `ViewportActions` - проследить, нужны ли она дли EventBus?
- dialogs/prefs/hotkeys.rs: добить HotkeyHandler и интегрировать в UI prefs (handle_key / bindings).
- entities/mod.rs: трейты UI (ProjectUI/TimelineUI/AttributeEditorUI/NodeUI) ИСПОЛЬЗОВАТЬ в виджетах, это интерфейс отрисовки. project, timeline, attribute editor и nodeed ДОЛЖНЫ использовать эти функции для отрисовки. Это инкапсулирует UI Comp в них самих. Плюс проследить чтобы был TimelineDragUI - во время драга.
- entities/attrs.rs: вспомогательные методы (get/remove/iter_mut/contains/len) вернуть в код если надо и поможет нам дедуплицировать код или убрать.
- entities/comp.rs: сеттеры/parent-геттеры вернуть в использование или выпилить, они нам нужны?; FrameStatus::color задействовать в индикаторе.
- loader_video.rs: поле frame_count использовать в метаданных. Проследить чтобы функциональность загрузчика не была потеряна
- project.rs: set_compositor/get_comp/remove_media убрать.
- events.rs: AppEvent/HotkeyWindow/CompEvent::TimelineChanged — подключить к EventBus, всё должно идти через шину сообщений.
- timeline.rs: поля drag_state (display_name/drag_start_pos/initial_end) и хелперы detect_layer_tool/draw_playhead — восстановить использование или удалить.
- viewport/renderer.rs: shader_error задействовать в UI ошибок.

Сверка с .orig (рабочий код, могло отвалиться):
- Таймслайдер: `.orig/src/timeslider.rs` рисует полоску статусов кадров под рулером (FrameStatus::color), кеш через Cache (cached_count + loaded_events + sequences_version), фон секвенсов и play range. В текущем widgets/timeline этого нет — надо вернуть индикатор.
- Секвенсы: `.orig/src/sequence.rs` поддерживает `*` и printf `%0xd`, padding, gaps; строит frame_path. Текущий `utils/sequences.rs` проще — стоит перенести поддержку printf/padding/gaps.
- Frame/EXR/Video: старое `frame.rs`, `exr.rs`, `video.rs`, `convert.rs` могут содержать детали загрузки/тонемап/ffmpeg, worth сверить с `entities/loader.rs` и viewport.
- Хоткеи/настройки: `.orig/src/prefs.rs`, `ui_encode.rs`, hotkey logic — возможно недоперенесено, совпадает с текущими warnings о неиспользуемых заглушках.

План возврата (предложение):
1) Вернуть полоску статусов на таймрулере (использовать FrameStatus::color, кеш статусов). Можно рисовать статусы одновременно с отрисовкой timeruler, если это оптимизирует время.
2) Расширить parser секвенсов printf-паттернами и аккуратным padding/frame_path. Вернуть логику detect_sequence и всего такого в Comp. Теперь детекцией сиквенсой у нас занимается Comp, так как это и есть бывший Sequence совмещённый с Layer.
3) Просмотреть loader/viewport vs .orig на предмет потерянных опций (EXR/video). Видео загрузка и кодирование работало прекрасно.
4) Интегрировать hotkey/prefs или сузить warnings. Нужно добавить в prefs окно настройки hotkeys, но это сложная работа, поэтому оставь там комментарий на потом.

