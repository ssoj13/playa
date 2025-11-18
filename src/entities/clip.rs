//! Clip: image sequence on disk.
//!
//! This is the old `Sequence`, renamed to `Clip`.

use log::info;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::frame::{Frame, FrameError};
use crate::attrs::{Attrs, AttrValue};

/// Detect clips from a list of paths (ported from original Sequence::detect).
pub fn detect(paths: Vec<PathBuf>) -> Result<Vec<Clip>, FrameError> {
    // Original implementation grouped by directory/pattern; for now,
    // we reuse the old behavior: treat each path as potential clip start.
    //
    // To keep this patch focused on structure, we call Clip::new for each
    // path and let it perform pattern detection internally.

    let mut clips = Vec::new();
    for path in paths {
        let clip = Clip::new(
            path.to_string_lossy().to_string(),
            0,
            0,
            None,
            None,
        )?;
        clips.push(clip);
    }
    Ok(clips)
}

/// Clip: sequence of frames with pattern-based file naming.
///
/// All editable properties are stored in `attrs`:
/// - "start" (UInt): First frame number
/// - "end" (UInt): Last frame number
/// - "padding" (UInt): Number of digits in frame numbers
#[derive(Debug, Clone, Serialize)]
pub struct Clip {
    /// Stable identifier inside Project / MediaPool
    pub uuid: String,

    #[serde(skip)]
    frames: Vec<Frame>,

    /// File pattern ("c:/temp/seq1/aaa.*.exr")
    pattern: String,

    /// Frame resolution (detected from first frame)
    xres: usize,
    yres: usize,

    /// Arbitrary attributes (all editable properties stored here)
    pub attrs: Attrs,
}

/// Custom deserialization: rebuild frames after loading from JSON
impl<'de> Deserialize<'de> for Clip {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Temporary struct for deserialization (matches serialized fields)
        #[derive(Deserialize)]
        struct ClipData {
            uuid: String,
            pattern: String,
            xres: usize,
            yres: usize,
            attrs: Attrs,
        }

        let data = ClipData::deserialize(deserializer)?;

        let mut clip = Clip {
            uuid: data.uuid,
            frames: Vec::new(),
            pattern: data.pattern,
            xres: data.xres,
            yres: data.yres,
            attrs: data.attrs,
        };

        // Rebuild frames from pattern (creates Frame::new_unloaded for each frame)
        clip.restore_frames();

        log::info!("Deserialized clip: {} with {} frames", clip.uuid, clip.frames.len());

        Ok(clip)
    }
}

fn gen_clip_uuid(pattern: &str, start: usize, end: usize) -> String {
    format!("clip:{}:{}:{}", pattern, start, end)
}

/// Expand a glob pattern into a list of paths.
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, FrameError> {
    let mut paths = Vec::new();
    for entry in glob::glob(pattern)
        .map_err(|e| FrameError::Image(format!("Glob error for pattern {}: {}", pattern, e)))?
    {
        match entry {
            Ok(path) => paths.push(path),
            Err(e) => {
                return Err(FrameError::Image(format!(
                    "Glob entry error for pattern {}: {}",
                    pattern, e
                )))
            }
        }
    }
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
    let prefix_local = &stem[..digit_start]; // e.g. "seq." or "seq_"
    let padding = number_str.len(); // Actual padding from filename

    // Build full prefix including parent directory
    let mut prefix = String::new();
    if let Some(parent) = path.parent() {
        prefix.push_str(&parent.to_string_lossy());
        if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
            prefix.push(std::path::MAIN_SEPARATOR);
        }
    }
    prefix.push_str(prefix_local);

    Ok(Some((prefix, number, ext, padding)))
}

impl Clip {
    // Getters for attrs-based properties
    pub fn start(&self) -> usize {
        self.attrs.get_u32("start").unwrap_or(0) as usize
    }

    pub fn end(&self) -> usize {
        self.attrs.get_u32("end").unwrap_or(0) as usize
    }

    pub fn padding(&self) -> usize {
        self.attrs.get_u32("padding").unwrap_or(4) as usize
    }

    // Setters for attrs-based properties
    pub fn set_start(&mut self, start: usize) {
        self.attrs.set("start", AttrValue::UInt(start as u32));
    }

    pub fn set_end(&mut self, end: usize) {
        self.attrs.set("end", AttrValue::UInt(end as u32));
    }

    pub fn set_padding(&mut self, padding: usize) {
        self.attrs.set("padding", AttrValue::UInt(padding as u32));
    }

