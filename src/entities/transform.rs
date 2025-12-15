//! 2D affine transforms for layer compositing.
//!
//! Uses glam::Affine2 for matrix math. Transform order (AE-style):
//! M = T(position) * T(pivot) * R(rotation_z) * S(scale) * T(-pivot)

use glam::{Affine2, Vec2};
use half::f16 as F16;
use rayon::prelude::*;

use super::frame::{Frame, PixelBuffer, PixelFormat};

/// Check if transform is identity (no-op).
/// 
/// Returns true if position=0, rotation_z=0, scale=1 — no transform needed.
#[inline]
pub fn is_identity(position: [f32; 3], rotation_z: f32, scale: [f32; 3]) -> bool {
    position[0] == 0.0 && position[1] == 0.0
        && rotation_z == 0.0
        && scale[0] == 1.0 && scale[1] == 1.0
}

/// Build inverse transform matrix for sampling.
/// 
/// Forward transform order (AE-style):
/// ```text
/// M = T(position) * T(pivot) * R(rotation_z) * S(scale) * T(-pivot)
/// ```
/// 
/// Returns inverse matrix for reverse-mapping: dst pixel → src coord.
fn build_inverse_transform(
    position: [f32; 3],
    rotation_z: f32,
    scale: [f32; 3],
    pivot: [f32; 3],
    src_center: Vec2,
) -> Affine2 {
    // Pivot is relative to source center (like AE anchor point)
    let pivot_pt = src_center + Vec2::new(pivot[0], pivot[1]);
    
    // Build forward transform (order matters!)
    let transform = Affine2::from_translation(Vec2::new(position[0], position[1]))
        * Affine2::from_translation(pivot_pt)
        * Affine2::from_angle(rotation_z)
        * Affine2::from_scale(Vec2::new(scale[0], scale[1]))
        * Affine2::from_translation(-pivot_pt);
    
    transform.inverse()
}

/// Sample F32 buffer with bilinear interpolation.
/// 
/// Returns `[R, G, B, A]` in 0-1 range, or `[0,0,0,0]` if outside bounds.
#[inline]
fn sample_f32(buffer: &[f32], width: usize, height: usize, x: f32, y: f32) -> [f32; 4] {
    // Bounds check - return transparent if outside
    if x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    
    // Sample 4 corners
    let idx00 = (y0 * width + x0) * 4;
    let idx10 = (y0 * width + x1) * 4;
    let idx01 = (y1 * width + x0) * 4;
    let idx11 = (y1 * width + x1) * 4;
    
    let mut result = [0.0f32; 4];
    for c in 0..4 {
        let c00 = buffer[idx00 + c];
        let c10 = buffer[idx10 + c];
        let c01 = buffer[idx01 + c];
        let c11 = buffer[idx11 + c];
        
        // Bilinear interpolation
        let top = c00 * (1.0 - fx) + c10 * fx;
        let bottom = c01 * (1.0 - fx) + c11 * fx;
        result[c] = top * (1.0 - fy) + bottom * fy;
    }
    
    result
}

/// Sample F16 buffer with bilinear interpolation.
/// 
/// Returns `[R, G, B, A]` in 0-1 range, or `[0,0,0,0]` if outside bounds.
#[inline]
fn sample_f16(buffer: &[F16], width: usize, height: usize, x: f32, y: f32) -> [f32; 4] {
    if x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    
    let idx00 = (y0 * width + x0) * 4;
    let idx10 = (y0 * width + x1) * 4;
    let idx01 = (y1 * width + x0) * 4;
    let idx11 = (y1 * width + x1) * 4;
    
    let mut result = [0.0f32; 4];
    for c in 0..4 {
        let c00 = buffer[idx00 + c].to_f32();
        let c10 = buffer[idx10 + c].to_f32();
        let c01 = buffer[idx01 + c].to_f32();
        let c11 = buffer[idx11 + c].to_f32();
        
        let top = c00 * (1.0 - fx) + c10 * fx;
        let bottom = c01 * (1.0 - fx) + c11 * fx;
        result[c] = top * (1.0 - fy) + bottom * fy;
    }
    
    result
}

/// Sample U8 buffer with bilinear interpolation.
/// 
/// Returns `[R, G, B, A]` in 0-1 range, or `[0,0,0,0]` if outside bounds.
#[inline]
fn sample_u8(buffer: &[u8], width: usize, height: usize, x: f32, y: f32) -> [f32; 4] {
    if x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    
    let idx00 = (y0 * width + x0) * 4;
    let idx10 = (y0 * width + x1) * 4;
    let idx01 = (y1 * width + x0) * 4;
    let idx11 = (y1 * width + x1) * 4;
    
    let mut result = [0.0f32; 4];
    for c in 0..4 {
        let c00 = buffer[idx00 + c] as f32 / 255.0;
        let c10 = buffer[idx10 + c] as f32 / 255.0;
        let c01 = buffer[idx01 + c] as f32 / 255.0;
        let c11 = buffer[idx11 + c] as f32 / 255.0;
        
        let top = c00 * (1.0 - fx) + c10 * fx;
        let bottom = c01 * (1.0 - fx) + c11 * fx;
        result[c] = top * (1.0 - fy) + bottom * fy;
    }
    
    result
}

