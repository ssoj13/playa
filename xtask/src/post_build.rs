use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::lib_discovery;

/// Copy native libraries and resources after build
///
/// This function:
/// 1. Finds all required libraries using lib_discovery
/// 2. Copies them to target/{profile}/
/// 3. Creates soname symlinks on Linux
/// 4. Copies shaders/ directory
/// 5. Verifies all dependencies present
pub fn copy_dependencies(profile: &str) -> Result<()> {
    println!("========================================");
    println!("Copying dependencies for profile: {}", profile);
    println!("========================================");
    println!();

    let target_dir = PathBuf::from(format!("target/{}", profile));

    // Step 1: Find libraries
    let libraries = lib_discovery::find_libraries(profile)?;

    if libraries.is_empty() {
        anyhow::bail!(
            "No libraries found! Build might have failed or libraries are in unexpected location."
        );
    }

    println!();

    // Step 2: Copy libraries
    println!("Copying libraries to {}...", target_dir.display());
    let mut copied_count = 0;

    for lib_path in &libraries {
        if let Some(file_name) = lib_path.file_name() {
            let dest_path = target_dir.join(file_name);

            match fs::copy(lib_path, &dest_path) {
                Ok(bytes) => {
                    println!("  ✓ Copied {} ({} bytes)", file_name.to_string_lossy(), bytes);
                    copied_count += 1;
                }
                Err(e) => {
                    println!("  ✗ Failed to copy {}: {}", file_name.to_string_lossy(), e);
                }
            }
        }
    }

    println!();
    println!("Copied {} libraries", copied_count);

    // Step 3: Create soname symlinks on Linux
    #[cfg(target_os = "linux")]
    {
        println!();
        create_soname_symlinks(&target_dir, &libraries)?;
    }

    // Step 4: Copy shaders directory
    println!();
    copy_shaders(&target_dir)?;

    // Step 5: Verify
    println!();
    lib_discovery::verify_library_count(&libraries)?;

    println!();
    println!("========================================");
    println!("Dependencies copied successfully!");
    println!("========================================");

    Ok(())
}

/// Copy shaders directory to target
fn copy_shaders(target_dir: &Path) -> Result<()> {
    let shaders_src = PathBuf::from("shaders");

    if !shaders_src.exists() {
        println!("Warning: shaders/ directory not found, skipping");
        return Ok(());
    }

    let shaders_dest = target_dir.join("shaders");

    println!("Copying shaders/ directory...");

    // Remove existing shaders directory if it exists
    if shaders_dest.exists() {
        fs::remove_dir_all(&shaders_dest)
            .context("Failed to remove existing shaders directory")?;
    }

    // Copy shaders directory
    fs_extra::dir::copy(
        &shaders_src,
        &target_dir,
        &fs_extra::dir::CopyOptions::new(),
    )
    .context("Failed to copy shaders directory")?;

    println!("  ✓ Copied shaders/ directory");

    Ok(())
}

/// Create soname symlinks for shared libraries on Linux
///
/// For example, libFoo.so.1.2.3 -> create libFoo.so.1
/// This makes packaging easier and follows Linux conventions
#[cfg(target_os = "linux")]
fn create_soname_symlinks(target_dir: &Path, libraries: &[PathBuf]) -> Result<()> {
    println!("Creating soname symlinks...");

    let mut symlink_count = 0;

    for lib_path in libraries {
        if let Some(file_name) = lib_path.file_name().and_then(|n| n.to_str()) {
            // Only process .so files with version numbers
            if !file_name.contains(".so.") {
                continue;
            }

            // Extract soname by removing the last version component
            // e.g., libFoo.so.29.0.0 -> libFoo.so.29
            if let Some(soname) = extract_soname(file_name) {
                let link = target_dir.join(&soname);

                // Skip if symlink already exists and points to the right file
                if link.exists() {
                    if let Ok(target) = fs::read_link(&link) {
                        if target == PathBuf::from(file_name) {
                            continue;
                        }
                    }
                    // Remove existing symlink if it points elsewhere
                    let _ = fs::remove_file(&link);
                }

                match std::os::unix::fs::symlink(file_name, &link) {
                    Ok(_) => {
                        println!("  ✓ Created symlink {} -> {}", soname, file_name);
                        symlink_count += 1;
                    }
                    Err(e) => {
                        println!("  ✗ Failed to create symlink {}: {}", soname, e);
                    }
                }
            }
        }
    }

    println!("Created {} soname symlinks", symlink_count);

    Ok(())
}

/// Extract soname from full library name
///
/// Examples:
/// - libFoo.so.29.0.0 -> Some("libFoo.so.29")
/// - libBar.so.28.0.2 -> Some("libBar.so.28")
/// - libBaz.so.1.2.11 -> Some("libBaz.so.1")
#[cfg(target_os = "linux")]
fn extract_soname(filename: &str) -> Option<String> {
    // Find .so. in the filename
    let so_pos = filename.find(".so.")?;

    // Get everything after .so.
    let version_part = &filename[so_pos + 4..];

    // Split by dots
    let version_components: Vec<&str> = version_part.split('.').collect();

    if version_components.is_empty() {
        return None;
    }

    // Take the base name + .so. + first version component
    // e.g., libFoo + .so. + 29
    Some(format!(
        "{}.so.{}",
        &filename[..so_pos],
        version_components[0]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_extract_soname() {
        assert_eq!(
            extract_soname("libOpenEXR-3_0.so.29.0.0"),
            Some("libOpenEXR-3_0.so.29".to_string())
        );

        assert_eq!(
            extract_soname("libImath-3_0.so.28.0.2"),
            Some("libImath-3_0.so.28".to_string())
        );

        assert_eq!(
            extract_soname("libz.so.1.2.11"),
            Some("libz.so.1".to_string())
        );

        assert_eq!(extract_soname("libfoo.so"), None);
        assert_eq!(extract_soname("libbar.dll"), None);
    }
}
