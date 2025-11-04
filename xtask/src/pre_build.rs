use anyhow::Result;

#[cfg(target_os = "linux")]
use anyhow::Context;

#[cfg(target_os = "linux")]
use std::fs;

#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

/// Files that need patching in OpenEXR headers for GCC 11+ compatibility
#[cfg(target_os = "linux")]
const HEADERS_TO_PATCH: &[&str] = &[
    "ImfTiledMisc.h",
    "ImfDeepTiledInputFile.h",
    "ImfDeepTiledInputPart.h",
];

/// The include statement to add
#[cfg(target_os = "linux")]
const INCLUDE_TO_ADD: &str = "#include <cstdint>";

/// Marker to check if already patched
#[cfg(target_os = "linux")]
const PATCH_MARKER: &str = "cstdint";

/// Patch OpenEXR headers for GCC 11+ compatibility
///
/// On Linux, OpenEXR 3.0.5 headers are missing #include <cstdint>
/// which causes compilation errors with GCC 11+.
///
/// This function locates the openexr-sys crate in cargo registry
/// and patches the required headers.
#[cfg(target_os = "linux")]
pub fn patch_headers() -> Result<()> {
    println!("Patching OpenEXR headers for GCC 11+ compatibility...");

    // Find openexr-sys in cargo registry
    let cargo_home = std::env::var("CARGO_HOME")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{}/.cargo", h)))
        .context("Could not determine CARGO_HOME")?;

    let registry_src = PathBuf::from(cargo_home).join("registry/src");

    if !registry_src.exists() {
        println!("Cargo registry not found. Running cargo fetch...");
        std::process::Command::new("cargo")
            .arg("fetch")
            .status()
            .context("Failed to run cargo fetch")?;
    }

    // Find openexr-sys directory (glob pattern to handle different registry indices)
    let openexr_sys_pattern = format!("{}/*/openexr-sys-*", registry_src.display());
    let openexr_sys_dirs = glob::glob(&openexr_sys_pattern)
        .context("Failed to glob for openexr-sys")?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    if openexr_sys_dirs.is_empty() {
        anyhow::bail!(
            "Could not find openexr-sys in cargo registry. Try running 'cargo fetch' first."
        );
    }

    // Use the first found directory (there should only be one version)
    let openexr_sys_dir = &openexr_sys_dirs[0];
    println!("Found openexr-sys at: {}", openexr_sys_dir.display());

    // Find OpenEXR headers directory
    let headers_dir = openexr_sys_dir
        .join("thirdparty/openexr/src/lib/OpenEXR");

    if !headers_dir.exists() {
        anyhow::bail!(
            "OpenEXR headers directory not found at {}",
            headers_dir.display()
        );
    }

    // Patch each header file
    let mut patched_count = 0;
    let mut already_patched_count = 0;

    for header_name in HEADERS_TO_PATCH {
        let header_path = headers_dir.join(header_name);

        if !header_path.exists() {
            println!("  Warning: {} not found, skipping", header_name);
            continue;
        }

        match patch_header_file(&header_path)? {
            PatchResult::Patched => {
                println!("  ✓ Patched {}", header_name);
                patched_count += 1;
            }
            PatchResult::AlreadyPatched => {
                println!("  - {} already patched", header_name);
                already_patched_count += 1;
            }
        }
    }

    println!();
    println!("Header patching complete:");
    println!("  - Patched: {}", patched_count);
    println!("  - Already patched: {}", already_patched_count);

    Ok(())
}

/// Result of patching a single header file
#[cfg(target_os = "linux")]
enum PatchResult {
    Patched,
    AlreadyPatched,
}

/// Patch a single header file
#[cfg(target_os = "linux")]
fn patch_header_file(path: &Path) -> Result<PatchResult> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    // Check if already patched
    if content.contains(PATCH_MARKER) {
        return Ok(PatchResult::AlreadyPatched);
    }

    // Find the first #include and insert our include after it
    let mut lines: Vec<&str> = content.lines().collect();
    let mut insert_index = None;

    for (i, line) in lines.iter().enumerate() {
        if line.trim().starts_with("#include") {
            insert_index = Some(i + 1);
            break;
        }
    }

    let insert_index = insert_index
        .context("Could not find any #include in header file")?;

    // Insert the new include
    lines.insert(insert_index, INCLUDE_TO_ADD);

    // Write back
    let new_content = lines.join("\n") + "\n";
    fs::write(path, new_content)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(PatchResult::Patched)
}

/// No-op on non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub fn patch_headers() -> Result<()> {
    println!("Header patching not needed on this platform");
    Ok(())
}
