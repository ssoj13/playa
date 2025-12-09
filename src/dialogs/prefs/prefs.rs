use eframe::egui;
use egui_ltreeview::TreeView;

/// Settings categories
#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsCategory {
    General,
    Input,
    UI,
}

impl SettingsCategory {
    fn as_str(&self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Input => "Input",
            SettingsCategory::UI => "UI",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "General" => Some(SettingsCategory::General),
            "Input" => Some(SettingsCategory::Input),
            "UI" => Some(SettingsCategory::UI),
            _ => None,
        }
    }
}

/// Compositor backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompositorBackend {
    Cpu,
    Gpu,
}

impl Default for CompositorBackend {
    fn default() -> Self {
        CompositorBackend::Cpu // Default to CPU for compatibility
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
    pub timeline_snap_enabled: bool,
    pub timeline_lock_work_area: bool,

    // Workers (applied to App::workers / playback/encoding threads)
    pub workers_override: u32, // 0 = auto, N = override (applies on restart)

    // Cache & Memory
    pub cache_memory_percent: f32,      // 25-95% of available (default 75%)
    pub reserve_system_memory_gb: f32,  // Reserve for system (default 2.0 GB)
    pub cache_strategy: crate::core::global_cache::CacheStrategy, // Caching strategy (LastOnly or All)

    // Compositor backend (CPU or GPU)
    pub compositor_backend: CompositorBackend,

    // Encoding dialog
    pub encode_dialog: crate::dialogs::encode::EncodeDialogSettings,

    // Input / Folder scanning
    pub scan_nested_media: bool,      // Scan subdirs for video files
    pub scan_nested_sequences: bool,  // Scan subdirs for image sequences

    // Internal
    pub selected_settings_category: Option<String>,
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
            font_size: 13.0,
            timeline_snap_enabled: true,
            timeline_lock_work_area: false,
            workers_override: 0,
            cache_memory_percent: 75.0,
            reserve_system_memory_gb: 2.0,
            cache_strategy: crate::core::global_cache::CacheStrategy::All, // Default: cache all frames
            compositor_backend: CompositorBackend::default(),
            encode_dialog: crate::dialogs::encode::EncodeDialogSettings::default(),
            scan_nested_media: true,
            scan_nested_sequences: true,
            selected_settings_category: Some("UI".to_string()),
        }
    }
}

/// Render General settings category
fn render_general_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("Playback");
    ui.add_space(8.0);

    ui.label("Default FPS:");
    ui.add(
        egui::Slider::new(&mut settings.fps_base, 1.0..=120.0)
            .suffix(" fps")
            .step_by(1.0),
    );
    ui.add_space(8.0);

    ui.checkbox(&mut settings.loop_enabled, "Loop playback by default");
}

/// Render Input settings category (folder scanning)
fn render_input_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("Folder Scanning");
    ui.add_space(8.0);

    ui.label("When adding a folder, scan for:");
    ui.add_space(4.0);

    ui.checkbox(
        &mut settings.scan_nested_media,
        "Video files in subdirectories (.mp4, .mov, .avi, etc.)",
    );
    ui.checkbox(
        &mut settings.scan_nested_sequences,
        "Image sequences in subdirectories (.exr, .png, .jpg, etc.)",
    );

    ui.add_space(16.0);
    ui.label("Note: Use 'Add Folder' button in Project panel to scan directories.");
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
    ui.add_space(16.0);

    ui.checkbox(&mut settings.dark_mode, "Dark Mode");
    ui.checkbox(&mut settings.show_tooltips, "Show Tooltips (2s delay on toolbar controls)");

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
        use crate::core::global_cache::CacheStrategy;
        ui.radio_value(&mut settings.cache_strategy, CacheStrategy::All, "All Frames");
        ui.radio_value(&mut settings.cache_strategy, CacheStrategy::LastOnly, "Last Only");
    });
    ui.label("All Frames: Maximum performance, more memory usage.");
    ui.label("Last Only: Minimal memory, only last accessed frame per comp.");

    ui.add_space(16.0);
    ui.heading("Compositing");
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Backend:");
        ui.radio_value(&mut settings.compositor_backend, CompositorBackend::Cpu, "CPU");
        ui.radio_value(&mut settings.compositor_backend, CompositorBackend::Gpu, "GPU");
    });
    ui.label("GPU compositor uses OpenGL for 10-50x faster multi-layer blending.");
    ui.label("Requires OpenGL 3.0+. Falls back to CPU on errors.");
}

/// Render settings window
pub fn render_settings_window(
    ctx: &egui::Context,
    show_settings: &mut bool,
    settings: &mut AppSettings,
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
                                builder.leaf(1, SettingsCategory::Input.as_str());
                                builder.leaf(2, SettingsCategory::UI.as_str());
                            });

                            // Handle selection from actions
                            for action in actions {
                                if let egui_ltreeview::Action::SetSelected(node_ids) = action
                                    && let Some(&node_id) = node_ids.first()
                                {
                                    selected = match node_id {
                                        0 => SettingsCategory::General,
                                        1 => SettingsCategory::Input,
                                        2 => SettingsCategory::UI,
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
                                SettingsCategory::Input => render_input_settings(ui, settings),
                                SettingsCategory::UI => render_ui_settings(ui, settings),
                            }
                        });
                    });
                });
        });

    // Save selected category
    settings.selected_settings_category = Some(selected.as_str().to_string());
}
