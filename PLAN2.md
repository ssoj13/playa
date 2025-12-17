# PLAN2: 3D Манипуляторы (Gizmos)

## Цель
Реализовать Move/Rotate/Scale манипуляторы как в Maya для Layer transforms.

---

## 1. Структура файлов

```
src/widgets/viewport/
├── mod.rs              # + pub mod tool; pub mod gizmo;
├── viewport.rs         # ViewportState (существует)
├── viewport_ui.rs      # render() - добавить вызов gizmo
├── viewport_events.rs  # (существует)
├── renderer.rs         # OpenGL (существует)
├── tool.rs             # NEW: ToolMode, SetToolEvent
└── gizmo.rs            # NEW: GizmoState, render_gizmo()

src/entities/
├── attr_schemas.rs     # + "current_tool" в PROJECT_SCHEMA
└── project.rs          # current_tool через attrs

src/main.rs             # gizmo_state field, Q/W/E/R hotkeys
```

---

## 2. Библиотека

```toml
# Cargo.toml
[dependencies]
transform-gizmo-egui = "0.5"
```

---

## 3. ToolMode (src/widgets/viewport/tool.rs)

```rust
//! Viewport tool modes and events.

use crate::core::event_bus::Event;

/// Active tool mode for viewport manipulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ToolMode {
    #[default]
    Select,   // Q - viewport scrubber, no gizmo
    Move,     // W - translate
    Rotate,   // E - rotate
    Scale,    // R - scale
}

impl ToolMode {
    /// Convert to string for attr storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolMode::Select => "select",
            ToolMode::Move => "move",
            ToolMode::Rotate => "rotate",
            ToolMode::Scale => "scale",
        }
    }

    /// Parse from attr string.
    pub fn from_str(s: &str) -> Self {
        match s {
            "move" => ToolMode::Move,
            "rotate" => ToolMode::Rotate,
            "scale" => ToolMode::Scale,
            _ => ToolMode::Select,
        }
    }

    /// Convert to GizmoMode for the library.
    pub fn to_gizmo_mode(self) -> Option<transform_gizmo_egui::GizmoMode> {
        use transform_gizmo_egui::GizmoMode;
        match self {
            ToolMode::Select => None,
            ToolMode::Move => Some(GizmoMode::Translate),
            ToolMode::Rotate => Some(GizmoMode::Rotate),
            ToolMode::Scale => Some(GizmoMode::Scale),
        }
    }
}

/// Event to change current tool.
pub struct SetToolEvent(pub ToolMode);

impl Event for SetToolEvent {
    fn apply(&self, ctx: &mut crate::core::event_bus::AppContext) {
        ctx.project.set_attr("current_tool",
            crate::entities::attrs::AttrValue::String(self.0.as_str().to_string())
        );
    }
}
```

---

## 4. Project Schema (src/entities/attr_schemas.rs)

```rust
// Добавить в PROJECT_DEFS:
AttrDef::new("current_tool", AttrType::String, 0),  // Non-DAG, just UI state
```

---

## 5. Project current_tool accessor (src/entities/project.rs)

```rust
impl Project {
    /// Get current tool mode.
    pub fn current_tool(&self) -> ToolMode {
        self.attrs.get_str("current_tool")
            .map(|s| ToolMode::from_str(s))
            .unwrap_or_default()
    }

    /// Set current tool mode.
    pub fn set_current_tool(&mut self, tool: ToolMode) {
        self.set_attr("current_tool", AttrValue::String(tool.as_str().to_string()));
    }
}
```

---

## 6. GizmoState (src/widgets/viewport/gizmo.rs)

