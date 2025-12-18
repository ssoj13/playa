//! Frame compositor - blends multiple frames into one.
//!
//! Private module used by Comp for multi-layer blending.
//! Provides modular compositing backend:
//! - CPU compositor (default, works everywhere)
//! - GPU compositor (requires OpenGL context, 10-50x faster)
//!
//! # GPU Transform Support (WIP)
//!
//! The blend API includes transform matrices `[f32; 9]` for GPU-accelerated
//! layer transforms. Current state:
//!
//! - **API ready**: `blend()` accepts `Vec<(Frame, f32, BlendMode, [f32; 9])>`
//! - **GPU shader ready**: `gpu_compositor.rs` has `u_top_transform` mat3 uniform
//! - **Matrix builder ready**: `transform::build_inverse_matrix_3x3()`
//! - **compose_internal ready**: passes inverse matrices from layer attrs
//!
//! **NOT YET WORKING:**
//! - CPU compositor ignores transform matrix (applies CPU transform beforehand)
//! - GPU compositor not used for compose (requires GL context, can't run in workers)
//! - Switching GPU/CPU in prefs only affects Project.compositor, not compose_internal
//!
//! **To enable GPU compositing:**
//! 1. compose_internal runs in main thread (has GL context)
//! 2. Pass Project.compositor to compose_internal via ComputeContext
//! 3. Remove CPU transform in compose_internal, let GPU handle it
//!
//! For now, CPU compositor + CPU transforms work fine. GPU is viewport-only.

use crate::entities::frame::{Frame, FrameStatus, PixelBuffer};

// === GPU Compositor Toggle ===
// To enable GPU compositor: uncomment the line below
// To disable GPU compositor: comment the line below
use super::gpu_compositor::GpuCompositor;

/// Supported blend modes for layer compositing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Screen,
    Add,
    Subtract,
    Multiply,
    Divide,
    Difference,
}

/// Compositor type enum - allows switching between CPU/GPU backends.
/// Note: Clone creates CPU compositor (GPU resources can't be cloned)
#[derive(Debug)]
pub enum CompositorType {
    /// CPU compositor - works everywhere, slower
    Cpu(CpuCompositor),
    /// GPU compositor - requires OpenGL context, 10-50x faster
    Gpu(GpuCompositor),
}

impl Clone for CompositorType {
    fn clone(&self) -> Self {
        // GPU compositor can't be cloned (OpenGL resources are tied to context)
        // This is expected during Project serialization (compositor is #[serde(skip)])
        // but should NOT happen in normal code - use RefCell or Arc instead
        if matches!(self, CompositorType::Gpu(_)) {
            log::warn!("CompositorType::clone() called on GPU variant - downgrading to CPU. \
                        This may indicate a bug if not during serialization.");
        }
        CompositorType::Cpu(CpuCompositor)
    }
}

/// Identity transform matrix (no transformation).
/// Column-major 3x3 for OpenGL: `[m00, m10, 0, m01, m11, 0, tx, ty, 1]`
/// 
/// Used when layer has no transform (position=0, rotation=0, scale=1).
/// See `transform::build_inverse_matrix_3x3()` for non-identity transforms.
pub const IDENTITY_TRANSFORM: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

impl CompositorType {
    /// Blend frames using the selected compositor backend.
    /// Each frame has: (pixels, opacity, blend_mode, inverse_transform_matrix)
    pub fn blend(&mut self, frames: Vec<(Frame, f32, BlendMode, [f32; 9])>) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend(frames),
            CompositorType::Gpu(gpu) => gpu.blend(frames),
        }
    }

    /// Blend frames into a canvas with explicit dimensions.
    pub fn blend_with_dim(
        &mut self,
        frames: Vec<(Frame, f32, BlendMode, [f32; 9])>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend_with_dim(frames, dim),
            CompositorType::Gpu(gpu) => gpu.blend_with_dim(frames, dim),
        }
    }
}

impl Default for CompositorType {
    fn default() -> Self {
        CompositorType::Cpu(CpuCompositor)
    }
}

