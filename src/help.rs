//! Per-window help system.
//!
//! Each widget implements `HelpProvider` trait to provide context-sensitive help.
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

/// Trait for widgets that provide context-sensitive help
pub trait HelpProvider {
    /// Section title (e.g., "Viewport", "Timeline")
    fn help_title(&self) -> &'static str;

    /// Help entries for this widget
    fn help_entries(&self) -> &'static [HelpEntry];
}

/// Global hotkeys shared across all windows
pub const GLOBAL_HELP: &[HelpEntry] = &[
    HelpEntry::new("F1", "Toggle context help"),
    HelpEntry::new("F2", "Toggle Project panel"),
    HelpEntry::new("F3", "Toggle Attributes panel"),
    HelpEntry::new("F4", "Toggle Encoder dialog"),
    HelpEntry::new("F12", "Toggle Preferences"),
    HelpEntry::new("ESC", "Exit Fullscreen / Quit"),
    HelpEntry::new("Ctrl+S", "Save Project"),
    HelpEntry::new("Ctrl+O", "Open Project"),
    HelpEntry::new("Z", "Toggle Fullscreen"),
    HelpEntry::new("Ctrl+R", "Reset Settings"),
];

/// Viewport-specific help
pub const VIEWPORT_HELP: &[HelpEntry] = &[
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
    HelpEntry::new("F", "Fit Timeline"),
    HelpEntry::new("A", "Reset Timeline Zoom"),
    HelpEntry::new("Mouse Wheel", "Zoom Timeline"),
    HelpEntry::new("Middle Drag", "Pan Timeline"),
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

/// Render help overlay for a widget
pub fn render_help_overlay(ui: &mut egui::Ui, entries: &[HelpEntry], include_global: bool) {
    let font_id = egui::FontId::proportional(13.0);
    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200);
    let key_color = egui::Color32::from_rgb(255, 200, 100);

    // Calculate max key width for alignment (estimate based on char count)
    let max_key_len = entries
        .iter()
        .chain(if include_global { GLOBAL_HELP.iter() } else { [].iter() })
        .map(|e| e.key.len())
        .max()
        .unwrap_or(10);
    let max_key_width = (max_key_len as f32) * 8.0 + 20.0;

    let render_entries = |ui: &mut egui::Ui, entries: &[HelpEntry]| {
        for entry in entries {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [max_key_width, 18.0],
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
        .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180))
        .inner_margin(12.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            render_entries(ui, entries);

            if include_global && !GLOBAL_HELP.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Global")
                        .font(font_id.clone())
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(4.0);
                render_entries(ui, GLOBAL_HELP);
            }
        });
}

/// Combine multiple help sections into one slice (for F11 global view)
pub fn all_help_sections() -> Vec<(&'static str, &'static [HelpEntry])> {
    vec![
        ("Global", GLOBAL_HELP),
        ("Viewport", VIEWPORT_HELP),
        ("Playback", PLAYBACK_HELP),
        ("Timeline", TIMELINE_HELP),
        ("Project", PROJECT_HELP),
        ("Attributes", AE_HELP),
    ]
}
