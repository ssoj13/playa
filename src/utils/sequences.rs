//! Image sequence detection utilities
//!
//! Detects image sequences from file paths and creates Comp objects in File mode

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use log::info;

use crate::entities::{Comp, AttrValue};
use crate::entities::frame::FrameError;
use crate::entities::loader::Loader;

/// Detect image sequences from a list of file paths
///
/// Groups files by sequence pattern and creates Comp (File mode) for each sequence
pub fn detect_sequences(paths: Vec<PathBuf>) -> Result<Vec<Comp>, FrameError> {
    let mut comps = Vec::new();

    for path in paths {
        // Try to detect if this is part of a sequence
        if let Some((prefix, _number, ext, padding)) = split_sequence_path(&path)? {
            // It's a sequence - find all files in this sequence
            let pattern = format!("{}*.{}", prefix, ext);

            match detect_sequence_from_pattern(&pattern, padding) {
                Ok(comp) => {
                    info!("Detected sequence: {} ({} frames)", pattern, comp.frame_count());
                    comps.push(comp);
                }
                Err(e) => {
                    info!("Failed to detect sequence for {}: {}", path.display(), e);
                    // Try as single file
                    if let Ok(comp) = create_single_file_comp(&path) {
                        comps.push(comp);
                    }
                }
            }
        } else {
            // Single file, not a sequence
            if let Ok(comp) = create_single_file_comp(&path) {
                comps.push(comp);
            }
        }
    }

    // Deduplicate comps by pattern
    let mut unique_comps: HashMap<String, Comp> = HashMap::new();
    for comp in comps {
        if let Some(mask) = &comp.file_mask {
            unique_comps.entry(mask.clone()).or_insert(comp);
        }
    }

    Ok(unique_comps.into_values().collect())
}

/// Detect sequence from glob pattern
fn detect_sequence_from_pattern(pattern: &str, padding: usize) -> Result<Comp, FrameError> {
    let paths = glob_paths(pattern)?;
    if paths.is_empty() {
        return Err(FrameError::Image(format!("No files matched pattern: {}", pattern)));
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
    let width = attrs.get_u32("width").unwrap_or(0) as usize;
    let height = attrs.get_u32("height").unwrap_or(0) as usize;

    // Create Comp with File mode
    let file_mask = format!("{}*.{}", prefix, ext);
    let mut comp = Comp::new_file_comp(file_mask.clone(), min_frame, max_frame, 24.0);

    // Store dimensions and padding
    comp.attrs.set("width", AttrValue::UInt(width as u32));
    comp.attrs.set("height", AttrValue::UInt(height as u32));
    comp.attrs.set("padding", AttrValue::UInt(padding as u32));

    // Set name from first file
    if let Some(filename) = first_path.file_stem().and_then(|s| s.to_str()) {
        comp.attrs.set("name", AttrValue::Str(filename.to_string()));
    }

    info!("Created sequence comp: {} ({} frames, {}x{})",
          file_mask, frames_data.len(), width, height);

    Ok(comp)
}

/// Create Comp from single file
fn create_single_file_comp(path: &Path) -> Result<Comp, FrameError> {
    let attrs = Loader::header(path)?;
    let width = attrs.get_u32("width").unwrap_or(0) as usize;
    let height = attrs.get_u32("height").unwrap_or(0) as usize;

    let file_mask = path.to_string_lossy().to_string();
    let mut comp = Comp::new_file_comp(file_mask.clone(), 0, 0, 24.0);

    comp.attrs.set("width", AttrValue::UInt(width as u32));
    comp.attrs.set("height", AttrValue::UInt(height as u32));

    if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
        comp.attrs.set("name", AttrValue::Str(filename.to_string()));
    }

    info!("Created single file comp: {} ({}x{})", file_mask, width, height);

    Ok(comp)
}

/// Expand a glob pattern into a list of paths
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>, FrameError> {
    let mut paths = Vec::new();
    for entry in glob::glob(pattern)
        .map_err(|e| FrameError::Image(format!("Glob error for pattern {}: {}", pattern, e)))?
    {
        match entry {
            Ok(path) => paths.push(path),
            Err(e) => return Err(FrameError::Image(format!("Glob entry error: {}", e))),
        }
    }
    Ok(paths)
}

/// Split a sequence filename into (prefix, number, ext, padding)
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
