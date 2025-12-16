1. Почему так сильно тормозит интерфейс на прелоаде? Давай сделаем flamegraph или какой-то профайлинг производительности?
  - Дай опции и план тестирования.

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

