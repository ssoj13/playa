#!/bin/bash
# Install FFmpeg runtime dependencies for WSL/Linux

set -e

echo "Installing FFmpeg dependencies..."

sudo apt update

sudo apt install -y \
    libasound2-dev \
    libpulse-dev \
    libjack-dev \
    libsdl2-dev \
    libcaca-dev \
    libcdio-dev \
    libcdio-paranoia-dev \
    libdc1394-dev \
    libraw1394-dev \
    libavc1394-dev \
    libiec61883-dev \
    libopenal-dev \
    libxv-dev \
    libxext-dev \
    libxcb-shm0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libpocketsphinx-dev \
    libsphinxbase-dev \
    xdg-desktop-portal \
    xdg-desktop-portal-gtk \
    zenity

echo "Done!"
