//! Minimal Cargo build script — only notifies Cargo when **`build.rs`** itself changes.
//!
//! FFmpeg (vcpkg → **`playa-ffmpeg`**), EXR (**`playa-io`** / **`vfx-exr`**), and other natives are
//! configured through normal crate dependencies — see **`DEVELOP.md`**. Maintainer automation lives in
//! **`crates/xtask`** (`cargo xtask …`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
