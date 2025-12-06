# Today's task:

## Prereqs: 
  - Answer in Russian in chat, write English code and .md files.
  - MANDATORY: Use filesystem MCP to work with files, memory MCP to remember, log things and create relations and github MCP or "gh" tool if needed. 
  - Use sub-agents and work in parallel.
  - В каталоге C:\projects\projects.rust\playa.old находится старый код в котором всё работало. Консультируйся с ним, но учти что там старая ненужная логика, а мы используем новую
  - Наша новая логика в reports/arch.md: complete module separation with EventBus and such.

## Workflow:
  - Check the app, try to spot some illogical places, errors, mistakes, unused and dead code and such things.
  - Find unused code and try to figure out why it was created. I think you haven't finished the big refactoring and lost pieces by the way.
  - Do not guess, you have to be sure and produce production-grade decisions and problem solutions. Consult context7 MCP use fetch MCP to search internet.
  - Create a comprehensive dataflow for human and for yourself to help you understand the logic.
  - Do not try to simplify things or take shortcuts or remove functionality, we need just the best practices: fast, compact and elegant, powerful code.
  - If you feel task is complex - ask questions, then just split it into sub-tasks, create a plan and follow it updating that plan on each step (setting checkboxes on what's done).
  - Create comprehensive report so you could "survive" after context compactification, re-read it and continue without losing details. Offer pro-grade solutions.

## Specific task: 
  1. при дабл-клике запоминать предыдущий uuid чтобы по u перейти обратно.
  2. При перетаскивании слоя на таймлайн давать ему имя по имени исходника _ДО_ цифр. т.е. clipper_runs_0017.tga превращается в clipper_runs_1, clipper_runs_2 (проверять уже существующие слои при создании слоя после дропа)
  3. если границы trim_in/trim_out активной композиции при дропе нового слоя равны in/out то менять trim_in/trim_out на новые in/out после изменения размера active comp (rebound)
  4. сейчас при изменении атрибута режима наложения в панели слева, перерисовка кадра происходит только если пошевелить время. Это проблема инвалидации кадра и его обновления - при изменениия режима наложения должен измениться хэш children attrs и атрибуты должны стать dirty - это должно вызвать инвалидацию кадра в кэше и т.к. таймслайдер стоит на текущем кадре - при следующем compose() - перерендере кадра, т.к. его нет в кэше (можно просто поменять кадру статус на header что должно его выгрузить, и запрос от вьюпорта в теории его обновит)
  5. Посмотри на возможные проблемы, поищи нелогичности, дублирующийся код, неверные подходы.
 


## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 





