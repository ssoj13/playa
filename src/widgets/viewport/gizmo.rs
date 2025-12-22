//! Viewport gizmo for layer transforms.
//!
//! Provides Move/Rotate/Scale manipulation gizmos using transform-gizmo-egui.
//!
//! ## Coordinate System
//!
//! Layer `position` is in **frame space** (centered, Y-up pixels):
//! - Origin at CENTER of comp, +Y up
//! - `position = (0, 0, 0)` = layer centered
//! - `position = (100, 50, 0)` = layer 100px right, 50px up from center
//!
//! Renderer and gizmo both use the same view matrix (zoom + pan), so layer
//! positions in frame space map directly to gizmo world coordinates.
//! See `ViewportState::get_view_matrix()` and `build_gizmo_matrices()`.

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
use crate::entities::node::Node;  // for dim()
use crate::entities::Project;
use crate::entities::keys::{A_POSITION, A_ROTATION, A_SCALE};
use crate::entities::space;


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
        let (transforms, layer_data) =
            self.collect_transforms(tool, project, comp_uuid, &selected);
        if transforms.is_empty() {
            return (false, Vec::new());
        }

        // Get camera matrices if 3D camera is active
        let frame_idx = player.current_frame(project);
        let camera_matrices = get_camera_matrices(project, comp_uuid, frame_idx, ui.clip_rect());

        // Build matrices (uses camera for 3D, ortho for 2D)
        let (view, proj) = build_gizmo_matrices(viewport_state, ui.clip_rect(), camera_matrices);

        // Configure gizmo
        let gizmo_prefs = project.gizmo_prefs();
        
        // Shift enables snapping
        let snapping = ui.input(|i| i.modifiers.shift);
        
        self.gizmo.update_config(GizmoConfig {
            view_matrix: view,
            projection_matrix: proj,
            viewport: ui.clip_rect(),
            modes: gizmo_modes,
            orientation: GizmoOrientation::Local,
            snapping,
            snap_angle: 5.0_f32.to_radians(),    // 5 degrees
            snap_distance: 10.0,                  // 10 units
            snap_scale: 0.1,                      // 0.1 step
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
    ) -> (Vec<Transform>, Vec<(Uuid, [f32; 3], [f32; 3], [f32; 3])>) {
        let mut transforms = Vec::new();
        let mut layer_data = Vec::new();

        for &layer_uuid in selected {
            if let Some((pos, rot, scale)) = get_layer_transform(project, comp_uuid, layer_uuid)
            {
                transforms.push(layer_to_gizmo_transform(tool, pos, rot, scale));
                layer_data.push((layer_uuid, pos, rot, scale));
            }
        }

        (transforms, layer_data)
    }

    fn build_transform_event(
        &self,
        tool: ToolMode,
        comp_uuid: Uuid,
        layer_data: &[(Uuid, [f32; 3], [f32; 3], [f32; 3])],
        new_transforms: &[Transform],
    ) -> Option<SetLayerTransformsEvent> {
        let mut updates = Vec::new();

        for (i, new_t) in new_transforms.iter().enumerate() {
            let Some((layer_uuid, old_pos, old_rot, old_scale)) = layer_data.get(i)
            else {
                continue;
            };
            let (gizmo_pos, gizmo_rot, gizmo_scale) = gizmo_to_layer_transform(new_t);

            // Update only the channel that the current tool edits.
            // With 3D support, we now take all three components for each tool.
            let (new_pos, new_rot, new_scale) = match tool {
                ToolMode::Move => (gizmo_pos, *old_rot, *old_scale),
                ToolMode::Rotate => (*old_pos, gizmo_rot, *old_scale),
                ToolMode::Scale => (*old_pos, *old_rot, gizmo_scale),
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
    ///
    /// # 3D Support
    ///
    /// All three axes are enabled for each tool:
    /// - Move: TranslateX/Y/Z + XY plane + View plane
    /// - Rotate: RotateX/Y/Z (full 3D rotation)
    /// - Scale: ScaleX/Y/Z + Uniform
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
                    | GizmoMode::TranslateView
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

/// Get camera view and projection matrices if 3D camera is active.
///
/// Returns (view, projection) separately for gizmo library.
/// Returns None if no camera in comp (2D mode).
fn get_camera_matrices(
    project: &Project,
    comp_uuid: Uuid,
    frame_idx: i32,
    _viewport_rect: egui::Rect,
) -> Option<(glam::Mat4, glam::Mat4)> {
    let media = project.media.read().ok()?;

    project.with_comp(comp_uuid, |comp| {
        let (camera, pos, rot) = comp.active_camera(frame_idx, &media)?;

        // Use COMP aspect for gizmo projection (same as compositor).
        // Gizmo must match how compositor rendered the scene, not viewport stretch.
        let (comp_w, comp_h) = comp.dim();
        let aspect = comp_w as f32 / comp_h as f32;

        let view = camera.view_matrix(pos, rot);
        let proj = camera.projection_matrix(aspect, comp_h as f32);

        Some((view, proj))
    }).flatten()
}

/// Build view and projection matrices for gizmo.
///
/// # 3D Camera Support
///
/// When camera matrices are provided, gizmo uses perspective projection
/// matching the rendered view. Viewport zoom/pan is applied to the view matrix.
///
/// When None (2D mode), uses simple ortho projection with viewport zoom/pan.
fn build_gizmo_matrices(
    viewport_state: &ViewportState,
    clip_rect: egui::Rect,
    camera_matrices: Option<(glam::Mat4, glam::Mat4)>,
) -> (mint::RowMatrix4<f64>, mint::RowMatrix4<f64>) {
    use glam::{DMat4, DVec3};

    if let Some((cam_view, cam_proj)) = camera_matrices {
        // 3D mode: use camera view and projection separately
        //
        // Camera projection outputs NDC [-1,1] representing comp space.
        // We need to transform this to screen NDC that matches image_to_screen().
        //
        // image_to_screen: frame -> (zoom, pan) -> screen
        // Camera NDC -> comp image -> frame -> (zoom, pan) -> screen -> screen NDC
        let view_f64 = DMat4::from_cols_array(&cam_view.to_cols_array().map(|v| v as f64));
        let proj_f64 = DMat4::from_cols_array(&cam_proj.to_cols_array().map(|v| v as f64));

        // Comp size (what camera renders to) and viewport size
        let comp_w = viewport_state.image_size.x as f64;
        let comp_h = viewport_state.image_size.y as f64;
        let vp_w = clip_rect.width() as f64;
        let vp_h = clip_rect.height() as f64;
        let zoom = viewport_state.zoom as f64;
        let pan_x = viewport_state.pan.x as f64;
        let pan_y = viewport_state.pan.y as f64;

        // Transform camera NDC to screen NDC:
        // 1. Camera NDC [-1,1] represents [-comp/2, comp/2] in world
        // 2. Screen NDC [-1,1] represents [-viewport/2, viewport/2] in screen
        // 3. Apply zoom and pan to match image_to_screen()
        //
        // screen_pos = world_pos * zoom + pan
        // screen_NDC = screen_pos / (viewport/2)
        //            = (cam_NDC * comp/2 * zoom + pan) / (viewport/2)
        //            = cam_NDC * (comp * zoom / viewport) + pan * 2 / viewport
        let scale_x = comp_w * zoom / vp_w;
        let scale_y = comp_h * zoom / vp_h;
        let trans_x = pan_x * 2.0 / vp_w;
        let trans_y = pan_y * 2.0 / vp_h;

        let viewport_transform = DMat4::from_cols(
            glam::DVec4::new(scale_x, 0.0, 0.0, 0.0),
            glam::DVec4::new(0.0, scale_y, 0.0, 0.0),
            glam::DVec4::new(0.0, 0.0, 1.0, 0.0),
            glam::DVec4::new(trans_x, trans_y, 0.0, 1.0),
        );

        // Final projection = viewport_transform * camera_proj
        let final_proj = viewport_transform * proj_f64;

        (to_row_matrix(view_f64), to_row_matrix(final_proj))
    } else {
        // 2D mode: simple ortho with zoom/pan
        let view = DMat4::from_scale_rotation_translation(
            DVec3::splat(viewport_state.zoom as f64),
            glam::DQuat::IDENTITY,
            DVec3::new(viewport_state.pan.x as f64, viewport_state.pan.y as f64, 0.0),
        );

        // Projection: orthographic
        let w = clip_rect.width() as f64;
        let h = clip_rect.height() as f64;
        let proj = DMat4::orthographic_rh(-w / 2.0, w / 2.0, -h / 2.0, h / 2.0, -1000.0, 1000.0);

        (to_row_matrix(view), to_row_matrix(proj))
    }
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
) -> Transform {
    use glam::{DQuat, DVec3};

    // Layer transform attributes use Y-up, clockwise-positive rotation (CW+).
    // transform-gizmo expects Y-up with counter-clockwise-positive rotation (CCW+).
    //
    // # 3D Support
    //
    // We now pass full 3D transform to gizmo:
    // - Position: all three components (X, Y, Z)
    // - Rotation: all three axes (X, Y, Z) converted from CW+ degrees to CCW+ radians
    // - Scale: X/Y for Scale tool, uniform 1.0 for others (prevents oval handles)
    //
    // The gizmo will display correct 3D orientation. Editing is still limited
    // by enabled GizmoModes (see to_gizmo_modes).
    //
    // Rotation order: ZYX (AE-style), same as in transform.rs
    let translation = DVec3::new(position[0] as f64, position[1] as f64, position[2] as f64);

    // Convert rotation: CW+ degrees → CCW+ radians
    // to_math_rot negates for CW→CCW and converts deg→rad
    let rot_x = space::to_math_rot(rotation[0]) as f64;
    let rot_y = space::to_math_rot(rotation[1]) as f64;
    let rot_z = space::to_math_rot(rotation[2]) as f64;

    // ZYX order: rotate Z first, then Y, then X (matches compositor)
    let rotation_quat = DQuat::from_euler(glam::EulerRot::ZYX, rot_z, rot_y, rot_x);

    let scale_vec = match tool {
        ToolMode::Scale => DVec3::new(scale[0] as f64, scale[1] as f64, scale[2] as f64),
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

    // ZYX order to match layer_to_gizmo_transform
    // Returns (z, y, x) for ZYX order
    let (rot_z, rot_y, rot_x) = rotation.to_euler(glam::EulerRot::ZYX);

    // Convert CCW+ radians back to CW+ degrees using from_math_rot
    (
        [translation.x as f32, translation.y as f32, translation.z as f32],
        [
            space::from_math_rot(rot_x as f32),
            space::from_math_rot(rot_y as f32),
            space::from_math_rot(rot_z as f32),
        ],
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
            let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
            let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
            let scale = layer.attrs.get_vec3(A_SCALE).unwrap_or([1.0, 1.0, 1.0]);
            (pos, rot, scale)
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
