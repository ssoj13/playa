//! CompNode - composites multiple layers into a single frame.
//!
//! Replaces the COMP_NORMAL mode from Comp. This node composites
//! frames from input layers with blend modes, opacity, transforms.
//!
//! # Dirty Flag & Caching
//!
//! CompNode uses dirty flags to avoid unnecessary recomposition:
//!
//! - **`is_dirty()`** - returns true if comp attrs OR any layer attrs are dirty
//! - **`clear_dirty()`** - clears dirty on comp AND all layers (called after compute)
//! - **`mark_dirty()`** - marks comp dirty (called by layer mutations)
//!
//! ## Methods that AUTO mark_dirty() (no need to call manually):
//!
//! - **`add_layer()`**, **`remove_layer()`** - layer add/remove
//! - **`move_layers()`** - horizontal layer move
//! - **`trim_layers()`** - trim adjustments
//! - **`set_child_attrs()`** - batch attr changes
//! - **`set_layer_in()`**, **`set_layer_play_start()`**, **`set_layer_play_end()`**
//!
//! ## Methods that DO NOT mark_dirty() (auto via schema):
//!
//! - **`set_frame()`** - playhead is non-DAG in schema, auto-skips dirty
//!
//! ## Direct field changes REQUIRE explicit mark_dirty():
//!
//! ```text
//! comp.layers = reordered;           // Direct assignment
//! comp.layers.insert(idx, layer);    // Direct insert
//! comp.layers.remove(idx);           // Direct remove
//! layer.attrs.set(...);              // Direct layer attr change
//! // After any of these → call comp.attrs.mark_dirty()
//! ```
//!
//! ## Trim Values (IMPORTANT)
//!
//! `trim_in` and `trim_out` are **OFFSETS**, not absolute frame numbers:
//!
//! - For CompNode: `work_start = _in + trim_in`, `work_end = _out - trim_out`
//! - For Layer: offsets in SOURCE frames, scaled by speed for parent timeline
//! - Value of 0 = no trim (full range)
//!
//! ## Layer Order
//!
//! `layers` vector stores layers from **bottom to top** (render order):
//! - `layers[0]` = bottom layer (rendered first, background)
//! - `layers[N-1]` = top layer (rendered last, foreground)
//!
//! In compose_internal(), layers are iterated with `.rev()` so that
//! bottom layers are added to source_frames first (correct blend order).

use std::cell::RefCell;
use std::collections::HashSet;

use half::f16;
use log::trace;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attr_schemas::{COMP_SCHEMA, LAYER_SCHEMA};
use super::attrs::{AttrValue, Attrs};
use super::compositor::{BlendMode, CpuCompositor};
use super::transform;
use super::frame::{Frame, FrameStatus, PixelBuffer, PixelFormat};
use super::keys::*;
use super::node::{ComputeContext, Node};

// Thread-local compositor and cycle detection
//
// ## Why thread_local is CORRECT here (not an architecture smell)
//
// **THREAD_COMPOSITOR** - per-thread CPU compositor:
// - Worker threads don't have OpenGL context (GPU requires main thread)
// - Each thread needs its own mutable compositor instance
// - Can't share RefCell<Compositor> across threads (not Send)
// - thread_local = per-thread singleton, exactly what we need
//
// **COMPOSE_STACK** - per-thread cycle detection:
// - Tracks call path during recursive composition
// - Each thread has independent call stack
// - Parallel workers composing different comps must not interfere
// - thread_local guarantees isolation between threads
//
// Moving these to ComputeContext would require:
// - Creating stack at every compose call site (boilerplate)
// - Sharing compositor via Rc<RefCell<>> (awkward, same semantics)
// - No actual benefit, just complexity
//
// See compositor.rs module docs for GPU transform integration plan.
thread_local! {
    /// Per-thread CPU compositor for blending layers.
    /// Workers use this; main thread could use GPU via Project.compositor.
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = const { RefCell::new(CpuCompositor) };
    
    /// Per-thread cycle detection stack.
    /// Prevents infinite recursion when comp A contains comp B contains comp A.
    static COMPOSE_STACK: RefCell<HashSet<Uuid>> = RefCell::new(HashSet::new());
}

/// Layer instance - reference to a source node with local attributes.
///
/// Layer is an INSTANCE of a source node. Changing source node attrs
/// affects ALL layers referencing it. Layer attrs are local to this instance.
///
/// All data stored in `attrs`:
/// - `uuid`: Instance UUID (unique per layer)
/// - `source_uuid`: Source node UUID in project.media
/// - `name`, `in`, `src_len`, `trim_in`, `trim_out`, `opacity`, `visible`, `blend_mode`, `speed`
/// - `width`, `height`, `position`, `rotation`, `scale`, `pivot`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// All layer attributes stored uniformly
    pub attrs: Attrs,
}

