//! FileNode - loads image sequences and video files from disk.
//!
//! Replaces the COMP_FILE mode from Comp. This node type has no inputs
//! and produces frames by loading them from disk based on file_mask pattern.

use std::collections::HashMap;
use std::path::{Path, PathBuf};


use log::info;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::attr_schemas::FILE_SCHEMA;
use super::attrs::{AttrValue, Attrs};
use super::frame::{CropAlign, Frame};
use super::keys::*;
use super::node::{ComputeContext, Node};
use crate::utils::media;

/// Node that loads frames from image sequences or video files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileNode {
    /// Persistent attributes: uuid, name, file_mask, file_start, file_end, fps, width, height
    pub attrs: Attrs,
}

impl FileNode {
    /// Create new file node from file mask pattern.
    ///
    /// # Arguments
    /// * `file_mask` - Path pattern with * for frame numbers (e.g., "frame.*.exr")
    /// * `start` - First frame number in sequence
    /// * `end` - Last frame number in sequence
    /// * `fps` - Frames per second
    pub fn new(file_mask: String, start: i32, end: i32, fps: f32) -> Self {
        let mut attrs = Attrs::with_schema(&*FILE_SCHEMA);
        let uuid = Uuid::new_v4();
        
        attrs.set_uuid(A_UUID, uuid);
        attrs.set(A_NAME, AttrValue::Str(file_mask.clone()));
        attrs.set(A_FILE_MASK, AttrValue::Str(file_mask));
        attrs.set(A_FILE_START, AttrValue::Int(start));
        attrs.set(A_FILE_END, AttrValue::Int(end));
        attrs.set(A_IN, AttrValue::Int(start));
        attrs.set(A_OUT, AttrValue::Int(end));
        attrs.set(A_TRIM_IN, AttrValue::Int(0));
        attrs.set(A_TRIM_OUT, AttrValue::Int(0));
        attrs.set(A_FPS, AttrValue::Float(fps));
        attrs.set(A_SPEED, AttrValue::Float(1.0));
        attrs.set(A_FRAME, AttrValue::Int(start));
        attrs.set(A_WIDTH, AttrValue::UInt(64));
        attrs.set(A_HEIGHT, AttrValue::UInt(64));
        
        Self { attrs }
    }
    
    /// Create with specified UUID (for deserialization)
    pub fn with_uuid(mut self, uuid: Uuid) -> Self {
        self.attrs.set_uuid(A_UUID, uuid);
        self
    }
    
    /// Attach schema after deserialization
    pub fn attach_schema(&mut self) {
        self.attrs.attach_schema(&*FILE_SCHEMA);
    }
    
    // --- Getters ---
    
    pub fn file_mask(&self) -> Option<String> {
        self.attrs.get_str(A_FILE_MASK).map(|s| s.to_string())
    }
    
    pub fn file_start(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_START)
    }
    
    pub fn file_end(&self) -> Option<i32> {
        self.attrs.get_i32(A_FILE_END)
    }
    
    // Timing methods (_in, _out, fps, dim, frame_count, frame, work_area)
    // are provided by Node trait with defaults from config.rs
    
    // --- Internal ---
    
    fn resolve_frame_path(&self, frame_number: i32) -> Option<PathBuf> {
        let mask = self.file_mask()?;
        if media::is_video(Path::new(&mask)) {
            // Video files use @frame suffix to target specific frame
            return Some(PathBuf::from(format!("{}@{}", mask, frame_number)));
        }

        if mask.contains('*') {
            let padding = self.attrs.get_u32("padding").unwrap_or(4) as usize;
            let mut parts = mask.splitn(2, '*');
            let prefix = parts.next().unwrap_or_default();
            let suffix = parts.next().unwrap_or_default();
            let path = format!("{}{:0padding$}{}", prefix, frame_number, suffix);
            Some(PathBuf::from(path))
        } else {
            Some(PathBuf::from(mask))
        }
    }
    
    // placeholder_frame() provided by Node trait
    
    fn frame_from_path(&self, path: PathBuf) -> Frame {
        let (w, h) = self.dim();
        let frame = Frame::new_unloaded(path);
        frame.crop(w, h, CropAlign::LeftTop);
        frame
    }
}

impl Node for FileNode {
    fn uuid(&self) -> Uuid {
        self.attrs.get_uuid(A_UUID).unwrap_or_else(Uuid::nil)
    }
    
    fn name(&self) -> &str {
        self.attrs.get_str(A_NAME).unwrap_or("Untitled")
    }
    
