# План рефакторинга UI (plan8.md)

## Требования из plan8.md
1. ✅ Переносим все кнопки кроме shader в timeline - они принадлежат ему
2. ✅ Shader оставляем во viewport или переносим куда-то в статусбар
3. ✅ В таймлайн добавляем слайдер от 0.1 до 4.0 с шагом 0.25 - зум таймлайна (от позиции playhead)
4. ✅ В playlist: Save, Load, Add Clip, Add Comp, Clear all - единый список клипов и компов
5. ✅ Double-click в Project Window → Comp становится current_comp
6. ✅ Drag'n'drop клипов/компов из Project Window на таймлайн
7. ✅ Timeline подсвечивает куда упадёт элемент
8. ✅ Drag'n'drop посылает сообщение: Player.current_comp.add_item(uuid, start_frame)
9. ✅ Перетаскивание слоёв влево-вправо (start_frame/end_frame), за края (trim_start/trim_end)
10. ✅ Курсор → двойная стрелка над краями слоёв

## Детальный план реализации

### 1. Timeline Zoom System
**Файлы:** `src/timeline.rs`

#### 1.1 Добавить TimelineState
```rust
pub struct TimelineState {
    pub zoom: f32,              // 1.0 = default, range 0.1..4.0
    pub pan_offset: f32,        // horizontal scroll offset
    pub selected_layer: Option<usize>,
    pub drag_state: Option<LayerDragState>,
}
```

#### 1.2 Обновить координатный маппинг
- frame_to_screen_x(frame) с учётом zoom/pan
- screen_x_to_frame(x) с учётом zoom/pan
- Формулы:
  - `x = rect.min.x + (frame - pan_offset) * ppf * zoom`
  - `frame = ((x - rect.min.x) / (ppf * zoom)) + pan_offset`

#### 1.3 Zoom относительно playhead
- При изменении zoom сохранять playhead в той же позиции экрана
- Пересчитывать pan_offset после изменения zoom

### 2. Timeline Toolbar (перенос кнопок)
**Файлы:** `src/ui.rs`, `src/timeline.rs`

#### 2.1 Удалить из timeline_panel верхней части
- Убрать transport controls (⏮ ▶ ⏹ ⏭)
- Убрать FPS/Shader/Loop строку

#### 2.2 Добавить toolbar в timeline widget
```rust
// В render_timeline():
ui.horizontal(|ui| {
    // Transport controls
    if ui.button("⏮").clicked() { action = TimelineAction::ToStart; }
    if ui.button(play_icon).clicked() { action = TimelineAction::TogglePlay; }
    if ui.button("⏹").clicked() { action = TimelineAction::Stop; }
    if ui.button("⏭").clicked() { action = TimelineAction::ToEnd; }

    ui.separator();

    // Zoom slider
    ui.label("Zoom:");
    ui.add(egui::Slider::new(&mut state.zoom, 0.1..=4.0).step_by(0.25));
});
```

#### 2.3 Обновить TimelineAction
```rust
pub enum TimelineAction {
    None,
    SetFrame(usize),
    SelectLayer(usize),
    ToStart,
    ToEnd,
    TogglePlay,
    Stop,
    // ... drag actions later
}
```

### 3. Shader в Viewport
**Файлы:** `src/ui.rs`

#### 3.1 Добавить shader selector в viewport overlay
```rust
// В render_viewport():
egui::Area::new("shader_overlay")
    .fixed_pos(egui::pos2(10.0, 10.0))
    .show(ctx, |ui| {
        ui.label("Shader:");
        egui::ComboBox::from_id_salt("shader_selector")
            .selected_text(&shaders.current_shader)
            .show_ui(ui, |ui| {
                for shader_name in shaders.shaders.keys() {
                    ui.selectable_value(&mut shaders.current_shader, shader_name.clone(), shader_name);
                }
            });
    });
```

### 4. Project Window - единый список
**Файлы:** `src/ui.rs`

#### 4.1 Новые кнопки
```rust
ui.horizontal(|ui| {
    if ui.button("Save").clicked() { actions.save_project = true; }
    if ui.button("Load").clicked() { actions.load_project = true; }
    if ui.button("Add Clip").clicked() { actions.add_clip = true; }
    if ui.button("Add Comp").clicked() { actions.new_comp = true; }
    if ui.button("Clear All").clicked() { actions.clear_all = true; }
});
```

