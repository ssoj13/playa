//! 3D affine transforms for layer compositing.
//!
//! Uses glam::Mat4 for matrix math in Y-up space.
//! Forward transform (layer -> comp):
//! comp = position + R * S * (object - pivot)
//!
//! Rotation order: ZYX (AE-style) - rotate Z first, then Y, then X.
//! Rotation sign: clockwise-positive when looking down axis (user convention).

use glam::{Mat4, Vec2, Vec3, Vec4, Quat, EulerRot};
use half::f16 as F16;
use rayon::prelude::*;

use super::frame::{Frame, PixelBuffer, PixelFormat};
use super::space;

/// Check if transform is identity (no-op).
///
/// Returns true if position==pivot, rotation=0, scale=1 — no transform needed.
#[inline]
pub fn is_identity(position: [f32; 3], rotation: [f32; 3], scale: [f32; 3], pivot: [f32; 3]) -> bool {
    position[0] == pivot[0]
        && position[1] == pivot[1]
        && position[2] == pivot[2]
        && rotation[0] == 0.0
        && rotation[1] == 0.0
        && rotation[2] == 0.0
        && scale[0] == 1.0
        && scale[1] == 1.0
        && scale[2] == 1.0
}

/// Check if a view-projection matrix is orthographic (affine) or perspective (projective).
///
/// Orthographic VP has bottom row ≈ [0, 0, 0, 1].
/// Perspective VP has bottom row with significant values in first 3 components.
#[inline]
fn is_orthographic_vp(vp: Mat4) -> bool {
    let row3 = vp.row(3);
    row3.x.abs() < 1e-6 && row3.y.abs() < 1e-6 && (row3.w - 1.0).abs() < 1e-6
}

/// Unproject NDC point to world space using ray-plane intersection.
///
/// For perspective projection, casts a ray from camera through NDC point
/// and intersects with the layer plane (defined by position and normal).
///
/// # Arguments
/// - `ndc` - Normalized device coordinates [-1, 1]
/// - `inv_vp` - Inverse view-projection matrix
/// - `plane_point` - A point on the layer plane (layer position)
/// - `plane_normal` - Normal of the layer plane (from rotation)
///
/// # Returns
/// World-space point on the plane, or None if ray is parallel to plane.
#[inline]
fn unproject_to_plane(
    ndc: Vec2,
    inv_vp: Mat4,
    plane_point: Vec3,
    plane_normal: Vec3,
) -> Option<Vec3> {
    // Unproject two points at near and far planes to get the ray
    let near_clip = Vec4::new(ndc.x, ndc.y, -1.0, 1.0);
    let far_clip = Vec4::new(ndc.x, ndc.y, 1.0, 1.0);

    let near_world4 = inv_vp * near_clip;
    let far_world4 = inv_vp * far_clip;

    // Perspective divide
    if near_world4.w.abs() < 1e-6 || far_world4.w.abs() < 1e-6 {
        return None;
    }
    let near_world = Vec3::new(
        near_world4.x / near_world4.w,
        near_world4.y / near_world4.w,
        near_world4.z / near_world4.w,
    );
    let far_world = Vec3::new(
        far_world4.x / far_world4.w,
        far_world4.y / far_world4.w,
        far_world4.z / far_world4.w,
    );

    // Ray direction
    let ray_dir = far_world - near_world;

    // Ray-plane intersection: (ray_origin + t * ray_dir - plane_point) · plane_normal = 0
    // t = ((plane_point - ray_origin) · plane_normal) / (ray_dir · plane_normal)
    let denom = ray_dir.dot(plane_normal);
    if denom.abs() < 1e-6 {
        return None; // Ray parallel to plane
    }
    let t = (plane_point - near_world).dot(plane_normal) / denom;

    Some(near_world + ray_dir * t)
}

