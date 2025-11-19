//! Timeline widget - After Effects-style layer stack
//!
//! Vertical stack of layers with horizontal bars

mod progress_bar;
mod timeline;
mod timeline_ui;
mod timeslider;

pub use progress_bar::ProgressBar;
pub use timeline::{
    TimelineConfig,
    TimelineState,
    GlobalDragState,
    LayerDragState, // deprecated
    TimelineAction,
};
pub use timeline_ui::render_timeline;
pub use timeslider::TimeSlider;
