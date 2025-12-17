//! Viewport gizmo for layer transforms.
//!
//! Provides Move/Rotate/Scale manipulation gizmos using transform-gizmo-egui.

use eframe::egui;
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoMode, GizmoOrientation, GizmoExt,
    math::Transform,
    mint, EnumSet,
};
use uuid::Uuid;

use super::tool::ToolMode;
use super::ViewportState;
use crate::core::player::Player;
use crate::entities::attrs::AttrValue;
use crate::entities::Project;

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
        ui: &egui::Ui,
        viewport_state: &ViewportState,
        project: &mut Project,
        player: &Player,
    ) -> bool {
        let tool = ToolMode::from_str(&project.tool());

        // No gizmo in Select mode
        let gizmo_modes = match tool.to_gizmo_modes() {
            Some(modes) => modes,
            None => return false,
        };

        // Get active comp
        let comp_uuid = match player.active_comp() {
            Some(uuid) => uuid,
            None => return false,
        };

        // Get selected layers from active comp.
        // NOTE: Project.selection() refers to selected media nodes in the Project panel,
        // not layer instances on the timeline.
        let selected = project
            .with_comp(comp_uuid, |comp| comp.layer_selection.clone())
            .unwrap_or_default();
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
            modes: gizmo_modes,
            orientation: GizmoOrientation::Local,
            ..Default::default()
        });

        // Interact
        if let Some((_result, new_transforms)) = self.gizmo.interact(ui, &transforms) {
            self.apply_transforms(project, comp_uuid, &layer_data, new_transforms);
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
        new_transforms: Vec<Transform>,
    ) {
        for (i, new_t) in new_transforms.iter().enumerate() {
            if let Some((layer_uuid, _, _, _)) = layer_data.get(i) {
                let (new_pos, new_rot, new_scale) = gizmo_to_layer_transform(new_t);

                project.modify_comp(comp_uuid, |comp| {
                    comp.set_child_attrs(
                        *layer_uuid,
                        vec![
                            ("position", AttrValue::Vec3(new_pos)),
                            ("rotation", AttrValue::Vec3(new_rot)),
                            ("scale", AttrValue::Vec3(new_scale)),
                        ],
                    );
                });
            }
        }
    }
}

// ============================================================================
// ToolMode gizmo conversion
// ============================================================================

impl ToolMode {
    /// Convert to GizmoMode set for the library.
    /// Returns None for Select mode (no gizmo).
    pub fn to_gizmo_modes(self) -> Option<EnumSet<GizmoMode>> {
        match self {
            ToolMode::Select => None,
            ToolMode::Move => Some(
                EnumSet::from(GizmoMode::TranslateX)
                    | GizmoMode::TranslateY
                    | GizmoMode::TranslateZ
                    | GizmoMode::TranslateXY
                    | GizmoMode::TranslateXZ
                    | GizmoMode::TranslateYZ
            ),
            ToolMode::Rotate => Some(
                EnumSet::from(GizmoMode::RotateX)
                    | GizmoMode::RotateY
                    | GizmoMode::RotateZ
            ),
            ToolMode::Scale => Some(
                EnumSet::from(GizmoMode::ScaleX)
                    | GizmoMode::ScaleY
                    | GizmoMode::ScaleZ
                    | GizmoMode::ScaleUniform
            ),
        }
    }
}

// ============================================================================
// Matrix helpers
// ============================================================================

fn build_gizmo_matrices(
    viewport_state: &ViewportState,
    clip_rect: egui::Rect,
) -> (mint::RowMatrix4<f64>, mint::RowMatrix4<f64>) {
    use glam::{DMat4, DVec3};

    // View matrix: apply viewport pan and zoom
    let view = DMat4::from_scale_rotation_translation(
        DVec3::splat(viewport_state.zoom as f64),
        glam::DQuat::IDENTITY,
        DVec3::new(viewport_state.pan.x as f64, viewport_state.pan.y as f64, 0.0),
    );

    // Projection: orthographic, flip Y for screen coords
    let w = clip_rect.width() as f64;
    let h = clip_rect.height() as f64;
    let proj = DMat4::orthographic_rh(-w / 2.0, w / 2.0, h / 2.0, -h / 2.0, -1000.0, 1000.0);

    // Convert to row-major for gizmo library
    (to_row_matrix(view), to_row_matrix(proj))
}

fn to_row_matrix(m: glam::DMat4) -> mint::RowMatrix4<f64> {
    let cols = m.to_cols_array_2d();
    mint::RowMatrix4 {
        x: mint::Vector4 { x: cols[0][0], y: cols[1][0], z: cols[2][0], w: cols[3][0] },
        y: mint::Vector4 { x: cols[0][1], y: cols[1][1], z: cols[2][1], w: cols[3][1] },
        z: mint::Vector4 { x: cols[0][2], y: cols[1][2], z: cols[2][2], w: cols[3][2] },
        w: mint::Vector4 { x: cols[0][3], y: cols[1][3], z: cols[2][3], w: cols[3][3] },
    }
}

// ============================================================================
// Transform conversion
// ============================================================================

fn layer_to_gizmo_transform(
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
) -> Transform {
    use glam::{DQuat, DVec3};

    let translation = DVec3::new(position[0] as f64, position[1] as f64, position[2] as f64);
    let rotation_quat = DQuat::from_euler(
        glam::EulerRot::XYZ,
        rotation[0] as f64,
        rotation[1] as f64,
        rotation[2] as f64,
    );
    let scale_vec = DVec3::new(scale[0] as f64, scale[1] as f64, scale[2] as f64);

    Transform::from_scale_rotation_translation(
        mint::Vector3::from(scale_vec.to_array()),
        mint::Quaternion {
            v: mint::Vector3::from([rotation_quat.x, rotation_quat.y, rotation_quat.z]),
            s: rotation_quat.w,
        },
        mint::Vector3::from(translation.to_array()),
    )
}

fn gizmo_to_layer_transform(t: &Transform) -> ([f32; 3], [f32; 3], [f32; 3]) {
    use glam::{DQuat, DVec3};

    let translation = DVec3::new(t.translation.x, t.translation.y, t.translation.z);
    let rotation = DQuat::from_xyzw(t.rotation.v.x, t.rotation.v.y, t.rotation.v.z, t.rotation.s);
    let scale = DVec3::new(t.scale.x, t.scale.y, t.scale.z);

    let euler = rotation.to_euler(glam::EulerRot::XYZ);

    (
        [translation.x as f32, translation.y as f32, translation.z as f32],
        [euler.0 as f32, euler.1 as f32, euler.2 as f32],
        [scale.x as f32, scale.y as f32, scale.z as f32],
    )
}

fn get_layer_transform(
    project: &Project,
    comp_uuid: Uuid,
    layer_uuid: Uuid,
) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    project.with_comp(comp_uuid, |comp| {
        comp.get_layer(layer_uuid).map(|layer| {
            let pos = layer.attrs.get_vec3("position").unwrap_or([0.0, 0.0, 0.0]);
            let rot = layer.attrs.get_vec3("rotation").unwrap_or([0.0, 0.0, 0.0]);
            let scale = layer.attrs.get_vec3("scale").unwrap_or([1.0, 1.0, 1.0]);
            (pos, rot, scale)
        })
    }).flatten()
}
