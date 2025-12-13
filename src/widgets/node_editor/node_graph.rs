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
//! 3. `render_node_editor()` - displays via egui-snarl with toolbar and returns hover flag
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

use std::collections::HashMap;
use std::sync::RwLockReadGuard;

use eframe::egui::{Color32, Pos2, Ui};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId, OutPin, OutPinId, Snarl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::core::event_bus::BoxedEvent;
use crate::entities::{AttrValue, Comp, Project};
use crate::entities::node::Node;
use egui_snarl::ui::get_selected_nodes;
use crate::entities::Attrs;

/// Node in the composition graph - just a UUID reference to Comp.
/// All data (name, type, children) comes from project.media at render time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompNode {
    /// Layer/instance UUID (unique per placement inside parent)
    pub uuid: Uuid,
    /// Source comp UUID (used for name and child resolution)
    pub source_uuid: Uuid,
}

/// SnarlViewer implementation for rendering CompNode.
/// Holds reference to Project for resolving Comp data at render time.
struct CompNodeViewer<'a> {
    project: &'a Project,
    output_uuid: Uuid, // current comp being viewed
}

impl<'a> CompNodeViewer<'a> {
    fn get_node(&self, source_uuid: Uuid) -> Option<crate::entities::NodeKind> {
        self.project.media.read().ok()?.get(&source_uuid).cloned()
    }
    
    fn get_comp(&self, source_uuid: Uuid) -> Option<Comp> {
        self.project.media.read().ok()?.get(&source_uuid).and_then(|n| n.as_comp()).cloned()
    }
}

