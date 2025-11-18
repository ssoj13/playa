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
use std::time::Instant;
use crate::entities::Clip;
use crate::entities::Comp;
use crate::frame::Frame;
use crate::entities::Layer;
use crate::entities::Project;

/// FPS presets for jog/shuttle control
const FPS_PRESETS: &[f32] = &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0];

/// Frame step size for Shift+Arrow and Shift+PageUp/PageDown
pub const FRAME_JUMP_STEP: i32 = 25;

/// Playback state manager with new architecture
pub struct Player {
    pub project: Project,
    pub active_comp: Option<String>, // UUID of active comp
    pub is_playing: bool,
    pub fps_base: f32,       // Base FPS (persistent setting)
    pub fps_play: f32,       // Current playback FPS (temporary, resets on stop)
    pub loop_enabled: bool,
    pub play_direction: f32, // 1.0 forward, -1.0 backward
    last_frame_time: Option<Instant>,
    /// Index of selected clip in Project.clips_order (playlist)
    pub selected_seq_idx: Option<usize>,
}

impl Player {
    /// Create new player with empty project and defaults
    pub fn new() -> Self {
        info!("Player initialized with Comp-based architecture");

        Self {
            project: Project::new(),
            active_comp: None,
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
            self.project.media.get_mut(uuid)?.as_comp_mut()
        } else {
            None
        }
    }

    fn active_comp(&self) -> Option<&Comp> {
        if let Some(ref uuid) = self.active_comp {
            self.project.media.get(uuid)?.as_comp()
        } else {
            None
        }
    }

    /// Get total frames of active comp (play_frame_count - work area)
    pub fn total_frames(&self) -> usize {
        self.active_comp().map(|c| c.play_frame_count()).unwrap_or(0)
    }

    /// Get current play range of active comp (start, end), or (0, 0) if none.
    pub fn play_range(&self) -> (usize, usize) {
        if let Some(comp) = self.active_comp() {
            comp.play_range()
        } else {
            (0, 0)
        }
    }

    /// Set play range of active comp in global comp frame indices (inclusive).
    ///
    /// Internally this is mapped to comp.play_start / comp.play_end offsets.
    pub fn set_play_range(&mut self, start: usize, end: usize) {
        if let Some(comp) = self.active_comp_mut() {
            if comp.end() < comp.start() {
                return;
            }

            let comp_start = comp.start();
            let comp_end = comp.end();

            // Clamp requested range to comp bounds
            let clamped_start = start.clamp(comp_start, comp_end);
            let clamped_end = end.clamp(comp_start, comp_end);
            if clamped_end < clamped_start {
                return;
            }

            let play_start = (clamped_start as i32 - comp_start as i32).max(0);
            let play_end = (comp_end as i32 - clamped_end as i32).max(0);

            comp.set_comp_play_start(play_start);
            comp.set_comp_play_end(play_end);

            // Ensure current_frame lies inside new play range
            let (visible_start, visible_end) = comp.play_range();
            let current = comp.current_frame;
            if current < visible_start || current > visible_end {
                comp.set_current_frame(visible_start);
            }
        }
    }

    /// Reset play range of active comp to its full range.
    pub fn reset_play_range(&mut self) {
        if let Some(comp) = self.active_comp_mut() {
            comp.set_comp_play_start(0);
            comp.set_comp_play_end(0);
            let start = comp.start();
            comp.set_current_frame(start);
        }
    }

    /// Get current frame index from active comp
    pub fn current_frame(&self) -> usize {
        self.active_comp().map(|c| c.current_frame).unwrap_or(0)
    }

    /// Get current frame as owned Frame (Composed)
    pub fn get_current_frame(&mut self) -> Option<Frame> {
        let frame_idx = self.current_frame();
        let comp_uuid = self.active_comp.clone()?;
        let comp = self.project.media.get(&comp_uuid)?.as_comp()?;
        comp.get_frame(frame_idx, &self.project)
    }

