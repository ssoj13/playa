## [0.1.29-dev] - 2025-11-05

### 🚀 Features

- Separate dev and release tagging scripts
- Consolidate all build commands into cargo xtask
- Add bootstrap scripts for dependency setup and xtask execution
- Consolidate all build commands into xtask, update README

### 🐛 Bug Fixes

- Use pre-release version format for dev tags (cargo-packager compatibility)
- Calculate version manually to support pre-release suffixes
- Add --no-confirm flag to cargo-release

### 🚜 Refactor

- Remove obsolete release command, rename tag-release to tag-rel

### 📚 Documentation

- Improve xtask command descriptions - more clear and actionable

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 15:44:46.53
- Disable dependabot - creates merge conflicts
- WIP Tue 11/04/2025 - 16:33:44.88
## [0.1.28] - 2025-11-04

### ⚙️ Miscellaneous Tasks

- Separate build/release workflows by branch
## [0.1.27] - 2025-11-04

### ⚙️ Miscellaneous Tasks

- Remove sccache and revert to cargo install for cargo-packager
- Revert to baptiste0928/cargo-install with proper caching order
- Invalidate rust-cache to fix OpenEXR build issues
- WIP Tue 11/04/2025 - 15:06:32.28
- Build workflow triggers only on tags, not every commit
- WIP Tue 11/04/2025 - 15:09:37.16
- Update GitHub Actions versions and invalidate cache keys
## [0.1.26] - 2025-11-04

### 🚜 Refactor

- Redesign release process with two-stage dev/main workflow

### ⚡ Performance

- Replace cargo-install with cargo-binstall for faster builds

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 13:40:38.93
- WIP Tue 11/04/2025 - 13:41:05.84
- Separate build and release workflows for dev/main branches
- WIP Tue 11/04/2025 - 14:23:54.75
## [0.1.25] - 2025-11-04

### ⚙️ Miscellaneous Tasks

- Disable sccache on Windows due to GitHub Cache API instability
## [0.1.24] - 2025-11-04

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 13:13:14.93
## [0.1.23] - 2025-11-04

### 🐛 Bug Fixes

- Add sccache connectivity test with automatic fallback
## [0.1.22] - 2025-11-04

### 🐛 Bug Fixes

- Improve GitHub Actions caching and reliability

### 📚 Documentation

- Add comprehensive GitHub Actions CI/CD documentation
- Document CARGO_INCREMENTAL=0 requirement for sccache
## [0.1.18] - 2025-11-04

### 🐛 Bug Fixes

- GitHub Actions workflow and badges
- Replace tokei.rs badge with ghloc for LOC count

### 📚 Documentation

- Remove marketing fluff from README
## [0.1.16] - 2025-11-04

### 🐛 Bug Fixes

- Add cargo fetch before Linux build in CI
## [0.1.15] - 2025-11-04
