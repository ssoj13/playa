#version 330 core

in vec2 v_uv;
out vec4 FragColor;

uniform sampler2D u_texture;
uniform float u_exposure;  // Exposure multiplier (default 1.0)
uniform float u_gamma;     // Gamma correction (default 2.2 for sRGB)
uniform int u_is_hdr;      // 1 for HDR (F16/F32), 0 for LDR (U8)

// ACES Filmic Tone Mapping Curve
// Function from: https://www.shadertoy.com/view/4sjXRh
vec3 ACESFilm(vec3 x) {
    float a = 2.51;
    float b = 0.03;
    float c = 2.43;
    float d = 0.59;
    float e = 0.14;
    return clamp((x*(a*x + b))/(x*(c*x + d) + e), 0.0, 1.0);
}

void main() {
    vec4 color = texture(u_texture, v_uv);

    if (u_is_hdr == 1) {
        // HDR path: apply exposure, tone mapping, and gamma
        vec3 exposed = color.rgb * u_exposure;
        vec3 tone_mapped = ACESFilm(exposed);
        vec3 gamma_corrected = pow(tone_mapped, vec3(1.0 / u_gamma));
        FragColor = vec4(gamma_corrected, color.a);
    } else {
        // LDR path: already in sRGB, no processing needed
        FragColor = color;
    }
}