//! Node Editor widget - visual graph representation of comp hierarchy.
//!
//! Built on the nodes-rs wgpu graph engine. Each Comp/layer becomes a node,
//! child relationships become wires. Shares data with the Timeline view.

pub mod node_events;
mod node_graph;

pub use node_events::*;
pub use node_graph::{NodeEditorState, render_node_editor};
