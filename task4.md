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

  1. контролы слева в таймлайне всё равно чуть не выровнены: имя разной длины. всё остальное ок.
  2. При клике на пустом месте под слоями селекшн должен сбрасываться, даже там где слоёв нет, ПОД ними если нажать - селекшн ДОЛЖЕН сбрасываться.
  3. Сделай толщину линии обводки слоя 1 px
  4. Сделай режим trim slide: когда слой обрезан по краям, если мы кликаем внутри обрезанной зоны но вне trim_in/trim_out - тогда мы можем двигать in/out клипа, оставляя его тримы на месте. Типа Slide tool? Если я сдвинул клип влево на 10 кадров за "пустоту", то in/out должны уменьшиться на 10 кадров, а тримы - прибавить по 10 кадров.

## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 





