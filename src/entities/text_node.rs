//! TextNode - generates rasterized text as Frame.
//!
//! Uses cosmic-text for high-quality text rendering with:
//! - Subpixel antialiasing
//! - Proper text shaping (HarfBuzz)
//! - Unicode support
//! - Multi-line layout

use cosmic_text::{
    Attrs as TextAttrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use uuid::Uuid;

use super::attr_schemas::TEXT_SCHEMA;
use super::attrs::{AttrValue, Attrs};
use super::frame::Frame;
use super::node::{ComputeContext, Node};
use super::keys::{A_HEIGHT, A_WIDTH};

// Global font system (expensive to create, reuse across all TextNodes)
lazy_static::lazy_static! {
    static ref FONT_SYSTEM: Mutex<FontSystem> = Mutex::new(FontSystem::new());
    static ref SWASH_CACHE: Mutex<SwashCache> = Mutex::new(SwashCache::new());
}

/// Text alignment options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "center" => TextAlign::Center,
            "right" => TextAlign::Right,
            _ => TextAlign::Left,
        }
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }
}

/// Text node - generates rasterized text image.
/// 
/// Attributes:
/// - text: the text content (supports \n for newlines)
/// - font: font family name or path to .ttf/.otf
/// - font_size: size in pixels
/// - color: RGBA [0-1]
/// - alignment: "left", "center", "right"
/// - line_height: multiplier (1.0 = normal)
/// - bg_color: background RGBA [0-1]
/// - width/height: output dimensions (0 = auto-size)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextNode {
    pub attrs: Attrs,
}

impl TextNode {
    /// Create new text node with default settings.
    pub fn new(name: &str, text: &str) -> Self {
        let mut attrs = Attrs::with_schema(&TEXT_SCHEMA);
        
        // Identity
        attrs.set("uuid", AttrValue::Uuid(Uuid::new_v4()));
        attrs.set("name", AttrValue::Str(name.to_string()));
        
        // Text content
        attrs.set("text", AttrValue::Str(text.to_string()));
        attrs.set("font", AttrValue::Str("sans-serif".to_string()));
        attrs.set("font_size", AttrValue::Float(72.0));
        attrs.set("color", AttrValue::Vec4([1.0, 1.0, 1.0, 1.0])); // white
        attrs.set("alignment", AttrValue::Str("left".to_string()));
        attrs.set("line_height", AttrValue::Float(1.2));
        
        // Background (transparent by default)
        attrs.set("bg_color", AttrValue::Vec4([0.0, 0.0, 0.0, 0.0]));
        
        // Dimensions (0 = auto)
        attrs.set(A_WIDTH, AttrValue::Int(0));
        attrs.set(A_HEIGHT, AttrValue::Int(0));
        
        attrs.clear_dirty();
        Self { attrs }
    }
    
    /// Create text node with specific UUID.
    pub fn with_uuid(name: &str, text: &str, uuid: Uuid) -> Self {
        let mut node = Self::new(name, text);
        node.attrs.set("uuid", AttrValue::Uuid(uuid));
        node.attrs.clear_dirty();
        node
    }
    
