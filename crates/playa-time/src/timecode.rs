//! SMPTE-style timecode with NDF and 12M-1 drop-frame support.
//!
//! Drop-frame is only valid for the NTSC rates 29.97 ([`Fps::NTSC_30`]) and 59.94
//! ([`Fps::NTSC_60`]); calling DF helpers with other rates falls back to NDF and
//! logs nothing — the caller is responsible for gating via
//! [`Fps::is_drop_frame_eligible`].
//!
//! Negative frame indices produce [`Timecode`] values with `negative = true`;
//! the absolute hh:mm:ss:ff field is the timecode of `-frame`. SMPTE itself does
//! not define negative timecode, so the formatted form prefixes a `-`.

use serde::{Deserialize, Serialize};

use crate::fps::Fps;

/// SMPTE-style hh:mm:ss(:|;)ff with sign and drop-frame marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Timecode {
    pub negative: bool,
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub frames: u32,
    pub drop_frame: bool,
}

impl Timecode {
    /// Construct from a (signed) frame index.
    ///
    /// `drop_frame` is honoured only when [`Fps::is_drop_frame_eligible`] is true;
    /// otherwise the conversion falls back to NDF (and `self.drop_frame` is set
    /// to `false`).
    pub fn from_frame(frame: i32, fps: Fps, drop_frame: bool) -> Self {
        let negative = frame < 0;
        let abs = (frame as i64).unsigned_abs();
        let use_df = drop_frame && fps.is_drop_frame_eligible();
        let (h, m, s, f) = if use_df {
            df_frame_to_smpte(abs, fps)
        } else {
            ndf_frame_to_smpte(abs, fps)
        };
        Self {
            negative,
            hours: h,
            minutes: m,
            seconds: s,
            frames: f,
            drop_frame: use_df,
        }
    }

    /// Convert back to a signed frame index.
    pub fn to_frame(self, fps: Fps) -> i32 {
        let total = if self.drop_frame && fps.is_drop_frame_eligible() {
            smpte_to_df_frame(self.hours, self.minutes, self.seconds, self.frames, fps)
        } else {
            smpte_to_ndf_frame(self.hours, self.minutes, self.seconds, self.frames, fps)
        };
        let signed = total as i64;
        let signed = if self.negative { -signed } else { signed };
        if signed >= i32::MAX as i64 {
            i32::MAX
        } else if signed <= i32::MIN as i64 {
            i32::MIN
        } else {
            signed as i32
        }
    }
}

// -----------------------------------------------------------------------------
// SMPTE 12M conversion math
// -----------------------------------------------------------------------------

/// NDF: simple division by nominal fps.
fn ndf_frame_to_smpte(frame: u64, fps: Fps) -> (u32, u32, u32, u32) {
    let n = fps.nominal() as u64;
    let frames = (frame % n) as u32;
    let total_secs = frame / n;
    let seconds = (total_secs % 60) as u32;
    let total_mins = total_secs / 60;
    let minutes = (total_mins % 60) as u32;
    let hours = (total_mins / 60) as u32;
    (hours, minutes, seconds, frames)
}

fn smpte_to_ndf_frame(h: u32, m: u32, s: u32, f: u32, fps: Fps) -> u64 {
    let n = fps.nominal() as u64;
    (h as u64 * 3600 + m as u64 * 60 + s as u64) * n + f as u64
}

