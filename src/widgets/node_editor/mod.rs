//! Node Editor widget - visual graph representation of comp hierarchy.
//!
//! Uses egui-snarl for node graph rendering. Each Comp becomes a node,
//! child relationships become connections. Shares data with Timeline view.

mod node_graph;
pub mod node_events;

pub use node_graph::{NodeEditorState, CompNode, render_node_editor};
pub use node_events::*;
