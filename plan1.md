# Playa Bug Hunt & Enhancement Report

**Date:** 2025-12-08
**Status:** Awaiting Approval

---

## Executive Summary

This report covers analysis and solutions for 6 tasks from `task.md`. Each task has been analyzed with production-grade recommendations.

---

## Task 1: Per-Window Help System (F1)

### Current State
- **Location:** `src/ui.rs::help_text()` (lines 24-74)
- **Rendering:** `src/widgets/viewport/viewport_ui.rs::render_help_overlay()` (lines 249-257)
- **Control:** Single `show_help: bool` in `PlayaApp` (main.rs)
- **Display:** Overlay on viewport only, shows ALL hotkeys in one block

### Problem
All windows share one help text. User cannot get context-specific help per window.

### Solution Options

#### Option A: Per-Widget Help Traits (Recommended)
Create a trait that each widget implements:

```rust
// src/help.rs (new file)
pub trait HelpProvider {
    fn help_title(&self) -> &'static str;
    fn help_items(&self) -> &'static [(&'static str, &'static str)]; // (key, description)
}
```

Each widget module provides its own help:
- `viewport/help.rs` - viewport-specific keys (zoom, pan, scrub)
- `timeline/help.rs` - timeline-specific keys (trim, align, select)
- `project/help.rs` - project panel keys (navigation, selection)
- `ae/help.rs` - attribute editor keys

**Pros:** Modular, each widget owns its help, easy to maintain
**Cons:** Need to refactor existing help into pieces

#### Option B: Help Registry Pattern
Central registry where widgets register their help on init:

```rust
// src/help.rs
pub struct HelpRegistry {
    sections: HashMap<&'static str, Vec<HelpEntry>>,
}

impl HelpRegistry {
    pub fn register(&mut self, section: &'static str, entries: &[HelpEntry]);
    pub fn get_section(&self, section: &str) -> Option<&[HelpEntry]>;
    pub fn all_sections(&self) -> impl Iterator<Item = (&str, &[HelpEntry])>;
}
```

**Pros:** Dynamic registration, central access for F11 overview
**Cons:** More complex initialization

#### Option C: Static Help Map (Simplest)
Just split current `help_text()` into sections:

```rust
// src/help.rs
pub fn viewport_help() -> &'static str { ... }
pub fn timeline_help() -> &'static str { ... }
pub fn project_help() -> &'static str { ... }
pub fn ae_help() -> &'static str { ... }
pub fn global_help() -> &'static str { ... } // ESC, F-keys, Ctrl+S, etc.
```

Each window renders its section + global section.

**Pros:** Minimal changes, fast to implement
**Cons:** Duplication if global keys shown in each window

### Recommendation
**Option A** - Trait-based. Clean architecture, each widget is self-documenting. Global help (F-keys, Ctrl+S) can be a separate trait impl shared by all.

### Implementation Plan
- [ ] Create `src/help.rs` with `HelpProvider` trait
- [ ] Implement trait for each widget (viewport, timeline, project, ae)
- [ ] Add `show_help: bool` to each widget's state struct
- [ ] Add F1 handling in each widget's input handler
- [ ] Render help overlay in each widget's `_ui.rs`
- [ ] Remove global `show_help` from `PlayaApp`

---

## Task 2: Global Help Panel (F11)

### Solution
Create dedicated help dialog that aggregates all help sections.

**Location:** `src/dialogs/help/mod.rs` (new)

```rust
// src/dialogs/help/mod.rs
pub fn render_help_window(ctx: &egui::Context, show: &mut bool) {
    egui::Window::new("Keyboard Shortcuts")
        .id(egui::Id::new("help_window"))
        .open(show)
        .default_size([600.0, 700.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Collapsible sections for each widget
                ui.collapsing("Global", |ui| { ... });
                ui.collapsing("Viewport", |ui| { ... });
                ui.collapsing("Timeline", |ui| { ... });
                ui.collapsing("Project Panel", |ui| { ... });
                ui.collapsing("Attribute Editor", |ui| { ... });
            });
        });
}
```

### Implementation Plan
- [ ] Create `src/dialogs/help/mod.rs`
- [ ] Add `show_global_help: bool` to `PlayaApp`
- [ ] Add F11 keybinding in `main.rs` hotkey handler
- [ ] Collect all help sections from `HelpProvider` implementations
- [ ] Render as collapsible sections with search/filter option

