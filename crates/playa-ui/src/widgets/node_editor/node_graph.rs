//! Node Editor widget — visual graph of the comp hierarchy on the `nodes-rs`
//! wgpu graph engine.
//!
//! The heavy lifting (GPU viewport, camera, pan/zoom/select, layout) lives in
//! `nodes-egui` / `nodes-view`. This module is the thin playa shell:
//!
//! 1. Build a `nodes_core::Subnet` from the comp DAG (one node per comp/layer,
//!    child→parent wires) — a *view* of playa's comp engine, which stays the
//!    source of truth.
//! 2. Implement [`GraphViewportHost`] over that subnet + registries.
//! 3. Drive [`GraphViewportState::ui`] each frame (read-only `DIAGRAM` config:
//!    pan/zoom/select/drag, no graph editing).
//!
//! The GPU device is wired once at startup via
//! [`NodeEditorState::configure_wgpu_render_state`] (called from the app's
//! eframe creation closure). Until then the tab shows a placeholder.
//!
//! v1 is rudimentary + view-only; the previous egui-snarl editor was the same.
//! KNOWN LIMITATION: a `PlayaNode` has a single `in` pin, so a parent with
//! multiple children shows one wire (the model's input holds one source). Full
//! multi-input wiring is a follow-up.

use std::collections::HashMap;
use std::sync::RwLockReadGuard;

use eframe::egui::{Pos2, Ui};
use nodes_core::{NodeId, NodeTypeRegistry, Subnet, ValueTypes};
use nodes_egui::{GraphViewportConfig, GraphViewportExtras, GraphViewportHost, GraphViewportState};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use playa_engine::core::event_bus::BoxedEvent;
use playa_engine::entities::node::Node;
use playa_engine::entities::{AttrValue, Project};

/// Fallback grid spacing when a node has no stored `node_pos`.
const HORIZONTAL_SPACING: f32 = 220.0;
const VERTICAL_SPACING: f32 = 120.0;

/// The node type every comp/layer is instantiated as in the view graph. One
/// generic input + output; the cook is a no-op (this graph never evaluates —
/// playa's comp engine does the real work).
const NODE_TYPE: &str = "PlayaNode";

fn default_true() -> bool {
    true
}

/// The `nodes-core` graph model + registries that back the viewport. Kept
/// separate from [`GraphViewportState`] so the viewport (`&mut gvs`) and the
/// host (`&mut host`) can be borrowed disjointly in `gvs.ui(ui, &mut host, …)`.
struct PlayaGraphHost {
    subnet: Subnet,
    registry: NodeTypeRegistry,
    value_types: ValueTypes,
}

impl Default for PlayaGraphHost {
    fn default() -> Self {
        let mut value_types = ValueTypes::new();
        nodes_core::register_builtin_types(&mut value_types);

        let mut registry = NodeTypeRegistry::new();
        nodes_core::node!(
            registry,
            "PlayaNode",
            "Playa",
            [nodes_core::prelude::any("in")] => [nodes_core::prelude::any("out")],
            |_node, _graph, _registry, _types| Ok(())
        );

        Self {
            subnet: Subnet::new(),
            registry,
            value_types,
        }
    }
}

impl GraphViewportHost for PlayaGraphHost {
    fn subnet(&self) -> &Subnet {
        &self.subnet
    }
    fn subnet_mut(&mut self) -> &mut Subnet {
        &mut self.subnet
    }
    fn registry(&self) -> &NodeTypeRegistry {
        &self.registry
    }
    fn value_types(&self) -> &ValueTypes {
        &self.value_types
    }
}

