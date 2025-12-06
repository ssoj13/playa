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
//!
//! # Selection Behavior
//!
//! `set_active_comp()` resets project selection to just the activated comp.
//! This prevents multi-selection accumulation when adding/switching clips.

use crate::entities::{Attrs, AttrValue, Project};
use crate::entities::frame::Frame;
use log::{debug, info};
use serde::{Deserialize, Serialize};
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
///
/// **Attrs keys**:
/// - `active_comp`: Option<Uuid> as JSON
/// - `is_playing`: Bool
/// - `fps_base`: Float (persistent base FPS)
/// - `fps_play`: Float (temporary playback FPS)
/// - `loop_enabled`: Bool
/// - `play_direction`: Float (1.0 forward, -1.0 backward)
/// - `selected_seq_idx`: Option<usize> as JSON
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    /// All serializable player state
    pub attrs: Attrs,

    /// Last frame timestamp (runtime-only, not serializable)
    #[serde(skip)]
    pub last_frame_time: Option<Instant>,
}

impl Player {
    /// Create new player with defaults (no Project - that's in PlayaApp)
    pub fn new() -> Self {
        info!("Player initialized (project-less architecture)");

        let mut attrs = Attrs::new();
        // Initialize defaults via attrs
        attrs.set_json("active_comp", &None::<Uuid>);
        attrs.set("is_playing", AttrValue::Bool(false));
        attrs.set("fps_base", AttrValue::Float(24.0));
        attrs.set("fps_play", AttrValue::Float(24.0));
        attrs.set("loop_enabled", AttrValue::Bool(true));
        attrs.set("play_direction", AttrValue::Float(1.0));
        attrs.set_json("selected_seq_idx", &None::<usize>);

        Self {
            attrs,
            last_frame_time: None,
        }
    }

    // === Accessor methods for attrs fields ===

    /// Get active comp UUID
    pub fn active_comp(&self) -> Option<Uuid> {
        self.attrs.get_json("active_comp").unwrap_or(None)
    }

    /// Set active comp UUID (low-level, does NOT update project state)
    /// Use `set_active_comp()` for full activation with project sync
    fn set_active_comp_uuid(&mut self, uuid: Option<Uuid>) {
        self.attrs.set_json("active_comp", &uuid);
    }

    /// Get previous comp UUID (for U key navigation back)
    pub fn previous_comp(&self) -> Option<Uuid> {
        self.attrs.get_json("previous_comp").unwrap_or(None)
    }

    /// Set previous comp UUID
    fn set_previous_comp_uuid(&mut self, uuid: Option<Uuid>) {
        self.attrs.set_json("previous_comp", &uuid);
    }

    /// Check if playing
    pub fn is_playing(&self) -> bool {
        self.attrs.get_bool_or("is_playing", false)
    }

    /// Set playing state
    pub fn set_is_playing(&mut self, playing: bool) {
        self.attrs.set("is_playing", AttrValue::Bool(playing));
    }

    /// Get base FPS
    pub fn fps_base(&self) -> f32 {
        self.attrs.get_float_or("fps_base", 24.0)
    }

    /// Set base FPS
    pub fn set_fps_base(&mut self, fps: f32) {
        self.attrs.set("fps_base", AttrValue::Float(fps));
    }

    /// Get playback FPS
    pub fn fps_play(&self) -> f32 {
        self.attrs.get_float_or("fps_play", 24.0)
    }

    /// Set playback FPS
    pub fn set_fps_play(&mut self, fps: f32) {
        self.attrs.set("fps_play", AttrValue::Float(fps));
    }

    /// Check if loop is enabled
    pub fn loop_enabled(&self) -> bool {
        self.attrs.get_bool_or("loop_enabled", true)
    }

