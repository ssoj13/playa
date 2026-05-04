//! Native link hints — FFmpeg's static `avdevice` on MSVC may reference Video for Windows
//! (`capCreateCaptureWindowA`), which lives in **vfw32**. The root `playa` binary historically
//! satisfied this via the full dependency graph; `cargo test -p playa-engine` builds a separate
//! test harness that needs the same system libs explicit.

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "windows" {
        println!("cargo:rustc-link-lib=vfw32");
    }
}