/// Persistent node-editor state. The GPU viewport + graph model are runtime-only
/// (`#[serde(skip)]`); only the lightweight sync flags would persist, and they
/// reset to a clean rebuild on load.
#[derive(Serialize, Deserialize)]
pub struct NodeEditorState {
    /// GPU graph viewport (camera, render, input). Configured with the wgpu
    /// device at startup; a fresh one is non-functional until then.
    #[serde(skip)]
    gvs: GraphViewportState,
    /// The comp-derived graph model + registries the viewport renders.
    #[serde(skip)]
    host: PlayaGraphHost,
    /// `true` once the wgpu device has been attached.
    #[serde(skip)]
    configured: bool,
    /// Comp currently shown; syncs from the player like the timeline.
    #[serde(skip)]
    comp_uuid: Option<Uuid>,
    /// Rebuild the subnet from the comp on the next frame.
    #[serde(skip, default = "default_true")]
    needs_rebuild: bool,
    /// Fit-all viewport on the next frame (set by node-editor events / hotkeys).
    #[serde(skip)]
    pub fit_all_requested: bool,
    /// Fit-selected viewport on the next frame.
    #[serde(skip)]
    pub fit_selected_requested: bool,
    /// Auto-layout the subnet on the next frame.
    #[serde(skip)]
    pub layout_requested: bool,
}

impl Default for NodeEditorState {
    fn default() -> Self {
        Self {
            gvs: GraphViewportState::default(),
            host: PlayaGraphHost::default(),
            configured: false,
            comp_uuid: None,
            needs_rebuild: true,
            fit_all_requested: false,
            fit_selected_requested: false,
            layout_requested: false,
        }
    }
}

impl NodeEditorState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the eframe/egui wgpu device so the GPU viewport can render. Call
    /// once at startup with `CreationContext::wgpu_render_state`.
    pub fn configure_wgpu_render_state(&mut self, render_state: eframe::egui_wgpu::RenderState) {
        self.gvs.configure_wgpu_render_state(render_state);
        self.configured = true;
    }

    /// Mark the graph as needing a rebuild from the comp.
    pub fn mark_dirty(&mut self) {
        self.needs_rebuild = true;
    }

    /// Set the composition to display (triggers a rebuild when it changes).
    pub fn set_comp(&mut self, comp_uuid: Uuid) {
        if self.comp_uuid != Some(comp_uuid) {
            self.comp_uuid = Some(comp_uuid);
            self.needs_rebuild = true;
        }
    }
}

/// Rebuild the host's subnet from the comp DAG: one `PlayaNode` per comp/layer,
/// positioned from the stored `node_pos` (else a depth grid), wired child→parent.
fn rebuild_subnet(host: &mut PlayaGraphHost, project: &Project, comp_uuid: Uuid) {
    host.subnet = Subnet::new();

    let media = project.media.read().expect("media lock");
    let mut node_info: HashMap<Uuid, NodeInfo> = HashMap::new();
    let mut ancestors: Vec<Uuid> = Vec::new();
    let mut max_depth = 0;
    collect_tree_recursive(
        comp_uuid,
        comp_uuid,
        0,
        &media,
        &mut node_info,
        &mut ancestors,
        &mut max_depth,
    );
    drop(media);

    // Create nodes (positions: stored node_pos, else a depth/slot grid).
    let mut depth_slots: HashMap<usize, usize> = HashMap::new();
    let mut uuid_to_node: HashMap<Uuid, NodeId> = HashMap::new();
    for info in node_info.values() {
        let slot = *depth_slots.get(&info.depth).unwrap_or(&0);
        depth_slots.insert(info.depth, slot + 1);
        let default_x = (max_depth - info.depth) as f32 * HORIZONTAL_SPACING + 50.0;
        let default_y = slot as f32 * VERTICAL_SPACING + 50.0;
        let pos = load_node_pos(
            project,
            comp_uuid,
            info.instance_uuid,
            Pos2::new(default_x, default_y),
        );
        if let Ok(id) =
            host.subnet
                .create_node(NODE_TYPE, [pos.x, pos.y], &host.registry, &host.value_types)
        {
            uuid_to_node.insert(info.instance_uuid, id);
        }
    }

    // Wire child output → parent input (single input per node — see module note).
    for info in node_info.values() {
        let Some(&parent) = uuid_to_node.get(&info.instance_uuid) else {
            continue;
        };
        for (child_instance, _) in &info.children {
            if let Some(&child) = uuid_to_node.get(child_instance) {
                let _ = host
                    .subnet
                    .connect(child, "out", parent, "in", &host.registry);
            }
        }
    }
}

