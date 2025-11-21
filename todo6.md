› а что это на child_ui вообще, и что за new_child? расскажи про оба и сравни

• В eframe/egui раньше для вложенных элементов использовали ui.child_ui(rect, layout, id), который создаёт новый Ui поверх выбранного прямоугольника с заданным лейаутом. Этот метод помечен deprecated: API сместили на более
  явный и удобный ui.new_child(layout)/ui.child_ui_with_id_source и связку с UiBuilder.

  Сейчас у тебя в timeline_ui.rs есть фрагмент:

  let (row_rect, response) = ui.allocate_exact_size(...);
  let mut row_ui = ui.child_ui(
      row_rect,
      egui::Layout::left_to_right(egui::Align::Center),
      None,
  );

  Это вызывает предупреждение.

  Новый способ (egui 0.27+):

  - ui.new_child(layout) возвращает новый Ui дочерний с тем же clip rect и cursor, но можно следом задать прямоугольник через ui.allocate_exact_size либо воспользоваться ui.child_ui_with_id_source(rect, layout, id_source),
    если нужно жёстко задать прямоугольник.

  Для твоего случая аналог без депрекейта:

  let (row_rect, response) = ui.allocate_exact_size(...);
  let mut row_ui = ui.new_child(egui::Layout::left_to_right(egui::Align::Center));
  row_ui.set_rect(row_rect); // или row_ui.set_clip_rect(row_rect)?

  Практичнее — использовать ui.child_ui_with_id_source(row_rect, layout, ui.id().with("outline_row")), это прямой replacement.

  Разница:

  - child_ui — старый, меньше контроля над id source, deprecated.
  - new_child/child_ui_with_id_source — явный лейаут, управление id/clip, соответствует новому egui API.

  Хочешь, починю предупреждение, заменив на child_ui_with_id_source (с уникальным id per row)?


› кнопки frames, snap и lock почему-то вообще не нажимаются и сплиттер между левой и правой частями не двигается. Переименуй кнопку R в Reset и добавь справа кнопку Fit - чтобы брал текущий диапазон кливов в текущем comp и фиттил его на таймлайне в упор (fit all)
а, и сделай чтобы окно Attributes было не вторым табом рядом с Project, а _под_ ним, чтобы они оба два были видны одно на другом (project сверху)

• I’m tackling unresponsive timeline buttons by syncing state updates immediately on clicks rather than relying only on event dispatch, which delays UI feedback. I’m also adding a “Fit” button that adjusts timeline zoom and
  pan to fit the current composition’s clips, carefully managing state flags and UI sizing across different panels, and updating deprecated egui child UI calls for smoother rendering.
