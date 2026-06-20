mod env_setup;
mod release;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "🎬 Playa build automation tasks")]
#[command(long_about = "\
🎬 Playa build automation tasks

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
COMMON WORKFLOWS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

  🔧 Local build:
     cargo xtask build

  🧪 Dev release (testing on CI):
     cargo xtask tag-dev patch

  🚀 Production release:
     cargo xtask pr v0.1.60        # Create PR: dev → main
     # Merge PR on GitHub
     git checkout main && git pull
     cargo xtask tag-rel patch     # Tag and release

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 🏗️  Build the project
    ///
    /// Examples:
    ///   cargo xtask build              # Release build (default)
    ///   cargo xtask build --debug      # Debug build
    Build {
        /// Build in release mode (default if no flag specified)
        #[arg(long)]
        release: bool,

        /// Build in debug mode
        #[arg(long)]
        debug: bool,

        /// Comma-separated cargo features to enable (e.g. "profiler")
        #[arg(short = 'f', long)]
        features: Option<String>,
    },

    /// 📋 Regenerate full CHANGELOG.md from git history
    Changelog,

    /// 🧪 Tag dev build on GitHub, trigger Build workflow (creates v0.1.x-dev)
    ///
    /// Creates a dev tag (e.g., v0.1.60-dev) that triggers CI Build workflow.
    /// CI builds artifacts for testing (NOT a GitHub Release).
    ///
    /// Workflow:
    ///   1. cargo xtask tag-dev patch              # Creates v0.1.60-dev tag
    ///   2. GitHub Actions builds artifacts for testing
    ///   3. Download artifacts from Actions to test
    ///   4. If good, create PR to main for official release
    ///
    /// Examples:
    ///   cargo xtask tag-dev patch       # Bump patch version (v0.1.59 → v0.1.60-dev)
    ///   cargo xtask tag-dev minor       # Bump minor version (v0.1.59 → v0.2.0-dev)
    ///   cargo xtask tag-dev --dry-run   # Preview changes without pushing
    TagDev {
        /// Release level: patch, minor, or major (default: patch)
        #[arg(default_value = "patch")]
        level: String,

        /// Dry run - don't actually commit or push
        #[arg(long)]
        dry_run: bool,
    },

    /// 🚀 Tag release on main, trigger Release workflow + GitHub Release (creates v0.1.x)
    ///
    /// Creates official release tag on main that triggers CI Release workflow.
    /// MUST be run from main branch after merging dev PR.
    /// Creates GitHub Release with installers.
    ///
    /// Full workflow:
    ///   1. cargo xtask pr v0.1.60                 # Create PR: dev → main
    ///   2. Merge PR on GitHub
    ///   3. git checkout main && git pull
    ///   4. cargo xtask tag-rel patch              # Creates v0.1.60 tag
    ///   5. GitHub Actions creates Release + installers
    ///
    /// Examples:
    ///   cargo xtask tag-rel patch       # Bump patch version (v0.1.59 → v0.1.60)
    ///   cargo xtask tag-rel minor       # Bump minor version (v0.1.59 → v0.2.0)
    ///   cargo xtask tag-rel --dry-run   # Preview changes without pushing
    TagRel {
        /// Release level: patch, minor, or major (default: patch)
        #[arg(default_value = "patch")]
        level: String,

        /// Dry run - don't actually commit or push
        #[arg(long)]
        dry_run: bool,
    },

    /// 🔀 Create Pull Request from dev to main with all commits
    Pr {
        /// Optional version for PR title (e.g., v0.2.0)
        version: Option<String>,
    },

    /// 💾 Install to system (Windows: %LOCALAPPDATA%\Programs, Linux: ~/.local/bin)
    Deploy {
        /// Custom install directory
        #[arg(long)]
        install_dir: Option<String>,
    },

    /// Remove executables and shared libraries from ./target (non-recursive)
    ///
    /// Clears installers and stray binaries/libs under `target/`, `target/release`, `target/debug`
    /// plus packager staging dirs — useful after restoring CI cache artifacts or rebuilding installers.
    Wipe {
        /// Verbose output (list scanned dirs and skipped files)
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,

        /// Dry run (show what would be removed without deleting)
        #[arg(long = "dry-run")]
        dry_run: bool,
    },

    /// Delete all GitHub Actions workflow runs for this repository (uses gh CLI)
    ///
    /// Usage:
    ///   cargo xtask wipe-wf
    #[clap(name = "wipe-wf")]
    WipeWf,

    /// 🧪 Run all workspace tests (unit + integration)
    ///
    /// Runs tests for **every workspace member**, not only the root `playa` package
    /// (`cargo test`'s default is `default-members`).
    ///
    /// Runs the complete test suite including:
    /// - Unit tests (fast, isolated component tests)
    /// - Integration tests (encoding, cache, sequence detection)
    ///
    /// Examples:
    ///   cargo xtask test              # Run all tests in release mode
    ///   cargo xtask test --debug      # Run all tests in debug mode
    ///   cargo xtask test --nocapture  # Show println! output from tests
    Test {
        /// Run in debug mode instead of release
        #[arg(long)]
        debug: bool,

        /// Show test output (pass --nocapture to cargo test)
        #[arg(long)]
        nocapture: bool,
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

    if matches!(cli.command, Commands::Build { .. } | Commands::Test { .. }) {
        env_setup::prepare_build_environment().context("Build environment bootstrap")?;
    }

    match cli.command {
        Commands::Build { release: _, debug, features } => {
            let is_release = !debug;
            cmd_build(is_release, features.as_deref())
        }
        Commands::Changelog => cmd_changelog(),
        Commands::TagDev { level, dry_run } => cmd_tag_dev(&level, dry_run),
        Commands::TagRel { level, dry_run } => cmd_tag_rel(&level, dry_run),
        Commands::Pr { version } => cmd_pr(version.as_deref()),
        Commands::Deploy { install_dir } => cmd_deploy(install_dir.as_deref()),
        Commands::Wipe { verbose, dry_run } => cmd_wipe(verbose, dry_run),
        Commands::WipeWf => cmd_wipe_wf(),
        Commands::Test { debug, nocapture } => {
            let is_release = !debug;
            cmd_test(is_release, nocapture)
        }
    }
}

