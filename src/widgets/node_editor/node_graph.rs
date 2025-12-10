//! Node graph implementation using egui-snarl.
//!
//! # Purpose
//!
//! Visual representation of composition hierarchy as a node network.
//! Each Comp becomes a node, child relationships become wire connections.
//! Alternative view to timeline - same data, different visualization.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │ Source Node │────▶│ Source Node │────▶│ Output Node │
//! │ (file comp) │     │ (layer comp)│     │ (current)   │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!       [F]                 [C]                [OUT]
//! ```
//!
//! - Source nodes: comps used as children (inputs to current comp)
//! - Output node: the currently viewed comp (has input pins)
//! - Wires: child->parent relationship (source's output to parent's input)
//!
//! # Crate Choice: egui-snarl 0.9.0
//!
//! Selected over alternatives because:
//! - egui 0.33 compatible (matches our Cargo.toml)
//! - Built-in serde support for save/load
//! - Active maintenance (Jan 2025)
//! - Professional wire rendering
//!
//! # Data Flow
//!
//! 1. `set_comp()` - called when user switches to different comp
//! 2. `rebuild_from_comp()` - reads Comp.children, creates nodes/wires
//! 3. `render_node_editor()` - displays via egui-snarl
//!
//! Currently READ-ONLY view. Future: edits in node graph sync back to Comp.
//!
//! # Dependencies
//!
//! - `egui-snarl` - node graph UI library
//! - `Comp`, `Project` - data sources
//! - Called from: timeline widget (as alternate tab)

use eframe::egui::{Color32, Pos2, Ui};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::event_bus::BoxedEvent;
use crate::entities::{Comp, Project};

/// Node types in the composition graph.
///
/// Two variants map to composition structure:
/// - `Source` - a comp used as child (displayed on left side)
/// - `Output` - the currently viewed comp (displayed on right side)
///
/// # Color Coding
///
/// Visual distinction helps users understand node types:
/// - Orange [F] - file mode comp (loads from disk)
/// - Green [C] - layer mode comp (composites children)
/// - Red [OUT] - output node (current view target)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompNode {
    /// Source composition - appears as input to the current comp.
    /// `is_file` determines color: orange for file comps, green for layer comps.
    Source {
        comp_uuid: Uuid,
        name: String,
        is_file: bool,
    },
    /// Output node - represents the currently viewed composition.
    /// Has input pins where source nodes connect.
    Output { comp_uuid: Uuid, name: String },
}

impl CompNode {
    pub fn name(&self) -> &str {
        match self {
            CompNode::Source { name, .. } => name,
            CompNode::Output { name, .. } => name,
        }
    }

    pub fn uuid(&self) -> Uuid {
        match self {
            CompNode::Source { comp_uuid, .. } => *comp_uuid,
            CompNode::Output { comp_uuid, .. } => *comp_uuid,
        }
    }

    pub fn is_output(&self) -> bool {
        matches!(self, CompNode::Output { .. })
    }
}

/// SnarlViewer implementation for rendering CompNode.
///
/// egui-snarl requires implementing SnarlViewer to define:
/// - How many inputs/outputs each node type has
/// - How to render pin labels and colors
/// - Header appearance with icons and colors
///
/// This is a stateless viewer - all data comes from CompNode variants.
struct CompNodeViewer;

#[allow(refining_impl_trait)]
impl SnarlViewer<CompNode> for CompNodeViewer {
    fn title(&mut self, node: &CompNode) -> String {
        node.name().to_string()
    }

    fn outputs(&mut self, node: &CompNode) -> usize {
        match node {
            CompNode::Source { .. } => 1,
            CompNode::Output { .. } => 0,
        }
    }

    fn inputs(&mut self, node: &CompNode) -> usize {
        match node {
            CompNode::Source { .. } => 0,
            CompNode::Output { .. } => 8,
        }
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<CompNode>,
    ) -> PinInfo {
        ui.label(format!("L{}", pin.id.input));
        PinInfo::circle().with_fill(Color32::from_rgb(100, 180, 255))
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<CompNode>,
    ) -> PinInfo {
        let node = &snarl[pin.id.node];
        match node {
            CompNode::Source { is_file, .. } => {
                let color = if *is_file {
                    Color32::from_rgb(255, 180, 100)
                } else {
                    Color32::from_rgb(100, 255, 180)
                };
                ui.label("Out");
                PinInfo::circle().with_fill(color)
            }
            _ => PinInfo::circle().with_fill(Color32::GRAY),
        }
    }

    fn has_body(&mut self, _node: &CompNode) -> bool {
        false
    }

    fn show_body(
        &mut self,
        _node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _ui: &mut Ui,
        _snarl: &mut Snarl<CompNode>,
    ) {
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CompNode>,
    ) {
        let node_data = &snarl[node];
        let (color, icon) = match node_data {
            CompNode::Source { is_file, .. } => {
                if *is_file {
                    (Color32::from_rgb(255, 180, 100), "[F]")
                } else {
                    (Color32::from_rgb(100, 255, 180), "[C]")
                }
            }
            CompNode::Output { .. } => (Color32::from_rgb(255, 100, 100), "[OUT]"),
        };

        ui.horizontal(|ui| {
            ui.colored_label(color, icon);
            ui.label(node_data.name());
        });
    }
}

