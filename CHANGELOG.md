## [0.1.115] - 2025-11-08

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 - 15:53:39.30
- WIP Fri 11/07/2025 - 16:01:14.39
- WIP Fri 11/07/2025 - 16:02:50.07
## [0.1.113] - 2025-11-07

### 🚀 Features

- Add macOS app notarization support
## [0.1.112] - 2025-11-07

### 🚀 Features

- Make release workflow wait for warm-cache completion
## [0.1.111] - 2025-11-07

### 🐛 Bug Fixes

- Make cache reusable across tags by reading from main
- Update code signing configuration and documentation
- Translate CI/CD workflow section to English

### 📚 Documentation

- Add comprehensive CI/CD workflow guide to README
## [0.1.110] - 2025-11-07

### 🐛 Bug Fixes

- Append temp keychain instead of replacing keychain list
## [0.1.108] - 2025-11-07

### 🐛 Bug Fixes

- Simplify cache to work across all refs
- Simplify cache strategy for reliable reuse between releases
- Remove non-existent shaders from packager resources
## [0.1.107] - 2025-11-07

### 🚀 Features

- Improve cache reuse and add code signing debug output
## [0.1.105] - 2025-11-07

### 📚 Documentation

- Add macOS code signing documentation and improve cert script
## [0.1.103] - 2025-11-07

### 🚀 Features

- Add detailed logging for macOS code signing
## [0.1.102] - 2025-11-07

### 🐛 Bug Fixes

- Update workflow badges to show push event status
- Enable macOS code signing with Developer ID
- Correct macOS code signing condition syntax
## [0.1.101] - 2025-11-07

### 🐛 Bug Fixes

- Check real cache existence instead of workflow runs
## [0.1.100] - 2025-11-07

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 -  2:08:57.37
## [0.1.98] - 2025-11-07

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 -  1:31:48.13
## [0.1.97] - 2025-11-07

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 -  1:18:18.32
- WIP Fri 11/07/2025 -  1:21:51.53
## [0.1.96] - 2025-11-07

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 -  0:53:44.72
## [0.1.95] - 2025-11-07

### 💼 Other

- Add diagnostic messages for found/not-found installers and created ZIPs on all platforms
- Add release summaries to  for Windows/Linux/macOS (list found/not-found assets)
- Add 'wipe-wf' to delete all GitHub Actions workflow runs via gh; no flags; prints progress

### ⚙️ Miscellaneous Tasks

- WIP Fri 11/07/2025 -  0:10:22.11
## [0.1.94] - 2025-11-07

### 💼 Other

- Unify Linux (.AppImage/.deb) and macOS (.dmg) artifact names to playa-<backend>-<triple>.<ext>; keep Windows unified as well
- Group platform installs at top; remove duplicate old-installer cleanup; keep wipe once after cache restore; drop separate NSIS/WiX install steps
- Remove 'Clean previous backend binaries' steps; make Linux packaging conditional (LD_LIBRARY_PATH only for openexr, exrs without it)
## [0.1.91] - 2025-11-07

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 22:06:32.20
## [0.1.89] - 2025-11-07

### 💼 Other

- Move 'cargo xtask wipe' step after both cache restores (target + cargo-packager) to avoid confusion; keep NSIS install adjacent
## [0.1.88] - 2025-11-07

### 💼 Other

- Add 'wipe' command to remove executables and shared libraries from ./target (non-recursive) with clear logging
- Remove accidental dump file
- Mention 'xtask wipe' in help; CI: run 'cargo xtask wipe' after cache restore and install NSIS on Windows to produce .exe installer; docs: add wipe to README usage

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 19:36:07.45
## [0.1.87] - 2025-11-07

### 💼 Other

- Recompute AutoFit on image load; UI: render bottom panels before central viewport so Fit uses visible area and image isn’t hidden behind toolbars
- Remove temporary disabled duplicate blocks after viewport (now rendered before viewport)
- Remove explanatory comments around reordered panels
## [0.1.83] - 2025-11-06

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 15:18:54.04
## [0.1.82] - 2025-11-06

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 13:57:33.32
- WIP Thu 11/06/2025 - 14:01:16.68
## [0.1.81] - 2025-11-06

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 12:21:39.83
- WIP Thu 11/06/2025 - 12:26:52.43
- WIP Thu 11/06/2025 - 12:33:27.79
## [0.1.80] - 2025-11-06

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 - 11:47:01.34
- WIP Thu 11/06/2025 - 11:59:17.29
## [0.1.79] - 2025-11-06

### 🚜 Refactor

- Extract reusable workflows to eliminate duplication
## [0.1.78] - 2025-11-06

### 🐛 Bug Fixes

- Add restore-keys to enable cache reuse between tags
## [0.1.77] - 2025-11-06

### 🐛 Bug Fixes

- Remove branch/event filters from Release Status badge
## [0.1.76] - 2025-11-06

### 🐛 Bug Fixes

- Revert to unified cache keys without dev/main suffixes
## [0.1.75] - 2025-11-06

### 🚜 Refactor

- Remove redundant check-branch dependencies from openexr jobs
## [0.1.73-dev] - 2025-11-06

### 🚀 Features

- Add restore-keys for cache sharing between dev and main

### 🚜 Refactor

- Rewrite main.yml using dev.yml pattern
## [0.1.74] - 2025-11-06

### ⚙️ Miscellaneous Tasks

- WIP Thu 11/06/2025 -  1:18:12.97
## [0.1.73] - 2025-11-06

### 🚀 Features

- Change changelog command to full regeneration

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

### ⚙️ Miscellaneous Tasks

- Bump version to 0.1.62-dev
## [0.1.62] - 2025-11-06

### 📚 Documentation

- Update README with dual EXR backend release info
## [0.1.61-dev] - 2025-11-06

### 🐛 Bug Fixes

- Remove build warnings for unused imports and dead code

### ⚙️ Miscellaneous Tasks

- Add dual EXR backend support to CI/CD workflows
## [0.1.61] - 2025-11-06

### 🐛 Bug Fixes

- Remove build warnings for unused imports and dead code

### 📚 Documentation

- Add comprehensive dual EXR backend documentation to README
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
