# Round 5 Fixes Verification Report

Generated: 2026-03-18  
Method: Static code read (no build)

---

## 1. Encode helpers dedup

**File:** `src/dialogs/encode/encode.rs`

### strip_alpha<T: Copy>(rgba: &[T]) -> Vec<T>
- **EXISTS:** Line 2064. Generic over `T: Copy`. Uses `chunks_exact(4)` internally (line 2066).
- **CORRECT** signature matches spec exactly.

### f16_to_f32_buf(data: &[half::f16]) -> Vec<f32>
- **EXISTS:** Line 2075. One-liner: `data.iter().map(|v| v.to_f32()).collect()`.
- **CORRECT.**

### pixel_buf_to_rgba8(buffer: &PixelBuffer) -> Vec<u8>
- **EXISTS:** Line 2080. Handles U8/F16/F32 variants with clamp(0..1)*255.
- **CORRECT.**

### Helpers are CALLED from write_* functions:
| Function | Calls strip_alpha | Calls f16_to_f32_buf | Calls pixel_buf_to_rgba8 |
|---|---|---|---|
| write_exr_frame (non-openexr, L2091) | L2117, L2136, L2156 | L2125 | -- (works with f32 natively) |
| write_exr_frame (openexr, L2171) | -- (delegates to write_exr_f32_data) | L2204 | -- |
| write_png_frame (L2260) | L2297, L2316 | -- | L2290 |
| write_jpeg_frame (L2327) | L2348 | -- | -- (requires U8 only) |
| write_tiff_frame (L2362) | L2389, L2414 | -- | L2378 |
| write_tga_frame (L2428) | L2457 | -- | -- (requires U8 only) |

### chunks_exact(4) occurrences:
Only 2 matches in the entire file:
1. **Line 2066** -- inside `strip_alpha` helper. CORRECT.
2. **Line 2232** -- inside `write_exr_f32_data` (openexr helper converting interleaved f32 to Rgba structs). This is a **different** pattern (struct conversion, not alpha stripping) so it is NOT a duplicate of strip_alpha.

**VERDICT: PASS.** No inline duplicates of the deduped patterns remain in write_* functions.

---

## 2. ARCH-01: AppEventContext

### AppEventContext struct
**File:** `src/main_events.rs`, line 202.

```rust
pub struct AppEventContext<'a> {
    pub player: &'a mut Player,
    pub project: &'a mut Project,
    pub timeline_state: &'a mut TimelineState,
    pub node_editor_state: &'a mut NodeEditorState,
    pub viewport_state: &'a mut ViewportState,
    pub settings: &'a mut AppSettings,
    pub show_help: &'a mut bool,
    pub show_playlist: &'a mut bool,
    pub show_settings: &'a mut bool,
    pub show_encode_dialog: &'a mut bool,
    pub show_attributes_editor: &'a mut bool,
    pub encode_dialog: &'a mut Option<EncodeDialog>,
    pub is_fullscreen: &'a mut bool,
    pub fullscreen_dirty: &'a mut bool,
    pub reset_settings_pending: &'a mut bool,
}
```
- **15 `&'a mut` fields.** CORRECT.

### handle_app_event signature
**Line 288:**
```rust
pub fn handle_app_event(
    event: &BoxedEvent,
    ctx: &mut AppEventContext<'_>,
) -> Option<EventResult> {
```
- Takes `(event, ctx)` -- NOT 16 separate params. CORRECT.

### Destructuring at top of body
**Lines 292-308:**
```rust
let AppEventContext {
    player, project, timeline_state, node_editor_state,
    viewport_state, settings, show_help, show_playlist,
    show_settings, show_encode_dialog, show_attributes_editor,
    encode_dialog, is_fullscreen, fullscreen_dirty,
    reset_settings_pending,
} = ctx;
```
- Full destructure at top. CORRECT.

### Call site: app/events.rs
**Line 125-143:** Constructs `&mut AppEventContext { ... }` inline with all 15 fields populated from `self.*`. CORRECT.

