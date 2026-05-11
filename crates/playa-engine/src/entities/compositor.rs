//! Frame compositor — blends multiple layers into one [`Frame`].
//!
//! Two backends live behind [`CompositorType`]:
//! - **CPU** — [`CpuCompositor`], works on any thread (workers, encode, nested preload).
//! - **WGPU** — [`crate::render_gpu::WgpuCompositor`], UI thread + shared `wgpu` queue from `eframe`.
//!
//! # How [`CompNode`](crate::entities::comp_node::CompNode) uses this
//!
//! Final `blend_with_dim` for the active project **must** run against the same [`CompositorType`] the
//! user selected. Workers do not surface the desktop `wgpu` queue, so when prefs pick the GPU raster
//! path,
//! [`CompNode::compose_internal`](crate::entities::comp_node::CompNode::compose_internal) moves the
//! stacked rasters through [`super::gpu_blend_bridge::GpuBlendBridge`]; the desktop host drains that
//! queue on the UI thread (`PlayaApp::drain_gpu_blend_queue`) **after** the GPU compositor is wired to
//! the current `wgpu::Device`/queue — see `gpu_blend_bridge.rs`. Cpu prefs keep everything on workers via a
//! per-thread Cpu compositor (`THREAD_COMPOSITOR` in `comp_node`).
//!
//! Encode / blocking `get_frame` paths intentionally omit the bridge
//! (`ComputeContext.gpu_blend_bridge == None`) so deterministic jobs never block on channels.
//!
//! # GPU transforms (WIP for full parity)
//!
//! The API carries inverse 3×3 matrices `[f32; 9]` per layer ([`IDENTITY_TRANSFORM`] when flat).
//!
//! **Present today:** matrices flow through `blend` / [`CompositorType::blend_with_dim`]; the WGSL
//! shader reads `u_top_transform` (see `render_gpu/shaders/layer_blend.wgsl`).
//!
//! **Still asymmetric:** the Cpu path ignores the matrix bundle — transforms are baked into pixels
//! earlier in compose. Skipping Cpu transform there while feeding raw mats only to Gpu is future work.

use crate::entities::frame::{Frame, FrameStatus, PixelBuffer};
use crate::render_gpu::WgpuCompositor;

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
    Overlay,
}

/// Compositor type enum - allows switching between CPU/GPU backends.
/// Note: Clone creates CPU compositor (GPU resources can't be cloned)
#[derive(Debug)]
pub enum CompositorType {
    /// CPU compositor - works everywhere, slower
    Cpu(CpuCompositor),
    /// wgpu raster compositor (UI thread).
    Wgpu(WgpuCompositor),
}

impl Clone for CompositorType {
    fn clone(&self) -> Self {
        // GPU compositor can't be cloned (wgpu resources are tied to the shared device queue)
        // This is expected during Project serialization (compositor is #[serde(skip)])
        // but should NOT happen in normal code - use RefCell or Arc instead
        if matches!(self, CompositorType::Wgpu(_)) {
            log::warn!(
                "CompositorType::clone() called on Wgpu variant - downgrading to CPU. \
                        This may indicate a bug if not during serialization."
            );
        }
        CompositorType::Cpu(CpuCompositor)
    }
}

/// Identity transform matrix (no transformation).
/// Column-major 3x3 for OpenGL: `[m00, m10, 0, m01, m11, 0, tx, ty, 1]`
///
/// Used when layer has no transform (position=0, rotation=0, scale=1)
/// AND src_size == canvas_size (no centering required).
/// See `transform::build_inverse_canvas_to_src_3x3()` for non-identity
/// transforms or for any case where src and canvas dimensions differ.
pub const IDENTITY_TRANSFORM: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

impl CompositorType {
    /// Blend frames using the selected compositor backend.
    /// Each frame has: (pixels, opacity, blend_mode, inverse_transform_matrix)
    pub fn blend(&mut self, frames: Vec<(Frame, f32, BlendMode, [f32; 9])>) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend(frames),
            CompositorType::Wgpu(gpu) => gpu.blend(frames),
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
            CompositorType::Wgpu(gpu) => gpu.blend_with_dim(frames, dim),
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
        BlendMode::Overlay => {
            // Overlay: Multiply if base < 0.5, Screen if base >= 0.5
            if b_clamped < 0.5 {
                2.0 * b_clamped * t_clamped
            } else {
                1.0 - 2.0 * (1.0 - b_clamped) * (1.0 - t_clamped)
            }
        }
    }
}

/// CPU compositor - simple alpha blending on CPU.
#[derive(Clone, Debug)]
pub struct CpuCompositor;

