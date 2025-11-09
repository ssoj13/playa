## [Unreleased]

### 🚀 Features

- **Static FFmpeg linking** across all platforms for portable, self-contained binaries
- **FFmpeg vcpkg caching** reduces CI build time from ~20 minutes to ~30 seconds

### 🐛 Bug Fixes

- **Windows builds** now properly use custom vcpkg triplet `x64-windows-static-md-release`
- Set `VCPKGRS_TRIPLET` environment variable for Windows cargo builds to ensure correct triplet detection
- Create custom vcpkg triplets **before** cache check to ensure proper FFmpeg library discovery
- Fixed vcpkg paths on Unix systems to use system location `/usr/local/share/vcpkg`

### 🔧 Improvements

- **Custom vcpkg triplets** for optimized CI builds:
  - Windows: `x64-windows-static-md-release` (static libraries + dynamic CRT)
  - macOS: `arm64-osx-release` / `x64-osx-release` (release-only builds)
  - Linux: `x64-linux-release` (release-only configuration)
- Optimized FFmpeg installation workflow with proper triplet creation order
- Added comprehensive documentation for FFmpeg setup and static linking strategy

### 📚 Documentation

- Updated Windows FFmpeg setup instructions with `VCPKGRS_TRIPLET` requirement
- Added CI/CD static linking strategy documentation
- Expanded video encoding features documentation (play range, resolution requirements)
- Added development environment setup guide for contributors

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
