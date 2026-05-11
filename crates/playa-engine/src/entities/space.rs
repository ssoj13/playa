//! Coordinate space conversions for compositing.
//!
//! ## Coordinate Spaces (simplified)
//!
//! - **Image space**: origin top-left, +Y down (pixels). Standard image/texture
//!   coordinates for sampling.
//! - **Frame space**: origin CENTER, +Y up (pixels). Used for layer transforms
//!   and viewport/gizmo. `position = (0,0,0)` = layer centered in comp. Same as
//!   viewport space — no conversion needed for gizmo.
//! - **Object space**: origin at layer center, +Y up (pixels). Local space for
//!   rotation/scale around pivot.
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
//!
//! The actual implementation lives in the `playa-coord` crate; this module
//! is a thin re-export so existing call sites (`playa_engine::entities::space::foo`)
//! continue to compile while playa-ui, playa-engine, and any future crate share
//! a single source. The crate split was to keep coord transforms independent
//! of time/rate primitives in `playa-time`.

pub use playa_coord::{
    flip_y, frame_to_image, frame_to_natural, frame_to_ndc, frame_to_viewport, from_math_rot,
    image_to_frame, image_to_natural, natural_to_frame, natural_to_image, ndc_to_frame,
    object_to_src, object_to_src_affine, screen_ndc_from_frame_ndc, screen_to_viewport,
    to_math_rot, viewport_to_frame, viewport_to_screen,
};
