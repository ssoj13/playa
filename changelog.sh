#!/usr/bin/env bash
# Generate full CHANGELOG.md from git history
# This regenerates the entire changelog from scratch

set -e

echo "========================================"
echo "Generating CHANGELOG.md"
echo "========================================"
echo ""

git-cliff -o CHANGELOG.md

echo ""
echo "========================================"
echo "CHANGELOG.md updated successfully!"
echo "========================================"
