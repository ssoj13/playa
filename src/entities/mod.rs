//! Entities module - core types with separation of business logic and GUI.
//! Data flow: UI/EventBus drives mutations on `Project`/`Comp`/`Attrs`; loaders
//! and compositor produce `Frame` data that UI/encoding consume.

pub mod attrs;
pub mod comp;
pub mod comp_events;
pub mod comp_node;
pub mod compositor;
pub mod file_node;
pub mod frame;
pub mod gpu_compositor;
pub mod keys;
pub mod layer;
pub mod loader;
pub mod loader_video;
pub mod node;
pub mod node_kind;
pub mod project;

pub use attrs::{AttrValue, Attrs};
pub use comp::{Comp, CompDfsIter, CompIterItem};
pub use comp_node::{CompNode, Layer as NodeLayer};
pub use compositor::CompositorType;
pub use file_node::FileNode;
pub use frame::{Frame, FrameStatus};
pub use layer::{Layer, Track};
pub use node::Node;
pub use node_kind::NodeKind;
pub use project::Project;
