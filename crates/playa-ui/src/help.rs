//! Per-window help system with two-column layout.
//!
//! Organized by panels:
//! - Left column: Viewport/Tools, Playback, Navigation
//! - Right column: Timeline, Project, Global
//!
//! Each section is a static const array for zero-cost access.

use eframe::egui;

// `HelpEntry` and the per-section row renderer (`help_section`) now live in the
// reusable `egui-help-overlay` crate (extracted from playa). This module keeps
// only the app-specific key tables and playa's overlay layout, delegating the
// actual row rendering to the crate so there is a single source of truth.
pub use egui_help_overlay::HelpEntry;

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

/// Render main help overlay (two-column layout, top-left corner). Section rows
/// are drawn by `egui_help_overlay::help_section`; this fn owns only the playa
/// layout (positioned, background-less, fixed two-column split).
pub fn render_main_help(ui: &mut egui::Ui, rect: egui::Rect) {
    use egui_help_overlay::help_section;

    // Top-left corner, no background overlay
    let overlay_size = egui::vec2(520.0, 340.0);
    let overlay_pos = rect.min + egui::vec2(16.0, 16.0);
    let content_rect = egui::Rect::from_min_size(overlay_pos, overlay_size);

    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));

    child_ui.horizontal(|ui| {
        // Left column: Viewport, Playback, Navigation
        ui.vertical(|ui| {
            ui.set_width(230.0);
            help_section(ui, "VIEWPORT", VIEWPORT_HELP);
            help_section(ui, "PLAYBACK", PLAYBACK_HELP);
            help_section(ui, "NAVIGATION", NAVIGATION_HELP);
        });

        ui.add_space(16.0);

        // Right column: Timeline, Project, Global
        ui.vertical(|ui| {
            ui.set_width(230.0);
            help_section(ui, "TIMELINE", TIMELINE_HELP);
            help_section(ui, "PROJECT", PROJECT_HELP);
            help_section(ui, "GLOBAL", GLOBAL_HELP);
        });
    });
}

/// Render help overlay for a specific widget (context-sensitive). Rows are drawn
/// by `egui_help_overlay::help_section`; this fn owns the translucent card and
/// the optional context/global split.
pub fn render_help_overlay(ui: &mut egui::Ui, entries: &[HelpEntry], include_global: bool) {
    use egui_help_overlay::help_section;

    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200))
        .inner_margin(12.0)
        .corner_radius(6.0)
        .show(ui, |ui| {
            if !entries.is_empty() {
                help_section(ui, "CONTEXT", entries);
            }
            if include_global {
                ui.add_space(8.0);
                help_section(ui, "GLOBAL", GLOBAL_HELP);
            }
        });
}
