//! Layer picking via raycast.
//!
//! On LMB click in Select mode, iterates visible layers top-to-bottom and
//! tests if click point falls within layer bounds after inverse transform.
//!
//! # Algorithm
//!
//! 1. Convert screen click position to comp space (Y-up, centered)
//! 2. For each visible layer (top-to-bottom order):
//!    a. Check if current frame is within layer's work area
//!    b. Apply inverse transform: comp_pos -> object_pos
//!    c. Check if object_pos is within layer bounds [-w/2, w/2] x [-h/2, h/2]
//! 3. First hit = selected layer
//!
//! Complexity: O(visible_layers) per click - negligible for typical layer counts.

use glam::{EulerRot, Quat, Vec2, Vec3};
use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::entities::comp_node::CompNode;
use crate::entities::node_kind::NodeKind;
use crate::entities::space;
use crate::entities::transform::build_inverse_transform;
use super::ViewportState;

/// Compute layer plane normal from rotation (same as transform.rs).
#[inline]
fn layer_plane_normal(rotation: [f32; 3]) -> Vec3 {
    let quat = Quat::from_euler(
        EulerRot::ZYX,
        -rotation[2],
        -rotation[1],
        -rotation[0],
    );
    quat * Vec3::Z
}

/// Result of a pick operation.
#[derive(Debug, Clone)]
pub struct PickResult {
    /// UUID of the picked layer (None if nothing hit)
    pub layer_uuid: Option<Uuid>,
}

