use eframe::egui;
use egui_ltreeview::TreeView;
use std::collections::HashMap;

use super::prefs_events::SetGizmoPrefsEvent;

/// Settings categories
#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsCategory {
    General,
    UI,
    Cache,
    Gizmo,
    Compositing,
    WebServer,
}

impl SettingsCategory {
    fn as_str(&self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::UI => "UI",
            SettingsCategory::Cache => "Cache",
            SettingsCategory::Gizmo => "Gizmo",
            SettingsCategory::Compositing => "Compositing",
            SettingsCategory::WebServer => "Web Server",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "General" => Some(SettingsCategory::General),
            "UI" => Some(SettingsCategory::UI),
            "Cache" => Some(SettingsCategory::Cache),
            "Gizmo" => Some(SettingsCategory::Gizmo),
            "Compositing" => Some(SettingsCategory::Compositing),
            "Web Server" => Some(SettingsCategory::WebServer),
            _ => None,
        }
    }
}

/// Compositor backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[derive(Default)]
pub enum CompositorBackend {
    #[default]
    Cpu,
    Gpu,
}

/// Event emitted when compositor backend changes
#[derive(Debug, Clone)]
pub struct CompositorBackendChangedEvent {
    pub backend: CompositorBackend,
}


/// UI Layout configuration (dock splits, timeline/viewport state)
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Layout {
    pub dock_state_json: String,
    pub timeline_zoom: f32,
    pub timeline_pan_offset: f32,
    pub timeline_outline_width: f32,
    pub timeline_view_mode: String,
    pub viewport_zoom: f32,
    pub viewport_pan: [f32; 2],
    pub viewport_mode: String,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            dock_state_json: String::new(),
            timeline_zoom: 1.0,
            timeline_pan_offset: 0.0,
            timeline_outline_width: 400.0,
            timeline_view_mode: "Split".to_string(),
            viewport_zoom: 1.0,
            viewport_pan: [0.0, 0.0],
            viewport_mode: "AutoFit".to_string(),
        }
    }
}

/// Application settings
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct AppSettings {
    // Playback
    pub fps_base: f32, // Base FPS (persistent)
    pub loop_enabled: bool,

    // Shader
    pub current_shader: String,

    // UI
    pub show_help: bool,
    pub show_playlist: bool,
    pub show_attributes_editor: bool,
    pub show_frame_numbers: bool, // Show frame numbers on timeslider
    pub show_tooltips: bool,      // Show tooltips on toolbar controls (2s delay)
    pub dark_mode: bool,
    pub font_size: f32,
    pub timeline_layer_height: f32,       // Layer row height in timeline (default 32.0)
    pub timeline_name_column_width: f32,  // Name column width in outline (default 150.0)
    pub timeline_outline_top_offset: f32, // Fine-tune outline vertical alignment with canvas
    pub timeline_snap_enabled: bool,
    pub timeline_lock_work_area: bool,
    pub viewport_hover_highlight: bool,
    pub timeline_hover_highlight: bool,
    pub preload_radius: i32, // Frames to preload around playhead (-1 = all, default 100)
    pub preload_delay_ms: u64, // Delay before full preload after attr change (default 500ms)

    // Workers (applied to App::workers / playback/encoding threads)
    pub workers_override: u32, // 0 = auto, N = override (applies on restart)

    // Cache & Memory
    pub cache_memory_percent: f32,      // 25-95% of available (default 75%)
    pub reserve_system_memory_gb: f32,  // Reserve for system (default 2.0 GB)
    pub cache_strategy: crate::entities::CacheStrategy, // Caching strategy (LastOnly or All)

    // Compositor backend (CPU or GPU)
    pub compositor_backend: CompositorBackend,

    // Encoding dialog
    pub encode_dialog: crate::dialogs::encode::EncodeDialogSettings,

    // Internal
    pub selected_settings_category: Option<String>,

    // REST API Server
    pub api_server_enabled: bool,
    pub api_server_port: Option<u16>,

    // Layouts (named UI configurations)
    pub layouts: HashMap<String, Layout>,
    pub current_layout: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            fps_base: 24.0,
            loop_enabled: true,
            current_shader: "default".to_string(),
            show_help: true,
            show_playlist: true,
            show_attributes_editor: true,
            show_frame_numbers: true,
            show_tooltips: true,
            dark_mode: true,
            font_size: 11.0,
            timeline_layer_height: 30.0,
            timeline_name_column_width: 80.0,
            timeline_outline_top_offset: 42.0, // fine-tuned for alignment with canvas
            timeline_snap_enabled: true,
            timeline_lock_work_area: false,
            viewport_hover_highlight: true,
            timeline_hover_highlight: false,
            preload_radius: -1,
            preload_delay_ms: 500,
            workers_override: 0,
            cache_memory_percent: 75.0,
            reserve_system_memory_gb: 2.0,
            cache_strategy: crate::entities::CacheStrategy::All, // Default: cache all frames
            compositor_backend: CompositorBackend::default(),
            encode_dialog: crate::dialogs::encode::EncodeDialogSettings::default(),
            selected_settings_category: Some("UI".to_string()),
            api_server_enabled: false,
            api_server_port: Some(9876),
            layouts: HashMap::new(),
            current_layout: String::new(),
        }
    }
}

