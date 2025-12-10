//! Node graph implementation using egui-snarl.
//!
//! # Purpose
//!
//! Visual representation of composition hierarchy as a node network.
//! Each Comp becomes a node, child relationships become wire connections.
//! Shows FULL TREE depth - recursively traverses all children.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │ Leaf Node   │────▶│ Parent Node │────▶│ Output Node │
//! │ (file comp) │     │ (layer comp)│     │ (current)   │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!       [F]                 [C]                [OUT]
//!
//! Full tree example:
//!   [F] clip1 ──┐
//!   [F] clip2 ──┼──▶ [C] precomp ──┐
//!   [F] clip3 ──┘                  │
//!   [F] clip4 ─────────────────────┼──▶ [OUT] main
//!   [F] clip5 ─────────────────────┘
//! ```
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
//! 2. `rebuild_from_comp()` - recursively reads Comp.children, creates nodes/wires
//! 3. `render_node_editor()` - displays via egui-snarl with toolbar
//!
//! # Toolbar
//!
//! - A (All) - zoom to fit all nodes
//! - F (Fit) - zoom to fit selected nodes (or all if none selected)
//! - L (Layout) - auto-arrange nodes in clean tree layout
//!
//! Currently READ-ONLY view. Future: edits in node graph sync back to Comp.
//!
//! # Dependencies
//!
//! - `egui-snarl` - node graph UI library
//! - `Comp`, `Project` - data sources
//! - Called from: timeline widget (as alternate tab)

use std::collections::{HashMap, HashSet};
use std::sync::RwLockReadGuard;

use eframe::egui::{Color32, Pos2, Ui};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::event_bus::BoxedEvent;
use crate::entities::{Comp, Project};

/// Node in the composition graph - just a UUID reference to Comp.
/// All data (name, type, children) comes from project.media at render time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompNode {
    pub uuid: Uuid,
}

/// SnarlViewer implementation for rendering CompNode.
/// Holds reference to Project for resolving Comp data at render time.
struct CompNodeViewer<'a> {
    project: &'a Project,
    output_uuid: Uuid, // current comp being viewed
}

impl<'a> CompNodeViewer<'a> {
    fn get_comp(&self, uuid: Uuid) -> Option<Comp> {
        self.project.media.read().ok()?.get(&uuid).cloned()
    }
}

#[allow(refining_impl_trait)]
impl<'a> SnarlViewer<CompNode> for CompNodeViewer<'a> {
    fn title(&mut self, node: &CompNode) -> String {
        self.get_comp(node.uuid)
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn outputs(&mut self, _node: &CompNode) -> usize {
        1
    }

    fn inputs(&mut self, node: &CompNode) -> usize {
        self.get_comp(node.uuid)
            .map(|c| c.children_len())
            .unwrap_or(0)
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
        _pin: &OutPin,
        ui: &mut Ui,
        _snarl: &mut Snarl<CompNode>,
    ) -> PinInfo {
        ui.label("Out");
        PinInfo::circle().with_fill(Color32::from_rgb(180, 180, 180))
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
        let uuid = snarl[node].uuid;
        let comp = self.get_comp(uuid);

        let (icon, color, name) = match comp {
            Some(c) => {
                let is_output = uuid == self.output_uuid;
                let is_file = c.is_file_mode();
                let name = c.name().to_string();

                if is_output {
                    ("[OUT]", Color32::from_rgb(255, 100, 100), name)
                } else if is_file {
                    ("[F]", Color32::from_rgb(255, 180, 100), name)
                } else {
                    ("[C]", Color32::from_rgb(100, 255, 180), name)
                }
            }
            None => ("[?]", Color32::GRAY, "Unknown".to_string()),
        };

        ui.horizontal(|ui| {
            ui.colored_label(color, icon);
            ui.label(name);
        });
    }
}

/// Layout constants for node positioning
const HORIZONTAL_SPACING: f32 = 200.0;
const VERTICAL_SPACING: f32 = 70.0;

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
/// This is efficient because comps typically have <100 nodes.
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

    /// Flag to trigger fit-all on next frame
    #[serde(skip)]
    pub fit_all_requested: bool,

