//! Coordinate-space transforms.
//!
//! Single source of truth for every Y-flip / center-offset / zoom-pan / NDC
//! conversion in playa. Inline math at call sites is the historic
//! source of "fix-once-break-twice" coord bugs (gizmo lags pan, image lands
//! in the wrong corner, brackets follow opposite of zoom). All such
//! conversions belong here.
//!
//! # Spaces
//!
//! Eleven distinct conventions in use across the engine + UI + GPU:
//!
//! 1. **Buffer space** (a.k.a. legacy "image space"): top-left origin, Y-down,
//!    integer pixels — matches how every PNG/EXR/TGA loader writes rows in
//!    memory. This is purely a memory-addressing convention, not a user-facing
//!    coord system.
//! 2. **ImageNatural space** (NEW, user-facing): bottom-left origin, Y-up,
//!    integer pixels — the way humans naturally read a Cartesian plane. All
//!    user-visible image coordinates (status bar readouts, picker tooltips,
//!    layer position attrs in the long term) should be expressed here.
//! 3. **Frame space**: center origin, Y-up — used for layer transforms,
//!    effects, and viewport math. AE convention.
//! 4. **Object space**: same convention as frame space, but anchored at the
//!    layer's own source size (used by per-layer effects before transform).
//! 5. **NDC** (Normalized Device Coordinates): center origin, Y-up,
//!    `[-1, +1]` — what wgpu/OpenGL vertex shaders consume.
//! 6. **Viewport space**: center origin, Y-up, screen pixels — the result
//!    of applying viewport zoom + pan to frame space.
//! 7. **Screen space**: top-left origin, Y-down, screen pixels — egui's
//!    `Pos2` / `Vec2` convention. This is where pointer events live.
//! 8. **UV space**: `[0, 1]²`, top-left origin, Y-down (texture sampler).
//! 9. **Layer attrs space**: same as Frame, but units are "user pixels"
//!    of the comp at author time. Identical conversions; semantic alias.
//! 10. **Source frame index**: integer, start-relative.
//! 11. **Math rotation**: CCW-positive radians (for glam) vs CW-positive
//!     degrees (AE / user-facing).
//!
//! # Conversion graph
//!
//! ```text
//!  buffer  ←→  natural  ←→  frame  ←→  ndc
//!                            ↕
//!                         viewport (zoom + pan)
//!                            ↕
//!                          screen (egui)
//! ```
//!
//! `object` is a frame-shaped space anchored at a layer's src size.
//!
//! # Which helper to call
//!
//! - User typed a coordinate in the UI? Treat as **natural**, convert to
//!   frame for transform math, frame to ndc for shader.
//! - Sampling a pixel buffer? Use **buffer** indices.
//! - Drawing an overlay on top of egui screen? Convert frame → viewport
//!   → screen via the chain.

use glam::{Affine2, Mat4, Vec2, Vec4};

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

// ---------------------------------------------------------------------------
// ImageNatural ↔ buffer (legacy "image")
// ---------------------------------------------------------------------------

/// Buffer space (top-left, Y-down — pixel rows in memory) → natural image
/// space (bottom-left, Y-up — Cartesian, the way a human reads coords).
///
/// Self-inverse: `image_to_natural(image_to_natural(p, sz), sz) == p`.
#[inline]
pub fn image_to_natural(p: Vec2, size: (usize, usize)) -> Vec2 {
    let h = size.1 as f32;
    Vec2::new(p.x, h - p.y)
}

/// Natural image space → buffer space. Same formula as [`image_to_natural`]
/// (the conversion is its own inverse), kept as a separate name to mark
/// the *intent* at call sites.
#[inline]
pub fn natural_to_image(p: Vec2, size: (usize, usize)) -> Vec2 {
    image_to_natural(p, size)
}

// ---------------------------------------------------------------------------
// Natural ↔ frame
// ---------------------------------------------------------------------------

/// Natural image space (bottom-left, Y-up) → frame space (center, Y-up).
/// Pure recenter — no Y-flip, both spaces are Y-up.
///
/// Note: composing with [`image_to_natural`] reproduces the legacy
/// [`image_to_frame`] (`natural_to_frame(image_to_natural(p, sz), sz)
/// == image_to_frame(p, sz)`).
#[inline]
pub fn natural_to_frame(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x - w * 0.5, p.y - h * 0.5)
}

