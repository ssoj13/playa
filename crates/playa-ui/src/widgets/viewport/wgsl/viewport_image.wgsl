// Viewport textured quad — matches bundled GLSL: exposure/gamma/HDR paths + Reinhard / ACES.

struct VsUniforms {
    model: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> vs_uni: VsUniforms;

struct FsUniforms {
    exposure: f32,
    gamma: f32,
    is_hdr: u32,
    tonemap_mode: u32, // 0 default (exposure+gamma only), 1 Reinhard, 2 ACES
}

@group(0) @binding(1)
var<uniform> fs_uni: FsUniforms;

@group(0) @binding(2)
var img: texture_2d<f32>;
@group(0) @binding(3)
var img_smp: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv_in: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    let world = vs_uni.model * vec4<f32>(position, 0.0, 1.0);
    out.clip_position = vs_uni.proj * vs_uni.view * world;
    out.uv = uv_in;
    return out;
}

fn reinhard_tm(c: vec3<f32>) -> vec3<f32> {
    return c / (vec3<f32>(1.0) + c);
}

fn aces_film(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + vec3<f32>(b))) / (x * (c * x + vec3<f32>(d)) + vec3<f32>(e)), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let samp = textureSample(img, img_smp, in.uv);
    var rgb = samp.rgb;

    if fs_uni.is_hdr == 1u {
        rgb = rgb * fs_uni.exposure;
        if fs_uni.tonemap_mode == 1u {
            rgb = reinhard_tm(rgb);
        } else if fs_uni.tonemap_mode == 2u {
            rgb = aces_film(rgb);
        }
        rgb = pow(rgb, vec3<f32>(1.0 / fs_uni.gamma));
    }

    return vec4<f32>(rgb, samp.a);
}