    /// Flag to trigger re-layout on next frame
    #[serde(skip)]
    pub layout_requested: bool,
}

impl NodeEditorState {
    pub fn new() -> Self {
        Self {
            snarl: Snarl::new(),
            comp_uuid: None,
            needs_rebuild: true,
            fit_all_requested: false,
            layout_requested: false,
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

    /// Rebuild graph from composition hierarchy (FULL TREE).
    ///
    /// Creates a visual node graph by recursively traversing all children:
    /// 1. Clear existing graph
    /// 2. Recursively collect all nodes in the tree (DFS)
    /// 3. Create nodes with proper types (Leaf/Intermediate/Output)
    /// 4. Connect wires between parent-child relationships
    /// 5. Apply tree layout (rightmost = output, leftmost = leaves)
    ///
    /// # Layout Algorithm
    ///
    /// Uses depth-first traversal to determine tree depth, then positions:
    /// - X position based on depth (deeper = more left)
    /// - Y position based on vertical slot within depth level
    ///
    /// # Cycle Detection
    ///
    /// Uses visited set to prevent infinite loops from cyclic references.
    pub fn rebuild_from_comp(&mut self, comp: &Comp, project: &Project) {
        if !self.needs_rebuild {
            return;
        }
        self.needs_rebuild = false;

        self.snarl = Snarl::new();

        let root_uuid = comp.get_uuid();
        let media = project.media.read().expect("media lock");

        log::debug!(
            "NodeEditor: rebuilding for comp '{}' ({}), media has {} items",
            comp.name(),
            root_uuid,
            media.len()
        );
        log::debug!("NodeEditor: comp is in media? {}", media.contains_key(&root_uuid));

        // Phase 1: Collect all nodes recursively with their depth and children count
        let mut node_info: HashMap<Uuid, NodeInfo> = HashMap::new();
        let mut visited: HashSet<Uuid> = HashSet::new();
        let mut max_depth = 0;

        collect_tree_recursive(
            root_uuid,
            0,
            &media,
            &mut node_info,
            &mut visited,
            &mut max_depth,
        );

        log::debug!(
            "NodeEditor rebuild: root={}, nodes={}, max_depth={}",
            root_uuid,
            node_info.len(),
            max_depth
        );

        // Phase 2: Calculate Y positions for each depth level
        let mut depth_slots: HashMap<usize, usize> = HashMap::new();
        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();

        // Create all nodes with layout positions
        for (&uuid, info) in &node_info {
            let depth = info.depth;
            let slot = *depth_slots.get(&depth).unwrap_or(&0);
            depth_slots.insert(depth, slot + 1);

            // X: rightmost for root (depth=0), leftmost for deepest
            let x = (max_depth - depth) as f32 * HORIZONTAL_SPACING + 50.0;
            let y = slot as f32 * VERTICAL_SPACING + 50.0;

            log::debug!("NodeEditor: creating node {} at ({}, {})", uuid, x, y);
            let node_id = self.snarl.insert_node(Pos2::new(x, y), CompNode { uuid });
            uuid_to_node.insert(uuid, node_id);
        }

        // Phase 3: Create wires (child output -> parent input)
        for (&parent_uuid, info) in &node_info {
            if let Some(&parent_id) = uuid_to_node.get(&parent_uuid) {
                for (input_idx, &child_uuid) in info.children.iter().enumerate() {
                    if let Some(&child_id) = uuid_to_node.get(&child_uuid) {
                        let out_pin = OutPinId {
                            node: child_id,
                            output: 0,
                        };
                        let in_pin = InPinId {
                            node: parent_id,
                            input: input_idx,
                        };
                        let _ = self.snarl.connect(out_pin, in_pin);
                    }
                }
            }
        }

        // Request fit-all after rebuild
        self.fit_all_requested = true;
    }

    /// Re-layout existing nodes in a clean tree arrangement
    pub fn relayout(&mut self, project: &Project) {
        // Check if we have any nodes
        if self.snarl.node_ids().next().is_none() {
            return;
        }

        // Collect UUID -> NodeId mapping
        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();
        for (node_id, node) in self.snarl.node_ids() {
            uuid_to_node.insert(node.uuid, node_id);
        }

        // Find root - use the comp_uuid from state
        let Some(root_uuid) = self.comp_uuid else { return };
        let media = project.media.read().expect("media lock");

        // Collect tree info
        let mut node_info: HashMap<Uuid, NodeInfo> = HashMap::new();
        let mut visited: HashSet<Uuid> = HashSet::new();
        let mut max_depth = 0;

        collect_tree_recursive(
            root_uuid,
            0,
            &media,
            &mut node_info,
            &mut visited,
            &mut max_depth,
        );

        // Build new positions map
        let mut new_positions: HashMap<Uuid, Pos2> = HashMap::new();
        let mut depth_slots: HashMap<usize, usize> = HashMap::new();

        for (&uuid, info) in &node_info {
            let depth = info.depth;
            let slot = *depth_slots.get(&depth).unwrap_or(&0);
            depth_slots.insert(depth, slot + 1);

            let x = (max_depth - depth) as f32 * HORIZONTAL_SPACING + 50.0;
            let y = slot as f32 * VERTICAL_SPACING + 50.0;
            new_positions.insert(uuid, Pos2::new(x, y));
        }

        // Apply new positions using nodes_info_mut (gives &mut Node with pub pos field)
        for node in self.snarl.nodes_info_mut() {
            if let Some(&new_pos) = new_positions.get(&node.value.uuid) {
                node.pos = new_pos;
            }
        }

        self.fit_all_requested = true;
    }
}

/// Info collected during tree traversal (only layout-relevant data)
struct NodeInfo {
    depth: usize,
    children: Vec<Uuid>,
}

/// Recursively collect all nodes in the composition tree
fn collect_tree_recursive(
    uuid: Uuid,
    depth: usize,
    media: &RwLockReadGuard<'_, HashMap<Uuid, Comp>>,
    node_info: &mut HashMap<Uuid, NodeInfo>,
    visited: &mut HashSet<Uuid>,
    max_depth: &mut usize,
) {
    // Cycle detection
    if visited.contains(&uuid) {
        return;
    }
    visited.insert(uuid);

    *max_depth = (*max_depth).max(depth);

    let Some(comp) = media.get(&uuid) else {
        // Unknown comp - add minimal info
        node_info.insert(uuid, NodeInfo { depth, children: vec![] });
        return;
    };

    // Collect children UUIDs
    let mut children: Vec<Uuid> = vec![];
    for (_, attrs) in comp.get_children() {
        if let Some(source_str) = attrs.get_str("uuid") {
            if let Ok(child_uuid) = Uuid::parse_str(source_str) {
                children.push(child_uuid);
            }
        }
    }

    node_info.insert(uuid, NodeInfo { depth, children: children.clone() });

    // Recurse into children
    for child_uuid in children {
        collect_tree_recursive(child_uuid, depth + 1, media, node_info, visited, max_depth);
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
/// 3. Renders toolbar with A/F/L buttons
/// 4. Renders via egui-snarl with default styling
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

    // Handle layout request
    if state.layout_requested {
        state.layout_requested = false;
        state.relayout(project);
    }

    // Toolbar
    ui.horizontal(|ui| {
        ui.add_space(4.0);

        // A - fit All nodes
        if ui
            .button("A")
            .on_hover_text("Fit All - zoom to see all nodes")
            .clicked()
        {
            state.fit_all_requested = true;
        }

        // F - fit selected (or all if none selected)
        if ui
            .button("F")
            .on_hover_text("Fit - zoom to selected nodes (or all)")
            .clicked()
        {
            state.fit_all_requested = true;
        }

        // L - Layout nodes
        if ui
            .button("L")
            .on_hover_text("Layout - arrange nodes in tree")
            .clicked()
        {
            state.layout_requested = true;
        }

        ui.separator();

        // Node count info
        let node_count = state.snarl.node_ids().count();
        ui.label(format!("{} nodes", node_count));
    });

    ui.separator();

    // Viewer with project reference for resolving comp data
    let mut viewer = CompNodeViewer {
        project,
        output_uuid: comp.get_uuid(),
    };

    // Render with default styling
    let style = SnarlStyle::default();
    state.snarl.show(&mut viewer, &style, "comp_node_editor", ui);
}
