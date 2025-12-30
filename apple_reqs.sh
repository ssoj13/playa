#!/usr/bin/env bash
# FFmpeg build requirements installer for macOS

set -e

ask() {
    read -p "$1 [Y/n]: " answer
    [[ -z "$answer" || "$answer" =~ ^[Yy]$ ]]
}

echo "=== FFmpeg Build Requirements ==="
echo ""

# 1. Xcode Command Line Tools
echo "Step 1/4: Xcode Command Line Tools"
if xcode-select -p &>/dev/null; then
    echo "  Already installed"
else
    if ask "Install Xcode Command Line Tools?"; then
        xcode-select --install
        echo "  Waiting for installation to complete..."
        read -p "Press Enter when done..."
    fi
fi
echo ""

# 2. Homebrew packages
echo "Step 2/4: Homebrew packages (pkg-config, nasm, yasm)"
if ! command -v brew &>/dev/null; then
    echo "  Homebrew not found. Please install from https://brew.sh"
else
    missing=""
    for pkg in pkg-config nasm yasm; do
        if ! brew list "$pkg" &>/dev/null; then
            missing="$missing $pkg"
        fi
    done
    if [ -n "$missing" ]; then
        if ask "Install$missing via Homebrew?"; then
            brew install $missing
        fi
    else
        echo "  All packages installed"
    fi
fi
echo ""

# 3. vcpkg check
echo "Step 3/4: vcpkg setup"
VCPKG_PATH="${VCPKG_ROOT:-$HOME/vcpkg}"
if [ ! -d "$VCPKG_PATH" ]; then
    if ask "vcpkg not found at $VCPKG_PATH. Clone it?"; then
        git clone https://github.com/microsoft/vcpkg.git "$VCPKG_PATH"
        "$VCPKG_PATH/bootstrap-vcpkg.sh"
    fi
else
    echo "  vcpkg found at $VCPKG_PATH"
fi
echo ""

# 4. FFmpeg installation
echo "Step 4/4: FFmpeg via vcpkg"
TRIPLET="arm64-osx-release"
if [[ "$(uname -m)" != "arm64" ]]; then
    TRIPLET="x64-osx-release"
fi

if [ -f "$VCPKG_PATH/installed/$TRIPLET/lib/pkgconfig/libavutil.pc" ]; then
    echo "  FFmpeg already installed for $TRIPLET"
else
    if ask "Install FFmpeg ($TRIPLET)? This takes 15-30 min"; then
        "$VCPKG_PATH/vcpkg" install "ffmpeg[core,avcodec,avdevice,avfilter,avformat,swresample,swscale]:$TRIPLET"
    fi
fi
echo ""

echo "=== Done ==="
echo ""
echo "Add to your shell profile:"
echo "  export VCPKG_ROOT=\"$VCPKG_PATH\""
echo "  export VCPKGRS_TRIPLET=\"$TRIPLET\""
echo "  export PKG_CONFIG_PATH=\"\$VCPKG_ROOT/installed/\$VCPKGRS_TRIPLET/lib/pkgconfig\""
