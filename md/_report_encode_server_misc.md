# Encode, Server & Misc Audit

## Critical Issues

### 1. encode.rs — FPS precision truncation causes broken timestamps (line ~1256)
`encoder.set_frame_rate` uses `settings.fps as i32`, truncating fractional rates.
Common rates like 23.976 (24000/1001), 29.97, 59.94 will all become wrong integers.
The result is that PTS/DTS calculations and stream time_base are based on the truncated
integer, so every frame's timestamp drifts. A 1-minute 29.97fps clip becomes ~0.1%
shorter, which breaks sync with audio.

**Fix**: Use a proper rational for fps:
```rust
let fps_rational = ffmpeg::util::rational::Rational::approximate(settings.fps as f64);
encoder.set_frame_rate(Some(fps_rational));
encoder.set_time_base(fps_rational.invert());
```
Or use explicit numerator/denominator pairs stored in settings.

### 2. encode.rs — `unwrap()` on stream index after write_header (line ~1392)
```rust
let stream_tb = octx.stream(0).unwrap().time_base();
```
If `octx.add_stream()` somehow fails or the muxer renumbers streams, index 0 may not
be the video stream. Should store the stream index from `add_stream` and use it here,
or propagate the error.

### 3. encode.rs — `sws_ctx.as_mut().unwrap()` in frame loop (lines ~1483, ~1494)
The 10-bit path calls `sws_ctx.as_mut().unwrap()` inside the hot encoding loop.
`sws_ctx` is `Some` only when `needs_yuv == true`. The logic is consistent but the
`.unwrap()` is still a panic risk if the logic ever diverges. Should use `?` or
`ok_or_else(...)` instead. Same pattern on the 8-bit YUV path one line below.

### 4. encode.rs — 10-bit HEVC with h264_nvenc/hevc_videotoolbox: wrong pixel format path
`hevc_videotoolbox` IS in the `needs_yuv` list but NOT in the `needs_10bit` check.
If a user selects H.265 with `main10` profile and `hevc_videotoolbox`, the code
correctly sets `pixel_format = YUV420P10LE` (because the outer block does check
videotoolbox), but `sws_ctx` will be created for `RGB48LE -> YUV420P10LE`, yet the
VideoToolbox encoder may not accept 10-bit input on all macOS versions. No validation
of this incompatibility is performed.

### 5. encode_ui.rs — `is_finished()` polling loop without Rust stable API guarantee
In `stop_encoding_internal()`, the timeout loop calls `handle.is_finished()` which
is only stable since Rust 1.61. This is fine for Rust 1.75+ but the loop also calls
`std::thread::sleep(Duration::from_millis(100))` on the **UI thread**. This blocks
the entire egui event loop for up to 2 seconds when stopping encoding. The UI will
freeze completely during this time.

**Fix**: Spin the join into a background thread or use a channel-based notification
instead of polling with sleep.

### 6. encode_ui.rs — `cleanup_orphan_handles()` silently drops join result
```rust
self.orphan_handles.retain(|handle| {
    if handle.is_finished() {
        finished_count += 1;
        false  // dropped here — join() is never called
    } else { true }
});
```
When a finished handle is removed from the Vec via `retain`, it is `Drop`ped, which
does NOT call `join()`. The thread result (including any panic info) is silently
discarded. The `Drop for EncodeDialog` does `handle.join()` but that code path is not
reached by `cleanup_orphan_handles`. It should explicitly join before discarding.

### 7. encode.rs — JPEG sequence path panics on non-U8 input with unhelpful error
`write_jpeg_frame` returns an error when buffer is not U8 but the error message says
"Apply tonemapping for HDR sources." — however the calling `encode_image_sequence`
only auto-applies tonemapping when `settings.apply_tonemap || (!format.is_hdr() && format != Rgba8)`.
JPEG is not `is_hdr()`, so the auto-tonemap trigger should fire. But if the compositor
produces `PixelFormat::Rgba8` (already 8-bit), the condition `frame.pixel_format() != PixelFormat::Rgba8`
evaluates incorrectly — the comparison uses `PixelFormat::Rgba8` (the enum variant)
not the const `PixelBuffer::U8`. If they don't map 1-to-1, the frame could reach
`write_jpeg_frame` as a non-U8 buffer. This is worth verifying.

