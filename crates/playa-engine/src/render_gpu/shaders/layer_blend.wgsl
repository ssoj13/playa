struct Uniforms {
    opacity: f32,
    blend_mode: i32,
    canvas_size: vec2<f32>,
    top_size: vec2<f32>,
    pad0: vec2<f32>,
    col0: vec4<f32>,
    col1: vec4<f32>,
    col2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var t_bottom: texture_2d<f32>;
@group(0) @binding(2) var t_top: texture_2d<f32>;
@group(0) @binding(3) var s_tex: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_blend(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.clip_position = vec4<f32>(pos, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn blend_rgb(bottom: vec3<f32>, top: vec3<f32>, mode: i32) -> vec3<f32> {
    let t_clamp = clamp(top, vec3(0.0), vec3(1.0));
    let b_clamp = clamp(bottom, vec3(0.0), vec3(1.0));
    if mode == 1 {
        return vec3(1.0) - (vec3(1.0) - b_clamp) * (vec3(1.0) - t_clamp);
    }
    if mode == 2 {
        return min(b_clamp + t_clamp, vec3(1.0));
    }
    if mode == 3 {
        return max(b_clamp - t_clamp, vec3(0.0));
    }
    if mode == 4 {
        return b_clamp * t_clamp;
    }
    if mode == 5 {
        return min(b_clamp / max(t_clamp, vec3(0.00001)), vec3(1.0));
    }
    if mode == 6 {
        return abs(b_clamp - t_clamp);
    }
    if mode == 7 {
        var r: vec3<f32>;
        r.x = select(
            1.0 - 2.0 * (1.0 - b_clamp.x) * (1.0 - t_clamp.x),
            2.0 * b_clamp.x * t_clamp.x,
            b_clamp.x < 0.5
        );
        r.y = select(
            1.0 - 2.0 * (1.0 - b_clamp.y) * (1.0 - t_clamp.y),
            2.0 * b_clamp.y * t_clamp.y,
            b_clamp.y < 0.5
        );
        r.z = select(
            1.0 - 2.0 * (1.0 - b_clamp.z) * (1.0 - t_clamp.z),
            2.0 * b_clamp.z * t_clamp.z,
            b_clamp.z < 0.5
        );
        return r;
    }
    return t_clamp;
}

@fragment
fn fs_blend(inp: VsOut) -> @location(0) vec4<f32> {
    let bottom_color = textureSample(t_bottom, s_tex, inp.uv);

    let m = mat3x3<f32>(
        u.col0.xyz,
        u.col1.xyz,
        u.col2.xyz,
    );

    // `build_inverse_matrix_3x3` (transform.rs) expects input in
    // canvas-centered, Y-up coordinates (range
    // [-W/2, W/2] × [-H/2, H/2]) — that's the AE convention used by
    // the CPU compositor path. Since wgpu's NDC is Y-up and the quad
    // vertex (-1, +1) ↔ uv (0, 1) maps to framebuffer top-left, our
    // `inp.uv * canvas_size` already gives a Y-up coord but with
    // bottom-left as origin, range [0, W] × [0, H]. Subtract half
    // the canvas to recentre.
    let canvas_pixel = inp.uv * u.canvas_size - u.canvas_size * 0.5;
    let transformed = m * vec3<f32>(canvas_pixel, 1.0);
    let top_tc = transformed.xy / u.top_size;

    var top_color: vec4<f32>;
    if (top_tc.x < 0.0 || top_tc.x > 1.0 || top_tc.y < 0.0 || top_tc.y > 1.0) {
        top_color = vec4(0.0);
    } else {
        top_color = textureSample(t_top, s_tex, top_tc);
    }

    let top_alpha = top_color.a * u.opacity;
    let blended = blend_rgb(bottom_color.rgb, top_color.rgb, u.blend_mode);
    let rgb = bottom_color.rgb * (1.0 - top_alpha) + blended * top_alpha;
    let a = bottom_color.a * (1.0 - top_alpha) + top_alpha;
    return vec4<f32>(rgb, a);
}
