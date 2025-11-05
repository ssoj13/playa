#!/usr/bin/env bash
# Create dev tag with -dev suffix for testing
#
# Usage:
#   ./gh_tag_dev.sh [patch|minor|major] [--dry-run]
#
# Examples:
#   ./gh_tag_dev.sh patch          - Create v0.1.14-dev tag
#   ./gh_tag_dev.sh minor          - Create v0.2.0-dev tag
#   ./gh_tag_dev.sh --dry-run      - Test without making changes
#
# What happens:
#   1. Updates version in Cargo.toml
#   2. Generates CHANGELOG.md
#   3. Creates commit and tag with -dev suffix
#   4. Pushes to dev branch
#   5. Build workflow creates test artifacts (NOT release)

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

# Build xtask command with -dev suffix
CMD="cargo xtask release $LEVEL --metadata dev"

if [ "$DRY_RUN" == "--dry-run" ]; then
    CMD="$CMD --dry-run"
fi

# Run the command
echo "Running: $CMD"
echo ""
echo "This will create a tag with -dev suffix (e.g., v0.1.14-dev)"
echo "Build workflow will create test artifacts (NOT GitHub Release)"
echo ""
$CMD
