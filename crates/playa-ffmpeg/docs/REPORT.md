# Rust 2024 Edition Migration Report

## Summary
Fixed all compilation warnings to comply with Rust 2024 edition requirements. The project now builds cleanly without warnings.

## Problem
Rust 2024 edition requires explicit `unsafe {}` blocks inside unsafe functions for all unsafe operations. Previously, the compiler allowed unsafe operations directly within unsafe function bodies.

## Changes Made
Added `unsafe {}` blocks around unsafe operations in **14 files**:

### Dictionary Module
- `src/util/dictionary/mutable.rs` - wrapped `immutable::Ref::wrap()`
- `src/util/dictionary/owned.rs` - wrapped `mutable::Ref::wrap()` and `as_mut_ptr()`

### Frame Module
- `src/util/frame/video.rs` - wrapped `Frame::wrap()` and `av_frame_get_buffer()`
- `src/util/frame/audio.rs` - same for audio frames
- `src/util/frame/mod.rs` - wrapped `av_frame_alloc()` and pointer dereferences

### Format/Stream Module
- `src/format/stream/stream.rs` - wrapped pointer arithmetic and dereferences
- `src/format/stream/stream_mut.rs` - wrapped `mem::transmute_copy()` and `Stream::wrap()`

### Format/Chapter Module
- `src/format/chapter/chapter.rs` - wrapped pointer operations
- `src/format/chapter/chapter_mut.rs` - wrapped `transmute_copy()` and pointer operations

### Format/Context Module
- `src/format/context/input.rs` - wrapped `Context::wrap()`
- `src/format/context/output.rs` - wrapped `Context::wrap()`
- `src/format/context/common.rs` - wrapped `Destructor::new()`

### Codec/Subtitle Module
- `src/codec/subtitle/rect.rs` - wrapped all `wrap()` and `as_ptr()` calls
- `src/codec/subtitle/rect_mut.rs` - wrapped all unsafe operations in variants (Bitmap, Text, Ass)

### Device Module
- `src/device/extensions.rs` - wrapped `avdevice_list_devices()`

## Results
- **Before**: 43+ warnings (E0133: unsafe operation in unsafe function)
- **After**: 0 warnings
- **Build status**: ✅ Clean release build

## Technical Details
All changes follow the pattern:
```rust
// Before (Rust 2021)
pub unsafe fn func() {
    unsafe_operation()
}

// After (Rust 2024)
pub unsafe fn func() {
    unsafe { unsafe_operation() }
}
```

This ensures that each unsafe operation is explicitly marked, improving code safety and maintainability while maintaining the same functionality.
