# Bug Verification Report

## BUG-01: NaN in F32 Porter-Duff blend
**Status:** DENIED
**Actual code (compositor.rs:156-164):**
```rust
for i in (0..bottom.len()).step_by(4) {
    let top_alpha = top[i + 3] * opacity;
    let inv_alpha = 1.0 - top_alpha;

    result[i] = bottom[i] * inv_alpha + apply_blend(bottom[i], top[i], mode) * top_alpha;
    result[i + 1] = bottom[i + 1] * inv_alpha + apply_blend(bottom[i + 1], top[i + 1], mode) * top_alpha;
    result[i + 2] = bottom[i + 2] * inv_alpha + apply_blend(bottom[i + 2], top[i + 2], mode) * top_alpha;
    result[i + 3] = bottom[i + 3] * inv_alpha + top_alpha;
}
```
**Analysis:** The claim says "divides by result_alpha". There is NO division anywhere in this function — it is a straight Porter-Duff over composite using `inv_alpha = 1.0 - top_alpha`. No `result_alpha` variable exists. No division by zero is possible. The F16 path (lines 179-195) is identical in structure with no division either.
**Additional context:** The claim appears to be about a different algorithm (pre-multiplied alpha un-premultiply step). That code does not exist here.

---

## BUG-02: ApiCommand::Play double-emits TogglePlayPauseEvent
**Status:** CONFIRMED
**Actual code (api.rs:90-95):**
```rust
ApiCommand::Play => {
    self.event_bus.emit(TogglePlayPauseEvent);
    if !self.player.is_playing() {
        self.event_bus.emit(TogglePlayPauseEvent);
    }
}
```
**Analysis:** The logic is inverted. The intent is "ensure playback starts", but:
1. First emit unconditionally fires — if player was already playing, this STOPS it.
2. Then it checks `!is_playing()` — which is now `true` because step 1 just stopped it.
3. Second emit fires again, restarting playback.

Net result when already playing: toggle off, then toggle on (two unnecessary state changes, brief stop). Net result when paused: toggle on (correct), condition is false so no second emit. So Play works when paused, but double-fires when already playing, causing a visible stutter/restart.

The correct logic should be:
```rust
if !self.player.is_playing() {
    self.event_bus.emit(TogglePlayPauseEvent);
}
```

---

## BUG-03: NodeKind::fps() shadows enum_dispatch — Camera/Text hardcode 24.0
**Status:** CONFIRMED
**Actual code (node_kind.rs:162-169):**
```rust
pub fn fps(&self) -> f32 {
    match self {
        NodeKind::File(n) => n.fps(),
        NodeKind::Comp(n) => n.fps(),
        NodeKind::Camera(_) => 24.0, // Default
        NodeKind::Text(_) => 24.0,   // Default
    }
}
```
**Trait default (node.rs:168-171):**
```rust
/// Frames per second. Default: DEFAULT_FPS (24.0)
fn fps(&self) -> f32 {
    self.attrs().get_float(A_FPS).unwrap_or(DEFAULT_FPS)
}
```
**CameraNode constructor (camera_node.rs:37-71):** No `A_FPS` / `fps` attr is ever set in `CameraNode::new()`.

**TextNode constructor:** The `text_node.rs` file has no `fps` attr set either (grep returned zero results for `fps` in text_node.rs).

**Analysis:** CONFIRMED with nuance. Both CameraNode and TextNode do not set `A_FPS` in their constructors, so even if `NodeKind::fps()` called `n.fps()` via the trait, it would fall back to `DEFAULT_FPS` (24.0) anyway — same result. The shadow is real code smell and will break if someone ever sets `A_FPS` on a camera or text node (e.g. after a project-level FPS change), but currently it is not a behavioral bug — it produces the same 24.0 both ways. The real fix is to delegate to `n.fps()` and also set `A_FPS` in constructors.

---

## BUG-04: FPS denominator zero-check
**Status:** CONFIRMED
**Actual code (loader_video.rs:43-49):**
```rust
let fps_rational = stream.avg_frame_rate();
let time_base = stream.time_base();

let duration_secs =
    duration as f64 * time_base.numerator() as f64 / time_base.denominator() as f64;
let fps = fps_rational.numerator() as f64 / fps_rational.denominator() as f64;
let frame_count = (duration_secs * fps) as usize;
```
**Analysis:** `fps_rational.denominator()` can be 0 for malformed or stream-less containers (ffmpeg returns `AVRational{0,0}` for unknown frame rates). Division by zero on integers in Rust panics in debug and wraps/undefined in release. Additionally `time_base.denominator()` can also theoretically be zero. `frame_count` would then be `NaN as usize` which is 0 on x86 but is UB per the Rust reference. No guard exists.