/// SMPTE 12M-1 drop-frame conversion.
///
/// Reference: 29.97 drops 2 frames at the start of every minute except every
/// 10th. 59.94 drops 4 frames in the same pattern. Other rates would call this
/// only by mistake — caller has already gated via [`Fps::is_drop_frame_eligible`].
fn df_frame_to_smpte(frame: u64, fps: Fps) -> (u32, u32, u32, u32) {
    let (fps_int, drop) = match fps {
        Fps::NTSC_30 => (30u64, 2u64),
        Fps::NTSC_60 => (60u64, 4u64),
        _ => return ndf_frame_to_smpte(frame, fps),
    };
    let frames_per_10min = fps_int * 60 * 10 - drop * 9;
    let frames_per_min_after_drop = fps_int * 60 - drop;

    let d = frame / frames_per_10min;
    let m = frame % frames_per_10min;

    // Add drops back so the resulting hh:mm:ss:ff lines up with wall-clock.
    let adjusted = if m > drop {
        frame + drop * 9 * d + drop * ((m - drop) / frames_per_min_after_drop)
    } else {
        frame + drop * 9 * d
    };

    let frames = (adjusted % fps_int) as u32;
    let total_secs = adjusted / fps_int;
    let seconds = (total_secs % 60) as u32;
    let total_mins = total_secs / 60;
    let minutes = (total_mins % 60) as u32;
    let hours = (total_mins / 60) as u32;
    (hours, minutes, seconds, frames)
}

fn smpte_to_df_frame(h: u32, m: u32, s: u32, f: u32, fps: Fps) -> u64 {
    let (fps_int, drop) = match fps {
        Fps::NTSC_30 => (30u64, 2u64),
        Fps::NTSC_60 => (60u64, 4u64),
        _ => return smpte_to_ndf_frame(h, m, s, f, fps),
    };
    let total_minutes = h as u64 * 60 + m as u64;
    let nominal = (h as u64 * 3600 + m as u64 * 60 + s as u64) * fps_int + f as u64;
    nominal.saturating_sub(drop * (total_minutes - total_minutes / 10))
}

// -----------------------------------------------------------------------------
// Display + parsing
// -----------------------------------------------------------------------------

/// How a frame index is rendered for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeDisplay {
    /// Raw frame index, e.g. `1234`.
    Frames,
    /// Seconds with three decimal digits, e.g. `12.345`.
    Seconds,
    /// SMPTE timecode. `drop_frame=true` is only honoured for NTSC rates.
    Timecode { drop_frame: bool },
}

impl Default for TimeDisplay {
    fn default() -> Self {
        Self::Frames
    }
}

/// Render a frame index according to `mode`.
pub fn format_time(frame: i32, fps: Fps, mode: TimeDisplay) -> String {
    match mode {
        TimeDisplay::Frames => frame.to_string(),
        TimeDisplay::Seconds => {
            let secs = (frame as f64) * fps.frame_duration_secs();
            format!("{:.3}", secs)
        }
        TimeDisplay::Timecode { drop_frame } => {
            let tc = Timecode::from_frame(frame, fps, drop_frame);
            let sep = if tc.drop_frame { ';' } else { ':' };
            let sign = if tc.negative { "-" } else { "" };
            format!(
                "{}{:02}:{:02}:{:02}{}{:02}",
                sign, tc.hours, tc.minutes, tc.seconds, sep, tc.frames
            )
        }
    }
}

/// Parse a user-entered time string into a frame index.
///
/// Accepts:
/// - Raw frame index: `1234`, `-1234`.
/// - Seconds: `12.345`, `12.345s` (any trailing `s` ignored).
/// - Timecode NDF: `hh:mm:ss:ff` or shorter (`mm:ss:ff`, `ss:ff`).
/// - Timecode DF: same shapes with `;` between seconds and frames.
///
/// Returns `None` on malformed input or on out-of-range numeric components.
pub fn parse_time(s: &str, fps: Fps, mode: TimeDisplay) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Sign extraction.
    let (sign, rest) = if let Some(rest) = s.strip_prefix('-') {
        (-1i64, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (1i64, rest)
    } else {
        (1i64, s)
    };

    let frame: i64 = match mode {
        TimeDisplay::Frames => rest.parse::<i64>().ok()?,
        TimeDisplay::Seconds => {
            let trimmed = rest.trim_end_matches(|c: char| c == 's' || c == 'S').trim();
            let secs: f64 = trimmed.parse::<f64>().ok()?;
            (secs * fps.as_f64()).round() as i64
        }
        TimeDisplay::Timecode { drop_frame } => parse_timecode(rest, fps, drop_frame)? as i64,
    };

    let signed = sign * frame;
    if signed >= i32::MAX as i64 {
        Some(i32::MAX)
    } else if signed <= i32::MIN as i64 {
        Some(i32::MIN)
    } else {
        Some(signed as i32)
    }
}

