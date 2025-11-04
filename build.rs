/// Build script for playa
///
/// This build.rs is intentionally minimal. Native dependency management
/// has been moved to cargo xtask for better control and reliability.
///
/// To build the project with all dependencies:
///   cargo xtask build [--release]
///
/// This will:
/// 1. Patch OpenEXR headers (Linux only)
/// 2. Run cargo build
/// 3. Copy all native libraries and shaders
///
/// See xtask/ directory for implementation details.
fn main() {
    // Only rerun if build.rs itself changes
    println!("cargo:rerun-if-changed=build.rs");

    // Note: Native library copying is now handled by cargo xtask post-build
    // This ensures libraries are copied after every build, not just on recompilation
}
