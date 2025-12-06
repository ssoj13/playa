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

  1. бары сделай 90%

  2. при начале драга слоя из проекта на timeline нужно генерировать хорошее имя: брать имя исходника,
    - если содержит числа
      - брать всё что находится ДО последней группы цифр в имени,
      - добавлять подчёркивание и текущий номер компа
      - Номер надо проверять по содержимому project.media
    - это должно единым образом работать для всех типов слоёв - и comp и file comp
    - видимо надо сделать project.generate_name(base_name) или типа того.

  3. проверяй на дропе uuid слоёв - если дропаём комп сам в себя - то ничего не делать, просто тихая отмена.

  4. контролы слоёв слева на таймлайне не совпадают по вертикали с барами слоёв справа. Надо выровнять левую часть как-то, проверь как.

  5. Когда в attribute editor изменяешь in/out - должны меняться и trim_in/trim_out если они были равны in/out изначально. Если нет - то не менять.



## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 





