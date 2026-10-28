#!/usr/bin/env bash
# Build and install pzwm.
#
#   ./install.sh                 build + install to /usr/local (asks for sudo)
#   ./install.sh --deps          also install build/runtime packages first
#   ./install.sh --prefix /usr   install somewhere else
#   ./install.sh --no-build      install an existing target/release/pzwm
#   ./install.sh --uninstall     remove everything this script installed
#
# Run it as your normal user: it only uses sudo for the system files and
# writes ~/.config/pzwm/config.toml for you (never overwriting your edits).
set -euo pipefail

cd "$(dirname "$0")"

PREFIX=/usr/local
BUILD=1
DEPS=0
UNINSTALL=0
SESSIONS_DIR=/usr/share/wayland-sessions

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="$2"; shift 2 ;;
        --prefix=*) PREFIX="${1#*=}"; shift ;;
        --no-build) BUILD=0; shift ;;
        --deps) DEPS=1; shift ;;
        --uninstall) UNINSTALL=1; shift ;;
        -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
    command -v sudo >/dev/null || die "sudo is needed to install into $PREFIX"
    SUDO=sudo
fi

FILES=(
    "$PREFIX/bin/pzwm"
    "$PREFIX/bin/pzwm-session"
    "$PREFIX/bin/umbriel"
    "$SESSIONS_DIR/pzwm.desktop"
    "$PREFIX/lib/systemd/user/pzwm-session.target"
    "$PREFIX/share/xdg-desktop-portal/pzwm-portals.conf"
)

if [ "$UNINSTALL" = 1 ]; then
    bold "Removing pzwm"
    for f in "${FILES[@]}"; do
        if [ -e "$f" ] || [ -L "$f" ]; then $SUDO rm -f "$f" && ok "removed $f"; fi
    done
    echo "Your config in ~/.config/pzwm was left alone."
    exit 0
fi

if [ "$DEPS" = 1 ]; then
    bash scripts/install-deps.sh
fi

bold "Checking build dependencies"
command -v cargo >/dev/null || die "Rust is missing. Install it from https://rustup.rs or your package manager (rust >= 1.85)."
command -v pkg-config >/dev/null || die "pkg-config is missing (run ./install.sh --deps)"
missing=()
for lib in libseat libinput libudev gbm libdrm xkbcommon egl glesv2 wayland-server; do
    pkg-config --exists "$lib" || missing+=("$lib")
done
if [ ${#missing[@]} -gt 0 ]; then
    die "missing development libraries: ${missing[*]} (run ./install.sh --deps)"
fi
ok "all libraries found"

if [ "$BUILD" = 1 ]; then
    bold "Building (release)"
    cargo build --release --locked 2>/dev/null || cargo build --release
    ok "target/release/pzwm"
fi
[ -x target/release/pzwm ] || die "target/release/pzwm not found; build first"

bold "Installing to $PREFIX"
$SUDO install -Dm755 target/release/pzwm "$PREFIX/bin/pzwm"
$SUDO install -Dm755 resources/pzwm-session "$PREFIX/bin/pzwm-session"
# DankMaterialShell switches workspaces through `umbriel msg` (see src/bar.rs).
$SUDO ln -sf pzwm "$PREFIX/bin/umbriel"
$SUDO install -Dm644 resources/pzwm.desktop "$SESSIONS_DIR/pzwm.desktop"
$SUDO install -Dm644 resources/pzwm-session.target "$PREFIX/lib/systemd/user/pzwm-session.target"
$SUDO install -Dm644 resources/pzwm-portals.conf "$PREFIX/share/xdg-desktop-portal/pzwm-portals.conf"
for f in "${FILES[@]}"; do ok "$f"; done
if [ "$PREFIX" != /usr ] && [ "$PREFIX" != /usr/local ]; then
    warn "$PREFIX/bin must be in PATH for display managers to find pzwm-session"
fi

if [ "$(id -u)" -ne 0 ]; then
    systemctl --user daemon-reload 2>/dev/null || true
    config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/pzwm"
    if [ ! -f "$config_dir/config.toml" ]; then
        install -Dm644 resources/default-config.toml "$config_dir/config.toml"
        ok "created $config_dir/config.toml"
    else
        ok "kept your $config_dir/config.toml"
    fi
fi

for opt in xwayland-satellite grim slurp wl-copy; do
    command -v "$opt" >/dev/null || warn "optional: '$opt' not found (see README)"
done

echo
bold "Done."
echo "  Log out and pick \"pzwm\" in your display manager, or run 'pzwm-session' from a TTY."
echo "  Try it inside your current desktop first:  pzwm --nested"