    fn node_type(&self) -> &'static str {
        "File"
    }
    
    fn attrs(&self) -> &Attrs {
        &self.attrs
    }
    
    fn attrs_mut(&mut self) -> &mut Attrs {
        &mut self.attrs
    }
    
    fn inputs(&self) -> Vec<Uuid> {
        vec![] // FileNode has no inputs
    }
    
    fn compute(&self, frame_idx: i32, ctx: &ComputeContext) -> Option<Frame> {
        let duration = self.frame_count();
        if duration <= 0 {
            return None;
        }

        let (work_start, work_end) = self.work_area();
        if work_end < work_start {
            return Some(self.placeholder_frame());
        }

        // Outside work area -> placeholder
        if frame_idx < work_start || frame_idx > work_end {
            return Some(self.placeholder_frame());
        }

        let comp_start = self._in();
        let comp_end = self._out();
        if comp_end < comp_start {
            return None;
        }

        // Convert absolute comp frame to local frame (0-based)
        let clamped_frame = frame_idx.clamp(comp_start, comp_end);
        let local_idx = clamped_frame - comp_start;
        if local_idx < 0 || local_idx >= duration {
            return Some(self.placeholder_frame());
        }

        // Map local frame_idx to absolute sequence number
        let seq_start = self.file_start().unwrap_or(self._in());
        let seq_end = self.file_end().unwrap_or(self._out());
        let seq_frame = seq_start.saturating_add(local_idx);
        if seq_frame < seq_start || seq_frame > seq_end {
            return Some(self.placeholder_frame());
        }

        // Check cache
        let my_uuid = self.uuid();
        if let Some(frame) = ctx.cache.get(my_uuid, frame_idx) {
            return Some(frame);
        }

        // Cache miss: create frame
        let frame_path = self.resolve_frame_path(seq_frame).unwrap_or_default();
        if frame_path.as_os_str().is_empty() {
            return Some(self.placeholder_frame());
        }

        let frame = self.frame_from_path(frame_path);

        // Load pixels from disk (compute() is always called from workers)
        if let Err(e) = frame.load() {
            log::warn!("Failed to load frame {}: {:?}", frame_idx, e);
        }

        // Insert into cache
        ctx.cache.insert(my_uuid, frame_idx, frame.clone());

        Some(frame)
    }
    
    fn is_dirty(&self, _ctx: Option<&ComputeContext>) -> bool {
        // FileNode has no sources, so ctx is ignored
        self.attrs.is_dirty()
    }
    
    fn mark_dirty(&self) {
        self.attrs.mark_dirty()
    }
    
    fn clear_dirty(&self) {
        self.attrs.clear_dirty()
    }
    
    // preload() uses default no-op - FileNode frames are loaded lazily during CompNode::compute()
    
    // --- Override timeline methods ---
    
    fn play_range(&self, _use_work_area: bool) -> (i32, i32) {
        self.work_area()
    }
    
    fn bounds(&self, _use_trim: bool, _selection_only: bool, _media: &std::collections::HashMap<Uuid, std::sync::Arc<super::node_kind::NodeKind>>) -> (i32, i32) {
        self.work_area()
    }
    
    fn frame_count(&self) -> i32 {
        (self._out() - self._in() + 1).max(0)
    }
    
    fn dim(&self) -> (usize, usize) {
        let w = self.attrs.get_u32(A_WIDTH).unwrap_or(64) as usize;
        let h = self.attrs.get_u32(A_HEIGHT).unwrap_or(64) as usize;
        (w.max(1), h.max(1))
    }
}

// --- Sequence Detection ---

use super::loader::Loader;
use super::loader_video;
use crate::entities::frame::FrameError;

impl FileNode {
    /// Detect image/video sequences from paths and create FileNodes.
    ///
    /// Analyzes file paths to detect image sequences (by trailing frame numbers)
    /// or video files, and creates appropriate FileNode instances.
    pub fn detect_from_paths(paths: Vec<PathBuf>) -> Result<Vec<FileNode>, FrameError> {
        let mut nodes = Vec::new();

        for path in paths {
            // Video file: create node from video metadata
            if media::is_video(&path) {
                nodes.push(create_video_node(&path)?);
                continue;
            }

            // Try to detect if this is part of an image sequence
            if let Some((prefix, _number, ext, padding)) = split_sequence_path(&path)? {
                let pattern = format!("{}*.{}", prefix, ext);
                match detect_sequence_from_pattern(&pattern, padding) {
                    Ok(node) => nodes.push(node),
                    Err(e) => {
                        info!("Failed to detect sequence for {}: {}", path.display(), e);
                        if let Ok(node) = create_single_file_node(&path) {
                            nodes.push(node);
                        }
                    }
                }
            } else if let Ok(node) = create_single_file_node(&path) {
                // Single file, not a sequence
                nodes.push(node);
            }
        }

        // Deduplicate nodes by file_mask
        let mut unique: HashMap<String, FileNode> = HashMap::new();
        for node in nodes {
            if let Some(mask) = node.file_mask() {
                unique.entry(mask).or_insert(node);
            }
        }

        Ok(unique.into_values().collect())
    }
}