/// Render General settings category
fn render_general_settings(ui: &mut egui::Ui, _settings: &mut AppSettings) {
    ui.label("General settings will be added here.");
}

/// Render Web Server settings category
fn render_webserver_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("REST API Server");
    ui.add_space(8.0);

    ui.checkbox(&mut settings.api_server_enabled, "Enable REST API server");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Port:");
        let mut port = settings.api_server_port.unwrap_or(9876) as i32;
        if ui.add(egui::DragValue::new(&mut port).range(1024..=65535)).changed() {
            settings.api_server_port = Some(port as u16);
        }
    });
    ui.add_space(12.0);

    if settings.api_server_enabled {
        let port = settings.api_server_port.unwrap_or(9876);
        ui.separator();
        ui.add_space(8.0);
        ui.label("Server URL:");
        ui.monospace(format!("http://0.0.0.0:{}", port));
        ui.add_space(8.0);
        ui.label("Endpoints:");
        ui.monospace("GET  /api/status");
        ui.monospace("GET  /api/player");
        ui.monospace("POST /api/player/play");
        ui.monospace("POST /api/player/pause");
        ui.monospace("POST /api/player/frame/{n}");
    } else {
        ui.add_space(8.0);
        ui.label("Enable to start the REST API server.");
    }
}

/// Render UI settings category
fn render_ui_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("Appearance");
    ui.add_space(8.0);

    ui.label("Font Size:");
    ui.add(
        egui::Slider::new(&mut settings.font_size, 10.0..=18.0)
            .suffix(" px")
            .step_by(0.5),
    );
    ui.add_space(8.0);

    ui.label("Timeline Layer Height:");
    ui.add(
        egui::Slider::new(&mut settings.timeline_layer_height, 20.0..=64.0)
            .suffix(" px")
            .step_by(2.0),
    );
    ui.add_space(8.0);

    ui.label("Timeline Name Column Width:");
    ui.add(
        egui::Slider::new(&mut settings.timeline_name_column_width, 80.0..=300.0)
            .suffix(" px")
            .step_by(10.0),
    );
    ui.add_space(8.0);

    ui.label("Timeline Outline Top Offset:");
    ui.add(
        egui::Slider::new(&mut settings.timeline_outline_top_offset, 0.0..=120.0)
            .suffix(" px")
            .step_by(1.0),
    );
    ui.add_space(16.0);

    ui.checkbox(&mut settings.dark_mode, "Dark Mode");
    ui.checkbox(&mut settings.show_tooltips, "Show Tooltips (2s delay on toolbar controls)");
    ui.checkbox(&mut settings.viewport_hover_highlight, "Viewport hover highlight");
    ui.checkbox(&mut settings.timeline_hover_highlight, "Timeline hover highlight");

    ui.add_space(16.0);
    ui.heading("Performance");
    ui.add_space(8.0);

    ui.label("Worker Threads Override (0 = Auto):");
    ui.add(
        egui::DragValue::new(&mut settings.workers_override)
            .speed(1.0)
            .range(0..=256),
    );
    ui.label("Takes effect on next launch. Defaults to ~75% of CPU cores.");

    ui.add_space(8.0);
    ui.label("Preload/cache settings moved to Settings → Cache.");
}

