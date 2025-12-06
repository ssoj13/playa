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
# Task 3: Code Quality Issues

## HIGH Priority

  1. контролы слева в таймлайне не выравнены из-за разных длин имён. Нужно выровнять, чтобы колонки контролов были вертикальными.
  2. в Attribute editor при нажатом shift значение должно меняться на 5%, при ctrl - на 1%.
  3. видимо кастомные сеттеры in/out/trim_in/trim_out надо интегрировать прямо в Attrs.set, это спецкейс. Как лучше сделать? Или какой-то единый сеттер для всех in/out/trim_in/trim_out?  Или вызов валидатора после установки? Нужно чтобы trim_in/out умно следовал на in/out, но не во все случаях.
  4. кнопки split/canvas/outline можно перенести справа от Loop, в одну строку, тем самым освободив чуть места по вертикали.
  5. при нескольких выделенных слоях, манипуляции с контролами слева в таймлайне должны сразу отражаться на всех слоях. Подвинул opacity - сразу на всех выделенных слоях должен быть установлен атрибут. Механизм такой же как в attribute editor.



## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 





