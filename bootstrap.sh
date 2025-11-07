#!/usr/bin/env bash
# Bootstrap script for playa project
# Checks dependencies, builds xtask, and runs commands
#
# Usage:
#   ./bootstrap.sh                    # Show xtask help
#   ./bootstrap.sh tag-dev patch      # Run xtask command
#   ./bootstrap.sh build --release    # Run xtask command
#   ./bootstrap.sh wipe               # Clean ./target from stale platform binaries (non-recursive)
#   ./bootstrap.sh wipe -v            # Verbose output
#   ./bootstrap.sh wipe --dry-run     # Show what would be removed
#   ./bootstrap.sh wipe-wf            # Delete all GitHub Actions workflow runs for this repo

set -e

# Check if cargo is installed
if ! command -v cargo &> /dev/null; then
    echo "Error: Rust/Cargo not found!"
    echo ""
    echo "Please install Rust from: https://rustup.rs/"
    exit 1
fi

echo "Checking dependencies..."
echo ""

# Check if cargo-binstall is installed
if ! cargo binstall --version &> /dev/null; then
    echo "[1/3] Installing cargo-binstall..."
    cargo install cargo-binstall
    echo "  ✓ cargo-binstall installed"
else
    echo "[1/3] ✓ cargo-binstall already installed"
fi

# Check if cargo-release is installed
if ! cargo release --version &> /dev/null; then
    echo "[2/3] Installing cargo-release..."
    if ! cargo binstall cargo-release --no-confirm; then
        echo "  Falling back to cargo install..."
        cargo install cargo-release
    fi
    echo "  ✓ cargo-release installed"
else
    echo "[2/3] ✓ cargo-release already installed"
fi

# Check if cargo-packager is installed
if ! cargo packager --version &> /dev/null; then
    echo "[3/3] Installing cargo-packager..."
    if ! cargo binstall cargo-packager --version 0.11.7 --no-confirm; then
        echo "  Falling back to cargo install..."
        cargo install cargo-packager --version 0.11.7 --locked
    fi
    echo "  ✓ cargo-packager installed"
else
    echo "[3/3] ✓ cargo-packager already installed"
fi

echo ""
echo "Dependencies ready!"
echo ""

# Check if xtask is built
if [ ! -f "target/debug/xtask" ]; then
    echo "Building xtask..."
    cargo build -p xtask
    echo "✓ xtask built"
    echo ""
fi

# Run xtask with all arguments
if [ $# -eq 0 ]; then
    # No arguments - show help
    cargo xtask --help
else
    # Pass all arguments to xtask
    cargo xtask "$@"
fi
