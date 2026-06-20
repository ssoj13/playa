# render_gpu — GPU compositing / effects (wgpu)

GPU-side rendering for playa-engine: layer compositor (`wgpu_compositor.rs`) and
post effects (`effects/`: blur, brightness, hsv, plus the shared `effects/mod.rs`
sampler/quad setup).

## wgpu 27 -> 29 migration (API-surface only, no behaviour change)

Applied across `wgpu_compositor.rs`, `effects/mod.rs`, `effects/blur.rs`,
`effects/brightness.rs`, `effects/hsv.rs`:

- `PipelineLayoutDescriptor.bind_group_layouts`: each element is now
  `Option<&BindGroupLayout>`. `&[bgl]` -> `&[Some(bgl)]`.
- `PipelineLayoutDescriptor.push_constant_ranges` removed. Push constants became
  "immediates" in wgpu 28. Replaced the field with `immediate_size: 0` (u32 byte
  count; 0 = no immediates, matching the previous empty push-constant ranges).
- `RenderPipelineDescriptor.multiview` -> `multiview_mask`
  (`Option<NonZeroU32>`, `None` preserved).
- `RenderPassDescriptor` gained a required `multiview_mask` field: added
  `multiview_mask: None` alongside `occlusion_query_set: None`.
- `SamplerDescriptor.mipmap_filter` type changed from `FilterMode` to the new
  dedicated `MipmapFilterMode` enum: `wgpu::FilterMode::Nearest` ->
  `wgpu::MipmapFilterMode::Nearest`. (mag/min filters stay `FilterMode`.)

Authoritative source for the mapping: wgpu-types 29.0.3 in the cargo registry.

The same wgpu-29 descriptor edits also apply to
`playa-ui/src/widgets/viewport/renderer.rs` (pipeline layout + multiview only;
that file has no render pass of its own).