/// Apply blend mode to two normalized color values (0.0-1.0).
/// Bottom is destination, top is source. Returns blended value.
#[inline]
fn apply_blend(b: f32, t: f32, mode: &BlendMode) -> f32 {
    let t_clamped = t.clamp(0.0, 1.0);
    let b_clamped = b.clamp(0.0, 1.0);
    match mode {
        BlendMode::Normal => t_clamped,
        BlendMode::Screen => 1.0 - (1.0 - b_clamped) * (1.0 - t_clamped),
        BlendMode::Add => (b_clamped + t_clamped).min(1.0),
        BlendMode::Subtract => (b_clamped - t_clamped).max(0.0),
        BlendMode::Multiply => b_clamped * t_clamped,
        BlendMode::Divide => {
            if t_clamped <= 0.00001 {
                b_clamped
            } else {
                (b_clamped / t_clamped).min(1.0)
            }
        }
        BlendMode::Difference => (b_clamped - t_clamped).abs(),
    }
}

/// CPU compositor - simple alpha blending on CPU.
#[derive(Clone, Debug)]
pub struct CpuCompositor;

impl CpuCompositor {
    /// Blend two F32 buffers with opacity (RGBA format)
    fn blend_f32(
        bottom: &[f32],
        top: &[f32],
        opacity: f32,
        mode: &BlendMode,
        result: &mut [f32],
    ) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = top[i + 3] * opacity;
            let inv_alpha = 1.0 - top_alpha;