    /// Set loop enabled
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        self.attrs.set("loop_enabled", AttrValue::Bool(enabled));
    }

    /// Get play direction (1.0 forward, -1.0 backward)
    pub fn play_direction(&self) -> f32 {
        self.attrs.get_float_or("play_direction", 1.0)
    }

    /// Set play direction
    fn set_play_direction(&mut self, dir: f32) {
        self.attrs.set("play_direction", AttrValue::Float(dir));
    }

    /// Get selected sequence index
    pub fn selected_seq_idx(&self) -> Option<usize> {
        self.attrs.get_json("selected_seq_idx").unwrap_or(None)
    }

    /// Set selected sequence index
    pub fn set_selected_seq_idx(&mut self, idx: Option<usize>) {
        self.attrs.set_json("selected_seq_idx", &idx);
    }

    /// Get total frames of active comp (play_frame_count - work area)
    pub fn total_frames(&self, project: &Project) -> i32 {
        self.active_comp()
            .and_then(|uuid| project.get_comp(uuid))
            .map(|c| c.play_frame_count())
            .unwrap_or(0)
    }

    /// Get current play range of active comp (start, end), or (0, 0) if none.
    pub fn play_range(&self, project: &Project) -> (i32, i32) {
        if let Some(comp) = self.active_comp().and_then(|uuid| project.get_comp(uuid)) {
            comp.play_range(true)
        } else {
            (0, 0)
        }
    }

    /// Set play range of active comp in global comp frame indices (inclusive).
    pub fn set_play_range(&mut self, start: i32, end: i32, project: &mut Project) {
        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                if comp._out() < comp._in() {
                    return;
                }

                let comp_start = comp._in();
                let comp_end = comp._out();

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
                let current = comp.frame();
                if current < visible_start || current > visible_end {
                    comp.set_frame(visible_start);
                }
            });
        }
    }

    /// Get current frame index from active comp
    pub fn current_frame(&self, project: &Project) -> i32 {
        self.active_comp()
            .and_then(|uuid| project.get_comp(uuid))
            .map(|c| c.frame())
            .unwrap_or(0)
    }

    /// Get current frame as owned Frame (Composed)
    /// Uses GPU compositor (main thread only)
    pub fn get_current_frame(&self, project: &Project) -> Option<Frame> {
        let comp_uuid = self.active_comp()?;
        let comp = project.get_comp(comp_uuid)?;
        let frame_idx = comp.frame();
        comp.get_frame(frame_idx, project, true) // use_gpu=true for main thread display
    }

    /// Switch to a different composition by UUID.
    ///
    /// Updates active_comp and emits CurrentFrameChanged event.
    /// Stops playback during transition.
    ///
    /// **Selection reset**: Resets project selection to just this comp.
    /// Prevents multi-selection accumulation when adding new clips or
    /// switching between comps (e.g., double-click on timeline layer).
    pub fn set_active_comp(&mut self, comp_uuid: Option<Uuid>, project: &mut Project) {
        // Handle None case - clear active
        let Some(uuid) = comp_uuid else {
            self.set_active_comp_uuid(None);
            project.set_active(None);
            return;
        };

        // Check if comp exists in media
        if !project.contains_comp(uuid) {
            log::warn!("Comp {} not found, cannot activate", uuid);
            return;
        }

        // Save current as previous (for U key navigation back)
        let current = self.active_comp();
        if current != Some(uuid) {
            self.set_previous_comp_uuid(current);
        }

        // Stop playback during transition
        self.set_is_playing(false);

        // Switch to new comp
        self.set_active_comp_uuid(Some(uuid));
        project.set_active(Some(uuid));

        // Recalculate bounds and emit CurrentFrameChanged event (triggers frame loading)
        project.modify_comp(uuid, |comp| {
            comp.on_activate();
            let frame = comp.frame();
            comp.set_frame(frame);
            log::info!("Activated comp {} at frame {}", uuid, frame);
        });

        // Reset selection to just the active comp
        project.set_selection(vec![uuid]);
    }



    /// Update playback state
    pub fn update(&mut self, project: &mut Project) {
        if !self.is_playing() || self.total_frames(project) == 0 {
            return;
        }

        // Ensure play_fps is not lower than base_fps
        // base_fps acts as a "floor" that pushes play_fps from below
        let fps_base = self.fps_base();
        let fps_play = self.fps_play();
        if fps_play < fps_base {
            self.set_fps_play(fps_base);
        }

        let now = Instant::now();

        if let Some(last_time) = self.last_frame_time {
            let elapsed = now.duration_since(last_time).as_secs_f32();
            let frame_duration = 1.0 / self.fps_play();

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
        let play_direction = self.play_direction();
        let loop_enabled = self.loop_enabled();

        // Closure returns whether playback should stop
        let mut should_stop = false;
        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                let mut current = comp.frame();
                if current < play_start || current > play_end {
                    current = if play_direction >= 0.0 {
                        play_start
                    } else {
                        play_end
                    };
                    comp.set_frame(current);
                }

                if play_direction > 0.0 {
                    // Forward
                    let next = current + 1;
                    if next > play_end {
                        if loop_enabled {
                            debug!("Frame loop: {} -> {}", current, play_start);
                            comp.set_frame(play_start);
                        } else {
                            debug!("Reached play range end, stopping");
                            comp.set_frame(play_end);
                            should_stop = true;
                        }
                    } else {
                        comp.set_frame(next);
                    }
                } else {
                    // Backward
                    if current <= play_start {
                        if loop_enabled {
                            debug!("Frame loop: {} -> {}", current, play_end);
                            comp.set_frame(play_end);
                        } else {
                            debug!("Reached play range start, stopping");
                            should_stop = true;
                        }
                    } else {
                        comp.set_frame(current - 1);
                    }
                }
            });
        }

        if should_stop {
            self.set_is_playing(false);
        }
    }

    /// Stop playback (always stops, doesn't toggle)
    pub fn stop(&mut self) {
        if self.is_playing() {
            self.set_is_playing(false);
            debug!("Playback stopped");
            self.last_frame_time = None;
            // Reset fps_play to fps_base on stop
            self.set_fps_play(self.fps_base());
        }
    }

    /// Rewind to start
    pub fn to_start(&mut self, project: &mut Project) {
        let (start, _) = self.play_range(project);
        debug!("Rewinding to frame {}", start);
        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                comp.set_frame(start);
            });
        }
        self.last_frame_time = None;
    }

    /// Skip to end
    pub fn to_end(&mut self, project: &mut Project) {
        let (_, end) = self.play_range(project);
        debug!("Skipping to end: frame {}", end);
        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                comp.set_frame(end);
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
        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                let comp_start = comp._in();
                let comp_end = comp._out();
                if comp_end < comp_start {
                    return;
                }
                let clamped = frame.clamp(comp_start, comp_end);
                comp.set_frame(clamped);
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
        let loop_enabled = self.loop_enabled();

        // Calculate target frame with saturating arithmetic
        let target = if count > 0 {
            current.saturating_add(count)
        } else {
            current.saturating_sub(count.unsigned_abs() as i32)
        };

        // Apply loop/clamp logic based on loop_enabled
        let final_frame = if target > play_end {
            if loop_enabled {
                // Loop: wrap around to play_start
                let overflow = target - play_end;
                let range_size = play_end - play_start + 1;
                play_start + ((overflow - 1) % range_size)
            } else {
                // Clamp to play_end
                play_end
            }
        } else if target < play_start {
            if loop_enabled {
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

        if let Some(uuid) = self.active_comp() {
            project.modify_comp(uuid, |comp| {
                comp.set_frame(final_frame);
            });
        }
    }

    /// Internal helper to start jogging in the specified direction
    fn start_jog(&mut self, direction: f32) {
        if !self.is_playing() {
            // Start playing in specified direction
            self.set_play_direction(direction);
            self.set_is_playing(true);
            self.set_fps_play(self.fps_base()); // Start with base FPS
            self.last_frame_time = Some(Instant::now());
        } else if self.play_direction().signum() != direction.signum() {
            // Change direction
            self.set_play_direction(direction);
            self.set_fps_play(self.fps_base()); // Reset on direction change
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
        let fps_base = self.fps_base();
        // Find first preset strictly greater than current FPS
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f > fps_base) {
            let new_fps = FPS_PRESETS[idx];
            self.set_fps_base(new_fps);
            // If not playing, update fps_play too
            if !self.is_playing() {
                self.set_fps_play(new_fps);
            }
            debug!("Base FPS increased to {}", new_fps);
        }
    }

    /// Decrease base FPS to previous preset (-/+ keys, Keypad)
    pub fn decrease_fps_base(&mut self) {
        let fps_base = self.fps_base();
        // Find last preset strictly less than current FPS
        if let Some(idx) = FPS_PRESETS.iter().rposition(|&f| f < fps_base) {
            let new_fps = FPS_PRESETS[idx];
            self.set_fps_base(new_fps);
            // If not playing, update fps_play too
            if !self.is_playing() {
                self.set_fps_play(new_fps);
            }
            debug!("Base FPS decreased to {}", new_fps);
        }
    }

    /// Increase play FPS to next preset (J/L when playing)
    fn increase_fps_play(&mut self) {
        let fps_play = self.fps_play();
        if let Some(idx) = FPS_PRESETS.iter().position(|&f| f >= fps_play)
            && idx + 1 < FPS_PRESETS.len()
        {
            let new_fps = FPS_PRESETS[idx + 1];
            self.set_fps_play(new_fps);
            debug!("Play FPS increased to {}", new_fps);
        }
    }


    /// Reset settings
    pub fn reset_settings(&mut self) {
        self.set_fps_base(24.0);
        self.set_fps_play(24.0);
        self.set_loop_enabled(true);
        info!("Player settings reset");
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
