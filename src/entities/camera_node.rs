//! CameraNode - 3D camera for perspective projection.
//!
//! Defines view/projection matrices for 3D layer transforms.
//! Cameras don't produce pixels - they define the viewpoint.

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attr_schemas::CAMERA_SCHEMA;
use super::attrs::{AttrValue, Attrs};
use super::frame::Frame;
use super::node::{ComputeContext, Node};
use super::keys::{A_IN, A_OUT, A_SRC_LEN, A_SPEED, A_TRIM_IN, A_TRIM_OUT};

/// Camera node for 3D compositing.
/// 
/// Standard layer attributes:
/// - position: camera location [x, y, z]
/// - rotation: Euler angles [rx, ry, rz] in degrees
/// - scale: [sx, sy, sz]
/// - pivot: anchor point offset
/// 
/// Camera-specific:
/// - point_of_interest: look-at target (alternative to rotation)
/// - use_poi: if true, use POI; if false, use rotation
/// - fov: field of view in degrees (default 39.6 like AE)
/// - near_clip, far_clip: clipping planes
/// - dof_enabled, focus_distance, aperture: depth of field (future)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CameraNode {
    pub attrs: Attrs,
}

impl CameraNode {
    /// Create new camera with default settings.
    pub fn new(name: &str) -> Self {
        let mut attrs = Attrs::with_schema(&*CAMERA_SCHEMA);
        
        // Identity
        attrs.set("uuid", AttrValue::Uuid(Uuid::new_v4()));
        attrs.set("name", AttrValue::Str(name.to_string()));

        // NOTE: No position/rotation/scale here - those come from Layer attrs
        // Camera only stores lens/projection settings

        // Camera-specific
        attrs.set("projection_type", AttrValue::Str("perspective".to_string()));
        attrs.set("point_of_interest", AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set("use_poi", AttrValue::Bool(false)); // default: rotation mode (POI mode available via toggle)
        
        // Lens - perspective mode (AE defaults)
        attrs.set("fov", AttrValue::Float(39.6));
        attrs.set("near_clip", AttrValue::Float(1.0));
        attrs.set("far_clip", AttrValue::Float(10000.0));
        
        // Lens - orthographic mode
        attrs.set("ortho_scale", AttrValue::Float(1.0)); // 1.0 = 1:1 pixel mapping
        
        // Depth of field (future)
        attrs.set("dof_enabled", AttrValue::Bool(false));
        attrs.set("focus_distance", AttrValue::Float(1000.0));
        attrs.set("aperture", AttrValue::Float(2.8));
        
        // Timing (unified: in, out, trim_in, trim_out, src_len, speed)
        attrs.set(A_IN, AttrValue::Int(0));
        attrs.set(A_OUT, AttrValue::Int(100));
        attrs.set(A_SRC_LEN, AttrValue::Int(100));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        attrs.set("opacity", AttrValue::Float(1.0));
        
        attrs.clear_dirty();
        Self { attrs }
    }
    
    /// Create camera with specific UUID (for deserialization).
    pub fn with_uuid(name: &str, uuid: Uuid) -> Self {
        let mut node = Self::new(name);
        node.attrs.set("uuid", AttrValue::Uuid(uuid));
        node.attrs.clear_dirty();
        node
    }
    
    /// Attach schema after deserialization.
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*CAMERA_SCHEMA);
    }
    
    // NOTE: No position/rotation/scale getters - those come from Layer attrs

    // === Camera-specific getters ===
    
    /// "perspective" or "orthographic"
    pub fn projection_type(&self) -> &str {
        self.attrs.get_str("projection_type").unwrap_or("perspective")
    }
    
    pub fn is_orthographic(&self) -> bool {
        self.projection_type() == "orthographic"
    }
    
    pub fn point_of_interest(&self) -> [f32; 3] {
        self.attrs.get_vec3("point_of_interest").unwrap_or([0.0, 0.0, 0.0])
    }
    
    pub fn use_poi(&self) -> bool {
        self.attrs.get_bool("use_poi").unwrap_or(true)
    }
    
    pub fn fov(&self) -> f32 {
        self.attrs.get_float("fov").unwrap_or(39.6)
    }
    
    pub fn near_clip(&self) -> f32 {
        self.attrs.get_float("near_clip").unwrap_or(1.0)
    }
    
    pub fn far_clip(&self) -> f32 {
        self.attrs.get_float("far_clip").unwrap_or(10000.0)
    }
    
    /// Orthographic scale factor (1.0 = 1:1 pixel mapping)
    pub fn ortho_scale(&self) -> f32 {
        self.attrs.get_float("ortho_scale").unwrap_or(1.0)
    }
    
    pub fn dof_enabled(&self) -> bool {
        self.attrs.get_bool("dof_enabled").unwrap_or(false)
    }
    
    pub fn focus_distance(&self) -> f32 {
        self.attrs.get_float("focus_distance").unwrap_or(1000.0)
    }
    
    pub fn aperture(&self) -> f32 {
        self.attrs.get_float("aperture").unwrap_or(2.8)
    }
    
    // === Matrix builders ===

    /// Build view matrix (world -> camera space).
    ///
    /// # Architecture: Why pos/rot are arguments, not stored in CameraNode
    ///
    /// Camera is a spatial object like any layer. Its transform (position, rotation,
    /// scale) lives on the Layer that references this CameraNode. This follows AE
    /// model and avoids duplicate attrs. CameraNode only stores lens parameters.
    ///
    /// Call site (comp_node.rs) reads layer.position/rotation and passes here.
    ///
    /// # Modes
    /// - `use_poi=true`: look_at mode, camera points at point_of_interest
    /// - `use_poi=false`: rotation mode, use Euler angles from layer
    ///
    /// # Rotation Convention (IMPORTANT)
    ///
    /// Camera uses the **same rotation convention as layers** for consistency:
    ///
    /// - **Order**: ZYX (After Effects style) - rotate Z first, then Y, then X
    /// - **Sign**: Clockwise-positive (CW+) - user convention, matches AE
    /// - **glam**: Uses counter-clockwise-positive (CCW+), so angles are **negated**
    ///
    /// ```text
    /// User input (degrees, CW+)  -->  negate  -->  glam (radians, CCW+)
    /// rotation = [10, 20, 30]    -->  [-10, -20, -30] in radians
    /// ```
    ///
    /// This ensures camera and layer rotations behave identically.
    /// See also: `transform.rs::build_model_matrix()`, `gizmo.rs::layer_to_gizmo_transform()`
    ///
    /// # Arguments
    /// - `position` - camera position from layer attrs [x, y, z]
    /// - `rotation` - camera rotation from layer attrs [rx, ry, rz] in degrees (CW+)
    pub fn view_matrix(&self, position: [f32; 3], rotation: [f32; 3]) -> Mat4 {
        let eye = Vec3::from(position);
        let up = Vec3::Y; // Y-up convention

        if self.use_poi() {
            // Look-at mode: camera points at POI
            let target = self.point_of_interest();
            let center = Vec3::from(target);
            Mat4::look_at_rh(eye, center, up)
        } else {
            // Rotation mode: use Euler angles (degrees)
            // ZYX order (AE-style), angles negated for CW→CCW convention
            // This matches layer rotation in transform.rs
            let rot_x = -rotation[0].to_radians();
            let rot_y = -rotation[1].to_radians();
            let rot_z = -rotation[2].to_radians();

            // Build rotation matrix (ZYX order, same as layers)
            let rot_mat = Mat4::from_euler(glam::EulerRot::ZYX, rot_z, rot_y, rot_x);
            let translation = Mat4::from_translation(-eye);

            rot_mat * translation
        }
    }

    /// Build projection matrix (camera -> clip space).
    ///
    /// Supports both perspective and orthographic projection.
    ///
    /// # Arguments
    /// - `aspect` - viewport width / height
    /// - `comp_height` - composition height in pixels (for ortho scale)
    pub fn projection_matrix(&self, aspect: f32, comp_height: f32) -> Mat4 {
        let near = self.near_clip();
        let far = self.far_clip();

        if self.is_orthographic() {
            // Orthographic: ortho_scale=1.0 means comp_height maps to view height
            let scale = self.ortho_scale();
            let half_h = (comp_height * 0.5) / scale;
            let half_w = half_h * aspect;
            Mat4::orthographic_rh_gl(-half_w, half_w, -half_h, half_h, near, far)
        } else {
            // Perspective
            let fov_rad = self.fov().to_radians();
            Mat4::perspective_rh_gl(fov_rad, aspect, near, far)
        }
    }

    /// Build combined view-projection matrix (world -> clip space).
    ///
    /// Position and rotation come from the Layer attrs (not stored in CameraNode).
    ///
    /// # Arguments
    /// - `position` - camera position from layer attrs [x, y, z]
    /// - `rotation` - camera rotation from layer attrs [rx, ry, rz] in degrees
    /// - `aspect` - viewport width / height ratio
    /// - `comp_height` - composition height in pixels (for ortho scale)
    pub fn view_projection_matrix(
        &self,
        position: [f32; 3],
        rotation: [f32; 3],
        aspect: f32,
        comp_height: f32,
    ) -> Mat4 {
        self.projection_matrix(aspect, comp_height) * self.view_matrix(position, rotation)
    }
}