---

## BUG-05: GPU compositor partial init
**Status:** CONFIRMED — partial init IS possible on error
**Actual code (gpu_compositor.rs:264-326):**
```rust
fn ensure_initialized(&mut self) -> Result<(), String> {
    if self.blend_program.is_some() && self.vao.is_some() && self.fbo.is_some() {
        return Ok(());
    }
    unsafe {
        self.blend_program = Some(self.compile_blend_shader()?);  // line 276
        // ... vao/vbo created ...
        self.vao = Some(vao);   // line 314
        self.vbo = Some(vbo);   // line 315
        let fbo = gl.create_framebuffer()
            .map_err(|e| format!("Failed to create FBO: {}", e))?;  // line 320 can early-return
        self.fbo = Some(fbo);   // line 321
    }
}
```
**Analysis:** CONFIRMED. The early-return guard checks `blend_program.is_some() && vao.is_some() && fbo.is_some()`. If `compile_blend_shader()` succeeds (sets `blend_program = Some`) and then VAO creation succeeds (sets `vao = Some`) but FBO creation fails with `?`, the function returns `Err` with `blend_program` and `vao` already set to `Some` but `fbo` still `None`. On the next call, the guard `blend_program.is_some() && vao.is_some() && fbo.is_some()` is `false` (fbo is None), so it re-enters the init block and attempts to compile the shader again and re-create the VAO, leaking the previously created OpenGL resources (shader program and VAO/VBO are orphaned on the GPU).

---

## BUG-06: deferred_load_sequences overwrites
**Status:** CONFIRMED
**Actual code (events.rs:165-166):**
```rust
if let Some(paths) = result.load_sequences {
    deferred_load_sequences = Some(paths);
}
```
**Analysis:** This is `= Some(paths)`, not `.extend()`. If two events in the same poll cycle both produce `load_sequences` results (e.g. two drag-drop events), the first is silently discarded. The same overwrite pattern is present for `load_project`, `save_project`, `new_comp`, `new_camera`, `new_text` (lines 159-174). For sequences specifically, users could drag-drop multiple folders and only the last one would be processed.

---

## BUG-07: ApiCommand::SetFps bypasses event bus
**Status:** CONFIRMED
**Actual code (api.rs:107-109):**
```rust
ApiCommand::SetFps(fps) => {
    self.player.set_fps_base(fps);
}
```
**Keyboard shortcut path (main_events.rs:142-151):**
```rust
fn adjust_fps_base(player: &mut Player, project: &mut Project, increase: bool) {
    if increase {
        player.increase_fps_base();
    } else {
        player.decrease_fps_base();
    }
    if let Some(comp_uuid) = player.active_comp() {
        let fps = player.fps_base();
        project.modify_comp(comp_uuid, |comp| comp.set_fps(fps));
    }
}
```
**Analysis:** CONFIRMED. `ApiCommand::SetFps` only calls `player.set_fps_base(fps)` — it does NOT update the active comp's `A_FPS` attribute via `project.modify_comp(...)`. The keyboard shortcut path does both. This means REST API FPS changes will not persist to the project, won't emit `AttrsChangedEvent`, won't invalidate the cache, and won't be saved if the project is serialized afterward.

---

## BUG-08: HSV default value = 2.0
**Status:** CONFIRMED
**Actual code (effects/mod.rs:145-146):**
```rust
// value: 0.0 (black) to 2.0 (overbright), 1.0 = no change
AttrDef::with_ui_order("value", AttrType::Float, FX, &["0", "2", "0.01"], 2.0),
```
**Analysis:** The comment explicitly says "1.0 = no change" but the default value (last argument `2.0`) is set to `2.0` (maximum overbright). Any newly created HSV effect will immediately double the brightness of the layer. The correct default should be `1.0`. This is a clear copy-paste/off-by-one bug — the `saturation` field directly above it correctly uses `1.0`.

---

