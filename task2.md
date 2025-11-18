при запуске приложения во второй раз, выдаётся вот такое:
---
[2025-11-18T05:54:30.071Z ERROR playa] Failed to load frame: Image error: The system cannot find the file specified. (os error 2)
[2025-11-18T05:54:30.071Z ERROR playa] Failed to load frame: Image error: The system cannot find the file specified. (os error 2)
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: The system cannot find the file specified. (os error 2)
[2025-11-18T05:54:30.069Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.069Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: The system cannot find the file specified. (os error 2)
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
[2025-11-18T05:54:30.070Z ERROR playa] Failed to load frame: Image error: Previously failed
---
В первый запуск я загрузил клипы и сделал вложенную комп. всё собралось и работало. Что-то не то с десериализацией состояния Clip/Comp/Project/чего-то ещё.
Проверь все пути сохранения и восстановления.
Желательно унифицировать инициализацию элементов и их восстановление из сериализации, чтобы это делала одна и та же функция.
Не нужно совместимости, можно поменять формат сериализации если надо.
Изучи всё, проверь логику, нелогичные места, косяки, нереализованные места и TODO, и подобные вещи.
Кроме того во второй раз не загружаются кадры, показываются ошибки загрузки на зеленом кадре.
---

Я хотел бы иметь несколько основных частей, полностью отделенных друг от друга, буквально в разных подкаталогах:
- app.rs - приложение которое содержит все остальные части, логику старта и выхода, глобальный hotkeys handler, который смотрит над каким окном нажали кнопку и шлёт сообщение с нужным префиксом для этого окна (<hotkey.del:pressed>), CLI args и help.
- project.rs widget: contains clips and comps
- viewport.rs widget: Generic viewport. Receives &Frame to display. Handles viewport controls, zoom, calculations, etc.
- timeline.rs controller widget: After Effects-like timeline with clips and nested comp
- ae.rs: ttribute editor widget: "Maya Attribute Editor": shows attributes of selected entity either in project or timeline window
- noded.rs controller widget: Houdini-like node view. Node editor пока делать не надо, но вообще суть Clip и Comp именно в том что они - просто ноды разного типа с единым интерфейсом.
- node.rs: base node for noded.rs
—-
- prefs dialog
- encoder dialog


Все части функционально отделены друг от друга, и имеют простые стандартные интерфейсы.
Общение частей происходит либо через каналы (crossbar?) либо через шину сообщений, какие в Rust есть варианты?
Т.е. нажатие кнопок, перемотка таймслайдера, процесс проигрывания - всё контрольные элементы просто посылают сообщения, а соответствующие функции "слушают" их и что-то делают.
Например drag'n'drop на окно проекта, кнопка Add Clip, аргумент -f - все просто испускают сигнал "добавить clip с аргументом для клипа".
Все кнопки, например Play/pause, Start, End - тоже сигналы. Клавиши JKL, B/N/Ctrl-B - все кнопки тоже испускают сигналы, а keyboard handler слушает.

У каждого окна свой handler hotkeys: он слушает сообщения, и если получает, скажем "<hotkeys.b: pressed>" - то "забирает" его себе и вызывает функцию которая висит на b.
Эти сообщения посылает глобальный хэндлер нажатий на кнопки (в App), который определяет какому элементу отправить сообщение 



Базовые структуры (Entities) должны работать без GUI: Project, Clip, Comp, Encoder, Node processing engine (Node struct with all required traits for parent-child relationship and graph processing)
(В теории потом можно будет прицепить питоновский API и процессить видео в батче без GUI.)

Entities должны поддерживать трейты для GUI типа
  - ::project_ui	: widget элемента для окна проекта: имя клипа или компа и какие-то метаданные: разрешение, кол-во кадров
  - ::timeline_ui	: widget элемента для таймлайна, обычно бар
  - ::ae_ui			: widget со всеми атрибутами элемента. При изменении значений, должны обновляться атрибуты соответствющего активного (selected) Clip/Comp. Аналог Maya Attribute Editor
То есть например Clip или Comp будут иметь одно представление для окна проекта, другое - для таймлайна, третье - для AE.
Если это будут функции рисования (как в egui это работает?) - ну и хорошо.

Entities предоставляют handlers, слоты или API/Interface для контроллеров:
  - Сигнал "добавить клип" просто вызывает Project.add_media(&metadata)
  - Сигнал "добавить комп" просто создаёт Comp с настройками по умолчанию (настройки проекта по умолчанию - отдельная секция конфига Project)
  - Процесс Drag'n'Drop сначала берёт клип или комп под мышью
    - dnd_start(&Media) запоминает что тащат
    - dnd_drag() - рисует временную визуализацию бара клипа или компа на месте мыши и снэпится к краям клипов которые предоставляет Comp
    - dnd_drop() опять же вызывает сигнал "project.add_media" -> принимается Comp.add_media(move |Media| = ....)
    - и так далее
  - Перетаскивание слайдера на таймлайне просто посылает сообщение "comp.set_frame", и оно диспатчится -> Comp.set_frame(num)
Таким образом мы можем применять эти функции и в неблокирующемся GUI и в API.


---
Теперь по интерфейсу:
  - Мы можем использовать https://crates.io/crates/hello_egui, в частности egui_dnd и egui_taffy, egui_dock - для стайлинга, drag'n'drop и для сплитов. Изучи это всё, мне нужен хороший интерфейс, а то мы проблему с прыгающим таймлайном так и не решили.


Изучи вопрос, подготовь доклан и мнения. Кратко но по делу.
Не гадай, используй интернет и mcp, sub-agents, работай в параллели.
