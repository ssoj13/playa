//! Viewport presenter — draws the current frame via wgpu inside egui’s paint pass.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui_wgpu::{CallbackResources, CallbackTrait, PaintCallbackInfo, ScreenDescriptor};
use log::{error, info, trace};
use playa_engine::entities::frame::{PixelBuffer, PixelFormat};
use wgpu::util::DeviceExt;

use super::ViewportRenderState;
use super::shaders::Shaders;

const WGSL_VIEWPORT: &str = include_str!("wgsl/viewport_image.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct VsUniforms {
    model: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FsUniforms {
    exposure: f32,
    gamma: f32,
    is_hdr: u32,
    tonemap_mode: u32,
}

/// CPU → GPU upload queued before the egui wgpu callback runs.
pub(super) enum StagedUpload {
    Rgba8 { pixels: Vec<u8>, w: u32, h: u32 },
    Rgba16F { half_bytes: Vec<u8>, w: u32, h: u32 },
    Rgba32F { pixels: Vec<u8>, w: u32, h: u32 },
}

/// wgpu resources for the docked / fullscreen viewport image.
pub struct ViewportRenderer {
    pub exposure: f32,
    pub gamma: f32,
    /// Swap-chain / egui output format (set each frame from `RenderState::target_format`).
    output_format: Option<wgpu::TextureFormat>,
    sampler: Option<wgpu::Sampler>,
    quad_vbo: Option<wgpu::Buffer>,
    vs_buf: Option<wgpu::Buffer>,
    fs_buf: Option<wgpu::Buffer>,
    bind_group: Option<wgpu::BindGroup>,
    bgl: Option<wgpu::BindGroupLayout>,
    pip_layout: Option<wgpu::PipelineLayout>,
    shader: Option<wgpu::ShaderModule>,
    pipelines: HashMap<(wgpu::TextureFormat, wgpu::TextureFormat), wgpu::RenderPipeline>,
    image_tex: Option<wgpu::Texture>,
    image_view: Option<wgpu::TextureView>,
    tex_w: u32,
    tex_h: u32,
    tex_format: PixelFormat,
    tonemap_mode: u32,
    current_shader_label: String,
    f16_scratch: Vec<u16>,
    last_error: Option<String>,
    staged: Option<StagedUpload>,
    queued_render_state: Option<ViewportRenderState>,
}

impl Default for ViewportRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewportRenderer {
    pub fn new() -> Self {
        let sm = Shaders::new();
        Self {
            exposure: 1.0,
            gamma: 2.2,
            output_format: None,
            sampler: None,
            quad_vbo: None,
            vs_buf: None,
            fs_buf: None,
            bind_group: None,
            bgl: None,
            pip_layout: None,
            shader: None,
            pipelines: HashMap::new(),
            image_tex: None,
            image_view: None,
            tex_w: 0,
            tex_h: 0,
            tex_format: PixelFormat::Rgba8,
            tonemap_mode: 0,
            current_shader_label: sm.current_shader.clone(),
            f16_scratch: Vec::new(),
            last_error: None,
            staged: None,
            queued_render_state: None,
        }
    }

    /// Must be called once per frame from the app host when using wgpu (before UI draws the viewport).
    pub fn set_output_format(&mut self, format: wgpu::TextureFormat) {
        self.output_format = Some(format);
    }

    pub fn shader_error(&self) -> Option<String> {
        self.last_error.clone()
    }

    pub fn update_shader(&mut self, shader_manager: &Shaders) {
        if self.current_shader_label == shader_manager.current_shader {
            return;
        }
        if shader_manager.shaders.contains_key(&shader_manager.current_shader) {
            info!("Viewport shader preset: {}", shader_manager.current_shader);
        } else {
            log::warn!(
                "Unknown shader `{}` — WGSL viewport supports embedded presets only; custom .glsl is ignored.",
                shader_manager.current_shader
            );
        }
        self.current_shader_label = shader_manager.current_shader.clone();
        self.tonemap_mode = match shader_manager.current_shader.as_str() {
            "tonemap_reinhard" => 1,
            "tonemap_aces" => 2,
            _ => 0,
        };
    }

    pub fn needs_texture_update(&self, width: usize, height: usize) -> bool {
        width as u32 != self.tex_w || height as u32 != self.tex_h || self.image_tex.is_none()
    }

    fn wgpu_tex_format(px: PixelFormat) -> wgpu::TextureFormat {
        match px {
            PixelFormat::Rgba8 => wgpu::TextureFormat::Rgba8UnormSrgb,
            PixelFormat::RgbaF16 => wgpu::TextureFormat::Rgba16Float,
            PixelFormat::RgbaF32 => wgpu::TextureFormat::Rgba32Float,
        }
    }

    pub fn stage_frame(
        &mut self,
        viewport_state: &ViewportRenderState,
        width: usize,
        height: usize,
        pixel_buffer: &PixelBuffer,
        pixel_format: PixelFormat,
    ) {
        self.queued_render_state = Some(*viewport_state);
        self.tex_format = pixel_format;

        let w = width as u32;
        let h = height as u32;
        self.staged = Some(match pixel_buffer {
            PixelBuffer::U8(data) => StagedUpload::Rgba8 {
                pixels: data.clone(),
                w,
                h,
            },
            PixelBuffer::F16(data) => {
                self.f16_scratch.clear();
                self.f16_scratch.extend(data.iter().map(|x| x.to_bits()));
                StagedUpload::Rgba16F {
                    half_bytes: bytemuck::cast_slice(&self.f16_scratch).to_vec(),
                    w,
                    h,
                }
            }
            PixelBuffer::F32(data) => StagedUpload::Rgba32F {
                pixels: bytemuck::cast_slice(data.as_slice()).to_vec(),
                w,
                h,
            },
        });
    }

    pub fn skip_upload_this_frame(&mut self, viewport_state: ViewportRenderState) {
        self.queued_render_state = Some(viewport_state);
        self.staged = None;
    }

    fn ensure_basics(&mut self, device: &wgpu::Device) -> Result<(), String> {
        if self.quad_vbo.is_none() {
            let quad: [[f32; 4]; 6] = [
                [-0.5, -0.5, 0.0, 1.0],
                [0.5, -0.5, 1.0, 1.0],
                [0.5, 0.5, 1.0, 0.0],
                [-0.5, -0.5, 0.0, 1.0],
                [0.5, 0.5, 1.0, 0.0],
                [-0.5, 0.5, 0.0, 0.0],
            ];
            self.quad_vbo = Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("viewport_quad_vbo"),
                contents: bytemuck::cast_slice(&quad),
                usage: wgpu::BufferUsages::VERTEX,
            }));
        }

        if self.vs_buf.is_none() {
            self.vs_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport_vs_uni"),
                size: std::mem::size_of::<VsUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.fs_buf.is_none() {
            self.fs_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("viewport_fs_uni"),
                size: std::mem::size_of::<FsUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        if self.sampler.is_none() {
            self.sampler = Some(device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("viewport_tex_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }));
        }

        if self.shader.is_none() {
            self.shader = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("viewport_image"),
                source: wgpu::ShaderSource::Wgsl(WGSL_VIEWPORT.into()),
            }));
            self.last_error = None;
        }

        if self.bgl.is_none() {
            let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("viewport_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
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
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
            self.pip_layout = Some(device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("viewport_pip_layout"),
                bind_group_layouts: &[&bgl],
                push_constant_ranges: &[],
            }));
            self.bgl = Some(bgl);
        }
        Ok(())
    }

    fn ensure_pipeline(
        &mut self,
        device: &wgpu::Device,
        target_fmt: wgpu::TextureFormat,
        src_fmt: PixelFormat,
    ) -> Result<(), String> {
        let img_native = Self::wgpu_tex_format(src_fmt);
        let key = (target_fmt, img_native);
        if self.pipelines.contains_key(&key) {
            return Ok(());
        }

        let shader = self.shader.as_ref().ok_or_else(|| "no shader".to_string())?;
        let pip_layout = self.pip_layout.as_ref().ok_or_else(|| "no layout".to_string())?;

        let pl = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport_present"),
            layout: Some(pip_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_fmt,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        trace!("viewport pipeline {:?} {:?}", target_fmt, img_native);
        self.pipelines.insert(key, pl);
        Ok(())
    }

    fn recreate_texture(
        &mut self,
        device: &wgpu::Device,
        w: u32,
        h: u32,
        native: wgpu::TextureFormat,
    ) {
        self.bind_group = None;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport_frame_tex"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: native,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.image_tex = Some(tex);
        self.image_view = Some(view);
        self.tex_w = w;
        self.tex_h = h;
    }

    fn rebuild_bind_group(&mut self, device: &wgpu::Device) {
        let bgl = self.bgl.as_ref().unwrap();
        let vs = self.vs_buf.as_ref().unwrap();
        let fs = self.fs_buf.as_ref().unwrap();
        let view = self.image_view.as_ref().unwrap();
        let samp = self.sampler.as_ref().unwrap();
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_bind_group"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: fs.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(samp),
                },
            ],
        }));
    }

    fn apply_staged(&mut self, queue: &wgpu::Queue, device: &wgpu::Device) {
        let Some(st) = self.staged.take() else {
            return;
        };
        let native = Self::wgpu_tex_format(self.tex_format);
        match st {
            StagedUpload::Rgba8 { pixels, w, h } => {
                if w != self.tex_w || h != self.tex_h || self.image_tex.is_none() {
                    self.recreate_texture(device, w, h, native);
                    self.rebuild_bind_group(device);
                }
                if let Some(ref tex) = self.image_tex {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(4 * w),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
            StagedUpload::Rgba16F { half_bytes, w, h } => {
                if w != self.tex_w || h != self.tex_h || self.image_tex.is_none() {
                    self.recreate_texture(device, w, h, native);
                    self.rebuild_bind_group(device);
                }
                if let Some(ref tex) = self.image_tex {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &half_bytes,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(8 * w),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
            StagedUpload::Rgba32F { pixels, w, h } => {
                if w != self.tex_w || h != self.tex_h || self.image_tex.is_none() {
                    self.recreate_texture(device, w, h, native);
                    self.rebuild_bind_group(device);
                }
                if let Some(ref tex) = self.image_tex {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(16 * w),
                            rows_per_image: None,
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
        }
    }

    fn write_uniforms(&mut self, queue: &wgpu::Queue) {
        let rs = match self.queued_render_state {
            Some(s) => s,
            None => return,
        };
        let vs = VsUniforms {
            model: rs.model_matrix,
            view: rs.view_matrix,
            proj: rs.projection_matrix,
        };
        let is_hdr = match self.tex_format {
            PixelFormat::Rgba8 => 0,
            PixelFormat::RgbaF16 | PixelFormat::RgbaF32 => 1,
        };
        let fs = FsUniforms {
            exposure: self.exposure,
            gamma: self.gamma,
            is_hdr,
            tonemap_mode: self.tonemap_mode,
        };
        if let Some(ref b) = self.vs_buf {
            queue.write_buffer(b, 0, bytemuck::bytes_of(&vs));
        }
        if let Some(ref b) = self.fs_buf {
            queue.write_buffer(b, 0, bytemuck::bytes_of(&fs));
        }
    }

    /// Drop GPU handles (call on shutdown).
    pub fn destroy(&mut self) {
        self.bind_group = None;
        self.pipelines.clear();
        self.image_view = None;
        self.image_tex = None;
        self.shader = None;
        self.pip_layout = None;
        self.bgl = None;
        self.sampler = None;
        self.vs_buf = None;
        self.fs_buf = None;
        self.quad_vbo = None;
        self.tex_w = 0;
        self.tex_h = 0;
        trace!("ViewportRenderer GPU resources destroyed");
    }
}

/// egui–wgpu bridge: uploads in `prepare`, draws in `paint`.
#[derive(Clone)]
pub struct ViewportPaintCallback {
    pub inner: Arc<Mutex<ViewportRenderer>>,
}

impl CallbackTrait for ViewportPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Ok(mut guard) = self.inner.lock() else {
            return Vec::new();
        };

        let Some(output_fmt) = guard.output_format else {
            guard.last_error =
                Some("ViewportRenderer: missing output_format (host must call set_output_format)".to_string());
            return Vec::new();
        };

        if let Err(e) = guard.ensure_basics(device) {
            guard.last_error = Some(e);
            return Vec::new();
        }

        if let Err(e) = guard.ensure_pipeline(device, output_fmt, guard.tex_format) {
            error!("{}", e);
            guard.last_error = Some(e);
            return Vec::new();
        }

        guard.apply_staged(queue, device);
        if guard.image_tex.is_some()
            && guard.bind_group.is_none()
            && guard.image_view.is_some()
        {
            guard.rebuild_bind_group(device);
        }
        guard.write_uniforms(queue);

        Vec::new()
    }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _res: &CallbackResources,
    ) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        let Some(output_fmt) = guard.output_format else {
            return;
        };
        let native = ViewportRenderer::wgpu_tex_format(guard.tex_format);
        let Some(pipeline) = guard.pipelines.get(&(output_fmt, native)) else {
            return;
        };
        let Some(ref bg) = guard.bind_group else {
            return;
        };
        let Some(ref vbo) = guard.quad_vbo else {
            return;
        };

        let clip_px = info.clip_rect_in_pixels();
        render_pass.set_scissor_rect(
            clip_px.left_px,
            clip_px.top_px,
            clip_px.width_px.max(1),
            clip_px.height_px.max(1),
        );

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bg, &[]);
        render_pass.set_vertex_buffer(0, vbo.slice(..));
        render_pass.draw(0..6, 0..1);
    }
}

impl Drop for ViewportRenderer {
    fn drop(&mut self) {
        if self.image_tex.is_some() {
            trace!("ViewportRenderer dropped with GPU resources still live (prefer destroy())");
        }
    }
}
