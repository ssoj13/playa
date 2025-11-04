#!/usr/bin/env bash
# Build and tag release from dev branch
#
# This script is a wrapper around 'cargo xtask release' for convenience.
# You can also call 'cargo xtask release' directly.
#
# Usage:
#   ./github_deploy_dev.sh [patch|minor|major] [--dry-run]
#
# Examples:
#   ./github_deploy_dev.sh patch          # Create patch release (0.1.13 -> 0.1.14)
#   ./github_deploy_dev.sh minor          # Create minor release (0.1.13 -> 0.2.0)
#   ./github_deploy_dev.sh major          # Create major release (0.1.13 -> 1.0.0)
#   ./github_deploy_dev.sh patch --dry-run # Test without making changes
#
# What happens:
#   1. Updates version in Cargo.toml
#   2. Generates CHANGELOG.md
#   3. Creates commit and tag
#   4. Pushes current branch (dev) and tags to GitHub
#   5. Build workflow runs and creates artifacts for testing
#   6. After testing, merge to main manually or via PR
#   7. Release workflow publishes the official release

set -e

# Check if xtask is available
if ! cargo xtask --help &>/dev/null; then
    echo "Error: cargo xtask is not available."
    echo "The xtask crate may not be built yet."
    echo ""
    echo "Solution: Run 'cargo build -p xtask' first to build the xtask tool."
    exit 1
fi

# Check if cargo-release is installed
if ! cargo release --version &>/dev/null; then
    echo "Error: cargo-release is not installed."
    echo ""
    echo "Solution: Install it with: cargo install cargo-release"
    exit 1
fi

# Get release level from argument (patch, minor, major), default to patch
LEVEL=${1:-patch}

# Get dry-run flag from argument
DRY_RUN=${2:-}

# Build xtask command
CMD="cargo xtask release $LEVEL"

if [ "$DRY_RUN" == "--dry-run" ]; then
    CMD="$CMD --dry-run"
fi

# Run the command
echo "Running: $CMD"
echo ""
$CMD
