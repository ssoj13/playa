//! Explicit rounding mode for frame conversions.
//!
//! Replaces ad-hoc `.round()` / `.ceil()` / `as i32` truncation across the previous
//! codebase, where the same expression rounded three different ways depending on
//! which call site the caller landed on (B1 in the audit).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Round {
    /// Round toward negative infinity.
    Floor,
    /// Round to nearest, ties away from zero (Rust `f64::round` semantics).
    Round,
    /// Round toward positive infinity.
    Ceil,
    /// Round toward zero.
    Trunc,
}

impl Round {
    #[inline]
    pub fn apply_f64(self, v: f64) -> f64 {
        match self {
            Self::Floor => v.floor(),
            Self::Round => v.round(),
            Self::Ceil => v.ceil(),
            Self::Trunc => v.trunc(),
        }
    }

    /// Round and saturate-cast to `i32`.
    #[inline]
    pub fn to_i32(self, v: f64) -> i32 {
        let v = self.apply_f64(v);
        if v >= i32::MAX as f64 {
            i32::MAX
        } else if v <= i32::MIN as f64 {
            i32::MIN
        } else {
            v as i32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mode_distinct_for_3_5() {
        let v = 3.5_f64;
        assert_eq!(Round::Floor.apply_f64(v), 3.0);
        assert_eq!(Round::Round.apply_f64(v), 4.0);
        assert_eq!(Round::Ceil.apply_f64(v), 4.0);
        assert_eq!(Round::Trunc.apply_f64(v), 3.0);
    }

    #[test]
    fn negative_values() {
        assert_eq!(Round::Floor.apply_f64(-3.5), -4.0);
        assert_eq!(Round::Round.apply_f64(-3.5), -4.0);
        assert_eq!(Round::Ceil.apply_f64(-3.5), -3.0);
        assert_eq!(Round::Trunc.apply_f64(-3.5), -3.0);
    }

    #[test]
    fn to_i32_saturates() {
        assert_eq!(Round::Round.to_i32(1e30), i32::MAX);
        assert_eq!(Round::Round.to_i32(-1e30), i32::MIN);
    }

    #[test]
    fn to_i32_passes_through_normal_values() {
        assert_eq!(Round::Round.to_i32(7.4), 7);
        assert_eq!(Round::Round.to_i32(7.5), 8);
        assert_eq!(Round::Round.to_i32(-7.5), -8);
    }
}