### 8. server/api.rs — `.unwrap()` on all RwLock reads (lines ~310-325)
```rust
let player = state.player.read().unwrap().clone();
let comp   = state.comp.read().unwrap().clone();
let cache  = state.cache.read().unwrap().clone();
```
If any thread that holds the write lock panics, `read().unwrap()` will panic in the
HTTP handler thread too, taking down the rouille server thread. Should use
`.unwrap_or_else(|p| p.into_inner())` (poison recovery) or map to an HTTP 503.

### 9. server/api.rs — Path traversal in LoadSequence
```rust
fn handle_load(request: &Request, tx: &mpsc::Sender<ApiCommand>) -> Response {
    match rouille::input::json_input::<LoadRequest>(request) {
        Ok(req) => Self::send_command(tx, ApiCommand::LoadSequence(req.path)),
```
The `path` value from the JSON body is passed directly to `LoadSequence` without any
sanitization. If the API server is enabled and accessible on the network (binds to
`0.0.0.0`), any caller can load any file on the filesystem the process can read. At
minimum this should be documented. No authentication is performed on any endpoint.

### 10. encode.rs — `encode_comp` is a one-liner wrapper that adds no value
```rust
pub fn encode_comp(...) -> Result<(), EncodeError> {
    encode_sequence_from_comp(comp, project, settings, progress_tx, cancel_flag)
}
```
`encode_comp` just calls `encode_sequence_from_comp`. The function `encode_sequence_from_comp`
is also `pub`. The two names are used in different places (UI uses `encode_comp`, test
uses `encode_comp` also). The `encode_sequence_from_comp` name should be removed and
callers consolidated to `encode_comp`, or vice versa.

---

## Performance Issues

### 11. encode.rs — RGB48 copy loop is byte-by-byte instead of slice copy
In `SwsContext::convert_rgb48`, the inner loop writes each u16 component individually:
```rust
for y in 0..height {
    for x in 0..row_pixels {
        dst_data[dst_offset..dst_offset + 2].copy_from_slice(&r.to_le_bytes());
        ...
    }
}
```
This is O(width * height * 3) individual `copy_from_slice` calls of 2 bytes each.
On a 4K frame (8.3M pixels) this is ~25M tiny copies. The entire RGB48 slice could
be written as a single `copy_from_slice` using `bytemuck::cast_slice` (already used
in PNG path) since the data is already in native little-endian format on x86/ARM-LE.

### 12. encode.rs — Frame clone per-frame on non-resized frames
```rust
let frame_cropped = if frame_width != width || frame_height != height {
    frame.crop_copy(...)
} else {
    frame.clone()  // unnecessary full clone of potentially large F32 RGBA buffer
};
```
When dimensions match (the common case), the entire frame buffer is cloned just to
satisfy the type system. This doubles peak memory usage in the encoding loop.
Use `Cow<Frame>` or take by move/Arc where possible.

### 13. encode.rs — Excessive `info!` logging inside per-frame loop
The encoding loop calls `info!()` every 10 frames unconditionally. At 24fps with a
large project and the log level set to info, this generates substantial string
formatting overhead. Should gate behind `log::log_enabled!(log::Level::Debug)` or
use `debug!()` instead of `info!()` for per-frame logging.

### 14. shaders.rs — `get_shader_names()` returns cloned strings every call
```rust
pub fn get_shader_names(&self) -> Vec<String> {
    self.shaders.keys().cloned().collect()
}
```
This allocates a fresh `Vec<String>` on every call with full key clones. If called
from the render loop (e.g., to populate a ComboBox), this is a per-frame allocation.
Should return `Vec<&str>` or cache the sorted list.

### 15. encode.rs — SwsContext uses BILINEAR flag for pixel format conversion
Format conversion (RGB24→YUV420P) should use `Flags::POINT` (nearest-neighbor) or
`SWS_FAST_BILINEAR` for pure integer-to-integer pixel format conversion where no
spatial resampling occurs (same width/height). BILINEAR wastes cycles on
a weighting calculation that has no effect when src and dst dimensions are identical.

---

## Code Deduplication

