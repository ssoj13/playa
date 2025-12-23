//! Brightness/Contrast effect implementation.
//!
//! Adjusts brightness and contrast of a frame using standard formula:
//! `output = (input - 0.5) * contrast_factor + 0.5 + brightness`
//!
//! # Parameters
//!
//! - `brightness`: -1.0 (black) to 1.0 (white), 0.0 = no change
//! - `contrast`: -1.0 (flat gray) to 1.0 (high contrast), 0.0 = no change
//!
//! # Algorithm
//!
//! 1. Subtract 0.5 to center around zero
//! 2. Multiply by contrast factor (1.0 + contrast)
//! 3. Add 0.5 back to restore center
//! 4. Add brightness offset

use half::f16 as F16;

use crate::entities::attrs::Attrs;
use crate::entities::frame::{Frame, PixelBuffer};

/// Apply brightness/contrast adjustment to a frame.
///
/// # Parameters
/// - `frame`: Source frame to adjust
/// - `attrs`: Effect attributes containing "brightness" and "contrast" floats
///
/// # Returns
/// New adjusted Frame, or None if processing fails
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let brightness = attrs.get_float("brightness").unwrap_or(0.0);
    let contrast = attrs.get_float("contrast").unwrap_or(0.0);

    // No adjustment needed
    if brightness.abs() < 0.0001 && contrast.abs() < 0.0001 {
        return Some(frame.clone());
    }

    // Contrast factor: 1.0 = no change, 0.0 = flat, 2.0 = double contrast
    let cf = 1.0 + contrast;

    let (width, height) = frame.resolution();
    let buffer = frame.buffer();

    let out_buffer = match buffer.as_ref() {
        PixelBuffer::U8(data) => {
            let mut result = Vec::with_capacity(data.len());

            for chunk in data.chunks_exact(4) {
                // Convert to 0-1 range for processing
                let r = chunk[0] as f32 / 255.0;
                let g = chunk[1] as f32 / 255.0;
                let b = chunk[2] as f32 / 255.0;
                let a = chunk[3]; // Alpha unchanged

                // Apply: (v - 0.5) * cf + 0.5 + brightness
                let r_out = ((r - 0.5) * cf + 0.5 + brightness).clamp(0.0, 1.0);
                let g_out = ((g - 0.5) * cf + 0.5 + brightness).clamp(0.0, 1.0);
                let b_out = ((b - 0.5) * cf + 0.5 + brightness).clamp(0.0, 1.0);

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
                let a = chunk[3]; // Alpha unchanged

                // Apply adjustment (no clamping for HDR - allow out-of-range)
                let r_out = (r - 0.5) * cf + 0.5 + brightness;
                let g_out = (g - 0.5) * cf + 0.5 + brightness;
                let b_out = (b - 0.5) * cf + 0.5 + brightness;

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
                let a = chunk[3]; // Alpha unchanged

                // Apply adjustment (no clamping for HDR)
                let r_out = (r - 0.5) * cf + 0.5 + brightness;
                let g_out = (g - 0.5) * cf + 0.5 + brightness;
                let b_out = (b - 0.5) * cf + 0.5 + brightness;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::attrs::AttrValue;
    use crate::entities::frame::PixelDepth;

    #[test]
    fn test_no_change() {
        let frame = Frame::new(10, 10, PixelDepth::U8);
        let mut attrs = Attrs::new();
        attrs.set("brightness", AttrValue::Float(0.0));
        attrs.set("contrast", AttrValue::Float(0.0));

        let result = apply(&frame, &attrs);
        assert!(result.is_some());
    }

    #[test]
    fn test_brightness_increase() {
        // Create frame with mid-gray pixel
        let mut data = vec![0u8; 4 * 4]; // 2x2 frame
        data[0] = 128; // R
        data[1] = 128; // G
        data[2] = 128; // B
        data[3] = 255; // A

        let frame = Frame::from_u8_buffer_with_status(
            data,
            2,
            2,
            crate::entities::frame::FrameStatus::Loaded,
        );

        let mut attrs = Attrs::new();
        attrs.set("brightness", AttrValue::Float(0.5)); // +50% brightness
        attrs.set("contrast", AttrValue::Float(0.0));

        let result = apply(&frame, &attrs).unwrap();
        let buffer = result.buffer();

        if let PixelBuffer::U8(data) = buffer.as_ref() {
            // Mid-gray + 0.5 brightness should be ~white
            assert!(data[0] > 200); // R should be brighter
        }
    }
}