            result[i] = bottom[i] * inv_alpha + apply_blend(bottom[i], top[i], mode) * top_alpha;
            result[i + 1] = bottom[i + 1] * inv_alpha + apply_blend(bottom[i + 1], top[i + 1], mode) * top_alpha;
            result[i + 2] = bottom[i + 2] * inv_alpha + apply_blend(bottom[i + 2], top[i + 2], mode) * top_alpha;
            result[i + 3] = bottom[i + 3] * inv_alpha + top_alpha;
        }
    }

    /// Blend two F16 buffers with opacity (RGBA format)
    fn blend_f16(
        bottom: &[half::f16],
        top: &[half::f16],
        opacity: f32,
        mode: &BlendMode,
        result: &mut [half::f16],
    ) {
        use half::f16;
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = top[i + 3].to_f32() * opacity;
            let inv_alpha = 1.0 - top_alpha;

            let b0 = bottom[i].to_f32();
            let b1 = bottom[i + 1].to_f32();
            let b2 = bottom[i + 2].to_f32();
            let b3 = bottom[i + 3].to_f32();
            let t0 = top[i].to_f32();
            let t1 = top[i + 1].to_f32();
            let t2 = top[i + 2].to_f32();

            result[i] = f16::from_f32(b0 * inv_alpha + apply_blend(b0, t0, mode) * top_alpha);
            result[i + 1] = f16::from_f32(b1 * inv_alpha + apply_blend(b1, t1, mode) * top_alpha);
            result[i + 2] = f16::from_f32(b2 * inv_alpha + apply_blend(b2, t2, mode) * top_alpha);
            result[i + 3] = f16::from_f32(b3 * inv_alpha + top_alpha);
        }
    }

    /// Blend two U8 buffers with opacity (RGBA format)
    fn blend_u8(bottom: &[u8], top: &[u8], opacity: f32, mode: &BlendMode, result: &mut [u8]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = (top[i + 3] as f32 / 255.0) * opacity;
            let inv_alpha = 1.0 - top_alpha;

            let r = bottom[i] as f32 / 255.0;
            let g = bottom[i + 1] as f32 / 255.0;
            let b = bottom[i + 2] as f32 / 255.0;
            let tr = top[i] as f32 / 255.0;
            let tg = top[i + 1] as f32 / 255.0;
            let tb = top[i + 2] as f32 / 255.0;

            let out_r = r * inv_alpha + apply_blend(r, tr, mode) * top_alpha;
            let out_g = g * inv_alpha + apply_blend(g, tg, mode) * top_alpha;
            let out_b = b * inv_alpha + apply_blend(b, tb, mode) * top_alpha;
            let out_a = bottom[i + 3] as f32 / 255.0 * inv_alpha + top_alpha;

            result[i] = (out_r.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 1] = (out_g.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 2] = (out_b.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 3] = (out_a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }

    /// Blend frames bottom-to-top with opacity.
    /// Blend frames using CPU.
    /// 
    /// Each frame: (pixels, opacity, blend_mode, inverse_transform_matrix)
    /// 
    /// **Note:** Transform matrix is IGNORED by CPU compositor.
    /// For CPU path, transforms are applied beforehand via `transform::transform_frame()`
    /// in `compose_internal`. The matrix is passed for API compatibility with GPU compositor.
    pub(crate) fn blend(&self, frames: Vec<(Frame, f32, BlendMode, [f32; 9])>) -> Option<Frame> {
        // Default to using first frame size
        if let Some((first, _, _, _)) = frames.first() {
            let dim = (first.width(), first.height());
            return self.blend_with_dim(frames, dim);
        }
        None
    }

    /// Blend frames onto a fixed-size canvas (width, height).
    /// 
    /// **CPU path:** Transform matrix `[f32; 9]` is ignored here.
    /// Transforms are pre-applied in `compose_internal` via `transform::transform_frame()`.
    /// This is less efficient than GPU (transforms pixels twice) but works in worker threads.
    /// 
    /// **TODO for GPU compositing:**
    /// When GPU compositor is used, transforms should NOT be pre-applied.
    /// Instead, pass original frames + matrices, let GPU shader handle transforms.
    pub(crate) fn blend_with_dim(
        &self,
        frames: Vec<(Frame, f32, BlendMode, [f32; 9])>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        use log::trace;
        trace!(
            "CpuCompositor::blend_with_dim() called with {} frames into {}x{}",
            frames.len(),
            dim.0,
            dim.1
        );

        if frames.is_empty() {
            trace!("  -> empty frames, returning None");
            return None;
        }

        // Calculate minimum status from all input frames
        // Composition is only as good as its worst component
        let min_status = frames
            .iter()
            .map(|(f, _, _, _)| f.status())
            .min_by_key(|s| match s {
                FrameStatus::Error => 0,
                FrameStatus::Placeholder => 1,
                FrameStatus::Header => 2,
                FrameStatus::Loading | FrameStatus::Composing | FrameStatus::Expired => 3,
                FrameStatus::Loaded => 4,
            })
            .unwrap_or(FrameStatus::Placeholder);

        trace!("  -> min_status from inputs: {:?}", min_status);

        let (width, height) = dim;
        // Start with first frame cropped to canvas
        let mut iter = frames.iter();
        let (base_frame, _, _, _) = iter.next().unwrap(); // safe: frames non-empty
        let mut result = base_frame.clone();
        result.crop(width, height, crate::entities::frame::CropAlign::LeftTop);

        // Blend each subsequent layer on top
        // Note: _transform is ignored - CPU path applies transform beforehand
        // in compose_internal via transform::transform_frame()
        // GPU path would use this matrix in shader instead
        for (layer_frame, opacity, mode, _transform) in iter {
            let result_buffer = result.buffer();
            let layer_buffer = layer_frame.buffer();

            let lw = layer_frame.width();
            let lh = layer_frame.height();
            let overlap_w = width.min(lw);
            let overlap_h = height.min(lh);
            if overlap_w == 0 || overlap_h == 0 {
                continue;
            }

            macro_rules! blend_rows {
                ($blend_fn:ident, $curr:expr, $layer:expr, $out:expr) => {{
                    let base_stride = width * 4;
                    let layer_stride = lw * 4;
                    for y in 0..overlap_h {
                        let b_off = y * base_stride;
                        let l_off = y * layer_stride;
                        let base_slice = &$curr[b_off..b_off + overlap_w * 4];
                        let layer_slice = &$layer[l_off..l_off + overlap_w * 4];
                        let out_slice = &mut $out[b_off..b_off + overlap_w * 4];
                        Self::$blend_fn(base_slice, layer_slice, *opacity, mode, out_slice);
                    }
                }};
            }

            // Blend based on pixel format - use min_status for composed result
            match (&*result_buffer, &*layer_buffer) {
                (PixelBuffer::F32(curr), PixelBuffer::F32(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_f32, curr, layer, blended);
                    result = Frame::from_f32_buffer_with_status(blended, width, height, min_status);
                }
                (PixelBuffer::F16(curr), PixelBuffer::F16(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_f16, curr, layer, blended);
                    result = Frame::from_f16_buffer_with_status(blended, width, height, min_status);
                }
                (PixelBuffer::U8(curr), PixelBuffer::U8(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_u8, curr, layer, blended);
                    result = Frame::from_u8_buffer_with_status(blended, width, height, min_status);
                }
                _ => {
                    log::warn!("Pixel format mismatch during compositing, skipping layer");
                    continue;
                }
            }
        }

        Some(result)
    }
}
