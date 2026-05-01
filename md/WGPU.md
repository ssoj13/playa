# WGPU/WASM Port Feasibility Report (Playa)

## TL;DR
- Full web (wasm32 + wgpu) port is possible but requires large refactors across rendering, IO, threading, video, and server subsystems.
- Main blockers: OpenGL renderer + GLSL, OpenGL GPU compositor, FFmpeg video pipeline, rouille HTTP server, native filesystem and dialogs, thread pool.
- Core data model, event bus, timeline logic, and CPU compositor are mostly portable with medium effort once IO and rendering are abstracted.
- Expect a staged approach: first get UI + CPU playback in web, then reintroduce GPU rendering/compositing, then tackle video and export.

## Feasibility Summary
- Feasible: UI logic, event bus, project model, CPU compositing, cache logic (with adjustments), timeline/node editor.
- Conditionally feasible: EXR (pure Rust exrs via image crate, with byte-based IO), shader system (WGSL or GLSL-to-WGSL), GPU compositor (rewrite).
- Not feasible as-is: FFmpeg decode/encode, rouille REST server, OpenGL rendering paths, filesystem paths and dialogs.

## Detailed Assessment By Area

### Entry/Runtime (src/main.rs, src/runner.rs)
- Current: eframe::run_native + NativeOptions, persistence path, drag-and-drop.
- Web impact: must use eframe WebRunner (wasm) and drop native options. Persistence uses localStorage (eframe web), not filesystem.
- Change size: medium.

### UI + Widgets (src/ui.rs, src/widgets/**)
- Current: egui is portable, most widgets are pure egui logic.
- Web impact: egui logic mostly OK, but any direct OpenGL callback usage must be replaced by wgpu render callbacks.
- Change size: small to medium (except viewport renderer).

### Viewport Rendering (src/widgets/viewport/renderer.rs, viewport_ui.rs, shaders.rs)
- Current: OpenGL via glow + GLSL shaders, PBO upload, glReadPixels screenshots, filesystem shader hot-load.
- Web impact:
  - Replace OpenGL renderer with wgpu pipeline and textures.
  - Convert GLSL to WGSL or use runtime translation (naga), and replace shader loader (no filesystem).
  - Replace PBO path with wgpu staging buffer or queue.write_texture.
  - Replace glReadPixels screenshot path with wgpu readback.
- Change size: large.

### GPU Compositor (src/entities/gpu_compositor.rs)
- Current: OpenGL FBO + GLSL blending, texture cache.
- Web impact: rewrite to wgpu render pipeline or compute shader. Alternatively disable GPU compositor on web.
- Change size: large (if retained).

### CPU Compositor (src/entities/compositor.rs, compositor.rs uses CPU blending)
- Current: pure Rust; should be portable.
- Web impact: OK; consider optimizing for single-threaded wasm.
- Change size: small.

### Loader and Media IO (src/entities/loader.rs, loader_video.rs)
- Current: image crate loads from filesystem Path; video via FFmpeg; EXR via openexr (optional) or image/exrs.
- Web impact:
  - Replace Path-based IO with byte-based IO abstraction (File, Blob, fetch, IndexedDB).
  - Drop openexr (C++ binding) on wasm; keep exrs via image crate if compatible.
  - Video decode: FFmpeg not available; must use WebCodecs (JS bridge) or ffmpeg.wasm (large, slower).
- Change size: large.

### REST API Server (src/server/**)
- Current: rouille sync HTTP server in background thread.
- Web impact: browser wasm cannot host TCP servers. Must remove or replace with:
  - Optional websocket client to a remote server.
  - PostMessage/JS API for external control.
- Change size: large (feature rewrite or removal).

### Workers/Concurrency (src/core/workers.rs)
- Current: std::thread work-stealing pool.
- Web impact: wasm single-thread by default. Options:
  - Single-threaded fallback + async tasks.
  - Enable wasm threads + SharedArrayBuffer + web worker pool (requires cross-origin isolation).
- Change size: medium to large (plus deployment constraints).

### Cache/Memory (src/core/cache_man.rs, global_cache.rs)
- Current: atomics, sysinfo for RAM sizing, LRU.
- Web impact: sysinfo not supported; must use fixed limits or browser memory heuristics. Atomics in wasm require threads feature if shared.
- Change size: medium.

