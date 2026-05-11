//! Time + coordinate conversion primitives for playa.
//!
//! # Why this crate exists
//!
//! Before `playa-time`, conversions like `src_len / speed` were duplicated across
//! `attrs.rs`, `comp_node.rs`, and `timeline_ui.rs` with three different rounding
//! modes (`.round()`, `.ceil()`, `as i32` truncation) and two divergent speed-clamp
//! policies (`clamp(0.1, 4.0)` vs `.abs().max(0.001)`). Same input → different
//! output depending on which call site the caller landed on. This crate is the
//! single source of truth.
//!
//! # Canonical units
//!
//! - **Frame index**: `i32`. Negative frames are first-class (After Effects-style
//!   infinite timeline support).
//! - **Frame rate**: [`Fps`] as exact `(num, den)` rational. Avoids f32 drift on
//!   NTSC fractional rates over long durations.
//! - **Speed**: [`Speed`] f32 with explicit sign (negative = reverse playback) and
//!   magnitude floored at `0.001` to prevent div-by-zero.
//!
//! Coordinate-space transforms previously lived here as a `coord` module;
//! they were extracted to the sibling `playa-coord` crate to keep this
//! crate scoped to time / rate.
//!
//! # Conversions are explicit about rounding
//!
//! Every helper that converts between integer frame counts takes a [`Round`] mode.
//! Callers must choose; there is no implicit default. The previous codebase mixed
//! `.round()`, `.ceil()`, and `as i32` (truncate) on the same expression in
//! different files — those bugs were silent and visible only on non-integer
//! `speed` values.

#![forbid(unsafe_code)]

pub mod conversion;
pub mod fps;
pub mod round;
pub mod speed;
pub mod timecode;

pub use conversion::{frames_to_seconds, seconds_to_frames};
pub use fps::Fps;
pub use round::Round;
pub use speed::Speed;
pub use timecode::{TimeDisplay, Timecode, format_time, parse_time};