/// Persistent state for node editor panel.
///
/// # Serialization
///
/// Only `comp_uuid` is serialized. The `snarl` graph is rebuilt from Comp
/// each time because:
/// 1. Comp is the source of truth (node positions are not persisted)
/// 2. Avoids sync issues between stored graph and actual Comp.children
/// 3. Simpler than trying to merge external changes
///
/// # Rebuild Strategy
///
/// `needs_rebuild` flag triggers full graph reconstruction when:
/// - User switches to different comp (`set_comp()`)
/// - External change to comp children (via `mark_dirty()`)
///
/// This is efficient because comps typically have <20 children.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct NodeEditorState {
    /// The egui-snarl graph containing nodes and wires.
    /// Skipped from serde - rebuilt from Comp on demand.
    #[serde(skip)]
    pub snarl: Snarl<CompNode>,

    /// UUID of the comp currently displayed in node view.
    /// Persisted to restore view on app restart.
    pub comp_uuid: Option<Uuid>,

    /// Internal flag: true means graph needs rebuild from Comp.
    /// Set by `set_comp()` or `mark_dirty()`, cleared by `rebuild_from_comp()`.
    #[serde(skip)]
    needs_rebuild: bool,
}

impl NodeEditorState {
    pub fn new() -> Self {
        Self {
            snarl: Snarl::new(),
            comp_uuid: None,
            needs_rebuild: true,
        }
    }

    /// Mark graph as needing rebuild
    pub fn mark_dirty(&mut self) {
        self.needs_rebuild = true;
    }

    /// Set the composition to display
    pub fn set_comp(&mut self, comp_uuid: Uuid) {
        if self.comp_uuid != Some(comp_uuid) {
            self.comp_uuid = Some(comp_uuid);
            self.needs_rebuild = true;
        }
    }

    /// Rebuild graph from composition hierarchy.
    ///
    /// Creates a visual node graph from Comp.children:
    /// 1. Clear existing graph
    /// 2. Create Output node for current comp (right side, x=400)
    /// 3. For each child layer: create Source node (left side, x=100)
    /// 4. Connect each source's output pin to output's input pin
    ///
    /// # Layout
    ///
    /// Simple vertical stack for source nodes (y += 60 per node).
    /// Future: could use actual node positions or auto-layout algorithm.
    ///
    /// # Lock Behavior
    ///
    /// Takes read lock on project.media to resolve source comp names/types.
    /// Lock is held for duration of rebuild (fast, typically <1ms).
    pub fn rebuild_from_comp(&mut self, comp: &Comp, project: &Project) {
        if !self.needs_rebuild {
            return;
        }
        self.needs_rebuild = false;

        self.snarl = Snarl::new();

        let comp_uuid = comp.get_uuid();
        let media = project.media.read().expect("media lock");

        // Output node (current comp) positioned on right side
        let output_pos = Pos2::new(400.0, 200.0);
        let output_id = self.snarl.insert_node(
            output_pos,
            CompNode::Output {
                comp_uuid,
                name: comp.name().to_string(),
            },
        );

        // Create source nodes for each child, stacked vertically on left
        let mut y = 50.0;
        let mut input_idx = 0usize;

        for (_, attrs) in comp.get_children() {
            if let Some(source_str) = attrs.get_str("uuid") {
                if let Ok(source_uuid) = Uuid::parse_str(source_str) {
                    let pos = Pos2::new(100.0, y);

                    // Resolve source comp to get name and type
                    let (name, is_file) = if let Some(source_comp) = media.get(&source_uuid) {
                        (source_comp.name().to_string(), source_comp.is_file_mode())
                    } else {
                        ("Unknown".to_string(), false)
                    };

                    let source_id = self.snarl.insert_node(
                        pos,
                        CompNode::Source {
                            comp_uuid: source_uuid,
                            name,
                            is_file,
                        },
                    );

                    // Wire: source output[0] -> output input[N]
                    let out_pin = OutPinId {
                        node: source_id,
                        output: 0,
                    };
                    let in_pin = InPinId {
                        node: output_id,
                        input: input_idx,
                    };
                    let _ = self.snarl.connect(out_pin, in_pin);

                    y += 60.0;
                    input_idx += 1;
                }
            }
        }
    }
}

/// Render node editor widget.
///
/// Entry point for node graph UI. Call from timeline panel (as alternate tab).
///
/// # Arguments
///
/// - `ui` - egui Ui context for rendering
/// - `state` - persistent node editor state (survives across frames)
/// - `project` - for accessing media map to resolve comp names
/// - `comp` - the composition to display as node graph
/// - `_dispatch` - event emitter (unused for now, for future edit operations)
///
/// # Behavior
///
/// 1. Syncs state to current comp (triggers rebuild if comp changed)
/// 2. Rebuilds graph from Comp.children if dirty
/// 3. Renders via egui-snarl with default styling
///
/// Currently read-only. Future: dispatch LayerAddedEvent etc on graph edits.
pub fn render_node_editor(
    ui: &mut Ui,
    state: &mut NodeEditorState,
    project: &Project,
    comp: &Comp,
    _dispatch: impl FnMut(BoxedEvent),
) {
    // Sync to current comp (sets needs_rebuild if comp changed)
    state.set_comp(comp.get_uuid());

    // Rebuild graph from Comp.children if needed
    state.rebuild_from_comp(comp, project);

    // Stateless viewer interprets CompNode for rendering
    let mut viewer = CompNodeViewer;

    // Render with default wire/node styling
    let style = SnarlStyle::default();
    state.snarl.show(&mut viewer, &style, "comp_node_editor", ui);
}
