//! Viewport gizmo for layer transforms.
//!
//! Provides Move/Rotate/Scale manipulation gizmos using the in-house
//! `egui-gizmo` crate (a glam-f32 facade over a vendored transform-gizmo core).
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
//!
//! ## Migration note (transform-gizmo-egui 0.9 -> egui-gizmo)
//!
//! Only the gizmo-library surface changed; playa's coordinate math is untouched:
//! - matrices are still built by `build_gizmo_matrices` / `get_camera_matrices`,
//!   now handed to the facade as **glam f32 column-major** `Mat4` (the facade
//!   widens to f64 internally without transposing — no row/`mint` shuffle).
//! - the per-tool handle set comes from `GizmoTool` (Move/Rotate/Scale) plus the
//!   `GizmoSpace` curation: 3D-camera comps stay full 3D, flat 2D comps use the
//!   2D handle set (Z translate/scale and X/Y rotation rings auto-hidden).
//! - the CW+deg <-> CCW+rad rotation conversion and ZYX euler order are identical.

use eframe::egui;
use egui_gizmo::{
    Gizmo, GizmoConfig, GizmoOrientation, GizmoSpace, GizmoTool, GizmoVisuals, Transform,
};
use uuid::Uuid;

use super::ViewportState;
use super::tool::ToolMode;
use playa_engine::core::event_bus::BoxedEvent;
use playa_engine::core::player::Player;
use playa_engine::entities::Project;
use playa_engine::entities::comp_events::SetLayerTransformsEvent;
use playa_engine::entities::keys::{A_POSITION, A_ROTATION, A_SCALE};
use playa_engine::entities::node::Node; // for dim()
use playa_engine::entities::space;

