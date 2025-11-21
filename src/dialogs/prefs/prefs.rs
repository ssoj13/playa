use eframe::egui;
use egui_ltreeview::TreeView;

/// Settings categories
#[derive(Debug, Clone, Copy, PartialEq)]
enum SettingsCategory {
    General,
    UI,
}

impl SettingsCategory {
    fn as_str(&self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::UI => "UI",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "General" => Some(SettingsCategory::General),
            "UI" => Some(SettingsCategory::UI),
            _ => None,
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
    pub show_frame_numbers: bool, // Show frame numbers on timeslider
    pub dark_mode: bool,
    pub font_size: f32,
    pub timeline_snap_enabled: bool,
    pub timeline_lock_work_area: bool,

    // Workers (applied to App::workers / playback/encoding threads)
    pub workers_override: u32, // 0 = auto, N = override (applies on restart)

    // Encoding dialog
    pub encode_dialog: crate::dialogs::encode::EncodeDialogSettings,

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
            show_frame_numbers: true,
            dark_mode: true,
            font_size: 13.0,
            timeline_snap_enabled: true,
            timeline_lock_work_area: false,
            workers_override: 0,
            encode_dialog: crate::dialogs::encode::EncodeDialogSettings::default(),
            selected_settings_category: Some("UI".to_string()),
        }
    }
}

/// Render General settings category
fn render_general_settings(ui: &mut egui::Ui, _settings: &mut AppSettings) {
    ui.label("(No settings yet)");
    ui.add_space(8.0);
    ui.label("General settings will be added here in the future.");
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
                                builder.leaf(1, SettingsCategory::UI.as_str());
                            });

                            // Handle selection from actions
                            for action in actions {
                                if let egui_ltreeview::Action::SetSelected(node_ids) = action
                                    && let Some(&node_id) = node_ids.first()
                                {
                                    selected = match node_id {
                                        0 => SettingsCategory::General,
                                        1 => SettingsCategory::UI,
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
                            }
                        });
                    });
                });
        });

    // Save selected category
    settings.selected_settings_category = Some(selected.as_str().to_string());
}
