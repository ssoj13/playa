use std::collections::HashSet;

/// Lightweight progress tracking for frame loading (GUI-only)
#[derive(Debug)]
pub struct LoadProgress {
    loaded_frames: HashSet<(usize, usize)>,
    total_frames: usize,
}

impl LoadProgress {
    /// Create new progress tracker
    pub fn new(total_frames: usize) -> Self {
        Self {
            loaded_frames: HashSet::new(),
            total_frames,
        }
    }

    /// Update progress with loaded frame
    pub fn update(&mut self, seq_idx: usize, frame_idx: usize) {
        let key = (seq_idx, frame_idx);
        self.loaded_frames.insert(key);
    }

    /// Set total frames (for when sequences are added/removed)
    pub fn set_total(&mut self, total: usize) {
        self.total_frames = total;
    }

    /// Clear all progress
    pub fn clear(&mut self) {
        self.loaded_frames.clear();
        self.total_frames = 0;
    }
}