/// Frame space → natural image space (inverse of [`natural_to_frame`]).
#[inline]
pub fn frame_to_natural(p: Vec2, size: (usize, usize)) -> Vec2 {
    let w = size.0 as f32;
    let h = size.1 as f32;
    Vec2::new(p.x + w * 0.5, p.y + h * 0.5)
}

// ---------------------------------------------------------------------------
// Frame ↔ NDC
// ---------------------------------------------------------------------------

/// Frame space (center, Y-up, pixels) → NDC (center, Y-up, `[-1, +1]`).
///
/// `comp_size` is the full composition dimensions (the whole `[-1, +1]`
/// range maps to `[-w/2, w/2] × [-h/2, h/2]` in frame space).
///
/// wgpu / OpenGL NDC is Y-up — no flip needed.
#[inline]
pub fn frame_to_ndc(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let half_w = comp_size.0 as f32 * 0.5;
    let half_h = comp_size.1 as f32 * 0.5;
    Vec2::new(p.x / half_w, p.y / half_h)
}

/// NDC → frame space (inverse of [`frame_to_ndc`]).
#[inline]
pub fn ndc_to_frame(p: Vec2, comp_size: (usize, usize)) -> Vec2 {
    let half_w = comp_size.0 as f32 * 0.5;
    let half_h = comp_size.1 as f32 * 0.5;
    Vec2::new(p.x * half_w, p.y * half_h)
}

// ---------------------------------------------------------------------------
// Frame ↔ viewport (zoom + pan)
// ---------------------------------------------------------------------------

/// Frame space → viewport space (centered, Y-up, screen pixels).
/// Applies viewport zoom and pan.
#[inline]
pub fn frame_to_viewport(p: Vec2, zoom: f32, pan: Vec2) -> Vec2 {
    p * zoom + pan
}

/// Viewport space → frame space (inverse of [`frame_to_viewport`]).
///
/// Caller's responsibility to ensure `zoom != 0`. Mirrors the historic
/// `viewport.rs::screen_to_image` behavior — no guard, since `zoom` is
/// clamped to `[0.01, 100.0]` upstream.
#[inline]
pub fn viewport_to_frame(p: Vec2, zoom: f32, pan: Vec2) -> Vec2 {
    (p - pan) / zoom
}

// ---------------------------------------------------------------------------
// Viewport ↔ screen (egui)
// ---------------------------------------------------------------------------

/// Viewport space (centered, Y-up) → egui screen space (top-left, Y-down).
#[inline]
pub fn viewport_to_screen(p: Vec2, viewport_size: Vec2) -> Vec2 {
    Vec2::new(
        p.x + viewport_size.x * 0.5,
        viewport_size.y * 0.5 - p.y,
    )
}

/// egui screen space → viewport space (centered, Y-up).
/// Inverse of [`viewport_to_screen`] — symmetric (Y-flip both halves).
#[inline]
pub fn screen_to_viewport(p: Vec2, viewport_size: Vec2) -> Vec2 {
    Vec2::new(
        p.x - viewport_size.x * 0.5,
        viewport_size.y * 0.5 - p.y,
    )
}

// ---------------------------------------------------------------------------
// Object-space affine (matrix form of object_to_src)
// ---------------------------------------------------------------------------

/// Affine matrix that maps object space (center, Y-up) → src buffer space
/// (top-left, Y-down). Matrix form of [`object_to_src`] for use in transform
/// composition (`build_inverse_matrix_3x3`, etc.).
///
/// Decomposes as: scale (1, -1) to flip Y, then translate by (w/2, h/2)
/// to move origin from center to top-left.
#[inline]
pub fn object_to_src_affine(src_size: (usize, usize)) -> Affine2 {
    let half = Vec2::new(src_size.0 as f32 * 0.5, src_size.1 as f32 * 0.5);
    Affine2::from_translation(half) * Affine2::from_scale(Vec2::new(1.0, -1.0))
}

// ---------------------------------------------------------------------------
// Y-flip (glam Vec2)
// ---------------------------------------------------------------------------