### 16. encode.rs — RGBA→RGB strip is copy-pasted 8+ times
The pattern:
```rust
let mut rgb_data = Vec::with_capacity(width * height * 3);
for chunk in rgba_data.chunks_exact(4) {
    rgb_data.push(chunk[0]);
    rgb_data.push(chunk[1]);
    rgb_data.push(chunk[2]);
}
```
appears in: `write_exr_frame` (x2 for U8 and F32 paths, x2 for RGB/RGBA),
`write_png_frame` (x2), `write_tiff_frame` (x2), `write_tga_frame`, plus the video
encoding path. This should be a standalone `fn strip_alpha_u8(rgba: &[u8]) -> Vec<u8>`
and `fn strip_alpha_u16(rgba: &[u16]) -> Vec<u16>`.

### 17. encode.rs — F16→F32 conversion duplicated across all format writers
```rust
let f32_data: Vec<f32> = data.iter().map(|v| v.to_f32()).collect();
```
This appears in every single `write_*_frame` function for the F16 buffer arm. A
shared `fn f16_to_f32_buf(data: &[f16]) -> Vec<f32>` would eliminate this.

### 18. encode.rs — Buffer-to-U8 conversion is duplicated in PNG, TIFF, TGA writers
All three do:
```rust
PixelBuffer::F16(data) => data.iter().map(|v| (v.to_f32().clamp(0.0, 1.0) * 255.0) as u8).collect(),
PixelBuffer::F32(data) => data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0) as u8).collect(),
```
This should be a shared `fn frame_to_rgba8(frame: &Frame) -> Vec<u8>` used by all
three.

### 19. encode_ui.rs — `load_from_settings` / `save_to_settings` verbose trace logs
Both functions have ~30 lines of identical `log::trace!` calls dumping every field.
This is an invitation for drift (fields added to settings but not to the trace dump).
A `#[derive(Debug)]` on `EncodeDialogSettings` and a single `log::trace!("{:?}", settings)`
would replace all of it.

### 20. encode_ui.rs — render_h264_settings / render_h265_settings are near-identical
The two functions differ only in which `codec_settings.h264` vs `codec_settings.h265`
field they mutate, and the preset list (which is actually identical for H.264/H.265).
They could be merged into `render_h26x_settings(ui, settings: &mut H264Settings, id: &str)`.

### 21. prefs.rs — `SettingsCategory::as_str` / `from_str` could be replaced by Display/FromStr traits
The manual `as_str()` + `from_str()` + `match` duplicates the category→string
mapping. A `strum`-derived enum or a single const `&[(SettingsCategory, &str)]` table
would eliminate the three parallel matches.

### 22. help.rs — `render_section` closure duplicated in `render_main_help` and `render_help_overlay`
Identical closure body (render a titled section of `[HelpEntry]`) is copy-pasted into
both public functions. Should be a private `fn render_section(ui, title, entries)`.

---

## Logic Issues

### 23. encode.rs — `parse_padding_pattern`: `consumed` calculation is wrong for non-zero-padded printf
```rust
let consumed = 1 + if has_zero { 1 } else { 0 } + width_str.len() + 1; // % + 0? + digits + d
```
For `%d` (no leading zero, no width), `has_zero = false`, `width_str = ""`,
`consumed = 1 + 0 + 0 + 1 = 2`. `filename[pos + 2..]` is correct (skips `%d`).
For `%4d` (no leading zero, width=1), `has_zero = false`, `width_str = "4"`,
`consumed = 1 + 0 + 1 + 1 = 3`. `filename[pos + 3..]` correctly skips `%4d`.
For `%04d`, `has_zero = true`, `width_str = "4"`, `consumed = 1 + 1 + 1 + 1 = 4`. Correct.

Appears correct. However, the parser does NOT handle `%d` (width absent, no zero).
`width_str.parse::<usize>().unwrap_or(1)` returns 1 for empty string, which gives
width=1 padding — correct for `%d` but inconsistent with the user's intent if they
typed `%d` meaning "no padding". Minor, but documented incorrectly in the comment.

### 24. encode.rs — Frame indexing: `play_range.0..=play_range.1` without checking range validity
`total_frames = play_range.1.saturating_sub(play_range.0) + 1`. If `play_range.0 >
play_range.1` (empty range, which `saturating_sub` would make 0, then +1 = 1), the
loop would encode 1 frame (frame at `play_range.0`) instead of 0. The correct
check should verify `play_range.0 <= play_range.1` before entering the loop.

