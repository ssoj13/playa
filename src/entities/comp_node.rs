//! CompNode - composites multiple layers into a single frame.
//!
//! Replaces the COMP_NORMAL mode from Comp. This node composites
//! frames from input layers with blend modes, opacity, transforms.

use std::cell::RefCell;
use std::collections::HashSet;

use half::f16;
use log::debug;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attrs::{AttrValue, Attrs};
use super::compositor::{BlendMode, CpuCompositor};
use super::frame::{Frame, FrameStatus, PixelBuffer, PixelDepth, PixelFormat};
use super::keys::*;
use super::node::{ComputeContext, Node};

// Thread-local compositor and cycle detection
thread_local! {
    static THREAD_COMPOSITOR: RefCell<CpuCompositor> = RefCell::new(CpuCompositor);
    static COMPOSE_STACK: RefCell<HashSet<Uuid>> = RefCell::new(HashSet::new());
}

/// Layer instance - reference to a source node with local attributes.
///
/// Layer is an INSTANCE of a source node. Changing source node attrs
/// affects ALL layers referencing it. Layer attrs are local to this instance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    /// Instance UUID of this layer (unique per layer)
    pub uuid: Uuid,
    /// Source node UUID in project.media
    pub source_uuid: Uuid,
    /// Instance attributes: in, src_len, trim_in, trim_out, opacity, visible, blend_mode, speed, transform
    pub attrs: Attrs,
}

impl Layer {
    /// Create new layer instance referencing a source node.
    pub fn new(source_uuid: Uuid, name: &str, start: i32, duration: i32, dim: (usize, usize)) -> Self {
        let mut attrs = Attrs::new();
        
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set("src_len", AttrValue::Int(duration));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_OPACITY, AttrValue::Float(1.0));
        attrs.set(A_VISIBLE, AttrValue::Bool(true));
        attrs.set(A_BLEND_MODE, AttrValue::Str("normal".to_string()));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        attrs.set(A_WIDTH, AttrValue::UInt(dim.0 as u32));
        attrs.set(A_HEIGHT, AttrValue::UInt(dim.1 as u32));
        // Transform
        attrs.set(A_POSITION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_ROTATION, AttrValue::Vec3([0.0, 0.0, 0.0]));
        attrs.set(A_SCALE, AttrValue::Vec3([1.0, 1.0, 1.0]));
        attrs.set(A_PIVOT, AttrValue::Vec3([0.0, 0.0, 0.0]));
        
        Self {
            uuid: Uuid::new_v4(),
            source_uuid,
            attrs,
        }
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
    
    /// Work area (trimmed range) in absolute frames
    pub fn work_area(&self) -> (i32, i32) {
        let trim_in = self.attrs.get_i32(A_TRIM_IN).unwrap_or(0);
        let trim_out = self.attrs.get_i32(A_TRIM_OUT).unwrap_or(0);
        let speed = self.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        let trim_in_scaled = (trim_in as f32 / speed) as i32;
        let trim_out_scaled = (trim_out as f32 / speed) as i32;
        (self.start() + trim_in_scaled, self.end() - trim_out_scaled)
    }
    
    /// Convert parent frame to source local frame
    pub fn parent_to_local(&self, parent_frame: i32) -> i32 {
        let start = self.start();
        let speed = self.attrs.get_float(A_SPEED).unwrap_or(1.0).abs().max(0.001);
        let trim_in = self.attrs.get_i32(A_TRIM_IN).unwrap_or(0);
        let offset = parent_frame - start;
        trim_in + (offset as f32 * speed) as i32
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
        let mut attrs = Attrs::new();
        let uuid = Uuid::new_v4();
        
        attrs.set_uuid(A_UUID, uuid);
        attrs.set(A_NAME, AttrValue::Str(name.to_string()));
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_FRAME, AttrValue::Int(start));
        attrs.set(A_WIDTH, AttrValue::UInt(1920));
        attrs.set(A_HEIGHT, AttrValue::UInt(1080));
        
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
    
    // --- Getters ---
    
    pub fn _in(&self) -> i32 {
        self.attrs.get_i32(A_IN).unwrap_or(0)
    }
    
    pub fn _out(&self) -> i32 {
        self.attrs.get_i32(A_OUT).unwrap_or(0)
    }
    
    pub fn fps(&self) -> f32 {
        self.attrs.get_float(A_FPS).unwrap_or(24.0)
    }
    
    pub fn dim(&self) -> (usize, usize) {
        let w = self.attrs.get_u32(A_WIDTH).unwrap_or(1920) as usize;
        let h = self.attrs.get_u32(A_HEIGHT).unwrap_or(1080) as usize;
        (w.max(1), h.max(1))
    }
    
    pub fn frame_count(&self) -> i32 {
        (self._out() - self._in() + 1).max(0)
    }
    
    /// Work area (trimmed range) in absolute frames
    pub fn work_area(&self) -> (i32, i32) {
        let trim_in = self.attrs.get_i32(A_TRIM_IN).unwrap_or(0);
        let trim_out = self.attrs.get_i32(A_TRIM_OUT).unwrap_or(0);
        (self._in() + trim_in, self._out() - trim_out)
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
    }
    
    /// Remove layer by UUID
    pub fn remove_layer(&mut self, layer_uuid: Uuid) -> Option<Layer> {
        if let Some(idx) = self.layers.iter().position(|l| l.uuid == layer_uuid) {
            self.mark_dirty();
            Some(self.layers.remove(idx))
        } else {
            None
        }
    }
    
