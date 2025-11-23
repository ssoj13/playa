План работ по Project/Attributes

Согласованные уточнения:
- Персистентность selection/active хранить в Project (поля selection: Vec<uuid>, active: Option<uuid>). Порядок selection сохраняется и передается как есть. Активный подсвечивается тонкой зеленой полосой слева (4 px).
- Даблклик активирует item (отправка события active_changed) и добавляет его в selection, если не был. Обычный клик обновляет selection (ctrl/shift).
- Кнопки save/load/add clip/add comp/clear all — в один ряд, без отдельной надписи “project”.
- Мульти-dnd: если выделено N items, тянем весь список (в порядке selection) — реализовать позже, подготовить основу.

План действий:
1) Диагностика списка в Project: найти причину “огромных” item’ов и крошечного скроллбара (вероятно layout/allocate_exact_size/layout_no_wrap) и предложить фикс.
2) Добавить персистентные поля selection/active в Project (serde), синхронизацию в Player/PlayaApp на load/save/rebuild.
3) Обновить Project UI:
   - Клик → selection_changed (с ctrl/shift логикой).
   - Даблклик → active_changed + обновление active в Project/Player.
   - Подсветка активного: 4px зелёная вертикальная полоса слева.
   - Кнопки в один ряд; убрать лишний заголовок.
4) События/Bus: добавить/использовать событие для активного item, чтобы Timeline/Viewport реагировали (Player.set_active_comp).
5) Подготовить основу для мульти-dnd: старт drag использует полный selection, drop добавляет список в Comp.

Дальше: начать с п.1 (diagnose list) и п.2 (персист selection/active), затем показать детали.


Впредь сначала показывай мне свои выводы, возможные варианты исправления и жди моего разрешения.