### 25. encode.rs — EXR `write_exr_frame` (non-openexr path) ignores `settings` parameter
```rust
let _ = settings; // TODO: Apply compression settings when image crate supports it
```
The `ExrSequenceSettings` is received but silently dropped. The `image` crate's EXR
encoder does support compression via `exr::meta::header::Header` when using the
lower-level API. This is documented as a TODO but the user-visible consequence is
that all EXR files are always written without compression regardless of the UI setting.

### 26. encode.rs — TGA `write_tga_frame` ignores `_settings.rle_compression`
TGA RLE compression is silently ignored (`// TODO: RLE compression when image crate
supports it`). The `image` crate TGA encoder does support RLE via
`ImageBuffer::save_with_format(..., ImageFormat::Tga)` but only as the default mode.
To control RLE, the `TgaEncoder` with `use_rle` must be used directly. This is
another silent settings override without user feedback.

### 27. encode.rs — TIFF `write_tiff_frame` ignores compression setting
```rust
let _ = settings.compression; // TODO: image crate doesn't expose TIFF compression settings easily
```
The `tiff` crate (separate from `image`) does expose compression. The `image` crate's
TIFF backend also supports `CompressionMethod` via `TiffEncoder`. Silent discard.

### 28. prefs.rs — Compositor backend changed event emitted on every render frame after change
```rust
let prev_backend = settings.compositor_backend;
// ... radio buttons mutate settings.compositor_backend
if settings.compositor_backend != prev_backend && let Some(bus) = event_bus {
    bus.emit(CompositorBackendChangedEvent { ... });
}
```
`prev_backend` is captured at the top of `render_compositing_settings` which is
called every egui frame. The event is emitted once (on the frame the user clicks) and
then never again because on the next frame `prev_backend` == `settings.compositor_backend`.
This is correct behavior, but the pattern is fragile: it depends on `render_*` being
called exactly once per frame. If egui re-renders the panel multiple times in one
frame (e.g., during resize), the event could fire multiple times. Prefer a proper
"on_changed" callback or use an explicit "pending change" flag.

### 29. input_handler.rs — Hotkey `handle_input` returns on FIRST match only
```rust
pub fn handle_input(&self, input: &egui::InputState) -> Option<BoxedEvent> {
    for event in &input.events {
        ...
        if let Some(ev) = self.handle_key_with_modifiers(...) {
            return Some(ev);
        }
        ...
    }
    None
}
```
Only the first matching event in the input queue is processed per frame. If two keys
were pressed in the same frame (possible with fast input or two-key shortcuts), only
the first is handled. This is by design for most UIs, but means chording/simultaneous
keys cannot be supported in the future without refactoring.

### 30. input_handler.rs — Duplicate key handling for unmodified keys
```rust
if let Some(ev) = self.handle_key_with_modifiers(&key_str, ctrl, shift, alt) {
    return Some(ev);
}
// Separate check for no-modifier case:
if !modifiers.any() && let Some(ev) = self.handle_key(&key_str) {
    return Some(ev);
}
```
`handle_key_with_modifiers` with all-false modifiers builds `key_combo = key_str`
(no prefix added) and calls `self.handle_key(&key_combo)`. So when `modifiers.any() == false`,
the first call already resolves the bare key. The second `if !modifiers.any()` check
is unreachable dead code — the first check would have already returned `Some(ev)`.
This is a logic bug: the second branch can never fire.

### 31. gizmo.rs — `build_gizmo_matrices` 2D mode: view matrix scale causes gizmo at wrong size
The 2D view matrix is built as:
```rust
DMat4::from_scale_rotation_translation(
    DVec3::splat(viewport_state.zoom),
    DQuat::IDENTITY,
    DVec3::new(pan.x, pan.y, 0.0),
)
```
`from_scale_rotation_translation` applies scale, then rotation, then translation.
But `glam::DMat4::orthographic_rh` is not scaled — the ortho box is `±w/2, ±h/2`.
When zoom > 1.0, the gizmo will appear larger than expected because the scale
multiplies object-space coordinates before the ortho projection clips them. Compare to
the 3D path which correctly encodes zoom into the viewport_transform matrix and keeps
the projection unscaled.

