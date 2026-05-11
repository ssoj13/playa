//! Gaussian blur effect — GPU port of `entities::effects::blur`.
//!
//! Multi-pass: BlurRunner internally allocates an intermediate texture
//! between the horizontal and vertical passes. From `EffectsRunner`'s
//! perspective, blur looks like a single effect with a single
//! input→output transition; the ping-pong is hidden here.

use std::collections::HashMap;

const WGSL: &str = include_str!("../shaders/effects/blur.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    radius: f32,
    horizontal: u32, // 1 for X-pass, 0 for Y-pass
    _pad: [f32; 2],
}

/// Per-output-format render pipeline cache for the blur effect.
/// Used for both H and V passes (uniform flag selects axis).
pub struct BlurRunner {
    shader: Option<wgpu::ShaderModule>,
    bgl: Option<wgpu::BindGroupLayout>,
    pip_layout: Option<wgpu::PipelineLayout>,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// Separate uniform buffers per pass so we can encode both passes
    /// in a single command queue submit without overwriting each
    /// other's uniforms.
    uniform_buf_h: Option<wgpu::Buffer>,
    uniform_buf_v: Option<wgpu::Buffer>,
}

impl BlurRunner {
    pub fn new() -> Self {
        Self {
            shader: None,
            bgl: None,
            pip_layout: None,
            pipelines: HashMap::new(),
            uniform_buf_h: None,
            uniform_buf_v: None,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device) {
        if self.shader.is_none() {
            self.shader = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("playa_effect_blur"),
                source: wgpu::ShaderSource::Wgsl(WGSL.into()),
            }));
        }
        if self.bgl.is_none() {
            self.bgl = Some(device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("playa_effect_blur_bgl"),
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
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            }));
        }
        if self.pip_layout.is_none() {
            let bgl = self.bgl.as_ref().unwrap();
            self.pip_layout = Some(device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("playa_effect_blur_layout"),
                bind_group_layouts: &[bgl],
                push_constant_ranges: &[],
            }));
        }
        if self.uniform_buf_h.is_none() {
            self.uniform_buf_h = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("playa_effect_blur_uni_h"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if self.uniform_buf_v.is_none() {
            self.uniform_buf_v = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("playa_effect_blur_uni_v"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    fn pipeline_for(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> &wgpu::RenderPipeline {
        if !self.pipelines.contains_key(&format) {
            let shader = self.shader.as_ref().unwrap();
            let layout = self.pip_layout.as_ref().unwrap();
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("playa_effect_blur_pipeline"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[super::QUAD_VERTEX_LAYOUT],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
                multiview: None,
                cache: None,
            });
            self.pipelines.insert(format, pipeline);
        }
        self.pipelines.get(&format).unwrap()
    }

    fn alloc_intermediate(
        device: &wgpu::Device,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("playa_effect_blur_intermediate"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_pass(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::RenderPipeline,
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        quad_vbo: &wgpu::Buffer,
        uniform_buf: &wgpu::Buffer,
        input_view: &wgpu::TextureView,
        output_view: &wgpu::TextureView,
        label: &str,
    ) {
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some(label),
        });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
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
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, &bg, &[]);
            rp.set_vertex_buffer(0, quad_vbo.slice(..));
            rp.draw(0..6, 0..1);
        }
        queue.submit(std::iter::once(enc.finish()));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        sampler: &wgpu::Sampler,
        quad_vbo: &wgpu::Buffer,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        format: wgpu::TextureFormat,
        radius: f32,
    ) {
        // Zero or negative radius — pass-through copy (matches CPU
        // behavior of returning the input frame unchanged).
        if radius <= 0.0 {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("playa_effect_blur_pass_through"),
            });
            enc.copy_texture_to_texture(
                input.as_image_copy(),
                output.as_image_copy(),
                input.size(),
            );
            queue.submit(std::iter::once(enc.finish()));
            return;
        }

        self.ensure(device);
        let _ = self.pipeline_for(device, format);

        // Two uniforms: H pass + V pass.
        let uni_h = Uniforms {
            radius,
            horizontal: 1,
            _pad: [0.0; 2],
        };
        let uni_v = Uniforms {
            radius,
            horizontal: 0,
            _pad: [0.0; 2],
        };
        let ub_h = self.uniform_buf_h.as_ref().unwrap();
        let ub_v = self.uniform_buf_v.as_ref().unwrap();
        queue.write_buffer(ub_h, 0, bytemuck::bytes_of(&uni_h));
        queue.write_buffer(ub_v, 0, bytemuck::bytes_of(&uni_v));

        // Allocate intermediate texture for H→V hand-off. Same size +
        // format as input/output (caller guarantees they match).
        let intermediate = Self::alloc_intermediate(device, input.size(), format);

        let pipeline = self.pipelines.get(&format).unwrap();
        let bgl = self.bgl.as_ref().unwrap();
        let in_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let mid_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        let out_view = output.create_view(&wgpu::TextureViewDescriptor::default());

        // H pass: input → intermediate
        Self::dispatch_pass(
            device,
            queue,
            pipeline,
            bgl,
            sampler,
            quad_vbo,
            ub_h,
            &in_view,
            &mid_view,
            "playa_effect_blur_pass_h",
        );

        // V pass: intermediate → output
        Self::dispatch_pass(
            device,
            queue,
            pipeline,
            bgl,
            sampler,
            quad_vbo,
            ub_v,
            &mid_view,
            &out_view,
            "playa_effect_blur_pass_v",
        );
    }
}

impl Default for BlurRunner {
    fn default() -> Self {
        Self::new()
    }
}