impl Layer {
    /// Create new layer instance referencing a source node.
    pub fn new(source_uuid: Uuid, name: &str, start: i32, duration: i32, dim: (usize, usize)) -> Self {
        let mut attrs = Attrs::with_schema(&*LAYER_SCHEMA);
        
        // Identity
        attrs.set_uuid(A_UUID, Uuid::new_v4());
        attrs.set_uuid("source_uuid", source_uuid);
        
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        // Timing (unified: in, out, trim_in, trim_out, src_len, speed)
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(start + duration));
        attrs.set(A_SRC_LEN, AttrValue::Int(duration));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_OPACITY, AttrValue::Float(1.0));
        attrs.set(A_VISIBLE, AttrValue::Bool(true));
        attrs.set("renderable", AttrValue::Bool(true));
        attrs.set(A_SOLO, AttrValue::Bool(false));
        attrs.set(A_BLEND_MODE, AttrValue::Str("normal".to_string()));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        attrs.set(A_WIDTH, AttrValue::UInt(dim.0 as u32));
        attrs.set(A_HEIGHT, AttrValue::UInt(dim.1 as u32));
        // Transform in frame space (origin = center, Y-up)
        // Position (0,0,0) = layer centered in comp
        attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_ROTATION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_SCALE, AttrValue::Vec3([1.0, 1.0, 1.0]));
        attrs.set(A_PIVOT, AttrValue::Vec3([0.0, 0.0, 0.0]));
        
        // Clear dirty after construction - these are initial values, not changes
        attrs.clear_dirty();
        
        Self { attrs }
    }
    
    /// Get layer instance UUID
    pub fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }
    
    /// Get source node UUID
    pub fn source_uuid(&self) -> Uuid {
        self.attrs.get_uuid("source_uuid").unwrap_or_else(Uuid::nil)
    }
    
    /// Create layer from existing attrs (for duplication/paste).
    /// Sets new uuid, keeps source_uuid from attrs.
    pub fn from_attrs(source_uuid: Uuid, mut attrs: Attrs) -> Self {
        attrs.set_uuid(A_UUID, Uuid::new_v4());
        attrs.set_uuid("source_uuid", source_uuid);
        Self { attrs }
    }
    
    /// Attach schema after deserialization
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*LAYER_SCHEMA);
    }
    
    /// Layer start frame in parent timeline
    pub fn start(&self) -> i32 {
        self.attrs.get_i32(A_IN).unwrap_or(0)
    }
    
    /// Layer end frame in parent timeline (computed from src_len and speed)
    pub fn end(&self) -> i32 {
        let start = self.start();
        let src_len = self.attrs.get_i32("src_len").unwrap_or(1);
        let speed = self.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        start + ((src_len as f32 / speed) as i32) - 1
    }
    
    /// Work area (trimmed range) in absolute frames.
    /// Layer trim_in/trim_out are OFFSETS in SOURCE frames, scaled by speed for parent timeline.
    pub fn work_area(&self) -> (i32, i32) {
        let trim_in = self.attrs.get_i32(A_TRIM_IN).unwrap_or(0);   // offset in source frames
        let trim_out = self.attrs.get_i32(A_TRIM_OUT).unwrap_or(0); // offset in source frames
        let speed = self.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        let trim_in_scaled = (trim_in as f32 / speed) as i32;  // convert to parent timeline frames
        let trim_out_scaled = (trim_out as f32 / speed) as i32;
        (self.start() + trim_in_scaled, self.end() - trim_out_scaled)
    }
    
    /// Convert parent timeline frame to source local frame.
    /// Accounts for layer start position and speed.
    ///
    /// NOTE: Don't add trim_in here! The offset from layer.start() (which is "in")
    /// already accounts for trim via the timeline position.
    /// When playhead is at play_start (= in + trim_in/speed), offset = trim_in/speed,
    /// so local_frame = trim_in - exactly the first visible source frame.
    pub fn parent_to_local(&self, parent_frame: i32) -> i32 {
        let start = self.start(); // = "in" (full bar start)
        let speed = self.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        let offset = parent_frame - start;
        (offset as f32 * speed) as i32
    }
    
    pub fn is_visible(&self) -> bool {
        self.attrs.get_bool(A_VISIBLE).unwrap_or(true)
    }
    
    pub fn opacity(&self) -> f32 {
        self.attrs.get_float(A_OPACITY).unwrap_or(1.0)
    }
    
    pub fn blend_mode(&self) -> BlendMode {
        self.attrs.get_str(A_BLEND_MODE)
            .map(|s| match s {
                "screen" => BlendMode::Screen,
                "add" => BlendMode::Add,
                "subtract" => BlendMode::Subtract,
                "multiply" => BlendMode::Multiply,
                "divide" => BlendMode::Divide,
                "difference" => BlendMode::Difference,
                "overlay" => BlendMode::Overlay,
                _ => BlendMode::Normal,
            })
            .unwrap_or(BlendMode::Normal)
    }
}

/// Node that composites multiple layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompNode {
    /// Persistent attributes: uuid, name, fps, in, out, trim_in, trim_out, width, height
    pub attrs: Attrs,
    /// Ordered layers (bottom to top render order)
    pub layers: Vec<Layer>,
    /// Selection state (runtime only)
    #[serde(default)]
    pub layer_selection: Vec<Uuid>,
    #[serde(default)]
    pub layer_selection_anchor: Option<Uuid>,
}

impl CompNode {
    /// Create new composition node.
    pub fn new(name: &str, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::with_schema(&*COMP_SCHEMA);
        let uuid = Uuid::new_v4();
        
        attrs.set_uuid(A_UUID, uuid);
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        attrs.set(A_FRAME, AttrValue::Int(start));
        attrs.set(A_WIDTH, AttrValue::UInt(1920));
        attrs.set(A_HEIGHT, AttrValue::UInt(1080));
        
        // Clear dirty after construction - these are initial values, not changes
        attrs.clear_dirty();
        
        Self {
            attrs,
            layers: Vec::new(),
            layer_selection: Vec::new(),
            layer_selection_anchor: None,
        }
    }
    
