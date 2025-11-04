//! Image sequence detection and indexing
//!
//! **Why**: Artists drag folders containing numbered frames (render.0001.exr, render.0002.exr...).
//! Auto-detection finds sequences, extracts frame range, builds path templates.
//!
//! **Used by**: Drag-drop handler (file discovery), Cache (frame path resolution)
//!
//! # Detection Algorithm
//!
//! 1. Scan directory for files matching `<prefix>.<digits>.<ext>`
//! 2. Group by (prefix, ext) - handles multiple sequences in one folder
//! 3. Parse frame numbers, detect gaps (missing frames)
//! 4. Build template: `render.####.exr` where #### = frame number
//!
//! # Supported Formats
//!
//! - EXR: OpenEXR with DWAA/DWAB compression
//! - PNG: 8/16-bit, alpha channel
//! - JPG: 8-bit RGB/RGBA
//!
//! # Frame Numbering
//!
//! Supports: 0001, 001, 1 (zero-padded or not)
//! Handles: Gaps in sequence (skipped frames)

use log::info;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::frame::{Frame, FrameError};

/// Sequence of frames with pattern-based file naming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    #[serde(skip)]
    frames: Vec<Frame>,
    pattern: String,        // "c:/temp/seq1/aaa.*.exr"
    start: usize,           // first frame number
    end: usize,             // last frame number
    padding: usize,         // number of digits (4 for "0001")
    xres: usize,
    yres: usize,
}

impl Sequence {
    /// Create sequence from file pattern or glob
    ///
    /// **Why**: Initialize sequence with known pattern and resolution
    ///
    /// **Used by**: Manual sequence creation, pattern-based loading
    ///
    /// # Arguments
    ///
    /// - `pattern`: File path or glob pattern (e.g., `"render.*.exr"` or `"shot.0001.jpg"`)
    /// - `xres`, `yres`: Expected frame resolution (width, height)
    /// - `start`, `end`: Optional frame range override
    ///
    /// If pattern contains `*`: glob files and detect range.
    /// Otherwise: auto-detect sequence from single file.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::sequence::Sequence;
    /// // Glob pattern - finds all matching files
    /// let seq = Sequence::new("render.*.exr".into(), 1920, 1080, None, None)?;
    ///
    /// // Single file - detects sequence automatically
    /// let seq = Sequence::new("shot.0001.jpg".into(), 1920, 1080, None, None)?;
    /// # Ok::<(), playa::frame::FrameError>(())
    /// ```
    pub fn new(pattern: String, xres: usize, yres: usize, start: Option<usize>, end: Option<usize>) -> Result<Self, FrameError> {
        let mut seq = Self {
            frames: Vec::new(),
            pattern: pattern.clone(),
            start: start.unwrap_or(0),
            end: end.unwrap_or(0),
            padding: 4,
            xres,
            yres,
        };

        // If pattern has "*" → glob files
        if pattern.contains('*') {
            let pattern_clone = pattern.clone();
            seq.init_from_glob(&pattern_clone)?;
        } else if pattern.contains('%') {
            // Support printf-style pattern: frame.%04d.exr
            let re = Regex::new(r"%0(\d+)d")
                .map_err(|e| FrameError::Image(format!("Regex error: {}", e)))?;
            if let Some(caps) = re.captures(&pattern) {
                if let Some(m) = caps.get(1) {
                    seq.padding = m.as_str().parse::<usize>().unwrap_or(4);
                }
            }
            // Convert to a glob pattern for discovery
            let glob_pattern = re.replace_all(&pattern, "*").to_string();
            seq.init_from_glob(&glob_pattern)?;
        } else {
            // Single file or auto-detect sequence
            seq.init_from_file(&pattern)?;
        }

        Ok(seq)
    }

    /// Restore frames from metadata after deserialization (called by Cache)
    pub fn restore_frames(&mut self) {
        self.frames.clear();

        // Create unloaded Frame placeholders for each frame number
        for frame_num in self.start..=self.end {
            let path = Self::format_path(&self.pattern, frame_num, self.padding);
            self.frames.push(Frame::new_unloaded(PathBuf::from(path)));
        }
    }

    /// Get frame path by frame number (public API)
    pub fn get_frame_path(&self, frame_num: usize) -> PathBuf {
        PathBuf::from(Self::format_path(&self.pattern, frame_num, self.padding))
    }

    /// Format frame path from pattern and frame number
    /// pattern: "/path/frame.*.exr" or "/path/frame.%04d.exr"
    /// Returns: "/path/frame.0001.exr"
    fn format_path(pattern: &str, frame_num: usize, padding: usize) -> String {
        if pattern.contains('%') {
            // printf-style: frame.%04d.exr
            // Replace %0Nd with actual frame number
            let re = Regex::new(r"%0(\d+)d").unwrap();
            re.replace(pattern, &format!("{:0width$}", frame_num, width = padding))
                .to_string()
        } else if pattern.contains('*') {
            // glob-style: frame.*.exr
            pattern.replace('*', &format!("{:0width$}", frame_num, width = padding))
        } else {
            // Fallback: just return pattern (shouldn't happen in normal usage)
            pattern.to_string()
        }
    }

