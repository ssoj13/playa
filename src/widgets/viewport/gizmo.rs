//! Viewport gizmo for layer transforms.
//!
//! Provides Move/Rotate/Scale manipulation gizmos using transform-gizmo-egui.

use eframe::egui;
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoMode, GizmoOrientation, GizmoVisuals, GizmoExt,
    math::Transform,
    mint, EnumSet,
};
use uuid::Uuid;

use super::tool::ToolMode;
use super::ViewportState;
use crate::core::event_bus::BoxedEvent;
use crate::core::player::Player;
use crate::entities::comp_events::SetLayerTransformsEvent;
use crate::entities::Project;
use crate::entities::keys::{A_HEIGHT, A_PIVOT, A_POSITION, A_ROTATION, A_SCALE, A_WIDTH};
use crate::entities::transform;
use super::coords;

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
        project: &Project,
        player: &Player,
    ) -> (bool, Vec<BoxedEvent>) {
        let tool = ToolMode::from_str(&project.tool());

        // No gizmo in Select mode
        let gizmo_modes = match tool.to_gizmo_modes() {
            Some(modes) => modes,
            None => return (false, Vec::new()),
        };

        // Get active comp
        let comp_uuid = match player.active_comp() {
            Some(uuid) => uuid,
            None => return (false, Vec::new()),
        };

        // Get selected layers from active comp.
        // NOTE: Project.selection() refers to selected media nodes in the Project panel,
        // not layer instances on the timeline.
        let selected = project
            .with_comp(comp_uuid, |comp| comp.layer_selection.clone())
            .unwrap_or_default();
        if selected.is_empty() {
            return (false, Vec::new());
        }

        // Collect layer transforms
        let (transforms, layer_data) = self.collect_transforms(tool, project, comp_uuid, &selected);
        if transforms.is_empty() {
            return (false, Vec::new());
        }

        // Build matrices
        let (view, proj) = build_gizmo_matrices(viewport_state, ui.clip_rect());

        // Configure gizmo
        let gizmo_prefs = project.gizmo_prefs();
        self.gizmo.update_config(GizmoConfig {
            view_matrix: view,
            projection_matrix: proj,
            viewport: ui.clip_rect(),
            modes: gizmo_modes,
            orientation: GizmoOrientation::Local,
            visuals: GizmoVisuals {
                gizmo_size: gizmo_prefs.pref_manip_size,
                stroke_width: gizmo_prefs.pref_manip_stroke_width,
                inactive_alpha: gizmo_prefs.pref_manip_inactive_alpha,
                highlight_alpha: gizmo_prefs.pref_manip_highlight_alpha,
                ..Default::default()
            },
            ..Default::default()
        });

        // Interact
        if let Some((_result, new_transforms)) = self.gizmo.interact(ui, &transforms) {
            if let Some(event) =
                self.build_transform_event(tool, comp_uuid, &layer_data, &new_transforms)
            {
                return (true, vec![Box::new(event)]);
            }
            return (true, Vec::new());
        }

        (false, Vec::new())
    }

    fn collect_transforms(
        &self,
        tool: ToolMode,
        project: &Project,
        comp_uuid: Uuid,
        selected: &[Uuid],
    ) -> (Vec<Transform>, Vec<(Uuid, [f32; 3], [f32; 3], [f32; 3], [f32; 3], (usize, usize))>) {
        let mut transforms = Vec::new();
        let mut layer_data = Vec::new();

        for &layer_uuid in selected {
            if let Some((pos, rot, scale, pivot, src_size)) =
                get_layer_transform(project, comp_uuid, layer_uuid)
            {
                transforms.push(layer_to_gizmo_transform(tool, pos, rot, scale, pivot, src_size));
                layer_data.push((layer_uuid, pos, rot, scale, pivot, src_size));
            }
        }

        (transforms, layer_data)
    }

    fn build_transform_event(
        &self,
        tool: ToolMode,
        comp_uuid: Uuid,
        layer_data: &[(Uuid, [f32; 3], [f32; 3], [f32; 3], [f32; 3], (usize, usize))],
        new_transforms: &[Transform],
    ) -> Option<SetLayerTransformsEvent> {
        let mut updates = Vec::new();

        for (i, new_t) in new_transforms.iter().enumerate() {
            let Some((layer_uuid, old_pos, old_rot, old_scale, old_pivot, src_size)) =
                layer_data.get(i)
            else {
                continue;
            };
            let (gizmo_pivot, gizmo_rot, gizmo_scale) = gizmo_to_layer_transform(new_t);

            // We normalize the input transform we pass into transform-gizmo (see
            // `layer_to_gizmo_transform`) so gizmo rings/handles render correctly in 2D.
            // Because of that, we must merge the output back into the original layer
            // attrs, updating only the channel that the current tool edits.
            let (new_pos, new_rot, new_scale) = match tool {
                ToolMode::Move => {
                    let pivot_pos = glam::Vec2::new(gizmo_pivot[0], gizmo_pivot[1]);
                    let pos = transform::position_from_pivot(pivot_pos, *old_pivot, *src_size, old_pos[2]);
                    (pos, *old_rot, *old_scale)
                }
                ToolMode::Rotate => (
                    *old_pos,
                    [old_rot[0], old_rot[1], gizmo_rot[2]],
                    *old_scale,
                ),
                ToolMode::Scale => (
                    *old_pos,
                    *old_rot,
                    [gizmo_scale[0], gizmo_scale[1], old_scale[2]],
                ),
                ToolMode::Select => (*old_pos, *old_rot, *old_scale),
            };

            // Avoid emitting redundant updates when values haven't changed meaningfully.
            if approx_vec3_equal(*old_pos, new_pos)
                && approx_vec3_equal(*old_rot, new_rot)
                && approx_vec3_equal(*old_scale, new_scale)
            {
                continue;
            }

            updates.push((*layer_uuid, new_pos, new_rot, new_scale));
        }

        if updates.is_empty() {
            return None;
        }

        Some(SetLayerTransformsEvent { comp_uuid, updates })
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
                    | GizmoMode::TranslateXY
                    | GizmoMode::TranslateView
            ),
            ToolMode::Rotate => Some(EnumSet::from(GizmoMode::RotateZ)),
            ToolMode::Scale => Some(
                EnumSet::from(GizmoMode::ScaleX)
                    | GizmoMode::ScaleY
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

    // Projection: orthographic. Do NOT flip Y here: transform-gizmo already flips Y
    // when converting NDC to screen coordinates (see transform_gizmo::math::world_to_screen).
    let w = clip_rect.width() as f64;
    let h = clip_rect.height() as f64;
    let proj = DMat4::orthographic_rh(-w / 2.0, w / 2.0, -h / 2.0, h / 2.0, -1000.0, 1000.0);

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
    tool: ToolMode,
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
    src_size: (usize, usize),
) -> Transform {
    use glam::{DQuat, DVec3};

    // Layer transform attributes follow AE-style 2D conventions:
    // - position.y is "down" in image pixel space
    // - rotation.z positive rotates clockwise on screen (because image y is down)
    //
    // Viewport camera uses a conventional y-up space, and transform-gizmo expects
    // a y-up view/projection. Convert here.
    //
    // NOTE: We intentionally normalize the transform we pass into transform-gizmo:
    // - ignore rotation.x/y (we're currently a 2D tool; compositor only uses rot.z)
    // - for Move/Rotate, force uniform scale=1 to prevent rings/handles becoming oval
    //   when the layer has non-uniform scale (scale.x != scale.y).
    // - translation is the layer pivot position in comp space (position + center + pivot offset).
    //
    // This is purely a *visual/input normalization* for gizmo interaction; we merge the
    // output back into the original layer attrs and only write the edited channel.
    let pivot_pos = transform::layer_pivot_in_comp(position, pivot, src_size);
    let translation = DVec3::new(pivot_pos.x as f64, -(pivot_pos.y as f64), 0.0);
    // Layer rotation attrs are stored in DEGREES. Gizmo expects radians.
    // In 2D we only care about Z.
    let rotation_quat = DQuat::from_euler(
        glam::EulerRot::XYZ,
        0.0,
        0.0,
        -((rotation[2] as f64).to_radians()),
    );
    let scale_vec = match tool {
        ToolMode::Scale => DVec3::new(scale[0] as f64, scale[1] as f64, 1.0),
        ToolMode::Move | ToolMode::Rotate | ToolMode::Select => DVec3::splat(1.0),
    };

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

    // Layer rotation attrs are stored in DEGREES.
    (
        [translation.x as f32, coords::flip_y_f64(translation.y) as f32, translation.z as f32],
        [
            (euler.0 as f32).to_degrees(),
            (euler.1 as f32).to_degrees(),
            (-(euler.2 as f32)).to_degrees(),
        ],
        [scale.x as f32, scale.y as f32, scale.z as f32],
    )
}

