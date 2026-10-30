#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Maksym Shymchenko (Makstvell)
# Installs pzwm build and runtime dependencies with the distro package manager.
set -euo pipefail

. /etc/os-release 2>/dev/null || true
ids=" ${ID:-} ${ID_LIKE:-} "

run() { echo "+ $*"; sudo "$@"; }

case "$ids" in
    *" arch "*|*" artix "*|*" manjaro "*|*" endeavouros "*|*" cachyos "*)
        run pacman -S --needed rust pkgconf libinput libxkbcommon mesa libdrm seatd systemd-libs wayland \
            xdg-desktop-portal-gtk python-gobject gtk4 libadwaita
        echo "optional: sudo pacman -S --needed xwayland-satellite grim slurp wl-clipboard xdg-desktop-portal-wlr"
        ;;
    *" fedora "*|*" rhel "*)
        run dnf install -y rust cargo pkgconf-pkg-config libinput-devel libxkbcommon-devel mesa-libgbm-devel \
            mesa-libEGL-devel mesa-libGLES-devel libdrm-devel libseat-devel systemd-devel wayland-devel \
            python3-gobject gtk4 libadwaita
        echo "optional: sudo dnf install xwayland-satellite grim slurp wl-clipboard xdg-desktop-portal-wlr"
        ;;
    *" debian "*|*" ubuntu "*)
        run apt-get update
        run apt-get install -y cargo rustc pkg-config libinput-dev libxkbcommon-dev libgbm-dev libegl-dev \
            libgles-dev libdrm-dev libseat-dev libudev-dev libwayland-dev \
            python3-gi gir1.2-gtk-4.0 gir1.2-adw-1
        echo "note: pzwm needs rust >= 1.85; if your rustc is older use https://rustup.rs"
        echo "optional: sudo apt install grim slurp wl-clipboard xdg-desktop-portal-wlr"
        ;;
    *" opensuse"*|*" suse "*)
        run zypper install -y rust cargo pkg-config libinput-devel libxkbcommon-devel libgbm-devel Mesa-libEGL-devel \
            Mesa-libGLESv2-devel libdrm-devel libseat-devel systemd-devel wayland-devel \
            python3-gobject typelib-1_0-Gtk-4_0 typelib-1_0-Adw-1
        ;;
    *" void "*)
        run xbps-install -Sy rust cargo pkg-config libinput-devel libxkbcommon-devel mesa-devel libdrm-devel \
            libseat-devel eudev-libudev-devel wayland-devel \
            python3-gobject gtk4 libadwaita
        ;;
    *)
        echo "Unknown distro (${ID:-?}). Install the development packages for:"
        echo "  libseat libinput libudev gbm libdrm xkbcommon egl glesv2 wayland-server, plus Rust >= 1.85"
        echo "  pzwm config also needs Python 3.11+ with PyGObject, GTK 4 and libadwaita"
        exit 1
        ;;
esac
