#!/usr/bin/env bash
# Build script using cargo xtask
# This automatically handles header patching and dependency copying

set -e

# Check if xtask is available
if ! cargo xtask --help &>/dev/null; then
    echo "Error: cargo xtask is not available."
    echo "The xtask crate may not be built yet."
    echo ""
    echo "Solution: Run 'cargo build -p xtask' first to build the xtask tool."
    exit 1
fi

cargo xtask build --release