---

## Task 3: Layer Storage Structure

### Current State
```rust
// src/entities/comp.rs:108
pub children: Vec<(Uuid, Attrs)>,
```

Layers are stored as simple `Vec<(Uuid, Attrs)>`:
- `Uuid` - layer's unique ID
- `Attrs` - key-value attribute storage (HashMap-based)

### Analysis
Current approach is **OK but not optimal**:
- **Pros:** Simple, flexible, serde-compatible
- **Cons:** No type safety, attrs can have invalid combinations, no per-layer state tracking

### Solution Options

#### Option A: Dedicated Layer Struct (Recommended)
```rust
// src/entities/layer.rs (new file)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub uuid: Uuid,
    pub source_uuid: Uuid,  // Reference to source Comp in media pool

    // Timeline positioning
    pub in_point: i32,
    pub out_point: i32,
    pub trim_in: i32,
    pub trim_out: i32,

    // Transform
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub pivot: [f32; 3],

    // Compositing
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub visible: bool,
    pub solo: bool,
    pub mute: bool,

    // Playback
    pub speed: f32,

    // Runtime (not serialized)
    #[serde(skip)]
    pub cache_valid: bool,
    #[serde(skip)]
    pub last_hash: u64,
}

impl Layer {
    pub fn invalidate(&mut self) { self.cache_valid = false; }
    pub fn is_visible_at(&self, frame: i32) -> bool { ... }
    pub fn local_frame(&self, comp_frame: i32) -> Option<i32> { ... }
}
```

Then Comp becomes:
```rust
pub struct Comp {
    pub attrs: Attrs,  // Comp-level attrs only
    pub layers: Vec<Layer>,  // Typed layers
    // ...
}
```

**Pros:**
- Type safety (no more `attrs.get_i32("in")`)
- Per-layer cache invalidation
- Clear documentation of layer properties
- Better IDE support

**Cons:**
- Migration of existing projects needed
- More boilerplate than HashMap

#### Option B: NewType Wrapper
```rust
// src/entities/layer.rs
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer(pub Attrs);

impl Layer {
    // Typed getters/setters
    pub fn in_point(&self) -> i32 { self.0.get_i32(A_IN).unwrap_or(0) }
    pub fn set_in_point(&mut self, v: i32) { self.0.set(A_IN, AttrValue::Int(v)); }
    // ...
}
```

**Pros:** Minimal migration, keeps Attrs flexibility
**Cons:** Still runtime key lookups, just wrapped

### Recommendation
**Option A** - Dedicated struct. The typed approach is better for:
1. IDE autocomplete/docs
2. Compile-time checking
3. Per-layer state (cache_valid, last_hash)
4. Future features (keyframes, expressions)

### Migration Strategy
```rust
// Auto-convert on deserialize
impl<'de> Deserialize<'de> for Comp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
        // Try new format first, fall back to legacy Vec<(Uuid, Attrs)>
    }
}
```

---

## Task 4: Multiple Layers per Track

### Current State
```rust
pub children: Vec<(Uuid, Attrs)>
```
Single layer per track position.

### Problem
Timeline is flat - one layer = one track. No way to have multiple clips on same track like in video editors (Premiere, DaVinci).

### Solution Options

#### Option A: Track-based Structure
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub uuid: Uuid,
    pub name: String,
    pub layers: Vec<Layer>,  // Multiple layers on this track
    pub locked: bool,
    pub visible: bool,
    pub solo: bool,
    pub mute: bool,
    pub color: [u8; 3],  // Track color for UI
}

