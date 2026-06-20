//! Multi-layer GPU blend for the viewport compositing path (`CompositorType`).
//!
//! Mirrors the semantics of the old OpenGL-backed compositor — same WGSL formulas,
//! sequential bottom→top merges, RGBA8 / RGBA16Float / RGBA32Float canvases.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::mpsc;

use super::effects::EffectsRunner;
use crate::entities::compositor::{BlendMode, CpuCompositor, IDENTITY_MAT4, LayerPayload};
use crate::entities::frame::{CropAlign, Frame, FrameStatus, PixelBuffer, PixelFormat};
use log::warn;
use wgpu::util::DeviceExt;

const BLEND_SHADER: &str = include_str!("shaders/layer_blend.wgsl");
const ROW_ALIGN: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

/// Layout matches `Uniforms` in `shaders/layer_blend.wgsl` exactly.
/// Field order + padding chosen so every vec/mat is 16-byte aligned
/// for std140 / wgsl uniform rules without explicit pad fields.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendUniforms {
    opacity: f32,                       // offset 0
    blend_mode: i32,                    // offset 4
    canvas_size: [f32; 2],              // offset 8  (vec2 — 8-aligned)
    top_size: [f32; 2],                 // offset 16
    use_camera: u32,                    // offset 24
    layer_z: f32,                       // offset 28
    col0: [f32; 4],                     // offset 32 (vec4 — 16-aligned)
    col1: [f32; 4],                     // offset 48
    col2: [f32; 4],                     // offset 64
    camera_vp_inv: [[f32; 4]; 4],       // offset 80 (mat4 — 16-aligned)
    layer_inv: [[f32; 4]; 4],           // offset 144
    // total: 208 bytes
}

/// wgpu-backed layer stack blender (runs on the `eframe` render thread).
pub struct WgpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    sampler: wgpu::Sampler,
    quad_vbo: wgpu::Buffer,
    blend_bind_group_layout: wgpu::BindGroupLayout,
    blend_pipeline_layout: wgpu::PipelineLayout,
    blend_shader: wgpu::ShaderModule,
    blend_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    uniform_buf: wgpu::Buffer,
    /// Per-layer GPU effect chain runner. Built once per compositor;
    /// `apply_chain` invoked between layer upload and blend pass when
    /// the layer has any [`LayerPayload::effects`].
    effects: EffectsRunner,
}

impl std::fmt::Debug for WgpuCompositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuCompositor")
            .field("pipelines_cached", &self.blend_pipelines.len())
            .finish()
    }
}

impl WgpuCompositor {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("playa_compositor_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            // wgpu 29: `mipmap_filter` now takes the dedicated `MipmapFilterMode` enum.
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let quad: [[f32; 4]; 6] = [
            [-1.0, -1.0, 0.0, 0.0],
            [1.0, -1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0, 1.0],
            [-1.0, -1.0, 0.0, 0.0],
            [1.0, 1.0, 1.0, 1.0],
            [-1.0, 1.0, 0.0, 1.0],
        ];

        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("playa_compositor_quad_vbo"),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let blend_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("playa_blend_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("playa_layer_blend"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(BLEND_SHADER)),
        });