/// Flip the Y component. Single-formula helper used wherever a Y-down
/// vector needs to become Y-up or vice versa without re-deriving the
/// negation from scratch.
#[inline]
pub fn flip_y(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

// ---------------------------------------------------------------------------
// Camera-NDC → screen-NDC Mat4 (matrix form of the viewport chain)
// ---------------------------------------------------------------------------

/// Build the 4×4 matrix that maps camera NDC (`[-1, +1]` over `comp_size`
/// in world) to screen NDC (`[-1, +1]` over `viewport_size` in screen),
/// applying viewport zoom + pan.
///
/// This is the matrix form of the chain
/// `viewport_to_screen ∘ frame_to_viewport ∘ ndc_to_frame` lifted into
/// NDC, suitable for multiplying into a camera projection matrix when
/// rendering an overlay (gizmo, brackets) on top of the camera-rendered
/// comp.
///
/// Derivation:
/// ```text
/// screen_pos = world_pos * zoom + pan
/// screen_NDC = screen_pos / (viewport / 2)
///            = (cam_NDC * comp/2 * zoom + pan) / (viewport / 2)
///            = cam_NDC * (comp * zoom / viewport) + pan * 2 / viewport
/// ```
///
/// Single source of truth for this chain. Gizmo overlay matrices and
/// the viewport image render share this algebra (cast to `DMat4` at the
/// call site if f64 precision is needed).
#[inline]
pub fn screen_ndc_from_frame_ndc(
    zoom: f32,
    pan: Vec2,
    comp_size: (usize, usize),
    viewport_size: Vec2,
) -> Mat4 {
    let comp_w = comp_size.0 as f32;
    let comp_h = comp_size.1 as f32;
    let scale_x = comp_w * zoom / viewport_size.x;
    let scale_y = comp_h * zoom / viewport_size.y;
    let trans_x = pan.x * 2.0 / viewport_size.x;
    let trans_y = pan.y * 2.0 / viewport_size.y;
    Mat4::from_cols(
        Vec4::new(scale_x, 0.0, 0.0, 0.0),
        Vec4::new(0.0, scale_y, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(trans_x, trans_y, 0.0, 1.0),
    )
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

    // ----- ImageNatural ↔ buffer (legacy image) -----

    #[test]
    fn image_to_natural_corners() {
        let size = (1920, 1080);
        // top-left buffer pixel → bottom-left in natural (y flipped)
        assert!(approx_eq(
            image_to_natural(Vec2::new(0.0, 0.0), size),
            Vec2::new(0.0, 1080.0)
        ));
        // bottom-right buffer pixel → top-right in natural
        assert!(approx_eq(
            image_to_natural(Vec2::new(1920.0, 1080.0), size),
            Vec2::new(1920.0, 0.0)
        ));
        // center stays in center
        assert!(approx_eq(
            image_to_natural(Vec2::new(960.0, 540.0), size),
            Vec2::new(960.0, 540.0)
        ));
    }

    #[test]
    fn image_natural_self_inverse() {
        let size = (640, 480);
        for p in [
            Vec2::new(0.0, 0.0),
            Vec2::new(123.0, 456.0),
            Vec2::new(640.0, 480.0),
        ] {
            assert!(approx_eq(natural_to_image(image_to_natural(p, size), size), p));
        }
    }

    // ----- Natural ↔ frame -----

    #[test]
    fn natural_to_frame_corners() {
        let size = (1920, 1080);
        // bottom-left of natural → bottom-left of frame
        assert!(approx_eq(
            natural_to_frame(Vec2::new(0.0, 0.0), size),
            Vec2::new(-960.0, -540.0)
        ));
        // top-right of natural → top-right of frame
        assert!(approx_eq(
            natural_to_frame(Vec2::new(1920.0, 1080.0), size),
            Vec2::new(960.0, 540.0)
        ));
        // center in natural → origin in frame
        assert!(approx_eq(
            natural_to_frame(Vec2::new(960.0, 540.0), size),
            Vec2::ZERO
        ));
    }

    #[test]
    fn natural_frame_round_trip() {
        let size = (640, 480);
        for p in [Vec2::ZERO, Vec2::new(100.0, 200.0), Vec2::new(640.0, 480.0)] {
            assert!(approx_eq(frame_to_natural(natural_to_frame(p, size), size), p));
        }
    }

    /// Algebra check: legacy `image_to_frame` must equal the new chain
    /// `natural_to_frame ∘ image_to_natural`. If this ever fails, one of the
    /// helpers drifted from the others.
    #[test]
    fn image_to_frame_equals_natural_chain() {
        let size = (800, 600);
        for p in [
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(400.0, 300.0),
            Vec2::new(800.0, 600.0),
            Vec2::new(123.4, 567.8),
        ] {
            let direct = image_to_frame(p, size);
            let chained = natural_to_frame(image_to_natural(p, size), size);
            assert!(
                approx_eq(direct, chained),
                "p={:?} direct={:?} chained={:?}",
                p,
                direct,
                chained
            );
        }
    }

    // ----- Frame ↔ NDC -----

    #[test]
    fn frame_to_ndc_corners() {
        let size = (1920, 1080);
        assert!(approx_eq(frame_to_ndc(Vec2::ZERO, size), Vec2::ZERO));
        assert!(approx_eq(
            frame_to_ndc(Vec2::new(960.0, 540.0), size),
            Vec2::new(1.0, 1.0)
        ));
        assert!(approx_eq(
            frame_to_ndc(Vec2::new(-960.0, -540.0), size),
            Vec2::new(-1.0, -1.0)
        ));
    }

    #[test]
    fn frame_ndc_round_trip() {
        let size = (1280, 720);
        for p in [
            Vec2::ZERO,
            Vec2::new(640.0, 360.0),
            Vec2::new(-100.0, 50.0),
        ] {
            assert!(approx_eq(ndc_to_frame(frame_to_ndc(p, size), size), p));
        }
    }

    // ----- Frame ↔ viewport (zoom + pan) -----

    #[test]
    fn frame_to_viewport_identity_at_unity() {
        // zoom=1, pan=0 → no-op
        for p in [Vec2::ZERO, Vec2::new(100.0, -50.0)] {
            assert!(approx_eq(
                frame_to_viewport(p, 1.0, Vec2::ZERO),
                p
            ));
        }
    }

    #[test]
    fn frame_viewport_round_trip() {
        let pan = Vec2::new(37.5, -22.0);
        for &zoom in &[0.5_f32, 1.0, 2.5, 100.0] {
            for p in [Vec2::ZERO, Vec2::new(100.0, 200.0), Vec2::new(-50.0, 0.0)] {
                let back = viewport_to_frame(frame_to_viewport(p, zoom, pan), zoom, pan);
                assert!(approx_eq(back, p), "zoom={} p={:?} back={:?}", zoom, p, back);
            }
        }
    }

    // ----- Viewport ↔ screen -----

    #[test]
    fn viewport_to_screen_corners() {
        let vp = Vec2::new(1000.0, 600.0);
        // viewport origin (0,0) → screen center
        assert!(approx_eq(
            viewport_to_screen(Vec2::ZERO, vp),
            Vec2::new(500.0, 300.0)
        ));
        // viewport (+x, +y) → screen (right, UP, so smaller y)
        assert!(approx_eq(
            viewport_to_screen(Vec2::new(100.0, 100.0), vp),
            Vec2::new(600.0, 200.0)
        ));
    }

    #[test]
    fn viewport_screen_round_trip() {
        let vp = Vec2::new(800.0, 600.0);
        for p in [Vec2::ZERO, Vec2::new(123.0, -45.0), Vec2::new(-200.0, 150.0)] {
            assert!(approx_eq(screen_to_viewport(viewport_to_screen(p, vp), vp), p));
        }
    }

    // ----- object_to_src_affine -----

    // ----- flip_y -----

    #[test]
    fn flip_y_negates_only_y() {
        assert_eq!(flip_y(Vec2::new(3.0, 4.0)), Vec2::new(3.0, -4.0));
        assert_eq!(flip_y(Vec2::new(-1.0, -2.0)), Vec2::new(-1.0, 2.0));
        assert_eq!(flip_y(Vec2::ZERO), Vec2::ZERO);
    }

    #[test]
    fn flip_y_self_inverse() {
        for p in [Vec2::ZERO, Vec2::new(7.5, -3.25), Vec2::new(100.0, 200.0)] {
            assert_eq!(flip_y(flip_y(p)), p);
        }
    }

    // ----- screen_ndc_from_frame_ndc -----

    #[test]
    fn screen_ndc_identity_when_comp_matches_viewport() {
        // zoom=1, pan=0, comp_size == viewport_size → matrix is identity on
        // the (x, y) channels (z and w pass through).
        let comp = (1920, 1080);
        let vp = Vec2::new(1920.0, 1080.0);
        let m = screen_ndc_from_frame_ndc(1.0, Vec2::ZERO, comp, vp);

        // Apply to a few NDC corners; result should match input.
        for ndc in [Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(-1.0, 0.5)] {
            let v = m * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
            assert!(
                (v.x - ndc.x).abs() < 1e-5 && (v.y - ndc.y).abs() < 1e-5,
                "ndc={:?} got=({}, {})",
                ndc,
                v.x,
                v.y
            );
        }
    }

    #[test]
    fn screen_ndc_pan_only_translates() {
        // zoom=1, pan=(50, -25), comp == vp.
        // Expected NDC translation = (50*2/1920, -25*2/1080).
        let comp = (1920, 1080);
        let vp = Vec2::new(1920.0, 1080.0);
        let pan = Vec2::new(50.0, -25.0);
        let m = screen_ndc_from_frame_ndc(1.0, pan, comp, vp);

        // Origin transforms to translation.
        let v = m * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let expected = (pan.x * 2.0 / vp.x, pan.y * 2.0 / vp.y);
        assert!(
            (v.x - expected.0).abs() < 1e-5 && (v.y - expected.1).abs() < 1e-5,
            "got=({}, {}) expected=({}, {})",
            v.x,
            v.y,
            expected.0,
            expected.1
        );
    }

    #[test]
    fn screen_ndc_zoom_only_scales() {
        // zoom=2, pan=0, comp == vp.
        // Expected scale = comp/vp * 2 = 2 on each axis.
        let comp = (800, 600);
        let vp = Vec2::new(800.0, 600.0);
        let m = screen_ndc_from_frame_ndc(2.0, Vec2::ZERO, comp, vp);

        let v = m * Vec4::new(0.5, 0.5, 0.0, 1.0);
        assert!(
            (v.x - 1.0).abs() < 1e-5 && (v.y - 1.0).abs() < 1e-5,
            "got=({}, {}) expected=(1, 1)",
            v.x,
            v.y
        );
    }

    /// Algebra check: matrix application equals the point chain
    /// `viewport_to_screen ∘ frame_to_viewport ∘ ndc_to_frame` lifted
    /// into NDC. If this fails, gizmo and viewport.rs have diverged.
    #[test]
    fn screen_ndc_matches_point_chain() {
        let comp = (1280, 720);
        let vp = Vec2::new(1600.0, 900.0);
        let zoom = 1.5_f32;
        let pan = Vec2::new(40.0, -20.0);
        let m = screen_ndc_from_frame_ndc(zoom, pan, comp, vp);

        for ndc in [Vec2::ZERO, Vec2::new(1.0, 1.0), Vec2::new(-0.5, 0.25)] {
            // Via matrix
            let via_matrix = m * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);

            // Via point chain → screen pos → screen NDC.
            let frame = ndc_to_frame(ndc, comp);
            let viewport = frame_to_viewport(frame, zoom, pan);
            // Convert viewport (centered, Y-up) to NDC (centered, Y-up,
            // [-1,+1] over viewport_size). NOTE: NOT viewport_to_screen
            // — that flips Y for egui consumption. Here we want NDC
            // before any Y-flip.
            let via_chain = Vec2::new(viewport.x * 2.0 / vp.x, viewport.y * 2.0 / vp.y);

            assert!(
                (via_matrix.x - via_chain.x).abs() < 1e-4
                    && (via_matrix.y - via_chain.y).abs() < 1e-4,
                "ndc={:?}: matrix=({}, {}) chain=({}, {})",
                ndc,
                via_matrix.x,
                via_matrix.y,
                via_chain.x,
                via_chain.y
            );
        }
    }

    #[test]
    fn object_to_src_affine_matches_point_helper() {
        let size = (640, 480);
        let m = object_to_src_affine(size);
        for p in [Vec2::ZERO, Vec2::new(100.0, 50.0), Vec2::new(-100.0, -50.0)] {
            let via_matrix = m.transform_point2(p);
            let via_helper = object_to_src(p, size);
            assert!(
                approx_eq(via_matrix, via_helper),
                "p={:?} matrix={:?} helper={:?}",
                p,
                via_matrix,
                via_helper
            );
        }
    }
}