## BUG-09: SetFrameEvent double preload
**Status:** CONFIRMED — double preload path exists
**SetFrameEvent handler (main_events.rs:243-265):**
```rust
if let Some(e) = downcast_event::<SetFrameEvent>(event) {
    // ...
    project.modify_comp(comp_uuid, |comp| { comp.set_frame(e.0); });
    result.enqueue_frames = true;
    return Some(result);
}
```
**`enqueue_frames` handling (events.rs:177, 266-267):**
```rust
deferred_enqueue_frames |= result.enqueue_frames;
// ...
if deferred_enqueue_frames {
    self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
}
```
**`CurrentFrameChangedEvent` handler (events.rs:42-45):**
```rust
if let Some(e) = downcast_event::<CurrentFrameChangedEvent>(&event) {
    self.enqueue_frame_loads_around_playhead(self.settings.preload_radius);
    continue;
}
```
**Analysis:** CONFIRMED. `comp.set_frame()` internally emits `CurrentFrameChangedEvent`. That event is caught in the same `handle_events()` loop and triggers `enqueue_frame_loads_around_playhead()` immediately (line 44). Then `result.enqueue_frames = true` causes a second call to the same function at the deferred step (line 267). So every `SetFrameEvent` causes two calls to `enqueue_frame_loads_around_playhead()` per frame step.

---

## BUG-10: Dehydrate vs Loading race
**Status:** PARTIALLY CONFIRMED — logic gap exists, severity depends on caller
**clear_comp dehydrate path (global_cache.rs:373-389):**
```rust
if dehydrate {
    if let Some(frames) = cache.get_mut(&comp_uuid) {
        for (&idx, frame) in frames.iter() {
            if except == Some(idx) { continue; }
            if frame.status() == FrameStatus::Loaded {
                let _ = frame.set_status(FrameStatus::Expired);
            }
        }
    }
}
```
**set_status Loading case (frame.rs:782-789):**
```rust
(FrameStatus::Loading, FrameStatus::Header) => {
    let mut data = self.data.lock().unwrap();
    data.status = FrameStatus::Header;
    // ...
}
```
**Analysis:** The dehydrate path only marks `Loaded` frames as `Expired`. Frames in `Loading` state are skipped entirely (the condition `frame.status() == FrameStatus::Loaded` is false for them). This means:
- A frame currently being loaded by a worker is NOT marked Expired.
- The worker finishes, stores `Loaded` pixels into a frame that the compositor thinks is now fresh.
- Those pixels are stale (computed before the attr change that triggered the dehydrate).
- The compositor may display the stale frame without triggering a recompute.

The race window is: `clear_comp(dehydrate=true)` runs → worker completes → frame transitions `Loading → Loaded` with old data → compositor picks it up as valid. This is a real correctness issue but not a crash.

---

## BUG-11: CameraNode use_poi mismatch
**Status:** CONFIRMED
**Constructor (camera_node.rs:50):**
```rust
attrs.set("use_poi", AttrValue::Bool(false)); // default: rotation mode
```
**Getter (camera_node.rs:108-109):**
```rust
pub fn use_poi(&self) -> bool {
    self.attrs.get_bool("use_poi").unwrap_or(true)
}
```
**Analysis:** The constructor sets `use_poi = false` (rotation mode). The getter's fallback is `unwrap_or(true)` (POI mode). The fallback is never reached for a normally constructed node since the attr is always set, BUT if a node is deserialized from an old project file that lacks the `use_poi` key, the getter returns `true` while the constructor default is `false`. This is an inconsistency that causes different behavior for legacy project files vs newly created cameras.

---

## BUG-12: contains_comp() type check
**Status:** CONFIRMED — no type discrimination
**Actual code (project.rs:649-652):**
```rust
/// Check if comp exists in media pool
pub fn contains_comp(&self, uuid: Uuid) -> bool {
    self.contains_node(uuid)
}
```
**contains_node (project.rs:644-647):**
```rust
pub fn contains_node(&self, uuid: Uuid) -> bool {
    self.media.read().expect("media lock poisoned").contains_key(&uuid)
}
```
**Analysis:** CONFIRMED. `contains_comp()` does not verify the node at `uuid` is actually a `NodeKind::Comp`. It returns `true` for any node UUID (File, Camera, Text). Any caller using `contains_comp()` to guard a comp-specific operation could receive a `true` result for a non-comp node, then pass that UUID to comp-specific methods and get a type mismatch or `None` return silently.

---

## BUG-13: Float truncation in video frame count
**Status:** CONFIRMED
**Actual code (loader_video.rs:48-49):**
```rust
let fps = fps_rational.numerator() as f64 / fps_rational.denominator() as f64;
let frame_count = (duration_secs * fps) as usize;
```
**Analysis:** CONFIRMED. `as usize` truncates toward zero — there is no rounding. For a 10-second clip at 29.97 fps: `10.0 * 29.97 = 299.7`, truncated to `299`. The last frame is lost. Standard practice is to use `.round() as usize` or `((duration_secs * fps) + 0.5) as usize`. Additionally, this combines with BUG-04: if `fps_rational.denominator()` is 0, the division produces `NaN` or `inf`, and `NaN as usize` is 0 on x86 but is documented undefined behavior in Rust.
