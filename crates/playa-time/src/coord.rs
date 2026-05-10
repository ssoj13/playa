//! Coordinate-space transforms.
//!
//! Verbatim from the previous `playa-engine::entities::space` module — extracted
//! into `playa-time` so callers across crates share one source of truth.
//!
//! # Spaces
//!
//! - **Image space**: top-left origin, Y-down (matches loaded pixel buffers).
//! - **Frame space**: center origin, Y-up (matches transform/effects math).
//! - **Object space**: same convention as frame space, but anchored at the
//!   layer's source size (used by per-layer effects before transform).

use glam::Vec2;

/// Image space → frame space.
///
/// - Image (0, 0) → Frame (-w/2, h/2)
/// - Image (w/2, h/2) → Frame (0, 0)
/// - Image (w, h) → Frame (w/2, -h/2)
#[inline]
pub fn image_to_frame(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x - w * 0.5, h * 0.5 - p.y)
}

/// Frame space → image space (inverse of [`image_to_frame`]).
#[inline]
pub fn frame_to_image(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x + w * 0.5, h * 0.5 - p.y)
}

/// Object space → source-pixel space. Mathematically identical to
/// [`frame_to_image`] (object-space and frame-space share the same center-origin
/// Y-up convention); kept as a named alias for call-site readability and to mark
/// the *intent* of operating on a layer's source rather than the comp's frame.
#[inline]
pub fn object_to_src(p: Vec2, src_size: (usize, usize)) -> Vec2 {
    frame_to_image(p, src_size)
}

/// User rotation (CW-positive, degrees) → math rotation (CCW-positive, radians).
///
/// User convention matches After Effects and common UI expectation.
/// Math convention matches glam, OpenGL, and standard mathematics.
#[inline]
pub fn to_math_rot(deg: f32) -> f32 {
    -deg.to_radians()
}

/// Math rotation (CCW-positive, radians) → user rotation (CW-positive, degrees).
#[inline]
pub fn from_math_rot(rad: f32) -> f32 {
    -rad.to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn image_to_frame_corners() {
        let size = (1920, 1080);
        assert!(approx_eq(
            image_to_frame(Vec2::new(0.0, 0.0), size),
            Vec2::new(-960.0, 540.0)
        ));
        assert!(approx_eq(
            image_to_frame(Vec2::new(960.0, 540.0), size),
            Vec2::ZERO
        ));
        assert!(approx_eq(
            image_to_frame(Vec2::new(1920.0, 1080.0), size),
            Vec2::new(960.0, -540.0)
        ));
    }

    #[test]
    fn image_frame_round_trip() {
        let size = (1920, 1080);
        for p in [
            Vec2::new(0.0, 0.0),
            Vec2::new(123.0, 456.0),
            Vec2::new(960.0, 540.0),
            Vec2::new(-50.0, 700.0),
        ] {
            let back = image_to_frame(frame_to_image(p, size), size);
            assert!(approx_eq(p, back), "round-trip failed at {:?} → {:?}", p, back);
        }
    }

    #[test]
    fn object_to_src_matches_frame_to_image() {
        let size = (640, 480);
        for p in [Vec2::ZERO, Vec2::new(10.0, -20.0)] {
            assert_eq!(object_to_src(p, size), frame_to_image(p, size));
        }
    }

    #[test]
    fn rotation_round_trip() {
        for deg in [0.0_f32, 45.0, 90.0, -90.0, 270.0, 359.5] {
            let rad = to_math_rot(deg);
            let back = from_math_rot(rad);
            assert!((back - deg).abs() < 1e-3, "deg={} round-trip={}", deg, back);
        }
    }
}