/// Pick layer at screen position.
///
/// # Arguments
/// - `screen_pos` - Click position in screen space (egui coords, Y-down)
/// - `panel_rect` - Viewport panel rectangle in screen space
/// - `viewport_zoom` - Current viewport zoom level
/// - `viewport_pan` - Current viewport pan offset
/// - `image_size` - Composited image size (width, height)
/// - `comp` - The composition to pick from
/// - `frame_idx` - Current frame index
/// - `media` - Media pool for dynamic layer timing
///
/// # Returns
/// UUID of the topmost visible layer under the click, or None if nothing hit.
/// Pick layer at screen position.
///
/// Uses ViewportState coordinate conversion + space::image_to_frame.
pub fn pick_layer_at(
    screen_pos: eframe::egui::Pos2,
    panel_rect: eframe::egui::Rect,
    viewport_state: &ViewportState,
    comp: &CompNode,
    frame_idx: i32,
    media: &HashMap<Uuid, Arc<NodeKind>>,
) -> PickResult {
    // Screen -> local (relative to panel, as viewport expects)
    let local = screen_pos - panel_rect.left_top();
    
    debug!("[pick] screen={:?} local={:?}", screen_pos, local);
    
    // Local -> image space (Y-down, 0..image_size) using viewport's method
    let Some(image_pos) = viewport_state.screen_to_image(eframe::egui::vec2(local.x, local.y)) else {
        debug!("[pick] outside image bounds");
        return PickResult { layer_uuid: None };
    };
    
    // Image -> frame/comp space (Y-up, centered) using space.rs
    let image_size = (viewport_state.image_size.x as usize, viewport_state.image_size.y as usize);
    let comp_pos = space::image_to_frame(Vec2::new(image_pos.x, image_pos.y), image_size);
    
    debug!("[pick] image={:?} comp={:?} zoom={} pan={:?}", image_pos, comp_pos, viewport_state.zoom, viewport_state.pan);

    // Iterate top-to-bottom for picking (first hit wins).
    // Timeline order: layers[0] = top row (foreground), layers[n-1] = bottom row (background).
    // Note: compose_internal uses rev() for bottom-up blending, pick uses forward for top-down hit test.
    debug!("[pick] checking {} layers", comp.layers.len());
    for (i, layer) in comp.layers.iter().enumerate() {
        let name = layer.attrs.get_str("name").unwrap_or("?");
        
        // Skip invisible layers
        if !layer.is_visible() {
            debug!("[pick] layer[{}] = {} SKIP (invisible)", i, name);
            continue;
        }

        // Skip non-renderable layers (camera, light, null, audio)
        if !layer.attrs.get_bool("renderable").unwrap_or(true) {
            debug!("[pick] layer[{}] = {} SKIP (non-renderable)", i, name);
            continue;
        }

        // Check if frame is within layer's work area
        let (play_start, play_end) = comp.get_layer_work_area(layer, media);
        if frame_idx < play_start || frame_idx > play_end {
            debug!("[pick] layer[{}] = {} SKIP (frame {} not in {}..{})", i, name, frame_idx, play_start, play_end);
            continue;
        }
        
        debug!("[pick] layer[{}] = {}", i, name);

        // Get layer transform
        let position = layer.attrs.get_vec3("position").unwrap_or([0.0, 0.0, 0.0]);
        let rotation_deg = layer.attrs.get_vec3("rotation").unwrap_or([0.0, 0.0, 0.0]);
        let scale = layer.attrs.get_vec3("scale").unwrap_or([1.0, 1.0, 1.0]);
        let pivot = layer.attrs.get_vec3("pivot").unwrap_or([0.0, 0.0, 0.0]);

        // Convert rotation to radians
        let rotation = [
            rotation_deg[0].to_radians(),
            rotation_deg[1].to_radians(),
            rotation_deg[2].to_radians(),
        ];

        // Build inverse transform: comp -> object space
        let inv_transform = build_inverse_transform(position, rotation, scale, pivot);
        
        // Check if layer is tilted (X/Y rotation)
        let plane_normal = layer_plane_normal(rotation);
        let layer_is_tilted = (plane_normal - Vec3::Z).length_squared() > 1e-6;
        
        // Transform comp position to object space
        // For tilted layers, use ray-plane intersection (same as compositor)
        let obj_pos = if layer_is_tilted {
            let plane_point = Vec3::from(position);
            let ray_origin = Vec3::new(comp_pos.x, comp_pos.y, 10000.0);
            let ray_dir = Vec3::NEG_Z;
            
            let denom = ray_dir.dot(plane_normal);
            if denom.abs() < 1e-6 {
                // Ray parallel to plane - can't hit
                debug!("[pick] layer={} SKIP (edge-on)", name);
                continue;
            }
            let t = (plane_point - ray_origin).dot(plane_normal) / denom;
            let world_pt = ray_origin + ray_dir * t;
            let obj_pt3 = inv_transform.transform_point3(world_pt);
            Vec2::new(obj_pt3.x, obj_pt3.y)
        } else {
            // Flat layer: direct affine transform
            let comp_pos_3d = Vec3::new(comp_pos.x, comp_pos.y, 0.0);
            let obj_pos_3d = inv_transform.transform_point3(comp_pos_3d);
            Vec2::new(obj_pos_3d.x, obj_pos_3d.y)
        };

        // Get layer dimensions (object space bounds are [-w/2, w/2] x [-h/2, h/2])
        let layer_w = layer.attrs.get_u32("width").unwrap_or(100) as f32;
        let layer_h = layer.attrs.get_u32("height").unwrap_or(100) as f32;
        let half_w = layer_w * 0.5;
        let half_h = layer_h * 0.5;

        // Check if point is within layer bounds
        let hit = obj_pos.x >= -half_w && obj_pos.x <= half_w
            && obj_pos.y >= -half_h && obj_pos.y <= half_h;
        
        debug!(
            "[pick] layer={} pos={:?} rot={:?} scale={:?} obj={:?} bounds=[{:.0}x{:.0}] hit={}",
            layer.attrs.get_str("name").unwrap_or("?"),
            position, rotation_deg, scale, obj_pos, layer_w, layer_h, hit
        );
        
        if hit {
            return PickResult {
                layer_uuid: Some(layer.uuid()),
            };
        }
    }

    debug!("[pick] no hit");
    PickResult { layer_uuid: None }
}