#[allow(refining_impl_trait)]
impl<'a> SnarlViewer<CompNode> for CompNodeViewer<'a> {
    fn title(&mut self, node: &CompNode) -> String {
        self.get_node(node.source_uuid)
            .map(|n| n.name().to_string())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    fn outputs(&mut self, _node: &CompNode) -> usize {
        1
    }

    fn inputs(&mut self, node: &CompNode) -> usize {
        // FileNodes have no inputs, CompNodes have layers as inputs
        self.get_comp(node.source_uuid)
            .map(|c| c.layers.len())
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
        let source_uuid = snarl[node].source_uuid;
        let comp = self.get_comp(source_uuid);

        let (icon, color, name) = match comp {
            Some(c) => {
                let is_output = source_uuid == self.output_uuid;
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

    /// Flag to trigger fit-selected on next frame
    #[serde(skip)]
    pub fit_selected_requested: bool,

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
            fit_selected_requested: false,
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

        let root_uuid = comp.uuid();
        let media = project.media.read().expect("media lock");

        log::trace!(
            "NodeEditor: rebuilding for comp '{}' ({}), media has {} items",
            comp.name(),
            root_uuid,
            media.len()
        );
        log::trace!("NodeEditor: comp is in media? {}", media.contains_key(&root_uuid));

        // Phase 1: Collect all nodes recursively with their depth and children count
        let mut node_info: HashMap<Uuid, NodeInfo> = HashMap::new();
        let mut ancestors: Vec<Uuid> = Vec::new();
        let mut max_depth = 0;

        collect_tree_recursive(
            root_uuid,
            root_uuid,
            0,
            &media,
            &mut node_info,
            &mut ancestors,
            &mut max_depth,
        );

        log::trace!(
            "NodeEditor rebuild: root={}, nodes={}, max_depth={}",
            root_uuid,
            node_info.len(),
            max_depth
        );

        // Phase 2: Calculate Y positions for each depth level
        let mut depth_slots: HashMap<usize, usize> = HashMap::new();
        let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();

        // Create all nodes with layout positions (prefer stored positions)
        for (_, info) in &node_info {
            let depth = info.depth;
            let slot = *depth_slots.get(&depth).unwrap_or(&0);
            depth_slots.insert(depth, slot + 1);

            // Default grid position
            let default_x = (max_depth - depth) as f32 * HORIZONTAL_SPACING + 50.0;
            let default_y = slot as f32 * VERTICAL_SPACING + 50.0;
            let default_pos = Pos2::new(default_x, default_y);

            let pos = load_node_pos(comp, info.instance_uuid, default_pos);

            log::trace!(
                "NodeEditor: creating node {} (src {}) at ({}, {})",
                info.instance_uuid,
                info.source_uuid,
                pos.x,
                pos.y
            );
            let node_id = self.snarl.insert_node(
                pos,
                CompNode {
                    uuid: info.instance_uuid,
                    source_uuid: info.source_uuid,
                },
            );
            uuid_to_node.insert(info.instance_uuid, node_id);
        }

        // Phase 3: Create wires (child output -> parent input)
        for (&parent_uuid, info) in &node_info {
            if let Some(&parent_id) = uuid_to_node.get(&parent_uuid) {
                for (input_idx, &(child_instance, _)) in info.children.iter().enumerate() {
                    if let Some(&child_id) = uuid_to_node.get(&child_instance) {
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
        let mut ancestors: Vec<Uuid> = Vec::new();
        let mut max_depth = 0;

        collect_tree_recursive(
            root_uuid,
            root_uuid,
            0,
            &media,
            &mut node_info,
            &mut ancestors,
            &mut max_depth,
        );

        // Build new positions map
        let mut new_positions: HashMap<Uuid, Pos2> = HashMap::new();
        let mut depth_slots: HashMap<usize, usize> = HashMap::new();

        for (&instance_uuid, info) in &node_info {
            let depth = info.depth;
            let slot = *depth_slots.get(&depth).unwrap_or(&0);
            depth_slots.insert(depth, slot + 1);

            let x = (max_depth - depth) as f32 * HORIZONTAL_SPACING + 50.0;
            let y = slot as f32 * VERTICAL_SPACING + 50.0;
            new_positions.insert(instance_uuid, Pos2::new(x, y));
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
    instance_uuid: Uuid,
    source_uuid: Uuid,
    children: Vec<(Uuid, Uuid)>, // (instance_uuid, source_uuid)
}

fn nodes_bounding_box(snarl: &Snarl<CompNode>, nodes: &[NodeId]) -> Option<(Pos2, Pos2)> {
    let mut min = Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    for node_id in nodes {
        if let Some(node) = snarl.get_node_info(*node_id) {
            min.x = min.x.min(node.pos.x);
            min.y = min.y.min(node.pos.y);
            max.x = max.x.max(node.pos.x);
            max.y = max.y.max(node.pos.y);
        }
    }

    if min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite() {
        Some((min, max))
    } else {
        None
    }
}

fn center_nodes(snarl: &mut Snarl<CompNode>, target: Pos2, nodes: &[NodeId]) {
    if nodes.is_empty() {
        return;
    }
    if let Some((min, max)) = nodes_bounding_box(snarl, nodes) {
        let center = Pos2::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5);
        let delta = target - center;
        for node_id in nodes {
            if let Some(node) = snarl.get_node_info_mut(*node_id) {
                node.pos += delta;
            }
        }
    }
}

fn load_node_pos(comp: &Comp, instance_uuid: Uuid, default: Pos2) -> Pos2 {
    let maybe_attr = if instance_uuid == comp.uuid() {
        comp.attrs.get("node_pos")
    } else {
        comp.layers_attrs_get(&instance_uuid)
            .and_then(|a| a.get("node_pos"))
    };

    if let Some(AttrValue::Vec3([x, y, _])) = maybe_attr {
        Pos2::new(*x, *y)
    } else {
        default
    }
}

fn set_node_pos(attrs: &mut Attrs, pos: Pos2) -> bool {
    let new_val = [pos.x, pos.y, 0.0];
    let mut changed = true;
    if let Some(AttrValue::Vec3(current)) = attrs.get("node_pos") {
        let dx = (current[0] - new_val[0]).abs();
        let dy = (current[1] - new_val[1]).abs();
        changed = dx > 0.001 || dy > 0.001;
    }
    if changed {
        attrs.set("node_pos", AttrValue::Vec3(new_val));
        attrs.clear_dirty(); // node_pos is UI-only; avoid cache invalidation
    }
    changed
}

/// Recursively collect all nodes in the composition tree
fn collect_tree_recursive(
    instance_uuid: Uuid,
    source_uuid: Uuid,
    depth: usize,
    media: &RwLockReadGuard<'_, HashMap<Uuid, crate::entities::NodeKind>>,
    node_info: &mut HashMap<Uuid, NodeInfo>,
    ancestors: &mut Vec<Uuid>,
    max_depth: &mut usize,
) {
    // Cycle detection by source path (allows multiple instances of the same comp elsewhere)
    if ancestors.contains(&source_uuid) {
        node_info.insert(
            instance_uuid,
            NodeInfo {
                depth,
                instance_uuid,
                source_uuid,
                children: vec![],
            },
        );
        return;
    }
    ancestors.push(source_uuid);

    *max_depth = (*max_depth).max(depth);

    let Some(comp) = media.get(&source_uuid).and_then(|n| n.as_comp()) else {
        // Unknown comp or FileNode - add minimal info
        node_info.insert(
            instance_uuid,
            NodeInfo {
                depth,
                instance_uuid,
                source_uuid,
                children: vec![],
            },
        );
        ancestors.pop();
        return;
    };

    // Collect children UUIDs
    let mut children: Vec<(Uuid, Uuid)> = vec![];
    for (layer_uuid, attrs) in comp.get_children() {
        if let Some(child_uuid) = attrs.get_uuid("uuid") {
            children.push((layer_uuid, child_uuid));
        }
    }

    node_info.insert(
        instance_uuid,
        NodeInfo {
            depth,
            instance_uuid,
            source_uuid,
            children: children.clone(),
        },
    );

    // Recurse into children
    for (child_instance, child_source) in children {
        collect_tree_recursive(
            child_instance,
            child_source,
            depth + 1,
            media,
            node_info,
            ancestors,
            max_depth,
        );
    }
    ancestors.pop();
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
    mut dispatch: impl FnMut(BoxedEvent),
) -> bool {
    let widget_id = ui.make_persistent_id("comp_node_editor");

    // Sync to current comp (sets needs_rebuild if comp changed)
    state.set_comp(comp.uuid());

    // Rebuild graph from Comp.children if needed
    if state.needs_rebuild {
        state.rebuild_from_comp(comp, project);
    }

    // Handle fit/layout requests from events
    if state.fit_all_requested {
        state.fit_all_requested = false;
        let all_nodes: Vec<NodeId> = state.snarl.node_ids().map(|(id, _)| id).collect();
        center_nodes(&mut state.snarl, ui.max_rect().center(), &all_nodes);
    }
    if state.fit_selected_requested {
        // Center selected nodes if any, otherwise fallback to all nodes
        let selected_nodes = get_selected_nodes(widget_id, ui.ctx());
        if !selected_nodes.is_empty() {
            center_nodes(&mut state.snarl, ui.max_rect().center(), &selected_nodes);
        } else {
            let all_nodes: Vec<NodeId> = state.snarl.node_ids().map(|(id, _)| id).collect();
            center_nodes(&mut state.snarl, ui.max_rect().center(), &all_nodes);
        }
        state.fit_selected_requested = false;
    }

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
        output_uuid: comp.uuid(),
    };

    // Render with default styling and detect node moves by comparing positions
    let style = SnarlStyle::default();
    let before_positions: HashMap<NodeId, Pos2> = state
        .snarl
        .nodes_pos_ids()
        .map(|(id, pos, _)| (id, pos))
        .collect();

    state
        .snarl
        .show(&mut viewer, &style, "comp_node_editor", ui);

    // Persist moved nodes via event bus (comp root uses direct attr update)
    if let Some(comp_uuid) = state.comp_uuid {
        let mut moved_layers = Vec::new();
        let mut moved_root = None;
        for (node_id, pos, node) in state.snarl.nodes_pos_ids() {
            let was = before_positions.get(&node_id).copied();
            if was.map(|p| p.distance(pos)).unwrap_or(f32::INFINITY) > 0.01 {
                if node.uuid == comp_uuid {
                    moved_root = Some(pos);
                } else {
                    moved_layers.push((node.uuid, pos));
                }
            }
        }

        if let Some(pos) = moved_root {
            project.modify_comp(comp_uuid, |c| {
                set_node_pos(&mut c.attrs, pos);
                c.attrs.clear_dirty();
            });
        }

        if !moved_layers.is_empty() {
            // Per-layer positions; send separate events to keep per-node values
            for (layer_uuid, pos) in moved_layers {
                dispatch(Box::new(crate::entities::comp_events::SetLayerAttrsEvent {
                    comp_uuid,
                    layer_uuids: vec![layer_uuid],
                    attrs: vec![("node_pos".to_string(), AttrValue::Vec3([pos.x, pos.y, 0.0]))],
                }));
            }
        }
    }
    ui.rect_contains_pointer(ui.max_rect())
}
