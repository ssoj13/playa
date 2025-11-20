//! Frame compositor - blends multiple frames into one.
//!
//! Private module used by Comp for multi-layer blending.
//! Provides modular compositing backend:
//! - CPU compositor (default, works everywhere)
//! - GPU compositor (future, requires OpenGL/WGPU context)

use crate::entities::frame::{Frame, PixelBuffer};

/// Compositor type enum - allows switching between CPU/GPU backends.
#[derive(Clone, Debug)]
pub enum CompositorType {
    /// CPU compositor - works everywhere, slower
    Cpu(CpuCompositor),
    // Gpu(GpuCompositor),  // Future: GPU compositor
}

impl CompositorType {
    /// Blend frames using the selected compositor backend.
    pub fn blend(&self, frames: Vec<(Frame, f32)>) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend(frames),
            // CompositorType::Gpu(gpu) => gpu.blend(frames),
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
    fn blend_f32(bottom: &[f32], top: &[f32], opacity: f32, result: &mut [f32]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = top[i + 3] * opacity;
            let inv_alpha = 1.0 - top_alpha;

            // Blend RGB channels
            result[i] = bottom[i] * inv_alpha + top[i] * top_alpha;
            result[i + 1] = bottom[i + 1] * inv_alpha + top[i + 1] * top_alpha;
            result[i + 2] = bottom[i + 2] * inv_alpha + top[i + 2] * top_alpha;
            result[i + 3] = bottom[i + 3] * inv_alpha + top_alpha;
        }
    }

    /// Blend two F16 buffers with opacity (RGBA format)
    fn blend_f16(bottom: &[half::f16], top: &[half::f16], opacity: f32, result: &mut [half::f16]) {
        use half::f16;
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = top[i + 3].to_f32() * opacity;
            let inv_alpha = 1.0 - top_alpha;

            result[i] = f16::from_f32(bottom[i].to_f32() * inv_alpha + top[i].to_f32() * top_alpha);
            result[i + 1] = f16::from_f32(bottom[i + 1].to_f32() * inv_alpha + top[i + 1].to_f32() * top_alpha);
            result[i + 2] = f16::from_f32(bottom[i + 2].to_f32() * inv_alpha + top[i + 2].to_f32() * top_alpha);
            result[i + 3] = f16::from_f32(bottom[i + 3].to_f32() * inv_alpha + top_alpha);
        }
    }

    /// Blend two U8 buffers with opacity (RGBA format)
    fn blend_u8(bottom: &[u8], top: &[u8], opacity: f32, result: &mut [u8]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = (top[i + 3] as f32 / 255.0) * opacity;
            let inv_alpha = 1.0 - top_alpha;

            result[i] = (bottom[i] as f32 * inv_alpha + top[i] as f32 * top_alpha) as u8;
            result[i + 1] = (bottom[i + 1] as f32 * inv_alpha + top[i + 1] as f32 * top_alpha) as u8;
            result[i + 2] = (bottom[i + 2] as f32 * inv_alpha + top[i + 2] as f32 * top_alpha) as u8;
            result[i + 3] = (bottom[i + 3] as f32 * inv_alpha + top_alpha * 255.0) as u8;
        }
    }

    /// Blend frames bottom-to-top with opacity.
    pub(crate) fn blend(&self, frames: Vec<(Frame, f32)>) -> Option<Frame> {
        if frames.is_empty() {
            return None;
        }

        // Single frame - return clone
        if frames.len() == 1 {
            return Some(frames[0].0.clone());
        }

        // Multiple frames - blend bottom-to-top
        let (first_frame, _) = &frames[0];
        let width = first_frame.width();
        let height = first_frame.height();

        // Start with first frame as base
        let mut result = first_frame.clone();

        // Blend each subsequent layer on top
        for (layer_frame, opacity) in frames.iter().skip(1) {
            // Verify dimensions match
            if layer_frame.width() != width || layer_frame.height() != height {
                log::warn!("Frame dimension mismatch during compositing, skipping layer");
                continue;
            }

            let result_buffer = result.buffer();
            let layer_buffer = layer_frame.buffer();

            // Blend based on pixel format
            match (&*result_buffer, &*layer_buffer) {
                (PixelBuffer::F32(curr), PixelBuffer::F32(layer)) => {
                    let mut blended = curr.clone();
                    Self::blend_f32(curr, layer, *opacity, &mut blended);
                    // Create new frame with blended data
                    result = Frame::from_f32_buffer(blended, width, height);
                }
                (PixelBuffer::F16(curr), PixelBuffer::F16(layer)) => {
                    let mut blended = curr.clone();
                    Self::blend_f16(curr, layer, *opacity, &mut blended);
                    result = Frame::from_f16_buffer(blended, width, height);
                }
                (PixelBuffer::U8(curr), PixelBuffer::U8(layer)) => {
                    let mut blended = curr.clone();
                    Self::blend_u8(curr, layer, *opacity, &mut blended);
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