fn parse_timecode(s: &str, fps: Fps, drop_frame: bool) -> Option<u64> {
    // Replace `;` (DF separator) with `:` for uniform splitting; auto-detect DF
    // when `;` is present in the input regardless of the requested flag.
    let detected_df = s.contains(';');
    let normalised: String = s.replace(';', ":");
    let parts: Vec<&str> = normalised.split(':').collect();
    if parts.is_empty() || parts.len() > 4 {
        return None;
    }

    // Right-align parts so that single-segment inputs are interpreted as frames,
    // two segments as `ss:ff`, three as `mm:ss:ff`, four as `hh:mm:ss:ff`.
    let mut h = 0u32;
    let mut m = 0u32;
    let mut sec = 0u32;
    let f: u32;
    match parts.len() {
        1 => {
            f = parts[0].parse().ok()?;
        }
        2 => {
            sec = parts[0].parse().ok()?;
            f = parts[1].parse().ok()?;
        }
        3 => {
            m = parts[0].parse().ok()?;
            sec = parts[1].parse().ok()?;
            f = parts[2].parse().ok()?;
        }
        4 => {
            h = parts[0].parse().ok()?;
            m = parts[1].parse().ok()?;
            sec = parts[2].parse().ok()?;
            f = parts[3].parse().ok()?;
        }
        _ => unreachable!(),
    }

    if m >= 60 || sec >= 60 {
        return None;
    }
    let nominal = fps.nominal();
    if nominal > 0 && f >= nominal {
        return None;
    }

    let use_df = (drop_frame || detected_df) && fps.is_drop_frame_eligible();
    if use_df {
        Some(smpte_to_df_frame(h, m, sec, f, fps))
    } else {
        Some(smpte_to_ndf_frame(h, m, sec, f, fps))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndf_round_trip_24fps() {
        let fps = Fps::FPS_24;
        for &frame in &[0, 1, 23, 24, 25, 1_000_000, -42, -86_400] {
            let tc = Timecode::from_frame(frame, fps, false);
            assert_eq!(tc.to_frame(fps), frame, "frame={}", frame);
        }
    }

    #[test]
    fn ndf_one_second_at_30fps() {
        let tc = Timecode::from_frame(30, Fps::FPS_30, false);
        assert_eq!((tc.hours, tc.minutes, tc.seconds, tc.frames), (0, 0, 1, 0));
        assert!(!tc.negative);
        assert!(!tc.drop_frame);
    }

    #[test]
    fn df_one_hour_at_2997() {
        // 1 hour at 29.97 DF = 107892 frames (= 29.97 * 3600).
        let tc = Timecode::from_frame(107892, Fps::NTSC_30, true);
        assert_eq!(
            (tc.hours, tc.minutes, tc.seconds, tc.frames),
            (1, 0, 0, 0),
            "tc={:?}",
            tc
        );
        assert!(tc.drop_frame);
    }

    #[test]
    fn df_round_trip_2997() {
        let fps = Fps::NTSC_30;
        for &frame in &[0, 1, 29, 30, 1798, 1799, 1800, 17982, 107892, 107893, 215784] {
            let tc = Timecode::from_frame(frame, fps, true);
            assert_eq!(tc.to_frame(fps), frame, "frame={} tc={:?}", frame, tc);
        }
    }

    #[test]
    fn df_round_trip_5994() {
        let fps = Fps::NTSC_60;
        for &frame in &[0, 1, 59, 60, 3596, 3600, 35964, 215784] {
            let tc = Timecode::from_frame(frame, fps, true);
            assert_eq!(tc.to_frame(fps), frame, "frame={} tc={:?}", frame, tc);
        }
    }

    #[test]
    fn df_falls_back_to_ndf_for_non_ntsc() {
        let tc = Timecode::from_frame(100, Fps::FPS_24, true);
        assert!(!tc.drop_frame, "DF should not apply at 24fps");
    }

    #[test]
    fn negative_frames_round_trip() {
        for &frame in &[-1, -24, -107892, -1_000_000] {
            let fps = if frame == -107892 {
                Fps::NTSC_30
            } else {
                Fps::FPS_24
            };
            let df = fps.is_drop_frame_eligible();
            let tc = Timecode::from_frame(frame, fps, df);
            assert_eq!(tc.to_frame(fps), frame, "frame={}", frame);
            assert!(tc.negative);
        }
    }

    #[test]
    fn format_frames_mode() {
        assert_eq!(format_time(1234, Fps::FPS_24, TimeDisplay::Frames), "1234");
        assert_eq!(format_time(-1, Fps::FPS_24, TimeDisplay::Frames), "-1");
    }

    #[test]
    fn format_seconds_mode() {
        assert_eq!(
            format_time(24, Fps::FPS_24, TimeDisplay::Seconds),
            "1.000"
        );
    }

    #[test]
    fn format_ndf_timecode() {
        let s = format_time(
            107892,
            Fps::FPS_30,
            TimeDisplay::Timecode { drop_frame: false },
        );
        // 107892 / 30 = 3596.4 → 59:56:12 at NDF 30fps.
        assert_eq!(s, "00:59:56:12");
    }

    #[test]
    fn format_df_timecode_uses_semicolon() {
        let s = format_time(
            107892,
            Fps::NTSC_30,
            TimeDisplay::Timecode { drop_frame: true },
        );
        assert_eq!(s, "01:00:00;00");
    }

    #[test]
    fn format_negative_timecode_has_minus() {
        let s = format_time(
            -30,
            Fps::FPS_30,
            TimeDisplay::Timecode { drop_frame: false },
        );
        assert_eq!(s, "-00:00:01:00");
    }

    #[test]
    fn parse_frames() {
        assert_eq!(
            parse_time("1234", Fps::FPS_24, TimeDisplay::Frames),
            Some(1234)
        );
        assert_eq!(
            parse_time("-1", Fps::FPS_24, TimeDisplay::Frames),
            Some(-1)
        );
        assert_eq!(parse_time("abc", Fps::FPS_24, TimeDisplay::Frames), None);
    }

    #[test]
    fn parse_seconds_with_unit_suffix() {
        assert_eq!(
            parse_time("1.0", Fps::FPS_24, TimeDisplay::Seconds),
            Some(24)
        );
        assert_eq!(
            parse_time("1s", Fps::FPS_24, TimeDisplay::Seconds),
            Some(24)
        );
        assert_eq!(
            parse_time("-2.0", Fps::FPS_24, TimeDisplay::Seconds),
            Some(-48)
        );
    }

    #[test]
    fn parse_timecode_short_forms() {
        let m = TimeDisplay::Timecode { drop_frame: false };
        assert_eq!(parse_time("00:00:01:00", Fps::FPS_24, m), Some(24));
        assert_eq!(parse_time("00:01:00", Fps::FPS_24, m), Some(24));
        assert_eq!(parse_time("01:00", Fps::FPS_24, m), Some(24));
    }

    #[test]
    fn parse_timecode_auto_detects_df_via_semicolon() {
        // Even with drop_frame=false in the mode, ';' in input switches to DF math
        // because DF is what the user typed.
        let m = TimeDisplay::Timecode { drop_frame: false };
        assert_eq!(parse_time("01:00:00;00", Fps::NTSC_30, m), Some(107892));
    }

    #[test]
    fn parse_timecode_rejects_invalid_frame_field() {
        let m = TimeDisplay::Timecode { drop_frame: false };
        // Frame 25 is invalid at 24fps (range 0..=23).
        assert_eq!(parse_time("00:00:00:25", Fps::FPS_24, m), None);
    }

    #[test]
    fn format_parse_round_trip_ntsc_df() {
        let fps = Fps::NTSC_30;
        let m = TimeDisplay::Timecode { drop_frame: true };
        for &frame in &[0, 1, 29, 30, 1799, 107892, -3600] {
            let s = format_time(frame, fps, m);
            assert_eq!(parse_time(&s, fps, m), Some(frame), "frame={} str={}", frame, s);
        }
    }
}
