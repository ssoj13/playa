//! Clip: image sequence on disk.
//!
//! This is the old `Sequence`, renamed to `Clip`.

use log::info;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::exr::{ExrImpl, ExrLoader};
use crate::frame::{Frame, FrameError};
use crate::utils::media;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    /// Stable identifier inside Project / MediaPool
    pub uuid: String,

    #[serde(skip)]
    frames: Vec<Frame>,
    pattern: String, // "c:/temp/seq1/aaa.*.exr"
    start: usize,    // first frame number
    end: usize,      // last frame number
    padding: usize,  // number of digits (4 for "0001")
    xres: usize,
    yres: usize,
    pub attrs: Attrs,
}

fn gen_clip_uuid(pattern: &str, start: usize, end: usize) -> String {
    format!("clip:{}:{}:{}", pattern, start, end)
}

impl Clip {
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

        let mut clip = Self {
            uuid: gen_clip_uuid(&pattern, start_val, end_val),
            frames: Vec::new(),
            pattern: pattern.clone(),
            start: start_val,
            end: end_val,
            padding: 4,
            xres,
            yres,
            attrs: Attrs::new(),
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
                clip.padding = m.as_str().parse::<usize>().unwrap_or(4);
            }
            // Convert to a glob pattern for discovery
            let glob_pattern = re.replace_all(&pattern, "*").to_string();
            clip.init_from_glob(&glob_pattern)?;
        } else {
            // Single file or auto-detect sequence
            clip.init_from_file(&pattern)?;
        }

        // Basic metadata for serialization
        clip.attrs.set("pattern", AttrValue::Str(clip.pattern.clone()));
        clip.attrs
            .set("xres", AttrValue::UInt(clip.xres as u32));
        clip.attrs
            .set("yres", AttrValue::UInt(clip.yres as u32));
        clip.attrs
            .set("start", AttrValue::UInt(clip.start as u32));
        clip.attrs
            .set("end", AttrValue::UInt(clip.end as u32));

        Ok(clip)
    }

    /// Helper for tests: create clip from in-memory frames.
    pub fn from_frames(frames: Vec<Frame>, pattern: String, xres: usize, yres: usize) -> Self {
        let end = if frames.is_empty() { 0 } else { frames.len() - 1 };
        let mut clip = Self {
            uuid: gen_clip_uuid(&pattern, 0, end),
            frames,
            pattern,
            start: 0,
            end,
            padding: 4,
            xres,
            yres,
            attrs: Attrs::new(),
        };

        clip.attrs
            .set("pattern", AttrValue::Str(clip.pattern.clone()));
        clip.attrs
            .set("xres", AttrValue::UInt(clip.xres as u32));
        clip.attrs
            .set("yres", AttrValue::UInt(clip.yres as u32));
        clip.attrs
            .set("start", AttrValue::UInt(clip.start as u32));
        clip.attrs
            .set("end", AttrValue::UInt(clip.end as u32));

        clip
    }

    /// Restore frames from metadata after deserialization
    pub fn restore_frames(&mut self) {
        self.frames.clear();

        for frame_num in self.start..=self.end {
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
        (self.start, self.end)
    }

    pub fn len(&self) -> usize {
        if self.end >= self.start {
            self.end - self.start + 1
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

    fn frame_path(&self, frame_num: usize) -> PathBuf {
        if self.pattern.contains('*') {
            let number = format!("{:0width$}", frame_num, width = self.padding);
            self.pattern.replace('*', &number).into()
        } else if self.pattern.contains("####") {
            let number = format!("{:0width$}", frame_num, width = self.padding);
            self.pattern.replace("####", &number).into()
        } else {
            let path = Path::new(&self.pattern);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("frame");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("exr");
            let number = format!("{:0width$}", frame_num, width = self.padding);
            let mut name = String::with_capacity(stem.len() + 1 + self.padding + ext.len() + 1);
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
        let paths = media::glob_paths(pattern)?;
        if paths.is_empty() {
            return Err(FrameError::Image(format!(
                "No files matched pattern: {}",
                pattern
            )));
        }

        // Group by (prefix, ext)
        let mut groups: HashMap<(String, String), Vec<(usize, PathBuf)>> = HashMap::new();

        for path in paths {
            if let Some((prefix, number, ext)) = media::split_sequence_path(&path)? {
                let key = (prefix, ext);
                groups.entry(key).or_default().push((number, path));
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
            .fold((usize::MAX, 0usize), |(min_f, max_f), (num, _)| {
                (min_f.min(*num), max_f.max(*num))
            });

        self.start = min_frame;
        self.end = max_frame;
        self.padding = frames
            .first()
            .map(|(num, _)| num.to_string().len())
            .unwrap_or(4);
        self.pattern = format!("{}*.{}", prefix, ext);

        self.frames.clear();
        for frame_num in self.start..=self.end {
            let path = self.frame_path(frame_num);
            self.frames.push(Frame::new_unloaded(path));
        }

        info!(
            "Detected clip: {} ({}..{}, {} frames)",
            self.pattern,
            self.start,
            self.end,
            self.len()
        );

        Ok(())
    }

    fn init_from_file(&mut self, path_str: &str) -> Result<(), FrameError> {
        let path = PathBuf::from(path_str);
        let (prefix, number, ext) = media::split_sequence_path(&path)?
            .ok_or_else(|| FrameError::Image(format!("Not a sequence file: {}", path_str)))?;

        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut frames: Vec<(usize, PathBuf)> = Vec::new();

        for entry in std::fs::read_dir(dir)
            .map_err(|e| FrameError::Image(format!("Failed to read dir: {}", e)))?
        {
            let entry = entry.map_err(|e| FrameError::Image(format!("Dir entry error: {}", e)))?;
            let p = entry.path();
            if let Some((pfx, num, ext2)) = media::split_sequence_path(&p)? {
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
        self.start = frames.first().map(|(n, _)| *n).unwrap_or(number);
        self.end = frames.last().map(|(n, _)| *n).unwrap_or(number);
        self.padding = number.to_string().len();
        self.pattern = format!("{}*.{}", prefix, ext);

        self.frames.clear();
        for (_, p) in frames {
            self.frames.push(Frame::new_unloaded(p));
        }

        info!(
            "Detected clip from file {}: {} ({}..{}, {} frames)",
            path_str,
            self.pattern,
            self.start,
            self.end,
            self.len()
        );

        Ok(())
    }
}
