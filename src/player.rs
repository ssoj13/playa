//! Playback engine with frame-accurate timing and JKL controls
//!
//! **Architecture**: Player does NOT own Project. It receives `&mut Project`
//! when methods need to access project data. This eliminates duplication
//! (PlayaApp.project is the single source of truth).
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

use crate::entities::Project;
use crate::entities::frame::Frame;
use log::{debug, info};
use std::time::Instant;
use uuid::Uuid;

/// FPS presets for jog/shuttle control
const FPS_PRESETS: &[f32] = &[1.0, 2.0, 4.0, 8.0, 12.0, 24.0, 30.0, 60.0, 120.0, 240.0];

/// Frame step size for Shift+Arrow and Shift+PageUp/PageDown
pub const FRAME_JUMP_STEP: i32 = 25;

/// Playback state manager (does NOT own Project)
///
/// Player manages playback state only. Project is passed by reference
/// to methods that need it. PlayaApp owns the single Project instance.
pub struct Player {
    pub active_comp: Option<Uuid>, // UUID of active comp
    pub is_playing: bool,
    pub fps_base: f32, // Base FPS (persistent setting)
    pub fps_play: f32, // Current playback FPS (temporary, resets on stop)
    pub loop_enabled: bool,
    pub play_direction: f32, // 1.0 forward, -1.0 backward
    pub last_frame_time: Option<Instant>,
    /// Index of selected media in Project.comps_order (playlist)
    pub selected_seq_idx: Option<usize>,
}

