mod lib_discovery;
mod post_build;
mod pre_build;
mod release;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::process::Command;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Playa build automation tasks", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Patch OpenEXR headers for Linux GCC 11+ compatibility
    Pre,

    /// Build the project and copy dependencies automatically
    Build {
        /// Build in release mode
        #[arg(long)]
        release: bool,
    },

    /// Copy native dependencies and shaders after build
    Post {
        /// Use release profile
        #[arg(long)]
        release: bool,
    },

    /// Create and publish a release (replaces release.sh)
    Release {
        /// Release level: patch, minor, or major
        #[arg(default_value = "patch")]
        level: String,

        /// Dry run - don't actually commit or push
        #[arg(long)]
        dry_run: bool,

        /// Metadata suffix for pre-release versions (e.g., "dev" creates v0.1.14-dev)
        #[arg(long)]
        metadata: Option<String>,
    },

    /// Verify all dependencies are present
    Verify {
        /// Use release profile
        #[arg(long)]
        release: bool,
    },

    /// Preview unreleased changelog (saves to CHANGELOG.preview.md)
    ChangelogPreview,

    /// Create dev tag with -dev suffix (auto-bumps patch if no version specified)
    TagDev {
        /// Release level: patch, minor, or major (default: patch)
        #[arg(default_value = "patch")]
        level: String,

        /// Dry run - don't actually commit or push
        #[arg(long)]
        dry_run: bool,
    },

    /// Create release tag on main branch (auto-bumps patch if no version specified)
    TagRelease {
        /// Release level: patch, minor, or major (default: patch)
        #[arg(default_value = "patch")]
        level: String,

        /// Dry run - don't actually commit or push
        #[arg(long)]
        dry_run: bool,
    },

    /// Deploy locally (install to system)
    Deploy {
        /// Install location (default: auto-detect)
        #[arg(long)]
        install_dir: Option<String>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Pre => cmd_pre(),
        Commands::Build { release } => cmd_build(release),
        Commands::Post { release } => cmd_post(release),
        Commands::Release { level, dry_run, metadata } => cmd_release(&level, dry_run, metadata.as_deref()),
        Commands::Verify { release } => cmd_verify(release),
        Commands::ChangelogPreview => cmd_changelog_preview(),
        Commands::TagDev { level, dry_run } => cmd_tag_dev(&level, dry_run),
        Commands::TagRelease { level, dry_run } => cmd_tag_release(&level, dry_run),
        Commands::Deploy { install_dir } => cmd_deploy(install_dir.as_deref()),
    }
}

/// Command: cargo xtask pre
fn cmd_pre() -> Result<()> {
    pre_build::patch_headers()
}

/// Command: cargo xtask build [--release]
fn cmd_build(release: bool) -> Result<()> {
    println!("========================================");
    println!("Building playa with automatic dependency management");
    println!("========================================");
    println!();

    // Step 1: Pre-build (Linux header patching)
    #[cfg(target_os = "linux")]
    {
        println!("Step 1/3: Patching headers...");
        pre_build::patch_headers()?;
        println!();
    }

    // Step 2: Run cargo build
    let step_num = if cfg!(target_os = "linux") {
        "2/3"
    } else {
        "1/2"
    };

    println!("Step {}: Building...", step_num);

    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if release {
        cmd.arg("--release");
    }

    let status = cmd.status()?;

    if !status.success() {
        anyhow::bail!("Build failed!");
    }

    println!();

    // Step 3: Post-build (copy dependencies)
    let step_num = if cfg!(target_os = "linux") {
        "3/3"
    } else {
        "2/2"
    };

    println!("Step {}: Copying dependencies...", step_num);
    println!();

    let profile = if release { "release" } else { "debug" };
    post_build::copy_dependencies(profile)?;

    Ok(())
}

/// Command: cargo xtask post [--release]
fn cmd_post(release: bool) -> Result<()> {
    let profile = if release { "release" } else { "debug" };
    post_build::copy_dependencies(profile)
}

/// Command: cargo xtask release [patch|minor|major] [--dry-run] [--metadata dev]
fn cmd_release(level: &str, dry_run: bool, metadata: Option<&str>) -> Result<()> {
    release::run_release(level, dry_run, metadata)
}

/// Command: cargo xtask verify [--release]
fn cmd_verify(release: bool) -> Result<()> {
    let profile = if release { "release" } else { "debug" };

    println!("========================================");
    println!("Verifying dependencies for profile: {}", profile);
    println!("========================================");
    println!();

    // Check libraries
    let libraries = lib_discovery::find_libraries(profile)?;

    if libraries.is_empty() {
        anyhow::bail!("No libraries found!");
    }

    println!();
    lib_discovery::verify_library_count(&libraries)?;

    // Check shaders
    let shaders_dir = std::path::PathBuf::from(format!("target/{}/shaders", profile));

    println!();
    println!("Checking shaders directory...");

    if !shaders_dir.exists() {
        println!("  ✗ shaders/ directory not found!");
        anyhow::bail!("Missing shaders directory");
    }

    println!("  ✓ shaders/ directory present");

    println!();
    println!("========================================");
    println!("All dependencies verified successfully!");
    println!("========================================");

    Ok(())
}