/// Render Cache settings category
fn render_cache_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("Preload");
    ui.add_space(8.0);

    ui.label("Preload Radius (frames):");
    ui.horizontal(|ui| {
        if settings.preload_radius < 0 {
            ui.label("All");
            if ui.small_button("Set limit").clicked() {
                settings.preload_radius = 100;
            }
        } else {
            ui.add(
                egui::Slider::new(&mut settings.preload_radius, 10..=500)
                    .step_by(10.0),
            );
            if ui.small_button("All").clicked() {
                settings.preload_radius = -1;
            }
        }
    });
    ui.label("Frames to preload around playhead (-1 = entire comp).");

    ui.add_space(8.0);
    ui.label("Preload Delay (ms):");
    ui.add(
        egui::Slider::new(&mut settings.preload_delay_ms, 0..=2000)
            .suffix(" ms")
            .step_by(50.0),
    );
    ui.label("Delay before full preload after attribute change. 0 = immediate.");

    ui.add_space(16.0);
    ui.heading("Cache & Memory");
    ui.add_space(8.0);

    ui.label("Cache Memory Limit (% of available):");
    ui.add(
        egui::Slider::new(&mut settings.cache_memory_percent, 25.0..=95.0)
            .suffix("%")
            .step_by(5.0),
    );
    ui.label("Maximum memory used for frame caching.");

    ui.add_space(8.0);
    ui.label("Reserve for System (GB):");
    ui.add(
        egui::Slider::new(&mut settings.reserve_system_memory_gb, 0.5..=8.0)
            .suffix(" GB")
            .step_by(0.5),
    );
    ui.label("Minimum memory reserved for OS and other apps.");

    ui.add_space(8.0);
    ui.label("Cache Strategy:");
    ui.horizontal(|ui| {
        use crate::entities::CacheStrategy;
        ui.radio_value(&mut settings.cache_strategy, CacheStrategy::All, "All Frames");
        ui.radio_value(&mut settings.cache_strategy, CacheStrategy::LastOnly, "Last Only");
    });
    ui.label("All Frames: Maximum performance, more memory usage.");
    ui.label("Last Only: Minimal memory, only last accessed frame per comp.");
}

/// Render Gizmo settings category (stored in the current Project)
fn render_gizmo_settings(
    ui: &mut egui::Ui,
    project: Option<&crate::entities::Project>,
    event_bus: Option<&crate::core::event_bus::EventBus>,
) {
    ui.heading("Gizmo");
    ui.add_space(8.0);

    let Some(project) = project else {
        ui.label("No active project - gizmo settings are stored per project.");
        return;
    };

    let Some(bus) = event_bus else {
        ui.label("Event bus unavailable - cannot apply changes.");
        return;
    };

    let current = project.gizmo_prefs();
    let mut next = current.clone();
    let mut changed = false;

    ui.label("These settings are saved inside the Project (attrs.prefs). ");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Size");
        changed |= ui
            .add(egui::Slider::new(&mut next.pref_manip_size, 20.0..=250.0).suffix(" px"))
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label("Stroke width");
        changed |= ui
            .add(egui::Slider::new(&mut next.pref_manip_stroke_width, 0.5..=8.0).suffix(" px"))
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label("Inactive alpha");
        changed |= ui
            .add(egui::Slider::new(&mut next.pref_manip_inactive_alpha, 0.0..=1.0))
            .changed();
    });

    ui.horizontal(|ui| {
        ui.label("Highlight alpha");
        changed |= ui
            .add(egui::Slider::new(&mut next.pref_manip_highlight_alpha, 0.0..=1.0))
            .changed();
    });

    if changed {
        bus.emit(SetGizmoPrefsEvent(next));
        ui.ctx().request_repaint();
    }
}

