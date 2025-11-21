# План по доработке Timeline (todo5)

1) Развернуть EventBus для таймлайна: добавить AppEvent для zoom/pan/snap/frame-numbers, reorder/move/trim результатов и playhead/set frame; реализовать обработку в main.rs/Comp, чтобы состояние/композиция менялись через события.
2) Убрать прослойку TimelineAction из ui.rs: render_outline/render_canvas должны сразу диспатчить в EventBus (через переданный sender); убрать накопление timeline_actions.
3) Перевести горизонтальный скролл/zoom на единый TimelineState через события, чтобы pan/zoom жили в одном месте и не дублировались.
4) Включить TimelineViewMode (Split/CanvasOnly/OutlineOnly) и коллапс/ширину outline; проверить панель скрытия.
5) Прогнать cargo fmt и сборку/запуск после рефактора; ручная проверка DnD/trim/playhead/zoom/pan.

## Прогресс
- [x] Renderers теперь диспатчат AppEvent напрямую в EventBus из ui.rs; добавлены события zoom/pan/snap/frame-numbers/lock-work-area, мост TimelineAction убран из ui.rs.
- [x] Пан/zoom переведён на события/state: ruler/canvas без ScrollArea по X, pan через middle-drag/колесо -> AppEvent; view mode переключается в UI (Split/Canvas/Outline).
- [x] Удалён неиспользуемый TimelineAction и лишние поля drag state; cargo fmt + cargo check (warning-only: legacy child_ui/dead code в других модулях).
- [ ] Ручная проверка интеракций (DnD/trim/playhead/pan/zoom/outline hide), дальнейшая чистка оставшихся ворнингов при необходимости.