### Config/Persistence (src/config.rs)
- Current: dirs-next + filesystem + env vars.
- Web impact: replace with localStorage/IndexedDB; no env vars or filesystem path.
- Change size: medium.

### File Dialogs + Drag/Drop (rfd usage, shell integration)
- Current: rfd native dialogs; OS drag-drop via eframe native.
- Web impact: use HTML file inputs or rfd web feature if available; drag-drop requires JS hooks.
- Change size: medium.

### Video Export (dialogs/encode/*)
- Current: FFmpeg-based encoding.
- Web impact: must switch to WebCodecs or ffmpeg.wasm or disable export in web.
- Change size: large.

### Misc Dependencies
- sysinfo: no wasm support (memory limits).
- rouille: no wasm support.
- playa-ffmpeg: no wasm support.
- openexr: no wasm support.
- egui_glow/glow: no wasm in wgpu backend.

## Plan

### TL;DR Plan (compressed)
1) Define target: browser wasm + wgpu, decide feature parity (video/REST/export). 2) Add platform abstraction for IO + dialogs + persistence + video. 3) Port viewport and GPU compositor to wgpu/WGSL. 4) Implement wasm runtime (WebRunner) and single-thread or web workers. 5) Replace video pipeline and server or disable on web. 6) Optimize, test, and ship web build.

### Detailed Plan (expanded)

Phase 0: Decisions and Constraints
- Decide target: browser-only or also native wgpu.
- Decide feature parity: video decode/encode, REST server, shader hot-load, filesystem-based projects.
- Decide threading model: single-thread or wasm threads (requires COOP/COEP headers).

Phase 1: Platform Abstractions
- Introduce traits for:
  - Media IO (load bytes, list sequences, read/write project files).
  - Video decode/encode.
  - Dialogs (open/save).
  - Persistence (config/settings).
- Add wasm implementation using web APIs (File/Blob, localStorage/IndexedDB, fetch).
- Keep native implementation for desktop.

Phase 2: WGPU Rendering Backbone
- Replace OpenGL viewport renderer with wgpu pipeline:
  - Create texture, sampler, bind group for frame texture.
  - Convert GLSL shaders to WGSL or use naga to translate.
  - Implement exposure/gamma uniforms in WGSL.
- Replace glReadPixels screenshot with wgpu readback buffer.
- Replace shader hot-load with embedded WGSL or user upload.

Phase 3: GPU Compositor Strategy
- Option A (fast path): port GPU compositor to wgpu (render pipeline per layer or compute shader).
- Option B (MVP): disable GPU compositor in web, rely on CPU compositor.
- Decide based on performance targets and timeline.

Phase 4: Media Pipeline
- Image: refactor loader to accept bytes; keep image crate (png/jpeg/tiff) and exrs if wasm-compatible.
- EXR: keep exrs path; drop openexr on wasm.
- Video decode: integrate WebCodecs via wasm-bindgen or use ffmpeg.wasm (heavy).
- Video encode: implement WebCodecs export or disable in web.

Phase 5: Runtime + Concurrency
- Replace worker pool with:
  - Single-threaded task queue for MVP, or
  - Web worker pool with wasm threads (opt-in).
- Adjust cache manager memory strategy without sysinfo.

Phase 6: Server/API
- Remove rouille server for web build.
- Replace with optional JS API or websocket client for remote control.

Phase 7: Build + Packaging
- Add wasm build (wasm32-unknown-unknown) and web glue (trunk/wasm-bindgen).
- Add CI target for web build.
- Add feature flags: native-only (ffmpeg, openexr, server), wasm-only (webcodecs).

## Rough Effort (Very High-Level)
- MVP web playback (images + CPU compositor + basic UI): large (weeks).
- Full parity (video decode/encode, GPU compositor, API server): very large (months).

## Risks / Unknowns
- Performance: CPU compositor + single-thread wasm may be slow for large comps.
- EXR performance and wasm memory limits for large frames.
- WebCodecs availability and codec coverage across browsers.
- Shader portability and dynamic shader loading.

## Recommended Next Step
- Clarify target scope and which features must be retained in the web port before refactoring.
