//! Debounced preloader - delays full cache preload after attribute changes.
//!
//! When attributes change rapidly (e.g., scrubbing sliders), we don't want to
//! flood the cache with preload requests. Instead:
//! 1. Immediately render only the current frame
//! 2. After a configurable delay, trigger full preload radius
//!
//! This prevents wasted work and keeps the UI responsive.

use std::time::{Duration, Instant};
use uuid::Uuid;

/// Debounced preloader for delayed cache warming after attribute changes.
/// 
/// # Usage
/// ```ignore
/// // On attribute change:
/// preloader.schedule(comp_uuid);
/// enqueue_single_frame(current_frame);  // immediate
/// 
/// // In update loop:
/// if let Some(uuid) = preloader.tick() {
///     enqueue_frame_loads_around_playhead(preload_radius);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DebouncedPreloader {
    /// Delay before triggering full preload
    delay: Duration,
    /// Pending preload: (comp_uuid, trigger_time)
    pending: Option<(Uuid, Instant)>,
}

impl Default for DebouncedPreloader {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(500),
            pending: None,
        }
    }
}

impl DebouncedPreloader {
    /// Create with custom delay
    pub fn new(delay_ms: u64) -> Self {
        Self {
            delay: Duration::from_millis(delay_ms),
            pending: None,
        }
    }

    /// Set delay duration
    pub fn set_delay(&mut self, delay_ms: u64) {
        self.delay = Duration::from_millis(delay_ms);
    }

    /// Get current delay in milliseconds
    pub fn delay_ms(&self) -> u64 {
        self.delay.as_millis() as u64
    }

    /// Schedule a delayed preload for composition.
    /// If already pending, resets the timer (debounce behavior).
    pub fn schedule(&mut self, comp_uuid: Uuid) {
        let trigger_at = Instant::now() + self.delay;
        self.pending = Some((comp_uuid, trigger_at));
        log::trace!(
            "DebouncedPreloader: scheduled preload for {} in {}ms",
            comp_uuid,
            self.delay.as_millis()
        );
    }

    /// Cancel any pending preload
    pub fn cancel(&mut self) {
        if self.pending.is_some() {
            log::trace!("DebouncedPreloader: cancelled pending preload");
        }
        self.pending = None;
    }

    /// Check if preload should trigger now.
    /// Returns Some(comp_uuid) if delay has elapsed, None otherwise.
    /// Clears the pending state when triggered.
    pub fn tick(&mut self) -> Option<Uuid> {
        let Some((uuid, trigger_at)) = self.pending else {
            return None;
        };

        if Instant::now() >= trigger_at {
            self.pending = None;
            log::trace!("DebouncedPreloader: triggering preload for {}", uuid);
            Some(uuid)
        } else {
            None
        }
    }

    /// Check if there's a pending preload
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Get pending comp UUID (if any)
    pub fn pending_comp(&self) -> Option<Uuid> {
        self.pending.map(|(uuid, _)| uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_immediate_no_trigger() {
        let mut preloader = DebouncedPreloader::new(100);
        let uuid = Uuid::new_v4();
        
        preloader.schedule(uuid);
        assert!(preloader.is_pending());
        
        // Should not trigger immediately
        assert!(preloader.tick().is_none());
    }

    #[test]
    fn test_trigger_after_delay() {
        let mut preloader = DebouncedPreloader::new(10); // 10ms
        let uuid = Uuid::new_v4();
        
        preloader.schedule(uuid);
        std::thread::sleep(Duration::from_millis(15));
        
        // Should trigger after delay
        assert_eq!(preloader.tick(), Some(uuid));
        assert!(!preloader.is_pending());
    }

    #[test]
    fn test_debounce_resets_timer() {
        let mut preloader = DebouncedPreloader::new(50);
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        
        preloader.schedule(uuid1);
        std::thread::sleep(Duration::from_millis(30));
        
        // Re-schedule with different UUID - resets timer
        preloader.schedule(uuid2);
        
        // Should not trigger yet (timer reset)
        assert!(preloader.tick().is_none());
        assert_eq!(preloader.pending_comp(), Some(uuid2));
    }
}