    /// Create clip from file pattern or glob.
    pub fn new(
        pattern: String,
        xres: usize,
        yres: usize,
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<Self, FrameError> {
        let start_val = start.unwrap_or(0);
        let end_val = end.unwrap_or(0);

        let mut attrs = Attrs::new();
        attrs.set("start", AttrValue::UInt(start_val as u32));
        attrs.set("end", AttrValue::UInt(end_val as u32));
        attrs.set("padding", AttrValue::UInt(4));

        let mut clip = Self {
            uuid: gen_clip_uuid(&pattern, start_val, end_val),
            frames: Vec::new(),
            pattern: pattern.clone(),
            xres,
            yres,
            attrs,
        };

        // If pattern has "*" – glob files
        if pattern.contains('*') {
            let pattern_clone = pattern.clone();
            clip.init_from_glob(&pattern_clone)?;
        } else if pattern.contains('%') {
            // Support printf-style pattern: frame.%04d.exr
            let re = Regex::new(r"%0(\d+)d")
                .map_err(|e| FrameError::Image(format!("Regex error: {}", e)))?;
            if let Some(caps) = re.captures(&pattern)
                && let Some(m) = caps.get(1)
            {
                let padding_val = m.as_str().parse::<usize>().unwrap_or(4);
                clip.set_padding(padding_val);
            }
            // Convert to a glob pattern for discovery
            let glob_pattern = re.replace_all(&pattern, "*").to_string();
            clip.init_from_glob(&glob_pattern)?;
        } else {
            // Single file or auto-detect sequence
            clip.init_from_file(&pattern)?;
        }

        // Additional metadata for serialization
        clip.attrs.set("pattern", AttrValue::Str(clip.pattern.clone()));
        clip.attrs.set("xres", AttrValue::UInt(clip.xres as u32));
        clip.attrs.set("yres", AttrValue::UInt(clip.yres as u32));
        // start, end, padding already set in attrs during initialization/init_from_*

        Ok(clip)
    }

    /// Helper for tests: create clip from in-memory frames.
    pub fn from_frames(frames: Vec<Frame>, pattern: String, xres: usize, yres: usize) -> Self {
        let end = if frames.is_empty() { 0 } else { frames.len() - 1 };

        let mut attrs = Attrs::new();
        attrs.set("start", AttrValue::UInt(0));
        attrs.set("end", AttrValue::UInt(end as u32));
        attrs.set("padding", AttrValue::UInt(4));
        attrs.set("pattern", AttrValue::Str(pattern.clone()));
        attrs.set("xres", AttrValue::UInt(xres as u32));
        attrs.set("yres", AttrValue::UInt(yres as u32));

        Self {
            uuid: gen_clip_uuid(&pattern, 0, end),
            frames,
            pattern,
            xres,
            yres,
            attrs,
        }
    }

    /// Restore frames from metadata after deserialization
    pub fn restore_frames(&mut self) {
        self.frames.clear();

        let start = self.start();
        let end = self.end();
        for frame_num in start..=end {
            let path = self.frame_path(frame_num);
            self.frames.push(Frame::new_unloaded(path));
        }
    }

    pub fn frames(&self) -> &Vec<Frame> {
        &self.frames
    }

    pub fn frames_mut(&mut self) -> &mut Vec<Frame> {
        &mut self.frames
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn frame_range(&self) -> (usize, usize) {
        (self.start(), self.end())
    }

    pub fn len(&self) -> usize {
        let start = self.start();
        let end = self.end();
        if end >= start {
            end - start + 1
        } else {
            0
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn resolution(&self) -> (usize, usize) {
        (self.xres, self.yres)
    }

    pub fn get_frame(&self, idx: usize) -> Option<&Frame> {
        self.frames.get(idx)
    }

    /// Get start frame
    pub fn start(&self) -> usize {
        self.start
    }

    /// Get end frame
    pub fn end(&self) -> usize {
        self.end
    }

    /// Total frames (alias for len())
    pub fn total_frames(&self) -> usize {
        self.len()
    }

    /// Get FPS from attrs or default to 24.0
    pub fn fps(&self) -> f32 {
        self.attrs.get_float("fps").unwrap_or(24.0)
    }

    fn frame_path(&self, frame_num: usize) -> PathBuf {
        let padding = self.padding();
        if self.pattern.contains('*') {
            let number = format!("{:0width$}", frame_num, width = padding);
            self.pattern.replace('*', &number).into()
        } else if self.pattern.contains("####") {
            let number = format!("{:0width$}", frame_num, width = padding);
            self.pattern.replace("####", &number).into()
        } else {
            let path = Path::new(&self.pattern);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("frame");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("exr");
            let number = format!("{:0width$}", frame_num, width = padding);
            let mut name = String::with_capacity(stem.len() + 1 + padding + ext.len() + 1);
            name.push_str(stem);
            name.push('.');
            name.push_str(&number);
            name.push('.');
            name.push_str(ext);
            if let Some(parent) = path.parent() {
                parent.join(name)
            } else {
                PathBuf::from(name)
            }
        }
    }

    fn init_from_glob(&mut self, pattern: &str) -> Result<(), FrameError> {
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
            if let Some((prefix, number, ext, padding)) = split_sequence_path(&path)? {
                let key = (prefix, ext);
                groups.entry(key).or_default().push((number, path, padding));
            }
        }

        // Select largest group as main sequence
        let (key, frames) = groups
            .into_iter()
            .max_by_key(|(_, v)| v.len())
            .ok_or_else(|| FrameError::Image("No valid sequence files found".into()))?;

        let (prefix, ext) = key;
        let (min_frame, max_frame) = frames
            .iter()
            .fold((usize::MAX, 0usize), |(min_f, max_f), (num, _, _)| {
                (min_f.min(*num), max_f.max(*num))
            });

        self.set_start(min_frame);
        self.set_end(max_frame);
        // Use padding from first frame in sequence
        let padding_val = frames
            .first()
            .map(|(_, _, padding)| *padding)
            .unwrap_or(4);
        self.set_padding(padding_val);
        self.pattern = format!("{}*.{}", prefix, ext);

        self.frames.clear();
        let start = self.start();
        let end = self.end();
        for frame_num in start..=end {
            let path = self.frame_path(frame_num);
            self.frames.push(Frame::new_unloaded(path));
        }

        info!(
            "Detected clip: {} ({}..{}, {} frames)",
            self.pattern,
            self.start(),
            self.end(),
            self.len()
        );

        Ok(())
    }

    fn init_from_file(&mut self, path_str: &str) -> Result<(), FrameError> {
        let path = PathBuf::from(path_str);
        let (prefix, number, ext, padding) = split_sequence_path(&path)?
            .ok_or_else(|| FrameError::Image(format!("Not a sequence file: {}", path_str)))?;

        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut frames: Vec<(usize, PathBuf)> = Vec::new();

        for entry in std::fs::read_dir(dir)
            .map_err(|e| FrameError::Image(format!("Failed to read dir: {}", e)))?
        {
            let entry = entry.map_err(|e| FrameError::Image(format!("Dir entry error: {}", e)))?;
            let p = entry.path();
            if let Some((pfx, num, ext2, _)) = split_sequence_path(&p)? {
                if pfx == prefix && ext2 == ext {
                    frames.push((num, p));
                }
            }
        }

        if frames.is_empty() {
            return Err(FrameError::Image(format!(
                "No matching sequence files found for {}",
                path_str
            )));
        }

        frames.sort_by_key(|(n, _)| *n);
        self.set_start(frames.first().map(|(n, _)| *n).unwrap_or(number));
        self.set_end(frames.last().map(|(n, _)| *n).unwrap_or(number));
        self.set_padding(padding); // Use actual padding from filename
        self.pattern = format!("{}*.{}", prefix, ext);

        self.frames.clear();
        for (_, p) in frames {
            self.frames.push(Frame::new_unloaded(p));
        }

        info!(
            "Detected clip from file {}: {} ({}..{}, {} frames)",
            path_str,
            self.pattern,
            self.start(),
            self.end(),
            self.len()
        );

        Ok(())
    }
}

// ===== GUI Trait Implementations =====

impl crate::entities::ProjectUI for Clip {
    fn project_ui(&self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            // Icon/type indicator
            ui.label("🎬");

            // Clip name (derived from pattern)
            let name = std::path::Path::new(&self.pattern)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Clip");
            ui.label(name);

            // Metadata
            let (xres, yres) = self.resolution();
            ui.label(format!("{}x{}", xres, yres));
            ui.label(format!("{}-{}", self.start(), self.end()));
            ui.label(format!("{} frames", self.len()));
        })
        .response
    }
}

impl crate::entities::TimelineUI for Clip {
    fn timeline_ui(
        &self,
        ui: &mut egui::Ui,
        bar_rect: egui::Rect,
        current_frame: usize,
    ) -> egui::Response {
        let painter = ui.painter();

        // Draw bar background
        let bar_color = egui::Color32::from_rgb(60, 100, 140);
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Draw border
        painter.rect_stroke(bar_rect, 2.0, (1.0, egui::Color32::WHITE));

        // Highlight current frame if within range
        let start = self.start();
        let end = self.end();
        if current_frame >= start && current_frame <= end {
            let frame_width = bar_rect.width() / (self.len() as f32);
            let offset = (current_frame - start) as f32 * frame_width;
            let playhead_rect = egui::Rect::from_min_size(
                egui::pos2(bar_rect.min.x + offset, bar_rect.min.y),
                egui::vec2(2.0, bar_rect.height()),
            );
            painter.rect_filled(playhead_rect, 0.0, egui::Color32::RED);
        }

        // Draw label
        let name = std::path::Path::new(&self.pattern)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Clip");
        painter.text(
            bar_rect.left_center() + egui::vec2(5.0, 0.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::default(),
            egui::Color32::WHITE,
        );

        ui.interact(bar_rect, ui.id().with(&self.uuid), egui::Sense::click_and_drag())
    }
}

impl crate::entities::AttributeEditorUI for Clip {
    fn ae_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clip");

        // All editable properties are now in attrs
        crate::entities::render_attrs_editor(ui, &mut self.attrs);

        ui.separator();

        // Info section (read-only runtime state)
        egui::CollapsingHeader::new("Info")
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Pattern:");
                    ui.label(&self.pattern);
                });

                ui.horizontal(|ui| {
                    ui.label("UUID:");
                    ui.label(&self.uuid);
                });

                ui.horizontal(|ui| {
                    ui.label("Total Frames:");
                    ui.label(format!("{}", self.len()));
                });

                // Resolution
                let (xres, yres) = self.resolution();
                ui.horizontal(|ui| {
                    ui.label("Resolution:");
                    ui.label(format!("{}x{}", xres, yres));
                });
            });
    }
}