/// Command: cargo xtask build [--release] [--features ...]
fn cmd_build(release: bool, features: Option<&str>) -> Result<()> {
    println!("========================================");
    println!("Building playa");
    println!("Profile: {}", if release { "release" } else { "debug" });
    println!("Backend: vfx-io / exr-core (pure Rust, all compressions)");
    if let Some(f) = features {
        println!("Features: {f}");
    }
    println!("========================================");
    println!();

    println!("Step 1/1: Building...");

    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", "playa"]);

    if release {
        cmd.arg("--release");
    }

    if let Some(f) = features {
        cmd.args(["--features", f]);
    }

    let status = cmd.status()?;

    if !status.success() {
        anyhow::bail!("Build failed!");
    }

    println!();
    println!("✓ Build complete (vfx-io EXR backend, pure Rust)");

    Ok(())
}

/// Command: cargo xtask wipe (non-recursive)
fn cmd_wipe(verbose: bool, dry_run: bool) -> Result<()> {
    println!("========================================");
    if verbose {
        println!("[wipe] scanning target directories");
    }
    println!(
        "Wiping executables, shared libraries, and packager staging from ./target, ./target/release, ./target/debug (non-recursive)"
    );
    println!("This removes platform-specific artifacts left by previous builds or cache restore.");
    println!("========================================");
    println!();

    let target_root = PathBuf::from("target");

    // Always clean packager staging directories if present
    for d in [
        target_root.join(".cargo-packager"),
        target_root.join("release/.cargo-packager"),
        target_root.join("debug/.cargo-packager"),
    ] {
        if d.exists() {
            if dry_run {
                println!("  would remove {}", d.display());
            } else {
                println!("  removing {}", d.display());
                let _ = fs::remove_dir_all(&d);
            }
        }
    }

    let dirs = [
        target_root.clone(),
        target_root.join("release"),
        target_root.join("debug"),
    ];

    let mut removed = 0usize;

    for dir in dirs.iter() {
        if !dir.exists() {
            continue;
        }
        let entries = match fs::read_dir(dir) {
            Ok(it) => it,
            Err(e) => {
                println!("Failed to read {}: {}", dir.display(), e);
                continue;
            }
        };
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                let meta = match fs::symlink_metadata(&path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let ftype = meta.file_type();
                if !(ftype.is_file() || ftype.is_symlink()) {
                    continue;
                }

                let name_lc = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                let stem_lc = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();

                // Never remove our own helper binary
                if stem_lc == "xtask" {
                    continue;
                }

                let is_installer = name_lc.ends_with(".msi")
                    || (name_lc.ends_with(".exe") && name_lc.contains("setup"));
                let is_win_bin = name_lc.ends_with(".exe") || name_lc.ends_with(".dll");
                let is_unix_lib = name_lc.contains(".so") || name_lc.ends_with(".dylib");

                // Regular files
                if ftype.is_file() && (is_installer || is_win_bin || is_unix_lib) {
                    if dry_run {
                        println!("  would remove {}", path.display());
                    } else {
                        println!("  removing {}", path.display());
                        let _ = fs::remove_file(&path);
                    }
                    removed += 1;
                    continue;
                }

                // Symlinks to shared libs
                #[cfg(unix)]
                if ftype.is_symlink() && is_unix_lib {
                    println!("  removing symlink {}", path.display());
                    let _ = fs::remove_file(&path);
                    removed += 1;
                    continue;
                }
            }
        }
    }

    println!();
    println!("Removed {} file(s)", removed);
    println!("Done.");
    Ok(())
}

