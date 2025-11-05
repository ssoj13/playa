#!/usr/bin/env bash
# Create release tag on main branch for official releases
#
# Usage:
#   ./gh_tag_release.sh [patch|minor|major] [--dry-run]
#
# Examples:
#   ./gh_tag_release.sh patch      - Create v0.1.14 tag on main
#   ./gh_tag_release.sh minor      - Create v0.2.0 tag on main
#   ./gh_tag_release.sh --dry-run  - Test without making changes
#
# IMPORTANT: Run this ONLY on main branch after merging from dev!
#
# What happens:
#   1. Updates version in Cargo.toml
#   2. Generates CHANGELOG.md
#   3. Creates commit and tag (NO -dev suffix)
#   4. Pushes to main branch
#   5. Release workflow creates official GitHub Release

set -e

# Check if on main branch
CURRENT_BRANCH=$(git branch --show-current)

if [ "$CURRENT_BRANCH" != "main" ]; then
    echo "Error: You must be on main branch to create a release tag!"
    echo "Current branch: $CURRENT_BRANCH"
    echo ""
    echo "Solution:"
    echo "  1. git checkout main"
    echo "  2. git merge dev"
    echo "  3. Run this script again"
    exit 1
fi

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

# Build xtask command (NO metadata = no -dev suffix)
CMD="cargo xtask release $LEVEL"

if [ "$DRY_RUN" == "--dry-run" ]; then
    CMD="$CMD --dry-run"
fi

# Run the command
echo "Running: $CMD"
echo ""
echo "This will create an official release tag (e.g., v0.1.14)"
echo "Release workflow will create GitHub Release with installers"
echo ""
$CMD
