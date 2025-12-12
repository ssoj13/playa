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
  - From time to time do a git commits with meaningful explanatory comments into dev branch - important checkpoints.

# Task:
  * нет *_events.rs для ae, node_editor, project. Они должны быть по соотв. подкаталогам. У нас ведь всё унифицировано и дедуплицировано, разложено по полочкам.
  * кнопки F и A похоже глобальные и работают на таймлайн даже если нажаты в node editor. И там и там они должны делать Fit All / Fit Selected, Но по-разному. Timeline делает это по времени, а Node editor - по выделенным или всем нодам, зумится на них.
    - мы пытались исправить это много раз, но оно не работает.
    - Надо сделать так: каждое окно имеет ещё один member: hotkey help
    - Сейчас это не работает.
    - Нужно сделать локальные кнопки F/A для timeline и Node editor и правильно их зароутить на emit events и проверить handlers.
  * Активная композиция не устанавливается в Node editor на восстановлении состояния - показывает 0 nodes пока не дабл-кликнешь на comp в проекте - Node editor должен использовать ту же Project.active_comp что и Timeline. Дедупликация. У нас же установка Active comp посылается сообщением? почему бы node editor не подписаться на него?
  * Надо переработать систему слоёв.
    - Project.media содержит pub media: Arc<RwLock<HashMap<Uuid, Comp>>> - это основное хранилище композиций Comp, которые используются везде как Uuid - память передаётся по ref а не копируется.
    - Все остальные конструкции в нашей системе ссылаются друг на друга.
  * Comp:
    - Functions:
      - Controlled with Comp.get_frame()/set_frame()
    - Attrs:
      - frame
      - 

    - compose()
    - children() 


## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 