/// Detect sequence from glob pattern.
fn detect_sequence_from_pattern(pattern: &str, padding: usize) -> Result<FileNode, FrameError> {
    let paths = glob_paths(pattern)?;
    if paths.is_empty() {
        return Err(FrameError::Image(format!(
            "No files matched pattern: {}",
            pattern
        )));
    }

    // Group by (prefix, ext), storing (number, path, padding)
    let mut groups: HashMap<(String, String), Vec<(usize, PathBuf, usize)>> = HashMap::new();

    for path in paths {
        if let Some((prefix, number, ext, pad)) = split_sequence_path(&path)? {
            let key = (prefix, ext);
            groups.entry(key).or_default().push((number, path, pad));
        }
    }

    // Select largest group as main sequence
    let (key, frames_data) = groups
        .into_iter()
        .max_by_key(|(_, v)| v.len())
        .ok_or_else(|| FrameError::Image("No valid sequence files found".into()))?;

    let (prefix, ext) = key;
    let (min_frame, max_frame) = frames_data
        .iter()
        .fold((usize::MAX, 0usize), |(min_f, max_f), (num, _, _)| {
            (min_f.min(*num), max_f.max(*num))
        });

    // Get frame dimensions from first frame
    let first_path = &frames_data[0].1;
    let attrs = Loader::header(first_path)?;
    let width = attrs.get_u32(A_WIDTH).unwrap_or(64) as usize;
    let height = attrs.get_u32(A_HEIGHT).unwrap_or(64) as usize;

    // Create FileNode
    let file_mask = format!("{}*.{}", prefix, ext);
    let mut node = FileNode::new(file_mask.clone(), min_frame as i32, max_frame as i32, 24.0);

    // Store dimensions and padding
    node.attrs.set(A_WIDTH, AttrValue::UInt(width as u32));
    node.attrs.set(A_HEIGHT, AttrValue::UInt(height as u32));
    node.attrs.set("padding", AttrValue::UInt(padding as u32));

    // Set name from first file
    if let Some(filename) = first_path.file_stem().and_then(|s| s.to_str()) {
        node.attrs.set(A_NAME, AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created sequence FileNode: {} ({} frames, {}x{})",
        file_mask,
        frames_data.len(),
        width,
        height
    );

    Ok(node)
}

/// Create FileNode from single image file.
fn create_single_file_node(path: &Path) -> Result<FileNode, FrameError> {
    if media::is_video(path) {
        return create_video_node(path);
    }

    let attrs = Loader::header(path)?;
    let width = attrs.get_u32(A_WIDTH).unwrap_or(64) as usize;
    let height = attrs.get_u32(A_HEIGHT).unwrap_or(64) as usize;

    let file_mask = path.to_string_lossy().to_string();
    let mut node = FileNode::new(file_mask.clone(), 0, 0, 24.0);

    node.attrs.set(A_WIDTH, AttrValue::UInt(width as u32));
    node.attrs.set(A_HEIGHT, AttrValue::UInt(height as u32));

    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
        node.attrs.set(A_NAME, AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created single file FileNode: {} ({}x{})",
        file_mask, width, height
    );

    Ok(node)
}

/// Create FileNode from video file using FFmpeg metadata.
fn create_video_node(path: &Path) -> Result<FileNode, FrameError> {
    let meta = loader_video::VideoMetadata::from_file(path)?;
    let last_frame = meta.frame_count.saturating_sub(1) as i32;
    let mut node = FileNode::new(
        path.to_string_lossy().to_string(),
        0,
        last_frame,
        meta.fps as f32,
    );

    node.attrs.set(A_WIDTH, AttrValue::UInt(meta.width));
    node.attrs.set(A_HEIGHT, AttrValue::UInt(meta.height));
    node.attrs.set("padding", AttrValue::UInt(0));
    node.attrs.set("frames", AttrValue::UInt(meta.frame_count as u32));
    node.attrs.set(A_FPS, AttrValue::Float(meta.fps as f32));
    node.attrs.set(
        "format",
        AttrValue::Str(format!("Video ({})", path.display())),
    );

    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
        node.attrs.set(A_NAME, AttrValue::Str(filename.to_string()));
    }

    info!(
        "Created video FileNode: {} ({} frames, {}x{})",
        path.display(),
        meta.frame_count,
        meta.width,
        meta.height
    );

    Ok(node)
}