fn get_layer_transform(
    project: &Project,
    comp_uuid: Uuid,
    layer_uuid: Uuid,
) -> Option<([f32; 3], [f32; 3], [f32; 3], [f32; 3], (usize, usize))> {
    project.with_comp(comp_uuid, |comp| {
        comp.get_layer(layer_uuid).map(|layer| {
            let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
            let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
            let scale = layer.attrs.get_vec3(A_SCALE).unwrap_or([1.0, 1.0, 1.0]);
            let pivot = layer.attrs.get_vec3(A_PIVOT).unwrap_or([0.0, 0.0, 0.0]);
            let width = layer.attrs.get_u32(A_WIDTH).unwrap_or(1).max(1) as usize;
            let height = layer.attrs.get_u32(A_HEIGHT).unwrap_or(1).max(1) as usize;
            (pos, rot, scale, pivot, (width, height))
        })
    }).flatten()
}

#[inline]
fn approx_vec3_equal(a: [f32; 3], b: [f32; 3]) -> bool {
    // Keep epsilon conservative: gizmo drags are continuous; this just avoids
    // emitting identical values due to float roundtrips.
    const EPS: f32 = 1.0e-6;
    (a[0] - b[0]).abs() <= EPS && (a[1] - b[1]).abs() <= EPS && (a[2] - b[2]).abs() <= EPS
}
