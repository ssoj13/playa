#!/usr/bin/env bash
# Bootstrap script for playa project
# Checks dependencies, builds xtask, and runs commands
#
# Usage:
#   ./bootstrap.sh                    # Show xtask help
#   ./bootstrap.sh tag-dev patch      # Run xtask command
#   ./bootstrap.sh build --release    # Run xtask command

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

# Check if cargo-release is installed
if ! cargo release --version &> /dev/null; then
    echo "[1/2] Installing cargo-release..."
    cargo install cargo-release
    echo "  ✓ cargo-release installed"
else
    echo "[1/2] ✓ cargo-release already installed"
fi

# Check if cargo-packager is installed
if ! cargo packager --version &> /dev/null; then
    echo "[2/2] Installing cargo-packager..."
    cargo install cargo-packager --version 0.11.7 --locked
    echo "  ✓ cargo-packager installed"
else
    echo "[2/2] ✓ cargo-packager already installed"
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