    /// Append detected clip to project playlist and add as Layer to active Comp.
    pub fn append_clip(&mut self, clip: Clip) {
        let uuid = clip.uuid.clone();
        let clip_len = clip.len();

        // Insert clip into unified media HashMap
        self.project.media.insert(uuid.clone(), crate::media::MediaSource::Clip(clip));
        self.project.clips_order.push(uuid.clone());

        // Ensure we have an active comp (creates "Main" if none exist)
        if self.active_comp.is_none() {
            let default_uuid = self.project.ensure_default_comp();
            self.active_comp = Some(default_uuid);
        }

        // Add clip as Layer to active comp
        if let Some(comp_uuid) = &self.active_comp.clone() {
            if let Some(source) = self.project.media.get_mut(comp_uuid) {
                if let Some(comp) = source.as_comp_mut() {
                    log::info!("Creating layer from clip {} with {} frames", uuid, clip_len);

                    // Position layer at end of comp timeline (sequential stacking)
                    // If comp is empty, start from 0, otherwise stack after last layer
                    let layer_start = if comp.layers.is_empty() {
                        0  // First layer starts at frame 0
                    } else {
                        comp.end() + 1  // Subsequent layers stack sequentially
                    };
                    let layer_end = layer_start + clip_len.saturating_sub(1);

                    // Create Layer with UUID reference
                    let layer = Layer::new(uuid.clone(), layer_start, layer_end);

                    // If this is the first layer, reset comp start to 0
                    let is_first_layer = comp.layers.is_empty();

                    comp.layers.push(layer);

                    // Extend comp timeline to include new layer
                    if is_first_layer {
                        comp.set_start(0);
                        comp.current_frame = 0;  // Reset playhead to start
                    }
                    comp.set_end(layer_end);

                    log::info!("Added clip {} as Layer to comp {} (timeline: {}..{})",
                        uuid, comp_uuid, layer_start, layer_end);
                }
            }
        }

        // Set selected index
        if self.selected_seq_idx.is_none() {
            self.selected_seq_idx = Some(0);
        }
    }

    /// Switch to a different composition by UUID.
    ///
    /// Updates active_comp and emits CurrentFrameChanged event.
    /// Stops playback during transition.
    pub fn set_active_comp(&mut self, comp_uuid: String) {
        // Check if comp exists in media
        if let Some(source) = self.project.media.get(&comp_uuid) {
            if !source.is_comp() {
                log::warn!("{} is not a comp, cannot activate", comp_uuid);
                return;
            }
        } else {
            log::warn!("Comp {} not found, cannot activate", comp_uuid);
            return;
        }

        // Stop playback during transition
        self.is_playing = false;

        // Switch to new comp
        self.active_comp = Some(comp_uuid.clone());

        // Emit CurrentFrameChanged event from new comp (triggers frame loading)
        if let Some(source) = self.project.media.get_mut(&comp_uuid) {
            if let Some(comp) = source.as_comp_mut() {
                let frame = comp.current_frame;
                comp.set_current_frame(frame);
                log::info!("Activated comp {} at frame {}", comp_uuid, frame);
            }
        }
    }

