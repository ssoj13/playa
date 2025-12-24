//! Per-window help system with two-column layout.
//!
//! Organized by panels:
//! - Left column: Viewport/Tools, Playback, Navigation
//! - Right column: Timeline, Project, Global
//!
//! Each section is a static const array for zero-cost access.

use eframe::egui;

/// Single help entry (key binding + description)
#[derive(Clone, Debug)]
pub struct HelpEntry {
    pub key: &'static str,
    pub desc: &'static str,
}

impl HelpEntry {
    pub const fn new(key: &'static str, desc: &'static str) -> Self {
        Self { key, desc }
    }
}

// =============================================================================
// LEFT COLUMN: Viewport, Playback, Navigation
// =============================================================================

/// Viewport tools and view controls
pub const VIEWPORT_HELP: &[HelpEntry] = &[
    HelpEntry::new("Q", "Select Tool"),
    HelpEntry::new("W", "Move Tool"),
    HelpEntry::new("E", "Rotate Tool"),
    HelpEntry::new("R", "Scale Tool"),
    HelpEntry::new("A / H", "100% Zoom"),
    HelpEntry::new("F", "Fit to View"),
    HelpEntry::new("Wheel", "Zoom"),
    HelpEntry::new("MMB Drag", "Pan"),
    HelpEntry::new("LMB", "Scrub / Pick"),
    HelpEntry::new("Backspace", "Frame Numbers"),
];

/// Playback controls (JKL style)
pub const PLAYBACK_HELP: &[HelpEntry] = &[
    HelpEntry::new("Space", "Play/Pause"),
    HelpEntry::new("K / /", "Stop"),
    HelpEntry::new("J / ,", "Jog Back"),
    HelpEntry::new("L / .", "Jog Forward"),
    HelpEntry::new("`", "Toggle Loop"),
    HelpEntry::new("- / +", "FPS Down/Up"),
];

/// Frame navigation
pub const NAVIGATION_HELP: &[HelpEntry] = &[
    HelpEntry::new("Left/Right", "Step 1 frame"),
    HelpEntry::new("Shift+Arrows", "Step 25 frames"),
    HelpEntry::new("Ctrl+Arrows", "Start/End"),
    HelpEntry::new("1 / Home", "Jump Start"),
    HelpEntry::new("2 / End", "Jump End"),
    HelpEntry::new("; / '", "Prev/Next Edge"),
    HelpEntry::new("B / N", "Set Range"),
    HelpEntry::new("Ctrl+B", "Reset Range"),
];

// =============================================================================
// RIGHT COLUMN: Timeline, Project, Global
// =============================================================================

/// Timeline layer operations
pub const TIMELINE_HELP: &[HelpEntry] = &[
    HelpEntry::new("[ / ]", "Align to Cursor"),
    HelpEntry::new("Alt+[ / ]", "Trim to Cursor"),
    HelpEntry::new("Delete", "Remove Layer"),
    HelpEntry::new("Ctrl+D", "Duplicate"),
    HelpEntry::new("Ctrl+C/V", "Copy/Paste"),
    HelpEntry::new("Ctrl+A", "Select All"),
    HelpEntry::new("Ctrl+R", "Reset Trims"),
    HelpEntry::new("F / A", "Fit / Work Area"),
    HelpEntry::new("Wheel", "Zoom"),
    HelpEntry::new("MMB Drag", "Pan"),
];

/// Project panel
pub const PROJECT_HELP: &[HelpEntry] = &[
    HelpEntry::new("Dbl-click", "Open Comp"),
    HelpEntry::new("Drag", "Reorder / Add"),
    HelpEntry::new("Delete", "Remove"),
    HelpEntry::new("Enter", "Rename"),
];

/// Global hotkeys
pub const GLOBAL_HELP: &[HelpEntry] = &[
    HelpEntry::new("F1", "Help"),
    HelpEntry::new("F2", "Project Panel"),
    HelpEntry::new("F3", "Attributes Panel"),
    HelpEntry::new("F4", "Encoder"),
    HelpEntry::new("F12", "Preferences"),
    HelpEntry::new("Z", "Fullscreen"),
    HelpEntry::new("ESC", "Exit / Quit"),
    HelpEntry::new("Ctrl+S", "Save"),
    HelpEntry::new("Ctrl+O", "Open"),
    HelpEntry::new("Ctrl+Alt+/", "Clear Cache"),
];

