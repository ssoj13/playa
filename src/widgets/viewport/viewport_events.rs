//! Viewport widget events.
//!
//! ## Events
//! - [`ZoomViewportEvent`] - Set zoom level directly
//! - [`ResetViewportEvent`] - Reset to default zoom/pan
//! - [`FitViewportEvent`] - Fit image to viewport (AutoFit mode)
//! - [`Viewport100Event`] - Set 100% zoom (Auto100 mode)
//! - [`ViewportRefreshEvent`] - Force frame re-render (used after attribute changes)
//!
//! ## ViewportRefreshEvent Flow
//! ```text
//! AttrsChangedEvent → increment_epoch() → emit ViewportRefreshEvent
//!                   → viewport_state.request_refresh()
//!                   → epoch mismatch detected → frame refreshed
//! ```

#[derive(Clone, Debug)]
pub struct ZoomViewportEvent(pub f32);

#[derive(Clone, Debug)]
pub struct ResetViewportEvent;

#[derive(Clone, Debug)]
pub struct FitViewportEvent;

#[derive(Clone, Debug)]
pub struct Viewport100Event;

/// Force viewport to refresh current frame (e.g., after attribute changes)
#[derive(Clone, Debug)]
pub struct ViewportRefreshEvent;
