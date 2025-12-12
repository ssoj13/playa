# Bug Hunt:

## Prereqs: 
  - Answer in Russian in chat, write English code and .md files.
  - MANDATORY: Use filesystem MCP to work with files, memory MCP to remember, log things and create relations and github MCP or "gh" tool if needed. 
  - Use sub-agents and work in parallel.

## Workflow:
  - Check the app, try to spot some illogical places, errors, mistakes, unused and dead code and such things.
  - Check interface compatibility, all FIXME, TODO, all unfinished code - try to understand what to do with it, offer suggestions.
  - Find unused code and try to figure out why it was created. I think you haven't finished the big refactoring and lost pieces by the way.
  - Do not guess, you have to be sure and produce production-grade decisions and problem solutions. Consult context7 MCP use fetch MCP to search internet.
  - Create a comprehensive dataflow for human and for yourself to help you understand the logic.
  - Do not try to simplify things or take shortcuts or remove functionality, we need just the best practices: fast, compact and elegant, powerful code.
  - If you feel task is complex - ask questions, then just split it into sub-tasks, create a plan and follow it updating that plan on each step (setting checkboxes on what's done).
  - Create comprehensive report so you could "survive" after context compactification, re-read it and continue without losing details. Offer pro-grade solutions.

# Task:
  1. Похоже последние изменения сломали кэш - теперь ничего само не загружатся и надо водить по времени мышью чтобы показалось что-то. Что-то отвалилось по пути. Сравни с прошлыми коммитами, посмотри что там поменялось и как чинить.
  2. сейчас можно открыть file comp и драгнуть его сам на себя - после этого всё зависает. Нужна элегантная проверка чтобы нельзя было вложить комп сам в себя или похожее.
  3. Для Comp:: стоит сделать итератор, видимо depth first который будет проходить по всем подслоям. упростит логику другим функциям. возможно стоит добавить опции. Нужно мнение.
  4. разнести F1 хелп по разным окнам - чтобы у каждого окна был свой хелп по своим кнопкам. Чтобы в каждом окне можно было по F1 открыть свой хелп, независимо от другого. Т.е. сделай Help видимо вообще функцией каждого окна. Предложи варианты.
    - расследовать почему после последних изменений хелп изменился и выглядит как кошмар на темной панели. Вернуть старый вид. Разбить все кнопки по панелям (timeline keys - в таймлайн итд) и в каждой панели прописать этот хелп. Потом сделать глобальную кнопку F1 которая будет смотреть текущую панель и выводить её хелп в неё же в левом верхнем углу (панели а не экрана). Задай вопросы.
  5. Как у нас хранятся слои в Comp? просто Vec<Tuple(Uuid, Attrs)> или у нас это отдельный тип с сериализацией? Ты бы как сделал? Может сделать отдельный файл Layer.rs и там определить и чтобы Comp оперировала с данными оттуда? Можно инвалидировать атрибуты per-layer и прочие другие штуки. Предложи солюшны.
  6. Рассмотреть возможность добавления нескольких слоёв на один трек в timeline: видимо перейти от Vec<Layer> на Vec<Vec<Layer, ..>, ..>? Предложи хорошие варианты.
  7. Изучи вопрос добавления Node editor. Какие крейты можно использовать? Для egui есть несколько с нодами, изучи вопрос. Каждый комп можно представить просто как ноду с кучей инпутов, куда воткнуты другие компы. Идеальная страктура для node network.


## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 