    /// Create with specified UUID
    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.attrs.set_uuid(A_UUID, uuid);
        self
    }
    
    /// Attach schema after deserialization (comp + all layers)
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*COMP_SCHEMA);
        for layer in &mut self.layers {
            layer.attach_schema();
        }
    }
    
    // --- Getters ---
    // Timing methods (_in, _out, fps, dim, frame_count, frame, work_area)
    // are provided by Node trait with defaults from config.rs
    
    /// Get comp name
    pub fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }

    /// Set current playhead frame.
    /// Changing playhead doesn't invalidate cache (frame is non-DAG in schema).
    pub fn set_frame(&mut self, frame: i32) {
        self.attrs.set(A_FRAME, super::attrs::AttrValue::Int(frame));
    }

    /// Play range (work area) - returns (start, end)
    pub fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
        self.work_area()
    }

    /// Number of frames in play range
    pub fn play_frame_count(&self) -> i32 {
        let (start, end) = self.play_range(true);
        (end - start + 1).max(0)
    }

    /// Set play start by adjusting trim_in offset.
    /// trim_in is OFFSET from _in: trim_in = desired_start - _in
    pub fn set_comp_play_start(&mut self, start: i32) {
        let trim_in = (start - self._in()).max(0); // offset from _in
        self.attrs.set(A_TRIM_IN, super::attrs::AttrValue::Int(trim_in));
    }

    /// Set play end by adjusting trim_out offset.
    /// trim_out is OFFSET from _out: trim_out = _out - desired_end
    pub fn set_comp_play_end(&mut self, end: i32) {
        let trim_out = (self._out() - end).max(0); // offset from _out
        self.attrs.set(A_TRIM_OUT, super::attrs::AttrValue::Int(trim_out));
    }

    /// Called when comp becomes active
    pub fn on_activate(&mut self) {
        self.rebound();
    }
    
    /// Calculate actual bounds from all visible layers (read-only).
    ///
    /// - `use_trim=true`: uses layer.work_area() (visible/trimmed range)
    /// - `use_trim=false`: uses layer.start()/end() (full bar range)
    /// - `selection_only=true`: only selected layers (falls back to all if none selected)
    ///
    /// Returns (min_frame, max_frame) or (0, 100) if no visible layers.
    pub fn bounds(&self, use_trim: bool, selection_only: bool) -> (i32, i32) {
        if self.layers.is_empty() {
            return (0, 100);
        }
        
        let use_selection = selection_only && !self.layer_selection.is_empty();
        
        let mut min_start = i32::MAX;
        let mut max_end = i32::MIN;
        
        for layer in &self.layers {
            if !layer.is_visible() {
                continue;
            }
            // Skip non-selected if selection_only mode
            if use_selection && !self.layer_selection.contains(&layer.uuid()) {
                continue;
            }
            let (start, end) = if use_trim {
                layer.work_area()
            } else {
                (layer.start(), layer.end())
            };
            min_start = min_start.min(start);
            max_end = max_end.max(end);
        }
        
        if min_start == i32::MAX || max_end == i32::MIN {
            (0, 100) // No visible layers
        } else {
            (min_start, max_end)
        }
    }
    
    /// Get dimensions of the first visible layer (by work_area start).
    /// Used to determine comp output size.
    pub fn get_first_size(&self) -> Option<(usize, usize)> {
        let mut earliest: Option<(i32, &Layer)> = None;
        
        for layer in &self.layers {
            if !layer.is_visible() {
                continue;
            }
            let (start, _) = layer.work_area();
            if earliest.is_none_or(|(s, _)| start < s) {
                earliest = Some((start, layer));
            }
        }
        
        earliest.map(|(_, layer)| {
            let w = layer.attrs.get_u32(A_WIDTH).unwrap_or(64) as usize;
            let h = layer.attrs.get_u32(A_HEIGHT).unwrap_or(64) as usize;
            (w.max(1), h.max(1))
        })
    }
    
    /// Recalculate comp bounds and dimensions based on layer extents.
    /// Updates _in/_out to encompass all visible layers.
    /// Updates width/height from first visible layer.
    pub fn rebound(&mut self) {
        let old_bounds = (self._in(), self._out());
        let old_work = self.work_area();
        
        let (new_start, new_end) = self.bounds(true, false);
        
        self.attrs.set(A_IN, AttrValue::Int(new_start));
        self.attrs.set(A_OUT, AttrValue::Int(new_end));
        
        // Update dimensions from first visible layer
        if let Some((w, h)) = self.get_first_size() {
            self.attrs.set(A_WIDTH, AttrValue::UInt(w as u32));
            self.attrs.set(A_HEIGHT, AttrValue::UInt(h as u32));
        }
        
        // Keep work area in sync only if it used to match full bounds
        // trim_in/trim_out are OFFSETS from _in/_out, not absolute values
        if old_work == old_bounds {
            self.attrs.set(A_TRIM_IN, AttrValue::Int(0));
            self.attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        }
        
        trace!(
            "rebound: comp={}, old=[{}..{}], new=[{}..{}]",
            self.name(), old_bounds.0, old_bounds.1, new_start, new_end
        );
    }

    /// Get frame at given index.
    /// - `blocking=false`: cache lookup only (for viewport)
    /// - `blocking=true`: compute if not in cache (for encode)
    pub fn get_frame(&self, frame_idx: i32, project: &super::project::Project, blocking: bool) -> Option<Frame> {
        let cache = project.global_cache.as_ref()?;
        
        // Try cache first
        if let Some(frame) = cache.get(self.uuid(), frame_idx) {
            return Some(frame);
        }
        
        // Cache miss
        if !blocking {
            return None;
        }
        
        // Blocking mode: compute now
        let media = project.media.read().expect("media lock");
        let ctx = super::node::ComputeContext {
            cache: cache.as_ref(),
            cache_arc: None, // Not needed for blocking compute
            media: &media,
            media_arc: None,
            workers: None,
            epoch: 0,
        };
        self.compute(frame_idx, &ctx)
    }

    // --- Layer management ---
    
    /// Add layer at specified position (None = append)
    pub fn add_layer(&mut self, layer: Layer, position: Option<usize>) {
        if let Some(idx) = position {
            self.layers.insert(idx.min(self.layers.len()), layer);
        } else {
            self.layers.push(layer);
        }
        self.mark_dirty();
        self.rebound();
    }
    
    /// Remove layer by UUID
    pub fn remove_layer(&mut self, layer_uuid: Uuid) -> Option<Layer> {
        if let Some(idx) = self.layers.iter().position(|l| l.uuid() == layer_uuid) {
            let layer = self.layers.remove(idx);
            self.mark_dirty();
            self.rebound();
            Some(layer)
        } else {
            None
        }
    }
    
    /// Get layer by UUID
    pub fn get_layer(&self, layer_uuid: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|l| l.uuid() == layer_uuid)
    }
    
    /// Get mutable layer by UUID
    pub fn get_layer_mut(&mut self, layer_uuid: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.uuid() == layer_uuid)
    }
    
    /// Find layers by source UUID
    pub fn layers_by_source(&self, source_uuid: Uuid) -> Vec<&Layer> {
        self.layers.iter().filter(|l| l.source_uuid() == source_uuid).collect()
    }
    
    /// Get the active camera for current frame.
    ///
    /// Returns the topmost visible camera layer that covers the given frame.
    /// Layers are checked from top to bottom (reverse iteration since layers
    /// are stored bottom-to-top). Returns None if no camera is active.
    ///
    /// # Return value: (CameraNode, position, rotation)
    ///
    /// Position/rotation come from the LAYER, not CameraNode. This is intentional:
    /// - CameraNode stores lens params only (fov, near/far, projection type)
    /// - Transform lives on Layer like any spatial object
    /// - Avoids duplicate position attrs and confusion
    /// - Matches After Effects architecture
    ///
    /// Default position [0,0,-1000] puts camera 1000 units back on Z axis,
    /// looking at origin (standard setup).
    ///
    /// # Arguments
    /// - `frame_idx` - frame to check camera visibility
    /// - `media` - node registry to look up source types
    pub fn active_camera<'a>(
        &self,
        frame_idx: i32,
        media: &'a std::collections::HashMap<Uuid, std::sync::Arc<super::node_kind::NodeKind>>,
    ) -> Option<(&'a super::camera_node::CameraNode, [f32; 3], [f32; 3])> {
        // Iterate from top to bottom (reverse of storage order)
        for layer in self.layers.iter().rev() {
            // Skip invisible layers
            if !layer.is_visible() {
                continue;
            }

            // Check if frame is within layer's work area
            let (play_start, play_end) = layer.work_area();
            if frame_idx < play_start || frame_idx > play_end {
                continue;
            }

            // Check if source is a camera
            if let Some(source) = media.get(&layer.source_uuid()) {
                if let Some(camera) = source.as_camera() {
                    // Position/rotation come from Layer attrs, not CameraNode
                    let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, -1000.0]);
                    let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
                    return Some((camera, pos, rot));
                }
            }
        }

        None
    }
    
    /// Check if composition has any 3D content (Z != 0 or XY rotation).
    /// 
    /// Used to determine if camera projection should be applied.
    /// Returns true if any visible layer has non-zero Z position or XY rotation.
    pub fn has_3d_content(&self, frame_idx: i32) -> bool {
        for layer in &self.layers {
            if !layer.is_visible() {
                continue;
            }
            
            let (play_start, play_end) = layer.work_area();
            if frame_idx < play_start || frame_idx > play_end {
                continue;
            }
            
            let pos = layer.attrs.get_vec3(super::keys::A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
            let rot = layer.attrs.get_vec3(super::keys::A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
            
            // Check for 3D: Z position or X/Y rotation
            if pos[2].abs() > 0.001 || rot[0].abs() > 0.001 || rot[1].abs() > 0.001 {
                return true;
            }
        }
        
        false
    }

    // --- Compat methods (for migration from old Comp) ---

    /// Alias for remove_layer
    pub fn remove_child(&mut self, layer_uuid: Uuid) -> Option<Layer> {
        self.remove_layer(layer_uuid)
    }

    /// Get children as (uuid, attrs) pairs - compat with old Comp
    pub fn get_children(&self) -> Vec<(Uuid, &Attrs)> {
        self.layers.iter().map(|l| (l.uuid(), &l.attrs)).collect()
    }

    /// Get children as (layer_uuid, source_uuid) pairs - for node editor
    pub fn get_children_sources(&self) -> Vec<(Uuid, Uuid)> {
        self.layers.iter().map(|l| (l.uuid(), l.source_uuid())).collect()
    }

    /// Set FPS
    pub fn set_fps(&mut self, fps: f32) {
        self.attrs.set(A_FPS, super::attrs::AttrValue::Float(fps));
    }

    /// Layer index to UUID
    pub fn idx_to_uuid(&self, idx: usize) -> Option<Uuid> {
        self.layers.get(idx).map(|l| l.uuid())
    }

    /// Layer UUID to index
    pub fn uuid_to_idx(&self, uuid: Uuid) -> Option<usize> {
        self.layers.iter().position(|l| l.uuid() == uuid)
    }

    /// Check if multiple layers are selected
    pub fn is_multi_selected(&self) -> bool {
        self.layer_selection.len() > 1
    }

    /// UUIDs to indices
    pub fn uuids_to_indices(&self, uuids: &[Uuid]) -> Vec<usize> {
        uuids.iter().filter_map(|u| self.uuid_to_idx(*u)).collect()
    }

    /// Get layer "in" frame (full bar start, ignores trims)
    pub fn child_in(&self, layer_uuid: Uuid) -> Option<i32> {
        self.get_layer(layer_uuid).and_then(|l| l.attrs.get_i32(A_IN))
    }

    /// Get layer visual start (play_start = in + trim_in/speed)
    /// This is where the VISIBLE content begins on timeline.
    pub fn child_start(&self, layer_uuid: Uuid) -> Option<i32> {
        self.get_layer(layer_uuid).map(|l| l.work_area().0)
    }

    /// Get layer visual end (play_end, accounts for trims)
    /// This is where the VISIBLE content ends on timeline.
    pub fn child_end(&self, layer_uuid: Uuid) -> Option<i32> {
        self.get_layer(layer_uuid).map(|l| l.work_area().1)
    }

    /// Get layer full bar end (ignores trims, = in + src_len/speed - 1)
    pub fn child_full_end(&self, layer_uuid: Uuid) -> Option<i32> {
        self.get_layer(layer_uuid).map(|l| l.end())
    }

    /// Get layer work area in absolute frames
    pub fn child_work_area_abs(&self, layer_uuid: Uuid) -> Option<(i32, i32)> {
        self.get_layer(layer_uuid).map(|l| l.work_area())
    }

    /// Set multiple attributes on a layer
    pub fn set_child_attrs(&mut self, layer_uuid: Uuid, attrs: Vec<(&str, super::attrs::AttrValue)>) {
        if let Some(layer) = self.get_layer_mut(layer_uuid) {
            for (key, value) in attrs {
                layer.attrs.set(key, value);
            }
            // Only mark comp dirty if layer actually became dirty (DAG attr changed)
            if layer.attrs.is_dirty() {
                self.mark_dirty();
            }
        }
    }

    /// Move layers by delta frames
    pub fn move_layers(&mut self, layer_uuids: &[Uuid], delta: i32) {
        log::trace!("move_layers: uuids={:?}, delta={}", layer_uuids, delta);
        for uuid in layer_uuids {
            if let Some(layer) = self.get_layer_mut(*uuid) {
                let current_in = layer.attrs.get_i32(A_IN).unwrap_or(0);
                layer.attrs.set(A_IN, super::attrs::AttrValue::Int(current_in + delta));
                log::trace!("move_layers: moved layer {} from {} to {}", uuid, current_in, current_in + delta);
            } else {
                log::warn!("move_layers: layer {} not found!", uuid);
            }
        }
        self.mark_dirty();
        self.rebound();
        log::trace!("move_layers: comp marked dirty, is_dirty={}", self.is_dirty());
    }

    /// Trim layers (adjust trim_in/trim_out)
    ///
    /// delta is in TIMELINE frames, will be converted to SOURCE frames via speed.
    /// For "in": positive delta = trim more from start (play_start moves right)
    /// For "out": negative delta = trim more from end (play_end moves left)
    pub fn trim_layers(&mut self, layer_uuids: &[Uuid], edge: &str, delta: i32) {
        for uuid in layer_uuids {
            if let Some(layer) = self.get_layer_mut(*uuid) {
                // Convert timeline delta to source frames
                let speed = layer.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
                let delta_source = (delta as f32 * speed).round() as i32;

                match edge {
                    "in" | "start" => {
                        // Positive delta_source increases trim_in (visible start moves right)
                        let current = layer.attrs.get_i32(A_TRIM_IN).unwrap_or(0);
                        layer.attrs.set(A_TRIM_IN, super::attrs::AttrValue::Int((current + delta_source).max(0)));
                    }
                    "out" | "end" => {
                        // Negative delta means user dragged left -> MORE trim_out
                        // So we SUBTRACT delta_source (which is negative) -> adds to trim_out
                        let current = layer.attrs.get_i32(A_TRIM_OUT).unwrap_or(0);
                        layer.attrs.set(A_TRIM_OUT, super::attrs::AttrValue::Int((current - delta_source).max(0)));
                    }
                    _ => {}
                }
            }
        }
        self.mark_dirty();
        self.rebound();
    }

    /// Add child layer.
    ///
    /// Layer position defaults to (0,0,0) which is CENTER of comp in frame space.
    /// Frame space: origin at center, +X right, +Y up.
    ///
    /// `renderable` - false for control layers (camera, light, null, audio).
    /// `initial_position` - optional override for layer position (e.g. camera at [0,0,-1000]).
    pub fn add_child_layer(
        &mut self,
        source_uuid: Uuid,
        name: &str,
        start_frame: i32,
        duration: i32,
        insert_idx: Option<usize>,
        source_dim: (usize, usize),
        renderable: bool,
        initial_position: Option<[f32; 3]>,
    ) -> anyhow::Result<Uuid> {
        let mut layer = Layer::new(source_uuid, name, start_frame, duration, source_dim);
        layer.attrs.set("renderable", AttrValue::Bool(renderable));
        if let Some(pos) = initial_position {
            layer.attrs.set(A_POSITION, AttrValue::Vec3(pos));
        }
        let uuid = layer.uuid();
        self.add_layer(layer, insert_idx);
        Ok(uuid)
    }

    // --- Additional compat methods ---

    /// Trim in OFFSET (0 = no trim). Returns absolute frame if not set (legacy fallback).
    pub fn trim_in(&self) -> i32 {
        self.attrs.get_i32(A_TRIM_IN).unwrap_or(0)
    }

    /// Trim out OFFSET (0 = no trim). Returns absolute frame if not set (legacy fallback).
    pub fn trim_out(&self) -> i32 {
        self.attrs.get_i32(A_TRIM_OUT).unwrap_or(0)
    }

    /// CompNode is never file mode (that's FileNode)
    pub fn is_file_mode(&self) -> bool {
        false
    }

    /// Get layer UUIDs as vector
    pub fn layers_uuids_vec(&self) -> Vec<Uuid> {
        self.layers.iter().map(|l| l.uuid()).collect()
    }

    /// Get layer attrs by UUID
    pub fn layers_attrs_get(&self, uuid: &Uuid) -> Option<&Attrs> {
        self.layers.iter().find(|l| l.uuid() == *uuid).map(|l| &l.attrs)
    }

    /// Get mutable layer attrs by UUID
    pub fn layers_attrs_get_mut(&mut self, uuid: &Uuid) -> Option<&mut Attrs> {
        self.layers.iter_mut().find(|l| l.uuid() == *uuid).map(|l| &mut l.attrs)
    }

    /// Get all layer edges (start, end) sorted by frame.
    /// Returns (frame, is_start) pairs.
    pub fn get_child_edges(&self) -> Vec<(i32, bool)> {
        let mut edges = Vec::new();
        for layer in &self.layers {
            let start = layer.attrs.layer_start();
            let end = layer.attrs.layer_end();
            if start <= end {
                edges.push((start, true));
                edges.push((end, false));
            }
        }
        edges.sort_by_key(|(frame, _)| *frame);
        edges.dedup_by_key(|(frame, _)| *frame);
        edges
    }

    /// Compute visual row for each layer (greedy non-overlapping layout)
    pub fn compute_layer_rows(&self, child_order: &[usize]) -> std::collections::HashMap<usize, usize> {
        use std::collections::HashMap;
        let mut layer_rows: HashMap<usize, usize> = HashMap::new();
        let mut occupied_rows: HashMap<usize, Vec<(i32, i32)>> = HashMap::new();

        for &idx in child_order {
            let Some(layer) = self.layers.get(idx) else { continue };
            let start = layer.attrs.full_bar_start();
            let end = layer.attrs.full_bar_end();

            let mut row = 0;
            loop {
                let mut row_free = true;
                if let Some(ranges) = occupied_rows.get(&row) {
                    for (occ_start, occ_end) in ranges {
                        if start <= *occ_end && end >= *occ_start {
                            row_free = false;
                            break;
                        }
                    }
                }
                if row_free {
                    occupied_rows.entry(row).or_default().push((start, end));
                    layer_rows.insert(idx, row);
                    break;
                }
                row += 1;
            }
        }
        layer_rows
    }

    /// Check for cycle if potential_child is added
    pub fn check_collisions(
        &self,
        potential_child: Uuid,
        media: &std::collections::HashMap<Uuid, std::sync::Arc<super::node_kind::NodeKind>>,
        hier: bool,
    ) -> bool {
        let my_uuid = self.uuid();
        if potential_child == my_uuid {
            return true;
        }
        if !hier {
            return self.layers.iter().any(|l| l.source_uuid() == potential_child);
        }
        // DFS check for cycles
        let mut stack = vec![potential_child];
        let mut visited = HashSet::new();
        while let Some(current) = stack.pop() {
            if current == my_uuid {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(node) = media.get(&current) {
                for input in node.inputs() {
                    stack.push(input);
                }
            }
        }
        false
    }

    /// Get frame cache statuses from global cache.
    /// Returns status for each frame in the comp's range.
    pub fn cache_frame_statuses(&self, global_cache: Option<&std::sync::Arc<crate::core::global_cache::GlobalFrameCache>>) -> Option<Vec<FrameStatus>> {
        let duration = self.frame_count();
        if duration <= 0 {
            return None;
        }
        
        let Some(cache) = global_cache else {
            return Some(vec![FrameStatus::Placeholder; duration as usize]);
        };
        
        let comp_uuid = self.uuid();
        let comp_start = self._in();
        let mut statuses = Vec::with_capacity(duration as usize);
        
        for frame_offset in 0..duration {
            let frame_idx = comp_start + frame_offset;
            let status = cache
                .get_status(comp_uuid, frame_idx)
                .unwrap_or(FrameStatus::Placeholder);
            statuses.push(status);
        }
        
        Some(statuses)
    }

    /// Move single layer to new start position
    pub fn move_child(&mut self, layer_idx: usize, new_start: i32) -> anyhow::Result<()> {
        let layer = self.layers.get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer index out of bounds"))?;
        layer.attrs.set(A_IN, AttrValue::Int(new_start));
        self.mark_dirty();
        self.rebound();
        Ok(())
    }

    /// Set layer play start (adjusts trim_in)
    pub fn set_child_start(&mut self, layer_idx: usize, new_play_start: i32) -> anyhow::Result<()> {
        let layer = self.layers.get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer index out of bounds"))?;
        let layer_in = layer.attrs.get_i32(A_IN).unwrap_or(0);
        let speed = layer.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        // trim_in in source frames
        let new_trim_in = ((new_play_start - layer_in) as f32 * speed) as i32;
        layer.attrs.set(A_TRIM_IN, AttrValue::Int(new_trim_in.max(0)));
        self.mark_dirty();
        self.rebound();
        Ok(())
    }

    /// Set layer play end (adjusts trim_out)
    pub fn set_child_end(&mut self, layer_idx: usize, new_play_end: i32) -> anyhow::Result<()> {
        let layer = self.layers.get_mut(layer_idx)
            .ok_or_else(|| anyhow::anyhow!("Layer index out of bounds"))?;
        let layer_end = layer.end();
        let speed = layer.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        // trim_out in source frames
        let new_trim_out = ((layer_end - new_play_end) as f32 * speed) as i32;
        layer.attrs.set(A_TRIM_OUT, AttrValue::Int(new_trim_out.max(0)));
        self.mark_dirty();
        self.rebound();
        Ok(())
    }

    // --- Internal compose ---
    // placeholder_frame() provided by Node trait
    
    fn compose_internal(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        let my_uuid = self.uuid();
        
        // Cycle detection
        let is_cycle = COMPOSE_STACK.with(|stack| {
            let mut s = stack.borrow_mut();
            if s.contains(&my_uuid) {
                log::warn!("Cycle detected in compose: {}", my_uuid);
                true
            } else {
                s.insert(my_uuid);
                false
            }
        });
        if is_cycle {
            return Some(self.placeholder_frame());
        }
        
        // New API: (frame, opacity, blend_mode, inverse_transform_matrix)
        let mut source_frames: Vec<(Frame, f32, BlendMode, [f32; 9])> = Vec::new();
        let identity_matrix = super::compositor::IDENTITY_TRANSFORM;
        let mut target_format = PixelFormat::Rgba8;
        let mut all_loaded = true;
        
        // Check if any layer has solo enabled
        let has_solo = self.layers.iter().any(|l| l.attrs.get_bool(A_SOLO).unwrap_or(false));
        
        // Collect visible renderable layers with their Z positions for sorting.
        // Each entry: (layer_index, z_position) - index used for stable sort fallback.
        //
        // LIMITATION: Layer-level sorting, not per-pixel depth testing.
        //
        // Currently we use painter's algorithm with whole-layer Z sorting based on
        // the layer's center position. This means:
        //   - Rotated layers that span multiple Z depths sort as a single unit
        //   - A layer rotated in 3D crossing Z=0 will entirely appear in front of
        //     or behind other layers, not correctly intersecting per-pixel
        //   - No proper depth buffer / Z-buffer compositing
        //
        // For true per-pixel 3D compositing, we would need:
        //   1. GPU depth buffer - render to FBO with depth attachment
        //   2. Per-pixel depth test in fragment shader
        //   3. Or Order-Independent Transparency (OIT) for alpha blending
        //
        // This is a fundamental architectural limitation of the current CPU/hybrid
        // compositor. GPU compositor with depth buffer would solve this.
        //
        let mut renderable_layers: Vec<(usize, f32)> = Vec::new();
        
        for (idx, layer) in self.layers.iter().enumerate() {
            let (play_start, play_end) = layer.work_area();
            
            // Skip if outside work area
            if frame_idx < play_start || frame_idx > play_end {
                continue;
            }
            
            // Skip invisible
            if !layer.is_visible() {
                continue;
            }
            
            // Solo mode: skip non-solo layers when any layer is solo'd
            if has_solo && !layer.attrs.get_bool(A_SOLO).unwrap_or(false) {
                continue;
            }

            // Skip non-renderable layers (camera, light, null, audio)
            if !layer.attrs.get_bool("renderable").unwrap_or(true) {
                continue;
            }

            // Get Z position for depth sorting
            let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
            renderable_layers.push((idx, pos[2]));
        }
        
        // Sort by Z position for painter's algorithm (back-to-front).
        // Camera at +Z looks toward -Z, so:
        //   - Lower Z = farther from camera = render first (behind)
        //   - Higher Z = closer to camera = render last (on top)
        // Ascending sort gives correct order for painter's algorithm.
        renderable_layers.sort_by(|a, b| {
            // Primary: Z position (lower = farther = render first)
            match a.1.partial_cmp(&b.1) {
                Some(std::cmp::Ordering::Equal) | None => {
                    // Secondary: layer index (lower = render first = behind)
                    a.0.cmp(&b.0)
                }
                Some(ord) => ord,
            }
        });
        
        // Get active camera for this frame (if any)
        // Camera provides view-projection matrix for 3D perspective/ortho rendering
        // Position/rotation come from Layer attrs, not CameraNode
        let view_projection: Option<glam::Mat4> = self
            .active_camera(frame_idx, ctx.media)
            .map(|(cam, pos, rot)| {
                let dim = self.dim();
                let aspect = dim.0 as f32 / dim.1 as f32;
                let comp_height = dim.1 as f32;
                cam.view_projection_matrix(pos, rot, aspect, comp_height)
            });
        
        // Render layers in sorted order
        for (layer_idx, _z) in renderable_layers {
            let layer = &self.layers[layer_idx];
            
            // Get source node (already validated above)
            let source = ctx.media.get(&layer.source_uuid());
            let Some(source_node) = source else {
                continue;
            };
            
            // Convert to source frame
            let local_frame = layer.parent_to_local(frame_idx);
            let source_in = source_node.attrs().get_i32(A_IN).unwrap_or(0);
            let source_frame = source_in + local_frame;
            
            // Recursively compute source frame
            if let Some(mut frame) = source_node.compute(source_frame, ctx) {
                if frame.status() != FrameStatus::Loaded {
                    all_loaded = false;
                }
                
                // Get layer transform attributes
                let pos = layer.attrs.get_vec3(A_POSITION).unwrap_or([0.0, 0.0, 0.0]);
                let rot = layer.attrs.get_vec3(A_ROTATION).unwrap_or([0.0, 0.0, 0.0]);
                let scl = layer.attrs.get_vec3(A_SCALE).unwrap_or([1.0, 1.0, 1.0]);
                let pvt = layer.attrs.get_vec3(A_PIVOT).unwrap_or([0.0, 0.0, 0.0]);
                // Convert rotation to radians (XYZ Euler angles)
                let rot_rad = [rot[0].to_radians(), rot[1].to_radians(), rot[2].to_radians()];
                let src_size = (frame.width(), frame.height());
                
                // Apply CPU transform with optional camera projection.
                // Camera enables perspective/ortho 3D; without camera uses 2D ortho.
                // Always transform if source size != comp size (frame space centering).
                let canvas = self.dim();
                let needs_transform = !transform::is_identity(pos, rot_rad, scl, pvt)
                    || view_projection.is_some()
                    || src_size != canvas;  // Source != output = needs centering

                if needs_transform {
                    frame = transform::transform_frame_with_camera(
                        &frame, canvas, pos, rot_rad, scl, pvt, view_projection
                    );
                }
                
                // Build inverse transform matrix for GPU compositor (WIP - not used yet)
                // Matrix is passed through API but CPU compositor ignores it.
                // GPU path still uses Z-only rotation until shader is updated.
                let inv_matrix = if transform::is_identity(pos, rot_rad, scl, pvt) {
                    identity_matrix
                } else {
                    transform::build_inverse_matrix_3x3(pos, rot_rad[2], scl, pvt, src_size)
                };
                
                let opacity = layer.opacity();
                let blend = layer.blend_mode();
                
                source_frames.push((frame, opacity, blend, inv_matrix));
                
                // Track highest precision
                target_format = match (target_format, source_frames.last().unwrap().0.pixel_format()) {
                    (PixelFormat::RgbaF32, _) | (_, PixelFormat::RgbaF32) => PixelFormat::RgbaF32,
                    (PixelFormat::RgbaF16, _) | (_, PixelFormat::RgbaF16) => PixelFormat::RgbaF16,
                    _ => PixelFormat::Rgba8,
                };
            }
        }
        
        // Use first visible layer's dimensions, fallback to comp dims
        let dim = self.get_first_size().unwrap_or_else(|| self.dim());
        
        // Promote frames to target format
        for (frame, _, _, _) in source_frames.iter_mut() {
            *frame = promote_frame(frame, target_format);
        }
        
        // Add black base with identity transform
        let base = create_base_frame(dim, target_format);
        source_frames.insert(0, (base, 1.0, BlendMode::Normal, identity_matrix));
        
        trace!(
            "CompNode::compose {} frames, dim={}x{}, all_loaded={}",
            source_frames.len(), dim.0, dim.1, all_loaded
        );
        
        // Blend with CPU compositor
        let result = THREAD_COMPOSITOR.with(|comp| {
            comp.borrow_mut().blend_with_dim(source_frames, dim)
        });
        
        // Cleanup compose stack
        COMPOSE_STACK.with(|stack| {
            stack.borrow_mut().remove(&my_uuid);
        });
        
        // Mark incomplete if not all source frames loaded yet
        result.inspect(|frame| {
            if !all_loaded {
                let _ = frame.set_status(FrameStatus::Composing);
            }
        })
    }
}

impl Node for CompNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }
    
    fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }
    
    fn node_type(&self) -> &'static str {
        "Comp"
    }
    
    fn attrs(&self) -> &Attrs {
        &self.attrs
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        self.layers.iter().map(|l| l.source_uuid()).collect()
    }
    
    fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        let (work_start, work_end) = self.work_area();
        if frame_idx < work_start || frame_idx > work_end {
            return None;
        }
        
        // Check dirty: self, layers, or sources
        let any_layer_dirty = self.layers.iter().any(|l| l.attrs.is_dirty());
        let any_source_dirty = self.layers.iter().any(|l| {
            ctx.media.get(&l.source_uuid())
                .map(|n| n.is_dirty())
                .unwrap_or(false)
        });
        // Check cache - if has Loaded frame and no dirty, return cached
        // If cached frame is Loading, recompute to check if sources are now Loaded
        let cached_frame = ctx.cache.get(self.uuid(), frame_idx);
        let cache_is_loading = cached_frame.as_ref()
            .map(|f| f.status() != FrameStatus::Loaded)
            .unwrap_or(false);

        let needs_recompute = self.attrs.is_dirty()
            || any_layer_dirty
            || any_source_dirty
            || cached_frame.is_none()
            || cache_is_loading;

        // Trace dirty state for debugging
        if self.attrs.is_dirty() || any_layer_dirty {
            trace!(
                "compute() dirty: comp={}, frame={}, self={}, layer={}, source={}, cache_loading={}",
                self.name(), frame_idx, self.attrs.is_dirty(), any_layer_dirty, any_source_dirty, cache_is_loading
            );
        }

        if !needs_recompute
            && let Some(frame) = cached_frame {
                return Some(frame);
            }
        
        // Compose
        let composed = self.compose_internal(frame_idx, ctx)?;
        
        // Cache result (even if Loading - will be replaced when sources finish)
        ctx.cache.insert(self.uuid(), frame_idx, composed.clone());

        // Always clear dirty after compose - dirty means "attrs changed", not "frame loaded"
        // Frame status (Loading vs Loaded) is tracked separately via FrameStatus
        self.attrs.clear_dirty();
        for layer in &self.layers {
            layer.attrs.clear_dirty();
        }
        
        Some(composed)
    }
    
    fn is_dirty(&self) -> bool {
        self.attrs.is_dirty() || self.layers.iter().any(|l| l.attrs.is_dirty())
    }
    
    fn mark_dirty(&self) {
        self.attrs.mark_dirty()
    }
    
    fn clear_dirty(&self) {
        self.attrs.clear_dirty();
        for layer in &self.layers {
            layer.attrs.clear_dirty();
        }
    }
    
    fn play_range(&self, use_work_area: bool) -> (i32, i32) {
        CompNode::play_range(self, use_work_area)
    }
    
    fn bounds(&self, use_trim: bool, selection_only: bool) -> (i32, i32) {
        CompNode::bounds(self, use_trim, selection_only)
    }
    
    // frame_count() and dim() use trait defaults from Node
    
    fn preload(&self, center: i32, radius: i32, ctx: &ComputeContext) {
        use super::frame::FrameStatus;

        // Nothing to preload for empty comp
        if self.layers.is_empty() {
            return;
        }

        let Some(workers) = ctx.workers else {
            return;
        };

        let (play_start, play_end) = self.work_area();
        if play_end < play_start {
            return;
        }

        trace!(
            "CompNode::preload: comp={}, center={}, work_area=[{}..{}], layers={}",
            self.name(), center, play_start, play_end, self.layers.len()
        );

        // Helper to enqueue compute for a frame
        // Status check moved to worker to avoid mutex locks on UI thread
        let Some(media_arc) = &ctx.media_arc else {
            log::warn!("preload: no media_arc in context");
            return;
        };
        let Some(cache_arc) = &ctx.cache_arc else {
            log::warn!("preload: no cache_arc in context");
            return;
        };
        let uuid = self.uuid();
        let enqueue_compute = |frame_idx: i32| {
            let cache = std::sync::Arc::clone(cache_arc);
            let media = std::sync::Arc::clone(media_arc);
            let epoch = ctx.epoch;

            workers.execute_with_epoch(epoch, Box::new(move || {
                // Check status in worker thread (not UI)
                if let Some(status) = cache.get_status(uuid, frame_idx)
                    && matches!(status, FrameStatus::Loaded | FrameStatus::Loading) {
                        return;
                    }
                
                // CRITICAL: Take snapshot and release lock immediately!
                // Without this, workers hold read lock during compute (50-500ms),
                // blocking UI thread from acquiring write lock → jank.
                //
                // Snapshot clones HashMap structure + Arc refcounts (microseconds).
                // Actual NodeKind data is NOT copied - Arc provides shared ownership.
                let media_snapshot: std::collections::HashMap<uuid::Uuid, std::sync::Arc<super::node_kind::NodeKind>> = {
                    let guard = media.read().expect("media lock");
                    guard.clone() // Clone HashMap of Arcs, not the nodes themselves
                }; // Lock released here - UI can proceed!
                
                let Some(node_arc) = media_snapshot.get(&uuid) else { return; };
                let Some(comp) = node_arc.as_comp() else { return; };
                
                let compute_ctx = ComputeContext {
                    cache: cache.as_ref(),
                    cache_arc: None, // Not needed for nested compute
                    media: &media_snapshot,
                    media_arc: None,
                    workers: None,
                    epoch,
                };
                comp.compute(frame_idx, &compute_ctx);
            }));
        };

        // Spiral from center up to radius
        let max_offset = radius.min(play_end - play_start);

        for offset in 0..=max_offset {
            if center >= offset {
                let idx = center - offset;
                if idx >= play_start && idx <= play_end {
                    enqueue_compute(idx);
                }
            }
            if offset > 0 {
                let idx = center + offset;
                if idx >= play_start && idx <= play_end {
                    enqueue_compute(idx);
                }
            }
        }
    }
}