    /// Attach schema after deserialization.
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&TEXT_SCHEMA);
    }
    
    // === Getters ===
    
    pub fn text(&self) -> String {
        self.attrs.get_str("text").unwrap_or("").to_string()
    }
    
    pub fn font(&self) -> String {
        self.attrs.get_str("font").unwrap_or("sans-serif").to_string()
    }
    
    pub fn font_size(&self) -> f32 {
        self.attrs.get_float("font_size").unwrap_or(72.0)
    }
    
    pub fn color(&self) -> [f32; 4] {
        self.attrs.get_vec4("color").unwrap_or([1.0, 1.0, 1.0, 1.0])
    }
    
    pub fn alignment(&self) -> TextAlign {
        let s = self.attrs.get_str("alignment").unwrap_or("left");
        TextAlign::from_str(s)
    }
    
    pub fn line_height(&self) -> f32 {
        self.attrs.get_float("line_height").unwrap_or(1.2)
    }
    
    pub fn bg_color(&self) -> [f32; 4] {
        self.attrs.get_vec4("bg_color").unwrap_or([0.0, 0.0, 0.0, 0.0])
    }
    
    pub fn width(&self) -> i32 {
        self.attrs.get_i32(A_WIDTH).unwrap_or(0)
    }
    
    pub fn height(&self) -> i32 {
        self.attrs.get_i32(A_HEIGHT).unwrap_or(0)
    }
    
    // === Setters ===
    
    pub fn set_text(&mut self, text: &str) {
        self.attrs.set("text", AttrValue::Str(text.to_string()));
    }
    
    pub fn set_font_size(&mut self, size: f32) {
        self.attrs.set("font_size", AttrValue::Float(size));
    }
    
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        self.attrs.set("color", AttrValue::Vec4(rgba));
    }
    
    // === Rendering ===
    
    /// Render text to RGBA buffer.
    fn render_text(&self) -> Frame {
        let text = self.text();
        let font_size = self.font_size();
        let line_height_mult = self.line_height();
        let color = self.color();
        let bg = self.bg_color();
        let alignment = self.alignment();
        let font_family = self.font();
        
        // Lock font system
        let mut font_system = FONT_SYSTEM.lock().unwrap();
        let mut swash_cache = SWASH_CACHE.lock().unwrap();
        
        // Metrics: font size and line height
        let line_height = font_size * line_height_mult;
        let metrics = Metrics::new(font_size, line_height);
        
        // Create text buffer
        let mut buffer = Buffer::new(&mut font_system, metrics);
        
        // Determine buffer width for layout
        let layout_width = if self.width() > 0 {
            self.width() as f32
        } else {
            // Auto-width: use large value, will trim later
            4096.0
        };
        
        buffer.set_size(&mut font_system, Some(layout_width), None);
        
        // Set text with attributes
        let family = if font_family.contains('/') || font_family.contains('\\') {
            // Path to font file - cosmic-text will try to load it
            Family::Name(&font_family)
        } else {
            // Named family
            match font_family.to_lowercase().as_str() {
                "serif" => Family::Serif,
                "monospace" | "mono" => Family::Monospace,
                "cursive" => Family::Cursive,
                "fantasy" => Family::Fantasy,
                _ => Family::SansSerif,
            }
        };
        
        let text_attrs = TextAttrs::new().family(family);
        buffer.set_text(&mut font_system, &text, text_attrs, Shaping::Advanced);
        
        // Shape and layout
        buffer.shape_until_scroll(&mut font_system, false);
        
        // Calculate actual bounds
        let (text_width, text_height) = {
            let mut max_x = 0.0f32;
            let mut max_y = 0.0f32;
            
            for run in buffer.layout_runs() {
                for glyph in run.glyphs.iter() {
                    let x = glyph.x + glyph.w;
                    if x > max_x {
                        max_x = x;
                    }
                }
                let y = run.line_y + line_height;
                if y > max_y {
                    max_y = y;
                }
            }
            
            (max_x.ceil() as usize, max_y.ceil() as usize)
        };
        
        // Final dimensions
        let width = if self.width() > 0 {
            self.width() as usize
        } else {
            text_width.max(1)
        };
        
        let height = if self.height() > 0 {
            self.height() as usize
        } else {
            text_height.max(1)
        };
        
        // Create RGBA buffer with background
        let mut pixels = vec![0u8; width * height * 4];
        
        // Fill background
        let bg_r = (bg[0] * 255.0) as u8;
        let bg_g = (bg[1] * 255.0) as u8;
        let bg_b = (bg[2] * 255.0) as u8;
        let bg_a = (bg[3] * 255.0) as u8;
        
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[0] = bg_r;
            chunk[1] = bg_g;
            chunk[2] = bg_b;
            chunk[3] = bg_a;
        }
        
        // Text color
        let text_color = Color::rgba(
            (color[0] * 255.0) as u8,
            (color[1] * 255.0) as u8,
            (color[2] * 255.0) as u8,
            (color[3] * 255.0) as u8,
        );
        
        // Render glyphs
        buffer.draw(&mut font_system, &mut swash_cache, text_color, |x, y, w, h, color| {
            // Calculate alignment offset
            let align_offset = match alignment {
                TextAlign::Left => 0.0,
                TextAlign::Center => (width as f32 - text_width as f32) / 2.0,
                TextAlign::Right => width as f32 - text_width as f32,
            };
            
            let px = (x as f32 + align_offset) as i32;
            let py = y;
            
            // Bounds check
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                return;
            }
            
            let px = px as usize;
            let py = py as usize;
            
            // Draw the glyph coverage rectangle
            for dy in 0..h as usize {
                for dx in 0..w as usize {
                    let dest_x = px + dx;
                    let dest_y = py + dy;
                    
                    if dest_x >= width || dest_y >= height {
                        continue;
                    }
                    
                    let idx = (dest_y * width + dest_x) * 4;
                    
                    // Alpha blend
                    let src_a = color.a() as f32 / 255.0;
                    let dst_a = pixels[idx + 3] as f32 / 255.0;
                    
                    let out_a = src_a + dst_a * (1.0 - src_a);
                    if out_a > 0.0 {
                        let blend = |src: u8, dst: u8| -> u8 {
                            let s = src as f32 / 255.0;
                            let d = dst as f32 / 255.0;
                            let out = (s * src_a + d * dst_a * (1.0 - src_a)) / out_a;
                            (out * 255.0) as u8
                        };
                        
                        pixels[idx] = blend(color.r(), pixels[idx]);
                        pixels[idx + 1] = blend(color.g(), pixels[idx + 1]);
                        pixels[idx + 2] = blend(color.b(), pixels[idx + 2]);
                        pixels[idx + 3] = (out_a * 255.0) as u8;
                    }
                }
            }
        });
        
        Frame::from_u8_buffer(pixels, width, height)
    }
}

