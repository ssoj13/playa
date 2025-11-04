use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Fallback vertex shader (embedded)
const VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;

uniform mat4 u_view;
uniform mat4 u_projection;

out vec2 v_uv;

void main() {
    gl_Position = u_projection * u_view * vec4(a_pos, 0.0, 1.0);
    v_uv = a_uv;
}
"#;

/// Fallback fragment shader (embedded) - simple passthrough with exposure/gamma support
const FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec2 v_uv;
out vec4 FragColor;

uniform sampler2D u_texture;
uniform float u_exposure;  // Exposure multiplier (default 1.0)
uniform float u_gamma;     // Gamma correction (default 2.2 for sRGB)
uniform int u_is_hdr;      // 1 for HDR (F16/F32), 0 for LDR (U8)

void main() {
    vec4 color = texture(u_texture, v_uv);

    if (u_is_hdr == 1) {
        // HDR path: apply exposure and gamma correction
        vec3 exposed = color.rgb * u_exposure;
        vec3 gamma_corrected = pow(exposed, vec3(1.0 / u_gamma));
        FragColor = vec4(gamma_corrected, color.a);
    } else {
        // LDR path: already in sRGB, no gamma needed
        FragColor = color;
    }
}
"#;

/// Manages different GLSL shaders for the viewport
#[derive(Clone)]
pub struct Shaders {
    pub shaders: HashMap<String, (String, String)>, // (vertex_shader, fragment_shader)
    pub current_shader: String,
}

impl Shaders {
    pub fn new() -> Self {
        let mut manager = Self {
            shaders: HashMap::new(),
            current_shader: "default".to_string(),
        };

        // Always load embedded default shader first
        manager.load_default();

        // Then try to load from directory (if default.glsl exists, it will override)
        if let Err(e) = manager.load_shader_directory(Path::new("shaders")) {
            log::info!("No shader directory found ({}), using embedded default only", e);
        }

        manager
    }

    pub fn reset_settings(&mut self) {
        self.current_shader = "default".to_string();
    }

    /// Load all available shaders from the shaders directory
    pub fn load_shader_directory(&mut self, shader_dir: &Path) -> Result<(), String> {
        if !shader_dir.exists() {
            return Err(format!("Shader directory does not exist: {:?}", shader_dir));
        }

        for entry in fs::read_dir(shader_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("glsl") {
                if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                    match fs::read_to_string(&path) {
                        Ok(fragment_shader) => {
                            // Use the embedded vertex shader for all fragment shaders
                            self.shaders.insert(
                                filename.to_string(),
                                (VERTEX_SHADER.to_string(), fragment_shader)
                            );
                            log::info!("Loaded shader: {}", filename);
                        },
                        Err(e) => {
                            log::warn!("Failed to read shader file {:?}: {}", path, e);
                        }
                    }
                }
            }
        }

        // Set default shader if available
        if self.shaders.contains_key("default") {
            self.current_shader = "default".to_string();
        } else if let Some(first_key) = self.shaders.keys().next() {
            self.current_shader = first_key.clone();
        }

        Ok(())
    }

    /// Load embedded default shader
    fn load_default(&mut self) {
        self.shaders.insert(
            "default".to_string(),
            (VERTEX_SHADER.to_string(), FRAGMENT_SHADER.to_string())
        );
        self.current_shader = "default".to_string();
        log::info!("Loaded embedded default shader");
    }

    /// Get the current vertex and fragment shaders
    pub fn get_current_shaders(&self) -> (&str, &str) {
        if let Some((vertex, fragment)) = self.shaders.get(&self.current_shader) {
            (vertex, fragment)
        } else {
            // Fallback to default if current shader not found
            if let Some((vertex, fragment)) = self.shaders.get("default") {
                (vertex, fragment)
            } else {
                // Ultimate fallback to embedded shaders
                (VERTEX_SHADER, FRAGMENT_SHADER)
            }
        }
    }

    /// Get a list of available shader names
    pub fn get_shader_names(&self) -> Vec<String> {
        self.shaders.keys().cloned().collect()
    }
}
