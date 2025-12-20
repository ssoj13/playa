//! Coordinate space conversions for compositing.
//!
//! ## Coordinate Spaces (simplified)
//!
//! - **Image space**: origin top-left, +Y down (pixels).
//!   Standard image/texture coordinates for sampling.
//!
//! - **Frame space**: origin CENTER, +Y up (pixels).
//!   Used for layer transforms and viewport/gizmo.
//!   `position = (0,0,0)` = layer centered in comp.
//!   Same as viewport space - no conversion needed for gizmo!
//!
//! - **Object space**: origin at layer center, +Y up (pixels).
//!   Local space for rotation/scale around pivot.
//!
//! ## Transform Pipeline
//!
//! ```text
//! Screen pixel (image space)
//!     |  image_to_frame()
//!     v
//! Frame space (centered, Y-up)
//!     |  inverse model transform
//!     v
//! Object space (layer center)
//!     |  object_to_src()
//!     v
//! Source pixel (for texture sampling)
//! ```

use glam::Vec2;

// =============================================================================
// Frame Space (centered, Y-up) - PRIMARY coordinate system for transforms
// =============================================================================

/// Image space -> Frame space (centered, Y-up).
/// 
/// Converts screen/image pixel coords to centered frame coords.
/// - Image (0, 0) = top-left -> Frame (-w/2, h/2)
/// - Image (w/2, h/2) = center -> Frame (0, 0)
/// - Image (w, h) = bottom-right -> Frame (w/2, -h/2)
#[inline]
pub fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x - w * 0.5, h * 0.5 - p.y)
}

/// Frame space -> Image space.
/// 
/// Inverse of image_to_frame.
#[inline]
pub fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x + w * 0.5, h * 0.5 - p.y)
}

// =============================================================================
// Object Space (layer center origin)
// =============================================================================

#[inline]
pub fn object_to_src(p: Vec2, src_size: (usize, usize)) -> Vec2 {
    let w = src_size.0 as f32;
    let h = src_size.1 as f32;
    Vec2::new(p.x + w * 0.5, h * 0.5 - p.y)
}

#[allow(dead_code)]
#[inline]
pub fn src_to_object(p: Vec2, src_size: (usize, usize)) -> Vec2 {
    let w = src_size.0 as f32;
    let h = src_size.1 as f32;
    Vec2::new(p.x - w * 0.5, h * 0.5 - p.y)
}
