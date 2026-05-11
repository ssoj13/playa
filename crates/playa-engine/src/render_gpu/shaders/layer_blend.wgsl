struct Uniforms {
    opacity: f32,
    blend_mode: i32,
    canvas_size: vec2<f32>,
    top_size: vec2<f32>,
    use_camera: u32,        // 0 = 2D path (3x3 inv_matrix). 1 = camera path.
    layer_z: f32,           // World-space Z of layer plane (camera path only).
    col0: vec4<f32>,
    col1: vec4<f32>,
    col2: vec4<f32>,
    camera_vp_inv: mat4x4<f32>,  // inverse(view * proj). camera path.
    layer_inv: mat4x4<f32>,      // inverse layer model. camera path.
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

/// Compute the src buffer-space pixel coord that the current canvas
/// pixel should sample, going through the active layer transform path
/// (2D inverse-affine OR camera ray-plane unproject).
fn canvas_to_src(canvas_pixel: vec2<f32>) -> vec2<f32> {
    if u.use_camera == 0u {
        // 2D path — single 3×3 affine, canvas (top-left, Y-down) → src.
        let m = mat3x3<f32>(u.col0.xyz, u.col1.xyz, u.col2.xyz);
        let t = m * vec3<f32>(canvas_pixel, 1.0);
        return t.xy;
    }

    // Camera path — non-tilted layer at world Z = u.layer_z.
    //
    // 1. canvas pixel (top-left, Y-down) → NDC (Y-up, [-1,+1])
    let ndc_x = canvas_pixel.x / u.canvas_size.x * 2.0 - 1.0;
    let ndc_y = 1.0 - canvas_pixel.y / u.canvas_size.y * 2.0;

    // 2. Unproject the near and far ends of the camera ray for this NDC.
    let near4 = u.camera_vp_inv * vec4<f32>(ndc_x, ndc_y, -1.0, 1.0);
    let far4  = u.camera_vp_inv * vec4<f32>(ndc_x, ndc_y,  1.0, 1.0);
    let p_near = near4.xyz / near4.w;
    let p_far  = far4.xyz  / far4.w;
    let dir = p_far - p_near;

    // 3. Intersect ray with the layer plane (Z = u.layer_z).
    //    Non-tilted layer = plane normal is (0, 0, 1). For perpendicular
    //    rays this collapses naturally to the parallel-ortho fast path.
    let denom = dir.z;
    if abs(denom) < 1.0e-6 {
        // Camera looking edge-on at the layer — out of bounds.
        return vec2<f32>(-1.0, -1.0);
    }
    let t = (u.layer_z - p_near.z) / denom;
    let world = p_near + dir * t;

    // 4. World → object via inverse layer model.
    let obj4 = u.layer_inv * vec4<f32>(world, 1.0);
    let object = obj4.xyz;

    // 5. object (center, Y-up) → src buffer pixel (top-left, Y-down).
    return vec2<f32>(
        object.x + u.top_size.x * 0.5,
        u.top_size.y * 0.5 - object.y,
    );
}

@fragment
fn fs_blend(inp: VsOut) -> @location(0) vec4<f32> {
    let bottom_color = textureSample(t_bottom, s_tex, inp.uv);
    let canvas_pixel = inp.uv * u.canvas_size;
    let src_pixel = canvas_to_src(canvas_pixel);
    let top_tc = src_pixel / u.top_size;

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
