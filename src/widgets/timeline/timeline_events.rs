//! Timeline widget events.

#[derive(Clone, Debug)]
pub struct TimelineZoomChangedEvent(pub f32);

#[derive(Clone, Debug)]
pub struct TimelinePanChangedEvent(pub f32);

#[derive(Clone, Debug)]
pub struct TimelineSnapChangedEvent(pub bool);

#[derive(Clone, Debug)]
pub struct TimelineLockWorkAreaChangedEvent(pub bool);

#[derive(Clone, Debug)]
pub struct TimelineFitAllEvent(pub f32);

#[derive(Clone, Debug)]
pub struct TimelineFitEvent;

#[derive(Clone, Debug)]
pub struct TimelineResetZoomEvent;
