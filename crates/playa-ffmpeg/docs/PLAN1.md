# CI/CD Fix Plan - November 8, 2025

## Problem Summary

All CI/CD jobs failing since commit `729552b` ("Rust 2024 / ffmpeg 8.0 / Windows build fixes")

## Root Cause Analysis

### Primary Issue: Incomplete Rust 2024 Migration

Commit `729552b` performed massive refactoring (125 files):
- Changed `edition = "2015"` → `edition = "2024"`
- Mass replacement `use ffi::*` → `use crate::ffi::*`

**MISSED LINE** in `src/software/scaling/flag.rs:4`:

```rust
#[cfg(feature = "ffmpeg_8_0")]
use software::scaling::SwsFlags::*;  // ❌ Old import style
```

Should be:
```rust
#[cfg(feature = "ffmpeg_8_0")]
use crate::software::scaling::SwsFlags::*;  // ✅ Rust 2024
```

### Failure Details

**macOS / Windows**: Build fails
```
error[E0433]: failed to resolve: use of unresolved module or unlinked crate `software`
 --> src/software/scaling/flag.rs:4:5
  |
4 | use software::scaling::SwsFlags::*;
  |     ^^^^^^^^ use of unresolved module or unlinked crate `software`
```

**Linux FFmpeg 6.1+**: Clippy lint fails
```
error: `crate` references the macro call's crate
  --> src/util/dictionary/mod.rs:16:19
   |
16 |             let mut dict = crate::ffmpeg::Dictionary::new();
   |                            ^^^^^ help: to reference the macro definition's crate, use: `$crate`
```

## Why Not Detected Earlier

1. CI wasn't run on commit `729552b` (no runs on master branch)
2. Error only manifests with `ffmpeg_8_0` feature enabled
3. My commits were first to trigger CI after refactoring

## Fix Checklist

- [ ] Fix `src/software/scaling/flag.rs` - missing `crate::` prefix
- [ ] Fix `src/util/dictionary/mod.rs` - clippy macro lint
- [ ] Add `nasm` to Linux setup script (user request)
- [ ] Test locally with `cargo build --examples`
- [ ] Test with `cargo clippy --examples -- -D warnings`
- [ ] Commit and push to dev branch
- [ ] Monitor CI runs to confirm fixes

## Fixes

### 1. src/software/scaling/flag.rs

```diff
 #[cfg(feature = "ffmpeg_8_0")]
-use software::scaling::SwsFlags::*;
+use crate::software::scaling::SwsFlags::*;
```

### 2. src/util/dictionary/mod.rs

```diff
-            let mut dict = crate::ffmpeg::Dictionary::new();
+            let mut dict = $crate::ffmpeg::Dictionary::new();
```

### 3. setup-vcpkg.sh - Add nasm

Add `nasm` to package installation for Linux distributions:
- Ubuntu/Debian: `nasm`
- Fedora/RHEL: `nasm`
- Arch/Manjaro: `nasm`

## Testing

```powershell
# Build examples
cargo build --examples --release

# Run clippy
cargo clippy --examples -- -D warnings

# Run tests
cargo test --examples
```

## Expected Outcome

- All CI jobs pass (macOS, Windows, Linux)
- No clippy warnings
- video-info example builds successfully with frame dumping feature