// --- Stubs for legacy API ---

impl CompNode {
    /// Stub: set event emitter (legacy API - not needed in new architecture)
    pub fn set_event_emitter(&mut self, _emitter: crate::core::event_bus::CompEventEmitter) {
        // No-op: events are handled through Project-level event bus
    }
    
    /// Stub: emit attrs changed event (legacy API)
    pub fn emit_attrs_changed(&self) {
        // No-op: dirty flags handle this now
        self.mark_dirty();
    }
    
    /// Signal background preload for frames around current position.
    ///
    /// Triggers preload for all source FileNodes in layers.
    /// Uses Node::preload() trait method which implements spiral/forward strategies.
    pub fn signal_preload(
        &self,
        workers: &crate::core::workers::Workers,
        project: &crate::entities::Project,
        radius: i32,
    ) {
        use super::node::ComputeContext;
        
        // Nothing to preload for empty comp
        if self.layers.is_empty() {
            return;
        }
        
        // Get cache and epoch
        let global_cache = match &project.global_cache {
            Some(cache) => cache,
            None => return,
        };
        
        let epoch = project.cache_manager()
            .map(|m| m.current_epoch())
            .unwrap_or(0);
        
        let center = self.frame();
        
        let (play_start, play_end) = self.work_area();
        if play_end < play_start {
            return;
        }
        
        trace!(
            "signal_preload: comp={}, center={}, work_area=[{}..{}], layers={}",
            self.name(), center, play_start, play_end, self.layers.len()
        );
        
        // Build ComputeContext and delegate to preload()
        let media = project.media.read().expect("media lock");
        let ctx = ComputeContext {
            cache: global_cache.as_ref(),
            cache_arc: Some(std::sync::Arc::clone(global_cache) as std::sync::Arc<dyn super::traits::FrameCache + Send + Sync>),
            media: &media,
            media_arc: Some(std::sync::Arc::clone(&project.media)),
            workers: Some(workers),
            epoch,
        };

        self.preload(center, radius, &ctx);
    }
}

