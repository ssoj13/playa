#[cfg(any(target_os = "linux", target_os = "macos"))]
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

/// Patch zlib for macOS compatibility with CMake 4.x
///
/// On macOS, the bundled zlib in openexr-sys 0.10.1 has two issues:
/// 1. CMakeLists.txt requires CMake 2.4.4, but CMake 4.x requires minimum 3.5
/// 2. zutil.h redefines fdopen as NULL, conflicting with macOS SDK headers
///
/// This function locates the openexr-sys crate in cargo registry
/// and patches both files.
#[cfg(target_os = "macos")]
pub fn patch_zlib_for_macos() -> Result<()> {
    use anyhow::Context;
    use std::path::PathBuf;

    println!("Patching zlib for macOS CMake 4.x compatibility...");

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

    // Patch CMakeLists.txt
    let cmake_file = openexr_sys_dir.join("thirdparty/zlib/CMakeLists.txt");
    if !cmake_file.exists() {
        anyhow::bail!(
            "zlib CMakeLists.txt not found at {}",
            cmake_file.display()
        );
    }

    let cmake_patched = patch_cmake_file(&cmake_file)?;

    // Patch zutil.h
    let zutil_file = openexr_sys_dir.join("thirdparty/zlib/zutil.h");
    if !zutil_file.exists() {
        anyhow::bail!("zutil.h not found at {}", zutil_file.display());
    }

    let zutil_patched = patch_zutil_file(&zutil_file)?;

    println!();
    println!("Zlib patching complete:");
    println!("  - CMakeLists.txt: {}", if cmake_patched { "patched" } else { "already patched" });
    println!("  - zutil.h: {}", if zutil_patched { "patched" } else { "already patched" });

    Ok(())
}

/// Patch CMakeLists.txt to require CMake 3.5 instead of 2.4.4
#[cfg(target_os = "macos")]
fn patch_cmake_file(path: &std::path::Path) -> Result<bool> {
    use anyhow::Context;
    use std::fs;

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    // Check if already patched
    if content.contains("cmake_minimum_required(VERSION 3.5)") {
        return Ok(false);
    }

    // Replace version requirement
    let new_content = content.replace(
        "cmake_minimum_required(VERSION 2.4.4)",
        "cmake_minimum_required(VERSION 3.5)"
    );

    if new_content == content {
        anyhow::bail!("Could not find cmake_minimum_required(VERSION 2.4.4) in CMakeLists.txt");
    }

    fs::write(path, new_content)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(true)
}

/// Patch zutil.h to skip fdopen redefinition on macOS
#[cfg(target_os = "macos")]
fn patch_zutil_file(path: &std::path::Path) -> Result<bool> {
    use anyhow::Context;
    use std::fs;

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    // Check if already patched
    if content.contains("#      ifndef __APPLE__") {
        return Ok(false);
    }

    // Find and replace the fdopen section
    let old_section = "#    else\n#      ifndef fdopen\n#        define fdopen(fd,mode) NULL /* No fdopen() */\n#      endif\n#    endif";
    let new_section = "#    else\n#      ifndef __APPLE__\n#        ifndef fdopen\n#          define fdopen(fd,mode) NULL /* No fdopen() */\n#        endif\n#      endif\n#    endif";

    let new_content = content.replace(old_section, new_section);

    if new_content == content {
        anyhow::bail!("Could not find fdopen section in zutil.h");
    }

    fs::write(path, new_content)
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(true)
}
