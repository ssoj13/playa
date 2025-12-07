//! Viewport widget events.

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