/// Node Editor help
pub const NODE_HELP: &[HelpEntry] = &[
    HelpEntry::new("A", "Fit All"),
    HelpEntry::new("F", "Fit Selected"),
    HelpEntry::new("L", "Re-layout"),
    HelpEntry::new("Delete", "Remove Node"),
    HelpEntry::new("MMB Drag", "Pan"),
    HelpEntry::new("Wheel", "Zoom"),
];

/// Attribute Editor help
pub const AE_HELP: &[HelpEntry] = &[
    HelpEntry::new("Enter", "Apply"),
    HelpEntry::new("Tab", "Next Field"),
    HelpEntry::new("Shift+Tab", "Prev Field"),
];

// =============================================================================
// Rendering
// =============================================================================

/// Render main help overlay (two-column layout, positioned at rect center)
pub fn render_main_help(ui: &mut egui::Ui, rect: egui::Rect) {
    let font = egui::FontId::proportional(12.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200);
    let key_color = egui::Color32::from_rgb(255, 200, 100);
    let section_color = egui::Color32::GRAY;

    // Render one section
    let render_section = |ui: &mut egui::Ui, title: &str, entries: &[HelpEntry]| {
        ui.label(
            egui::RichText::new(title)
                .font(font.clone())
                .color(section_color),
        );
        ui.add_space(2.0);
        for e in entries {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [90.0, 14.0],
                    egui::Label::new(
                        egui::RichText::new(e.key)
                            .font(font.clone())
                            .color(key_color),
                    ),
                );
                ui.label(
                    egui::RichText::new(e.desc)
                        .font(font.clone())
                        .color(text_color),
                );
            });
        }
        ui.add_space(6.0);
    };

    // Overlay frame
    let overlay_size = egui::vec2(520.0, 340.0);
    let overlay_pos = rect.center() - overlay_size / 2.0;
    let overlay_rect = egui::Rect::from_min_size(overlay_pos, overlay_size);

    // Background
    ui.painter().rect_filled(
        overlay_rect,
        6.0,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220),
    );

    // Content area
    let content_rect = overlay_rect.shrink(16.0);
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));

    child_ui.horizontal(|ui| {
        // Left column: Viewport, Playback, Navigation
        ui.vertical(|ui| {
            ui.set_width(230.0);
            render_section(ui, "VIEWPORT", VIEWPORT_HELP);
            render_section(ui, "PLAYBACK", PLAYBACK_HELP);
            render_section(ui, "NAVIGATION", NAVIGATION_HELP);
        });

        ui.add_space(16.0);

        // Right column: Timeline, Project, Global
        ui.vertical(|ui| {
            ui.set_width(230.0);
            render_section(ui, "TIMELINE", TIMELINE_HELP);
            render_section(ui, "PROJECT", PROJECT_HELP);
            render_section(ui, "GLOBAL", GLOBAL_HELP);
        });
    });
}

/// Render help overlay for a specific widget (context-sensitive)
pub fn render_help_overlay(ui: &mut egui::Ui, entries: &[HelpEntry], include_global: bool) {
    let font = egui::FontId::proportional(12.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200);
    let key_color = egui::Color32::from_rgb(255, 200, 100);
    let section_color = egui::Color32::GRAY;

    let render_section = |ui: &mut egui::Ui, title: &str, entries: &[HelpEntry]| {
        ui.label(
            egui::RichText::new(title)
                .font(font.clone())
                .color(section_color),
        );
        ui.add_space(2.0);
        for e in entries {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [90.0, 14.0],
                    egui::Label::new(
                        egui::RichText::new(e.key)
                            .font(font.clone())
                            .color(key_color),
                    ),
                );
                ui.label(
                    egui::RichText::new(e.desc)
                        .font(font.clone())
                        .color(text_color),
                );
            });
        }
    };

    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200))
        .inner_margin(12.0)
        .corner_radius(6.0)
        .show(ui, |ui| {
            if !entries.is_empty() {
                render_section(ui, "CONTEXT", entries);
            }
            if include_global {
                ui.add_space(8.0);
                render_section(ui, "GLOBAL", GLOBAL_HELP);
            }
        });
}