impl CpuCompositor {
    /// Blend two F32 buffers with opacity (RGBA format)
    fn blend_f32(bottom: &[f32], top: &[f32], opacity: f32, mode: &BlendMode, result: &mut [f32]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        for i in (0..bottom.len()).step_by(4) {
            let top_alpha = top[i + 3] * opacity;
            let inv_alpha = 1.0 - top_alpha;

            result[i] = bottom[i] * inv_alpha + apply_blend(bottom[i], top[i], mode) * top_alpha;
            result[i + 1] = bottom[i + 1] * inv_alpha
                + apply_blend(bottom[i + 1], top[i + 1], mode) * top_alpha;
            result[i + 2] = bottom[i + 2] * inv_alpha
                + apply_blend(bottom[i + 2], top[i + 2], mode) * top_alpha;
            result[i + 3] = bottom[i + 3] * inv_alpha + top_alpha;
        }
    }

    /// Blend two F16 buffers with opacity — decodes to f32, delegates to blend_f32, encodes back.
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

        let n = bottom.len();
        let b_f32: Vec<f32> = bottom.iter().map(|v| v.to_f32()).collect();
        let t_f32: Vec<f32> = top.iter().map(|v| v.to_f32()).collect();
        let mut r_f32 = vec![0.0f32; n];
        Self::blend_f32(&b_f32, &t_f32, opacity, mode, &mut r_f32);
        for (i, &v) in r_f32.iter().enumerate() {
            result[i] = f16::from_f32(v);
        }
    }

    /// Blend two U8 buffers with opacity — decodes to f32, delegates to blend_f32, encodes back.
    fn blend_u8(bottom: &[u8], top: &[u8], opacity: f32, mode: &BlendMode, result: &mut [u8]) {
        debug_assert_eq!(bottom.len(), top.len());
        debug_assert_eq!(bottom.len(), result.len());

        let n = bottom.len();
        let b_f32: Vec<f32> = bottom.iter().map(|&v| v as f32 / 255.0).collect();
        let t_f32: Vec<f32> = top.iter().map(|&v| v as f32 / 255.0).collect();
        let mut r_f32 = vec![0.0f32; n];
        Self::blend_f32(&b_f32, &t_f32, opacity, mode, &mut r_f32);
        for (i, &v) in r_f32.iter().enumerate() {
            result[i] = (v.clamp(0.0, 1.0) * 255.0) as u8;
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
        let base_cropped =
            base_frame.crop_copy(width, height, crate::entities::frame::CropAlign::LeftTop);

        let canvas_pixels = width * height * 4;

        // Two-buffer ping-pong: curr holds the accumulator, out is the write target.
        // After each blend we swap them; no allocation inside the loop.
        enum Buf {
            F32(Vec<f32>),
            F16(Vec<half::f16>),
            U8(Vec<u8>),
        }

        // Extract pixel data from cropped base — zero-copy when sole owner,
        // falls back to clone only if Arc is shared (e.g. same-size no-op crop)
        let mut curr = match base_cropped.into_pixel_buffer() {
            PixelBuffer::F32(v) => Buf::F32(v),
            PixelBuffer::F16(v) => Buf::F16(v),
            PixelBuffer::U8(v) => Buf::U8(v),
        };
        let mut out = match &curr {
            Buf::F32(_) => Buf::F32(vec![0.0f32; canvas_pixels]),
            Buf::F16(_) => Buf::F16(vec![half::f16::ZERO; canvas_pixels]),
            Buf::U8(_) => Buf::U8(vec![0u8; canvas_pixels]),
        };

        // Blend each subsequent layer on top
        // Note: _transform is ignored - CPU path applies transform beforehand
        // in compose_internal via transform::transform_frame()
        // GPU path would use this matrix in shader instead
        for (layer_frame, opacity, mode, _transform) in iter {
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

            // Copy curr into out (no allocation), blend the overlap region into out,
            // then swap: curr holds the accumulated result for the next iteration.
            match (&mut curr, &*layer_buffer, &mut out) {
                (Buf::F32(c), PixelBuffer::F32(layer), Buf::F32(o)) => {
                    o.copy_from_slice(c);
                    blend_rows!(blend_f32, c, layer, o);
                    std::mem::swap(c, o);
                }
                (Buf::F16(c), PixelBuffer::F16(layer), Buf::F16(o)) => {
                    o.copy_from_slice(c);
                    blend_rows!(blend_f16, c, layer, o);
                    std::mem::swap(c, o);
                }
                (Buf::U8(c), PixelBuffer::U8(layer), Buf::U8(o)) => {
                    o.copy_from_slice(c);
                    blend_rows!(blend_u8, c, layer, o);
                    std::mem::swap(c, o);
                }
                _ => {
                    log::warn!("Pixel format mismatch during compositing, skipping layer");
                }
            }
        }

        // Wrap the final accumulated buffer in a Frame exactly once
        let result = match curr {
            Buf::F32(v) => Frame::from_f32_buffer_with_status(v, width, height, min_status),
            Buf::F16(v) => Frame::from_f16_buffer_with_status(v, width, height, min_status),
            Buf::U8(v) => Frame::from_u8_buffer_with_status(v, width, height, min_status),
        };

        Some(result)
    }
}
