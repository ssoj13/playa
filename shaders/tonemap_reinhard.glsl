#version 330 core

in vec2 v_uv;
out vec4 FragColor;

uniform sampler2D u_texture;
uniform float u_exposure;  // Exposure multiplier (default 1.0)
uniform float u_gamma;     // Gamma correction (default 2.2 for sRGB)
uniform int u_is_hdr;      // 1 for HDR (F16/F32), 0 for LDR (U8)

// Reinhard Tonemapping
vec3 ReinhardTonemap(vec3 color) {
    return color / (1.0 + color);
}

void main() {
    vec4 texColor = texture(u_texture, v_uv);

    if (u_is_hdr == 1) {
        // HDR path: apply exposure, tone mapping, and gamma
        vec3 exposed = texColor.rgb * u_exposure;
        vec3 tonemapped = ReinhardTonemap(exposed);
        vec3 gamma_corrected = pow(tonemapped, vec3(1.0 / u_gamma));
        FragColor = vec4(gamma_corrected, texColor.a);
    } else {
        // LDR path: already in sRGB, no processing needed
        FragColor = texColor;
    }
}