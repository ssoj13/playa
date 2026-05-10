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
//! The actual implementation lives in [`playa_time::coord`]; this module is a
//! thin re-export so existing call sites (`playa_engine::entities::space::foo`)
//! continue to compile while playa-ui, playa-engine, and any future crate share
//! a single source. See `playa-time` crate docs for the rationale (audit found
//! `frame_to_image` and `object_to_src` were bit-exact duplicates in the
//! previous code path).

pub use playa_time::coord::{
    frame_to_image, from_math_rot, image_to_frame, object_to_src, to_math_rot,
};
