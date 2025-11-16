//! Project: top-level scene container (playlist).
//!
//! Holds clips (MediaPool) and compositions (Comps) that reference clips.
//! Project is the unit of serialization: scenes are saved and loaded via
//! `Project::to_json` / `Project::from_json`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::attrs::Attrs;
use crate::clip::Clip;
use crate::comp::Comp;

/// Top-level project / scene.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// Global project attributes (fps defaults, resolution presets, etc.)
    pub attrs: Attrs,

    /// All clips in the media pool, keyed by UUID.
    pub clips: HashMap<String, Clip>,

    /// All compositions, keyed by UUID.
    pub comps: HashMap<String, Comp>,

    /// Playlist order for clips (UUIDs).
    pub order_clips: Vec<String>,

    /// Order for compositions in UI (UUIDs).
    pub order_comps: Vec<String>,
}

impl Project {
    pub fn new() -> Self {
        Self {
            attrs: Attrs::new(),
            clips: HashMap::new(),
            comps: HashMap::new(),
            order_clips: Vec::new(),
            order_comps: Vec::new(),
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

        project.rebuild_runtime();
        Ok(project)
    }

    /// Ensure project has at least one composition.
    /// Creates "Main" comp if none exist, and sets it as active.
    ///
    /// Returns UUID of the default/first comp.
    pub fn ensure_default_comp(&mut self) -> String {
        if self.comps.is_empty() {
            let comp = Comp::new("Main", 0, 0, 24.0);
            let uuid = comp.uuid.clone();
            self.comps.insert(uuid.clone(), comp);
            self.order_comps.push(uuid.clone());
            log::info!("Created default comp: {}", uuid);
            uuid
        } else {
            // Return first comp UUID from order
            self.order_comps.first()
                .or_else(|| self.comps.keys().next())
                .cloned()
                .unwrap_or_else(|| {
                    // Fallback: create new if order is broken
                    let comp = Comp::new("Main", 0, 0, 24.0);
                    let uuid = comp.uuid.clone();
                    self.comps.insert(uuid.clone(), comp);
                    self.order_comps.push(uuid.clone());
                    uuid
                })
        }
    }

    /// Rebuild runtime-only state after deserialization.
    ///
    /// - Initializes per-comp caches.
    /// - Rebuilds Layer.clip from Clip UUIDs using Arc<Clip>.
    pub fn rebuild_runtime(&mut self) {
        // Build shared Arc<Clip> map so all layers reference the same instances.
        let mut clip_arcs: HashMap<String, Arc<Clip>> = HashMap::new();
        for (uuid, clip) in &self.clips {
            clip_arcs.insert(uuid.clone(), Arc::new(clip.clone()));
        }

        // Rebuild comps: clear caches and reconnect layers to clips.
        for comp in self.comps.values_mut() {
            comp.clear_cache();

            for layer in comp.layers.iter_mut() {
                if let Some(ref clip_uuid) = layer.clip_uuid {
                    if let Some(arc) = clip_arcs.get(clip_uuid) {
                        layer.clip = Some(Arc::clone(arc));
                    } else {
                        layer.clip = None;
                    }
                } else {
                    layer.clip = None;
                }
            }
        }
    }
}

