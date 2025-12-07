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

## Outputs:
  - At the end create a professional comprehensive report and update plan and write it to planN.md where N is the next available number, and wait for approval! 

## Task:

В приложении есть окно таймлайна: Исследовать это окно и найти все косяки. У нас там полно багов:

  1. Элементы управления cлоями не выровнены горизонтально, как в "сетке".
    Это из-за разной длины имён. Нужно колонку имён сделать фиксированной длины 150 или другие решения, например как вертикальные сепараторы как в Attribute Editor. Это сложно?

  2. Слева не нужен бокс для реордера, у нас есть для этого drag'n'drop в правой части, где слои.
  
  3. при перемещении слоя за пределы timeline влево или вправо он внезапно вообще исчезает и не рисуется. Похоже это происходит только если слоёв 3 или больше, и только с верхними.
    Это какое-то нарушение логики рисования, проверь почему это вообще происходит? Трэк леера должен показываться даже если он за границами. Я думаю что-то происходит с атрибутами.

  4. нужен какой-то механизм обновления viewport если текущий кадр сменил статус. Что-то типа: мы меняем атрибут -> включился флвг dirty. Если dirty выключен - берём готовый кадр, если включён - compose() и кладём в кэш.

  5. Почему так странно рисуется timeline? У меня такое ощущение что там не просто два окна соединённые слайдером, а они рисуются с какими-то дикими оффсетами, как-то неверно. Мне нужно просто два окра разделённых слайдером. 

  6. Возможно что-то ещё.


Сделать отчёт и план исправлений и ждать.









