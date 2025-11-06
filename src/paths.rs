use anyhow::{Context, Result};
use std::path::PathBuf;

/// Configuration for overriding default application paths
#[derive(Debug, Clone)]
pub struct PathConfig {
    /// Custom config directory (from CLI or ENV)
    pub config_dir: Option<PathBuf>,
}

impl PathConfig {
    /// Create PathConfig from CLI arguments and environment variables
    ///
    /// Priority: CLI args → ENV var (PLAYA_CONFIG_DIR) → None (use defaults)
    pub fn from_env_and_cli(cli_dir: Option<PathBuf>) -> Self {
        let config_dir = cli_dir.or_else(|| {
            std::env::var("PLAYA_CONFIG_DIR")
                .ok()
                .map(PathBuf::from)
        });

        Self { config_dir }
    }
}

/// Get path to a configuration file
///
/// Priority:
/// 1. CLI --config-dir argument
/// 2. PLAYA_CONFIG_DIR environment variable
/// 3. Local folder IF any config files exist (playa.json, playa_cache.json, playa.log)
/// 4. Platform-specific config directory from dirs-next (default)
///
/// Platform paths:
/// - Linux: ~/.config/playa/{name}
/// - macOS: ~/Library/Application Support/playa/{name}
/// - Windows: %APPDATA%\playa\{name}
pub fn config_file(name: &str, config: &PathConfig) -> PathBuf {
    get_config_dir(config).join(name)
}

/// Get path to a data file (cache, logs, etc.)
///
/// Priority:
/// 1. CLI --config-dir argument
/// 2. PLAYA_CONFIG_DIR environment variable
/// 3. Local folder IF any config files exist (playa.json, playa_cache.json, playa.log)
/// 4. Platform-specific data directory from dirs-next (default)
///
/// Platform paths:
/// - Linux: ~/.local/share/playa/{name}
/// - macOS: ~/Library/Application Support/playa/{name}
/// - Windows: %APPDATA%\playa\{name}
pub fn data_file(name: &str, config: &PathConfig) -> PathBuf {
    get_data_dir(config).join(name)
}

/// Ensure that configuration and data directories exist
///
/// Creates directories if they don't exist. Returns error if creation fails.
pub fn ensure_dirs(config: &PathConfig) -> Result<()> {
    let config_dir = get_config_dir(config);
    let data_dir = get_data_dir(config);

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir)
            .with_context(|| format!("Failed to create config directory: {}", config_dir.display()))?;
    }

    // Only create data_dir if it's different from config_dir
    if data_dir != config_dir && !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory: {}", data_dir.display()))?;
    }

    Ok(())
}

/// Check if any config files exist in the given directory
fn has_local_config_files(dir: &PathBuf) -> bool {
    let files = ["playa.json", "playa_cache.json", "playa.log"];
    files.iter().any(|f| dir.join(f).exists())
}

/// Get the configuration directory
fn get_config_dir(config: &PathConfig) -> PathBuf {
    // Priority 1: Custom directory from CLI or ENV
    if let Some(dir) = &config.config_dir {
        return dir.clone();
    }

    // Priority 2: Local folder IF config files exist there
    if let Ok(current_dir) = std::env::current_dir() {
        if has_local_config_files(&current_dir) {
            return current_dir;
        }
    }

    // Priority 3: Platform-specific config directory
    if let Some(dir) = dirs_next::config_dir() {
        return dir.join("playa");
    }

    // Fallback: "." if everything else fails
    PathBuf::from(".")
}

/// Get the data directory
fn get_data_dir(config: &PathConfig) -> PathBuf {
    // Priority 1: Custom directory from CLI or ENV (same as config)
    if let Some(dir) = &config.config_dir {
        return dir.clone();
    }

    // Priority 2: Local folder IF config files exist there
    if let Ok(current_dir) = std::env::current_dir() {
        if has_local_config_files(&current_dir) {
            return current_dir;
        }
    }

    // Priority 3: Platform-specific data directory
    if let Some(dir) = dirs_next::data_dir() {
        return dir.join("playa");
    }

    // Fallback: "." if everything else fails
    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_with_custom_dir() {
        let config = PathConfig {
            config_dir: Some(PathBuf::from("/custom")),
        };

        let path = config_file("test.json", &config);
        assert_eq!(path, PathBuf::from("/custom/test.json"));
    }

    #[test]
    fn test_data_file_with_custom_dir() {
        let config = PathConfig {
            config_dir: Some(PathBuf::from("/custom")),
        };

        let path = data_file("cache.json", &config);
        assert_eq!(path, PathBuf::from("/custom/cache.json"));
    }

    #[test]
    fn test_config_file_uses_platform_defaults() {
        let config = PathConfig { config_dir: None };

        let path = config_file("test.json", &config);
        // Should contain "playa" and "test.json" in the path
        assert!(path.to_string_lossy().contains("playa"));
        assert!(path.to_string_lossy().contains("test.json"));
    }

    #[test]
    fn test_show_actual_paths() {
        println!("\n=== Platform-specific paths (no local files) ===");

        let config = PathConfig { config_dir: None };

        let cfg_path = config_file("playa.json", &config);
        let cache_path = data_file("playa_cache.json", &config);
        let log_path = data_file("playa.log", &config);

        println!("Config file: {}", cfg_path.display());
        println!("Cache file:  {}", cache_path.display());
        println!("Log file:    {}", log_path.display());

        println!("\n=== With custom directory ===");
        let custom_config = PathConfig {
            config_dir: Some(PathBuf::from("/tmp/playa-test")),
        };

        println!("Config file: {}", config_file("playa.json", &custom_config).display());
        println!("Cache file:  {}", data_file("playa_cache.json", &custom_config).display());
        println!("Log file:    {}", data_file("playa.log", &custom_config).display());
    }

    #[test]
    fn test_local_files_priority() {
        use std::fs;

        // Create a temporary directory
        let temp_dir = std::env::temp_dir().join("playa_test_local");
        let _ = fs::create_dir_all(&temp_dir);

        // Save current dir
        let original_dir = std::env::current_dir().unwrap();

        // Change to temp dir
        std::env::set_current_dir(&temp_dir).unwrap();

        let config = PathConfig { config_dir: None };

        // Test 1: No local files - should use platform defaults
        let path_without_local = config_file("playa.json", &config);
        println!("\nWithout local files: {}", path_without_local.display());
        assert!(path_without_local.to_string_lossy().contains("playa"));
        assert!(!path_without_local.starts_with(&temp_dir));

        // Test 2: Create a local file - should use current directory
        fs::write(temp_dir.join("playa.json"), "{}").unwrap();
        let path_with_local = config_file("playa.json", &config);
        println!("With local playa.json: {}", path_with_local.display());
        // Compare canonicalized paths to handle symlinks (e.g., /var vs /private/var on macOS)
        let canonical_result = path_with_local.canonicalize().unwrap();
        let canonical_expected = temp_dir.join("playa.json").canonicalize().unwrap();
        assert_eq!(canonical_result, canonical_expected);

        // Cleanup
        std::env::set_current_dir(&original_dir).unwrap();
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
