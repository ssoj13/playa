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
    pub fps_base: f32,       // Base FPS (persistent setting)
    pub fps_play: f32,       // Current playback FPS (temporary, resets on stop)
    pub loop_enabled: bool,
    pub play_direction: f32, // 1.0 forward, -1.0 backward
    last_frame_time: Option<Instant>,
    pub selected_seq_idx: Option<usize>, // Currently selected sequence in playlist
}

impl Player {
    /// Create new player with empty cache and defaults
    /// Returns (Player, UI message receiver)
    pub fn new() -> (Self, mpsc::Receiver<CacheMessage>) {
        Self::new_with_config(0.75, None)
    }

    /// Create new player with configurable memory budget and worker count
    pub fn new_with_config(
        max_mem_fraction: f64,
        workers: Option<usize>,
    ) -> (Self, mpsc::Receiver<CacheMessage>) {
        info!("Player initialized with new architecture");

        let (cache, ui_rx) = Cache::new(max_mem_fraction, workers); // configurable memory/workers

        let player = Self {
            cache,
            is_playing: false,
            fps_base: 24.0,
            fps_play: 24.0,
            loop_enabled: true,
            play_direction: 1.0,
            last_frame_time: None,
            selected_seq_idx: None,
        };

        (player, ui_rx)
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
            let frame_duration = 1.0 / self.fps_play;

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
        let (play_start, play_end) = self.cache.get_play_range();

        if self.play_direction > 0.0 {
            // Forward
            let next = current + 1;
            if next > play_end {
                if self.loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_start);
                    self.cache.set_frame(play_start);
                } else {
                    debug!("Reached play range end, stopping");
                    self.cache.set_frame(play_end);
                    self.is_playing = false;
                }
            } else {
                self.cache.set_frame(next);
            }
        } else {
            // Backward
            if current <= play_start {
                if self.loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_end);
                    self.cache.set_frame(play_end);
                } else {
                    debug!("Reached play range start, stopping");
                    self.is_playing = false;
                }
            } else {
                self.cache.set_frame(current - 1);
            }
        }
    }

    /// Toggle play/pause (Space, K, ArrowUp)
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
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Stop playback (always stops, doesn't toggle)
    pub fn stop(&mut self) {
        if self.is_playing {
            self.is_playing = false;
            debug!("Playback stopped at frame {}", self.cache.frame());
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
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
    #[inline]
    pub fn current_frame(&self) -> usize {
        self.cache.frame()
    }

    /// Get total frames
    pub fn total_frames(&self) -> usize {
        self.cache.total_frames()
    }

    /// Jog forward (L, >, ArrowRight)
    pub fn jog_forward(&mut self) {
        if !self.is_playing {
            self.play_direction = 1.0;
            self.is_playing = true;
            self.fps_play = self.fps_base; // Start with base FPS
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction < 0.0 {
            self.play_direction = 1.0; // Change direction
            self.fps_play = self.fps_base; // Reset on direction change
        } else {
            self.increase_fps_play(); // Increase play speed
        }
    }

    /// Jog backward (J, <, ArrowLeft)
    pub fn jog_backward(&mut self) {
        if !self.is_playing {
            self.play_direction = -1.0;
            self.is_playing = true;
            self.fps_play = self.fps_base; // Start with base FPS
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction > 0.0 {
            self.play_direction = -1.0; // Change direction
            self.fps_play = self.fps_base; // Reset on direction change
        } else {
            self.increase_fps_play(); // Increase play speed
        }
    }

    /// Increase base FPS to next preset (-/+ keys, Keypad)
    pub fn increase_fps_base(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f >= self.fps_base) {
            if idx + 1 < FPS_PRESETS.len() {
                self.fps_base = FPS_PRESETS[idx + 1];
                // If not playing, update fps_play too
                if !self.is_playing {
                    self.fps_play = self.fps_base;
                }
                debug!("Base FPS increased to {}", self.fps_base);
            }
        }
    }

    /// Decrease base FPS to previous preset (-/+ keys, Keypad)
    pub fn decrease_fps_base(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().rposition(|&f| f <= self.fps_base) {
            if idx > 0 {
                self.fps_base = FPS_PRESETS[idx - 1];
                // If not playing, update fps_play too
                if !self.is_playing {
                    self.fps_play = self.fps_base;
                }
                debug!("Base FPS decreased to {}", self.fps_base);
            }
        }
    }

    /// Increase play FPS to next preset (J/L when playing)
    fn increase_fps_play(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f >= self.fps_play) {
            if idx + 1 < FPS_PRESETS.len() {
                self.fps_play = FPS_PRESETS[idx + 1];
                debug!("Play FPS increased to {}", self.fps_play);
            }
        }
    }

    /// Decrease play FPS to previous preset (ArrowDown when playing)
    pub fn decrease_fps_play(&mut self) {
        if self.is_playing {
            if let Some(idx) = FPS_PRESETS.iter().rposition(|&f| f <= self.fps_play) {
                if idx > 0 {
                    self.fps_play = FPS_PRESETS[idx - 1];
                    debug!("Play FPS decreased to {}", self.fps_play);
                }
            }
        }
    }

    /// Jump to next sequence start (] key)
    /// If within sequence -> jump to next sequence start
    /// If on last sequence -> jump to end of range
    /// If at end and loop enabled -> jump to first sequence start
    pub fn jump_next_sequence(&mut self) {
        let sequences = self.cache.sequences();
        if sequences.is_empty() {
            return;
        }

        let (global_start, global_end) = self.cache.range();
        let current_frame = self.cache.frame();

        // Check if we're already at the end
        if current_frame >= global_end {
            if self.loop_enabled {
                // Loop to first sequence start
                self.cache.set_frame(global_start);
                debug!("Looped from end to start: frame {}", global_start);
            }
            // If loop disabled, stay at end
        } else if let Some((seq_idx, _local_frame)) = self.cache.current_sequence() {
            // We're inside a sequence
            if seq_idx + 1 < sequences.len() {
                // Jump to start of next sequence
                if let Some(next_start) = self.cache.local_to_global(seq_idx + 1, 0) {
                    self.cache.set_frame(next_start);
                    debug!("Jumped to next sequence start: frame {}", next_start);
                }
            } else {
                // We're on last sequence, jump to end
                self.cache.set_frame(global_end);
                debug!("Jumped to end: frame {}", global_end);
            }
        }

        self.last_frame_time = None;
        self.cache.signal_preload();
    }

    /// Jump to previous sequence start ([ key)
    /// If within sequence -> jump to current sequence start
    /// If already at current sequence start -> jump to previous sequence start
    /// If on first sequence start and loop enabled -> jump to end
    pub fn jump_prev_sequence(&mut self) {
        let sequences = self.cache.sequences();
        if sequences.is_empty() {
            return;
        }

        let (_, global_end) = self.cache.range();
        let current_frame = self.cache.frame();

        if let Some((seq_idx, local_frame)) = self.cache.current_sequence() {
            // We're inside a sequence
            if local_frame == 0 {
                // Already at sequence start, jump to previous sequence
                if seq_idx > 0 {
                    if let Some(prev_start) = self.cache.local_to_global(seq_idx - 1, 0) {
                        self.cache.set_frame(prev_start);
                        debug!("Jumped to previous sequence start: frame {}", prev_start);
                    }
                } else if self.loop_enabled {
                    // We're at first sequence start, loop to end
                    self.cache.set_frame(global_end);
                    debug!("Looped to end: frame {}", global_end);
                }
            } else {
                // Not at sequence start, jump to current sequence start
                if let Some(cur_start) = self.cache.local_to_global(seq_idx, 0) {
                    self.cache.set_frame(cur_start);
                    debug!("Jumped to current sequence start: frame {}", cur_start);
                }
            }
        } else if current_frame >= global_end && self.loop_enabled {
            // We're at the end, position at end
            self.cache.set_frame(global_end);
            debug!("At end, positioned at frame {}", global_end);
        }

        self.last_frame_time = None;
        self.cache.signal_preload();
    }

    /// Reset settings
    pub fn reset_settings(&mut self) {
        self.fps_base = 24.0;
        self.fps_play = 24.0;
        self.loop_enabled = true;
        info!("Player settings reset");
    }
}

impl Default for Player {
    fn default() -> Self {
        let (player, _rx) = Self::new();
        player
    }
}
