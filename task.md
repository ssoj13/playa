1. Почему так сильно тормозит интерфейс на прелоаде? Давай сделаем flamegraph или какой-то профайлинг производительности?
  - Дай опции и план тестирования.
Вот информация к размышлению:

3. Изучить вопрос создания Python API и обьектной модели отражающей структуру приложения: Player/Project/Media/Timeline/Viewport/Node_editor с соответствующими командами для каждого модуля.
  - import playa
  - plr = playa.player()
  - prj = plr.project()
  - p.add_clip(...)
  - p.add_folder(...)
  - p.add_comp(...)
  - plr.timeline.set_frame(0)
  - plr.timeline.play()
  - оборачиваем всё в простой python api и делаем из этого python extension с maturin. как тебе идея?

4. изучить возможность добавления web server в плеер и REST api endpoint / endpoints чтобы можно было управлять по вебу.

4. Дай отчёт, запиши в report2.md

