//! Frame compositor - blends multiple frames into one.
//!
//! Private module used by Comp for multi-layer blending.
//! Provides modular compositing backend:
//! - CPU compositor (default, works everywhere)
//! - GPU compositor (requires OpenGL context, 10-50x faster)

use crate::entities::frame::{Frame, PixelBuffer};

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
#[derive(Clone, Debug)]
pub enum CompositorType {
    /// CPU compositor - works everywhere, slower
    Cpu(CpuCompositor),
    /// GPU compositor - requires OpenGL context, 10-50x faster
    Gpu(GpuCompositor),
}

impl CompositorType {
    /// Blend frames using the selected compositor backend.
    pub fn blend(&mut self, frames: Vec<(Frame, f32, BlendMode)>) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend(frames),
            CompositorType::Gpu(gpu) => gpu.blend(frames),
        }
    }

    /// Blend frames into a canvas with explicit dimensions.
    pub fn blend_with_dim(
        &mut self,
        frames: Vec<(Frame, f32, BlendMode)>,
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

            // Apply blend mode to color channels
            let blend = |b: f32, t: f32| -> f32 {
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
            };

            result[i] = bottom[i] * inv_alpha + blend(bottom[i], top[i]) * top_alpha;
            result[i + 1] = bottom[i + 1] * inv_alpha + blend(bottom[i + 1], top[i + 1]) * top_alpha;
            result[i + 2] = bottom[i + 2] * inv_alpha + blend(bottom[i + 2], top[i + 2]) * top_alpha;
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

            let blend = |b: f32, t: f32| -> f32 {
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
            };

            result[i] = f16::from_f32(bottom[i].to_f32() * inv_alpha + blend(bottom[i].to_f32(), top[i].to_f32()) * top_alpha);
            result[i + 1] =
                f16::from_f32(bottom[i + 1].to_f32() * inv_alpha + blend(bottom[i + 1].to_f32(), top[i + 1].to_f32()) * top_alpha);
            result[i + 2] =
                f16::from_f32(bottom[i + 2].to_f32() * inv_alpha + blend(bottom[i + 2].to_f32(), top[i + 2].to_f32()) * top_alpha);
            result[i + 3] = f16::from_f32(bottom[i + 3].to_f32() * inv_alpha + top_alpha);
        }
    }

    /// Blend two U8 buffers with opacity (RGBA format)
    fn blend_u8(bottom: &[u8], top: &[u8], opacity: f32, mode: &BlendMode, result: &mut [u8]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = (top[i + 3] as f32 / 255.0) * opacity;
            let inv_alpha = 1.0 - top_alpha;

            let blend = |b: f32, t: f32| -> f32 {
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
            };

            let r = bottom[i] as f32 / 255.0;
            let g = bottom[i + 1] as f32 / 255.0;
            let b = bottom[i + 2] as f32 / 255.0;
            let tr = top[i] as f32 / 255.0;
            let tg = top[i + 1] as f32 / 255.0;
            let tb = top[i + 2] as f32 / 255.0;

            let out_r = r * inv_alpha + blend(r, tr) * top_alpha;
            let out_g = g * inv_alpha + blend(g, tg) * top_alpha;
            let out_b = b * inv_alpha + blend(b, tb) * top_alpha;
            let out_a = bottom[i + 3] as f32 / 255.0 * inv_alpha + top_alpha;

            result[i] = (out_r.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 1] = (out_g.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 2] = (out_b.clamp(0.0, 1.0) * 255.0) as u8;
            result[i + 3] = (out_a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }

    /// Blend frames bottom-to-top with opacity.
    pub(crate) fn blend(&self, frames: Vec<(Frame, f32, BlendMode)>) -> Option<Frame> {
        // Default to using first frame size
        if let Some((first, _, _)) = frames.get(0) {
            let dim = (first.width(), first.height());
            return self.blend_with_dim(frames, dim);
        }
        None
    }

    /// Blend frames onto a fixed-size canvas (width, height).
    pub(crate) fn blend_with_dim(
        &self,
        frames: Vec<(Frame, f32, BlendMode)>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        use log::debug;
        debug!(
            "CpuCompositor::blend_with_dim() called with {} frames into {}x{}",
            frames.len(),
            dim.0,
            dim.1
        );

        if frames.is_empty() {
            debug!("  -> empty frames, returning None");
            return None;
        }

        let (width, height) = dim;
        // Start with first frame cropped to canvas
        let mut iter = frames.iter();
        let (base_frame, _, _) = iter.next().unwrap(); // safe: frames non-empty
        let mut result = base_frame.clone();
        result.crop(width, height, crate::entities::frame::CropAlign::LeftTop);

        // Blend each subsequent layer on top
        for (layer_frame, opacity, mode) in iter {
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

            // Blend based on pixel format
            match (&*result_buffer, &*layer_buffer) {
                (PixelBuffer::F32(curr), PixelBuffer::F32(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_f32, curr, layer, blended);
                    result = Frame::from_f32_buffer(blended, width, height);
                }
                (PixelBuffer::F16(curr), PixelBuffer::F16(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_f16, curr, layer, blended);
                    result = Frame::from_f16_buffer(blended, width, height);
                }
                (PixelBuffer::U8(curr), PixelBuffer::U8(layer)) => {
                    let mut blended = curr.clone();
                    blend_rows!(blend_u8, curr, layer, blended);
                    result = Frame::from_u8_buffer(blended, width, height);
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
