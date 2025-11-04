use anyhow::{Context, Result};
use std::process::Command;

/// Run the complete release process
///
/// This replaces the release.sh script and performs:
/// 1. cargo release {level} --no-publish --execute [--dry-run]
/// 2. git push --tags (if not dry-run) - triggers build workflow in dev branch
///
/// The release level should be one of: patch, minor, major
///
/// Note: Tags should be pushed from dev branch to trigger build workflow.
/// Merge to main happens manually or via GitHub PR after testing the build.
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

    // Step 2: Get current branch
    println!();
    println!("[2/4] Detecting current branch...");

    let output = Command::new("git")
        .args(&["branch", "--show-current"])
        .output()
        .context("Failed to get current branch")?;

    let current_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Current branch: {}", current_branch);

    // Step 3: Push current branch
    println!();
    println!("[3/4] Pushing to {} branch...", current_branch);

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

    // Step 4: Push tags (triggers build workflow)
    println!();
    println!("[4/4] Pushing tags (this triggers the build workflow)...");

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
