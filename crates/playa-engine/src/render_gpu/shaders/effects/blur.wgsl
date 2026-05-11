// Separable Gaussian blur.
//
// CPU equivalent: entities::effects::blur. Same formula —
// sigma = radius / 2, half_size = ceil(radius * 2), Gaussian weights
// computed from sigma. Two passes (H then V) are dispatched
// externally by BlurRunner; this shader handles one pass at a time
// (axis selected by `horizontal` uniform).
//
// Edge handling: sampler is set to ClampToEdge at the framework level,
// so out-of-range UVs degrade to the nearest valid texel — matching
// CPU's `.clamp(0, width-1)`.

struct Uniforms {
    radius: f32,
    horizontal: u32,   // 1 = X axis pass, 0 = Y axis pass
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

fn gauss_weight(x: f32, sigma2: f32) -> f32 {
    let two_pi = 6.28318530718;
    return exp(-x * x / (2.0 * sigma2)) / sqrt(two_pi * sigma2);
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let tex_dims = vec2<f32>(textureDimensions(t_in));
    let half_size = i32(ceil(u.radius * 2.0));
    let sigma = u.radius / 2.0;
    let sigma2 = sigma * sigma;

    let dir = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), u.horizontal != 0u);
    let pix_step = dir / tex_dims;

    var sum: vec4<f32> = vec4<f32>(0.0);
    var weight_sum: f32 = 0.0;
    for (var i: i32 = -half_size; i <= half_size; i = i + 1) {
        let w = gauss_weight(f32(i), sigma2);
        let uv_off = inp.uv + f32(i) * pix_step;
        sum = sum + textureSample(t_in, s_tex, uv_off) * w;
        weight_sum = weight_sum + w;
    }
    return sum / weight_sum;
}
