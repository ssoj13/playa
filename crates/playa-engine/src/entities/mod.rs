//! Entities module - core types with separation of business logic and GUI.
//! Data flow: UI/EventBus drives mutations on `Project`/`Comp`/`Attrs`; loaders
//! and compositor produce `Frame` data that UI/encoding consume.

pub mod attr_schemas;
pub mod attrs;
pub mod camera_node;
pub mod comp_events; // Events for comp/layer manipulation
pub mod comp_node;
pub mod compositor;
pub mod effects;
pub mod file_node;
pub mod frame;
pub mod gpu_blend_bridge;
pub mod keys;
pub mod loader;
pub mod node;
pub mod node_kind;
pub mod project;
pub mod space;
pub mod text_node;
pub mod traits;
pub mod transform;

pub use attrs::{AttrValue, Attrs};
// Type alias for backwards compatibility
pub type Comp = CompNode;
pub use comp_node::{CompNode, Layer as NodeLayer};
pub use compositor::CompositorType;
pub use file_node::FileNode;
pub use frame::{Frame, FrameStatus};
pub use gpu_blend_bridge::{GpuBlendBridge, GpuBlendReport, GpuBlendRequest, gpu_blend_arc_pair};
// Layer is now only in comp_node.rs (pub use comp_node::Layer as NodeLayer above)
pub use node::{ComputeContext, Node};
pub use node_kind::NodeKind;
pub use project::{NodeIter, NodeIterItem, Project};

pub use camera_node::CameraNode;
pub use effects::{Effect, EffectType};
pub use playa_io::{SourceImage, pick_display_layer};
pub use text_node::TextNode;
pub use traits::{CacheStatsSnapshot, CacheStrategy, FrameCache, WorkerPool};
