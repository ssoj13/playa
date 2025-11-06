# Plan: Add macOS OpenEXR Support

## Current Status
- Windows: ✅ OpenEXR works (both backends: exrs, openexr)
- Linux: ✅ OpenEXR works (both backends: exrs, openexr) - patches OpenEXR headers for GCC 11+
- macOS: ❌ OpenEXR fails to build (zlib incompatibility with CMake 4.x)

## Problem on macOS
1. **CMake 4.x incompatibility**: Bundled zlib in openexr-sys 0.10.1 requires `cmake_minimum_required(VERSION 2.4.4)`, but CMake 4.x requires minimum 3.5
2. **fdopen macro conflict**: Old zlib redefines `fdopen` as `NULL`, conflicts with modern macOS SDK headers

## Solution
Add automatic patching in `xtask` (similar to existing Linux header patching):
- Patch `thirdparty/zlib/CMakeLists.txt`: `VERSION 2.4.4` → `VERSION 3.5`
- Patch `thirdparty/zlib/zutil.h`: Wrap `fdopen` macro in `#ifndef __APPLE__`

## Implementation Tasks

### 1. Add macOS Patching to xtask/src/pre_build.rs
- [ ] Add `#[cfg(target_os = "macos")]` section
- [ ] Implement `patch_zlib_for_macos()` function
  - [ ] Find openexr-sys in cargo registry (glob pattern)
  - [ ] Patch `thirdparty/zlib/CMakeLists.txt`: change cmake_minimum_required to 3.5
  - [ ] Patch `thirdparty/zlib/zutil.h`: add `#ifndef __APPLE__` around fdopen macro (lines 139-143)
  - [ ] Handle already-patched files (idempotent)
  - [ ] Pretty-print progress similar to Linux header patching
- [ ] Update no-op fallback to exclude macOS from "not needed" message

### 2. Update xtask/src/main.rs Build Flow
- [ ] In `cmd_build()`: add macOS pre-build step for OpenEXR
  - [ ] Add `#[cfg(target_os = "macos")]` condition
  - [ ] Call `pre_build::patch_zlib_for_macos()` when `openexr == true`
  - [ ] Update step numbering (1/3, 2/3, 3/3 for macOS with OpenEXR)
- [ ] Update help text/comments to mention macOS support

### 3. Enable macOS in .github/workflows/build.yml
- [ ] Uncomment and update `build-macos` job
- [ ] Add matrix strategy with backends: [exrs, openexr]
- [ ] Install dependencies:
  - [ ] CMake: `brew install cmake`
- [ ] Set runner: `macos-latest` (Apple Silicon) or add both architectures
- [ ] Build with xtask:
  - [ ] `cargo xtask build` for exrs backend
  - [ ] `cargo xtask build --openexr` for openexr backend
- [ ] Package with cargo-packager
- [ ] Create binstall ZIP: `playa-{backend}-{arch}-apple-darwin.zip`
- [ ] Find and rename DMG with backend suffix
- [ ] Upload artifacts:
  - [ ] macOS DMG (if exists)
  - [ ] macOS binstall ZIP
- [ ] Set retention-days: 7

### 4. Enable macOS in .github/workflows/release.yml
- [ ] Uncomment and update `build-macos` job (lines 175-241)
- [ ] Add matrix strategy with backends: [exrs, openexr]
- [ ] Install dependencies (same as build.yml)
- [ ] Add cache restore/save for cargo artifacts (like Windows/Linux)
- [ ] Build and package (same as build.yml)
- [ ] Upload to GitHub Release:
  - [ ] DMG installer
  - [ ] binstall ZIP
- [ ] Use `softprops/action-gh-release@v2` for uploading

### 5. Test Locally on macOS
- [ ] Clean build: `cargo clean`
- [ ] Test exrs backend: `cargo xtask build`
- [ ] Test OpenEXR backend: `cargo xtask build --openexr`
  - [ ] Verify zlib patches are applied automatically
  - [ ] Verify build succeeds
- [ ] Test packaging: `cargo packager --release`
  - [ ] Verify DMG is created
  - [ ] Verify shaders are included in app bundle

### 6. CI Testing
- [ ] Push to dev branch
- [ ] Create dev tag: `cargo xtask tag-dev patch`
- [ ] Verify Build workflow succeeds for macOS
- [ ] Download and test artifacts
- [ ] Merge to main when ready
- [ ] Create release tag: `cargo xtask tag-rel patch`
- [ ] Verify Release workflow creates GitHub Release with macOS installers

## File Changes Summary
- `xtask/src/pre_build.rs` - Add macOS zlib patching
- `xtask/src/main.rs` - Call macOS patching in build flow
- `.github/workflows/build.yml` - Uncomment and update macOS job
- `.github/workflows/release.yml` - Uncomment and update macOS job
- `plan.md` - This file (delete after completion)

## Notes
- Do NOT change openexr-sys version (must stay at 0.10.1)
- Patches are applied to cargo registry sources (idempotent, safe to re-run)
- macOS will support both backends like Windows/Linux
- Apple Silicon (aarch64) and Intel (x86_64) may need separate jobs
