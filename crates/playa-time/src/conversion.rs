//! Frame ↔ seconds conversions using exact-rational [`Fps`].

use crate::fps::Fps;
use crate::round::Round;

/// Convert frame index to seconds. Negative frames produce negative seconds
/// (After Effects-style infinite timeline support).
#[inline]
pub fn frames_to_seconds(frame: i32, fps: Fps) -> f64 {
    (frame as f64) * fps.frame_duration_secs()
}

/// Convert seconds to frame index with explicit rounding mode.
#[inline]
pub fn seconds_to_frames(secs: f64, fps: Fps, mode: Round) -> i32 {
    mode.to_i32(secs * fps.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_fps_round_trip() {
        for frame in [-1000, -1, 0, 1, 24, 100, 100_000] {
            let secs = frames_to_seconds(frame, Fps::FPS_24);
            let back = seconds_to_frames(secs, Fps::FPS_24, Round::Round);
            assert_eq!(back, frame, "frame={}", frame);
        }
    }

    #[test]
    fn ntsc_round_trip_first_minute() {
        for frame in [-1000, 0, 1, 24, 1438] {
            let secs = frames_to_seconds(frame, Fps::NTSC_24);
            let back = seconds_to_frames(secs, Fps::NTSC_24, Round::Round);
            assert_eq!(back, frame, "frame={}", frame);
        }
    }

    #[test]
    fn ntsc_no_drift_over_an_hour() {
        // Round-trip via f64 should be exact for any integer frame within an hour
        // when fps is the rational 24000/1001.
        let target = 86_313_i32;
        let secs = frames_to_seconds(target, Fps::NTSC_24);
        let back = seconds_to_frames(secs, Fps::NTSC_24, Round::Round);
        assert_eq!(back, target);
    }

    #[test]
    fn negative_seconds_give_negative_frames() {
        assert_eq!(seconds_to_frames(-1.0, Fps::FPS_24, Round::Round), -24);
    }

    #[test]
    fn round_modes_observable_at_subframe_seconds() {
        let secs = 1.0 / 48.0; // half a frame at 24fps
        assert_eq!(seconds_to_frames(secs, Fps::FPS_24, Round::Floor), 0);
        assert_eq!(seconds_to_frames(secs, Fps::FPS_24, Round::Ceil), 1);
        // Round at exactly 0.5 — Rust `round()` ties away from zero ⇒ 1.
        assert_eq!(seconds_to_frames(secs, Fps::FPS_24, Round::Round), 1);
    }
}
