//! Project panel / media pool — now a thin playa glue layer over the reusable
//! `egui-asset-browser` widget.
//!
//! The widget owns the grouped row list rendering, selection/hover painting,
//! the per-item delete control, the right-click "Create" menu and drag-out
//! start reporting. It is engine-agnostic: it speaks plain `u64` ids and
//! returns a `Vec<AssetAction>` describing user intent.
//!
//! This module keeps only the playa-specific glue the widget intentionally does
//! NOT do:
//! - Save / Load project buttons + their `rfd` file dialogs.
//! - The "Add media" file dialog (wired to [`AssetAction::AddMedia`]).
//! - The +Folder / +AI / Clear top controls (folder dialog, AI provider
//!   default, clear-all — none expressible through the generic widget).
//! - Translating each [`AssetAction`] back into the existing playa events.
//! - The `Uuid <-> u64` id bridge (the widget is `Uuid`-free).

use eframe::egui;
use std::collections::HashMap;
use uuid::Uuid;

use egui_asset_browser::{
    AssetAction, AssetBrowserConfig, AssetBrowserModel, AssetItem, KindStyle,
    show as asset_browser_show,
};

use crate::widgets::dnd::{GlobalDragState, global_drag_state_id};
use crate::widgets::file_dialogs::create_media_dialog;
use crate::widgets::project::project::ProjectActions;
use crate::widgets::project::project_events::*;
use playa_engine::core::player::Player;
use playa_engine::entities::Project;
use playa_engine::entities::node::Node;

/// Per-frame metadata carried alongside the stable `u64` id so widget actions
/// (which only know `u64`) can be translated back into playa's `Uuid` world
/// without re-locking the media pool.
struct ItemMeta {
    /// Original node UUID.
    uuid: Uuid,
    /// Frame count, used as the drag-out duration hint.
    frame_count: i32,
}

/// Derive a stable `u64` id from a `Uuid` (first 8 bytes, little-endian).
///
/// Random v4 UUIDs make a head collision astronomically unlikely; the
/// per-frame `id_map` remains the single source of truth for the reverse
/// lookup, and unknown ids are simply ignored when translating actions.
fn uuid_to_u64(uuid: &Uuid) -> u64 {
    let bytes = uuid.as_bytes();
    let mut head = [0u8; 8];
    head.copy_from_slice(&bytes[0..8]);
    u64::from_le_bytes(head)
}

/// Build the asset-browser config: create-menu entries + the per-kind
/// icon/colour table mirroring the old hard-coded styling. Grouping is off to
/// preserve playa's flat, user-ordered media list; rename is off because there
/// is no project rename event.
fn build_config() -> AssetBrowserConfig {
    AssetBrowserConfig::default()
        .with_create_kinds(["Comp", "Camera", "Text"])
        .with_add_media(true)
        .with_rename(false)
        .with_grouping(false)
        .with_kind_style(
            "Clip",
            KindStyle::new("[F]", egui::Color32::from_rgb(100, 180, 100)),
        )
        .with_kind_style(
            "Comp",
            KindStyle::new("[C]", egui::Color32::from_rgb(100, 150, 255)),
        )
        .with_kind_style(
            "Camera",
            KindStyle::new("[K]", egui::Color32::from_rgb(255, 200, 100)),
        )
        .with_kind_style(
            "Text",
            KindStyle::new("[T]", egui::Color32::from_rgb(200, 150, 255)),
        )
        .with_kind_style(
            "AI",
            KindStyle::new("[AI]", egui::Color32::from_rgb(255, 150, 150)),
        )
        .with_kind_style(
            "Ref",
            KindStyle::new("[R]", egui::Color32::from_rgb(180, 180, 180)),
        )
}

