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
        let mut attrs = Attrs::with_schema(&CAMERA_SCHEMA);
        
        // Identity
        attrs.set("uuid", AttrValue::Uuid(Uuid::new_v4()));
        attrs.set("name", AttrValue::Str(name.to_string()));
        
        // Standard layer transform
        attrs.set("position", AttrValue::Vec3([0.0, 0.0, -1000.0])); // pulled back
        attrs.set("rotation", AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set("scale", AttrValue::Vec3([1.0, 1.0, 1.0]));
        attrs.set("pivot", AttrValue::Vec3([0.0, 0.0, 0.0]));
        
        // Camera-specific
        attrs.set("point_of_interest", AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set("use_poi", AttrValue::Bool(true)); // default: use POI like AE
        
        // Lens (AE defaults)
        attrs.set("fov", AttrValue::Float(39.6));
        attrs.set("near_clip", AttrValue::Float(1.0));
        attrs.set("far_clip", AttrValue::Float(10000.0));
        
        // Depth of field (future)
        attrs.set("dof_enabled", AttrValue::Bool(false));
        attrs.set("focus_distance", AttrValue::Float(1000.0));
        attrs.set("aperture", AttrValue::Float(2.8));
        
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
        self.attrs.attach_schema(&CAMERA_SCHEMA);
    }
    
    // === Standard layer getters ===
    
    pub fn position(&self) -> [f32; 3] {
        self.attrs.get_vec3("position").unwrap_or([0.0, 0.0, -1000.0])
    }
    
    pub fn rotation(&self) -> [f32; 3] {
        self.attrs.get_vec3("rotation").unwrap_or([0.0, 0.0, 0.0])
    }
    
    pub fn scale(&self) -> [f32; 3] {
        self.attrs.get_vec3("scale").unwrap_or([1.0, 1.0, 1.0])
    }
    
    pub fn pivot(&self) -> [f32; 3] {
        self.attrs.get_vec3("pivot").unwrap_or([0.0, 0.0, 0.0])
    }
    
    // === Camera-specific getters ===
    
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
    /// Uses either point_of_interest (if use_poi=true) or rotation angles.
    pub fn view_matrix(&self) -> Mat4 {
        let pos = self.position();
        let eye = Vec3::from(pos);
        let up = Vec3::Y; // Y-up convention
        
        if self.use_poi() {
            // Look-at mode: camera points at POI
            let target = self.point_of_interest();
            let center = Vec3::from(target);
            Mat4::look_at_rh(eye, center, up)
        } else {
            // Rotation mode: use Euler angles (degrees)
            let rot = self.rotation();
            let rot_x = rot[0].to_radians();
            let rot_y = rot[1].to_radians();
            let rot_z = rot[2].to_radians();
            
            // Build rotation matrix (XYZ order)
            let rotation = Mat4::from_euler(glam::EulerRot::XYZ, rot_x, rot_y, rot_z);
            let translation = Mat4::from_translation(-eye);
            
            rotation * translation
        }
    }
    
    /// Build projection matrix (camera -> clip space).
    /// 
    /// # Arguments
    /// - `aspect` - viewport width / height
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        let fov_rad = self.fov().to_radians();
        let near = self.near_clip();
        let far = self.far_clip();
        
        Mat4::perspective_rh_gl(fov_rad, aspect, near, far)
    }
    
    /// Build combined view-projection matrix.
    pub fn view_projection_matrix(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
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
        assert_eq!(cam.position(), [0.0, 0.0, -1000.0]);
        assert_eq!(cam.point_of_interest(), [0.0, 0.0, 0.0]);
        assert!((cam.fov() - 39.6).abs() < 0.01);
    }
    
    #[test]
    fn test_view_matrix() {
        let cam = CameraNode::new("Test");
        let view = cam.view_matrix();
        
        // View matrix should be valid (not NaN/Inf)
        assert!(!view.is_nan());
    }
    
    #[test]
    fn test_projection_matrix() {
        let cam = CameraNode::new("Test");
        let proj = cam.projection_matrix(16.0 / 9.0);
        
        // Projection matrix should be valid
        assert!(!proj.is_nan());
    }
}
