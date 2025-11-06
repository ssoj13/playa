## [0.1.73-dev] - 2025-11-06

### 🚀 Features

- Add restore-keys for cache sharing between dev and main

### 🚜 Refactor

- Rename workflows and apply sequential build pattern to main.yml
## [0.1.72-dev] - 2025-11-06

### 🚜 Refactor

- Sequential builds with shared cache per platform
## [0.1.71-dev] - 2025-11-06

### 🐛 Bug Fixes

- Remove platform-specific stub functions in pre_build.rs
- Cache save only from dev branch for tag inheritance
## [0.1.70-dev] - 2025-11-06

### 🚀 Features

- Embed Reinhard and ACES tonemapping shaders
- Make external shaders directory optional

### ⚙️ Miscellaneous Tasks

- Remove tonemap shader files (now embedded in code)
## [0.1.69-dev] - 2025-11-06

### 🐛 Bug Fixes

- Unify cache keys between dev and main workflows
## [0.1.68-dev] - 2025-11-06

### 🐛 Bug Fixes

- Add set +H to workflows for certificate password handling
## [0.1.67-dev] - 2025-11-06

### 🐛 Bug Fixes

- Split cache restore/save in build.yml for proper tag inheritance

### ⚙️ Miscellaneous Tasks

- Add certificate export files to .gitignore
- Add dev branch trigger to build.yml for cache creation
- Only trigger build workflow on tags, save cache on dev tags
## [0.1.66-dev] - 2025-11-06

### 🚜 Refactor

- Rename export-cert.sh to apple_cert.sh and add set +H

### ⚙️ Miscellaneous Tasks

- Remove plan.md
## [0.1.65-dev] - 2025-11-06

### 📚 Documentation

- Add comprehensive certificate export script with instructions
## [0.1.64-dev] - 2025-11-06

### 🚀 Features

- Add platform-specific path management with configurable overrides
- Add macOS code signing support to CI/CD

### 🐛 Bug Fixes

- Change path priority to prefer local files when they exist

### 📚 Documentation

- Add configuration path management documentation
## [0.1.63-dev] - 2025-11-06

### 🚀 Features

- Add macOS support for OpenEXR backend

### 🐛 Bug Fixes

- Add dylib symlinks creation for macOS
- Correct dylib copy pattern for macOS binstall ZIP

### 📚 Documentation

- Update README with dual EXR backend release info

### ⚙️ Miscellaneous Tasks

- Bump version to 0.1.62-dev
## [0.1.61-dev] - 2025-11-06

### 🐛 Bug Fixes

- Remove build warnings for unused imports and dead code

### 📚 Documentation

- Add comprehensive dual EXR backend documentation to README

### ⚙️ Miscellaneous Tasks

- Add dual EXR backend support to CI/CD workflows
## [0.1.60-dev] - 2025-11-06

### 🚀 Features

- Add dual EXR backend support (exrs default, openexr optional)
- Add professional emoji-rich formatting to xtask help

### 🐛 Bug Fixes

- Update CI/CD workflows to use correct EXR backends and reduce log spam

### 📚 Documentation

- Add comprehensive workflow examples and xtask help documentation
## [0.1.59] - 2025-11-05

### 🐛 Bug Fixes

- Clean old installer artifacts before packaging
- Move installer cleanup before build to fix cached artifacts
## [0.1.58] - 2025-11-05

### 🐛 Bug Fixes

- Remove main branch trigger from Release workflow
## [0.1.57] - 2025-11-05

### 🐛 Bug Fixes

- Remove cancel-in-progress to prevent workflow race conditions
## [0.1.56] - 2025-11-05

### 🐛 Bug Fixes

- Use separate cache keys for release/dev with restore/save split
- Cache cargo-packager binary and skip redundant cache saves

### ⚙️ Miscellaneous Tasks

- WIP Wed 11/05/2025 - 11:07:34.02
- WIP Wed 11/05/2025 - 11:43:49.60
- Bug: Fix screenshot
- WIP Wed 11/05/2025 - 12:03:50.26
## [0.1.54] - 2025-11-05

### 🐛 Bug Fixes

- Enable cache saving on all builds for cross-tag reuse
- Cache all crates including build artifacts
- Replace rust-cache with actions/cache for complete artifact preservation

### ⚙️ Miscellaneous Tasks

- WIP Wed 11/05/2025 -  9:33:39.87
## [0.1.53] - 2025-11-05

### 🐛 Bug Fixes

- Save cache only from main branch to enable cross-tag reuse
## [0.1.52] - 2025-11-05

### 🐛 Bug Fixes

- Disable job-id in rust-cache key for cross-job cache reuse
## [0.1.48] - 2025-11-05

### 🐛 Bug Fixes

- Correct rust-cache parameter for version-independent caching
## [0.1.47] - 2025-11-05

### 🐛 Bug Fixes

- Enable cache reuse across versions with workspaces: false
## [0.1.46] - 2025-11-05

### 🐛 Bug Fixes

- Use rust target names for binstall ZIP files
## [0.1.44] - 2025-11-05

### ⚙️ Miscellaneous Tasks

- Bug: Codex Action changes
## [0.1.43] - 2025-11-05

### 🐛 Bug Fixes

- Fix rust-cache restoration in CI workflows

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 22:44:29.72
- Bug: Fix cache
## [0.1.42] - 2025-11-05

### 🚀 Features

- Add cargo-binstall support and fix xtask build flags
- Add cargo-binstall to bootstrap scripts

### 🚜 Refactor

- Simplify xtask build flags and update CI
## [0.1.41] - 2025-11-05

### ⚙️ Miscellaneous Tasks

- Bug: Fix release badge in README.md
## [0.1.40] - 2025-11-05

### ⚙️ Miscellaneous Tasks

- Bug: Fix xtask to build release
## [0.1.39-dev] - 2025-11-05

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 21:27:00.52
## [0.1.38] - 2025-11-05

### 🚀 Features

- Prepare Playa for crates.io publication

### ⚙️ Miscellaneous Tasks

- WIP Tue 11/04/2025 - 19:39:35.36
## [0.1.37] - 2025-11-05

### 🐛 Bug Fixes

- Change status badge to Build workflow (more frequently updated)
- Revert badge to Release workflow
## [0.1.35] - 2025-11-05

### 🚜 Refactor

- Simplify CI caching - remove cargo-install, keep only rust-cache
## [0.1.34] - 2025-11-05

### 🐛 Bug Fixes

- Remove duplicate workflow trigger from release.yml
## [0.1.30-dev] - 2025-11-05

### 🐛 Bug Fixes

- Use --yes instead of --no-confirm (more readable)
- Revert to --no-confirm (correct cargo-release flag)
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
