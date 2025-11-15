#!/usr/bin/env bash
# Bootstrap script for playa project
# Checks dependencies, builds xtask, and runs commands
#
# Usage:
#   ./bootstrap.sh                    # Show xtask help
#   ./bootstrap.sh tag-dev patch      # Run xtask command
#   ./bootstrap.sh build --release    # Run xtask command
#   ./bootstrap.sh test               # Run encoding integration test
#   ./bootstrap.sh install            # Install playa from crates.io (checks FFmpeg dependencies)
#   ./bootstrap.sh publish            # Publish crate to crates.io
#   ./bootstrap.sh wipe               # Clean ./target from stale platform binaries (non-recursive)
#   ./bootstrap.sh wipe -v            # Verbose output
#   ./bootstrap.sh wipe --dry-run     # Show what would be removed
#   ./bootstrap.sh wipe-wf            # Delete all GitHub Actions workflow runs for this repo

set -e

# Set FFmpeg/vcpkg environment variables for this script session
if [ -d "/usr/local/share/vcpkg" ]; then
    export VCPKG_ROOT="/usr/local/share/vcpkg"

    # Determine triplet based on OS and architecture
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        if [[ "$(uname -m)" == "arm64" ]]; then
            export VCPKGRS_TRIPLET="arm64-osx-release"
        else
            export VCPKGRS_TRIPLET="x64-osx-release"
        fi
    else
        # Linux
        export VCPKGRS_TRIPLET="x64-linux-release"
    fi

    export PKG_CONFIG_PATH="$VCPKG_ROOT/installed/$VCPKGRS_TRIPLET/lib/pkgconfig"
    echo "✓ vcpkg configured: $VCPKG_ROOT"
    echo "✓ triplet: $VCPKGRS_TRIPLET"
    echo ""
fi

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

# Handle special commands
if [ "$1" = "test" ]; then
    # Run all tests via xtask
    cargo xtask test
    exit 0
fi

if [ "$1" = "publish" ]; then
    # Publish crate to crates.io
    echo "Publishing crate to crates.io..."
    echo ""
    cargo publish
    exit 0
fi

if [ "$1" = "install" ]; then
    # Install playa from crates.io with FFmpeg dependencies
    echo "Checking FFmpeg dependencies..."
    echo ""

    # Determine vcpkg path and triplet based on OS
    if [[ "$OSTYPE" == "darwin"* ]]; then
        VCPKG_PATH="/usr/local/share/vcpkg"
        if [[ "$(uname -m)" == "arm64" ]]; then
            TRIPLET="arm64-osx-release"
        else
            TRIPLET="x64-osx-release"
        fi
    else
        VCPKG_PATH="/usr/local/share/vcpkg"
        TRIPLET="x64-linux-release"
    fi

    # Check vcpkg
    if [ ! -d "$VCPKG_PATH" ]; then
        echo "Error: vcpkg not found at $VCPKG_PATH"
        echo ""
        read -p "Install vcpkg? (y/N): " install_vcpkg
        if [[ "$install_vcpkg" =~ ^[Yy]$ ]]; then
            echo "Installing vcpkg..."
            git clone https://github.com/microsoft/vcpkg.git "$VCPKG_PATH"
            "$VCPKG_PATH/bootstrap-vcpkg.sh"
            echo "✓ vcpkg installed"
        else
            echo "Installation cancelled."
            exit 1
        fi
    else
        echo "✓ vcpkg found"
    fi

    # Check FFmpeg
    if [ ! -f "$VCPKG_PATH/installed/$TRIPLET/lib/pkgconfig/libavutil.pc" ]; then
        echo ""
        echo "Error: FFmpeg not found"
        echo ""
        read -p "Install FFmpeg via vcpkg? (y/N): " install_ffmpeg
        if [[ "$install_ffmpeg" =~ ^[Yy]$ ]]; then
            echo "Installing FFmpeg with hardware acceleration support..."
            # macOS doesn't support nvcodec, Linux does
            if [[ "$OSTYPE" == "darwin"* ]]; then
                "$VCPKG_PATH/vcpkg" install "ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:$TRIPLET"
            else
                "$VCPKG_PATH/vcpkg" install "ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale,nvcodec]:$TRIPLET"
            fi
            echo "✓ FFmpeg installed"
        else
            echo "Installation cancelled."
            exit 1
        fi
    else
        echo "✓ FFmpeg found"
    fi

    # Check pkg-config
    if ! command -v pkg-config &> /dev/null; then
        echo ""
        echo "Error: pkg-config not found"
        echo ""
        read -p "Install pkg-config via vcpkg? (y/N): " install_pkgconfig
        if [[ "$install_pkgconfig" =~ ^[Yy]$ ]]; then
            echo "Installing pkg-config..."
            "$VCPKG_PATH/vcpkg" install "pkgconf:$TRIPLET"
            echo "✓ pkg-config installed"
        else
            echo "Installation cancelled."
            exit 1
        fi
    else
        echo "✓ pkg-config found"
    fi

    echo ""
    echo "Installing playa from crates.io..."
    echo ""
    cargo install playa
    exit 0
fi

# Run xtask with all arguments
if [ $# -eq 0 ]; then
    # No arguments - show bootstrap help
    cat << 'EOF'
Bootstrap script for playa project

USAGE:
  ./bootstrap.sh [COMMAND] [OPTIONS]

SPECIAL COMMANDS:
  test               Run all tests (unit + integration)
  install            Install playa from crates.io (checks FFmpeg deps)
  publish            Publish crate to crates.io

XTASK COMMANDS (forwarded to cargo xtask):
  build              Build playa (use --openexr for full EXR support)
  test               Run all tests (unit + integration) [can also use: bootstrap test]
  post               Copy native libraries (OpenEXR builds only)
  verify             Verify dependencies present
  deploy             Install to system
  tag-dev            Create dev tag (triggers Build workflow)
  tag-rel            Create release tag (triggers Release workflow)
  pr                 Create PR: dev -> main
  changelog          Preview unreleased CHANGELOG.md
  wipe               Clean target directory from stale binaries
  wipe-wf            Delete all GitHub workflow runs
  pre                Linux only: Patch OpenEXR headers

EXAMPLES:
  ./bootstrap.sh                    # Show this help
  ./bootstrap.sh build --release    # Build release binary
  ./bootstrap.sh test               # Run encoding test
  ./bootstrap.sh tag-dev patch      # Create v0.1.x-dev tag

For xtask command details, run: ./bootstrap.sh [command] --help
EOF
else
    # Pass all arguments to xtask
    cargo xtask "$@"
fi