    /// Get layer by UUID
    pub fn get_layer(&self, layer_uuid: Uuid) -> Option<&Layer> {
        self.layers.iter().find(|l| l.uuid == layer_uuid)
    }
    
    /// Get mutable layer by UUID
    pub fn get_layer_mut(&mut self, layer_uuid: Uuid) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.uuid == layer_uuid)
    }
    
    /// Find layers by source UUID
    pub fn layers_by_source(&self, source_uuid: Uuid) -> Vec<&Layer> {
        self.layers.iter().filter(|l| l.source_uuid == source_uuid).collect()
    }
    
    // --- Internal compose ---
    
    fn placeholder_frame(&self) -> Frame {
        let (w, h) = self.dim();
        Frame::new(w, h, PixelDepth::U8)
    }
    
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
        
        let mut source_frames: Vec<(Frame, f32, BlendMode)> = Vec::new();
        let mut earliest: Option<(i32, usize)> = None;
        let mut target_format = PixelFormat::Rgba8;
        let mut all_loaded = true;
        
        // Collect frames from layers (reverse order: last = bottom, first = top)
        for layer in self.layers.iter().rev() {
            let (play_start, play_end) = layer.work_area();
            
            // Skip if outside work area
            if frame_idx < play_start || frame_idx > play_end {
                continue;
            }
            
            // Skip invisible
            if !layer.is_visible() {
                continue;
            }
            
            // Get source node
            let source = ctx.media.get(&layer.source_uuid);
            let Some(source_node) = source else {
                continue;
            };
            
            // Convert to source frame
            let local_frame = layer.parent_to_local(frame_idx);
            let source_in = source_node.attrs().get_i32(A_IN).unwrap_or(0);
            let source_frame = source_in + local_frame;
            
            // Recursively compute source frame
            if let Some(frame) = source_node.compute(source_frame, ctx) {
                if frame.status() != FrameStatus::Loaded {
                    all_loaded = false;
                }
                
                let opacity = layer.opacity();
                let blend = layer.blend_mode();
                source_frames.push((frame, opacity, blend));
                
                let idx = source_frames.len() - 1;
                let start = layer.start();
                if earliest.map_or(true, |(s, _)| start < s) {
                    earliest = Some((start, idx));
                }
                
                // Track highest precision
                target_format = match (target_format, source_frames[idx].0.pixel_format()) {
                    (PixelFormat::RgbaF32, _) | (_, PixelFormat::RgbaF32) => PixelFormat::RgbaF32,
                    (PixelFormat::RgbaF16, _) | (_, PixelFormat::RgbaF16) => PixelFormat::RgbaF16,
                    _ => PixelFormat::Rgba8,
                };
            }
        }
        
        // Determine output dimensions
        let dim = earliest
            .and_then(|(_, idx)| source_frames.get(idx))
            .map(|(f, _, _)| (f.width().max(1), f.height().max(1)))
            .unwrap_or_else(|| self.dim());
        
        // Promote frames to target format
        for (frame, _, _) in source_frames.iter_mut() {
            *frame = promote_frame(frame, target_format);
        }
        
        // Add black base
        let base = create_base_frame(dim, target_format);
        source_frames.insert(0, (base, 1.0, BlendMode::Normal));
        
        debug!(
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
        
        // Mark incomplete if not all loaded
        result.map(|frame| {
            if !all_loaded {
                let _ = frame.set_status(FrameStatus::Loading);
            }
            frame
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
        self.layers.iter().map(|l| l.source_uuid).collect()
    }
    
    fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        let (work_start, work_end) = self.work_area();
        if frame_idx < work_start || frame_idx > work_end {
            return None;
        }
        
        // Check dirty: self, layers, or sources
        let any_layer_dirty = self.layers.iter().any(|l| l.attrs.is_dirty());
        let any_source_dirty = self.layers.iter().any(|l| {
            ctx.media.get(&l.source_uuid)
                .map(|n| n.is_dirty())
                .unwrap_or(false)
        });
        let needs_recompute = self.attrs.is_dirty()
            || any_layer_dirty
            || any_source_dirty
            || !ctx.cache.contains(self.uuid(), frame_idx);
        
        if !needs_recompute {
            if let Some(frame) = ctx.cache.get(self.uuid(), frame_idx) {
                return Some(frame);
            }
        }
        
        // Compose
        let composed = self.compose_internal(frame_idx, ctx)?;
        
        // Cache if fully loaded
        ctx.cache.insert(self.uuid(), frame_idx, composed.clone());
        if composed.status() == FrameStatus::Loaded {
            self.attrs.clear_dirty();
            for layer in &self.layers {
                layer.attrs.clear_dirty();
            }
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
        assert_eq!(layer.source_uuid, source_uuid);
        assert_eq!(layer.start(), 10);
        assert_eq!(layer.end(), 59); // 10 + 50 - 1
    }
    
    #[test]
    fn test_add_remove_layer() {
        let mut node = CompNode::new("Test", 0, 100, 24.0);
        let source_uuid = Uuid::new_v4();
        let layer = Layer::new(source_uuid, "Layer 1", 0, 50, (1920, 1080));
        let layer_uuid = layer.uuid;
        
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
