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
  1. разнести F1 хелп по разным окнам - чтобы у каждого окна был свой хелп по своим кнопкам. Чтобы в каждом окне можно было по F1 открыть свой хелп, независимо от другого. Т.е. сделай Help видимо вообще функцией каждого окна. Предложи варианты.
  2. По F11 можно сделать новую панель куда будут сведены все клавиши от всех панелей - просто панель помощи. Надо продумать как сделать не загромождая всё. Отдельный какой-то файл в src/dialogs/help?
  3. Как у нас хранятся слои в Comp? просто Vec<Tuple(Uuid, Attrs)> или у нас это отдельный тип с сериализацией? Ты бы как сделал? Может сделать отдельный файл Layer.rs и там определить и чтобы Comp оперировала с данными оттуда? Можно инвалидировать атрибуты per-layer и прочие другие штуки. Предложи солюшны.
  4. Рассмотреть возможность добавления нескольких слоёв на один трек в timeline: видимо перейти от Vec<Layer> на Vec<Vec<Layer, ..>, ..>? Предложи хорошие варианты.
  5. Нужно акцептить и файлы и каталоги как инпут. Если это каталог - то искать всю вложенную медию(видео) - опция один, и все вложенные сиквенсы - опция два. Ввести для этого отдельный раздел в Prefs с двумя чекбоксами. 
    - рабочий крейт сканеры сиквенсов - вот тут: C:\projects\projects.rust\scanseq-rs. Можешь скопировать нужные исходники в подмодуль ./src/core/scanseq.rs. Можно все нужные исходники положить либо в один файл либо сделать подкаталог core.scanseq.
  6. Изучи вопрос добавления Node editor. Какие крейты можно использовать? Для egui есть несколько с нодами, изучи вопрос. Каждый комп можно представить просто как ноду с кучей инпутов, куда воткнуты другие компы. Идеальная страктура для node network.

## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 