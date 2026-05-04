//! Node Editor widget - visual graph representation of comp hierarchy.
//!
//! Uses egui-snarl for node graph rendering. Each Comp becomes a node,
//! child relationships become connections. Shares data with Timeline view.

pub mod node_events;
mod node_graph;

pub use node_events::*;
pub use node_graph::{CompNode, NodeEditorState, render_node_editor};
