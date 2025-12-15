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

/// Fit timeline view to layers.
/// If `selected_only` is true and layers are selected, fit to selection.
/// Otherwise fit to all layers.
#[derive(Clone, Debug)]
pub struct TimelineFitEvent {
    pub selected_only: bool,
}

impl TimelineFitEvent {
    pub fn all() -> Self { Self { selected_only: false } }
    pub fn selected() -> Self { Self { selected_only: true } }
}