/// Render Compositing settings category
fn render_compositing_settings(
    ui: &mut egui::Ui,
    settings: &mut AppSettings,
    event_bus: Option<&crate::core::event_bus::EventBus>,
) {
    ui.heading("Backend");
    ui.add_space(8.0);

    let prev_backend = settings.compositor_backend;
    ui.horizontal(|ui| {
        ui.label("Compositor:");
        ui.radio_value(&mut settings.compositor_backend, CompositorBackend::Cpu, "CPU");
        ui.radio_value(&mut settings.compositor_backend, CompositorBackend::Gpu, "GPU");
    });
    // Emit event if changed
    if settings.compositor_backend != prev_backend
        && let Some(bus) = event_bus {
            bus.emit(CompositorBackendChangedEvent {
                backend: settings.compositor_backend,
            });
        }
    ui.label("GPU compositor uses OpenGL for 10-50x faster multi-layer blending.");
    ui.label("Requires OpenGL 3.0+. Falls back to CPU on errors.");

    ui.add_space(16.0);
    ui.heading("Safety");
    ui.add_space(8.0);
    ui.label("Cycle detection is always enabled.");
    ui.label("Prevents infinite loops when compositions reference each other.");
}

/// Render settings window
pub fn render_settings_window(
    ctx: &egui::Context,
    show_settings: &mut bool,
    settings: &mut AppSettings,
    project: Option<&crate::entities::Project>,
    event_bus: Option<&crate::core::event_bus::EventBus>,
) {
    // Get selected category from settings or use default
    let mut selected = settings
        .selected_settings_category
        .as_ref()
        .and_then(|s| SettingsCategory::from_str(s))
        .unwrap_or(SettingsCategory::UI);

    egui::Window::new("Settings")
        .id(egui::Id::new("settings_window"))
        .open(show_settings)
        .default_size([700.0, 500.0])
        .min_size([500.0, 400.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Left panel: TreeView (200px fixed width)
                        ui.vertical(|ui| {
                            ui.set_width(200.0);
                            ui.add_space(4.0);

                            let tree_id = ui.make_persistent_id("settings_tree_view");
                            let (_response, actions) = TreeView::new(tree_id).show(ui, |builder| {
                                builder.leaf(0, SettingsCategory::General.as_str());
                                builder.leaf(1, SettingsCategory::UI.as_str());
                                builder.leaf(2, SettingsCategory::Cache.as_str());
                                builder.leaf(3, SettingsCategory::Gizmo.as_str());
                                builder.leaf(4, SettingsCategory::Compositing.as_str());
                                builder.leaf(5, SettingsCategory::WebServer.as_str());
                            });

                            // Handle selection from actions
                            for action in actions {
                                if let egui_ltreeview::Action::SetSelected(node_ids) = action
                                    && let Some(&node_id) = node_ids.first()
                                {
                                    selected = match node_id {
                                        0 => SettingsCategory::General,
                                        1 => SettingsCategory::UI,
                                        2 => SettingsCategory::Cache,
                                        3 => SettingsCategory::Gizmo,
                                        4 => SettingsCategory::Compositing,
                                        5 => SettingsCategory::WebServer,
                                        _ => selected,
                                    };
                                }
                            }
                        });

                        ui.separator();

                        // Right panel: content for selected category
                        ui.vertical(|ui| {
                            ui.add_space(8.0);

                            match selected {
                                SettingsCategory::General => render_general_settings(ui, settings),
                                SettingsCategory::UI => render_ui_settings(ui, settings),
                                SettingsCategory::Cache => render_cache_settings(ui, settings),
                                SettingsCategory::Gizmo => render_gizmo_settings(ui, project, event_bus),
                                SettingsCategory::Compositing => render_compositing_settings(ui, settings, event_bus),
                                SettingsCategory::WebServer => render_webserver_settings(ui, settings),
                            }
                        });
                    });
                });
        });

    // Save selected category
    settings.selected_settings_category = Some(selected.as_str().to_string());
}