#### 4.2 Единый список
```rust
ui.label("Items:");
egui::ScrollArea::vertical().show(ui, |ui| {
    // Clips first
    for clip_uuid in &project.clips_order {
        if let Some(MediaSource::Clip(clip)) = project.media.get(clip_uuid) {
            ui.horizontal(|ui| {
                ui.label("📹"); // Clip icon
                let response = ui.selectable_label(false, clip.pattern());

                // Drag source
                if response.hovered() && ui.input(|i| i.pointer.primary_down()) {
                    ui.memory_mut(|mem| {
                        mem.data.insert_temp("dragging_media", clip_uuid.clone());
                    });
                }

                if ui.button("✖").clicked() {
                    actions.remove_clip = Some(clip_uuid.clone());
                }
            });
        }
    }

    // Comps second
    for comp_uuid in &project.comps_order {
        if let Some(MediaSource::Comp(comp)) = project.media.get(comp_uuid) {
            let is_active = player.active_comp.as_ref() == Some(comp_uuid);
            ui.horizontal(|ui| {
                ui.label("🎬"); // Comp icon
                let response = ui.selectable_label(is_active, &comp.name);

                // Double-click to activate
                if response.double_clicked() {
                    actions.set_active_comp = Some(comp_uuid.clone());
                }

                // Drag source
                if response.hovered() && ui.input(|i| i.pointer.primary_down()) {
                    ui.memory_mut(|mem| {
                        mem.data.insert_temp("dragging_media", comp_uuid.clone());
                    });
                }

                if ui.button("✖").clicked() {
                    actions.remove_comp = Some(comp_uuid.clone());
                }
            });
        }
    }
});
```

### 5. Drag'n'Drop на Timeline
**Файлы:** `src/timeline.rs`, `src/comp.rs`

#### 5.1 Drop detection в timeline
```rust
// В render_timeline():
let timeline_response = ui.allocate_rect(timeline_rect, egui::Sense::click_and_drag());

// Check for drop
if timeline_response.hovered() {
    if let Some(dragging_uuid) = ui.memory(|mem| {
        mem.data.get_temp::<String>("dragging_media")
    }) {
        // Calculate drop position
        if let Some(pointer_pos) = ui.ctx().pointer_hover_pos() {
            let drop_frame = screen_x_to_frame(pointer_pos.x);

            // Snap to grid
            let snapped_frame = drop_frame; // Already rounds

            // Show drop preview
            let preview_rect = egui::Rect::from_min_max(
                egui::pos2(frame_to_screen_x(snapped_frame), timeline_rect.min.y),
                egui::pos2(frame_to_screen_x(snapped_frame + source_duration), timeline_rect.max.y),
            );
            ui.painter().rect_stroke(
                preview_rect,
                0.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 255, 0)),
            );

            // Handle drop
            if ui.input(|i| i.pointer.primary_released()) {
                ui.memory_mut(|mem| {
                    mem.data.remove::<String>("dragging_media");
                });
                return TimelineAction::AddLayer {
                    source_uuid: dragging_uuid,
                    start_frame: snapped_frame,
                };
            }
        }
    }
}
```

#### 5.2 Добавить метод в Comp
```rust
impl Comp {
    pub fn add_layer(&mut self, source_uuid: String, start_frame: usize, project: &Project) -> Result<()> {
        // Get source duration
        let source = project.media.get(&source_uuid)
            .ok_or_else(|| anyhow!("Source not found"))?;

        let duration = match source {
            MediaSource::Clip(clip) => clip.len(),
            MediaSource::Comp(comp) => comp.total_frames(),
        };

        // Create new layer
        let mut layer = Layer::new(source_uuid);
        layer.attrs.set("start", AttrValue::UInt(start_frame as u32));
        layer.attrs.set("end", AttrValue::UInt((start_frame + duration - 1) as u32));
        layer.attrs.set("trim_start", AttrValue::Int(0));
        layer.attrs.set("trim_end", AttrValue::Int(0));

        // Add to layers (top)
        self.layers.push(layer);

        // Emit event
        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }
}
```

#### 5.3 Обновить TimelineAction
```rust
pub enum TimelineAction {
    // ... existing ...
    AddLayer { source_uuid: String, start_frame: usize },
}
```

### 6. Перетаскивание слоёв
**Файлы:** `src/timeline.rs`, `src/comp.rs`

#### 6.1 LayerDragState
```rust
#[derive(Clone)]
pub enum LayerDragState {
    MovingLayer {
        layer_idx: usize,
        initial_start: usize,
        initial_end: usize,
        drag_start_x: f32,
    },
    TrimStart {
        layer_idx: usize,
        initial_trim: i32,
        drag_start_x: f32,
    },
    TrimEnd {
        layer_idx: usize,
        initial_trim: i32,
        drag_start_x: f32,
    },
}
```

#### 6.2 Определение режима драга
```rust
const EDGE_THRESHOLD: f32 = 10.0; // pixels

fn detect_drag_mode(pointer_x: f32, layer_rect: egui::Rect) -> DragMode {
    let left_edge = layer_rect.min.x;
    let right_edge = layer_rect.max.x;

    if (pointer_x - left_edge).abs() < EDGE_THRESHOLD {
        DragMode::TrimStart
    } else if (pointer_x - right_edge).abs() < EDGE_THRESHOLD {
        DragMode::TrimEnd
    } else {
        DragMode::Move
    }
}
```

#### 6.3 Курсоры
```rust
// При hover над слоем:
match detect_drag_mode(pointer_x, layer_rect) {
    DragMode::TrimStart | DragMode::TrimEnd => {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    DragMode::Move => {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

// При активном драге:
if state.drag_state.is_some() {
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
}
```

