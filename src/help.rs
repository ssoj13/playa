//! Per-window help system.
//!
//! Provides context-sensitive help for each widget via static const arrays.
//! Global help (F-keys, Ctrl+S, etc.) is separate and shown in all contexts.

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

/// Global hotkeys shared across all windows
pub const GLOBAL_HELP: &[HelpEntry] = &[
    HelpEntry::new("F1", "Toggle context help"),
    HelpEntry::new("F2", "Toggle Project panel"),
    HelpEntry::new("F3", "Toggle Attributes panel"),
    HelpEntry::new("F4", "Toggle Encoder dialog"),
    HelpEntry::new("F12", "Toggle Preferences"),
    HelpEntry::new("ESC", "Exit Fullscreen / Quit"),
    HelpEntry::new("Z", "Toggle Fullscreen"),
    HelpEntry::new("U", "Previous Comp"),
    HelpEntry::new("Ctrl+S", "Save Project"),
    HelpEntry::new("Ctrl+O", "Open Project"),
    HelpEntry::new("Ctrl+Alt+/", "Clear Frame Cache"),
];

/// Viewport-specific help
pub const VIEWPORT_HELP: &[HelpEntry] = &[
    HelpEntry::new("Q", "Select Tool (scrub)"),
    HelpEntry::new("W", "Move Tool"),
    HelpEntry::new("E", "Rotate Tool"),
    HelpEntry::new("R", "Scale Tool"),
    HelpEntry::new("A / H", "100% Zoom"),
    HelpEntry::new("F", "Fit to View"),
    HelpEntry::new("Mouse Wheel", "Zoom"),
    HelpEntry::new("Middle Drag", "Pan"),
    HelpEntry::new("Left Click", "Scrub"),
    HelpEntry::new("Backspace", "Toggle Frame Numbers"),
];

/// Playback controls (shown in Viewport)
pub const PLAYBACK_HELP: &[HelpEntry] = &[
    HelpEntry::new("Space / Insert", "Play/Pause"),
    HelpEntry::new("K / /", "Stop"),
    HelpEntry::new("J / ,", "Jog Backward"),
    HelpEntry::new("L / .", "Jog Forward"),
    HelpEntry::new("Left/Right", "Step 1 frame"),
    HelpEntry::new("Shift+Arrows", "Step 25 frames"),
    HelpEntry::new("Ctrl+Arrows", "Jump to Start/End"),
    HelpEntry::new("1 / Home", "Jump to Start"),
    HelpEntry::new("2 / End", "Jump to End"),
    HelpEntry::new("; / '", "Prev/Next Layer Edge"),
    HelpEntry::new("`", "Toggle Loop"),
    HelpEntry::new("B / N", "Set Play Range Start/End"),
    HelpEntry::new("Ctrl+B", "Reset Play Range"),
    HelpEntry::new("- / = / +", "Decrease/Increase FPS"),
];

/// Timeline-specific help
pub const TIMELINE_HELP: &[HelpEntry] = &[
    HelpEntry::new("[", "Align Layer Start to Cursor"),
    HelpEntry::new("]", "Align Layer End to Cursor"),
    HelpEntry::new("Alt+[", "Trim Layer Start to Cursor"),
    HelpEntry::new("Alt+]", "Trim Layer End to Cursor"),
    HelpEntry::new("Delete", "Remove Selected Layer"),
    HelpEntry::new("F", "Fit to Selection"),
    HelpEntry::new("A", "Fit to Work Area (B/N)"),
    HelpEntry::new("- / = / +", "Zoom Timeline In/Out"),
    HelpEntry::new("Mouse Wheel", "Zoom Timeline"),
    HelpEntry::new("Middle Drag", "Pan Timeline"),
    HelpEntry::new("Ctrl+D", "Duplicate Layers"),
    HelpEntry::new("Ctrl+C", "Copy Layers"),
    HelpEntry::new("Ctrl+V", "Paste Layers"),
    HelpEntry::new("Ctrl+A", "Select All Layers"),
    HelpEntry::new("Ctrl+R", "Reset Trims"),
];

/// Project panel help
pub const PROJECT_HELP: &[HelpEntry] = &[
    HelpEntry::new("Double-click", "Open Comp"),
    HelpEntry::new("Drag", "Reorder / Add to Timeline"),
    HelpEntry::new("Delete", "Remove Selected"),
    HelpEntry::new("Enter", "Rename Selected"),
];

/// Attribute Editor help
pub const AE_HELP: &[HelpEntry] = &[
    HelpEntry::new("Enter", "Apply Changes"),
    HelpEntry::new("Tab", "Next Field"),
    HelpEntry::new("Shift+Tab", "Previous Field"),
];

/// Node Editor help
pub const NODE_HELP: &[HelpEntry] = &[
    HelpEntry::new("A", "Fit All Nodes"),
    HelpEntry::new("F", "Fit Selected Nodes"),
    HelpEntry::new("L", "Re-layout Nodes"),
    HelpEntry::new("Delete", "Remove Selected Node"),
    HelpEntry::new("Middle Drag", "Pan"),
    HelpEntry::new("Mouse Wheel", "Zoom"),
];

/// Render help overlay for a widget (two-column layout)
pub fn render_help_overlay(ui: &mut egui::Ui, entries: &[HelpEntry], include_global: bool) {
    let font_id = egui::FontId::proportional(12.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200);
    let key_color = egui::Color32::from_rgb(255, 200, 100);
    let section_color = egui::Color32::GRAY;

    // Helper to render a section
    let render_section = |ui: &mut egui::Ui, title: &str, entries: &[HelpEntry]| {
        ui.label(
            egui::RichText::new(title)
                .font(font_id.clone())
                .color(section_color),
        );
        ui.add_space(4.0);
        for entry in entries {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [110.0, 16.0],
                    egui::Label::new(
                        egui::RichText::new(entry.key)
                            .font(font_id.clone())
                            .color(key_color),
                    ),
                );
                ui.label(
                    egui::RichText::new(entry.desc)
                        .font(font_id.clone())
                        .color(text_color),
                );
            });
        }
    };

    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200))
        .inner_margin(16.0)
        .corner_radius(6.0)
        .show(ui, |ui| {
            // Two-column layout
            ui.horizontal(|ui| {
                // Left column: context-specific + playback
                ui.vertical(|ui| {
                    ui.set_min_width(260.0);
                    
                    if !entries.is_empty() {
                        render_section(ui, "Context", entries);
                        ui.add_space(8.0);
                    }
                    
                    render_section(ui, "Playback", PLAYBACK_HELP);
                });

                ui.add_space(24.0);

                // Right column: global
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    if include_global && !GLOBAL_HELP.is_empty() {
                        render_section(ui, "Global", GLOBAL_HELP);
                    }
                });
            });
        });
}
