//! Layer playback speed: signed (sign = direction), magnitude floored at
//! [`Speed::MIN_MAGNITUDE`] (0.001).
//!
//! Replaces two divergent clamp policies in the previous codebase:
//! - `attrs.rs` used `clamp(0.1, 4.0)` (UI-slider range).
//! - `comp_node.rs` used `.abs().max(0.001)` (allowed reverse + extreme slow-mo).
//!
//! The `comp_node.rs` policy is canonical going forward: negative speed = reverse
//! playback, magnitude floored at 0.001 to prevent div-by-zero in timeline-frame
//! conversions.

use serde::{Deserialize, Serialize};

use crate::round::Round;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Speed(f32);

impl Speed {
    /// Smallest absolute speed magnitude allowed; below this the timeline-frame
    /// math would explode.
    pub const MIN_MAGNITUDE: f32 = 0.001;

    pub const ONE: Self = Self(1.0);

    /// Construct a sanitised speed: keeps the sign, floors the magnitude at
    /// [`Self::MIN_MAGNITUDE`]. Negative speeds represent reverse playback.
    pub fn new(v: f32) -> Self {
        // Treat NaN as forward 1.0 — never let it propagate.
        if v.is_nan() {
            return Self::ONE;
        }
        let sign = if v < 0.0 { -1.0_f32 } else { 1.0_f32 };
        let mag = v.abs().max(Self::MIN_MAGNITUDE);
        Self(sign * mag)
    }

    /// Underlying signed speed value.
    #[inline]
    pub fn raw(self) -> f32 {
        self.0
    }

    /// Magnitude, always ≥ [`Self::MIN_MAGNITUDE`].
    #[inline]
    pub fn magnitude(self) -> f32 {
        self.0.abs()
    }

    /// True iff layer plays in reverse.
    #[inline]
    pub fn is_reverse(self) -> bool {
        self.0 < 0.0
    }

    /// Convert source frames to timeline frames at this speed.
    /// `src_frames=10, speed=0.5` ⇒ 20 timeline frames (slow-mo). Sign is **not**
    /// applied to the count — direction is consumed at playhead-step time, not at
    /// duration-measurement time.
    #[inline]
    pub fn scale_src_to_timeline(self, src_frames: i32, mode: Round) -> i32 {
        let v = (src_frames as f64) / (self.magnitude() as f64);
        mode.to_i32(v)
    }

    /// Convert timeline frames to source frames at this speed (inverse of
    /// [`Self::scale_src_to_timeline`]).
    #[inline]
    pub fn scale_timeline_to_src(self, tl_frames: i32, mode: Round) -> i32 {
        let v = (tl_frames as f64) * (self.magnitude() as f64);
        mode.to_i32(v)
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self::ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_floors_magnitude_below_limit() {
        assert_eq!(Speed::new(0.0).magnitude(), Speed::MIN_MAGNITUDE);
        assert_eq!(Speed::new(0.0001).magnitude(), Speed::MIN_MAGNITUDE);
        assert_eq!(Speed::new(-0.0001).magnitude(), Speed::MIN_MAGNITUDE);
        assert!(Speed::new(-0.0001).is_reverse());
    }

    #[test]
    fn new_preserves_normal_values() {
        assert_eq!(Speed::new(0.5).raw(), 0.5);
        assert_eq!(Speed::new(-2.0).raw(), -2.0);
        assert_eq!(Speed::new(100.0).raw(), 100.0);
    }

    #[test]
    fn nan_input_returns_unit_speed() {
        let s = Speed::new(f32::NAN);
        assert_eq!(s.raw(), 1.0);
    }

    #[test]
    fn slow_mo_doubles_timeline_count() {
        let s = Speed::new(0.5);
        assert_eq!(s.scale_src_to_timeline(10, Round::Round), 20);
    }

    #[test]
    fn fast_mo_halves_timeline_count() {
        let s = Speed::new(2.0);
        assert_eq!(s.scale_src_to_timeline(10, Round::Round), 5);
    }

    #[test]
    fn reverse_does_not_affect_count() {
        let s = Speed::new(-1.0);
        assert_eq!(s.scale_src_to_timeline(10, Round::Round), 10);
        assert!(s.is_reverse());
    }

    #[test]
    fn round_modes_distinguishable_for_non_integer_speed() {
        let s = Speed::new(1.5);
        // 10 / 1.5 = 6.666...
        assert_eq!(s.scale_src_to_timeline(10, Round::Floor), 6);
        assert_eq!(s.scale_src_to_timeline(10, Round::Round), 7);
        assert_eq!(s.scale_src_to_timeline(10, Round::Ceil), 7);
        assert_eq!(s.scale_src_to_timeline(10, Round::Trunc), 6);
    }

    #[test]
    fn round_trip_within_one_frame() {
        let s = Speed::new(1.5);
        let tl = s.scale_src_to_timeline(100, Round::Round);
        let back = s.scale_timeline_to_src(tl, Round::Round);
        assert!((back - 100).abs() <= 1, "100 → {} → {}", tl, back);
    }
}