pub struct Comp {
    pub tracks: Vec<Track>,  // Instead of children
    // ...
}
```

**Pros:**
- Industry-standard model (NLE-style)
- Layers on same track auto-exclude (no overlap)
- Track-level controls (lock, mute all layers)

**Cons:**
- Significant refactor
- Timeline UI needs major changes
- Changes event model

#### Option B: Layer Groups (Simpler)
Keep flat structure, add group UUID:
```rust
pub struct Layer {
    pub track_id: Option<Uuid>,  // None = own track, Some = grouped
    // ...
}
```

Layers with same `track_id` are on same track, rendered top-to-bottom.

**Pros:** Minimal changes, backwards compatible
**Cons:** Not as clean as proper tracks

#### Option C: Nested Vec (Your Suggestion)
```rust
pub children: Vec<Vec<Layer>>  // outer = tracks, inner = layers on track
```

**Pros:** Direct, simple
**Cons:** Loses track metadata, order matters

### Recommendation
**Option A** - Track-based. This is the professional approach:
- Premiere Pro, After Effects, DaVinci use tracks
- Clean separation of concerns
- Future-proof for features like track effects, track markers

### Data Flow Diagram
```
Comp
 |
 +-- Track 0 (Video)
 |    |-- Layer: clip1.mov [0-100]
 |    |-- Layer: clip2.mov [100-200]
 |    +-- Layer: title.png [50-150] (overlaps both)
 |
 +-- Track 1 (Overlay)
 |    +-- Layer: logo.png [0-200]
 |
 +-- Track 2 (Audio) [future]
      +-- Layer: music.wav [0-200]
```

---

## Task 5: Directory Input & Sequence Scanning

### Current State
- App accepts files only (drag-drop, CLI)
- No preferences for directory handling
- `scanseq-rs` crate available at `C:\projects\projects.rust\scanseq-rs`

### Solution

#### 5.1 Prefs Extension
Add to `SettingsCategory`:
```rust
enum SettingsCategory {
    General,
    UI,
    Input,  // NEW
}
```

Add to `AppSettings`:
```rust
pub struct AppSettings {
    // ... existing ...

    // Input settings
    pub scan_nested_media: bool,      // Scan subdirs for video files
    pub scan_nested_sequences: bool,  // Scan subdirs for image sequences
}
```

UI:
```rust
fn render_input_settings(ui: &mut egui::Ui, settings: &mut AppSettings) {
    ui.heading("Directory Handling");
    ui.add_space(8.0);

    ui.checkbox(&mut settings.scan_nested_media,
        "Scan subdirectories for video files (.mp4, .mov, .avi, etc.)");

    ui.checkbox(&mut settings.scan_nested_sequences,
        "Scan subdirectories for image sequences (.exr, .png, .jpg, etc.)");

    ui.add_space(8.0);
    ui.label("When a directory is dropped or opened, these options control\nwhether to search inside subdirectories for media.");
}
```

#### 5.2 Scanseq Integration
Copy from `scanseq-rs`:
- `core/mod.rs` -> `src/core/scanseq/mod.rs`
- `core/scan.rs` -> `src/core/scanseq/scan.rs`
- `core/file/mod.rs` -> `src/core/scanseq/file.rs`
- `core/seq/mod.rs` -> `src/core/scanseq/seq.rs`

Or simpler - add as dependency:
```toml
# Cargo.toml
scanseq = { path = "../scanseq-rs" }
```

#### 5.3 Input Handler
```rust
// src/input.rs or extend existing drop handler
pub fn handle_input(path: &Path, settings: &AppSettings) -> Vec<MediaItem> {
    if path.is_file() {
        return vec![MediaItem::File(path.to_path_buf())];
    }

    // Directory handling
    let mut items = Vec::new();

    if settings.scan_nested_sequences {
        let scanner = scanseq::Scanner::new(
            vec![path.to_string_lossy().to_string()],
            true,  // recursive
            Some("*.exr;*.png;*.jpg;*.tif"),  // image patterns
            2      // min sequence length
        );
        for seq in scanner.iter() {
            items.push(MediaItem::Sequence(seq.into()));
        }
    }

    if settings.scan_nested_media {
        // Walk for video files
        for entry in walkdir::WalkDir::new(path) {
            if let Ok(e) = entry {
                if is_video_extension(e.path()) {
                    items.push(MediaItem::File(e.path().to_path_buf()));
                }
            }
        }
    }

    items
}
```

### Implementation Plan
- [ ] Add `Input` category to prefs
- [ ] Add `scan_nested_*` fields to `AppSettings`
- [ ] Integrate scanseq as dependency (not copy)
- [ ] Create `src/input.rs` for unified input handling
- [ ] Update drag-drop handler in `main.rs`
- [ ] Update CLI handler in `cli.rs`

---

## Task 6: Node Editor Research

### Findings

| Crate | Stars | egui Version | Status | Recommendation |
|-------|-------|--------------|--------|----------------|
| **egui-snarl** | 485 | 0.33 | Active (3 days ago) | **Best Choice** |
| egui-graph-edit | 25 | 0.32 | Active | Good Alternative |
| egui_node_graph2 | - | 0.29 | Maintained | Legacy compat |
| egui_node_graph | - | 0.19 | Archived | Don't use |
| egui_nodes | 120 | 0.16 | Inactive | Don't use |

### Recommendation: egui-snarl

**Why:**
1. **Matches project's egui version** - Playa uses eframe 0.33, egui-snarl supports 0.33
2. **Most active development** - 485 stars, updated 3 days ago
3. **Serde support** - Built-in serialization for save/load
4. **Type-safe** - Generic over node type
5. **Good examples** - Demo included

### Integration Concept

```rust
// src/dialogs/node_editor/mod.rs
use egui_snarl::{Snarl, SnarlViewer};

