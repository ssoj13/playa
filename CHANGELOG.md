## [Unreleased] - dev4 Branch

### 🚀 Features

- **REST API Server**: New HTTP API for remote control of Playa
  - Endpoints: `/api/status`, `/api/player/*`, `/api/screenshot`
  - Configurable port in Settings -> Web Server
  - CORS enabled for browser access
  - Commands: Play, Pause, Stop, SetFrame, SetFps, Screenshot, Exit

- **Viewport Gizmo**: Interactive transform manipulation
  - Move/Rotate/Scale gizmos via transform-gizmo-egui
  - Tool modes: Select, Move, Rotate, Scale
  - Multi-layer selection support

- **Layer Picker**: Click-to-select layers in viewport
  - Left click in Select mode (Q) picks topmost layer under cursor
  - Raycast-based hit testing with inverse transform
  - Click empty space to deselect

- **Hover Highlight**: Visual feedback when hovering layers
  - Mouse over layer in viewport highlights it in timeline (orange border)
  - Works in Select mode (Q)
  - Helps identify layers before clicking

- **3D Transform System**: Full affine transforms with perspective
  - Frame space as primary coordinate system (centered, Y-up)
  - ZYX rotation order (After Effects compatible)
  - Ray-plane intersection for perspective unproject
  - Improved coordinate space conversions

- **Timeline Improvements**
  - Trim hotkeys: `[`/`]` snap edges to cursor
  - `Alt-[`/`Alt-]` trim at cursor regardless of mouse
  - Layer selection and multi-select
  - Jump to layer edges navigation

- **Camera Node Enhancements**
  - Improved transform handling
  - Better integration with gizmo system

- **Layer Effects System**: Per-layer post-processing effects
  - Gaussian Blur (separable, O(n*r) per pass)
  - Brightness/Contrast adjustment
  - HSV color correction (hue shift, saturation, value)
  - Effects UI in Attribute Editor with add/remove/reorder
  - Schema-based parameters with proper validation
  - Integrated into compositor pipeline (applied before transform)

### 🏗️ Architecture

- **Arc<NodeKind> in media pool**: Lock-free worker access
  - Workers clone Arc (nanoseconds), release lock immediately
  - UI can acquire write lock without waiting for compute
  - Eliminates jank during heavy computation

- **Attribute System Improvements**
  - Schema-based validation for all node types
  - DAG vs non-DAG attribute distinction
  - Auto-emit AttrsChangedEvent on dirty

### 📚 Documentation

- Create comprehensive AGENTS.md with:
  - Component architecture documentation
  - Dataflow diagrams (integrated from DATAFLOW.txt)
  - AI assistant guidelines
  - Coordinate system reference
- Remove obsolete DATAFLOW.txt (content merged into AGENTS.md)
- Add src/README.md with module structure

### 🐛 Bug Fixes

- Fix event downcasting with blanket impl types
- Improved dirty tracking in modify_comp() pattern
- Better epoch-based cancellation handling

### ⚙️ Miscellaneous Tasks

- Multiple WIP commits during active development (Dec 19-21, 2025)
- Cleanup of temporary plan/report files

---

## [0.1.133] - 2025-11-15

### 🚀 Features

- Add `cargo xtask test` command for unified test execution
- Add PowerShell bootstrap script with VCPKG_ROOT support

### 🐛 Bug Fixes

- Bootstrap scripts now show their own help instead of xtask help

### 🚜 Refactor

- Move test command from bootstrap to xtask
- Sync bootstrap.sh with bootstrap.ps1 VCPKG_ROOT logic

### 📚 Documentation

- Update bootstrap help text for test command

### ⚙️ Miscellaneous Tasks

- WIP Sat 11/15/2025 - 11:34:02.18
## [0.1.132] - 2025-11-15

### 🐛 Bug Fixes

- Critical bugs and optimizations from plan.md

### 📚 Documentation

- Update README with recent fixes and optimizations

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/13/2025 -  0:08:10.00
## [0.1.131] - 2025-11-12

### 🚀 Features

- Add frame stepping with loop/clamp support
- Remap arrow keys and JKL shortcuts

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/11/2025 -  8:02:45.31
- Remove unused decrease_fps_play method
## [0.1.130] - 2025-11-11

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/11/2025 -  7:43:41.61
- WIP Tue 11/11/2025 -  7:45:40.58
## [0.1.129] - 2025-11-11

### 🚀 Features

