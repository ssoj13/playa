## [0.1.119-dev] - 2025-11-09

### 🚀 Features

- *(ci)* Setup vcpkg for FFmpeg auto-install by playa-ffmpeg
- Add video encoding support + FFmpeg CI fixesnSummary of changes:n1. Video Encoding Feature:n   - FFmpeg integration for video exportnn   - Setup vcpkg on all platforms (Linux/macOS/Windows)n   - Static linking on all platforms (x64-windows-static-md, arm64-osx-release, etc)nn   - Cache system updatesn   - UI refinementsnAll CI builds should now pass with FFmpeg auto-installation.n🤖 Generated with [Claude Code](https://claude.com/claude-code)nCo-Authored-By: Claude <noreply@anthropic.com>
- Add play range and video encoding infrastructure
- Implement FFmpeg video encoding
- Add integration test for video encoding
- Add MPEG-4 codec support to encoding dialog
- Improve encoder options and hardware encoder support
- Add RGB to YUV pixel format conversion for hardware encoders
- Add ProRes profile quality mapping and improve encoder settings
- Add static FFmpeg linking via vcpkg in CI

### 🐛 Bug Fixes

- *(ci)* Install FFmpeg on all platforms via native package managers
- Encoder now respects play range (B/N markers)
- Trigger cache preload when adding sequences
- Prevent cache corruption and clear errors on successful load
- Enable MP4 seeking with movflags=faststart
- Enable static FFmpeg linking for cross-platform builds

### 🚜 Refactor

- Apply Clippy performance and style improvements

### 📚 Documentation

- Add video encoding implementation plan
- Add comprehensive FFmpeg and video encoding documentation

### ⚡ Performance

- Major performance and correctness improvements

### ⚙️ Miscellaneous Tasks

- Bump playa-ffmpeg to 8.0.2 with vcpkg auto-install
- Add 'test' command to bootstrap scripts
- WIP Sat 11/08/2025 - 23:02:21.23
- Bump playa-ffmpeg to 8.0.3
- WIP Sun 11/09/2025 -  1:00:51.76
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