### 32. pick.rs — Layer bounds check uses `>=` and `<=` (inclusive) on both sides
```rust
let hit = obj_pos.x >= -half_w && obj_pos.x <= half_w
       && obj_pos.y >= -half_h && obj_pos.y <= half_h;
```
Inclusive on both edges means a 1-pixel layer would match points on both the left AND
right edge. This is consistent with how compositing renders the layer (inclusive
bounds), so it is correct — but worth noting that picking a 0-width or 0-height layer
(after inverse transform) would never match.

### 33. encode.rs — `EncodeStage::Error(String)` has `#[allow(dead_code)]` comment
```rust
#[allow(dead_code)] // Used in ui_encode.rs pattern matching
Error(String),
```
The comment says it is used in pattern matching in `ui_encode.rs`. But the UI actually
handles `EncodeStage::Error(msg)` in `render()` inside `encode_ui.rs`. The
`encode_sequence_from_comp` function never emits `EncodeStage::Error` — errors are
returned as `Err(EncodeError::*)` instead of being sent through the progress channel.
So the `Error` variant exists in the enum but is never actually produced by the
encoder. The `#[allow(dead_code)]` suppresses a legitimate warning about unused code.

---

## Dead Code

### 34. encode.rs — `ExrBitDepth` enum is defined but never read from `SequenceSettings`
`ExrBitDepth` (Half/Float) and `ExrSequenceSettings::bit_depth` are defined and shown
in UI, but `write_exr_frame` receives `bit_depth: OutputBitDepth`, not `ExrBitDepth`.
The `ExrSequenceSettings::bit_depth` field is never read in the writer path. The
`OutputBitDepth` passed to `write_exr_frame` comes from `settings.bit_depth` (the
top-level field), not from `settings.format_settings.exr.bit_depth`. The per-format
bit depth setting is dead.

### 35. encode.rs — `TiffBitDepth` enum is defined but not used in writer
Same as above: `TiffBitDepth` and `TiffSequenceSettings::bit_depth` exist but
`write_tiff_frame` receives `bit_depth: OutputBitDepth`. The tiff-specific depth
field is dead.

### 36. encode.rs — `PngSequenceSettings::compression` range 0..=9 but only three buckets
```rust
let compression = match settings.compression {
    0 => CompressionType::Fast,
    1..=3 => CompressionType::Fast,
    4..=6 => CompressionType::Default,
    _ => CompressionType::Best,
};
```
PNG compression slider goes 0-9 but only maps to 3 distinct values. The range
`1..=3` and `0` both map to Fast. User adjusting slider from 0 to 3 sees no
difference. The label says "level" implying 0-9 granularity, which is misleading.

### 37. prefs.rs — `render_general_settings` is empty stub
```rust
fn render_general_settings(ui: &mut egui::Ui, _settings: &mut AppSettings) {
    ui.label("General settings will be added here.");
}
```
The General settings category shows a placeholder label. The function signature takes
`AppSettings` but uses it for nothing. Either add content or remove the category from
the tree view.

### 38. prefs_events.rs — `HotkeyWindow` enum is in prefs_events but not a prefs event
`HotkeyWindow` is defined in `prefs_events.rs` but is not an event — it is state
used by `HotkeyHandler` in `input_handler.rs`. It belongs in `input_handler.rs` or a
shared hotkeys module, not in the prefs events file.

### 39. server/mod.rs — Comments list endpoints not reflected in api.rs
The mod-level doc table lists `POST /api/player/frame/{n}` but the actual router does
not have this as a static route — it is handled manually before the `router!` macro.
The comment is accurate but misleads readers into thinking it is in the router. Minor.

### 40. file_dialogs.rs — `create_media_dialog` takes a `title` parameter but sets no title
```rust
pub fn create_media_dialog(title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .add_filter("All Supported Files", crate::utils::media::ALL_EXTS)
        .set_title(title)
}
```
Actually `set_title(title)` IS called — this is fine. But the function body is only
3 lines and could be inlined at the two call sites (if any exist) or kept for DRY.
Not a bug.

---

## Security

### 41. server/api.rs — API server binds to `0.0.0.0` (all interfaces)
The server binds `"0.0.0.0:{port}"` with no authentication, rate limiting, or access
control. Any machine on the local network (or internet if a port is forwarded) can:
- Load arbitrary files via `/api/project/load`  
- Exit the application via `/api/app/exit`  
- Take screenshots via `/api/screenshot`  
- Control playback

