//! Coordinate space conversions (Y-up) for comp/layer sampling.
//!
//! Conventions:
//! - Comp space: origin at left-bottom, +Y up (pixels).
//! - Viewport space: origin at viewport center, +Y up (pixels).
//! - Object space: origin at layer center, +Y up (pixels).
//! - Image space: origin at top-left, +Y down (pixels).

use glam::Vec2;

#[inline]
pub fn comp_to_viewport(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let w = comp_size.0 as f32;
    let h = comp_size.1 as f32;
    p - Vec2::new(w * 0.5, h * 0.5)
}

#[inline]
pub fn viewport_to_comp(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let w = comp_size.0 as f32;
    let h = comp_size.1 as f32;
    p + Vec2::new(w * 0.5, h * 0.5)
}

#[inline]
pub fn image_to_comp(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let h = comp_size.1 as f32;
    Vec2::new(p.x, h - p.y)
}

#[allow(dead_code)]
#[inline]
pub fn comp_to_image(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let h = comp_size.1 as f32;
    Vec2::new(p.x, h - p.y)
}

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
