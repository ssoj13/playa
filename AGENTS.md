# AGENTS.md - AI Agent Guidelines for Playa

## Project Overview

**Playa** is a cross-platform image sequence player written in Rust.
Key features: async frame loading, OpenGL rendering, FFmpeg video encoding, egui UI, single binary distribution.

**Repository**: https://github.com/ssoj13/playa
**Version**: 0.1.133+
**Rust Edition**: 2024

---

## Architecture Summary

```
playa/
├── src/
│   ├── main.rs              # Entry point, event loop, window init
│   ├── lib.rs               # Library re-exports
│   ├── cli.rs               # CLI argument parsing (clap)
│   ├── config.rs            # Settings persistence (JSON)
│   ├── help.rs              # Help overlay
│   ├── shell.rs             # Shell integration
│   ├── ui.rs                # Main UI orchestration
│   ├── utils.rs             # Utility functions
│   ├── main_events.rs       # Application-level events
│   │
│   ├── core/                # Engine (UI-independent)
│   │   ├── cache_man.rs     # CacheManager - orchestrates caching
│   │   ├── event_bus.rs     # Type-erased event system
│   │   ├── global_cache.rs  # GlobalFrameCache - LRU + epoch counter
│   │   ├── player.rs        # Playback state machine
│   │   ├── player_events.rs # Player-specific events
│   │   ├── project_events.rs# Project lifecycle events
│   │   └── workers.rs       # Thread pool for frame loading
│   │
│   ├── entities/            # Domain models
│   │   ├── attrs.rs         # Layer attributes (blend mode, opacity)
│   │   ├── comp.rs          # Composition (layers, timeline)
│   │   ├── comp_events.rs   # Composition events
│   │   ├── compositor.rs    # CPU compositor
│   │   ├── gpu_compositor.rs# GPU compositor (OpenGL)
│   │   ├── frame.rs         # Frame data + status
│   │   ├── keys.rs          # Keyframe data
│   │   ├── loader.rs        # Image loaders (EXR, PNG, etc.)
│   │   ├── loader_video.rs  # FFmpeg video loader
│   │   └── project.rs       # Project state
│   │
│   ├── widgets/             # UI components (egui)
│   │   ├── ae/              # After Effects-style panels
│   │   ├── project/         # Project panel
│   │   ├── status/          # Status bar + progress
│   │   ├── timeline/        # Timeline widget
│   │   └── viewport/        # Viewport + OpenGL rendering
│   │
│   ├── dialogs/             # Modal dialogs
│   │   ├── encode/          # Video encoding dialog
│   │   └── prefs/           # Preferences dialog
│   │
│   └── bin/                 # Additional binary targets
│       ├── attributes.rs
│       ├── encoder.rs
│       ├── prefs.rs
│       ├── project.rs
│       ├── timeline.rs
│       └── viewport.rs
│
├── xtask/                   # Build automation
│   └── src/
│       ├── main.rs          # CLI dispatcher
│       ├── lib_discovery.rs # Native library detection
│       ├── post_build.rs    # Post-build tasks
│       ├── pre_build.rs     # Pre-build tasks (Linux header patching)
│       └── release.rs       # Release management
│
├── .github/workflows/       # CI/CD
│   ├── main.yml             # Main build/release workflow
│   ├── warm-cache.yml       # Dependency cache warming
│   ├── _build-backend.yml   # Backend matrix (exrs/openexr)
│   └── _build-platform.yml  # Platform matrix
│
├── bootstrap.ps1            # Windows bootstrap (PowerShell)
├── bootstrap.sh             # Unix bootstrap
├── build.rs                 # Cargo build script
├── Cargo.toml               # Workspace + dependencies
└── cliff.toml               # git-cliff changelog config
```

---

## Key Patterns

### 1. EventBus Pattern
Type-erased pub/sub system for decoupling components:
```rust
// Emit event
event_bus.emit(PlayerEvent::FrameChanged { frame: 42 });

// Subscribe
event_bus.subscribe::<PlayerEvent>(|event| { ... });
```
Location: `src/core/event_bus.rs`

### 2. Epoch-Based Cancellation
Atomic counter to cancel stale async requests on scrub/seek:
```rust
// On scrub: increment epoch
current_epoch.fetch_add(1, Ordering::SeqCst);

// Workers check epoch before processing
if req.epoch != current_epoch.load(Ordering::SeqCst) {
    return; // Skip stale request
}
```
Location: `src/core/global_cache.rs`

### 3. Frame Status State Machine
```
Placeholder -> Header -> Loading -> Loaded
                               \-> Error
```
Location: `src/entities/frame.rs`

### 4. LRU Cache with Memory Budget
Self-limiting cache based on system RAM percentage.
Location: `src/core/global_cache.rs`

---

## Build System

### Bootstrap (Recommended)
```powershell
# Windows (PowerShell ONLY - no bash/cmd)
.\bootstrap.ps1 build              # exrs backend (fast)
.\bootstrap.ps1 build --openexr    # OpenEXR backend (full DWAA/DWAB)
.\bootstrap.ps1 test               # Run tests
```