/// Compute layer plane normal from rotation.
///
/// In object space, the layer plane is z=0 with normal [0, 0, 1].
/// After rotation, the normal is transformed.
#[inline]
fn layer_plane_normal(rotation: [f32; 3]) -> Vec3 {
    // Build rotation matrix (same as in build_model_matrix)
    // Our convention: CW+ (clockwise positive)
    // glam convention: CCW+ (counter-clockwise positive)
    // Convert: negate angles
    let quat = Quat::from_euler(
        EulerRot::ZYX,
        -rotation[2], // Z first (negated for CW→CCW)
        -rotation[1], // then Y
        -rotation[0], // then X
    );
    // Transform the local Z axis (normal of z=0 plane)
    quat * Vec3::Z
}

/// Build inverse transform matrix for sampling.
/// 
/// Forward transform order:
/// ```text
/// comp = position + R * S * (object - pivot)
/// ```
/// 
/// Returns inverse Mat4 for reverse-mapping: comp → object.
/// 
/// # Arguments
/// - `position` — layer position in comp space (XYZ)
/// - `rotation` — rotation in radians [rx, ry, rz], clockwise-positive
/// - `scale` — scale factors [sx, sy, sz]
/// - `pivot` — pivot offset from layer center [px, py, pz]
pub fn build_inverse_transform(
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Mat4 {
    let pos = Vec3::from(position);
    let pvt = Vec3::from(pivot);
    let inv_scale = Vec3::new(
        if scale[0].abs() > f32::EPSILON { 1.0 / scale[0] } else { 0.0 },
        if scale[1].abs() > f32::EPSILON { 1.0 / scale[1] } else { 0.0 },
        if scale[2].abs() > f32::EPSILON { 1.0 / scale[2] } else { 0.0 },
    );

    // Inverse rotation: reverse order (XYZ is inverse of ZYX).
    // Our convention is CW+ (clockwise positive), glam uses CCW+ (math convention).
    // Forward rotation in our convention = negative in glam.
    // Inverse of forward = positive in glam = just pass our values as-is!
    let inv_rot = Quat::from_euler(
        EulerRot::XYZ,  // reverse of ZYX
        rotation[0],    // NOT negated: our CW+ -> glam CCW+ for inverse
        rotation[1],
        rotation[2],
    );

    // Inverse transform chain (right-to-left application):
    // 1. T(-pos): subtract position
    // 2. R^(-1): inverse rotate
    // 3. S^(-1): inverse scale
    // 4. T(pvt): add pivot
    // Result: object = pivot + S^(-1) * R^(-1) * (comp - position)
    Mat4::from_translation(pvt)
        * Mat4::from_scale(inv_scale)
        * Mat4::from_quat(inv_rot)
        * Mat4::from_translation(-pos)
}

/// Build model matrix (object -> world/comp space).
///
/// Forward transform: comp = position + R * S * (object - pivot)
///
/// # Arguments
/// - `position` — layer position in comp space (XYZ)
/// - `rotation` — rotation in radians [rx, ry, rz], clockwise-positive (user convention)
/// - `scale` — scale factors [sx, sy, sz]
/// - `pivot` — pivot offset from layer center [px, py, pz]
pub fn build_model_matrix(
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Mat4 {
    let pos = Vec3::from(position);
    let pvt = Vec3::from(pivot);
    let scl = Vec3::from(scale);

    // Rotation: ZYX order (AE-style)
    // Our convention: CW+ (clockwise positive)
    // glam convention: CCW+ (counter-clockwise positive)
    // Convert: negate angles
    let rot = Quat::from_euler(
        EulerRot::ZYX,
        -rotation[2], // negated for CW→CCW
        -rotation[1],
        -rotation[0],
    );

    // Forward transform: translate(-pivot) -> scale -> rotate -> translate(position)
    Mat4::from_translation(pos)
        * Mat4::from_quat(rot)
        * Mat4::from_scale(scl)
        * Mat4::from_translation(-pvt)
}

/// Build full inverse MVP matrix for camera-aware transform.
/// 
/// Combines model (layer), view (camera), and projection matrices.
/// Returns inverse for reverse-mapping: screen pixel -> object space.
/// 
/// # Arguments
/// - `model` — layer model matrix from `build_model_matrix()`
/// - `view_projection` — camera view-projection matrix (or identity for 2D)
pub fn build_inverse_mvp(model: Mat4, view_projection: Mat4) -> Mat4 {
    let mvp = view_projection * model;
    mvp.inverse()
}

/// Build inverse transform as column-major 3x3 matrix for OpenGL/GPU (2D only).
/// 
/// Same as `build_inverse_transform` but returns `[f32; 9]` in column-major
/// order suitable for `glUniformMatrix3fv`. Only uses Z rotation.
/// 
/// The matrix maps comp-space pixels (Y-up) to source image pixels (Y-down).
pub fn build_inverse_matrix_3x3(
    position: [f32; 3],
    rotation_z: f32,
    scale: [f32; 3],
    pivot: [f32; 3],
    src_size: (usize, usize),
) -> [f32; 9] {
    // Use 2D path for backwards compatibility
    use glam::Affine2;
    
    let pos = Vec2::new(position[0], position[1]);
    let pvt = Vec2::new(pivot[0], pivot[1]);
    let inv_scale = Vec2::new(
        if scale[0].abs() > f32::EPSILON { 1.0 / scale[0] } else { 0.0 },
        if scale[1].abs() > f32::EPSILON { 1.0 / scale[1] } else { 0.0 },
    );

    let inv = Affine2::from_translation(pvt)
        * Affine2::from_angle(rotation_z)
        * Affine2::from_scale(inv_scale)
        * Affine2::from_translation(-pos);
        
    let src_half = Vec2::new(src_size.0 as f32 * 0.5, src_size.1 as f32 * 0.5);

    // object -> src (image space, Y-down): x' = x + w/2, y' = h/2 - y
    let object_to_src = Affine2::from_translation(Vec2::new(src_half.x, src_half.y))
        * Affine2::from_scale(Vec2::new(1.0, -1.0));

    let total = object_to_src * inv;

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

/// Transform frame with 3D affine matrix (no camera).
/// 
/// Applies position, rotation, scale around pivot point using parallel
/// pixel processing (rayon). Output format matches input format.
/// Uses orthographic projection (no perspective).
/// 
/// # Arguments
/// - `src` — Source frame (U8/F16/F32)
/// - `canvas` — Output dimensions `(width, height)`
/// - `position` — `[x, y, z]` pivot position in comp space
/// - `rotation` — `[rx, ry, rz]` rotation in radians (clockwise-positive)
/// - `scale` — `[sx, sy, sz]` scale factors
/// - `pivot` — `[px, py, pz]` offset from layer center
/// 
/// # Example
/// ```ignore
/// let transformed = transform_frame(
///     &frame,
///     (1920, 1080),
///     [100.0, 50.0, 0.0],       // move right 100, up 50
///     [0.0, 0.0, 0.785],        // 45deg Z rotation
///     [0.5, 0.5, 1.0],          // 50% XY scale
///     [0.0, 0.0, 0.0],          // pivot at center
/// );
/// ```
pub fn transform_frame(
    src: &Frame,
    canvas: (usize, usize),
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
) -> Frame {
    // No camera - use identity view-projection
    transform_frame_with_camera(src, canvas, position, rotation, scale, pivot, None)
}

/// Transform frame with optional camera view-projection.
///
/// When camera is provided, applies full MVP transform for perspective/ortho projection.
/// For perspective, uses ray-plane intersection to properly handle the nonlinear projection.
///
/// # Arguments
/// - `src` - Source frame (U8/F16/F32)
/// - `canvas` - Output dimensions (width, height)
/// - `position` - Layer position [x, y, z] in world/comp space
/// - `rotation` - Rotation [rx, ry, rz] in radians (ZYX order)
/// - `scale` - Scale factors [sx, sy, sz]
/// - `pivot` - Pivot offset from layer center [px, py, pz]
/// - `view_projection` - Camera view-projection matrix (None = identity/ortho)
pub fn transform_frame_with_camera(
    src: &Frame,
    canvas: (usize, usize),
    position: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
    pivot: [f32; 3],
    view_projection: Option<Mat4>,
) -> Frame {
    let src_w = src.width();
    let src_h = src.height();
    let (dst_w, dst_h) = canvas;

    let comp_size = canvas;
    let src_size = (src_w, src_h);

    // Build inverse model transform: world/comp -> object space
    let inv_model = build_inverse_transform(position, rotation, scale, pivot);

    // Layer plane in world space (for ray-plane intersection)
    let plane_point = Vec3::from(position);
    let plane_normal = layer_plane_normal(rotation);

    // Precompute NDC scale factors
    let half_w = dst_w as f32 * 0.5;
    let half_h = dst_h as f32 * 0.5;

    // Check camera type and prepare transforms
    let camera_info: Option<(Mat4, Mat4, bool)> = view_projection.map(|vp| {
        let inv_vp = vp.inverse();
        let is_ortho = is_orthographic_vp(vp);
        (vp, inv_vp, is_ortho)
    });

    // Get source buffer
    let src_buffer = src.buffer();
    let src_format = src.pixel_format();

    // Helper closure to transform screen point to object space
    // Returns None if point is outside valid range (e.g., ray parallel to plane)
    let transform_point = |frame_pt: Vec2| -> Option<Vec2> {
        match camera_info {
            Some((_vp, inv_vp, is_ortho)) => {
                if is_ortho {
                    // Orthographic: use simple affine transform
                    // inv_vp is affine, so we can just multiply
                    let ndc = Vec3::new(frame_pt.x / half_w, frame_pt.y / half_h, 0.0);
                    let world_pt = inv_vp.transform_point3(ndc);
                    let obj_pt3 = inv_model.transform_point3(world_pt);
                    Some(Vec2::new(obj_pt3.x, obj_pt3.y))
                } else {
                    // Perspective: use ray-plane intersection
                    let ndc = Vec2::new(frame_pt.x / half_w, frame_pt.y / half_h);
                    let world_pt = unproject_to_plane(ndc, inv_vp, plane_point, plane_normal)?;
                    let obj_pt3 = inv_model.transform_point3(world_pt);
                    Some(Vec2::new(obj_pt3.x, obj_pt3.y))
                }
            }
            None => {
                // No camera: direct affine transform
                let frame_pt3 = Vec3::new(frame_pt.x, frame_pt.y, 0.0);
                let obj_pt3 = inv_model.transform_point3(frame_pt3);
                Some(Vec2::new(obj_pt3.x, obj_pt3.y))
            }
        }
    };

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
                        // Transform dst coord (image space) -> frame space (centered)
                        let dst_pt = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let frame_pt = space::image_to_frame(dst_pt, comp_size);

                        // Transform to object space
                        let color = if let Some(obj_pt) = transform_point(frame_pt) {
                            let src_pt = space::object_to_src(obj_pt, src_size);
                            sample_f32(buf, src_w, src_h, src_pt.x, src_pt.y)
                        } else {
                            [0.0, 0.0, 0.0, 0.0] // Transparent for invalid points
                        };

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
                        let frame_pt = space::image_to_frame(dst_pt, comp_size);

                        let color = if let Some(obj_pt) = transform_point(frame_pt) {
                            let src_pt = space::object_to_src(obj_pt, src_size);
                            sample_f16(buf, src_w, src_h, src_pt.x, src_pt.y)
                        } else {
                            [0.0, 0.0, 0.0, 0.0]
                        };

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
                        let frame_pt = space::image_to_frame(dst_pt, comp_size);

                        let color = if let Some(obj_pt) = transform_point(frame_pt) {
                            let src_pt = space::object_to_src(obj_pt, src_size);
                            sample_u8(buf, src_w, src_h, src_pt.x, src_pt.y)
                        } else {
                            [0.0, 0.0, 0.0, 0.0]
                        };

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
        assert!(is_identity([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], [0.1, 0.0, 0.0], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], [0.0, 0.0, 0.1], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [2.0, 1.0, 1.0], [0.0, 0.0, 0.0]));
        assert!(!is_identity([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [5.0, 0.0, 0.0]));
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
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
        );
        
        assert_eq!(result.width(), 4);
        assert_eq!(result.height(), 4);
    }
}
