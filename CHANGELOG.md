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

### 📚 Documentation

- Update documentation for FFmpeg vcpkg and static linking
- Fix outdated commands in CONTRIBUTING.md

### ⚙️ Miscellaneous Tasks

- WIP Sun 11/09/2025 -  9:59:44.65
## [0.1.123] - 2025-11-09

### 🐛 Bug Fixes

- Set VCPKGRS_TRIPLET env for Windows cargo build
## [0.1.122] - 2025-11-09

### 🚀 Features

- *(ci)* Setup vcpkg for FFmpeg auto-install by playa-ffmpeg
- Add video encoding support + FFmpeg CI fixesnSummary of changes:n1. Video Encoding Feature:n   - FFmpeg integration for video exportnn   - Setup vcpkg on all platforms (Linux/macOS/Windows)n   - Static linking on all platforms (x64-windows-static-md, arm64-osx-release, etc)nn   - Cache system updatesn   - UI refinementsnAll CI builds should now pass with FFmpeg auto-installation.n🤖 Generated with [Claude Code](https://claude.com/claude-code)nCo-Authored-By: Claude <noreply@anthropic.com>
- Add play range and video encoding infrastructure
- Implement FFmpeg video encoding
- Add integration test for video encoding
- Add MPEG-4 codec support to encoding dialog
- Improve encoder options and hardware encoder support
- Add RGB to YUV pixel format conversion for hardware encoders

### 🐛 Bug Fixes

- *(ci)* Install FFmpeg on all platforms via native package managers
- Encoder now respects play range (B/N markers)

### 📚 Documentation

- Add video encoding implementation plan
- Add comprehensive FFmpeg and video encoding documentation

### ⚙️ Miscellaneous Tasks

- Bump playa-ffmpeg to 8.0.2 with vcpkg auto-install
- Add 'test' command to bootstrap scripts
- WIP Sat 11/08/2025 - 23:02:21.23
- Remove AGENTS.md
## [0.1.118-dev] - 2025-11-09

### 🐛 Bug Fixes

- *(ci)* Add FFmpeg dependencies to CI pipeline
- *(ci)* Add pkg-config to all platforms - FFmpeg via vcpkg

### ⚙️ Miscellaneous Tasks

- WIP Sat 11/08/2025 - 19:47:30.74
## [0.1.117-dev] - 2025-11-09

### 🚀 Features

- Add video playback support (.mp4, .mov, .avi, .mkv)

### 📚 Documentation

- Add video support implementation plan
## [0.1.116] - 2025-11-08

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 - 23:21:02.30
## [0.1.115] - 2025-11-08

### ⚙️ Miscellaneous Tasks

- Initial commit - Playa v0.1.115