    /// Initialize from glob pattern
    fn init_from_glob(&mut self, pattern: &str) -> Result<(), FrameError> {
        let paths = glob::glob(pattern)
            .map_err(|e| FrameError::Image(format!("Glob error: {}", e)))?;

        let mut files: Vec<PathBuf> = paths.filter_map(Result::ok).collect();
        files.sort();

        if files.is_empty() {
            return Err(FrameError::Image(format!("No files match pattern: {}", pattern)));
        }

        // Extract frame numbers
        let re = Regex::new(r"(\d+)")
            .map_err(|e| FrameError::Image(format!("Regex error: {}", e)))?;
        let mut frame_map: HashMap<usize, PathBuf> = HashMap::new();

        for path in files {
            let stem = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            // Use last number in filename as frame number
            if let Some(last_match) = re.find_iter(stem).last() {
                if let Ok(num) = last_match.as_str().parse::<usize>() {
                    frame_map.insert(num, path);
                }
            }
        }

        if frame_map.is_empty() {
            return Err(FrameError::Image("No frame numbers found".to_string()));
        }

        // Determine range
        let mut frame_nums: Vec<usize> = frame_map.keys().copied().collect();
        frame_nums.sort();

        // Safe to unwrap: we checked frame_map is not empty above
        self.start = *frame_nums.first().expect("frame_nums should not be empty");
        self.end = *frame_nums.last().expect("frame_nums should not be empty");

        // Detect padding from first file
        if let Some(first_path) = frame_map.get(&self.start) {
            let stem = first_path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if let Some(last_match) = re.find_iter(stem).last() {
                self.padding = last_match.as_str().len();
            }
        }

        // Create frames
        for i in self.start..=self.end {
            let mut frame = Frame::new(self.xres, self.yres);
            if let Some(path) = frame_map.get(&i) {
                frame.set_file(path.clone());
            }
            self.frames.push(frame);
        }

        info!("Sequence: {} frames ({}-{}), padding={}",
              self.frames.len(), self.start, self.end, self.padding);

        Ok(())
    }

    /// Initialize from single file (detect sequence or single frame)
    fn init_from_file(&mut self, path: &str) -> Result<(), FrameError> {
        let path_buf = PathBuf::from(path);

        if !path_buf.exists() {
            return Err(FrameError::Image(format!("File not found: {}", path)));
        }

        // Try to find last digit group in filename
        let stem = path_buf.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Regex to find digit groups (will take last one)
        let re = Regex::new(r"(\d+)")
            .map_err(|e| FrameError::Image(format!("Regex error: {}", e)))?;

        if let Some(last_match) = re.find_iter(stem).last() {
            // Found digits → try to detect sequence
            let frame_num_str = last_match.as_str();
            self.padding = frame_num_str.len();

            // Replace digits with "*" to create pattern
            let pattern_stem = stem[..last_match.start()].to_string()
                + "*"
                + &stem[last_match.end()..];

            let ext = path_buf.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("exr");

            let dir = path_buf.parent()
                .unwrap_or_else(|| Path::new("."));

            let pattern_path = dir.join(format!("{}.{}", pattern_stem, ext));
            let pattern_str = pattern_path.to_string_lossy().to_string();

            // Glob with new pattern
            self.init_from_glob(&pattern_str)?;
            self.pattern = pattern_str;
        } else {
            // No digits → single static image
            self.start = 0;
            self.end = 0;
            self.padding = 0;
            self.pattern = path.to_string();

            let mut frame = Frame::new(self.xres, self.yres);
            frame.set_file(path_buf);
            self.frames.push(frame);

            info!("Single frame: {}", path);
        }

        Ok(())
    }

    /// Get frame with index wrapping/clamping
    pub fn idx(&self, i: isize, looping: bool) -> Option<&Frame> {
        if self.frames.is_empty() {
            return None;
        }

        let len = self.frames.len() as isize;
        let index = if looping {
            // Wrap around
            ((i % len) + len) % len
        } else {
            // Clamp
            i.clamp(0, len - 1)
        };

        self.frames.get(index as usize)
    }

    /// Get mutable frame
    pub fn idx_mut(&mut self, i: isize, looping: bool) -> Option<&mut Frame> {
        if self.frames.is_empty() {
            return None;
        }

        let len = self.frames.len() as isize;
        let index = if looping {
            ((i % len) + len) % len
        } else {
            i.clamp(0, len - 1)
        };

        self.frames.get_mut(index as usize)
    }

    /// Get frame count
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Get pattern
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Get range
    pub fn range(&self) -> (usize, usize) {
        (self.start, self.end)
    }