#### 6.4 Логика драга
```rust
// Mouse down - start drag
if layer_response.drag_started() {
    let mode = detect_drag_mode(pointer_x, layer_rect);
    state.drag_state = Some(match mode {
        DragMode::Move => LayerDragState::MovingLayer {
            layer_idx: i,
            initial_start: layer.get_start(),
            initial_end: layer.get_end(),
            drag_start_x: pointer_x,
        },
        DragMode::TrimStart => LayerDragState::TrimStart {
            layer_idx: i,
            initial_trim: layer.get_trim_start(),
            drag_start_x: pointer_x,
        },
        DragMode::TrimEnd => LayerDragState::TrimEnd {
            layer_idx: i,
            initial_trim: layer.get_trim_end(),
            drag_start_x: pointer_x,
        },
    });
}

// Mouse move - update drag
if let Some(drag_state) = &state.drag_state {
    let delta_x = pointer_x - drag_state.drag_start_x();
    let delta_frames = (delta_x / (config.pixels_per_frame * state.zoom)) as i32;

    match drag_state {
        LayerDragState::MovingLayer { layer_idx, initial_start, .. } => {
            let new_start = (*initial_start as i32 + delta_frames).max(0) as usize;
            return TimelineAction::MoveLayer {
                layer_idx: *layer_idx,
                new_start,
            };
        }
        LayerDragState::TrimStart { layer_idx, initial_trim, .. } => {
            let new_trim = *initial_trim + delta_frames;
            return TimelineAction::TrimLayerStart {
                layer_idx: *layer_idx,
                new_trim,
            };
        }
        LayerDragState::TrimEnd { layer_idx, initial_trim, .. } => {
            let new_trim = *initial_trim - delta_frames;
            return TimelineAction::TrimLayerEnd {
                layer_idx: *layer_idx,
                new_trim,
            };
        }
    }
}

// Mouse up - end drag
if ui.input(|i| i.pointer.primary_released()) {
    state.drag_state = None;
}
```

#### 6.5 Методы в Comp
```rust
impl Comp {
    pub fn move_layer(&mut self, idx: usize, new_start: usize) -> Result<()> {
        let layer = self.layers.get_mut(idx)
            .ok_or_else(|| anyhow!("Layer not found"))?;

        let old_start = layer.get_start();
        let duration = layer.get_end() - old_start;

        layer.attrs.set("start", AttrValue::UInt(new_start as u32));
        layer.attrs.set("end", AttrValue::UInt((new_start + duration) as u32));

        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    pub fn trim_layer_start(&mut self, idx: usize, new_trim: i32) -> Result<()> {
        let layer = self.layers.get_mut(idx)
            .ok_or_else(|| anyhow!("Layer not found"))?;

        layer.attrs.set("trim_start", AttrValue::Int(new_trim));

        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }

    pub fn trim_layer_end(&mut self, idx: usize, new_trim: i32) -> Result<()> {
        let layer = self.layers.get_mut(idx)
            .ok_or_else(|| anyhow!("Layer not found"))?;

        layer.attrs.set("trim_end", AttrValue::Int(new_trim));

        self.event_sender.emit(CompEvent::LayersChanged {
            comp_uuid: self.uuid.clone(),
        });

        Ok(())
    }
}
```

#### 6.6 Обновить TimelineAction
```rust
pub enum TimelineAction {
    // ... existing ...
    MoveLayer { layer_idx: usize, new_start: usize },
    TrimLayerStart { layer_idx: usize, new_trim: i32 },
    TrimLayerEnd { layer_idx: usize, new_trim: i32 },
}
```

### 7. Helper методы в Layer
**Файл:** `src/layer.rs`

```rust
impl Layer {
    pub fn get_start(&self) -> usize {
        self.attrs.get_uint("start").unwrap_or(0) as usize
    }

    pub fn get_end(&self) -> usize {
        self.attrs.get_uint("end").unwrap_or(0) as usize
    }

    pub fn get_trim_start(&self) -> i32 {
        self.attrs.get_int("trim_start").unwrap_or(0)
    }

    pub fn get_trim_end(&self) -> i32 {
        self.attrs.get_int("trim_end").unwrap_or(0)
    }
}
```

## Порядок реализации

1. ✅ **Timeline zoom system** - базовая инфраструктура
2. ✅ **Timeline toolbar** - перенос кнопок
3. ✅ **Shader в viewport** - overlay
4. ✅ **Project Window refactor** - единый список + кнопки
5. ✅ **Drag'n'drop на timeline** - из Project Window
6. ✅ **Layer dragging** - move + trim
7. ✅ **Testing** - все interaction patterns

## Тестирование

- [ ] Зум таймлайна сохраняет playhead на месте
- [ ] Кнопки в timeline toolbar работают
- [ ] Shader selector в viewport работает
- [ ] Project Window показывает единый список
- [ ] Double-click активирует композицию
- [ ] Drag клипа на timeline создаёт слой
- [ ] Drag композиции на timeline создаёт слой
- [ ] Drop preview показывается корректно
- [ ] Перетаскивание слоя меняет позицию
- [ ] Trim левого края работает
- [ ] Trim правого края работает
- [ ] Курсоры меняются корректно