/// Command: cargo xtask wipe-wf
/// Deletes all workflow runs via GitHub CLI (gh)
fn cmd_wipe_wf() -> Result<()> {
    println!("========================================");
    println!("Deleting all GitHub Actions workflow runs (via gh)");
    println!("========================================");
    println!();

    // Ensure gh is available
    let gh_ok = Command::new("gh").arg("--version").output().is_ok();
    if !gh_ok {
        anyhow::bail!(
            "'gh' CLI not found. Please install GitHub CLI and authenticate (gh auth login)"
        );
    }

    // List runs (IDs only)
    let out = Command::new("gh")
        .args([
            "run",
            "list",
            "--limit",
            "1000",
            "--json",
            "databaseId",
            "--jq",
            ".[].databaseId",
        ]) // up to 1000
        .output()
        .context("Failed to list workflow runs via 'gh run list'")?;
    if !out.status.success() {
        anyhow::bail!(
            "gh run list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let ids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if ids.is_empty() {
        println!("No workflow runs found.");
        return Ok(());
    }

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
        .min(16)
        .max(4);
    println!(
        "Found {} run(s). Deleting with {} workers...",
        ids.len(),
        workers
    );

    // Progress bar
    let pb = ProgressBar::new(ids.len() as u64);
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );

    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let ids = Arc::new(ids);
    let next = Arc::new(AtomicUsize::new(0));
    let deleted = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let pb_arc = Arc::new(pb);

    let mut handles = Vec::new();
    for _ in 0..workers {
        let ids_cl = Arc::clone(&ids);
        let next_cl = Arc::clone(&next);
        let deleted_cl = Arc::clone(&deleted);
        let failed_cl = Arc::clone(&failed);
        let pb_cl = Arc::clone(&pb_arc);
        handles.push(std::thread::spawn(move || {
            loop {
                let idx = next_cl.fetch_add(1, Ordering::Relaxed);
                if idx >= ids_cl.len() {
                    break;
                }
                let id = &ids_cl[idx];
                let endpoint = format!("repos/:owner/:repo/actions/runs/{}", id);
                let status = Command::new("gh")
                    .args(["api", "-X", "DELETE", &endpoint])
                    .status();
                match status {
                    Ok(st) if st.success() => {
                        println!("Deleted run #{}", id);
                        deleted_cl.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(_) | Err(_) => {
                        println!("Failed to delete run #{}", id);
                        failed_cl.fetch_add(1, Ordering::Relaxed);
                    }
                }
                pb_cl.inc(1);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let del = deleted.load(Ordering::Relaxed);
    let fail = failed.load(Ordering::Relaxed);
    pb_arc.finish_with_message(format!("deleted {} failed {}", del, fail));
    println!("Done. Deleted {} run(s), failed {}", del, fail);
    Ok(())
}

/// Command: cargo xtask changelog
fn cmd_changelog() -> Result<()> {
    use anyhow::Context;

    println!("========================================");
    println!("Regenerating full CHANGELOG.md...");
    println!("========================================");
    println!();

    let status = Command::new("git-cliff")
        .args(&["-o", "CHANGELOG.md"])
        .status()
        .context("Failed to run git-cliff. Is it installed?")?;

    if !status.success() {
        anyhow::bail!("git-cliff failed with exit code: {:?}", status.code());
    }

    println!("✓ CHANGELOG.md regenerated from full git history");
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

/// Command: cargo xtask tag-rel [patch|minor|major] [--dry-run]
fn cmd_tag_rel(level: &str, dry_run: bool) -> Result<()> {
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

/// Command: cargo xtask pr [version]
fn cmd_pr(version: Option<&str>) -> Result<()> {
    use anyhow::Context;

    println!("========================================");
    println!("Creating Pull Request: dev → main");
    println!("========================================");
    println!();

    // Count commits between main and dev
    println!("Calculating changes between main and dev...");
    let output = Command::new("git")
        .args(&["rev-list", "--count", "origin/main..dev"])
        .output()
        .context("Failed to count commits")?;

    let commit_count = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Determine version for title
    let title = if let Some(ver) = version {
        let ver_clean = ver.trim_start_matches('v');
        format!("Release v{}", ver_clean)
    } else {
        "Release".to_string()
    };

    let body = format!("{} - {} commits from dev branch", title, commit_count);

    println!("Creating Pull Request:");
    println!("  From: dev");
    println!("  To:   main");
    println!("  Title: {}", title);
    println!("  Commits: {}", commit_count);
    println!();

    // Create PR using gh CLI
    let status = Command::new("gh")
        .args(&[
            "pr", "create", "--base", "main", "--head", "dev", "--title", &title, "--body", &body,
        ])
        .status()
        .context("Failed to run 'gh pr create'. Is GitHub CLI installed?")?;

    if !status.success() {
        println!();
        println!("Error: Failed to create pull request");
        println!("Make sure you have:");
        println!("  - Pushed your dev branch to origin");
        println!("  - Authenticated with 'gh auth login'");
        anyhow::bail!("PR creation failed");
    }

    println!();
    println!("✓ Pull Request created successfully!");
    println!();
    println!("Next steps:");
    println!("  1. Review the PR on GitHub");
    println!("  2. Merge when ready: gh pr merge --merge");
    if let Some(ver) = version {
        let ver_clean = ver.trim_start_matches('v');
        println!("  3. Create release: cargo xtask tag-rel patch (from main)");
        println!("     (Version will be bumped to v{})", ver_clean);
    }
    println!();

    Ok(())
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
            let local_app_data = env::var("LOCALAPPDATA").context("LOCALAPPDATA not set")?;
            PathBuf::from(local_app_data).join("Programs").join("playa")
        } else if cfg!(target_os = "macos") {
            // macOS: /Applications/playa.app
            PathBuf::from("/Applications/playa.app/Contents/MacOS")
        } else {
            // Linux: ~/.local/bin
            let home = env::var("HOME").context("HOME not set")?;
            PathBuf::from(home).join(".local").join("bin")
        }
    };

    println!("Install directory: {}", target_dir.display());
    println!();

    // Create directory if it doesn't exist
    if !target_dir.exists() {
        println!("Creating directory...");
        std::fs::create_dir_all(&target_dir).context("Failed to create install directory")?;
    }

    // Build in release mode first
    println!("Building release version...");
    cmd_build(true, None)?;
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

    std::fs::copy(&source_exe, &target_exe).context("Failed to copy executable")?;
    println!("  ✓ Copied {}", exe_name);

    // Native deps are linked statically (vfx-exr, playa-ffmpeg, …); no extra DLL/SO bundles to ship.

    // Copy shaders directory from project root (optional)
    let source_shaders = PathBuf::from("shaders");

    if source_shaders.exists() {
        fs_extra::dir::copy(
            &source_shaders,
            &target_dir,
            &fs_extra::dir::CopyOptions::new().overwrite(true),
        )
        .context("Failed to copy shaders directory")?;
        println!("  ✓ Copied shaders/");
    } else {
        println!("  ⚠ shaders/ directory not found (using embedded shaders only)");
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

/// Command: cargo xtask test [--debug] [--nocapture]
fn cmd_test(is_release: bool, nocapture: bool) -> Result<()> {
    println!("🧪 Running all tests...");
    println!();

    let profile = if is_release { "release" } else { "debug" };
    println!("Profile: {}", profile);
    println!();

    // Build cargo test command (workspace: include playa-engine and other member crates, not only `default-members`)
    let mut cmd = Command::new("cargo");
    cmd.args(["test", "--workspace"]);

    if is_release {
        cmd.arg("--release");
    }

    // Always pass -- separator for test binary args
    cmd.arg("--");

    if nocapture {
        cmd.arg("--nocapture");
    }

    // Show test output
    cmd.arg("--show-output");

    let mut line = String::from("Running: cargo test --workspace");
    if is_release {
        line.push_str(" --release");
    }
    line.push_str(" --");
    if nocapture {
        line.push_str(" --nocapture");
    }
    line.push_str(" --show-output");
    println!("{line}");
    println!();

    // Run tests
    let status = cmd.status().context("Failed to run cargo test")?;

    println!();

    if status.success() {
        println!("✅ All tests passed!");
        Ok(())
    } else {
        anyhow::bail!("❌ Tests failed");
    }
}