    /// Detect image sequences from files or directories
    ///
    /// **Why**: Auto-discover frame range and numbering pattern from drag-dropped paths
    ///
    /// **Used by**: Drag-drop handler, command-line args, playlist builder
    ///
    /// # Detection
    ///
    /// 1. If path is file: detect sequence from filename pattern
    /// 2. If path is dir: scan for all sequences (multiple per folder supported)
    /// 3. Parse frame numbers, build range (start..=end)
    /// 4. Extract resolution from first frame header
    /// 5. Build template for path generation (`render.####.exr`)
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<Sequence>)`: All sequences found (can detect multiple sequences)
    /// - `Err(FrameError)`: No valid sequences detected
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use playa::sequence::Sequence;
    /// # use std::path::PathBuf;
    /// // Single file: detects render.0001-0240.exr
    /// let seqs = Sequence::detect(vec![PathBuf::from("render.0001.exr")])?;
    ///
    /// // Directory: finds all sequences
    /// let seqs = Sequence::detect(vec![PathBuf::from("/renders/")])?;
    /// // → Multiple sequences: ["shot_A.####.exr", "shot_B.####.jpg"]
    /// # Ok::<(), playa::frame::FrameError>(())
    /// ```
    pub fn detect(paths: Vec<PathBuf>) -> Result<Vec<Self>, FrameError> {
        let mut sequences = Vec::new();
        let mut processed_patterns = std::collections::HashSet::new();

        for path in paths {
            if path.is_dir() {
                // Scan directory for sequences
                sequences.extend(Self::detect_in_dir(&path)?);
            } else if path.is_file() {
                // Detect sequence from single file
                let seq = Self::detect_from_file(&path)?;

                // Deduplicate by pattern
                if !processed_patterns.contains(&seq.pattern) {
                    processed_patterns.insert(seq.pattern.clone());
                    sequences.push(seq);
                }
            }
        }

        if sequences.is_empty() {
            return Err(FrameError::Image("No sequences detected".to_string()));
        }

        Ok(sequences)
    }

    /// Detect sequence from single file
    fn detect_from_file(path: &Path) -> Result<Self, FrameError> {
        // Get resolution from first file (header only)
        let (xres, yres) = Self::get_resolution(path)?;

        // Create sequence
        Self::new(path.to_string_lossy().to_string(), xres, yres, None, None)
    }

    /// Detect all sequences in directory
    fn detect_in_dir(dir: &Path) -> Result<Vec<Self>, FrameError> {
        let mut sequences = Vec::new();
        let mut grouped: HashMap<String, Vec<PathBuf>> = HashMap::new();

        // Group files by pattern
        let entries = std::fs::read_dir(dir)
            .map_err(|e| FrameError::Image(format!("Failed to read dir: {}", e)))?;

        let re = Regex::new(r"\d+")
            .map_err(|e| FrameError::Image(format!("Regex error: {}", e)))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Filter by extension
            let ext = path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            if !matches!(ext.to_lowercase().as_str(), "exr" | "png" | "jpg" | "jpeg" | "tif" | "tiff") {
                continue;
            }

            let stem = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");

            // Replace digits with placeholder
            let pattern_key = re.replace_all(stem, "####").to_string() + "." + ext;

            grouped.entry(pattern_key).or_default().push(path);
        }

        // Create sequence for each group
        for (_, mut files) in grouped {
            if files.is_empty() {
                continue;
            }

            files.sort();
            let first_file = &files[0];

            // Get resolution
            let (xres, yres) = Self::get_resolution(first_file)?;

            // Create sequence from first file
            let seq = Self::new(first_file.to_string_lossy().to_string(), xres, yres, None, None)?;
            sequences.push(seq);
        }

        Ok(sequences)
    }

    /// Get image resolution without loading full image (header only)
    fn get_resolution(path: &Path) -> Result<(usize, usize), FrameError> {
        let ext = path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "exr" => {
                use openexr::prelude::*;
                let file = RgbaInputFile::new(path, 1)
                    .map_err(|e| FrameError::Exr(e.to_string()))?;
                let header = file.header();
                let data_window = header.data_window::<[i32; 4]>();
                let width = (data_window[2] - data_window[0] + 1) as usize;
                let height = (data_window[3] - data_window[1] + 1) as usize;
                Ok((width, height))
            }
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "tga" => {
                let reader = image::ImageReader::open(path)
                    .map_err(|e| FrameError::Image(e.to_string()))?;
                let (width, height) = reader.into_dimensions()
                    .map_err(|e| FrameError::Image(e.to_string()))?;
                Ok((width as usize, height as usize))
            }
            _ => Err(FrameError::UnsupportedFormat(format!(".{}", ext))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idx_wrap() {
        let seq = Sequence {
            frames: vec![
                Frame::new(100, 100),
                Frame::new(100, 100),
                Frame::new(100, 100),
            ],
            pattern: "test".to_string(),
            start: 0,
            end: 2,
            padding: 1,
            xres: 100,
            yres: 100,
        };

        // Test wrapping
        assert!(seq.idx(-1, true).is_some());
        assert!(seq.idx(3, true).is_some());
        assert!(seq.idx(10, true).is_some());

        // Test clamping
        assert_eq!(seq.idx(-1, false).is_some(), true);
        assert_eq!(seq.idx(3, false).is_some(), true);
    }
}