    /// Helper: set active clip by UUID (for playlist navigation).
    pub fn set_active_clip_by_uuid(&mut self, clip_uuid: &str) {
        // Find which layer in active comp contains this clip
        if let Some(comp) = self.active_comp_mut() {
            for layer in comp.layers.iter() {
                if layer.source_uuid == clip_uuid {
                    // Jump to start of this layer
                    let layer_start = layer.attrs.get_u32("start").unwrap_or(0) as usize;
                    comp.set_current_frame(layer_start);
                    log::debug!("Jumped to clip {} at frame {}", clip_uuid, layer_start);
                    return;
                }
            }
        }
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

        // Copy values before borrowing comp mutably
        let play_direction = self.play_direction;
        let loop_enabled = self.loop_enabled;

        let comp = match self.active_comp_mut() {
            Some(c) => c,
            None => return,
        };

        let current = comp.current_frame;
        let (play_start, play_end) = (0, total_frames.saturating_sub(1));

        if play_direction > 0.0 {
            // Forward
            let next = current + 1;
            if next > play_end {
                if loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_start);
                    comp.set_current_frame(play_start);
                } else {
                    debug!("Reached play range end, stopping");
                    comp.set_current_frame(play_end);
                    self.is_playing = false;
                }
            } else {
                comp.set_current_frame(next);
            }
        } else {
            // Backward
            if current <= play_start {
                if loop_enabled {
                    debug!("Frame loop: {} -> {}", current, play_end);
                    comp.set_current_frame(play_end);
                } else {
                    debug!("Reached play range start, stopping");
                    self.is_playing = false;
                }
            } else {
                comp.set_current_frame(current - 1);
            }
        }
    }

    /// Toggle play/pause (Space, K, ArrowUp)
    pub fn toggle_play_pause(&mut self) {
        self.is_playing = !self.is_playing;
        if self.is_playing {
            debug!("Playback started at frame {}", self.current_frame());
            self.last_frame_time = Some(Instant::now());
        } else {
            debug!("Playback paused at frame {}", self.current_frame());
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Stop playback (always stops, doesn't toggle)
    pub fn stop(&mut self) {
        if self.is_playing {
            self.is_playing = false;
            debug!("Playback stopped at frame {}", self.current_frame());
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Rewind to start
    pub fn to_start(&mut self) {
        let (start, _) = self.play_range();
        debug!("Rewinding to frame {}", start);
        if let Some(comp) = self.active_comp_mut() {
            comp.set_current_frame(start);
        }
        self.last_frame_time = None;
    }

    /// Skip to end
    pub fn to_end(&mut self) {
        let (_, end) = self.play_range();
        debug!("Skipping to end: frame {}", end);
        if let Some(comp) = self.active_comp_mut() {
            comp.set_current_frame(end);
        }
        self.last_frame_time = None;
    }

    /// Set current frame (emits CompEvent::CurrentFrameChanged).
    ///
    /// Clamps to full comp timeline [comp.start..=comp.end], not play_range,
    /// so scrubbing/timeline can move outside work area while playback still
    /// respects play_range.
    pub fn set_frame(&mut self, frame: usize) {
        if let Some(comp) = self.active_comp_mut() {
            let comp_start = comp.start();
            let comp_end = comp.end();
            if comp_end < comp_start {
                return;
            }
            let clamped = frame.clamp(comp_start, comp_end);
            comp.set_current_frame(clamped);
        }

        self.last_frame_time = None;
    }

    /// Step by N frames (positive = forward, negative = backward)
    /// Respects play range and loop_enabled setting
    pub fn step(&mut self, count: i32) {
        if count == 0 {
            return;
        }

        let current = self.current_frame();
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

        if let Some(comp) = self.active_comp_mut() {
            comp.set_current_frame(final_frame);
        }
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
        if self.project.clips_order.is_empty() {
            return;
        }

        let len = self.project.clips_order.len();
        let idx = self.selected_seq_idx.unwrap_or(0);

        let next_idx = if idx + 1 < len {
            idx + 1
        } else if self.loop_enabled {
            0
        } else {
            idx
        };

        if next_idx != idx {
            if let Some(uuid) = self.project.clips_order.get(next_idx).cloned() {
                self.set_active_clip_by_uuid(&uuid);
                self.selected_seq_idx = Some(next_idx);
                debug!("Jumped to next clip index {}", next_idx);
            }
        }

        self.last_frame_time = None;
    }

    /// Jump to previous sequence start ([ key)
    pub fn jump_prev_sequence(&mut self) {
        if self.project.clips_order.is_empty() {
            return;
        }

        let len = self.project.clips_order.len();
        let idx = self.selected_seq_idx.unwrap_or(0);

        let prev_idx = if idx > 0 {
            idx - 1
        } else if self.loop_enabled {
            len.saturating_sub(1)
        } else {
            idx
        };

        if prev_idx != idx {
            if let Some(uuid) = self.project.clips_order.get(prev_idx).cloned() {
                self.set_active_clip_by_uuid(&uuid);
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