impl Node for CameraNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid("uuid").unwrap_or_else(Uuid::nil)
    }
    
    fn name(&self) -> &str {
        self.attrs.get_str("name").unwrap_or("Camera")
    }
    
    fn node_type(&self) -> &'static str {
        "Camera"
    }
    
    fn attrs(&self) -> &Attrs {
        &self.attrs
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        vec![] // Cameras have no inputs
    }
    
    /// Cameras don't produce pixels.
    fn compute(&self, _frame: i32, _ctx: &ComputeContext) -> Option<Frame> {
        None
    }
    
    fn is_dirty(&self) -> bool {
        self.attrs.is_dirty()
    }
    
    fn mark_dirty(&self) {
        self.attrs.mark_dirty();
    }
    
    fn clear_dirty(&self) {
        self.attrs.clear_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_defaults() {
        let cam = CameraNode::new("Test Camera");

        assert_eq!(cam.name(), "Test Camera");
        assert_eq!(cam.node_type(), "Camera");
        // position comes from Layer now, not CameraNode
        assert_eq!(cam.point_of_interest(), [0.0, 0.0, 0.0]);
        assert!((cam.fov() - 39.6).abs() < 0.01);
    }

    #[test]
    fn test_view_matrix() {
        let cam = CameraNode::new("Test");
        // Position/rotation now come from layer attrs
        let pos = [0.0, 0.0, -1000.0];
        let rot = [0.0, 0.0, 0.0];
        let view = cam.view_matrix(pos, rot);

        // View matrix should be valid (not NaN/Inf)
        assert!(!view.is_nan());
    }

    #[test]
    fn test_projection_matrix_perspective() {
        let cam = CameraNode::new("Test");
        // Default is perspective mode
        let proj = cam.projection_matrix(16.0 / 9.0, 1080.0);

        assert!(!proj.is_nan());
        assert!(!cam.is_orthographic());
    }

    #[test]
    fn test_projection_matrix_orthographic() {
        let mut cam = CameraNode::new("Test");
        cam.attrs.set("projection_type", super::AttrValue::Str("orthographic".to_string()));

        let proj = cam.projection_matrix(16.0 / 9.0, 1080.0);

        assert!(!proj.is_nan());
        assert!(cam.is_orthographic());
    }
}
