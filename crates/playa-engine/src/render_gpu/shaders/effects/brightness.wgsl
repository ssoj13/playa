// Brightness + contrast adjustment.
//
// CPU equivalent: entities::effects::brightness::apply.
// out.rgb = (in.rgb - 0.5) * (1 + contrast) + 0.5 + brightness; out.a = in.a

struct Uniforms {
    brightness: f32,
    contrast: f32,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var t_in: texture_2d<f32>;
@group(0) @binding(2) var s_tex: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.clip_position = vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let src = textureSample(t_in, s_tex, inp.uv);
    let cf = 1.0 + u.contrast;
    let r = (src.r - 0.5) * cf + 0.5 + u.brightness;
    let g = (src.g - 0.5) * cf + 0.5 + u.brightness;
    let b = (src.b - 0.5) * cf + 0.5 + u.brightness;
    return vec4<f32>(r, g, b, src.a);
}
