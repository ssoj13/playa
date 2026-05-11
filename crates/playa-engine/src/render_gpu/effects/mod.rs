//! GPU-side per-layer effect rendering.
//!
//! Mirror of `entities::effects` but implemented as WGSL render passes.
//! When the GPU compositor is active, `compose_internal` builds a list
//! of [`GpuEffect`]s from the layer's CPU `Effect` list (for effect
//! types that have a GPU implementation here), and attaches it to the
//! [`LayerPayload`]. The compositor's `EffectsRunner::apply_chain` is
//! invoked between layer upload and the blend pass.
//!
//! # Adding a new effect
//!
//! 1. Add the variant to [`GpuEffect`] in `entities::compositor`.
//! 2. Add a WGSL file under `render_gpu/shaders/effects/<name>.wgsl`.
//! 3. Create `effects/<name>.rs` with a runner struct holding the
//!    pipeline cache and a `run` method matching the brightness
//!    example.
//! 4. Wire dispatch in [`EffectsRunner::dispatch`].
//! 5. Map the CPU `Effect` to the new `GpuEffect` variant in
//!    `comp_node::compose_internal`.

pub mod brightness;

use wgpu::util::DeviceExt;

use crate::entities::compositor::GpuEffect;

/// Fullscreen-quad vertex buffer shared by every effect shader.
/// Each vertex is `(pos_xy, uv_xy)`. Triangle strip in NDC.
const QUAD_VERTICES: [[f32; 4]; 6] = [
    [-1.0, -1.0, 0.0, 1.0],
    [1.0, -1.0, 1.0, 1.0],
    [1.0, 1.0, 1.0, 0.0],
    [-1.0, -1.0, 0.0, 1.0],
    [1.0, 1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0, 0.0],
];

/// Vertex layout the effect framework hands to every effect's
/// pipeline construction: vec2 pos + vec2 uv, stride 16.
pub(super) const QUAD_VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> =
    wgpu::VertexBufferLayout {
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
    };

/// Shared per-frame GPU resources for running effect chains on layer
/// textures. Owned by `WgpuCompositor`; each layer's effect list is
/// processed via [`Self::apply_chain`] before that layer's blend pass.
pub struct EffectsRunner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    sampler: wgpu::Sampler,
    quad_vbo: wgpu::Buffer,
    brightness: brightness::BrightnessRunner,
}

impl EffectsRunner {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("playa_effects_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("playa_effects_quad_vbo"),
            contents: bytemuck::cast_slice(&QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let brightness = brightness::BrightnessRunner::new();
        Self {
            device: device.clone(),
            queue: queue.clone(),
            sampler,
            quad_vbo,
            brightness,
        }
    }

    /// Run every effect in `effects` against `input`, returning the
    /// final output texture. Allocates an intermediate texture per
    /// pass — simple ping-pong, no pooling in this first cut.
    ///
    /// `format` is the texture format used for both input and
    /// intermediates (matches the layer's wgpu pixel format).
    pub fn apply_chain(
        &mut self,
        input: wgpu::Texture,
        effects: &[GpuEffect],
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        if effects.is_empty() {
            return input;
        }
        let mut current = input;
        for effect in effects {
            let next = self.alloc_intermediate(current.size(), format);
            self.dispatch(effect, &current, &next, format);
            current = next;
        }
        current
    }

    fn alloc_intermediate(
        &self,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("playa_effects_inter"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    fn dispatch(
        &mut self,
        effect: &GpuEffect,
        input: &wgpu::Texture,
        output: &wgpu::Texture,
        format: wgpu::TextureFormat,
    ) {
        match effect {
            GpuEffect::BrightnessContrast {
                brightness,
                contrast,
            } => {
                self.brightness.run(
                    &self.device,
                    &self.queue,
                    &self.sampler,
                    &self.quad_vbo,
                    input,
                    output,
                    format,
                    *brightness,
                    *contrast,
                );
            }
            // Effects not yet ported to GPU should never reach here:
            // comp_node only attaches GpuEffect variants that have a
            // dispatch arm. If one slips through (programmer error),
            // pass through unchanged + warn.
            other => {
                log::warn!(
                    "GpuEffect '{}' not implemented yet; passing through",
                    other.name()
                );
                self.copy_texture(input, output);
            }
        }
    }

    fn copy_texture(&self, input: &wgpu::Texture, output: &wgpu::Texture) {
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("playa_effects_pass_through_copy"),
            });
        enc.copy_texture_to_texture(input.as_image_copy(), output.as_image_copy(), input.size());
        self.queue.submit(std::iter::once(enc.finish()));
    }
}
