//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::attrs::Attrs;
use crate::comp::Comp;
use crate::compositor::CompositorType;
use crate::media::MediaSource;

/// Top-level project / scene.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// Global project attributes (fps defaults, resolution presets, etc.)
    pub attrs: Attrs,

    /// Unified media pool: all clips and comps keyed by UUID
    pub media: HashMap<String, MediaSource>,

    /// Order for clips in playlist (UUIDs)
    pub clips_order: Vec<String>,

    /// Order for compositions in UI (UUIDs)
    pub comps_order: Vec<String>,

    /// Frame compositor (runtime-only, not serialized)
    /// Used by Comp.compose() for multi-layer blending
    #[serde(skip)]
    #[serde(default)]
    pub compositor: CompositorType,
}

impl Project {
    pub fn new() -> Self {
        Self {
            attrs: Attrs::new(),
            media: HashMap::new(),
            clips_order: Vec::new(),
            comps_order: Vec::new(),
            compositor: CompositorType::default(), // CPU compositor by default
        }
    }

    /// Serialize project to JSON file.
    pub fn to_json<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize project error: {}", e))?;

        let path = path.as_ref();
        let path = if path.extension().and_then(|s| s.to_str()) != Some("json") {
            path.with_extension("json")
        } else {
            path.to_path_buf()
        };

        fs::write(&path, json).map_err(|e| format!("Write project error: {}", e))?;
        Ok(())
    }

    /// Load project from JSON file and rebuild runtime-only state (caches, Arc links).
    pub fn from_json<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let json = fs::read_to_string(path.as_ref())
            .map_err(|e| format!("Read project error: {}", e))?;

        let mut project: Project =
            serde_json::from_str(&json).map_err(|e| format!("Parse project error: {}", e))?;

        // Rebuild without event sender (caller must set it)
        project.rebuild_runtime(None);
        Ok(project)
    }

    /// Ensure project has at least one composition.
    /// Creates "Main" comp if none exist, and sets it as active.
    ///
    /// Returns UUID of the default/first comp.
    pub fn ensure_default_comp(&mut self) -> String {
        // Check if we have any comps in media
        let has_comps = self.media.values().any(|s| s.is_comp());

        if !has_comps {
            let comp = Comp::new("Main", 0, 0, 24.0);
            let uuid = comp.uuid.clone();
            self.media.insert(uuid.clone(), MediaSource::Comp(comp));
            self.comps_order.push(uuid.clone());
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            // Return first comp UUID from order
            self.comps_order.first()
                .or_else(|| {
                    // Find first comp UUID in media
                    self.media.iter()
                        .find(|(_, s)| s.is_comp())
                        .map(|(uuid, _)| uuid)
                })
                .cloned()
                .unwrap_or_else(|| {
                    // Fallback: create new if order is broken
                    let comp = Comp::new("Main", 0, 0, 24.0);
                    let uuid = comp.uuid.clone();
                    self.media.insert(uuid.clone(), MediaSource::Comp(comp));
                    self.comps_order.push(uuid.clone());
                    uuid
                })
        }
    }

    /// Rebuild runtime-only state after deserialization.
    ///
    /// - Clears per-comp caches.
    /// - Reinitializes compositor to default (CPU).
    /// - Sets event sender for all comps.
    pub fn rebuild_runtime(&mut self, event_sender: Option<crate::events::CompEventSender>) {
        // Reinitialize compositor (not serialized)
        self.compositor = CompositorType::default();

        // Rebuild comps in unified media HashMap
        for source in self.media.values() {
            if let Some(comp) = source.as_comp() {
                comp.clear_cache();

                // TODO: Set event sender for comps in media HashMap
                // This requires mut access - consider using RefCell for event_sender
                let _ = event_sender;
            }
        }
    }

    /// Set compositor type (CPU or GPU).
    ///
    /// Allows switching between CPU and GPU compositing backends.
    /// GPU compositor requires OpenGL/WGPU context (future feature).
    pub fn set_compositor(&mut self, compositor: CompositorType) {
        log::info!("Compositor changed to: {:?}", compositor);
        self.compositor = compositor;
    }
}

