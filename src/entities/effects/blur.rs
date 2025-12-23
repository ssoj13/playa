//! Gaussian Blur effect implementation.
//!
//! Applies separable Gaussian blur to a frame. The algorithm uses two passes
//! (horizontal + vertical) for O(n*r) complexity instead of O(n*r^2).
//!
//! # Algorithm
//!
//! 1. Build 1D Gaussian kernel based on radius
//! 2. Horizontal pass: convolve each row with kernel
//! 3. Vertical pass: convolve each column with kernel
//!
//! # Usage
//!
//! ```ignore
//! let blurred = blur::apply(&frame, &attrs)?;
//! // attrs should contain "radius" float (default 5.0)
//! ```

use half::f16 as F16;

use crate::entities::attrs::Attrs;
use crate::entities::frame::{Frame, PixelBuffer, PixelFormat};

/// Apply Gaussian blur effect to a frame.
///
/// # Parameters
/// - `frame`: Source frame to blur
/// - `attrs`: Effect attributes containing "radius" (blur radius in pixels)
///
/// # Returns
/// New blurred Frame, or None if processing fails
pub fn apply(frame: &Frame, attrs: &Attrs) -> Option<Frame> {
    let radius = attrs.get_float("radius").unwrap_or(5.0);

    // No blur needed for zero or negative radius
    if radius <= 0.0 {
        return Some(frame.clone());
    }

    let (width, height) = frame.resolution();
    let buffer = frame.buffer();

    // Convert to f32 for processing (unified pipeline)
    let src_f32 = to_f32_buffer(&buffer, width, height);

    // Build Gaussian kernel
    let kernel = gaussian_kernel(radius);

    // Separable blur: horizontal pass
    let temp = convolve_horizontal(&src_f32, width, height, &kernel);

    // Vertical pass on temp result
    let result = convolve_vertical(&temp, width, height, &kernel);

    // Convert back to original format
    let out_buffer = from_f32_buffer(&result, frame.pixel_format(), width, height);

    Some(Frame::from_buffer(out_buffer, frame.pixel_format(), width, height))
}

/// Convert any PixelBuffer to f32 RGBA for processing.
fn to_f32_buffer(buffer: &PixelBuffer, width: usize, height: usize) -> Vec<f32> {
    let size = width * height * 4;
    let mut result = Vec::with_capacity(size);

    match buffer {
        PixelBuffer::U8(data) => {
            for &v in data.iter() {
                result.push(v as f32 / 255.0);
            }
        }
        PixelBuffer::F16(data) => {
            for &v in data.iter() {
                result.push(v.to_f32());
            }
        }
        PixelBuffer::F32(data) => {
            result.extend_from_slice(data);
        }
    }

    result
}

/// Convert f32 buffer back to original pixel format.
fn from_f32_buffer(
    data: &[f32],
    format: PixelFormat,
    width: usize,
    height: usize,
) -> PixelBuffer {
    match format {
        PixelFormat::Rgba8 => {
            let mut result = Vec::with_capacity(width * height * 4);
            for &v in data.iter() {
                result.push((v.clamp(0.0, 1.0) * 255.0) as u8);
            }
            PixelBuffer::U8(result)
        }
        PixelFormat::RgbaF16 => {
            let mut result = Vec::with_capacity(width * height * 4);
            for &v in data.iter() {
                result.push(F16::from_f32(v));
            }
            PixelBuffer::F16(result)
        }
        PixelFormat::RgbaF32 => {
            PixelBuffer::F32(data.to_vec())
        }
    }
}

/// Build 1D Gaussian kernel for given radius.
///
/// Kernel size = 2*ceil(radius*2) + 1 (captures ~95% of Gaussian)
/// Values are normalized to sum to 1.0.
fn gaussian_kernel(radius: f32) -> Vec<f32> {
    // Kernel half-size: capture 2 sigma (~95% of distribution)
    let half_size = (radius * 2.0).ceil() as i32;
    let size = (half_size * 2 + 1) as usize;

    let sigma = radius / 2.0; // sigma = radius/2 gives nice falloff
    let sigma2 = sigma * sigma;
    let norm = 1.0 / (2.0 * std::f32::consts::PI * sigma2).sqrt();

    let mut kernel = Vec::with_capacity(size);
    let mut sum = 0.0;

    for i in 0..size as i32 {
        let x = (i - half_size) as f32;
        let weight = norm * (-x * x / (2.0 * sigma2)).exp();
        kernel.push(weight);
        sum += weight;
    }

    // Normalize kernel to sum to 1.0
    for w in &mut kernel {
        *w /= sum;
    }

    kernel
}

/// Horizontal convolution pass.
///
/// For each row, convolve with kernel. Edge pixels use clamped sampling.
fn convolve_horizontal(src: &[f32], width: usize, height: usize, kernel: &[f32]) -> Vec<f32> {
    let mut dst = vec![0.0f32; src.len()];
    let half = (kernel.len() / 2) as i32;

    for y in 0..height {
        for x in 0..width {
            let mut r = 0.0;
            let mut g = 0.0;
            let mut b = 0.0;
            let mut a = 0.0;

            for (ki, &weight) in kernel.iter().enumerate() {
                // Sample x coordinate with clamping
                let sx = (x as i32 + ki as i32 - half).clamp(0, width as i32 - 1) as usize;
                let idx = (y * width + sx) * 4;

                r += src[idx] * weight;
                g += src[idx + 1] * weight;
                b += src[idx + 2] * weight;
                a += src[idx + 3] * weight;
            }

            let dst_idx = (y * width + x) * 4;
            dst[dst_idx] = r;
            dst[dst_idx + 1] = g;
            dst[dst_idx + 2] = b;
            dst[dst_idx + 3] = a;
        }
    }

    dst
}

/// Vertical convolution pass.
///
/// For each column, convolve with kernel. Edge pixels use clamped sampling.
fn convolve_vertical(src: &[f32], width: usize, height: usize, kernel: &[f32]) -> Vec<f32> {
    let mut dst = vec![0.0f32; src.len()];
    let half = (kernel.len() / 2) as i32;

    for y in 0..height {
        for x in 0..width {
            let mut r = 0.0;
            let mut g = 0.0;
            let mut b = 0.0;
            let mut a = 0.0;

            for (ki, &weight) in kernel.iter().enumerate() {
                // Sample y coordinate with clamping
                let sy = (y as i32 + ki as i32 - half).clamp(0, height as i32 - 1) as usize;
                let idx = (sy * width + x) * 4;

                r += src[idx] * weight;
                g += src[idx + 1] * weight;
                b += src[idx + 2] * weight;
                a += src[idx + 3] * weight;
            }

            let dst_idx = (y * width + x) * 4;
            dst[dst_idx] = r;
            dst[dst_idx + 1] = g;
            dst[dst_idx + 2] = b;
            dst[dst_idx + 3] = a;
        }
    }

    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaussian_kernel() {
        let kernel = gaussian_kernel(5.0);

        // Kernel should be odd-sized
        assert!(kernel.len() % 2 == 1);

        // Kernel should sum to ~1.0
        let sum: f32 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 0.001);

        // Center should be largest
        let center = kernel.len() / 2;
        assert!(kernel[center] > kernel[0]);
        assert!(kernel[center] > kernel[kernel.len() - 1]);
    }

    #[test]
    fn test_zero_radius_noop() {
        // Zero radius should return cloned frame
        let frame = Frame::placeholder(10, 10);
        let mut attrs = Attrs::new();
        attrs.set("radius", crate::entities::attrs::AttrValue::Float(0.0));

        let result = apply(&frame, &attrs);
        assert!(result.is_some());
    }
}