### Call site: shell.rs
**Lines 123-139 (process_events) and 172-188 (process_events_with_state):** Both construct `AppEventContext { ... }` with all 15 fields. CORRECT.

**VERDICT: PASS.**

---

## 3. Misc dedup helpers

### loader.rs: classify_ext and path_ext
**File:** `src/entities/loader.rs`

- `classify_ext(ext: &str) -> FileKind` at line 27. Dispatches to Video/Exr/Generic.
- `path_ext(path: &Path) -> String` at line 38. Extracts lowercased extension.
- `header()` at line 56 uses `classify_ext(&path_ext(path))`. CORRECT.
- `load()` at line 65 uses `classify_ext(&path_ext(path))`. CORRECT.

**VERDICT: PASS.**

### frame.rs: make_placeholder_u8
**File:** `src/entities/frame.rs`

- `make_placeholder_u8(width, height) -> Vec<u8>` at line 156. Creates green RGBA placeholder.
- Used in `set_status`:
  - Line 741: `data.buffer = Arc::new(PixelBuffer::U8(make_placeholder_u8(data.width, data.height)));` (Loaded->Header transition)
  - Line 769: Same pattern (Error->Header transition)
- Total 2 call sites found, both in `set_status`. CORRECT.

**VERDICT: PASS.**

### config.rs: get_app_dir
**File:** `src/config.rs`

- `get_app_dir(config: &PathConfig, platform_dir: fn() -> Option<PathBuf>) -> PathBuf` at line 105.
  - Priority: config_dir override -> local config files -> platform_dir() + "playa" -> "."
- `get_config_dir` at line 124: `get_app_dir(config, dirs_next::config_dir)`. CORRECT.
- `get_data_dir` at line 129: `get_app_dir(config, dirs_next::data_dir)`. CORRECT.

**VERDICT: PASS.**

---

## 4. ARCH-02: Loop state single source

### ToggleLoopEvent handler (line 440-444):
```rust
if downcast_event::<ToggleLoopEvent>(event).is_some() {
    // player is the runtime source of truth; settings.loop_enabled is synced
    // from player in save() before serialization, so no write needed here.
    player.set_loop_enabled(!player.loop_enabled());
    return Some(result);
}
```
- Writes to `player.set_loop_enabled()` ONLY. Does NOT touch `settings.loop_enabled`. CORRECT.

### SetLoopEvent handler (line 446-448):
```rust
if let Some(e) = downcast_event::<SetLoopEvent>(event) {
    player.set_loop_enabled(e.0);
    return Some(result);
}
```
- Writes to `player.set_loop_enabled()` ONLY. Does NOT touch `settings.loop_enabled`. CORRECT.

### Global search for `settings.loop_enabled` in main_events.rs:
Only 1 match at line 441 -- inside a **comment** explaining the design. No assignment to `settings.loop_enabled` anywhere.

**VERDICT: PASS.**

---

## Summary

| Check | Status |
|---|---|
| 1. Encode helpers dedup (strip_alpha, f16_to_f32_buf, pixel_buf_to_rgba8) | PASS |
| 1b. Helpers called from write_* functions | PASS |
| 1c. No inline duplicates (chunks_exact(4) only in helpers + 1 unrelated use) | PASS |
| 2. AppEventContext struct (15 fields) | PASS |
| 2b. handle_app_event takes (event, ctx) | PASS |
| 2c. Destructures ctx at top | PASS |
| 2d. Call site in app/events.rs | PASS |
| 2e. Call site in shell.rs | PASS |
| 3a. loader.rs: classify_ext + path_ext | PASS |
| 3b. frame.rs: make_placeholder_u8 | PASS |
| 3c. config.rs: get_app_dir | PASS |
| 4. Loop state: only player.set_loop_enabled(), no settings.loop_enabled writes | PASS |

**All 12 checks PASS. No issues found.**
