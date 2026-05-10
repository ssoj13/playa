//! Exact rational frame rate.

use serde::{Deserialize, Serialize};

/// Frame rate as `num / den`. Rational form preserves NTSC fps (e.g. 24000/1001)
/// without f32 drift over long durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

impl Fps {
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    pub const FPS_48: Self = Self { num: 48, den: 1 };
    pub const FPS_50: Self = Self { num: 50, den: 1 };
    pub const FPS_60: Self = Self { num: 60, den: 1 };

    /// 23.976 (24000/1001) — NTSC film.
    pub const NTSC_24: Self = Self {
        num: 24000,
        den: 1001,
    };
    /// 29.97 (30000/1001) — NTSC video.
    pub const NTSC_30: Self = Self {
        num: 30000,
        den: 1001,
    };
    /// 59.94 (60000/1001) — NTSC HFR.
    pub const NTSC_60: Self = Self {
        num: 60000,
        den: 1001,
    };

    /// Construct an arbitrary rate. Panics if either component is zero.
    pub const fn new(num: u32, den: u32) -> Self {
        assert!(num > 0 && den > 0, "Fps components must be > 0");
        Self { num, den }
    }

    /// Best-effort recovery of an exact rate from a possibly-rounded f32. Matches
    /// the existing tolerance-based recognition in
    /// `playa-ui::dialogs::encode::encode::fps_to_rational`.
    pub fn from_f32_lossy(v: f32) -> Self {
        const TOLERANCE: f32 = 0.01;
        const KNOWN: &[Fps] = &[
            Fps::NTSC_24,
            Fps::NTSC_30,
            Fps::NTSC_60,
            Fps::FPS_24,
            Fps::FPS_25,
            Fps::FPS_30,
            Fps::FPS_48,
            Fps::FPS_50,
            Fps::FPS_60,
        ];
        for k in KNOWN {
            if (k.as_f32() - v).abs() < TOLERANCE {
                return *k;
            }
        }
        // Fallback: integer scale to preserve up to 3 decimal digits.
        let v_clamped = v.max(0.001);
        Self {
            num: (v_clamped * 1000.0).round().max(1.0) as u32,
            den: 1000,
        }
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    #[inline]
    pub fn as_f32(self) -> f32 {
        self.as_f64() as f32
    }

    /// Duration of one frame in seconds.
    #[inline]
    pub fn frame_duration_secs(self) -> f64 {
        self.den as f64 / self.num as f64
    }

    /// True iff this rate is one of the SMPTE drop-frame eligible NTSC rates.
    #[inline]
    pub fn is_drop_frame_eligible(self) -> bool {
        matches!(self, Self::NTSC_30 | Self::NTSC_60)
    }

    /// Nominal integer fps used by SMPTE timecode (NDF and DF both label hours/
    /// minutes/seconds in terms of the rounded rate). For 23.976 this is 24, for
    /// 29.97 it is 30, for 59.94 it is 60.
    #[inline]
    pub fn nominal(self) -> u32 {
        ((self.num as f64) / (self.den as f64)).round() as u32
    }
}

impl Default for Fps {
    fn default() -> Self {
        Self::FPS_24
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntsc_constants_match_published_rates() {
        assert!((Fps::NTSC_24.as_f64() - 23.976023976).abs() < 1e-6);
        assert!((Fps::NTSC_30.as_f64() - 29.97002997).abs() < 1e-6);
        assert!((Fps::NTSC_60.as_f64() - 59.94005994).abs() < 1e-6);
    }

    #[test]
    fn from_f32_lossy_recovers_ntsc() {
        assert_eq!(Fps::from_f32_lossy(23.976), Fps::NTSC_24);
        assert_eq!(Fps::from_f32_lossy(29.97), Fps::NTSC_30);
        assert_eq!(Fps::from_f32_lossy(59.94), Fps::NTSC_60);
    }

    #[test]
    fn from_f32_lossy_recovers_integer_rates() {
        assert_eq!(Fps::from_f32_lossy(24.0), Fps::FPS_24);
        assert_eq!(Fps::from_f32_lossy(30.0), Fps::FPS_30);
        assert_eq!(Fps::from_f32_lossy(60.0), Fps::FPS_60);
    }

    #[test]
    fn from_f32_lossy_fallback_for_unusual_rate() {
        let f = Fps::from_f32_lossy(12.345);
        assert!((f.as_f32() - 12.345).abs() < 0.001);
    }

    #[test]
    fn frame_duration_inverse_of_rate() {
        assert!((Fps::NTSC_24.frame_duration_secs() - 1001.0 / 24000.0).abs() < 1e-12);
    }

    #[test]
    fn nominal_rounds_ntsc_correctly() {
        assert_eq!(Fps::NTSC_24.nominal(), 24);
        assert_eq!(Fps::NTSC_30.nominal(), 30);
        assert_eq!(Fps::NTSC_60.nominal(), 60);
        assert_eq!(Fps::FPS_25.nominal(), 25);
    }

    #[test]
    fn drop_frame_eligibility() {
        assert!(Fps::NTSC_30.is_drop_frame_eligible());
        assert!(Fps::NTSC_60.is_drop_frame_eligible());
        assert!(!Fps::NTSC_24.is_drop_frame_eligible());
        assert!(!Fps::FPS_30.is_drop_frame_eligible());
    }
}
