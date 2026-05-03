//! Viewport widget events.

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ZoomViewportEvent(pub f32);

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct ResetViewportEvent;

#[derive(Clone, Debug)]
pub struct FitViewportEvent;

#[derive(Clone, Debug)]
pub struct Viewport100Event;

#[derive(Clone, Debug)]
pub struct ViewportRefreshEvent;
