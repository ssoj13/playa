//! Playback engine with frame-accurate timing and JKL controls
//!
//! **Why**: Professional playback requires:
//! - Frame-accurate timing (not wall-clock)
//! - JKL shuttle controls (J=reverse, K=pause, L=forward)
//! - Dropped frame handling for heavy sequences
//!
//! **Used by**: Keyboard handler (JKL/Space), UI (timeline position)
//!
//! # Timing Model
//!
//! FPS-based: Each frame has fixed duration (1/fps seconds).
//! No dropped frames from timer - we advance by frame count.
//! If frame not loaded: display last good frame (no black flash).
//!
//! # JKL Controls
//!
//! - **J**: Reverse at 1x, 2x, 4x (tap to increase speed)
//! - **K**: Pause (hold K+L/J for slow motion)
//! - **L**: Forward at 1x, 2x, 4x (tap to increase speed)
//! - **Space**: Play/Pause toggle
//!
//! # Playback Loop
//!
//! `update()` called at 60Hz, advances frame index based on FPS.
//! Handles sequence boundaries (loop or stop at end).

use log::{debug, info};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use crate::cache::{Cache, CacheMessage};
use crate::frame::Frame;

/// FPS presets for jog/shuttle control
const FPS_PRESETS: &[f32] = &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0];

/// Playback state manager with new architecture
pub struct Player {
    pub cache: Cache,
    pub is_playing: bool,
    pub fps: f32,
    pub loop_enabled: bool,
    pub play_direction: f32, // 1.0 forward, -1.0 backward
    last_frame_time: Option<Instant>,
    pub selected_seq_idx: Option<usize>, // Currently selected sequence in playlist
}

impl Player {
    /// Create new player with empty cache
    /// Returns (Player, UI message receiver, Path sender)
    pub fn new() -> (Self, mpsc::Receiver<CacheMessage>, mpsc::Sender<PathBuf>) {
        info!("Player initialized with new architecture");

        let (cache, ui_rx, path_tx) = Cache::new(0.75); // 75% of available memory

        let player = Self {
            cache,
            is_playing: false,
            fps: 24.0,
            loop_enabled: true,
            play_direction: 1.0,
            last_frame_time: None,
            selected_seq_idx: None,
        };

        (player, ui_rx, path_tx)
    }

    /// Get current frame from cache
    pub fn get_current_frame(&mut self) -> Option<&Frame> {
        let frame_idx = self.cache.frame();
        self.cache.get_frame(frame_idx)
    }

    /// Update playback state
    pub fn update(&mut self) {
        if !self.is_playing || self.cache.total_frames() == 0 {
            return;
        }

        let now = Instant::now();

        if let Some(last_time) = self.last_frame_time {
            let elapsed = now.duration_since(last_time).as_secs_f32();
            let frame_duration = 1.0 / self.fps;

            if elapsed >= frame_duration {
                self.advance_frame();
                self.last_frame_time = Some(now);
            }
        } else {
            self.last_frame_time = Some(now);
        }
    }

    /// Advance to next frame
    fn advance_frame(&mut self) {
        let total_frames = self.cache.total_frames();
        if total_frames == 0 {
            return;
        }

        let current = self.cache.frame();
        let (_, global_end) = self.cache.range();

        if self.play_direction > 0.0 {
            // Forward
            let next = current + 1;
            if next > global_end {
                if self.loop_enabled {
                    debug!("Frame loop: {} -> 0", current);
                    self.cache.set_frame(0);
                } else {
                    debug!("Reached end, stopping");
                    self.cache.set_frame(global_end);
                    self.is_playing = false;
                }
            } else {
                self.cache.set_frame(next);
            }
        } else {
            // Backward
            if current == 0 {
                if self.loop_enabled {
                    debug!("Frame loop: 0 -> {}", global_end);
                    self.cache.set_frame(global_end);
                } else {
                    debug!("Reached start, stopping");
                    self.is_playing = false;
                }
            } else {
                self.cache.set_frame(current - 1);
            }
        }
    }

    /// Toggle play/pause
    pub fn toggle_play_pause(&mut self) {
        self.is_playing = !self.is_playing;
        if self.is_playing {
            debug!("Playback started at frame {}", self.cache.frame());
            self.last_frame_time = Some(Instant::now());
            // Start preloading when playback begins
            self.cache.signal_preload();
        } else {
            debug!("Playback paused at frame {}", self.cache.frame());
            self.last_frame_time = None;
        }
    }

    /// Rewind to start
    pub fn to_start(&mut self) {
        debug!("Rewinding to frame 0");
        self.cache.set_frame(0);
        self.last_frame_time = None;
        self.cache.signal_preload();
    }

    /// Skip to end
    pub fn to_end(&mut self) {
        let (_, global_end) = self.cache.range();
        debug!("Skipping to end: frame {}", global_end);
        self.cache.set_frame(global_end);
        self.last_frame_time = None;
        self.cache.signal_preload();
    }

    /// Set current frame
    pub fn set_frame(&mut self, frame: usize) {
        let (_, global_end) = self.cache.range();
        let clamped = frame.min(global_end);
        self.cache.set_frame(clamped);
        self.last_frame_time = None;

        // Start preloading from new position
        self.cache.signal_preload();
    }

    /// Get current frame index
    pub fn current_frame(&self) -> usize {
        self.cache.frame()
    }

    /// Get total frames
    pub fn total_frames(&self) -> usize {
        self.cache.total_frames()
    }

    /// Jog forward (L key)
    pub fn jog_forward(&mut self) {
        if !self.is_playing {
            self.play_direction = 1.0;
            self.is_playing = true;
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction < 0.0 {
            self.play_direction = 1.0;
        } else {
            self.increase_fps();
        }
    }

    /// Jog backward (J key)
    pub fn jog_backward(&mut self) {
        if !self.is_playing {
            self.play_direction = -1.0;
            self.is_playing = true;
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction > 0.0 {
            self.play_direction = -1.0;
        } else {
            self.increase_fps();
        }
    }

    /// Increase FPS to next preset
    fn increase_fps(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f >= self.fps) {
            if idx + 1 < FPS_PRESETS.len() {
                self.fps = FPS_PRESETS[idx + 1];
            }
        }
    }

    /// Decrease FPS to previous preset
    fn decrease_fps(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().rposition(|&f| f < self.fps) {
            self.fps = FPS_PRESETS[idx];
        } else {
            self.fps = FPS_PRESETS[0];
        }
    }

    /// Stop or decrease FPS (K key)
    pub fn stop_or_decrease_fps(&mut self) {
        if self.is_playing {
            debug!("Playback stopped at frame {}", self.cache.frame());
            self.is_playing = false;
            self.last_frame_time = None;
        } else {
            self.decrease_fps();
            debug!("FPS decreased to {}", self.fps);
        }
    }

    /// Reset settings
    pub fn reset_settings(&mut self) {
        self.fps = 24.0;
        self.loop_enabled = true;
        info!("Player settings reset");
    }
}

impl Default for Player {
    fn default() -> Self {
        let (player, _rx, _path_tx) = Self::new();
        player
    }
}
