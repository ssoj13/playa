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
    },

    /// Verify all dependencies are present
    Verify {
        /// Use release profile
        #[arg(long)]
        release: bool,
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
        Commands::Release { level, dry_run } => cmd_release(&level, dry_run),
        Commands::Verify { release } => cmd_verify(release),
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

/// Command: cargo xtask release [patch|minor|major] [--dry-run]
fn cmd_release(level: &str, dry_run: bool) -> Result<()> {
    release::run_release(level, dry_run)
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