/// Expand a glob pattern into a list of paths.
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, FrameError> {
    // Normalize path separators for cross-platform glob compatibility
    let pattern = pattern.replace('\\', "/");
    info!("glob_paths: pattern = {}", pattern);
    
    // Use case-insensitive matching for Windows compatibility (TGA vs tga)
    let options = glob::MatchOptions {
        case_sensitive: false,
        ..Default::default()
    };
    
    let mut paths = Vec::new();
    for entry in glob::glob_with(&pattern, options)
        .map_err(|e| FrameError::Image(format!("Glob error for pattern {}: {}", pattern, e)))?
    {
        match entry {
            Ok(path) => paths.push(path),
            Err(e) => return Err(FrameError::Image(format!("Glob entry error: {}", e))),
        }
    }
    info!("glob_paths: found {} files", paths.len());
    Ok(paths)
}

/// Split a sequence filename into (prefix, number, ext, padding).
///
/// Example: "/path/seq.0001.exr" -> ("/path/seq.", 1, "exr", 4)
fn split_sequence_path(path: &Path) -> Result<Option<(String, usize, String, usize)>, FrameError> {
    let ext = match path.extension().and_then(|s| s.to_str()) {
        Some(e) => e.to_string(),
        None => return Ok(None),
    };

    let stem = match path.file_stem().and_then(|s| s.to_str()) {
        Some(s) => s,
        None => return Ok(None),
    };

    // Find trailing digits in stem
    let mut digit_start = stem.len();
    for (i, ch) in stem.char_indices().rev() {
        if ch.is_ascii_digit() {
            digit_start = i;
        } else {
            break;
        }
    }

    if digit_start == stem.len() {
        // No trailing digits -> not a sequence frame
        return Ok(None);
    }

    let number_str = &stem[digit_start..];
    let number = number_str
        .parse::<usize>()
        .map_err(|e| FrameError::Image(format!("Invalid frame number '{}': {}", number_str, e)))?;
    let prefix_local = &stem[..digit_start];
    let padding = number_str.len();

    // Build full prefix including parent directory
    let parent = path.parent().map(|p| p.to_string_lossy().to_string());
    let full_prefix = match parent {
        Some(p) if !p.is_empty() => format!("{}/{}", p, prefix_local),
        _ => prefix_local.to_string(),
    };

    Ok(Some((full_prefix, number, ext, padding)))
}


#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_file_node_creation() {
        let node = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        assert_eq!(node.file_mask(), Some("test.*.exr".to_string()));
        assert_eq!(node.file_start(), Some(1));
        assert_eq!(node.file_end(), Some(100));
        assert_eq!(node.fps(), 24.0);
        assert_eq!(node.frame_count(), 100);
    }
    
    #[test]
    fn test_file_node_trait() {
        let node = FileNode::new("test.*.exr".to_string(), 1, 100, 24.0);
        assert_eq!(node.node_type(), "File");
        assert!(node.inputs().is_empty());
    }
    
    #[test]
    fn test_cliven_sequence() {
        let test_path = std::path::Path::new(r"D:\_demo\Srcs\Cliven\cliven.0001.TGA");
        if !test_path.exists() {
            println!("Skipping test - path not found");
            return;
        }
        
        // Test split_sequence_path
        let result = split_sequence_path(test_path).unwrap();
        println!("split_sequence_path result: {:?}", result);
        assert!(result.is_some(), "Should detect as sequence");
        
        let (prefix, number, ext, padding) = result.unwrap();
        println!("prefix: {}", prefix);
        println!("number: {}", number);
        println!("ext: {}", ext);
        println!("padding: {}", padding);
        
        // Test glob pattern
        let pattern = format!("{}*.{}", prefix, ext);
        println!("glob pattern: {}", pattern);
        
        let paths = glob_paths(&pattern).unwrap();
        println!("glob found {} files", paths.len());
        assert!(paths.len() > 1, "Should find multiple files, got {}", paths.len());
    }
    
    #[test]
    fn test_cliven_detect_from_paths() {
        let test_path = std::path::PathBuf::from(r"D:\_demo\Srcs\Cliven\cliven.0001.TGA");
        if !test_path.exists() {
            println!("Skipping test - path not found");
            return;
        }
        
        let nodes = FileNode::detect_from_paths(vec![test_path]).unwrap();
        println!("detect_from_paths returned {} nodes", nodes.len());
        
        for node in &nodes {
            println!("Node: mask={:?} start={:?} end={:?} frames={}", 
                node.file_mask(), node.file_start(), node.file_end(), node.frame_count());
        }
        
        assert_eq!(nodes.len(), 1, "Should have exactly 1 sequence node");
        assert!(nodes[0].frame_count() > 1, "Sequence should have more than 1 frame, got {}", nodes[0].frame_count());
    }
}
