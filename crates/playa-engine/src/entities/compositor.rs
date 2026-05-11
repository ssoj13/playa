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

use crate::entities::frame::{Frame, FrameStatus, PixelBuffer, PixelFormat};
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

/// 4×4 identity, column-major. Used as the placeholder camera VP /
/// layer-inv when [`LayerPayload::camera_path`] is `None` (the GPU
/// shader sees these but the `use_camera` flag short-circuits the
/// camera path before it touches them).
pub const IDENTITY_MAT4: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Channel of a mask source to use for a track matte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskChannel {
    Red,
    Green,
    Blue,
    Alpha,
    Luminance,
}

/// Track matte information attached to a layer.
///
/// Phase A: always `None` — track mattes are still pre-multiplied into
/// the layer alpha by `apply_track_matte` before the layer reaches the
/// compositor. Phase C will populate this so CPU/GPU compositors can
/// sample the mask in their resample-blend pass instead.
#[derive(Clone, Debug)]
pub struct MaskInfo {
    pub frame: Frame,
    pub channel: MaskChannel,
}

/// Per-layer camera-projection bundle used by the GPU shader's
/// camera-path branch (Phase B-camera).
///
/// Carries everything the shader needs to ray-march from a canvas
/// pixel to a src pixel via the camera's inverse view-projection
/// and the layer's inverse model matrix.
///
/// `None` on `LayerPayload` means the comp has no active camera and
/// the 2D path (`inv_matrix` 3×3) is used instead.
#[derive(Clone, Copy, Debug)]
pub struct CameraPathInfo {
    /// Inverse of the camera's `view * projection` matrix.
    /// Column-major. Maps clip-space `[ndc.x, ndc.y, ndc.z, 1]` → world.
    pub camera_vp_inv: [[f32; 4]; 4],
    /// Inverse of the layer's model matrix (4×4, column-major).
    /// Maps world → layer-local object space.
    /// For non-tilted (no X/Y rot) layers this collapses cleanly when
    /// applied to the world point produced by the ray-plane intersection
    /// at `layer_z`.
    pub layer_inv: [[f32; 4]; 4],
    /// World-space Z position of the layer's plane. Used by the shader's
    /// ray-plane intersection to find where the camera ray for a given
    /// canvas pixel hits the layer.
    pub layer_z: f32,
}

/// One layer's full payload to the compositor.
///
/// Both [`CpuCompositor`] and [`WgpuCompositor`] consume `Vec<LayerPayload>`.
/// The struct is the **single source of truth** for what data crosses
/// the compositor boundary — no more 4-tuple drift between
/// `comp_node`, `gpu_blend_bridge`, and the two backends.
///
/// During the GPU-first unification (see
/// `.bughunt/gpu_compositor_unification.md`):
/// - **Phase A** (this commit): all fields exist, but only `frame`,
///   `opacity`, `blend_mode`, `inv_matrix` are populated by
///   `compose_internal`. Behavior is byte-identical to the previous
///   tuple-based API.
/// - **Phase B**: `inv_matrix` becomes the canvas-to-src 3×3 for
///   layers GPU can resample inline (skips CPU pre-render).
///   `camera_vp_inv` populated for camera-projected layers.
/// - **Phase C**: CPU compositor becomes matrix-aware; pre-render
///   stops on CPU path too.
/// - **Phase D**: `z_position` consumed by GPU depth buffer + OIT.
/// - **Phase E**: `mask` populated; compositors apply it inline.
#[derive(Clone, Debug)]
pub struct LayerPayload {
    /// Layer pixel data — raw source after Phase B; pre-rendered
    /// canvas-sized in Phase A.
    pub frame: Frame,
    /// Layer opacity multiplier `[0.0, 1.0]`.
    pub opacity: f32,
    /// Blend mode against the accumulator below.
    pub blend_mode: BlendMode,
    /// Inverse 3×3 column-major matrix mapping canvas-buffer pixels
    /// (top-left, Y-down) to src-buffer pixels (top-left, Y-down).
    /// `IDENTITY_TRANSFORM` when the frame is already pre-rendered
    /// at canvas size.
    pub inv_matrix: [f32; 9],
    /// Camera-projection bundle for layers rendered through an active
    /// camera (perspective or ortho). `None` when the comp has no
    /// camera — the 2D path uses `inv_matrix` only. See [`CameraPathInfo`].
    pub camera_path: Option<CameraPathInfo>,
    /// Layer Z position for depth-buffer / OIT (Phase D). 0.0 = comp
    /// plane; positive = closer to camera.
    pub z_position: f32,
    /// Track matte mask. Phase A: always `None` (pre-applied to alpha).
    /// Phase E: populated; compositors sample inline.
    pub mask: Option<MaskInfo>,
    /// True when layer has X or Y rotation that makes its plane
    /// non-orthogonal to the comp Z axis. Tilted layers need
    /// ray-plane intersection for accurate sampling — handled by CPU
    /// pre-render (kept indefinitely as the small-fraction edge
    /// case).
    pub layer_is_tilted: bool,
}

