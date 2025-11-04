use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;

/// Progress tracking for frame loading with separate log line
#[derive(Debug)]
pub struct LoadProgress {
    log_line: ProgressBar,
    progress_bar: ProgressBar,
    loaded_frames: HashSet<(usize, usize)>,
}

impl LoadProgress {
    /// Create new progress tracker with separate log and progress lines
    pub fn new(total_frames: usize) -> Self {
        let multi = MultiProgress::new();

        // Top line for log messages (non-scrolling, always visible)
        let log_line = multi.add(ProgressBar::new_spinner());
        log_line.set_style(
            ProgressStyle::default_spinner()
                .template("{msg}")
                .unwrap()
        );
        log_line.set_message("Loading frames...");

        // Bottom line for progress bar
        let progress_bar = multi.add(ProgressBar::new(total_frames as u64));
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40.cyan/blue}] {pos}/{len} frames ({percent}%) | {msg}")
                .unwrap()
                .progress_chars("█▓░")
        );

        Self {
            log_line,
            progress_bar,
            loaded_frames: HashSet::new(),
        }
    }

    /// Update progress with loaded frame
    pub fn update(&mut self, seq_idx: usize, frame_idx: usize) {
        let key = (seq_idx, frame_idx);
        if self.loaded_frames.insert(key) {
            self.progress_bar.set_position(self.loaded_frames.len() as u64);
        }
    }

    /// Set total frames (for when sequences are added/removed)
    pub fn set_total(&mut self, total: usize) {
        self.progress_bar.set_length(total as u64);
    }

    /// Clear all progress
    pub fn clear(&mut self) {
        self.loaded_frames.clear();
        self.progress_bar.set_position(0);
        self.log_line.set_message("Ready");
        self.progress_bar.set_message("");
    }
}
