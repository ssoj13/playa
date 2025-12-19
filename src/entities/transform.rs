//! 2D affine transforms for layer compositing.
//!
//! Uses glam::Affine2 for matrix math in Y-up space.
//! Forward transform (layer -> comp):
//! comp = position + R * S * (object - pivot)

use glam::{Affine2, Vec2};
use half::f16 as F16;
use rayon::prelude::*;

use super::frame::{Frame, PixelBuffer, PixelFormat};
use super::space;

/// Check if transform is identity (no-op).
/// 
/// Returns true if position==pivot, rotation_z=0, scale=1 — no transform needed.
#[inline]
pub fn is_identity(position: [f32; 3], rotation_z: f32, scale: [f32; 3], pivot: [f32; 3]) -> bool {
    position[0] == pivot[0]
        && position[1] == pivot[1]
        && rotation_z == 0.0
        && scale[0] == 1.0
        && scale[1] == 1.0
}

/// Build inverse transform matrix for sampling.
/// 
/// Forward transform order:
/// ```text
/// comp = position + R * S * (object - pivot)
/// ```
/// 
/// Returns inverse matrix for reverse-mapping: comp → object.
/// rotation_z is clockwise-positive (user convention).
pub fn build_inverse_transform(
    position: [f32; 3],
    rotation_z: f32,
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Affine2 {
    let pos = Vec2::new(position[0], position[1]);
    let pivot = Vec2::new(pivot[0], pivot[1]);
    let inv_scale = Vec2::new(
        if scale[0].abs() > f32::EPSILON { 1.0 / scale[0] } else { 0.0 },
        if scale[1].abs() > f32::EPSILON { 1.0 / scale[1] } else { 0.0 },
    );

    // rotation_z is clockwise-positive (user space). For the inverse transform we
    // rotate counter-clockwise by the same magnitude.
    Affine2::from_translation(pivot)
        * Affine2::from_angle(rotation_z)
        * Affine2::from_scale(inv_scale)
        * Affine2::from_translation(-pos)
}

/// Build inverse transform as column-major 3x3 matrix for OpenGL/GPU.
/// 
/// Same as `build_inverse_transform` but returns `[f32; 9]` in column-major
/// order suitable for `glUniformMatrix3fv`.
/// 
/// The matrix maps comp-space pixels (Y-up) to source image pixels (Y-down).
/// 
/// Matrix layout (column-major):
/// ```text
/// [m00, m10, 0,  m01, m11, 0,  tx, ty, 1]
///   col0        col1        col2
/// ```
/// 
/// For identity transform, returns `[1,0,0, 0,1,0, 0,0,1]}.
/// 
/// # GPU Compositor Integration (WIP)
/// 
/// This function is called from `compose_internal` to build matrices for each layer.
/// Currently these matrices are passed through the blend API but:
/// - **CPU compositor** ignores them (uses `transform_frame()` instead)
/// - **GPU compositor** shader is ready (`u_top_transform` uniform) but not connected
/// 
/// See `compositor.rs` module docs for full GPU transform status.
pub fn build_inverse_matrix_3x3(
    position: [f32; 3],
    rotation_z: f32,
    scale: [f32; 3],
    pivot: [f32; 3],
    src_size: (usize, usize),
) -> [f32; 9] {
    let inv = build_inverse_transform(position, rotation_z, scale, pivot);
    let src_half = Vec2::new(src_size.0 as f32 * 0.5, src_size.1 as f32 * 0.5);

    // object -> src (image space, Y-down): x' = x + w/2, y' = h/2 - y
    let object_to_src = Affine2::from_translation(Vec2::new(src_half.x, src_half.y))
        * Affine2::from_scale(Vec2::new(1.0, -1.0));

    let total = object_to_src * inv;

    // Affine2 stores: matrix2 (2x2 rotation/scale), translation (2D offset)
    // Convert to 3x3 column-major:
    // Col 0: [m00, m10, 0]
    // Col 1: [m01, m11, 0]
    // Col 2: [tx, ty, 1]
    let m = total.matrix2;
    let t = total.translation;

    [
        m.x_axis.x, m.x_axis.y, 0.0,  // column 0
        m.y_axis.x, m.y_axis.y, 0.0,  // column 1
        t.x,        t.y,        1.0,  // column 2
    ]
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
/// - `position` — `[x, y, z]` pivot position in comp space (z ignored)
/// - `rotation_z` — Z-axis rotation in radians (clockwise-positive)
/// - `scale` — `[sx, sy, sz]` scale factors (sz ignored)
/// - `pivot` — `[px, py, pz]` offset from layer center (pz ignored)
/// 
/// # Example
/// ```ignore
/// let transformed = transform_frame(
///     &frame,
///     (1920, 1080),
///     [100.0, 50.0, 0.0],  // move right 100, up 50
///     0.785,                // 45° clockwise rotation
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
    
    let comp_size = canvas;
    let src_size = (src_w, src_h);

    // Inverse transform: comp space -> object space
    let inv = build_inverse_transform(position, rotation_z, scale, pivot);
    
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
                        let comp_pt = space::image_to_comp(dst_pt, comp_size);
                        let obj_pt = inv.transform_point2(comp_pt);
                        let src_pt = space::object_to_src(obj_pt, src_size);

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
                        let comp_pt = space::image_to_comp(dst_pt, comp_size);
                        let obj_pt = inv.transform_point2(comp_pt);
                        let src_pt = space::object_to_src(obj_pt, src_size);

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
                        let comp_pt = space::image_to_comp(dst_pt, comp_size);
                        let obj_pt = inv.transform_point2(comp_pt);
                        let src_pt = space::object_to_src(obj_pt, src_size);

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
        assert!(is_identity([0.0, 0.0, 0.0], 0.0, [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([10.0, 0.0, 0.0], 0.0, [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], 0.1, [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], 0.0, [2.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], 0.0, [1.0, 1.0, 1.0], [5.0, 0.0, 0.0]));
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
