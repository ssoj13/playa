use anyhow::{Context, Result};
use std::process::Command;

/// Run the complete release process
///
/// This replaces the release.sh script and performs:
/// 1. Calculate next version with optional -dev suffix
/// 2. cargo release <version> --no-publish --execute [--dry-run]
/// 3. git push --tags (if not dry-run) - triggers build workflow
///
/// The release level should be one of: patch, minor, major
/// metadata: Optional pre-release suffix (e.g., "dev" creates v0.1.29-dev tags)
///
/// Note: Use metadata="dev" for dev branch tags, None for main branch releases.
pub fn run_release(level: &str, dry_run: bool, metadata: Option<&str>) -> Result<()> {
    println!("========================================");
    print!("Preparing release with level: {}", level);
    if let Some(meta) = metadata {
        println!(" (pre-release: {})", meta);
    } else {
        println!();
    }
    if dry_run {
        println!("DRY RUN MODE: No changes will be committed or pushed");
    }
    println!("========================================");
    println!();

    // Validate release level
    match level {
        "patch" | "minor" | "major" => {}
        _ => {
            anyhow::bail!(
                "Invalid release level: '{}'. Must be one of: patch, minor, major",
                level
            );
        }
    }

    // Step 1: Calculate next version
    println!("[1/4] Calculating next version...");

    // Read current version from Cargo.toml
    let cargo_toml = std::fs::read_to_string("Cargo.toml")
        .context("Failed to read Cargo.toml")?;

    let current_version = cargo_toml
        .lines()
        .find(|line| line.starts_with("version"))
        .and_then(|line| line.split('"').nth(1))
        .ok_or_else(|| anyhow::anyhow!("Could not find version in Cargo.toml"))?;

    // Parse version
    let parts: Vec<&str> = current_version.split('.').collect();
    if parts.len() != 3 {
        anyhow::bail!("Invalid version format in Cargo.toml: {}", current_version);
    }

    let major: u32 = parts[0].parse().context("Invalid major version")?;
    let minor: u32 = parts[1].parse().context("Invalid minor version")?;
    let patch: u32 = parts[2].split('-').next().unwrap()
        .parse().context("Invalid patch version")?;

    // Calculate next version
    let (next_major, next_minor, next_patch) = match level {
        "major" => (major + 1, 0, 0),
        "minor" => (major, minor + 1, 0),
        "patch" => (major, minor, patch + 1),
        _ => unreachable!(),
    };

    // Build version string with optional pre-release suffix
    let next_version = if let Some(meta) = metadata {
        format!("{}.{}.{}-{}", next_major, next_minor, next_patch, meta)
    } else {
        format!("{}.{}.{}", next_major, next_minor, next_patch)
    };

    println!("Current version: {}", current_version);
    println!("Next version: {}", next_version);
    println!();

    // Step 2: Run cargo release with explicit version
    println!("[2/4] Updating version and preparing release...");
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("release")
        .arg(&next_version)  // Pass version directly instead of level
        .arg("--no-publish");

    if dry_run {
        cmd.arg("--dry-run");
    } else {
        cmd.arg("--execute");
    }

    let status = cmd.status().context("Failed to run cargo release")?;

    if !status.success() {
        anyhow::bail!("cargo release failed with exit code: {:?}", status.code());
    }

    // If dry run, stop here
    if dry_run {
        println!();
        println!("========================================");
        println!("DRY RUN COMPLETE! No changes were made.");
        println!("========================================");
        return Ok(());
    }

    // Step 3: Get current branch
    println!();
    println!("[3/4] Detecting current branch...");

    let output = Command::new("git")
        .args(&["branch", "--show-current"])
        .output()
        .context("Failed to get current branch")?;

    let current_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Current branch: {}", current_branch);

    // Step 4: Push current branch and tags
    println!();
    println!("[4/4] Pushing to {} branch and tags (triggers workflow)...", current_branch);

    let status = Command::new("git")
        .args(&["push", "origin", &current_branch])
        .status()
        .context("Failed to push current branch")?;

    if !status.success() {
        anyhow::bail!(
            "Failed to push to {} branch. Exit code: {:?}",
            current_branch,
            status.code()
        );
    }

    // Push tags (triggers build workflow)
    println!("Pushing tags...");

    let status = Command::new("git")
        .args(&["push", "--tags"])
        .status()
        .context("Failed to push tags")?;

    if !status.success() {
        anyhow::bail!("Failed to push tags. Exit code: {:?}", status.code());
    }

    // Success message
    println!();
    println!("========================================");
    println!("SUCCESS! Release tag pushed from {} branch", current_branch);
    println!("========================================");
    println!();
    println!("Next steps:");
    println!("1. Build workflow will run at: https://github.com/ssoj13/playa/actions");
    println!("2. Download and test the build artifacts (retained for 7 days)");
    println!("3. If everything works, merge {} to main (manually or via PR)", current_branch);
    println!("4. After merge to main, the release workflow will publish the release");
    println!();

    Ok(())
}
