use anyhow::{Context, Result};
use std::process::Command;

/// Run the complete release process
///
/// This replaces the release.sh script and performs:
/// 1. cargo release {level} --no-publish --execute [--dry-run]
/// 2. git push --tags (if not dry-run)
/// 3. git push origin main (if not dry-run)
///
/// The release level should be one of: patch, minor, major
pub fn run_release(level: &str, dry_run: bool) -> Result<()> {
    println!("========================================");
    println!("Preparing release with level: {}", level);
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

    // Step 1: Run cargo release
    println!("[1/4] Updating version and preparing release...");
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("release")
        .arg(level)
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

    // Step 2: Push tags
    println!();
    println!("[2/4] Pushing tags...");

    let status = Command::new("git")
        .args(&["push", "--tags"])
        .status()
        .context("Failed to push tags")?;

    if !status.success() {
        anyhow::bail!("Failed to push tags. Exit code: {:?}", status.code());
    }

    // Step 3: Push to main branch
    println!();
    println!("[3/4] Pushing to main branch...");

    let status = Command::new("git")
        .args(&["push", "origin", "main"])
        .status()
        .context("Failed to push to main branch")?;

    if !status.success() {
        anyhow::bail!(
            "Failed to push to main branch. Exit code: {:?}",
            status.code()
        );
    }

    // Step 4: Success message
    println!();
    println!("========================================");
    println!("SUCCESS! Release created with cargo release");
    println!("========================================");
    println!();
    println!("GitHub Actions will now build the release.");
    println!("Monitor progress at: https://github.com/ssoj13/playa/actions");
    println!();

    Ok(())
}