/// Gizmo state - lives in PlayaApp, not saved.
///
/// The `Gizmo` is **persistent across frames**: it owns the active-drag state
/// (which handle is grabbed, the drag-start basis), so it must NOT be recreated
/// each frame. We reconfigure it via `update_config` and then `interact`.
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
    /// Returns `(consumed, events)` where `consumed` is true while a handle is
    /// being actively dragged this frame.
    pub fn render(
        &mut self,
        ui: &egui::Ui,
        viewport_state: &ViewportState,
        project: &Project,
        player: &Player,
    ) -> (bool, Vec<BoxedEvent>) {
        let tool = ToolMode::from_str(&project.tool());

        // No gizmo in Select mode; map the transform tools to the gizmo tool.
        let gizmo_tool = match tool_to_gizmo_tool(tool) {
            Some(t) => t,
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

        // Get camera matrices if 3D camera is active
        let frame_idx = player.current_frame(project);
        let camera_matrices = get_camera_matrices(project, comp_uuid, frame_idx, ui.clip_rect());

        // Preserve the pre-migration behavior: always expose the full 3D handle
        // set. The retired transform-gizmo-egui gizmo did NOT curate handles by
        // projection type, so a flat ortho comp still showed Z translate/scale +
        // X/Y rotation rings. egui-gizmo CAN curate to `GizmoSpace::TwoD` (hiding
        // those for ortho comps) — switch here if 2D-curated handles are desired;
        // left at `ThreeD` to avoid a (runtime-only, unverifiable) UX regression.
        let space = GizmoSpace::ThreeD;

        // Build matrices (uses camera for 3D, ortho for 2D)
        let (view, proj) = build_gizmo_matrices(viewport_state, ui.clip_rect(), camera_matrices);

        // Configure gizmo
        let gizmo_prefs = project.gizmo_prefs();

        // Shift enables snapping; the increments below restore playa's classic
        // 5deg / 10px / 0.1 ladder (now settable via the egui-gizmo facade).
        let snapping = ui.input(|i| i.modifiers.shift);

        self.gizmo.update_config(GizmoConfig {
            view,
            projection: proj,
            viewport: ui.clip_rect(),
            space,
            tool: gizmo_tool,
            orientation: GizmoOrientation::Local,
            snapping,
            snap_angle: 5.0_f32.to_radians(),
            snap_distance: 10.0,
            snap_scale: 0.1,
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
            if let Some((pos, rot, scale)) = get_layer_transform(project, comp_uuid, layer_uuid) {
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
            let Some((layer_uuid, old_pos, old_rot, old_scale)) = layer_data.get(i) else {
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
// Tool -> gizmo tool (crate-local helper; `ToolMode` lives in `playa-events`.)
// ============================================================================

/// Map playa's `ToolMode` to the gizmo's exclusive `GizmoTool`.
///
/// Only the transform tools manipulate layers; `Select` shows no gizmo (returns
/// `None`, so `render` bails before drawing anything). The narrowing of the
/// handle set *within* a tool (which axes/planes/rings) is left to the gizmo's
/// `GizmoSpace` curation (see `render`) and the default `GizmoFeatures::all()`.
fn tool_to_gizmo_tool(tool: ToolMode) -> Option<GizmoTool> {
    match tool {
        ToolMode::Select => None,
        ToolMode::Move => Some(GizmoTool::Move),
        ToolMode::Rotate => Some(GizmoTool::Rotate),
        ToolMode::Scale => Some(GizmoTool::Scale),
    }
}

// ============================================================================
// Matrix helpers
// ============================================================================

/// Get camera view and projection matrices if 3D camera is active.
///
/// Returns `(view, projection)` separately for gizmo library.
/// Returns `None` if no camera in comp (2D mode).
///
/// # Aspect Ratio
///
/// Uses **comp aspect** (`comp_w / comp_h`), NOT viewport aspect.
///
/// Why: Compositor renders the scene with comp aspect ratio. The viewport
/// then stretches/letterboxes this texture to fit. Gizmo must match the
/// compositor's projection, not the viewport's display stretch.
///
/// ```text
/// WRONG: aspect = viewport_w / viewport_h  (gizmo misaligned with layer)
/// RIGHT: aspect = comp_w / comp_h          (gizmo matches layer exactly)
/// ```
fn get_camera_matrices(
    project: &Project,
    comp_uuid: Uuid,
    frame_idx: i32,
    _viewport_rect: egui::Rect,
) -> Option<(glam::Mat4, glam::Mat4)> {
    let media = project.media.read().ok()?;

    project
        .with_comp(comp_uuid, |comp| {
            let (camera, pos, rot) = comp.active_camera(frame_idx, &media)?;

            // Use COMP aspect for gizmo projection (same as compositor).
            // Gizmo must match how compositor rendered the scene, not viewport stretch.
            let (comp_w, comp_h) = comp.dim();
            let aspect = comp_w as f32 / comp_h as f32;

            let view = camera.view_matrix(pos, rot);
            let proj = camera.projection_matrix(aspect, comp_h as f32);

            Some((view, proj))
        })
        .flatten()
}

/// Build view and projection matrices for gizmo.
///
/// # Coordinate Pipeline
///
/// The gizmo must appear at the same screen position as the rendered layer.
/// This requires matching the compositor's projection with viewport's display transform.
///
/// ```text
/// COMPOSITOR (renders to texture):          VIEWPORT (displays texture):
/// world -> camera VP -> comp texture        comp texture -> zoom/pan -> screen
///
/// GIZMO (must match both):
/// world -> camera VP -> viewport_transform -> screen NDC
/// ```
///
/// # 3D Mode (with camera)
///
/// When camera is active, we chain: `camera_view * camera_proj * viewport_transform`
///
/// The viewport_transform converts camera NDC to screen NDC:
///
/// ```text
/// Camera NDC [-1,1] represents world coords [-comp/2, comp/2]
/// Screen NDC [-1,1] represents screen coords [-viewport/2, viewport/2]
///
/// To match image_to_screen():
///   screen_pos = world_pos * zoom + pan
///   screen_NDC = screen_pos / (viewport/2)
///
/// Substituting world_pos = cam_NDC * comp/2:
///   screen_NDC = (cam_NDC * comp/2 * zoom + pan) / (viewport/2)
///              = cam_NDC * (comp * zoom / viewport) + pan * 2 / viewport
///
/// This gives us the viewport_transform matrix:
///   | comp_w*zoom/vp_w    0                   0    pan_x*2/vp_w |
///   | 0                   comp_h*zoom/vp_h    0    pan_y*2/vp_h |
///   | 0                   0                   1    0            |
///   | 0                   0                   0    1            |
/// ```
///
/// # 2D Mode (no camera)
///
/// Without camera, layer positions are already in frame space (centered, Y-up).
/// We use simple orthographic projection with zoom/pan in the view matrix.
///
/// # Aspect Ratio (IMPORTANT)
///
/// Camera projection uses **comp aspect** (not viewport aspect) because:
/// - Compositor renders with comp aspect
/// - Viewport stretches the result to fit
/// - Gizmo must match compositor, not viewport stretch
///
/// See `get_camera_matrices()` where aspect is computed from `comp.dim()`.
///
/// # Precision / matrix convention
///
/// The chain is computed in `DMat4` (f64) for precision, then narrowed to
/// `glam::Mat4` (f32, column-major) at the boundary — `egui-gizmo` takes
/// column-major f32 and widens it back to f64 internally **without** a
/// transpose, so no row-major / `mint` conversion is needed.
fn build_gizmo_matrices(
    viewport_state: &ViewportState,
    clip_rect: egui::Rect,
    camera_matrices: Option<(glam::Mat4, glam::Mat4)>,
) -> (glam::Mat4, glam::Mat4) {
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

        // Camera NDC → screen NDC chain (zoom + pan, comp/viewport
        // remap). Single source of truth lives in
        // `playa_coord::screen_ndc_from_frame_ndc` — gizmo and any
        // future viewport overlay share the algebra. f32 helper cast
        // up to DMat4 here for gizmo's f64 precision pipeline.
        let comp_size = (
            viewport_state.image_size.x as usize,
            viewport_state.image_size.y as usize,
        );
        let vp_size_f32 = glam::Vec2::new(clip_rect.width(), clip_rect.height());
        let pan_f32 = glam::Vec2::new(viewport_state.pan.x, viewport_state.pan.y);
        let viewport_transform_f32 = space::screen_ndc_from_frame_ndc(
            viewport_state.zoom,
            pan_f32,
            comp_size,
            vp_size_f32,
        );
        let viewport_transform =
            DMat4::from_cols_array(&viewport_transform_f32.to_cols_array().map(f64::from));

        // Final projection = viewport_transform * camera_proj
        let final_proj = viewport_transform * proj_f64;

        (view_f64.as_mat4(), final_proj.as_mat4())
    } else {
        // 2D mode: simple ortho with zoom/pan
        let view = DMat4::from_scale_rotation_translation(
            DVec3::splat(viewport_state.zoom as f64),
            glam::DQuat::IDENTITY,
            DVec3::new(
                viewport_state.pan.x as f64,
                viewport_state.pan.y as f64,
                0.0,
            ),
        );

        // Projection: orthographic
        let w = clip_rect.width() as f64;
        let h = clip_rect.height() as f64;
        let proj = DMat4::orthographic_rh(-w / 2.0, w / 2.0, -h / 2.0, h / 2.0, -1000.0, 1000.0);

        (view.as_mat4(), proj.as_mat4())
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
    // The gizmo expects Y-up with counter-clockwise-positive rotation (CCW+).
    //
    // # 3D Support
    //
    // We pass a full 3D transform to the gizmo:
    // - Position: all three components (X, Y, Z)
    // - Rotation: all three axes (X, Y, Z) converted from CW+ degrees to CCW+ radians
    // - Scale: X/Y/Z for Scale tool, uniform 1.0 for others (prevents oval handles)
    //
    // The gizmo displays the correct orientation; the visible handle set is
    // curated by GizmoSpace + GizmoTool (see `render`).
    //
    // Rotation order: ZYX (AE-style), same as in transform.rs.
    //
    // The math runs in f64 for precision, then narrows to the gizmo's public
    // glam-f32 `Transform` at the boundary.
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
        scale_vec.as_vec3(),
        rotation_quat.as_quat(),
        translation.as_vec3(),
    )
}

fn gizmo_to_layer_transform(t: &Transform) -> ([f32; 3], [f32; 3], [f32; 3]) {
    use glam::DQuat;

    // Widen the gizmo's f32 quaternion to f64 for the euler decomposition, to
    // match the precision of `layer_to_gizmo_transform`'s forward path.
    let rotation = DQuat::from_xyzw(
        t.rotation.x as f64,
        t.rotation.y as f64,
        t.rotation.z as f64,
        t.rotation.w as f64,
    );

    // ZYX order to match layer_to_gizmo_transform
    // Returns (z, y, x) for ZYX order
    let (rot_z, rot_y, rot_x) = rotation.to_euler(glam::EulerRot::ZYX);

    // Convert CCW+ radians back to CW+ degrees using from_math_rot
    (
        [t.translation.x, t.translation.y, t.translation.z],
        [
            space::from_math_rot(rot_x as f32),
            space::from_math_rot(rot_y as f32),
            space::from_math_rot(rot_z as f32),
        ],
        [t.scale.x, t.scale.y, t.scale.z],
    )
}

fn get_layer_transform(
    project: &Project,
    comp_uuid: Uuid,
    layer_uuid: Uuid,
) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    project
        .with_comp(comp_uuid, |comp| {
            comp.get_layer(layer_uuid).map(|layer| {
                let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
                let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
                let scale = layer.attrs.get_vec3(A_SCALE).unwrap_or([1.0, 1.0, 1.0]);
                (pos, rot, scale)
            })
        })
        .flatten()
}

#[inline]
fn approx_vec3_equal(a: [f32; 3], b: [f32; 3]) -> bool {
    // Keep epsilon conservative: gizmo drags are continuous; this just avoids
    // emitting identical values due to float roundtrips.
    const EPS: f32 = 1.0e-6;
    (a[0] - b[0]).abs() <= EPS && (a[1] - b[1]).abs() <= EPS && (a[2] - b[2]).abs() <= EPS
}