/// Render project window (dock tab): unified list of Clips & Compositions,
/// driven by the `egui-asset-browser` widget.
pub fn render(ui: &mut egui::Ui, _player: &mut Player, project: &Project) -> ProjectActions {
    let mut actions = ProjectActions::new();

    // Capture the full panel rect up-front for hover detection (input routing).
    // A hover-only sense never competes with the widget's click/drag handling.
    let panel_rect = ui.available_rect_before_wrap();

    // --- playa-specific top controls (glue: file dialogs + AI + Clear) -------
    ui.horizontal(|ui| {
        if ui.button("Save").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["json"])
                .set_title("Save Project")
                .save_file()
        {
            actions.send(SaveProjectEvent(path));
        }
        if ui.button("Load").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Playa Project", &["json"])
                .set_title("Load Project")
                .pick_file()
        {
            actions.send(LoadProjectEvent(path));
        }
        ui.separator();
        if ui.button("+Folder").clicked()
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Add Media Folder")
                .pick_folder()
        {
            actions.send(AddFolderEvent(folder));
        }
        if ui
            .button("+AI")
            .on_hover_text(
                "Create a new AINode (text-to-video by default). Edit the\n\
                 prompt / provider / seed in the Attribute Editor, then click\n\
                 Generate to submit a Generation.",
            )
            .clicked()
        {
            actions.send(AddAINodeEvent {
                name: "AI Generation".to_string(),
                provider: "seedance.text_to_video".to_string(),
            });
        }
        ui.separator();
        if ui.button("Clear").clicked() {
            actions.send(ClearAllMediaEvent);
        }
    });
    ui.separator();

    // --- Build the asset-browser model from the project (this frame) ---------
    let config = build_config();
    let mut model = AssetBrowserModel::new();
    let mut id_map: HashMap<u64, ItemMeta> = HashMap::new();

    let order = project.order();
    let active = project.active();
    {
        let media = project.media.read().unwrap_or_else(|e| e.into_inner());
        for uuid in &order {
            let Some(node) = media.get(uuid) else {
                continue;
            };
            // Skip unlisted items (the preview comp singleton).
            if !node.is_listed() {
                continue;
            }

            let id = uuid_to_u64(uuid);
            let frame_count = node.frame_count();
            let fps = node.fps() as u32;

            // Per-kind label + display name, mirroring the old icon/label table.
            let (kind, name) = if node.is_file() {
                let name = if let Some(mask) = node.file_mask() {
                    let filename = std::path::Path::new(&mask)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&mask)
                        .to_string();
                    format!("{} • {}", node.name(), filename)
                } else {
                    node.name().to_string()
                };
                ("Clip", name)
            } else if node.is_camera() {
                ("Camera", node.name().to_string())
            } else if node.is_text() {
                ("Text", node.name().to_string())
            } else if node.is_ai() {
                ("AI", node.name().to_string())
            } else if node.is_ref() {
                ("Ref", node.name().to_string())
            } else {
                ("Comp", format!("{} (Layer)", node.name()))
            };

            // Active-node indicator. The widget exposes no per-item "active"
            // channel (`KindStyle` is icon+colour only), so the old green
            // stripe is substituted by a caller-side name marker. This is a
            // model-side choice, not a widget hack. See module/report caveat.
            let name = if active.as_ref() == Some(uuid) {
                format!("▶ {name}")
            } else {
                name
            };

            model.items.push(
                AssetItem::new(id, name, kind).with_subtitle(format!("{frame_count}f  {fps}fps")),
            );
            id_map.insert(
                id,
                ItemMeta {
                    uuid: *uuid,
                    frame_count,
                },
            );
        }
    }

    // Map the current Uuid selection to the widget's u64 id space.
    model.selection = project.selection().iter().map(uuid_to_u64).collect();

    // --- Render the widget and translate its actions back to playa events ----
    let raw_actions = asset_browser_show(ui, &model, &config);
    let ctx = ui.ctx().clone();
    for action in raw_actions {
        translate_action(action, project, &order, &id_map, &ctx, &mut actions);
    }

    // Hover state for input routing (hover-only sense; no click contention).
    actions.hovered = ui
        .interact(
            panel_rect,
            ui.id().with("project_panel_hover"),
            egui::Sense::hover(),
        )
        .hovered();

    actions
}