Should default to `127.0.0.1` (localhost only) and document the security implications
of changing it. Consider adding a simple token-based auth header check.

### 42. server/api.rs — No input validation on FPS value
```rust
if let Ok(fps) = fps_str.parse::<f32>() {
    return Self::send_command(tx, ApiCommand::SetFps(fps));
}
```
A caller can set FPS to `0.0`, `NaN`, `Inf`, or negative values. If the main thread
uses this FPS value in division (fps as denominator), it will produce NaN/Inf/div-zero.
Should clamp: `fps.clamp(0.001, 960.0)` before sending.

### 43. server/api.rs — No input validation on frame number
`/api/player/frame/{n}` accepts any `i32`. Sending a very large or very negative
frame index could cause undefined behavior downstream if the frame index is used as
an array index without bounds check. Should be documented that the receiver is
responsible for validation.

---

## Recommendations

### R1. Replace `info!()` with `debug!()` for per-frame encode logging
The encoding loop logs at `info!` level every 10 frames. In production this creates
significant noise. Use `debug!()` for frame-level, keep `info!()` only for
encode-start, encode-complete, and errors.

### R2. Add `cancel_flag` check in SwsContext::convert calls
The format conversion inside `SwsContext::convert` and `convert_rgb48` cannot be
cancelled mid-operation. For very large frames this is fine. But between the crop,
tonemap, and two conversion steps, the cancel check only runs once (before the crop).
Add checks at each pipeline stage boundary.

### R3. Store stream index explicitly
After `octx.add_stream(codec)`, capture the returned stream index:
```rust
let stream_idx = ost.index();
```
Then use `stream_idx` for all `encoded.set_stream()` and `octx.stream(stream_idx)`
calls instead of hardcoding `0`.

### R4. `SettingsCategory::from_str` should handle unknown categories gracefully in the tree
Currently if a serialized settings file contains an unrecognized category name,
`from_str` returns `None` and the code falls back to `SettingsCategory::UI`. This
is handled correctly. But `as_str` / `from_str` is a maintenance trap — adding a
new category requires updating 3 places (the enum, `as_str`, `from_str`). Use a
derived approach or a `const` table.

### R5. Encode dialog window should show remaining time estimate
During long encodes, users have no idea how long is left. A simple
`elapsed / current_frame * remaining_frames` calculation would provide useful ETA.

### R6. Screenshot comment says "viewport_only=true means full window" but the parameter is named `viewport_only`
In `handle_request`:
```rust
(GET) ["/api/screenshot"] => {
    Self::handle_screenshot(tx, state, true)  // viewport_only=true means full window
}
```
The comment is inverted relative to what `viewport_only=true` implies. Either the
parameter name or the comment is wrong. Clarify the semantics.

### R7. `shaders.rs` — `load_shader_directory` uses a relative path "shaders"
```rust
if manager.load_shader_directory(Path::new("shaders")).is_err() {
```
This is relative to the process CWD, which will vary depending on how the app is
launched (from IDE, from terminal, from file manager). Should use the executable's
directory or an absolute path derived from it.

### R8. `utils.rs` — `ALL_EXTS` includes `mkv` but `VIDEO_EXTS` also includes `mkv`
Both lists include `mkv`. `ALL_EXTS` includes all VIDEO_EXTS inline — they are not
derived from `VIDEO_EXTS`. Adding a new video format requires updating two places.
Consider: `pub const ALL_EXTS: &[&str] = &[VIDEO_EXTS, IMAGE_EXTS].concat()` or
at least derive ALL_EXTS from VIDEO_EXTS + IMAGE_EXTS as a const fn.

### R9. `coords.rs` — Helper functions are unused if the viewport uses ViewportState directly
`screen_to_viewport_centered` and `screen_delta_to_viewport` in `coords.rs` may be
dead code if `pick.rs` and `gizmo.rs` both use `ViewportState::screen_to_image`
directly. Verify call sites exist; if not, remove or mark `#[allow(dead_code)]`.

### R10. Encode: consider parallel frame rendering
`encode_sequence_from_comp` calls `comp.get_frame(frame_idx, ...)` serially. If the
compositor is CPU-bound, pre-rendering several frames in parallel into a bounded
queue would hide latency. This is a significant architectural improvement for long
sequences on multi-core systems.