```rust
//! Viewport gizmo for layer transforms.

use egui::Ui;
use transform_gizmo_egui::prelude::*;
use uuid::Uuid;

use super::tool::ToolMode;
use super::ViewportState;
use crate::entities::Project;
use crate::core::player::Player;

/// Gizmo state - lives in PlayaApp, not saved.
pub struct GizmoState {
    gizmo: Gizmo,
}

impl Default for GizmoState {
    fn default() -> Self {
        Self {
            gizmo: Gizmo::default(),
        }
    }
}

impl GizmoState {
    /// Render gizmo and handle interaction.
    /// Returns true if gizmo consumed the input.
    pub fn render(
        &mut self,
        ui: &Ui,
        viewport_state: &ViewportState,
        project: &mut Project,
        player: &Player,
    ) -> bool {
        let tool = project.current_tool();

        // No gizmo in Select mode
        let gizmo_mode = match tool.to_gizmo_mode() {
            Some(mode) => mode,
            None => return false,
        };

        // Get active comp
        let comp_uuid = match player.active_comp() {
            Some(uuid) => uuid,
            None => return false,
        };

        // Get selected layers
        let selected = project.selection().layers_in_comp(comp_uuid);
        if selected.is_empty() {
            return false;
        }

        // Collect layer transforms
        let (transforms, layer_data) = self.collect_transforms(project, comp_uuid, &selected);
        if transforms.is_empty() {
            return false;
        }

        // Build matrices
        let (view, proj) = build_gizmo_matrices(viewport_state, ui.clip_rect());

        // Configure gizmo
        self.gizmo.update_config(GizmoConfig {
            view_matrix: view,
            projection_matrix: proj,
            viewport: ui.clip_rect(),
            modes: gizmo_mode.into(),
            orientation: GizmoOrientation::Local,
            ..Default::default()
        });

        // Interact
        if let Some((_result, new_transforms)) = self.gizmo.interact(ui, &transforms) {
            self.apply_transforms(project, comp_uuid, &layer_data, &new_transforms);
            return true;
        }

        false
    }

    fn collect_transforms(
        &self,
        project: &Project,
        comp_uuid: Uuid,
        selected: &[Uuid],
    ) -> (Vec<Transform>, Vec<(Uuid, [f32; 3], [f32; 3], [f32; 3])>) {
        let mut transforms = Vec::new();
        let mut layer_data = Vec::new();

        for &layer_uuid in selected {
            if let Some((pos, rot, scale)) = get_layer_transform(project, comp_uuid, layer_uuid) {
                transforms.push(layer_to_gizmo_transform(pos, rot, scale));
                layer_data.push((layer_uuid, pos, rot, scale));
            }
        }

        (transforms, layer_data)
    }

    fn apply_transforms(
        &self,
        project: &mut Project,
        comp_uuid: Uuid,
        layer_data: &[(Uuid, [f32; 3], [f32; 3], [f32; 3])],
        new_transforms: &[Transform],
    ) {
        use crate::entities::attrs::AttrValue;

        for (i, new_t) in new_transforms.iter().enumerate() {
            if let Some((layer_uuid, _, _, _)) = layer_data.get(i) {
                let (new_pos, new_rot, new_scale) = gizmo_to_layer_transform(new_t);

                project.modify_comp(comp_uuid, |comp| {
                    comp.set_child_attr(*layer_uuid, "position", AttrValue::Vec3(new_pos));
                    comp.set_child_attr(*layer_uuid, "rotation", AttrValue::Vec3(new_rot));
                    comp.set_child_attr(*layer_uuid, "scale", AttrValue::Vec3(new_scale));
                });
            }
        }
    }
}

// ============================================================================
// Matrix helpers
// ============================================================================

fn build_gizmo_matrices(
    viewport_state: &ViewportState,
    clip_rect: egui::Rect,
) -> (mint::ColumnMatrix4<f32>, mint::ColumnMatrix4<f32>) {
    use glam::{Mat4, Vec3};

    // View matrix: apply viewport pan and zoom
    let view = Mat4::from_scale_rotation_translation(
        Vec3::splat(viewport_state.zoom),
        glam::Quat::IDENTITY,
        Vec3::new(viewport_state.pan.x, viewport_state.pan.y, 0.0),
    );

    // Projection: orthographic, flip Y for screen coords
    let w = clip_rect.width();
    let h = clip_rect.height();
    let proj = Mat4::orthographic_rh(-w / 2.0, w / 2.0, h / 2.0, -h / 2.0, -1000.0, 1000.0);

    (view.to_cols_array_2d().into(), proj.to_cols_array_2d().into())
}

// ============================================================================
// Transform conversion
// ============================================================================

fn layer_to_gizmo_transform(
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
) -> Transform {
    use glam::{Quat, Vec3};

    let translation = Vec3::from(position);
    let rotation_quat = Quat::from_euler(
        glam::EulerRot::XYZ,
        rotation[0],
        rotation[1],
        rotation[2],
    );
    let scale_vec = Vec3::from(scale);

    Transform::from_scale_rotation_translation(
        mint::Vector3::from(scale_vec.to_array()),
        mint::Quaternion::from(rotation_quat.to_array()),
        mint::Vector3::from(translation.to_array()),
    )
}

fn gizmo_to_layer_transform(t: &Transform) -> ([f32; 3], [f32; 3], [f32; 3]) {
    use glam::{Quat, Vec3};

    let translation = Vec3::new(t.translation.x, t.translation.y, t.translation.z);
    let rotation = Quat::from_xyzw(t.rotation.v.x, t.rotation.v.y, t.rotation.v.z, t.rotation.s);
    let scale = Vec3::new(t.scale.x, t.scale.y, t.scale.z);

    let euler = rotation.to_euler(glam::EulerRot::XYZ);

    (
        translation.to_array(),
        [euler.0, euler.1, euler.2],
        scale.to_array(),
    )
}

fn get_layer_transform(
    project: &Project,
    comp_uuid: Uuid,
    layer_uuid: Uuid,
) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    project.with_comp(comp_uuid, |comp| {
        comp.with_child(layer_uuid, |layer| {
            let pos = layer.attrs().get_vec3("position").unwrap_or([0.0, 0.0, 0.0]);
            let rot = layer.attrs().get_vec3("rotation").unwrap_or([0.0, 0.0, 0.0]);
            let scale = layer.attrs().get_vec3("scale").unwrap_or([1.0, 1.0, 1.0]);
            (pos, rot, scale)
        })
    }).flatten()
}
```