#[derive(Clone, Serialize, Deserialize)]
enum NodeType {
    Comp(Uuid),           // Reference to existing comp
    Input { path: String },
    Output,
    Transform,
    ColorCorrect,
    // ...
}

struct CompNodeViewer;

impl SnarlViewer<NodeType> for CompNodeViewer {
    fn title(&self, node: &NodeType) -> String {
        match node {
            NodeType::Comp(uuid) => format!("Comp: {}", uuid),
            NodeType::Input { path } => format!("Input: {}", path),
            NodeType::Output => "Output".to_string(),
            // ...
        }
    }

    fn inputs(&self, node: &NodeType) -> usize {
        match node {
            NodeType::Comp(_) => 4,  // 4 input slots
            NodeType::Transform => 1,
            // ...
        }
    }
    // ...
}
```

### Node Network Data Model
```rust
// Each Comp can be represented as a node
// Connections define layer hierarchy

CompA (root)
  |
  +--[input 0]-- CompB (layer 0)
  |               |
  |               +--[input 0]-- FileComp (sequence.exr)
  |
  +--[input 1]-- CompC (layer 1)
                  |
                  +--[input 0]-- VideoComp (clip.mov)
```

### Implementation Phases
1. **Phase 1:** Add egui-snarl dependency, create basic node editor dialog (F9?)
2. **Phase 2:** Visualize existing comp hierarchy as nodes
3. **Phase 3:** Allow creating connections (replaces drag-drop in project panel)
4. **Phase 4:** Add processing nodes (transform, color, etc.)

---

## Summary of Recommendations

| Task | Solution | Effort | Priority |
|------|----------|--------|----------|
| 1. Per-window Help | HelpProvider trait | Medium | High |
| 2. Global Help F11 | New dialog in dialogs/help/ | Low | Medium |
| 3. Layer Structure | Dedicated Layer struct | High | High |
| 4. Multi-layer Tracks | Track-based model | High | Low |
| 5. Directory Input | Prefs + scanseq integration | Medium | High |
| 6. Node Editor | egui-snarl | High | Low (future) |

---

## Dead Code Found

1. **Legacy aliases** (comp.rs:160-161):
   ```rust
   attrs.set("transparency", AttrValue::Float(1.0));
   attrs.set("layer_mode", AttrValue::Str("normal".to_string()));
   ```
   These are never read. Safe to remove.

2. **Empty General settings** (prefs.rs:108-112):
   ```rust
   fn render_general_settings(ui: &mut egui::Ui, _settings: &mut AppSettings) {
       ui.label("(No settings yet)");
   }
   ```
   Placeholder - use for Input settings from Task 5.

3. **Unused _radius parameter** (main.rs:285):
   ```rust
   fn enqueue_frame_loads_around_playhead(&mut self, _radius: usize) {
       // TODO: implement spiraling preload
   }
   ```
   Never implemented. Either implement or remove.

---

## Action Required

Please review this plan and approve or request changes. Once approved, I'll proceed with implementation starting from highest priority items.

**Approval Checklist:**
- [ ] Task 1: HelpProvider trait approach OK?
- [ ] Task 2: F11 global help dialog OK?
- [ ] Task 3: Dedicated Layer struct OK? Migration acceptable?
- [ ] Task 4: Track-based model OK? Or defer?
- [ ] Task 5: scanseq as dependency (not copied files) OK?
- [ ] Task 6: egui-snarl for future node editor OK?

---

*Report generated by Claude Code*
