# План рефакторинга режимов отображения таймлайна

## Проблема

В режиме CanvasOnly при нажатии B и N начало/конец play range ставятся "в космосе" (неправильная позиция). 

**Корневая причина:** Режимы CanvasOnly и OutlineOnly имеют условную логику внутри `render_canvas` и `render_outline`, которая отличается от Split режима. Это приводит к разным оффсетам и неправильному вычислению позиций кадров.

## Текущая архитектура (НЕПРАВИЛЬНАЯ)

### Split режим:
- `render_outline` вызывается в `SidePanel::left`
- `render_canvas` вызывается в `CentralPanel`
- Обе функции получают `view_mode = Split`
- Внутри функций: НЕТ спейсеров слева (потому что `view_mode == Split`)

### CanvasOnly режим:
- `render_canvas` вызывается в `CentralPanel`
- Функция получает `view_mode = CanvasOnly`
- Внутри функции: ЕСТЬ спейсер слева (потому что `view_mode != Split`)
- Проблема: `timeline_rect` создается внутри ScrollArea, который находится внутри horizontal layout со спейсером, но при вычислении кадра используется неправильная базовая позиция

### OutlineOnly режим:
- `render_outline` вызывается в `CentralPanel`
- Функция получает `view_mode = OutlineOnly`
- Внутри функции: условная логика для ширины строк

## Правильная архитектура

**Принцип:** Canvas и Outline должны рисоваться ОДИНАКОВО во всех режимах. Split режим должен просто оборачивать готовые функции в панели.

### План изменений:

1. **Убрать условную логику из `render_canvas` и `render_outline`**
   - Убрать все проверки `if matches!(view_mode, Split)`
   - Функции должны всегда рисоваться одинаково
   - В CanvasOnly/OutlineOnly спейсеры должны добавляться на уровне `ui.rs`, а не внутри функций

2. **Изменить `ui.rs` для CanvasOnly режима:**
   - Добавить горизонтальный layout со спейсером слева (400px)
   - Внутри спейсера вызвать `render_canvas` с `view_mode = Split` (или вообще убрать view_mode из параметров)

3. **Изменить `ui.rs` для OutlineOnly режима:**
   - Добавить горизонтальный layout со спейсером справа (или центрировать)
   - Внутри вызвать `render_outline` с `view_mode = Split`

4. **Упростить сигнатуры функций:**
   - Убрать параметр `view_mode` из `render_canvas` и `render_outline`
   - Или всегда передавать `Split` (так как функции теперь не зависят от режима)

5. **Исправить вычисление кадров:**
   - Убедиться, что везде используется `ruler_rect.min.x` для вычисления кадров
   - Убрать условную логику с `timeline_rect.min.x` vs `ruler_rect.min.x`

## Детальный план изменений

### Шаг 1: Убрать условную логику из `render_canvas`
- Убрать строки 293-297 (вычисление `available_for_timeline`)
- Убрать строки 322-327 (спейсер перед ruler)
- Убрать строки 365-370 (спейсер перед ScrollArea)
- Всегда использовать полную ширину `ui.available_width()`

### Шаг 2: Убрать условную логику из `render_outline`
- Убрать строки 140-144 (условная ширина строк)
- Всегда использовать `ui.available_width()`

### Шаг 3: Изменить `ui.rs` для CanvasOnly
```rust
CanvasOnly => {
    ui.horizontal(|ui| {
        // Добавить спейсер слева (имитация outline колонки)
        ui.allocate_exact_size(
            Vec2::new(config.name_column_width, splitter_height),
            Sense::hover(),
        );
        
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.set_height(splitter_height);
            ui.set_max_height(splitter_height);
            timeline_actions = render_canvas(
                ui,
                comp_uuid,
                comp,
                &config,
                timeline_state,
                TimelineViewMode::Split, // Всегда Split, функция не зависит от режима
                |evt| event_bus.send(evt),
            );
        });
    });
}
```

### Шаг 4: Изменить `ui.rs` для OutlineOnly
```rust
OutlineOnly => {
    egui::CentralPanel::default().show_inside(ui, |ui| {
        ui.set_height(splitter_height);
        ui.set_max_height(splitter_height);
        render_outline(
            ui,
            comp_uuid,
            comp,
            &config,
            timeline_state,
            TimelineViewMode::Split, // Всегда Split
            |evt| event_bus.send(evt),
        );
    });
}
```

### Шаг 5: Исправить вычисление кадров
- В `render_canvas` при клике на timeline использовать `ruler_rect.min.x` вместо `timeline_rect.min.x`
- Убедиться, что `ruler_rect` доступен в области, где вычисляется кадр

## Ожидаемый результат

- CanvasOnly и OutlineOnly рисуются точно так же, как в Split режиме
- Split режим просто оборачивает готовые функции в панели
- Нет условной логики внутри функций рендеринга
- Правильное вычисление позиций кадров во всех режимах
- Кнопки B и N работают правильно в CanvasOnly режиме