// --- Helpers ---

fn promote_frame(frame: &Frame, target: PixelFormat) -> Frame {
    match (frame.pixel_format(), target) {
        (PixelFormat::Rgba8, PixelFormat::Rgba8)
        | (PixelFormat::RgbaF16, PixelFormat::RgbaF16)
        | (PixelFormat::RgbaF32, PixelFormat::RgbaF32) => frame.clone(),
        
        (PixelFormat::Rgba8, PixelFormat::RgbaF16) => {
            if let PixelBuffer::U8(buf) = &*frame.buffer() {
                let out: Vec<f16> = buf.iter()
                    .map(|&b| f16::from_f32(b as f32 / 255.0))
                    .collect();
                Frame::from_f16_buffer(out, frame.width(), frame.height())
            } else {
                frame.clone()
            }
        }
        
        (PixelFormat::Rgba8, PixelFormat::RgbaF32) => {
            if let PixelBuffer::U8(buf) = &*frame.buffer() {
                let out: Vec<f32> = buf.iter()
                    .map(|&b| b as f32 / 255.0)
                    .collect();
                Frame::from_f32_buffer(out, frame.width(), frame.height())
            } else {
                frame.clone()
            }
        }
        
        (PixelFormat::RgbaF16, PixelFormat::RgbaF32) => {
            if let PixelBuffer::F16(buf) = &*frame.buffer() {
                let out: Vec<f32> = buf.iter().map(|f| f.to_f32()).collect();
                Frame::from_f32_buffer(out, frame.width(), frame.height())
            } else {
                frame.clone()
            }
        }
        
        _ => frame.clone(),
    }
}