/// Render the node editor tab. Same signature as before so the caller is
/// unchanged. Returns whether the pointer is over the panel.
pub fn render_node_editor(
    ui: &mut Ui,
    state: &mut NodeEditorState,
    project: &Project,
    comp_uuid: Uuid,
    _dispatch: impl FnMut(BoxedEvent),
) -> bool {
    state.set_comp(comp_uuid);

    if !state.configured {
        ui.centered_and_justified(|ui| {
            ui.weak("Node editor: GPU viewport not initialised yet.");
        });
        return false;
    }

    if state.needs_rebuild {
        rebuild_subnet(&mut state.host, project, comp_uuid);
        state.needs_rebuild = false;
    }

    // Honor fit / layout requests (from hotkeys handled here or node events).
    // `gvs.ui` also handles A/F/L while hovered; these cover programmatic requests.
    if state.fit_all_requested || state.fit_selected_requested {
        state.fit_all_requested = false;
        state.fit_selected_requested = false;
        state.gvs.request_initial_fit();
    }
    if state.layout_requested {
        state.layout_requested = false;
        state.gvs.layout_nodes(&mut state.host.subnet);
    }

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.weak("A: fit all · F: fit selected · L: layout · MMB/wheel: pan/zoom");
        ui.separator();
        ui.label(format!("{} nodes", state.host.subnet.nodes.len()));
    });
    ui.separator();

    // `gvs` and `host` are disjoint fields, so both can be borrowed mutably.
    state.gvs.ui(
        ui,
        &mut state.host,
        &GraphViewportConfig::DIAGRAM,
        GraphViewportExtras::default(),
    );

    ui.rect_contains_pointer(ui.max_rect())
}

// =============================================================================
// Comp-DAG traversal helpers (independent of the rendering backend)
// =============================================================================

/// Info collected during tree traversal (only layout-relevant data).
struct NodeInfo {
    depth: usize,
    instance_uuid: Uuid,
    #[allow(dead_code)]
    source_uuid: Uuid,
    children: Vec<(Uuid, Uuid)>, // (instance_uuid, source_uuid)
}

/// Load a node's stored UI position from comp attrs (root) or layer attrs.
fn load_node_pos(project: &Project, comp_uuid: Uuid, instance_uuid: Uuid, default: Pos2) -> Pos2 {
    let maybe_pos = project
        .with_comp(comp_uuid, |comp| {
            let maybe_attr = if instance_uuid == comp.uuid() {
                comp.attrs.get("node_pos")
            } else {
                comp.layers_attrs_get(&instance_uuid)
                    .and_then(|a| a.get("node_pos"))
            };
            if let Some(AttrValue::Vec3([x, y, _])) = maybe_attr {
                Some(Pos2::new(*x, *y))
            } else {
                None
            }
        })
        .flatten();
    maybe_pos.unwrap_or(default)
}

/// Recursively collect all nodes in the composition tree.
fn collect_tree_recursive(
    instance_uuid: Uuid,
    source_uuid: Uuid,
    depth: usize,
    media: &RwLockReadGuard<'_, HashMap<Uuid, std::sync::Arc<playa_engine::entities::NodeKind>>>,
    node_info: &mut HashMap<Uuid, NodeInfo>,
    ancestors: &mut Vec<Uuid>,
    max_depth: &mut usize,
) {
    // Cycle detection by source path (allows multiple instances of the same comp).
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

    let children: Vec<(Uuid, Uuid)> = comp.get_children_sources();
    node_info.insert(
        instance_uuid,
        NodeInfo {
            depth,
            instance_uuid,
            source_uuid,
            children: children.clone(),
        },
    );

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
