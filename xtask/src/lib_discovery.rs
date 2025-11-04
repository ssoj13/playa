use anyhow::{Context, Result};
use glob::{glob_with, MatchOptions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Platform-specific glob patterns for finding native libraries
#[derive(Debug, Clone)]
pub struct PlatformPatterns {
    pub windows: Vec<&'static str>,
    pub linux: Vec<&'static str>,
    pub macos: Vec<&'static str>,
}

/// Creates a HashMap of library groups with their platform-specific search patterns
pub fn create_library_map() -> HashMap<&'static str, PlatformPatterns> {
    let mut map = HashMap::new();

    // OpenEXR Core Libraries (4 files)
    map.insert(
        "openexr-core",
        PlatformPatterns {
            windows: vec![
                "OpenEXR*.dll",
                "OpenEXRUtil*.dll",
                "IlmThread*.dll",
                "Iex*.dll",
            ],
            linux: vec![
                "libOpenEXR*.so*",
                "libOpenEXRUtil*.so*",
                "libIlmThread*.so*",
                "libIex*.so*",
            ],
            macos: vec![
                "libOpenEXR*.dylib",
                "libOpenEXRUtil*.dylib",
                "libIlmThread*.dylib",
                "libIex*.dylib",
            ],
        },
    );

    // Imath Library (1 file)
    map.insert(
        "imath",
        PlatformPatterns {
            windows: vec!["Imath*.dll"],
            linux: vec!["libImath*.so*"],
            macos: vec!["libImath*.dylib"],
        },
    );

    // Zlib Library (1 file)
    map.insert(
        "zlib",
        PlatformPatterns {
            windows: vec!["zlib*.dll", "z*.dll"],
            linux: vec!["libz.so*"],
            macos: vec!["libz*.dylib"],
        },
    );

    // OpenEXR-C wrapper from openexr-sys (1 file)
    map.insert(
        "openexr-c",
        PlatformPatterns {
            windows: vec!["openexr-c*.dll", "*openexr-c*.dll"],
            linux: vec!["libopenexr-c*.so*", "*openexr-c*.so*"],
            macos: vec!["libopenexr-c*.dylib", "*openexr-c*.dylib"],
        },
    );

    map
}

/// Gets platform-specific patterns from the library map
fn get_platform_patterns() -> Vec<&'static str> {
    let lib_map = create_library_map();
    let mut patterns = Vec::new();

    for (_name, platform_patterns) in lib_map.iter() {
        if cfg!(target_os = "windows") {
            patterns.extend(platform_patterns.windows.iter());
        } else if cfg!(target_os = "linux") {
            patterns.extend(platform_patterns.linux.iter());
        } else if cfg!(target_os = "macos") {
            patterns.extend(platform_patterns.macos.iter());
        }
    }

    patterns
}

/// Find all native libraries in the target directory
///
/// Searches in two locations:
/// 1. target/{profile}/lib/ (Linux/macOS) or target/{profile}/bin/ (Windows) - main library directory
/// 2. target/{profile}/build/openexr-sys-*/out/ - openexr-c wrapper
///
/// Returns a Vec of canonical paths to found libraries (symlinks resolved, duplicates removed)
pub fn find_libraries(profile: &str) -> Result<Vec<PathBuf>> {
    // On Windows, cmake puts DLLs in bin/, on Linux/macOS in lib/
    #[cfg(target_os = "windows")]
    let lib_dir = PathBuf::from(format!("target/{}/bin", profile));

    #[cfg(not(target_os = "windows"))]
    let lib_dir = PathBuf::from(format!("target/{}/lib", profile));

    let build_pattern = format!("target/{}/build/openexr-sys-*/out", profile);

    println!("Searching for libraries in:");
    println!("  - {}", lib_dir.display());
    println!("  - {}", build_pattern);

    let patterns = get_platform_patterns();
    let mut found = HashSet::new();

    // Configure glob for case-insensitive search (important for Windows)
    let glob_options = MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    // Search in lib/ directory
    if lib_dir.exists() {
        for pattern in &patterns {
            let full_pattern = lib_dir.join(pattern);
            let pattern_str = full_pattern
                .to_str()
                .context("Invalid pattern path")?;

            if let Ok(entries) = glob_with(pattern_str, glob_options) {
                for entry in entries.flatten() {
                    // Skip symlinks, only process actual files
                    if entry.is_symlink() {
                        continue;
                    }

                    // Canonicalize to resolve any remaining symlinks and get absolute path
                    if let Ok(canonical) = std::fs::canonicalize(&entry) {
                        found.insert(canonical);
                    }
                }
            }
        }
    }

    // Search in openexr-sys build directory for openexr-c wrapper
    if let Ok(entries) = glob_with(&build_pattern, glob_options) {
        for build_dir in entries.flatten() {
            for pattern in &patterns {
                let full_pattern = build_dir.join(pattern);
                let pattern_str = full_pattern
                    .to_str()
                    .context("Invalid pattern path")?;

                if let Ok(lib_entries) = glob_with(pattern_str, glob_options) {
                    for entry in lib_entries.flatten() {
                        if entry.is_symlink() {
                            continue;
                        }

                        if let Ok(canonical) = std::fs::canonicalize(&entry) {
                            found.insert(canonical);
                        }
                    }
                }
            }
        }
    }

    let mut result: Vec<PathBuf> = found.into_iter().collect();
    result.sort();

    println!("Found {} libraries:", result.len());
    for lib in &result {
        if let Some(name) = lib.file_name() {
            println!("  - {}", name.to_string_lossy());
        }
    }

    Ok(result)
}

/// Verify that all expected libraries were found
///
/// We expect 7 libraries total:
/// - OpenEXR core: 4 files (OpenEXR, OpenEXRUtil, IlmThread, Iex)
/// - Imath: 1 file
/// - Zlib: 1 file
/// - OpenEXR-C: 1 file
pub fn verify_library_count(libs: &[PathBuf]) -> Result<()> {
    const EXPECTED_COUNT: usize = 7;

    if libs.len() < EXPECTED_COUNT {
        anyhow::bail!(
            "Expected {} libraries but found only {}. Missing libraries!",
            EXPECTED_COUNT,
            libs.len()
        );
    }

    if libs.len() > EXPECTED_COUNT {
        println!(
            "Warning: Found {} libraries (expected {}). This might be OK if there are extra dependencies.",
            libs.len(),
            EXPECTED_COUNT
        );
    }

    Ok(())
}
