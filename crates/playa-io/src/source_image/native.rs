//! Multi-layer EXR source image wrapper.

use std::path::Path;

use vfx_io::{ImageLayer, LayeredImage};

#[derive(Debug, Clone)]
pub struct SourceImage {
    pub layered: LayeredImage,
    pub display_layer_idx: usize,
}

impl SourceImage {
    pub fn open_exr(path: &Path) -> Result<Self, String> {
        let reader = vfx_io::exr::ExrReader::new();
        let layered = reader
            .read_layers(path)
            .map_err(|e| format!("vfx-io EXR read failed: {}", e))?;
        let display_layer_idx = pick_display_layer(&layered);
        Ok(Self {
            layered,
            display_layer_idx,
        })
    }

    pub fn layer_count(&self) -> usize {
        self.layered.layers.len()
    }

    pub fn layer_names(&self) -> Vec<String> {
        self.layered
            .layers
            .iter()
            .enumerate()
            .map(|(i, layer)| {
                if layer.name.is_empty() {
                    format!("Layer{}", i)
                } else {
                    layer.name.clone()
                }
            })
            .collect()
    }

    pub fn layer_compressions(&self) -> Vec<String> {
        self.layered
            .layers
            .iter()
            .map(|layer| {
                layer
                    .spec
                    .attributes
                    .get("compression")
                    .and_then(|v| v.as_str())
                    .unwrap_or("zip")
                    .to_string()
            })
            .collect()
    }

    /// The display layer, or `None` if the image carries no layers. Avoids an
    /// indexing panic when `display_layer_idx` (defaulted to 0 by
    /// `pick_display_layer`) points past an empty layer list.
    pub fn display_layer(&self) -> Option<&ImageLayer> {
        self.layered.layers.get(self.display_layer_idx)
    }
}

pub fn pick_display_layer(layered: &LayeredImage) -> usize {
    if layered.layers.is_empty() {
        return 0;
    }

    let preferred = ["", "rgba", "RGBA", "beauty", "Beauty"];
    if let Some((idx, _)) = layered
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| preferred.contains(&l.name.as_str()))
    {
        return idx;
    }

    layered
        .layers
        .iter()
        .enumerate()
        .max_by_key(|(_, l)| (l.width as u64) * (l.height as u64))
        .map(|(i, _)| i)
        .unwrap_or(0)
}
