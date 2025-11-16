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
use std::sync::Arc;
use std::time::Instant;
use crate::clip::Clip;
use crate::comp::Comp;
use crate::frame::Frame;
use crate::layer::Layer;
use crate::project::Project;

/// FPS presets for jog/shuttle control
const FPS_PRESETS: &[f32] = &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0];

/// Frame step size for Shift+Arrow and Shift+PageUp/PageDown
pub const FRAME_JUMP_STEP: i32 = 25;

/// Playback state manager with new architecture
pub struct Player {
    pub project: Project,
    pub active_comp: Option<String>, // UUID of active comp
    pub current_frame: usize,
    pub is_playing: bool,
    pub fps_base: f32,       // Base FPS (persistent setting)
    pub fps_play: f32,       // Current playback FPS (temporary, resets on stop)
    pub loop_enabled: bool,
    pub play_direction: f32, // 1.0 forward, -1.0 backward
    last_frame_time: Option<Instant>,
    /// Index of selected clip in Project.order_clips (playlist)
    pub selected_seq_idx: Option<usize>,
}

impl Player {
    /// Create new player with empty project and defaults
    pub fn new() -> Self {
        info!("Player initialized with Comp-based architecture");

        Self {
            project: Project::new(),
            active_comp: None,
            current_frame: 0,
            is_playing: false,
            fps_base: 24.0,
            fps_play: 24.0,
            loop_enabled: true,
            play_direction: 1.0,
            last_frame_time: None,
            selected_seq_idx: None,
        }
    }

    fn active_comp_mut(&mut self) -> Option<&mut Comp> {
        if let Some(ref uuid) = self.active_comp {
            self.project.comps.get_mut(uuid)
        } else {
            None
        }
    }

    fn active_comp(&self) -> Option<&Comp> {
        if let Some(ref uuid) = self.active_comp {
            self.project.comps.get(uuid)
        } else {
            None
        }
    }

    /// Get total frames of active comp
    pub fn total_frames(&self) -> usize {
        self.active_comp().map(|c| c.total_frames()).unwrap_or(0)
    }

    /// Get current play range of active comp (start, end), or (0, 0) if none.
    pub fn play_range(&self) -> (usize, usize) {
        if let Some(comp) = self.active_comp() {
            comp.play_range()
        } else {
            (0, 0)
        }
    }

    /// Set play range of active comp (clamped to total_frames).
    pub fn set_play_range(&mut self, start: usize, end: usize) {
        if let Some(comp) = self.active_comp_mut() {
            let total = comp.total_frames();
            if total == 0 {
                return;
            }
            let max_idx = total.saturating_sub(1);
            let clamped_start = start.min(max_idx);
            let clamped_end = end.min(max_idx);
            comp.set_play_range(clamped_start, clamped_end);
            // Ensure current_frame lies inside new range
            if self.current_frame < clamped_start || self.current_frame > clamped_end {
                self.current_frame = clamped_start;
            }
        }
    }

    /// Reset play range of active comp to its full range.
    pub fn reset_play_range(&mut self) {
        if let Some(comp) = self.active_comp_mut() {
            comp.reset_play_range();
            self.current_frame = comp.play_range().0;
        }
    }

    /// Get current frame as owned Frame (Composed)
    pub fn get_current_frame(&mut self) -> Option<Frame> {
        let frame_idx = self.current_frame;
        self.active_comp_mut()?.get_frame(frame_idx)
    }

    /// Append detected clip to project playlist.
    pub fn append_clip(&mut self, clip: Clip) {
        let uuid = if clip.uuid.is_empty() {
            // Fallback: generate UUID from pattern/range
            format!(
                "clip:{}",
                clip.pattern()
            )
        } else {
            clip.uuid.clone()
        };

        // Insert clip into project
        self.project.clips.insert(uuid.clone(), clip);
        self.project.order_clips.push(uuid.clone());

        // If no active comp yet, create one for this clip
        if self.active_comp.is_none() {
            self.set_active_clip_by_uuid(&uuid);
            self.selected_seq_idx = Some(0);
        }
    }