impl Player {
    /// Create new player with defaults (no Project - that's in PlayaApp)
    pub fn new() -> Self {
        info!("Player initialized (project-less architecture)");

        Self {
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

    /// Get total frames of active comp (play_frame_count - work area)
    pub fn total_frames(&self, project: &Project) -> i32 {
        self.active_comp
            .and_then(|uuid| project.get_comp(uuid))
            .map(|c| c.play_frame_count())
            .unwrap_or(0)
    }

    /// Get current play range of active comp (start, end), or (0, 0) if none.
    pub fn play_range(&self, project: &Project) -> (i32, i32) {
        if let Some(comp) = self.active_comp.and_then(|uuid| project.get_comp(uuid)) {
            comp.play_range(true)
        } else {
            (0, 0)
        }
    }

    /// Set play range of active comp in global comp frame indices (inclusive).
    pub fn set_play_range(&mut self, start: i32, end: i32, project: &mut Project) {
        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                if comp.end() < comp.start() {
                    return;
                }

                let comp_start = comp.start();
                let comp_end = comp.end();

                // Clamp requested range to comp bounds
                let clamped_start = start.clamp(comp_start, comp_end);
                let clamped_end = end.clamp(comp_start, comp_end);
                let (final_start, final_end) = if clamped_end < clamped_start {
                    (clamped_end, clamped_start)
                } else {
                    (clamped_start, clamped_end)
                };

                comp.set_comp_play_start(final_start);
                comp.set_comp_play_end(final_end);

                // Ensure current_frame lies inside new play range
                let (visible_start, visible_end) = comp.play_range(true);
                let current = comp.current_frame;
                if current < visible_start || current > visible_end {
                    comp.set_current_frame(visible_start);
                }
            });
        }
    }

    /// Get current frame index from active comp
    pub fn current_frame(&self, project: &Project) -> i32 {
        self.active_comp
            .and_then(|uuid| project.get_comp(uuid))
            .map(|c| c.current_frame)
            .unwrap_or(0)
    }

    /// Get current frame as owned Frame (Composed)
    /// Uses GPU compositor (main thread only)
    pub fn get_current_frame(&self, project: &Project) -> Option<Frame> {
        let comp_uuid = self.active_comp?;
        let comp = project.get_comp(comp_uuid)?;
        let frame_idx = comp.current_frame;
        comp.get_frame(frame_idx, project, true) // use_gpu=true for main thread display
    }

    /// Switch to a different composition by UUID.
    ///
    /// Updates active_comp and emits CurrentFrameChanged event.
    /// Stops playback during transition.
    pub fn set_active_comp(&mut self, comp_uuid: Uuid, project: &mut Project) {
        // Check if comp exists in media
        if !project.contains_comp(comp_uuid) {
            log::warn!("Comp {} not found, cannot activate", comp_uuid);
            return;
        }

        // Stop playback during transition
        self.is_playing = false;

        // Switch to new comp
        self.active_comp = Some(comp_uuid);
        project.active = Some(comp_uuid);

        // Recalculate bounds and emit CurrentFrameChanged event (triggers frame loading)
        project.modify_comp(comp_uuid, |comp| {
            comp.on_activate();
            let frame = comp.current_frame;
            comp.set_current_frame(frame);
            log::info!("Activated comp {} at frame {}", comp_uuid, frame);
        });

        // Keep selection in sync: ensure active is included and ordered
        project.selection.retain(|u| *u != comp_uuid);
        project.selection.push(comp_uuid);
    }



    /// Update playback state
    pub fn update(&mut self, project: &mut Project) {
        if !self.is_playing || self.total_frames(project) == 0 {
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
                self.advance_frame(project);
                self.last_frame_time = Some(now);
            }
        } else {
            self.last_frame_time = Some(now);
        }
    }

    /// Advance to next frame
    fn advance_frame(&mut self, project: &mut Project) {
        let total_frames = self.total_frames(project);
        if total_frames == 0 {
            return;
        }

        let (play_start, play_end) = self.play_range(project);
        if play_end < play_start {
            return;
        }

        // Copy values before closure
        let play_direction = self.play_direction;
        let loop_enabled = self.loop_enabled;

        // Closure returns whether playback should stop
        let mut should_stop = false;
        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                let mut current = comp.current_frame;
                if current < play_start || current > play_end {
                    current = if play_direction >= 0.0 {
                        play_start
                    } else {
                        play_end
                    };
                    comp.set_current_frame(current);
                }

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
                            should_stop = true;
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
                            should_stop = true;
                        }
                    } else {
                        comp.set_current_frame(current - 1);
                    }
                }
            });
        }

        if should_stop {
            self.is_playing = false;
        }
    }

    /// Stop playback (always stops, doesn't toggle)
    pub fn stop(&mut self) {
        if self.is_playing {
            self.is_playing = false;
            debug!("Playback stopped");
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.fps_play = self.fps_base;
        }
    }

    /// Rewind to start
    pub fn to_start(&mut self, project: &mut Project) {
        let (start, _) = self.play_range(project);
        debug!("Rewinding to frame {}", start);
        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                comp.set_current_frame(start);
            });
        }
        self.last_frame_time = None;
    }

    /// Skip to end
    pub fn to_end(&mut self, project: &mut Project) {
        let (_, end) = self.play_range(project);
        debug!("Skipping to end: frame {}", end);
        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                comp.set_current_frame(end);
            });
        }
        self.last_frame_time = None;
    }

    /// Set current frame (emits CompEvent::CurrentFrameChanged).
    ///
    /// Clamps to full comp timeline [comp.start..=comp.end], not play_range,
    /// so scrubbing/timeline can move outside work area while playback still
    /// respects play_range.
    pub fn set_frame(&mut self, frame: i32, project: &mut Project) {
        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                let comp_start = comp.start();
                let comp_end = comp.end();
                if comp_end < comp_start {
                    return;
                }
                let clamped = frame.clamp(comp_start, comp_end);
                comp.set_current_frame(clamped);
            });
        }

        self.last_frame_time = None;
    }

    /// Step by N frames (positive = forward, negative = backward)
    /// Respects play range and loop_enabled setting
    pub fn step(&mut self, count: i32, project: &mut Project) {
        if count == 0 {
            return;
        }

        let current = self.current_frame(project);
        let (play_start, play_end) = self.play_range(project);

        // Calculate target frame with saturating arithmetic
        let target = if count > 0 {
            current.saturating_add(count)
        } else {
            current.saturating_sub(count.unsigned_abs() as i32)
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

        if let Some(uuid) = self.active_comp {
            project.modify_comp(uuid, |comp| {
                comp.set_current_frame(final_frame);
            });
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
            && idx + 1 < FPS_PRESETS.len()
        {
            self.fps_play = FPS_PRESETS[idx + 1];
            debug!("Play FPS increased to {}", self.fps_play);
        }
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
