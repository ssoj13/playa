#!/usr/bin/env bash
# Deploy script using cargo xtask
# Builds and packages the application with all dependencies

set -e

# Check if xtask is available
if ! cargo xtask --help &>/dev/null; then
    echo "Error: cargo xtask is not available."
    echo "The xtask crate may not be built yet."
    echo ""
    echo "Solution: Run 'cargo build -p xtask' first to build the xtask tool."
    exit 1
fi

# Check if cargo packager is available
if ! cargo packager --version &>/dev/null; then
    echo "Error: cargo-packager is not installed."
    echo ""
    echo "Solution: Install it with: cargo install cargo-packager"
    exit 1
fi

# Build the application
cargo xtask build --release

# Set LD_LIBRARY_PATH so linuxdeploy can find our .so files when building AppImage
export LD_LIBRARY_PATH="$(pwd)/target/release:$LD_LIBRARY_PATH"

# Package the application
cargo packager --release