- Add H.265 profile support and fix ProRes HDR encoding
- Add flamegraph profiling support to bootstrap scripts
- Improve FPS control and playback workflow

### 💼 Other

- Add extensive logging for ProRes encoding investigation
- Add timestamp logging for performance profiling

### ⚙️ Miscellaneous Tasks

- WIP Mon 11/10/2025 - 21:46:33.22
- WIP Mon 11/10/2025 - 23:29:05.42
- WIP Tue 11/11/2025 -  0:26:53.66
## [0.1.128] - 2025-11-10

### 🚀 Features

- Add comprehensive CLI arguments for player control
- Add sequence navigation and frame number display

### 📚 Documentation

- Add CLI documentation and improve shader messages
- Reorganize installation instructions, prioritize pre-built installers

### ⚙️ Miscellaneous Tasks

- WIP Mon 11/10/2025 -  1:24:03.47
## [0.1.127] - 2025-11-10

### 🚀 Features

- Improve video encoding with frame conversion, GOP settings, and UI fixes
- Add timeline support, fix AV1/ProRes encoding, improve encoding UI
- Add smart frame status management and play range cache optimization

### 🐛 Bug Fixes

- Fix MP4 timeline and improve encode cancellation

### 🚜 Refactor

- Remove LRU cache, use status-based frame management

### 📚 Documentation

- Add CLAUDE.md for AI-assisted development

### ⚙️ Miscellaneous Tasks

- WIP Sun 11/09/2025 - 17:51:52.86
- WIP Sun 11/09/2025 - 20:21:00.01
- WIP Sun 11/09/2025 - 21:44:14.83
- WIP Sun 11/09/2025 - 23:07:29.37
## [0.1.126] - 2025-11-09

### 🚀 Features

- Add install command to bootstrap scripts

### 🐛 Bug Fixes

- Use release triplet in bootstrap.cmd for static FFmpeg
- Correct FFmpeg features to match CI/CD configuration

### 📚 Documentation

- Add bootstrap install as recommended installation method
## [0.1.125] - 2025-11-09

### 🚀 Features

- Add vcpkg env variables and package command to bootstrap

### ⚙️ Miscellaneous Tasks

- WIP Sun 11/09/2025 - 10:09:49.37
## [0.1.124] - 2025-11-09

### 🚀 Features

- Add video playback support (.mp4, .mov, .avi, .mkv)
- *(ci)* Setup vcpkg for FFmpeg auto-install by playa-ffmpeg
- Add video encoding support + FFmpeg CI fixesnSummary of changes:n1. Video Encoding Feature:n   - FFmpeg integration for video exportnn   - Setup vcpkg on all platforms (Linux/macOS/Windows)n   - Static linking on all platforms (x64-windows-static-md, arm64-osx-release, etc)nn   - Cache system updatesn   - UI refinementsnAll CI builds should now pass with FFmpeg auto-installation.n🤖 Generated with [Claude Code](https://claude.com/claude-code)nCo-Authored-By: Claude <noreply@anthropic.com>
- Add play range and video encoding infrastructure
- Implement FFmpeg video encoding
- Add integration test for video encoding
- Add MPEG-4 codec support to encoding dialog
- Improve encoder options and hardware encoder support
- Add RGB to YUV pixel format conversion for hardware encoders

### 🐛 Bug Fixes

- *(ci)* Add FFmpeg dependencies to CI pipeline
- *(ci)* Add pkg-config to all platforms - FFmpeg via vcpkg
- *(ci)* Install FFmpeg on all platforms via native package managers
- Encoder now respects play range (B/N markers)
- Set VCPKGRS_TRIPLET env for Windows cargo build

### 📚 Documentation

- Add video support implementation plan
- Add video encoding implementation plan
- Add comprehensive FFmpeg and video encoding documentation
- Update documentation for FFmpeg vcpkg and static linking
- Fix outdated commands in CONTRIBUTING.md

### ⚙️ Miscellaneous Tasks

- Initial commit - Playa v0.1.115
- WIP Fri 11/07/2025 - 23:21:02.30
- WIP Sat 11/08/2025 - 19:47:30.74
- Bump playa-ffmpeg to 8.0.2 with vcpkg auto-install
- Add 'test' command to bootstrap scripts
- WIP Sat 11/08/2025 - 23:02:21.23
- Remove AGENTS.md
- WIP Sun 11/09/2025 -  9:59:44.65