impl LayerPayload {
    /// Construct a payload for a pre-rendered canvas-sized layer with
    /// no transform, camera, or mask metadata. The shape used by
    /// Phase A `compose_internal` after `transform_frame_with_camera`.
    pub fn pre_rendered(frame: Frame, opacity: f32, blend_mode: BlendMode) -> Self {
        Self {
            frame,
            opacity,
            blend_mode,
            inv_matrix: IDENTITY_TRANSFORM,
            camera_path: None,
            z_position: 0.0,
            mask: None,
            layer_is_tilted: false,
        }
    }
}

impl CompositorType {
    /// Blend layers using the selected compositor backend.
    /// Each layer carries its own transform / blend / opacity — see
    /// [`LayerPayload`].
    pub fn blend(&mut self, layers: Vec<LayerPayload>) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend(layers),
            CompositorType::Wgpu(gpu) => gpu.blend(layers),
        }
    }

    /// Blend layers into a canvas with explicit dimensions.
    pub fn blend_with_dim(
        &mut self,
        layers: Vec<LayerPayload>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        match self {
            CompositorType::Cpu(cpu) => cpu.blend_with_dim(layers, dim),
            CompositorType::Wgpu(gpu) => gpu.blend_with_dim(layers, dim),
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

    /// Blend layers bottom-to-top with opacity.
    ///
    /// **Note:** [`LayerPayload::inv_matrix`], `camera_vp_inv`, `z_position`,
    /// and `mask` are **ignored** by CPU compositor in Phase A.
    /// For CPU path, transforms are applied beforehand via
    /// `transform::transform_frame_with_camera()` in `compose_internal`,
    /// and track mattes are pre-multiplied into alpha. Phase C will
    /// rewrite this to a matrix-aware single-pass resample-blend.
    pub(crate) fn blend(&self, layers: Vec<LayerPayload>) -> Option<Frame> {
        // Default to using first frame size
        if let Some(first) = layers.first() {
            let dim = (first.frame.width(), first.frame.height());
            return self.blend_with_dim(layers, dim);
        }
        None
    }

    /// Blend layers onto a fixed-size canvas (width, height).
    ///
    /// Dispatches to one of two internal implementations:
    ///
    /// - **Legacy pre-rendered path**: when every layer has identity
    ///   `inv_matrix` AND no `camera_path` (i.e. all layers are
    ///   pre-rendered canvas-sized buffers — what comp_node still
    ///   produces for the CPU backend until `gpu_inline` is dropped).
    ///   Two-buffer ping-pong, format-specific blend macros. Fast.
    ///
    /// - **Matrix-aware path** (Phase C): when any layer has a
    ///   non-identity `inv_matrix` or a `camera_path`. Per-pixel
    ///   resample + blend in F32 accumulator; converts to output
    ///   format at end. Mirrors the wgpu shader's `canvas_to_src`
    ///   chain so CPU and GPU produce equivalent output (modulo
    ///   bilinear-sample rounding tolerance).
    pub(crate) fn blend_with_dim(
        &self,
        layers: Vec<LayerPayload>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        if layers.is_empty() {
            return None;
        }
        let needs_resample = layers
            .iter()
            .any(|l| l.inv_matrix != IDENTITY_TRANSFORM || l.camera_path.is_some());
        if needs_resample {
            Self::blend_matrix_aware(layers, dim)
        } else {
            Self::blend_legacy_pre_rendered(layers, dim)
        }
    }

    /// Pre-rendered fast path: all layers come in canvas-sized with
    /// identity matrices. Two-buffer ping-pong, format-specific.
    fn blend_legacy_pre_rendered(
        layers: Vec<LayerPayload>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        use log::trace;
        trace!(
            "CpuCompositor::blend_legacy_pre_rendered() called with {} layers into {}x{}",
            layers.len(),
            dim.0,
            dim.1
        );

        // Calculate minimum status from all input frames
        // Composition is only as good as its worst component
        let min_status = layers
            .iter()
            .map(|l| l.frame.status())
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
        let mut iter = layers.iter();
        let base = iter.next().unwrap(); // safe: layers non-empty
        let base_cropped = base
            .frame
            .crop_copy(width, height, crate::entities::frame::CropAlign::LeftTop);

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

        // Blend each subsequent layer on top.
        // Phase A: inv_matrix / camera_vp_inv / mask / z_position / layer_is_tilted
        // are ignored — the CPU path consumes pre-rendered canvas-sized
        // frames with track mattes already pre-applied. Phase C
        // rewrites this loop as a matrix-aware single-pass resampler.
        for layer in iter {
            let layer_frame = &layer.frame;
            let opacity = layer.opacity;
            let mode = &layer.blend_mode;
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
                        Self::$blend_fn(base_slice, layer_slice, opacity, mode, out_slice);
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

    // -----------------------------------------------------------------
    // Phase C — matrix-aware single-pass resample-blend
    // -----------------------------------------------------------------

    /// Multiply a column-major mat4 by a vec4: `out = M * v`.
    ///
    /// `m[col][row]` matches the layout produced by `glam::Mat4::to_cols_array_2d`.
    #[inline]
    fn mat4_mul_vec4(m: &[[f32; 4]; 4], v: [f32; 4]) -> [f32; 4] {
        [
            m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2] + m[3][0] * v[3],
            m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2] + m[3][1] * v[3],
            m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2] + m[3][2] * v[3],
            m[0][3] * v[0] + m[1][3] * v[1] + m[2][3] * v[2] + m[3][3] * v[3],
        ]
    }

    /// Mirror of `canvas_to_src` in `layer_blend.wgsl`: from a canvas
    /// pixel (top-left, Y-down), compute the src buffer pixel that the
    /// layer should be sampled at — via 2D inverse matrix OR camera
    /// ray-plane unproject.
    ///
    /// Returns `(-1, -1)` (out of bounds, will short-circuit to
    /// transparent in the sampler) when the camera ray is parallel
    /// to the layer plane.
    #[inline]
    fn canvas_to_src_cpu(
        cx: f32,
        cy: f32,
        layer: &LayerPayload,
        canvas_w: f32,
        canvas_h: f32,
    ) -> (f32, f32) {
        if let Some(cam) = &layer.camera_path {
            // Camera path: unproject NDC → world ray → intersect z=layer_z.
            let ndc_x = cx / canvas_w * 2.0 - 1.0;
            let ndc_y = 1.0 - cy / canvas_h * 2.0;
            let near4 = Self::mat4_mul_vec4(&cam.camera_vp_inv, [ndc_x, ndc_y, -1.0, 1.0]);
            let far4 = Self::mat4_mul_vec4(&cam.camera_vp_inv, [ndc_x, ndc_y, 1.0, 1.0]);
            let p_near = [near4[0] / near4[3], near4[1] / near4[3], near4[2] / near4[3]];
            let p_far = [far4[0] / far4[3], far4[1] / far4[3], far4[2] / far4[3]];
            let dir = [p_far[0] - p_near[0], p_far[1] - p_near[1], p_far[2] - p_near[2]];
            let denom = dir[2];
            if denom.abs() < 1.0e-6 {
                return (-1.0, -1.0); // edge-on, treat as out-of-bounds
            }
            let t = (cam.layer_z - p_near[2]) / denom;
            let world = [
                p_near[0] + dir[0] * t,
                p_near[1] + dir[1] * t,
                p_near[2] + dir[2] * t,
            ];
            let obj = Self::mat4_mul_vec4(&cam.layer_inv, [world[0], world[1], world[2], 1.0]);
            // object (center, Y-up) → src buffer pixel (top-left, Y-down)
            let layer_w = layer.frame.width() as f32;
            let layer_h = layer.frame.height() as f32;
            (obj[0] + layer_w * 0.5, layer_h * 0.5 - obj[1])
        } else {
            // 2D path: column-major 3×3 inverse transform.
            let m = &layer.inv_matrix;
            let sx = m[0] * cx + m[3] * cy + m[6];
            let sy = m[1] * cx + m[4] * cy + m[7];
            (sx, sy)
        }
    }

    /// Sample the layer's raw buffer at a sub-pixel location, decoding
    /// to f32 RGBA regardless of the underlying format. Out-of-bounds
    /// returns transparent black.
    #[inline]
    fn sample_layer(layer: &LayerPayload, sx: f32, sy: f32) -> [f32; 4] {
        let w = layer.frame.width();
        let h = layer.frame.height();
        let buffer = layer.frame.buffer();
        match &*buffer {
            PixelBuffer::F32(b) => crate::entities::transform::sample_bilinear(
                b, w, h, sx, sy, |v| v,
            ),
            PixelBuffer::F16(b) => crate::entities::transform::sample_bilinear(
                b, w, h, sx, sy, |v| v.to_f32(),
            ),
            PixelBuffer::U8(b) => crate::entities::transform::sample_bilinear(
                b, w, h, sx, sy, |v| v as f32 / 255.0,
            ),
        }
    }

    /// Matrix-aware single-pass resample-blend.
    ///
    /// Iterates layers bottom-to-top. For each canvas pixel, computes
    /// the src pixel via the layer's transform (2D inverse matrix or
    /// camera ray-plane unproject), bilinear-samples, and blends into
    /// an F32 accumulator using the layer's blend mode + opacity.
    /// At the end, converts the accumulator to the first-layer's
    /// pixel format.
    ///
    /// First layer is treated as REPLACE (mirrors the legacy path
    /// where source_frames[0] is the canvas-sized base — comp_node
    /// always inserts a black canvas there).
    fn blend_matrix_aware(layers: Vec<LayerPayload>, dim: (usize, usize)) -> Option<Frame> {
        use log::trace;
        use rayon::prelude::*;
        trace!(
            "CpuCompositor::blend_matrix_aware() called with {} layers into {}x{}",
            layers.len(),
            dim.0,
            dim.1
        );

        let min_status = layers
            .iter()
            .map(|l| l.frame.status())
            .min_by_key(|s| match s {
                FrameStatus::Error => 0,
                FrameStatus::Placeholder => 1,
                FrameStatus::Header => 2,
                FrameStatus::Loading | FrameStatus::Composing | FrameStatus::Expired => 3,
                FrameStatus::Loaded => 4,
            })
            .unwrap_or(FrameStatus::Placeholder);

        let (width, height) = dim;
        let canvas_w = width as f32;
        let canvas_h = height as f32;
        let canvas_pixels = width * height * 4;
        let mut acc = vec![0.0f32; canvas_pixels];

        // Output format = first layer's format (matches comp_node's
        // promote_frame which has already promoted all layers to the
        // target format).
        let target_format = layers[0].frame.pixel_format();

        for (idx, layer) in layers.iter().enumerate() {
            let opacity = layer.opacity;
            let mode = &layer.blend_mode;

            // Per-row parallelization. sample_layer reads only from
            // layer.frame's buffer (immutable), accumulator row is
            // exclusive to each task.
            acc.par_chunks_mut(width * 4)
                .enumerate()
                .for_each(|(y, row)| {
                    let cy = y as f32 + 0.5;
                    for x in 0..width {
                        let cx = x as f32 + 0.5;
                        let (sx, sy) =
                            Self::canvas_to_src_cpu(cx, cy, layer, canvas_w, canvas_h);
                        let sample = Self::sample_layer(layer, sx, sy);

                        let pix = &mut row[x * 4..x * 4 + 4];
                        if idx == 0 {
                            // First layer = base, replace.
                            pix[0] = sample[0];
                            pix[1] = sample[1];
                            pix[2] = sample[2];
                            pix[3] = sample[3];
                        } else {
                            // Blend over accumulator.
                            let top_alpha = sample[3] * opacity;
                            let inv_alpha = 1.0 - top_alpha;
                            let r_blended = apply_blend(pix[0], sample[0], mode);
                            let g_blended = apply_blend(pix[1], sample[1], mode);
                            let b_blended = apply_blend(pix[2], sample[2], mode);
                            pix[0] = pix[0] * inv_alpha + r_blended * top_alpha;
                            pix[1] = pix[1] * inv_alpha + g_blended * top_alpha;
                            pix[2] = pix[2] * inv_alpha + b_blended * top_alpha;
                            pix[3] = pix[3] * inv_alpha + top_alpha;
                        }
                    }
                });
        }

        // Convert F32 accumulator → output format.
        let result = match target_format {
            PixelFormat::Rgba8 => {
                let buf: Vec<u8> = acc
                    .iter()
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8)
                    .collect();
                Frame::from_u8_buffer_with_status(buf, width, height, min_status)
            }
            PixelFormat::RgbaF16 => {
                let buf: Vec<half::f16> =
                    acc.iter().map(|v| half::f16::from_f32(*v)).collect();
                Frame::from_f16_buffer_with_status(buf, width, height, min_status)
            }
            PixelFormat::RgbaF32 => {
                Frame::from_f32_buffer_with_status(acc, width, height, min_status)
            }
        };
        Some(result)
    }
}
