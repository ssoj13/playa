//! Brightness + contrast effect — direct GPU port of
//! `entities::effects::brightness`.
//!
//! Shader formula matches the CPU implementation exactly:
//! `out.rgb = (in.rgb - 0.5) * (1 + contrast) + 0.5 + brightness`.
//! Alpha untouched.

use std::collections::HashMap;
use wgpu::util::DeviceExt;

const WGSL: &str = include_str!("../shaders/effects/brightness.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    brightness: f32,
    contrast: f32,
    _pad: [f32; 2],
}

/// Per-output-format render pipeline cache for the brightness effect.
/// Shader source is identical across formats but pipelines must be
/// created per-format because the color target format is baked into
/// the pipeline state.
pub struct BrightnessRunner {
    shader: Option<wgpu::ShaderModule>,
    bgl: Option<wgpu::BindGroupLayout>,
    pip_layout: Option<wgpu::PipelineLayout>,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    uniform_buf: Option<wgpu::Buffer>,
}

impl BrightnessRunner {
    pub fn new() -> Self {
        Self {
            shader: None,
            bgl: None,
            pip_layout: None,
            pipelines: HashMap::new(),
            uniform_buf: None,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device) {
        if self.shader.is_none() {
            self.shader = Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("playa_effect_brightness"),
                source: wgpu::ShaderSource::Wgsl(WGSL.into()),
            }));
        }
        if self.bgl.is_none() {
            self.bgl = Some(device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("playa_effect_brightness_bgl"),
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
                label: Some("playa_effect_brightness_layout"),
                bind_group_layouts: &[bgl],
                push_constant_ranges: &[],
            }));
        }
        if self.uniform_buf.is_none() {
            self.uniform_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("playa_effect_brightness_uni"),
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
                label: Some("playa_effect_brightness_pipeline"),
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
        brightness: f32,
        contrast: f32,
    ) {
        self.ensure(device);
        // Borrow pipeline before grabbing &uniform_buf to avoid double mut borrow.
        let _ = self.pipeline_for(device, format);

        let uni = Uniforms {
            brightness,
            contrast,
            _pad: [0.0; 2],
        };
        let uniform_buf = self.uniform_buf.as_ref().unwrap();
        queue.write_buffer(uniform_buf, 0, bytemuck::bytes_of(&uni));

        let pipeline = self.pipelines.get(&format).unwrap();

        let in_view = input.create_view(&wgpu::TextureViewDescriptor::default());
        let out_view = output.create_view(&wgpu::TextureViewDescriptor::default());

        let bgl = self.bgl.as_ref().unwrap();
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("playa_effect_brightness_bg"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("playa_effect_brightness_enc"),
        });
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("playa_effect_brightness_rp"),
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
            });
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, &bg, &[]);
            rp.set_vertex_buffer(0, quad_vbo.slice(..));
            rp.draw(0..6, 0..1);
        }
        queue.submit(std::iter::once(enc.finish()));
    }
}

impl Default for BrightnessRunner {
    fn default() -> Self {
        Self::new()
    }
}