impl Node for TextNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid("uuid").unwrap_or_else(Uuid::nil)
    }
    
    fn name(&self) -> &str {
        self.attrs.get_str("name").unwrap_or("Text")
    }
    
    fn node_type(&self) -> &'static str {
        "Text"
    }
    
    fn attrs(&self) -> &Attrs {
        &self.attrs
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        vec![] // Text nodes have no inputs
    }
    
    /// Render text to Frame.
    fn compute(&self, _frame: i32, ctx: &ComputeContext) -> Option<Frame> {
        // Check cache first (text is always frame 0 since static)
        if let Some(cached) = ctx.cache.get(self.uuid(), 0) {
            return Some(cached);
        }
        
        // Render text
        let frame = self.render_text();
        
        // Cache result (frame 0 since text is static)
        ctx.cache.insert(self.uuid(), 0, frame.clone());
        
        Some(frame)
    }
    
    fn is_dirty(&self) -> bool {
        self.attrs.is_dirty()
    }
    
    fn mark_dirty(&self) {
        self.attrs.mark_dirty();
    }
    
    fn clear_dirty(&self) {
        self.attrs.clear_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_node_defaults() {
        let node = TextNode::new("Test", "Hello World");
        
        assert_eq!(node.name(), "Test");
        assert_eq!(node.text(), "Hello World");
        assert_eq!(node.node_type(), "Text");
        assert!((node.font_size() - 72.0).abs() < 0.01);
    }
    
    #[test]
    fn test_text_alignment() {
        assert_eq!(TextAlign::from_str("left"), TextAlign::Left);
        assert_eq!(TextAlign::from_str("center"), TextAlign::Center);
        assert_eq!(TextAlign::from_str("right"), TextAlign::Right);
        assert_eq!(TextAlign::from_str("CENTER"), TextAlign::Center);
    }
}