---

## 7. Hotkeys (main.rs)

### 7.1 Убрать Q = Quit
```rust
// Было:
if input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::Q) {

// Станет:
if input.key_pressed(egui::Key::Escape) {
```

### 7.2 Добавить tool hotkeys (глобальные)
```rust
// В handle_global_hotkeys() или где hotkeys обрабатываются:
use crate::widgets::viewport::tool::{ToolMode, SetToolEvent};

if input.key_pressed(egui::Key::Q) {
    event_bus.send(SetToolEvent(ToolMode::Select));
}
if input.key_pressed(egui::Key::W) && !input.modifiers.ctrl {
    event_bus.send(SetToolEvent(ToolMode::Move));
}
if input.key_pressed(egui::Key::E) {
    event_bus.send(SetToolEvent(ToolMode::Rotate));
}
if input.key_pressed(egui::Key::R) && !input.modifiers.ctrl {
    event_bus.send(SetToolEvent(ToolMode::Scale));
}
```

Note: Ctrl+W/Ctrl+R могут быть заняты другими hotkeys.

---

## 8. Интеграция в viewport_ui.rs

```rust
// В render() после image rendering, перед overlays:
use super::gizmo::GizmoState;

// gizmo_state передаётся как параметр
let gizmo_consumed = gizmo_state.render(ui, viewport_state, project, player);

// Если gizmo consumed input, не обрабатывать scrubbing
if !gizmo_consumed {
    if let Some(frame_idx) = viewport_state.handle_scrubbing(...) {
        // ...
    }
}
```

---

## 9. PlayaApp changes (main.rs)

```rust
// В struct PlayaApp:
gizmo_state: GizmoState,

// В PlayaApp::new():
gizmo_state: GizmoState::default(),

// Передать в render_viewport_tab():
// &mut self.gizmo_state
```

---

## 10. Файлы - итого

| Файл | Действие | Что делаем |
|------|----------|------------|
| Cargo.toml | EDIT | + transform-gizmo-egui = "0.5" |
| src/widgets/viewport/mod.rs | EDIT | + pub mod tool; pub mod gizmo; |
| src/widgets/viewport/tool.rs | CREATE | ToolMode, SetToolEvent |
| src/widgets/viewport/gizmo.rs | CREATE | GizmoState, render, helpers |
| src/entities/attr_schemas.rs | EDIT | + "current_tool" в PROJECT_SCHEMA |
| src/entities/project.rs | EDIT | + current_tool(), set_current_tool() |
| src/widgets/viewport/viewport_ui.rs | EDIT | + gizmo.render() call |
| src/main.rs | EDIT | gizmo_state field, Q/W/E/R hotkeys, remove Q=quit |

---

## 11. Порядок реализации

### Phase 1: Tool System (без gizmo)
1. `attr_schemas.rs` - добавить "current_tool"
2. `project.rs` - current_tool() accessor
3. `viewport/tool.rs` - ToolMode, SetToolEvent
4. `viewport/mod.rs` - pub mod tool
5. `main.rs` - Q/W/E/R hotkeys, remove Q=quit
6. **Test:** hotkeys меняют current_tool (видно в attrs panel)

### Phase 2: Gizmo Integration
1. Cargo.toml - transform-gizmo-egui
2. `viewport/gizmo.rs` - GizmoState
3. `viewport/mod.rs` - pub mod gizmo
4. `main.rs` - gizmo_state field
5. `viewport_ui.rs` - вызов gizmo.render()
6. **Test:** gizmo появляется при выборе слоя + W/E/R

### Phase 3: Transform Application
1. Проверить matrix math
2. Test dragging - layer двигается
3. Fix coordinate spaces если надо

### Phase 4: Polish
1. Status bar: show current tool name
2. Multi-select center calculation
3. Gizmo не блокирует pan/zoom когда не над ним

---

## 12. Потенциальные проблемы

1. **Coordinate spaces** - gizmo работает в 3D, мы в 2D. Может потребоваться tweaking матриц.

2. **Gizmo size** - может быть слишком большим/маленьким. GizmoConfig имеет параметры размера.

3. **Input conflict** - gizmo может перехватывать input когда не нужно. Проверить hovered state.

4. **Multi-select** - библиотека принимает массив transforms, но рисует один gizmo. Нужно посмотреть как она это делает.