    /// Helper: rebuild active comp for given clip UUID.
    fn set_active_clip_by_uuid(&mut self, clip_uuid: &str) {
        let clip = match self.project.clips.get(clip_uuid) {
            Some(c) => c.clone(),
            None => return,
        };

        let clip_arc = Arc::new(clip);
        let layer = Layer::new(Arc::clone(&clip_arc));

        let total_frames = clip_arc.len();
        let end = total_frames.saturating_sub(1);
        let mut comp = Comp::new("Main", 0, end, self.fps_base);
        comp.layers.push(layer);

        let comp_uuid = comp.uuid.clone();
        self.project.comps.insert(comp_uuid.clone(), comp);
        if !self.project.order_comps.contains(&comp_uuid) {
            self.project.order_comps.push(comp_uuid.clone());
        }

        self.active_comp = Some(comp_uuid);
        self.current_frame = 0;
    }

    /// Update playback state
    pub fn update(&mut self) {
        if !self.is_playing || self.total_frames() == 0 {
            return;
        }

        // Ensure play_fps is not lower than base_fps
        // base_fps acts as a "floor" that pushes play_fps from below
        if self.fps_play < self.fps_base {
            self.fps_play = self.fps_base;
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
        let total_frames = self.total_frames();
        if total_frames == 0 {
            return;
        }

        let current = self.current_frame;
        let (play_start, play_end) = (0, total_frames.saturating_sub(1));

        if self.play_direction > 0.0 {
            // Forward
            let next = current + 1;
            if next > play_end {
                if self.loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_start);
                    self.current_frame = play_start;
                } else {
                    debug!("Reached play range end, stopping");
                    self.current_frame = play_end;
                    self.is_playing = false;
                }
            } else {
                self.current_frame = next;
            }
        } else {
            // Backward
            if current <= play_start {
                if self.loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_end);
                    self.current_frame = play_end;
                } else {
                    debug!("Reached play range start, stopping");
                    self.is_playing = false;
                }
            } else {
                self.current_frame = current - 1;
            }
        }
    }

    /// Toggle play/pause (Space, K, ArrowUp)
    pub fn toggle_play_pause(&mut self) {
        self.is_playing = !self.is_playing;
        if self.is_playing {
            debug!("Playback started at frame {}", self.current_frame);
            self.last_frame_time = Some(Instant::now());
        } else {
            debug!("Playback paused at frame {}", self.current_frame);
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Stop playback (always stops, doesn't toggle)
    pub fn stop(&mut self) {
        if self.is_playing {
            self.is_playing = false;
            debug!("Playback stopped at frame {}", self.current_frame);
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Rewind to start
    pub fn to_start(&mut self) {
        let (start, _) = self.play_range();
        debug!("Rewinding to frame {}", start);
        self.current_frame = start;
        self.last_frame_time = None;
    }

    /// Skip to end
    pub fn to_end(&mut self) {
        let (_, end) = self.play_range();
        debug!("Skipping to end: frame {}", end);
        self.current_frame = end;
        self.last_frame_time = None;
    }

    /// Set current frame
    pub fn set_frame(&mut self, frame: usize) {
        let (start, end) = self.play_range();
        let clamped = frame.clamp(start, end);
        self.current_frame = clamped;
        self.last_frame_time = None;
    }

    /// Step by N frames (positive = forward, negative = backward)
    /// Respects play range and loop_enabled setting
    pub fn step(&mut self, count: i32) {
        if count == 0 {
            return;
        }

        let current = self.current_frame;
        let (play_start, play_end) = self.play_range();

        // Calculate target frame with saturating arithmetic
        let target = if count > 0 {
            current.saturating_add(count as usize)
        } else {
            current.saturating_sub(count.unsigned_abs() as usize)
        };

        // Apply loop/clamp logic based on loop_enabled
        let final_frame = if target > play_end {
            if self.loop_enabled {
                // Loop: wrap around to play_start
                let overflow = target - play_end;
                let range_size = play_end - play_start + 1;
                play_start + ((overflow - 1) % range_size)
            } else {
                // Clamp to play_end
                play_end
            }
        } else if target < play_start {
            if self.loop_enabled {
                // Loop: wrap around to play_end
                let underflow = play_start - target;
                let range_size = play_end - play_start + 1;
                play_end - ((underflow - 1) % range_size)
            } else {
                // Clamp to play_start
                play_start
            }
        } else {
            target
        };

        self.current_frame = final_frame;
    }

    /// Get current frame index in play range
    #[inline]
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    /// Internal helper to start jogging in the specified direction
    fn start_jog(&mut self, direction: f32) {
        if !self.is_playing {
            // Start playing in specified direction
            self.play_direction = direction;
            self.is_playing = true;
            self.fps_play = self.fps_base; // Start with base FPS
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction.signum() != direction.signum() {
            // Change direction
            self.play_direction = direction;
            self.fps_play = self.fps_base; // Reset on direction change
        } else {
            // Already playing in same direction, increase speed
            self.increase_fps_play();
        }
    }

    /// Jog forward (L, >, ArrowRight)
    pub fn jog_forward(&mut self) {
        self.start_jog(1.0);
    }

    /// Jog backward (J, <, ArrowLeft)
    pub fn jog_backward(&mut self) {
        self.start_jog(-1.0);
    }

    /// Increase base FPS to next preset (-/+ keys, Keypad)
    pub fn increase_fps_base(&mut self) {
        // Find first preset strictly greater than current FPS
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f > self.fps_base) {
            self.fps_base = FPS_PRESETS[idx];
            // If not playing, update fps_play too
            if !self.is_playing {
                self.fps_play = self.fps_base;
            }
            debug!("Base FPS increased to {}", self.fps_base);
        }
    }

    /// Decrease base FPS to previous preset (-/+ keys, Keypad)
    pub fn decrease_fps_base(&mut self) {
        // Find last preset strictly less than current FPS
        if let Some(idx) = FPS_PRESETS.iter().rposition(|&f| f < self.fps_base) {
            self.fps_base = FPS_PRESETS[idx];
            // If not playing, update fps_play too
            if !self.is_playing {
                self.fps_play = self.fps_base;
            }
            debug!("Base FPS decreased to {}", self.fps_base);
        }
    }

    /// Increase play FPS to next preset (J/L when playing)
    fn increase_fps_play(&mut self) {
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f >= self.fps_play)
            && idx + 1 < FPS_PRESETS.len() {
                self.fps_play = FPS_PRESETS[idx + 1];
                debug!("Play FPS increased to {}", self.fps_play);
            }
    }

    /// Jump to next sequence start (] key)
    pub fn jump_next_sequence(&mut self) {
        if self.project.order_clips.is_empty() {
            return;
        }

        let len = self.project.order_clips.len();
        let idx = self.selected_seq_idx.unwrap_or(0);

        let next_idx = if idx + 1 < len {
            idx + 1
        } else if self.loop_enabled {
            0
        } else {
            idx
        };

        if next_idx != idx {
            if let Some(uuid) = self.project.order_clips.get(next_idx) {
                self.set_active_clip_by_uuid(uuid);
                self.selected_seq_idx = Some(next_idx);
                debug!("Jumped to next clip index {}", next_idx);
            }
        }

        self.last_frame_time = None;
    }

    /// Jump to previous sequence start ([ key)
    pub fn jump_prev_sequence(&mut self) {
        if self.project.order_clips.is_empty() {
            return;
        }

        let len = self.project.order_clips.len();
        let idx = self.selected_seq_idx.unwrap_or(0);

        let prev_idx = if idx > 0 {
            idx - 1
        } else if self.loop_enabled {
            len.saturating_sub(1)
        } else {
            idx
        };

        if prev_idx != idx {
            if let Some(uuid) = self.project.order_clips.get(prev_idx) {
                self.set_active_clip_by_uuid(uuid);
                self.selected_seq_idx = Some(prev_idx);
                debug!("Jumped to previous clip index {}", prev_idx);
            }
        }

        self.last_frame_time = None;
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
        Self::new()
    }
}
