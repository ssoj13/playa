//! Multi-layer EXR source image — thin wrapper around [`vfx_io::LayeredImage`].
//!
//! Loaded on demand for EXR sources to expose multi-layer + per-layer
//! compression info to the encode dialog and (later) UI layer pickers.
//! Not serialized — runtime cache only. The compositor display path keeps
//! using the lighter [`crate::entities::Frame`] for viewport rendering.

use std::path::Path;

use vfx_io::{ImageLayer, LayeredImage};

/// Wraps a fully-loaded multi-layer EXR with its OIIO-aligned per-layer
/// `ImageSpec` (compression, channelformats, custom attrs preserved).
#[derive(Debug, Clone)]
pub struct SourceImage {
    /// Full multi-layer image: every layer carries `spec.attributes` populated
    /// by `vfx_io::ExrReader::read_layers` (compression as OIIO string,
    /// channel formats, typed EXR header attrs).
    pub layered: LayeredImage,
    /// Index of the layer that should feed the viewport / single-layer encode
    /// when the user hasn't picked one explicitly. Auto-picked by
    /// [`pick_display_layer`].
    pub display_layer_idx: usize,
}

impl SourceImage {
    /// Open an EXR and return the full multi-layer source.
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

    /// Number of layers in the source.
    pub fn layer_count(&self) -> usize {
        self.layered.layers.len()
    }

    /// Layer names in source order. Empty / missing names get `Layer{i}`.
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

    /// Convenience: per-layer compression strings (OIIO style).
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

    /// Reference to the auto-picked display layer.
    pub fn display_layer(&self) -> &ImageLayer {
        &self.layered.layers[self.display_layer_idx]
    }
}

/// Auto-pick the layer that best fits viewport display: prefer empty/unnamed
/// or `"rgba"`/`"beauty"` named layers. Falls back to the first layer with
/// the largest pixel count.
pub fn pick_display_layer(layered: &LayeredImage) -> usize {
    if layered.layers.is_empty() {
        return 0;
    }

    // Pass 1: prefer canonical "primary" names.
    let preferred = ["", "rgba", "RGBA", "beauty", "Beauty"];
    if let Some((idx, _)) = layered
        .layers
        .iter()
        .enumerate()
        .find(|(_, l)| preferred.contains(&l.name.as_str()))
    {
        return idx;
    }

    // Pass 2: largest layer by pixel area.
    layered
        .layers
        .iter()
        .enumerate()
        .max_by_key(|(_, l)| (l.width as u64) * (l.height as u64))
        .map(|(i, _)| i)
        .unwrap_or(0)
}