/// Transform frame with 2D affine matrix.
/// 
/// Applies position, rotation, scale around pivot point using parallel
/// pixel processing (rayon). Output format matches input format.
/// 
/// # Arguments
/// - `src` — Source frame (U8/F16/F32)
/// - `canvas` — Output dimensions `(width, height)`
/// - `position` — `[x, y, z]` offset in pixels (z ignored)
/// - `rotation_z` — Z-axis rotation in radians
/// - `scale` — `[sx, sy, sz]` scale factors (sz ignored)
/// - `pivot` — `[px, py, pz]` anchor relative to source center (pz ignored)
/// 
/// # Example
/// ```ignore
/// let transformed = transform_frame(
///     &frame,
///     (1920, 1080),
///     [100.0, 50.0, 0.0],  // move right 100, down 50
///     0.785,                // 45° rotation
///     [0.5, 0.5, 1.0],      // 50% scale
///     [0.0, 0.0, 0.0],      // pivot at center
/// );
/// ```
pub fn transform_frame(
    src: &Frame,
    canvas: (usize, usize),
    position: [f32; 3],
    rotation_z: f32,
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Frame {
    let src_w = src.width();
    let src_h = src.height();
    let (dst_w, dst_h) = canvas;
    
    // Source center for pivot calculation
    let src_center = Vec2::new(src_w as f32 / 2.0, src_h as f32 / 2.0);
    
    // Inverse transform: for each dst pixel, find src coord
    let inv = build_inverse_transform(position, rotation_z, scale, pivot, src_center);
    
    // Get source buffer
    let src_buffer = src.buffer();
    let src_format = src.pixel_format();
    
    // Transform based on pixel format (output same format as input)
    match (src_buffer.as_ref(), src_format) {
        (PixelBuffer::F32(buf), PixelFormat::RgbaF32) => {
            let mut dst_buf = vec![0.0f32; dst_w * dst_h * 4];
            
            // Parallel row processing with rayon
            dst_buf
                .par_chunks_mut(dst_w * 4)
                .enumerate()
                .for_each(|(y, row)| {
                    for x in 0..dst_w {
                        // Transform dst coord to src coord
                        let dst_pt = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let src_pt = inv.transform_point2(dst_pt);
                        
                        let color = sample_f32(buf, src_w, src_h, src_pt.x, src_pt.y);
                        let idx = x * 4;
                        row[idx..idx + 4].copy_from_slice(&color);
                    }
                });
            
            Frame::from_f32_buffer(dst_buf, dst_w, dst_h)
        }
        
        (PixelBuffer::F16(buf), PixelFormat::RgbaF16) => {
            let mut dst_buf = vec![F16::ZERO; dst_w * dst_h * 4];
            
            dst_buf
                .par_chunks_mut(dst_w * 4)
                .enumerate()
                .for_each(|(y, row)| {
                    for x in 0..dst_w {
                        let dst_pt = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let src_pt = inv.transform_point2(dst_pt);
                        
                        let color = sample_f16(buf, src_w, src_h, src_pt.x, src_pt.y);
                        let idx = x * 4;
                        row[idx] = F16::from_f32(color[0]);
                        row[idx + 1] = F16::from_f32(color[1]);
                        row[idx + 2] = F16::from_f32(color[2]);
                        row[idx + 3] = F16::from_f32(color[3]);
                    }
                });
            
            Frame::from_f16_buffer(dst_buf, dst_w, dst_h)
        }
        
        (PixelBuffer::U8(buf), PixelFormat::Rgba8) => {
            let mut dst_buf = vec![0u8; dst_w * dst_h * 4];
            
            dst_buf
                .par_chunks_mut(dst_w * 4)
                .enumerate()
                .for_each(|(y, row)| {
                    for x in 0..dst_w {
                        let dst_pt = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let src_pt = inv.transform_point2(dst_pt);
                        
                        let color = sample_u8(buf, src_w, src_h, src_pt.x, src_pt.y);
                        let idx = x * 4;
                        row[idx] = (color[0] * 255.0).clamp(0.0, 255.0) as u8;
                        row[idx + 1] = (color[1] * 255.0).clamp(0.0, 255.0) as u8;
                        row[idx + 2] = (color[2] * 255.0).clamp(0.0, 255.0) as u8;
                        row[idx + 3] = (color[3] * 255.0).clamp(0.0, 255.0) as u8;
                    }
                });
            
            Frame::from_u8_buffer(dst_buf, dst_w, dst_h)
        }
        
        // Fallback: copy without transform if format mismatch
        _ => {
            log::warn!("transform_frame: unsupported format {:?}, returning copy", src_format);
            src.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_identity_check() {
        assert!(is_identity([0.0, 0.0, 0.0], 0.0, [1.0, 1.0, 1.0]));
        assert!(!is_identity([10.0, 0.0, 0.0], 0.0, [1.0, 1.0, 1.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], 0.1, [1.0, 1.0, 1.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], 0.0, [2.0, 1.0, 1.0]));
    }
    
    #[test]
    fn test_transform_identity() {
        // Create 4x4 red frame
        let buf = vec![1.0f32, 0.0, 0.0, 1.0].repeat(16);
        let frame = Frame::from_f32_buffer(buf, 4, 4);
        
        // Apply identity transform
        let result = transform_frame(
            &frame,
            (4, 4),
            [0.0, 0.0, 0.0],
            0.0,
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
        );
        
        assert_eq!(result.width(), 4);
        assert_eq!(result.height(), 4);
    }
}
