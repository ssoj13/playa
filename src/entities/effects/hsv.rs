//! HSV (Hue, Saturation, Value) adjustment effect.
//!
//! Converts RGB to HSV color space, applies adjustments, converts back.
//! Useful for color grading and correction.
//!
//! # Parameters
//!
//! - `hue_shift`: -180 to 180 degrees rotation on color wheel
//! - `saturation`: 0.0 (grayscale) to 2.0 (oversaturated), 1.0 = no change
//! - `value`: 0.0 (black) to 2.0 (overbright), 1.0 = no change
//!
//! # Algorithm
//!
//! 1. Convert each pixel RGB -> HSV
//! 2. H += hue_shift (wrap around 0-360)
//! 3. S *= saturation (clamp 0-1)
//! 4. V *= value (no clamp for HDR)
//! 5. Convert HSV -> RGB

use half::f16 as F16;

use crate::entities::attrs::Attrs;
use crate::entities::frame::{Frame, PixelBuffer};

/// Apply HSV adjustment to a frame.
///
/// # Parameters
/// - `frame`: Source frame to adjust
/// - `attrs`: Effect attributes containing "hue_shift", "saturation", "value"
///
/// # Returns
/// New adjusted Frame, or None if processing fails
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let hue_shift = attrs.get_float("hue_shift").unwrap_or(0.0);
    let saturation = attrs.get_float("saturation").unwrap_or(1.0);
    let value = attrs.get_float("value").unwrap_or(1.0);

    // No adjustment needed
    if hue_shift.abs() < 0.01 && (saturation - 1.0).abs() < 0.001 && (value - 1.0).abs() < 0.001 {
        return Some(frame.clone());
    }

    let (width, height) = frame.resolution();
    let buffer = frame.buffer();

    let out_buffer = match buffer.as_ref() {
        PixelBuffer::U8(data) => {
            let mut result = Vec::with_capacity(data.len());

            for chunk in data.chunks_exact(4) {
                let r = chunk[0] as f32 / 255.0;
                let g = chunk[1] as f32 / 255.0;
                let b = chunk[2] as f32 / 255.0;
                let a = chunk[3];

                let (h, s, v) = rgb_to_hsv(r, g, b);

                // Apply adjustments
                let h_new = (h + hue_shift).rem_euclid(360.0);
                let s_new = (s * saturation).clamp(0.0, 1.0);
                let v_new = (v * value).clamp(0.0, 1.0); // Clamp for LDR

                let (r_out, g_out, b_out) = hsv_to_rgb(h_new, s_new, v_new);

                result.push((r_out * 255.0) as u8);
                result.push((g_out * 255.0) as u8);
                result.push((b_out * 255.0) as u8);
                result.push(a);
            }

            PixelBuffer::U8(result)
        }

        PixelBuffer::F16(data) => {
            let mut result = Vec::with_capacity(data.len());

            for chunk in data.chunks_exact(4) {
                let r = chunk[0].to_f32();
                let g = chunk[1].to_f32();
                let b = chunk[2].to_f32();
                let a = chunk[3];

                let (h, s, v) = rgb_to_hsv(r, g, b);

                // Apply adjustments (no value clamp for HDR)
                let h_new = (h + hue_shift).rem_euclid(360.0);
                let s_new = (s * saturation).clamp(0.0, 1.0);
                let v_new = v * value;

                let (r_out, g_out, b_out) = hsv_to_rgb(h_new, s_new, v_new);

                result.push(F16::from_f32(r_out));
                result.push(F16::from_f32(g_out));
                result.push(F16::from_f32(b_out));
                result.push(a);
            }

            PixelBuffer::F16(result)
        }

        PixelBuffer::F32(data) => {
            let mut result = Vec::with_capacity(data.len());

            for chunk in data.chunks_exact(4) {
                let r = chunk[0];
                let g = chunk[1];
                let b = chunk[2];
                let a = chunk[3];

                let (h, s, v) = rgb_to_hsv(r, g, b);

                // Apply adjustments (no value clamp for HDR)
                let h_new = (h + hue_shift).rem_euclid(360.0);
                let s_new = (s * saturation).clamp(0.0, 1.0);
                let v_new = v * value;

                let (r_out, g_out, b_out) = hsv_to_rgb(h_new, s_new, v_new);

                result.push(r_out);
                result.push(g_out);
                result.push(b_out);
                result.push(a);
            }

            PixelBuffer::F32(result)
        }
    };

    Some(Frame::from_buffer(out_buffer, frame.pixel_format(), width, height))
}

/// Convert RGB to HSV.
///
/// - R, G, B: 0.0 to 1.0 (or higher for HDR)
/// - H: 0 to 360 degrees
/// - S: 0 to 1
/// - V: 0 to max(R,G,B)
fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    // Value = max component
    let v = max;

    // Saturation
    let s = if max > 0.0 { delta / max } else { 0.0 };

    // Hue
    let h = if delta.abs() < 0.0001 {
        0.0 // Achromatic (gray)
    } else if (max - r).abs() < 0.0001 {
        // Red is max
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() < 0.0001 {
        // Green is max
        60.0 * ((b - r) / delta + 2.0)
    } else {
        // Blue is max
        60.0 * ((r - g) / delta + 4.0)
    };

    // Normalize hue to 0-360
    let h = if h < 0.0 { h + 360.0 } else { h };

    (h, s, v)
}

/// Convert HSV to RGB.
///
/// - H: 0 to 360 degrees
/// - S: 0 to 1
/// - V: any positive value (supports HDR)
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    if s <= 0.0 {
        // Achromatic (gray)
        return (v, v, v);
    }

    let h = h % 360.0;
    let h = if h < 0.0 { h + 360.0 } else { h };

    let c = v * s; // Chroma
    let h_prime = h / 60.0;
    let x = c * (1.0 - ((h_prime % 2.0) - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = match h_prime as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x), // 5 or edge cases
    };

    (r1 + m, g1 + m, b1 + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_hsv_roundtrip() {
        // Red
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        assert!((h - 0.0).abs() < 1.0); // Hue ~0
        assert!((s - 1.0).abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        let (r, g, b) = hsv_to_rgb(h, s, v);
        assert!((r - 1.0).abs() < 0.01);
        assert!(g.abs() < 0.01);
        assert!(b.abs() < 0.01);
    }

    #[test]
    fn test_hue_shift_red_to_green() {
        // Red shifted by 120 degrees should be green-ish
        let (h, s, v) = rgb_to_hsv(1.0, 0.0, 0.0);
        let (r, g, b) = hsv_to_rgb(h + 120.0, s, v);

        assert!(g > r); // Green should be dominant now
        assert!(g > b);
    }

    #[test]
    fn test_gray_unchanged() {
        // Gray has no hue, should be unaffected by hue shift
        let (h, s, v) = rgb_to_hsv(0.5, 0.5, 0.5);
        assert!(s < 0.01); // Saturation ~0 for gray

        let (r, g, b) = hsv_to_rgb(h + 180.0, s, v); // Shift hue by 180
        assert!((r - 0.5).abs() < 0.01); // Should still be gray
        assert!((g - 0.5).abs() < 0.01);
        assert!((b - 0.5).abs() < 0.01);
    }
}