fn create_base_frame(dim: (usize, usize), format: PixelFormat) -> Frame {
    match format {
        PixelFormat::RgbaF32 => {
            let mut buf = vec![0.0f32; dim.0 * dim.1 * 4];
            for px in buf.chunks_exact_mut(4) {
                px[3] = 1.0;
            }
            Frame::from_f32_buffer(buf, dim.0, dim.1)
        }
        PixelFormat::RgbaF16 => {
            let mut buf = vec![f16::from_f32(0.0); dim.0 * dim.1 * 4];
            for px in buf.chunks_exact_mut(4) {
                px[3] = f16::from_f32(1.0);
            }
            Frame::from_f16_buffer(buf, dim.0, dim.1)
        }
        PixelFormat::Rgba8 => {
            let mut buf = vec![0u8; dim.0 * dim.1 * 4];
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255;
            }
            Frame::from_buffer(PixelBuffer::U8(buf), PixelFormat::Rgba8, dim.0, dim.1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_comp_node_creation() {
        let node = CompNode::new("Test Comp", 0, 100, 24.0);
        assert_eq!(node.name(), "Test Comp");
        assert_eq!(node._in(), 0);
        assert_eq!(node._out(), 100);
        assert_eq!(node.fps(), 24.0);
        assert!(node.layers.is_empty());
    }
    
    #[test]
    fn test_layer_creation() {
        let source_uuid = Uuid::new_v4();
        let layer = Layer::new(source_uuid, "Layer 1", 10, 50, (1920, 1080));
        assert_eq!(layer.source_uuid(), source_uuid);
        assert_eq!(layer.start(), 10);
        assert_eq!(layer.end(), 59); // 10 + 50 - 1
    }
    
    #[test]
    fn test_add_remove_layer() {
        let mut node = CompNode::new("Test", 0, 100, 24.0);
        let source_uuid = Uuid::new_v4();
        let layer = Layer::new(source_uuid, "Layer 1", 0, 50, (1920, 1080));
        let layer_uuid = layer.uuid();
        
        node.add_layer(layer, None);
        assert_eq!(node.layers.len(), 1);
        
        let removed = node.remove_layer(layer_uuid);
        assert!(removed.is_some());
        assert!(node.layers.is_empty());
    }
    
    #[test]
    fn test_node_trait() {
        let node = CompNode::new("Test", 0, 100, 24.0);
        assert_eq!(node.node_type(), "Comp");
        assert!(node.inputs().is_empty());
    }
}
