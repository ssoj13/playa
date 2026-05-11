// HSV (hue/saturation/value) adjustment.
//
// CPU equivalent: entities::effects::hsv::apply. Algorithm mirrors
// rgb_to_hsv / hsv_to_rgb in that module exactly. No value clamp —
// HDR-safe; downstream texture format chooses whether to clamp at
// output stage (Rgba8Unorm clamps; F16/F32 keep range).

struct Uniforms {
    hue_shift: f32,   // degrees, wraps mod 360
    saturation: f32,  // multiplier; clamped to [0, 1]
    value: f32,       // multiplier; unclamped (HDR)
    _pad: f32,
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

fn rgb_to_hsv(rgb: vec3<f32>) -> vec3<f32> {
    let max_c = max(max(rgb.r, rgb.g), rgb.b);
    let min_c = min(min(rgb.r, rgb.g), rgb.b);
    let delta = max_c - min_c;
    let v = max_c;
    let s = select(0.0, delta / max_c, max_c > 0.0);
    var h: f32;
    if abs(delta) < 0.0001 {
        h = 0.0;
    } else if abs(max_c - rgb.r) < 0.0001 {
        h = 60.0 * (((rgb.g - rgb.b) / delta) % 6.0);
    } else if abs(max_c - rgb.g) < 0.0001 {
        h = 60.0 * ((rgb.b - rgb.r) / delta + 2.0);
    } else {
        h = 60.0 * ((rgb.r - rgb.g) / delta + 4.0);
    }
    if h < 0.0 {
        h = h + 360.0;
    }
    return vec3<f32>(h, s, v);
}

fn hsv_to_rgb(hsv: vec3<f32>) -> vec3<f32> {
    let h = hsv.x;
    let s = hsv.y;
    let v = hsv.z;
    if s <= 0.0 {
        return vec3<f32>(v, v, v);
    }
    // Wrap to [0, 360) positive remainder.
    var hn = h % 360.0;
    if hn < 0.0 {
        hn = hn + 360.0;
    }
    let c = v * s;
    let h_prime = hn / 60.0;
    let x = c * (1.0 - abs((h_prime % 2.0) - 1.0));
    let m = v - c;
    var rgb: vec3<f32>;
    if h_prime < 1.0 {
        rgb = vec3<f32>(c, x, 0.0);
    } else if h_prime < 2.0 {
        rgb = vec3<f32>(x, c, 0.0);
    } else if h_prime < 3.0 {
        rgb = vec3<f32>(0.0, c, x);
    } else if h_prime < 4.0 {
        rgb = vec3<f32>(0.0, x, c);
    } else if h_prime < 5.0 {
        rgb = vec3<f32>(x, 0.0, c);
    } else {
        rgb = vec3<f32>(c, 0.0, x);
    }
    return rgb + vec3<f32>(m, m, m);
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let src = textureSample(t_in, s_tex, inp.uv);
    let hsv = rgb_to_hsv(src.rgb);

    // Hue: wrap to [0, 360) positive remainder.
    var h_new = (hsv.x + u.hue_shift) % 360.0;
    if h_new < 0.0 {
        h_new = h_new + 360.0;
    }
    // Saturation: clamped to [0, 1] (mirrors CPU path).
    let s_new = clamp(hsv.y * u.saturation, 0.0, 1.0);
    // Value: unclamped (HDR-safe; output format clamps if needed).
    let v_new = hsv.z * u.value;

    let rgb_new = hsv_to_rgb(vec3<f32>(h_new, s_new, v_new));
    return vec4<f32>(rgb_new, src.a);
}