/// Command: cargo xtask changelog-preview
fn cmd_changelog_preview() -> Result<()> {
    use anyhow::Context;

    println!("========================================");
    println!("Generating changelog preview...");
    println!("========================================");
    println!();

    let status = Command::new("git-cliff")
        .args(&["--unreleased", "-o", "CHANGELOG.preview.md"])
        .status()
        .context("Failed to run git-cliff. Is it installed?")?;

    if !status.success() {
        anyhow::bail!("git-cliff failed with exit code: {:?}", status.code());
    }

    println!("✓ Preview saved to CHANGELOG.preview.md");
    println!();
    println!("This file shows unreleased changes that will be added");
    println!("to CHANGELOG.md on the next release.");
    println!();

    Ok(())
}

/// Command: cargo xtask tag-dev [patch|minor|major] [--dry-run]
fn cmd_tag_dev(level: &str, dry_run: bool) -> Result<()> {
    println!("========================================");
    println!("Creating DEV tag with level: {}", level);
    if dry_run {
        println!("DRY RUN MODE: No changes will be made");
    }
    println!("========================================");
    println!();
    println!("This will create a tag with -dev suffix (e.g., v0.1.14-dev)");
    println!("Build workflow will create test artifacts (NOT GitHub Release)");
    println!();

    // Call release command with metadata="dev"
    release::run_release(level, dry_run, Some("dev"))
}

/// Command: cargo xtask tag-release [patch|minor|major] [--dry-run]
fn cmd_tag_release(level: &str, dry_run: bool) -> Result<()> {
    use anyhow::Context;

    // Check if on main branch
    let output = Command::new("git")
        .args(&["branch", "--show-current"])
        .output()
        .context("Failed to get current branch")?;

    let current_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if current_branch != "main" {
        println!("========================================");
        println!("ERROR: You must be on main branch!");
        println!("========================================");
        println!();
        println!("Current branch: {}", current_branch);
        println!();
        println!("Solution:");
        println!("  1. git checkout main");
        println!("  2. git merge dev");
        println!("  3. Run this command again");
        println!();
        anyhow::bail!("Not on main branch");
    }

    println!("========================================");
    println!("Creating RELEASE tag with level: {}", level);
    if dry_run {
        println!("DRY RUN MODE: No changes will be made");
    }
    println!("========================================");
    println!();
    println!("This will create an official release tag (e.g., v0.1.14)");
    println!("Release workflow will create GitHub Release with installers");
    println!();

    // Call release command WITHOUT metadata (no -dev suffix)
    release::run_release(level, dry_run, None)
}

/// Command: cargo xtask deploy [--install-dir /path/to/install]
fn cmd_deploy(install_dir: Option<&str>) -> Result<()> {
    use anyhow::Context;
    use std::env;
    use std::path::PathBuf;

    println!("========================================");
    println!("Local deployment (install to system)");
    println!("========================================");
    println!();

    // Determine install directory
    let target_dir = if let Some(dir) = install_dir {
        PathBuf::from(dir)
    } else {
        // Auto-detect based on OS
        if cfg!(target_os = "windows") {
            // Windows: %LOCALAPPDATA%\Programs\playa
            let local_app_data = env::var("LOCALAPPDATA")
                .context("LOCALAPPDATA not set")?;
            PathBuf::from(local_app_data).join("Programs").join("playa")
        } else if cfg!(target_os = "macos") {
            // macOS: /Applications/playa.app
            PathBuf::from("/Applications/playa.app/Contents/MacOS")
        } else {
            // Linux: ~/.local/bin
            let home = env::var("HOME")
                .context("HOME not set")?;
            PathBuf::from(home).join(".local").join("bin")
        }
    };

    println!("Install directory: {}", target_dir.display());
    println!();

    // Create directory if it doesn't exist
    if !target_dir.exists() {
        println!("Creating directory...");
        std::fs::create_dir_all(&target_dir)
            .context("Failed to create install directory")?;
    }

    // Build in release mode first
    println!("Building release version...");
    cmd_build(true)?;
    println!();

    // Copy files
    println!("Copying files to install directory...");

    let exe_name = if cfg!(target_os = "windows") {
        "playa.exe"
    } else {
        "playa"
    };

    let source_exe = PathBuf::from("target/release").join(exe_name);
    let target_exe = target_dir.join(exe_name);

    std::fs::copy(&source_exe, &target_exe)
        .context("Failed to copy executable")?;
    println!("  ✓ Copied {}", exe_name);

    // Copy DLLs/SOs
    let lib_pattern = if cfg!(target_os = "windows") {
        "*.dll"
    } else {
        "*.so*"
    };

    for entry in glob::glob(&format!("target/release/{}", lib_pattern))
        .context("Failed to read library files")?
    {
        let entry = entry?;
        let file_name = entry.file_name().unwrap();
        let target_file = target_dir.join(file_name);
        std::fs::copy(&entry, &target_file)
            .context(format!("Failed to copy {}", file_name.to_string_lossy()))?;
        println!("  ✓ Copied {}", file_name.to_string_lossy());
    }

    // Copy shaders directory from project root
    let source_shaders = PathBuf::from("shaders");

    if source_shaders.exists() {
        fs_extra::dir::copy(
            &source_shaders,
            &target_dir,
            &fs_extra::dir::CopyOptions::new().overwrite(true),
        )
        .context("Failed to copy shaders directory")?;
        println!("  ✓ Copied shaders/");
    }

    println!();
    println!("========================================");
    println!("Deployment complete!");
    println!("========================================");
    println!();
    println!("Installed to: {}", target_dir.display());
    println!();

    if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
        println!("To run playa from anywhere, add to PATH:");
        println!("  export PATH=\"{}:$PATH\"", target_dir.display());
        println!();
    }

    Ok(())
}