/// Translate one [`AssetAction`] from the widget into the corresponding playa
/// event(s). Actions whose id is not in `id_map` (stale frame) are ignored.
fn translate_action(
    action: AssetAction,
    project: &Project,
    order: &[Uuid],
    id_map: &HashMap<u64, ItemMeta>,
    ctx: &egui::Context,
    actions: &mut ProjectActions,
) {
    match action {
        // Single click: replicate the old plain/ctrl/shift selection model.
        AssetAction::Select {
            id,
            additive,
            range,
        } => {
            let Some(meta) = id_map.get(&id) else {
                return;
            };
            emit_selection(project, order, meta.uuid, additive, range, actions);
        }
        // Double click: activate (show in timeline/viewport). The widget emits
        // a preceding `Select`, so selection events are already covered.
        AssetAction::Open { id } => {
            let Some(meta) = id_map.get(&id) else {
                return;
            };
            actions
                .events
                .push(Box::new(ProjectActiveChangedEvent::new(meta.uuid)));
        }
        // Click on empty space → clear selection (new, widget-provided UX).
        AssetAction::ClearSelection => {
            actions.events.push(Box::new(ProjectSelectionChangedEvent {
                selection: Vec::new(),
                anchor: None,
            }));
            actions
                .events
                .push(Box::new(SelectionFocusEvent(Vec::new())));
        }
        // Create-menu entries → the matching create event. Clip/Folder/AI are
        // handled by the glue top row, not the generic Create menu.
        AssetAction::Create { kind } => match kind.as_str() {
            "Comp" => actions.send(AddCompEvent {
                name: "New Comp".to_string(),
                fps: 30.0,
            }),
            "Camera" => actions.send(AddCameraEvent {
                name: "Camera 1".to_string(),
            }),
            "Text" => actions.send(AddTextEvent {
                name: "New Text".to_string(),
                text: "Hello World".to_string(),
            }),
            other => log::warn!("project panel: unhandled create kind '{other}'"),
        },
        AssetAction::Delete { id } => {
            let Some(meta) = id_map.get(&id) else {
                return;
            };
            actions.send(RemoveMediaEvent(meta.uuid));
        }
        // "Add media" button → playa's file dialog (the old +Clip behaviour).
        AssetAction::AddMedia => {
            if let Some(paths) = create_media_dialog("Add Media Files").pick_files()
                && !paths.is_empty()
            {
                actions.send(AddClipsEvent(paths));
            }
        }
        // Drag-out start → seed the global drag state for the timeline ghost.
        AssetAction::BeginDrag { id } => {
            let Some(meta) = id_map.get(&id) else {
                return;
            };
            let source_uuid = meta.uuid;
            let duration = Some(meta.frame_count);
            ctx.data_mut(|data| {
                data.insert_temp(
                    global_drag_state_id(),
                    GlobalDragState::ProjectItem {
                        source_uuid,
                        duration,
                    },
                );
            });
        }
        // Rename is disabled in config (no project rename event); ignore.
        AssetAction::Rename { .. } => {}
    }
}

/// Emit the selection-change + focus events for a clicked node, computing the
/// new selection with the same plain/ctrl/shift semantics as the old panel.
fn emit_selection(
    project: &Project,
    order: &[Uuid],
    clicked_uuid: Uuid,
    additive: bool,
    range: bool,
    actions: &mut ProjectActions,
) {
    let Some(clicked_idx) = order.iter().position(|u| *u == clicked_uuid) else {
        return;
    };
    let current = project.selection();
    let (sel, anchor) = compute_selection(
        order,
        &current,
        project.selection_anchor,
        clicked_idx,
        additive,
        range,
    );
    actions.events.push(Box::new(ProjectSelectionChangedEvent {
        selection: sel.clone(),
        anchor,
    }));
    actions.events.push(Box::new(SelectionFocusEvent(sel)));
}

/// Compute a new selection from a click.
///
/// `range` (shift) extends from the anchor; `additive` (ctrl/cmd) toggles the
/// clicked item; otherwise the click replaces the selection. Behaviour is
/// identical to the pre-widget panel — only the modifier source changed (the
/// widget already decoded `egui::Modifiers` into `additive` / `range`).
fn compute_selection(
    order: &[Uuid],
    current_selection: &[Uuid],
    anchor: Option<usize>,
    clicked_idx: usize,
    additive: bool,
    range: bool,
) -> (Vec<Uuid>, Option<usize>) {
    let mut selection: Vec<Uuid> = current_selection.to_vec();
    let mut new_anchor = anchor;

    if range {
        let anchor_idx = new_anchor
            .or_else(|| {
                selection
                    .last()
                    .and_then(|u| order.iter().position(|x| x == u))
            })
            .unwrap_or(clicked_idx);
        let (start, end) = if anchor_idx <= clicked_idx {
            (anchor_idx, clicked_idx)
        } else {
            (clicked_idx, anchor_idx)
        };
        for u in order.iter().skip(start).take(end - start + 1) {
            if !selection.contains(u) {
                selection.push(*u);
            }
        }
        new_anchor = Some(clicked_idx);
    } else if additive {
        if let Some(pos) = selection.iter().position(|u| *u == order[clicked_idx]) {
            selection.remove(pos);
        } else {
            selection.push(order[clicked_idx]);
        }
        new_anchor = Some(clicked_idx);
    } else {
        selection.clear();
        selection.push(order[clicked_idx]);
        new_anchor = Some(clicked_idx);
    }

    (selection, new_anchor)
}
