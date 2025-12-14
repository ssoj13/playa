//! Entities module - core types with separation of business logic and GUI.
//! Data flow: UI/EventBus drives mutations on `Project`/`Comp`/`Attrs`; loaders
//! and compositor produce `Frame` data that UI/encoding consume.

pub mod attrs;
// pub mod comp;  // DEPRECATED: replaced by comp_node
pub mod comp_events;  // Events for comp/layer manipulation
pub mod comp_node;
pub mod compositor;
pub mod file_node;
pub mod frame;
pub mod gpu_compositor;
pub mod keys;
pub mod loader;
pub mod loader_video;
pub mod node;
pub mod node_kind;
pub mod project;

pub use attrs::{AttrValue, Attrs};
// Type alias for backwards compatibility
pub type Comp = CompNode;
pub use comp_node::{CompNode, Layer as NodeLayer};
pub use compositor::CompositorType;
pub use file_node::FileNode;
pub use frame::{Frame, FrameStatus};
// Layer is now only in comp_node.rs (pub use comp_node::Layer as NodeLayer above)
pub use node::Node;
pub use node_kind::NodeKind;
pub use project::{Project, NodeIter, NodeIterItem};
