//! Entities module - core types with separation of business logic and GUI.
//! Data flow: UI/EventBus drives mutations on `Project`/`Comp`/`Attrs`; loaders
//! and compositor produce `Frame` data that UI/encoding consume.

pub mod attrs;
pub mod comp;
pub mod comp_events;
pub mod compositor;
pub mod gpu_compositor;
pub mod frame;
pub mod keys;
pub mod loader;
pub mod loader_video;
pub mod project;

pub use attrs::{AttrValue, Attrs};
pub use comp::Comp;
pub use compositor::CompositorType;
pub use frame::Frame;
pub use project::Project;
