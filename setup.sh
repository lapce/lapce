#!/bin/bash

if command -v apt &> /dev/null; then
    sudo apt install clang libxkbcommon-x11-dev pkg-config libvulkan-dev libwayland-dev xorg-dev libxcb-shape0-dev libxcb-xfixes0-dev
elif command -v dnf &> /dev/null; then
    sudo dnf install clang libxkbcommon-x11-devel libxcb-devel vulkan-loader-devel wayland-devel openssl-devel pkgconf
elif command -v pacman &> /dev/null; then
    sudo pacman -S clang vulkan-headers wayland xorg-server-devel xcb-util xcb-util-wm openssl base-devel libxkbcommon-x11
elif command -v xbps-install &> /dev/null; then
    sudo xbps-install -S base-devel clang libxkbcommon-devel vulkan-loader wayland-devel
else
    echo "Error: your distribution is not supported. Please create an issue on the GitHub issues page (https://github.com/lapce/lapce/issues/new?template=bug_report.md)"
    exit 1
fi