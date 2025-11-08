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
        .arg("--no-publish")
        .arg("--no-confirm");  // Skip confirmation prompt

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

    // Success message (cargo-release handles pushing branch and tags by default)
    println!();
    println!("========================================");
    println!("SUCCESS! cargo-release executed (branch + tags handled by cargo-release)");
    println!("========================================");
    println!();
    println!("Next steps:");
    println!("1. Build workflow will run at: https://github.com/ssoj13/playa/actions");
    println!("2. Download and test the build artifacts (retained for 7 days)");
    println!("3. Verify release artifacts and publish notes as needed");
    println!();

    Ok(())
}