        let blend_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("playa_blend_pipeline_layout"),
            // wgpu 29: each bind group layout entry is now Option<&BindGroupLayout>.
            bind_group_layouts: &[Some(&blend_bind_group_layout)],
            // wgpu 28+: push constant ranges replaced by `immediate_size` (byte count; 0 = none).
            immediate_size: 0,
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("playa_blend_uniform"),
            size: std::mem::size_of::<BlendUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            device: device.clone(),
            queue: queue.clone(),
            sampler,
            quad_vbo,
            blend_bind_group_layout,
            blend_pipeline_layout,
            blend_shader,
            blend_pipelines: HashMap::new(),
            uniform_buf,
            effects: EffectsRunner::new(device, queue),
        }
    }

    fn texture_format(pix: PixelFormat) -> Result<wgpu::TextureFormat, String> {
        Ok(match pix {
            PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8Unorm,
            PixelFormat::RgbaF16 => wgpu::TextureFormat::Rgba16Float,
            PixelFormat::RgbaF32 => wgpu::TextureFormat::Rgba32Float,
        })
    }

    fn blend_mode_idx(mode: &BlendMode) -> i32 {
        match mode {
            BlendMode::Normal => 0,
            BlendMode::Screen => 1,
            BlendMode::Add => 2,
            BlendMode::Subtract => 3,
            BlendMode::Multiply => 4,
            BlendMode::Divide => 5,
            BlendMode::Difference => 6,
            BlendMode::Overlay => 7,
        }
    }

    fn build_uniforms(layer: &LayerPayload, canvas_w: usize, canvas_h: usize) -> BlendUniforms {
        let m = &layer.inv_matrix;
        let (use_camera, layer_z, camera_vp_inv, layer_inv) = match &layer.camera_path {
            Some(c) => (1u32, c.layer_z, c.camera_vp_inv, c.layer_inv),
            None => (0u32, 0.0, IDENTITY_MAT4, IDENTITY_MAT4),
        };
        BlendUniforms {
            opacity: layer.opacity,
            blend_mode: Self::blend_mode_idx(&layer.blend_mode),
            canvas_size: [canvas_w as f32, canvas_h as f32],
            top_size: [layer.frame.width() as f32, layer.frame.height() as f32],
            use_camera,
            layer_z,
            col0: [m[0], m[1], m[2], 0.0],
            col1: [m[3], m[4], m[5], 0.0],
            col2: [m[6], m[7], m[8], 0.0],
            camera_vp_inv,
            layer_inv,
        }
    }

    fn pipeline_for_fmt(&mut self, fmt: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        self.blend_pipelines
            .entry(fmt)
            .or_insert_with(|| {
                Self::mk_blend_pipeline(
                    &self.device,
                    &self.blend_shader,
                    &self.blend_pipeline_layout,
                    fmt,
                )
            })
            .clone()
    }

    fn mk_blend_pipeline(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        fmt: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("playa_blend_pipeline"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_blend"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_blend"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: fmt,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            // wgpu 29: `multiview` renamed to `multiview_mask`.
            multiview_mask: None,
            cache: None,
        })
    }

    fn mk_tex(
        device: &wgpu::Device,
        w: u32,
        h: u32,
        fmt: wgpu::TextureFormat,
        label: &str,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: fmt,
            usage,
            view_formats: &[],
        })
    }

    fn pixel_row_bytes(width_px: usize, pix_fmt: PixelFormat, tx_fmt: wgpu::TextureFormat) -> usize {
        let ch = Self::channel_scalar_bytes(pix_fmt, tx_fmt);
        ch.saturating_mul(4).saturating_mul(width_px)
    }

    fn channel_scalar_bytes(pix_fmt: PixelFormat, tx_fmt: wgpu::TextureFormat) -> usize {
        match (pix_fmt, tx_fmt) {
            (PixelFormat::Rgba8, wgpu::TextureFormat::Rgba8Unorm) => 1,
            (PixelFormat::RgbaF16, wgpu::TextureFormat::Rgba16Float) => 2,
            (PixelFormat::RgbaF32, wgpu::TextureFormat::Rgba32Float) => 4,
            _ => 0,
        }
    }

    fn upload_frame(&self, frame: &Frame, tf: wgpu::TextureFormat) -> Result<wgpu::Texture, String> {
        let w_u32 = frame.width() as u32;
        let h_u32 = frame.height() as u32;

        let usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT;

        let texture = Self::mk_tex(&self.device, w_u32, h_u32, tf, "playa_blend_upload", usage);

        let buffer = frame.buffer();
        let pix = buffer.as_ref();
        let row = Self::pixel_row_bytes(frame.width(), frame.pixel_format(), tf);
        if row == 0 {
            return Err("unsupported pixel/format combination".into());
        }

        let data: &[u8] = match pix {
            PixelBuffer::U8(d) => bytemuck::cast_slice(d),
            PixelBuffer::F16(d) => bytemuck::cast_slice(d),
            PixelBuffer::F32(d) => bytemuck::cast_slice(d),
        };

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row as u32),
                rows_per_image: Some(h_u32),
            },
            wgpu::Extent3d {
                width: w_u32,
                height: h_u32,
                depth_or_array_layers: 1,
            },
        );

        Ok(texture)
    }

    fn blend_bind_group(&self, bottom: &wgpu::TextureView, top: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("playa_blend_bg"),
            layout: &self.blend_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(bottom),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(top),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    fn blend_pass(
        &self,
        bottom: &wgpu::Texture,
        top: &wgpu::Texture,
        uniforms: BlendUniforms,
        width: u32,
        height: u32,
        tf: wgpu::TextureFormat,
        pipeline: &wgpu::RenderPipeline,
        label: &str,
    ) -> wgpu::Texture {
        let out = Self::mk_tex(
            &self.device,
            width,
            height,
            tf,
            label,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let out_view = out.create_view(&wgpu::TextureViewDescriptor::default());

        let b_view = bottom.create_view(&wgpu::TextureViewDescriptor::default());
        let t_view = top.create_view(&wgpu::TextureViewDescriptor::default());

        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniforms));
        let bg = self.blend_bind_group(&b_view, &t_view);

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("playa_blend_enc"),
            });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("playa_blend_rp"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                // wgpu 29: render passes require an explicit multiview mask.
                multiview_mask: None,
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, &bg, &[]);
            rp.set_vertex_buffer(0, self.quad_vbo.slice(..));
            rp.draw(0..6, 0..1);
        }

        self.queue.submit(std::iter::once(enc.finish()));

        out
    }

    fn padded_row_bytes(width_px: usize, frame_fmt: PixelFormat) -> Result<usize, String> {
        let wf = Self::texture_format(frame_fmt)?;
        let unpadded = Self::pixel_row_bytes(width_px, frame_fmt, wf);
        Ok((unpadded + ROW_ALIGN - 1) & !(ROW_ALIGN - 1))
    }

    fn readback_to_frame(
        &self,
        texture: &wgpu::Texture,
        wf: wgpu::TextureFormat,
        width_px: usize,
        height_px: usize,
        frame_fmt: PixelFormat,
        status: FrameStatus,
    ) -> Result<Frame, String> {
        if wf != Self::texture_format(frame_fmt)? {
            return Err("readback pixel format mismatch".into());
        }

        let unpadded_row = Self::pixel_row_bytes(width_px, frame_fmt, wf);
        let padded_row = Self::padded_row_bytes(width_px, frame_fmt)?;
        let staging_size = padded_row
            .checked_mul(height_px)
            .ok_or("staging size overflow")?;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("playa_readback_staging"),
            size: staging_size as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("playa_readback_enc"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row as u32),
                    rows_per_image: Some(height_px as u32),
                },
            },
            wgpu::Extent3d {
                width: width_px as u32,
                height: height_px as u32,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(enc.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = mpsc::channel::<Result<(), wgpu::BufferAsyncError>>();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .map_err(|e| format!("map channel: {e}"))?
            .map_err(|e| format!("map_async: {e}"))?;

        let out = {
            let view = slice.get_mapped_range();
            Self::unpack_padded_pixels(
                &view,
                padded_row,
                unpadded_row,
                height_px,
                width_px,
                frame_fmt,
                status,
            )
        };

        staging.unmap();
        out
    }

    fn unpack_padded_pixels(
        raw: &[u8],
        padded_row: usize,
        unpadded_row: usize,
        height_px: usize,
        width_px: usize,
        frame_fmt: PixelFormat,
        status: FrameStatus,
    ) -> Result<Frame, String> {
        match frame_fmt {
            PixelFormat::Rgba8 => {
                let mut out = vec![0u8; unpadded_row * height_px];
                for row in 0..height_px {
                    let s = row * padded_row;
                    let d = row * unpadded_row;
                    out[d..d + unpadded_row].copy_from_slice(&raw[s..s + unpadded_row]);
                }
                Ok(Frame::from_u8_buffer_with_status(out, width_px, height_px, status))
            }
            PixelFormat::RgbaF16 => {
                let mut flat: Vec<half::f16> = Vec::with_capacity(width_px * height_px * 4);
                for row in 0..height_px {
                    let s = row * padded_row;
                    let row_u16: &[u16] = bytemuck::cast_slice(&raw[s..s + unpadded_row]);
                    flat.extend(row_u16.iter().copied().map(half::f16::from_bits));
                }
                Ok(Frame::from_f16_buffer_with_status(
                    flat, width_px, height_px, status,
                ))
            }
            PixelFormat::RgbaF32 => {
                let mut flat: Vec<f32> = Vec::with_capacity(width_px * height_px * 4);
                let nbytes = unpadded_row;
                for row in 0..height_px {
                    let s = row * padded_row;
                    flat.extend_from_slice(bytemuck::cast_slice(&raw[s..s + nbytes]));
                }
                Ok(Frame::from_f32_buffer_with_status(
                    flat, width_px, height_px, status,
                ))
            }
        }
    }

    pub(crate) fn blend(&mut self, layers: Vec<LayerPayload>) -> Option<Frame> {
        match self.blend_inner(layers.clone()) {
            Ok(r) => Some(r),
            Err(e) => {
                warn!("WGPU compositor failed: {}, falling back to CPU", e);
                CpuCompositor.blend(layers)
            }
        }
    }

    pub(crate) fn blend_with_dim(
        &mut self,
        layers: Vec<LayerPayload>,
        dim: (usize, usize),
    ) -> Option<Frame> {
        let out = self.blend(layers)?;
        out.crop(dim.0, dim.1, CropAlign::LeftTop);
        Some(out)
    }

    fn blend_inner(&mut self, layers: Vec<LayerPayload>) -> Result<Frame, String> {
        if layers.is_empty() {
            return Err("no layers".into());
        }

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

        let width = layers[0].frame.width();
        let height = layers[0].frame.height();
        let pf = layers[0].frame.pixel_format();
        let wf = Self::texture_format(pf)?;

        let pipeline = self.pipeline_for_fmt(wf);
        let w_u32 = width as u32;
        let h_u32 = height as u32;

        // Upload each layer's raw frame to its own texture, then run
        // the layer's GPU effect chain (Phase E) before that layer
        // participates in the blend pass.
        let uploads: Vec<wgpu::Texture> = layers
            .iter()
            .map(|l| {
                let tex = self.upload_frame(&l.frame, wf)?;
                Ok(self.effects.apply_chain(tex, &l.effects, wf))
            })
            .collect::<Result<_, String>>()?;

        let mut uploads = uploads.into_iter();
        let mut acc = uploads.next().ok_or_else(|| "no textures".to_string())?;

        // Phase B-camera: shader handles both 2D (3×3 inv_matrix) and
        // camera-projected (camera_vp_inv + layer_inv + layer_z) paths,
        // selected per-layer via use_camera flag in build_uniforms.
        // Phase D consumes z_position. Phase E consumes mask.
        for (layer, top_tex) in layers.iter().skip(1).zip(uploads) {
            let u = Self::build_uniforms(layer, width, height);
            let out =
                self.blend_pass(&acc, &top_tex, u, w_u32, h_u32, wf, &pipeline, "playa_blend_step");
            acc = out;
        }

        let result = self.readback_to_frame(&acc, wf, width, height, pf, min_status)?;
        Ok(result)
    }
}
