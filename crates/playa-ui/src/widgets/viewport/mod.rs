//! Viewport widget - image viewer with pan/zoom / scrubbing
//!
//! Raster is presented through wgpu inside egui’s paint callbacks.

mod coords;
pub mod gizmo;
mod pick;
mod renderer;
pub mod shaders;
pub mod tool;
mod viewport;
pub mod viewport_events;
mod viewport_ui;

pub use renderer::{ViewportPaintCallback, ViewportRenderer};
pub use shaders::Shaders;
pub use viewport::{ViewportMode, ViewportRenderState, ViewportState};
pub use viewport_events::ViewportRefreshEvent;
pub use viewport_ui::render;