### xtask Commands
```powershell
cargo xtask build [--release] [--openexr]
cargo xtask post [--release]       # Copy native libs
cargo xtask verify [--release]     # Verify deps
cargo xtask deploy                 # Install to system
cargo xtask tag-dev [patch|minor|major]
cargo xtask tag-rel [patch|minor|major]
cargo xtask pr [version]
cargo xtask changelog
```

### Features
- `default` - Pure Rust exrs backend
- `openexr` - C++ OpenEXR backend (DWAA/DWAB support)

---

## Development Guidelines

### Code Style
- **Modern Rust**: Edition 2024, use latest idioms
- **Typing**: Explicit types where it aids readability
- **Comments**: Concise, explain WHY not WHAT
- **Docstrings**: All public items
- **Naming**: Short but meaningful (`get_tr()` not `extract_translation()`)
- **Paths**: Use `std::path::PathBuf`, never string paths
- **Strings**: f-strings / format! macros

### Platform Notes
- **Primary dev**: Windows 11
- **Shell**: PowerShell ONLY (`pwsh.exe`), NEVER bash/cmd
- **Paths**: Use `\\` or raw strings on Windows
- **vcpkg**: `VCPKG_ROOT=c:\vcpkg` (already installed)
- **Build warnings**: Always fix, never ignore

### Dependencies
- **UI**: egui 0.33, eframe, egui_glow
- **Graphics**: glow (OpenGL), half (f16)
- **Image**: image 0.25 (exrs), openexr 0.11 (optional)
- **Video**: playa-ffmpeg 8.0.3
- **Async**: crossbeam-channel, rayon
- **CLI**: clap 4.5
- **Serialization**: serde, serde_json

---

## Common Tasks

### Adding a New Event
1. Define event struct in appropriate `*_events.rs`
2. Implement `std::any::Any` trait
3. Emit via `EventBus::emit()`
4. Subscribe in relevant component

### Adding a Widget
1. Create module in `src/widgets/`
2. Create `mod.rs` with widget struct
3. Implement `egui::Widget` or custom draw method
4. Wire up events in `EventBus`
5. Add to `src/widgets/mod.rs` exports

### Adding a Loader
1. Add format detection in `src/entities/loader.rs`
2. Implement `load_*` function
3. Update `Frame::load()` dispatch
4. Add file extension to CLI and file associations

### Testing Video Encoding
```powershell
.\bootstrap.ps1 build --release
.\target\release\playa.exe -f test.0001.exr
# Press F4 to open encoding dialog
```

---

## CI/CD Notes

### Workflows
- `main.yml` - Triggered on push to main/dev, releases on tag
- `warm-cache.yml` - Pre-builds dependencies for faster CI
- Matrix: `exrs` x `openexr` x `windows` x `linux` x `macos`

### Release Process
1. `cargo xtask tag-rel patch` - Create release tag
2. CI builds all platforms
3. Artifacts: `.exe`, `.msi`, `.dmg`, `.deb`, `.AppImage`

---

## Common Pitfalls

### Windows-Specific
- NEVER use bash commands - always PowerShell
- Paths: `C:\path` not `/c/path`
- Double backslashes in strings: `"C:\\vcpkg"`
- vcpkg triplet: `x64-windows-static-md-release`

### Build Issues
- OpenEXR feature requires C++ compiler + CMake
- FFmpeg requires vcpkg with specific features
- Linux: May need `cargo xtask pre` for GCC 11+ header patching

### Memory Management
- Cache respects memory budget (50% RAM default)
- Workers cancel stale requests via epoch
- Large sequences: preload thread uses spiral pattern

---

## Quick Reference

| Task | Command |
|------|---------|
| Build debug | `.\bootstrap.ps1 build` |
| Build release | `.\bootstrap.ps1 build --release` |
| Run tests | `.\bootstrap.ps1 test` |
| Format code | `cargo fmt` |
| Lint | `cargo clippy` |
| Update deps | `cargo update` |
| Check features | `cargo check --features openexr` |

---

## Contact and Resources

- **Issues**: https://github.com/ssoj13/playa/issues
- **Changelog**: CHANGELOG.md
- **Architecture**: See README.md "Architecture" section
- **Data Flow**: See README.md "Data Flow" section

---

## Agent-Specific Instructions

### For Code Review Agents
- Check for platform-specific issues (Windows paths, shell commands)
- Verify epoch-based cancellation in async code
- Ensure EventBus events are properly typed
- Look for memory leaks in Arc/Mutex patterns

### For Implementation Agents
- Always read existing code before modifying
- Use EventBus for component communication
- Follow existing patterns in similar modules
- Test on Windows first (primary platform)
- Fix ALL build warnings before committing

### For Documentation Agents
- Keep README.md updated with new features
- Update CHANGELOG.md via git-cliff
- Document public APIs with rustdoc
- Keep this agents.md in sync with codebase

### For Testing Agents
- Unit tests in same file as code
- Integration tests in `tests/` directory
- Visual testing: manual verification required
- Encoding tests: verify FFmpeg output

---

*Last updated: 2025-12-12*
*Generated for Playa v0.1.133+*
