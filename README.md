# pzwm - Pan And Zoom Window Manager

![pzwm](docs/screenshot.png)

## What is it

pzwm is a Wayland compositor (a window manager) for Linux, written in Rust.

Your windows live on one big **infinite canvas**. You move around it and
zoom in and out, like in Figma or Miro. There is no tiling grid: you put
windows wherever you want, and zoom out to see all of them at once.

## Features

- **Infinite canvas** with smooth zoom around the cursor and panning by
  mouse, touchpad or keyboard; each workspace remembers its own view.
- **Fit a window to the screen**: `Super+C` centers the focused window and
  zooms until it fills the visible area.
- **Same size for every new window**: apps open at 60 % × 70 % of the screen
  instead of whatever size they remembered (configurable).
- **Minimap** with live window previews: jump to a window or drag the view.
- **10 workspaces**, shown in the DankMaterialShell bar.
- **Desktop profiles**: save where your apps are and restore them later,
  apps that are not running are started and put back in place.
- **Several monitors**: mode, scale and position per monitor, or
  **mirroring** one monitor onto another.
- **Screen sharing** and screenshots through `xdg-desktop-portal-wlr`
  (wlr-screencopy).
- Maximize, fullscreen, pin to screen, half-screen tiling by dragging to an
  edge, magnetic edges, server-side title bars.
- **pzwm config**: a settings app for everything in the config file.
- Scriptable through `pzwm msg …`; the config reloads when you save it.

## Works with

pzwm draws only the windows and the canvas. A bar, launcher, notifications
and lock screen come from other programs, and these work:

- **[DankMaterialShell (DMS)](https://github.com/AvengeMedia/DankMaterialShell)**:
  bar, launcher, notifications, lock screen
- **Bars**: waybar
- **Launchers**: rofi, fuzzel
- **Notifications**: mako, swaync
- **Wallpaper**: swaybg, awww
- **Lock and idle**: hyprlock, hypridle, swayidle
- **Screenshots and clipboard**: grim, slurp, wl-clipboard, cliphist
- **X11 apps**: [xwayland-satellite](https://github.com/Supreeeme/xwayland-satellite)

### DMS setup

The DMS workspace widget shows pzwm's workspaces 1 … 10 and switches them
on click. pzwm speaks the small protocol DMS already supports for the
Umbriel compositor: it exports `UMBRIEL_SOCKET`, and the installer adds an
`umbriel` command (a link to `pzwm`) that DMS calls to switch workspaces.

If DMS runs as a systemd service, pzwm starts it for you. If DMS was
only set up for another compositor (for example Hyprland), run this once:

```sh
systemctl --user add-wants pzwm-session.target dms.service
```

Or start any program from the config:

```toml
[general]
autostart = ["waybar", "swaybg -i ~/wall.jpg"]
```

## Install

You need Rust 1.85 or newer.

```sh
git clone https://github.com/Makstvell/pzwm
cd pzwm
./install.sh --deps
```

`--deps` installs the system libraries first (Arch, Fedora, Debian/Ubuntu,
openSUSE, Void). If you already have them, run just `./install.sh`.

The settings app needs Python 3.11+ with PyGObject, GTK 4 and libadwaita
(`--deps` installs them).

Recommended extras:
`xwayland-satellite grim slurp wl-clipboard xdg-desktop-portal-gtk xdg-desktop-portal-wlr`.

**Try it first**, without logging out. This opens pzwm in a window:

```sh
pzwm --nested
```

**Start it**: log out and pick **pzwm** in your login screen (GDM, SDDM,
greetd, ly…), or run `pzwm-session` from a TTY.

**Uninstall**: `./install.sh --uninstall`

## How to use it

### Zoom: always hold Super

**Hold `Super` and scroll the mouse wheel** to zoom. This is the best way:
it works everywhere, also over windows, and it zooms around the mouse
cursor.

Scrolling without `Super` zooms only when the mouse is over empty canvas.
Over a window, the wheel scrolls that window as usual.

### Move around the canvas

- **Hold the middle mouse button and drag** (anywhere), or
- **`Super` + left mouse drag** (anywhere), or
- **left mouse drag** on empty canvas.

On a touchpad: pinch to zoom, three fingers to move.

### Mouse

| Shortcut | Action |
|---|---|
| `Super` + scroll | zoom (recommended) |
| middle drag / `Super` + left drag | move the canvas |
| `Super+Ctrl` + scroll | move the canvas up / down |
| `Super+Shift` + scroll | move the canvas left / right |
| drag the title bar / `Super+Shift` + left drag | move a window |
| drag a window edge / `Super` + right drag | resize a window |
| double-click the title bar | maximize / restore |
| drop a window on the left / right screen edge | fill half the screen |
| drop a window on the top screen edge | maximize |

### Keyboard

| Shortcut | Action |
|---|---|
| `Super+Return` | terminal |
| `Super+D` | app launcher |
| `Super+E` | file manager |
| `Super+Q` | close window |
| `Super+F` | maximize / restore |
| `Super+Shift+F` | fullscreen / restore |
| `Super+P` | pin window to the screen (it stays while the canvas moves) |
| `Super+Tab` or `Super+Z` | show all windows; press again to go back |
| `Super+M` | minimap (tap: stays open, hold: closes when you let go) |
| `Super+C` | center the focused window and zoom so it fills the screen |
| `Super+Home` | go to the start of the canvas |
| `Super+=` / `Super+-` | zoom in / out |
| `Super+BackSpace` | zoom to 100 % |
| `Super+arrows` / `Super+H J K` | focus the window in that direction |
| `Alt+Tab` / `Alt+Shift+Tab` | next / previous window |
| `Super+Ctrl+arrows` | move window |
| `Super+Shift+arrows` | resize window |
| `Super+Alt+arrows` | move the canvas |
| `Super+[` / `Super+]` | window to left / right half |
| `Super+1` … `Super+0` | go to window 1 … 10 (in the order you opened them) |
| `Super+Ctrl+1` … `0` | go to workspace 1 … 10 |
| `Super+Shift+1` … `0` | move window to workspace 1 … 10 |
| `Super+Shift+Space` | next keyboard layout |
| `Print` | screenshot to `~/Pictures/Screenshots` |
| `Super+Shift+S` | screenshot of an area to the clipboard |
| `Super+Shift+R` | reload the config |
| `Super+Ctrl+Q` | quit pzwm |

### Workspaces

There are 10 workspaces. Each monitor shows one of them, and every
workspace has its own camera (position and zoom). Switch with
`Super+Ctrl+1` … `0` or by clicking a workspace in the DMS bar.

## Config

### Settings app

Open **pzwm config** from your app launcher, or run `pzwm-config`.

It edits `~/.config/pzwm/config.toml` with switches, sliders, color
pickers and lists instead of text:

- **General**: autostart commands, scale, cursor, X11 apps
- **Input**: keyboard layouts, key repeat, touchpad, pointer speed
- **Canvas**: new window size, zoom limits, mouse behaviour, grid
- **Monitors**: mode, scale, position, mirroring; lists connected monitors
- **Appearance**, **Animation**, **Minimap**: sizes and colors
- **Shortcuts** and **Mouse**: your bindings and the built-in ones; add,
  change or turn off a binding, or press *Record* and the keys

Saving keeps your comments and the order of the file, checks the result
with `pzwm --check-config` first, and leaves the previous file as
`config.toml.bak`. pzwm applies the changes right away.

### Config file

The config file is:

```
~/.config/pzwm/config.toml
```

The installer creates it. All options, with comments, are in
[`resources/default-config.toml`](resources/default-config.toml).
Changes apply when you save the file.

Change shortcuts in `[binds]` (keyboard) and `[mouse]` (mouse):

```toml
[binds]
"Super+Return" = "spawn kitty"   # your terminal
"Super+Q" = "none"               # remove a shortcut

[mouse]
"Alt+left" = "move-window"       # add your own
```

### New windows

New windows open at a share of the screen; dialogs and fixed-size windows
keep their own size:

```toml
[canvas]
new_window_width = 0.6    # 0.1 … 1.0
new_window_height = 0.7
```

### Monitors

Monitor names (`eDP-1`, `HDMI-A-1`, …) are printed in the log at startup
and by `pzwm msg outputs`.

```toml
[[output]]
name = "eDP-1"
mode = "2560x1440@165"
scale = 1.5
position = "0,0"

[[output]]
name = "HDMI-A-1"
scale = 1.0
position = "1707,0"       # right of the laptop: 2560 / 1.5
```

To show a copy of one monitor on another (projector, TV) instead of
extending the desktop:

```toml
[[output]]
name = "HDMI-A-1"
mirror = "eDP-1"
```

A mirror has no workspace or bar of its own. It is scaled to fit its
resolution without cropping. The setting applies when the monitor is
plugged in.

### Screen sharing

Install `xdg-desktop-portal-wlr`. The installer adds
`pzwm-portals.conf`, which sends screen sharing and screenshots to it, so
browsers, Discord and OBS can share a monitor.

### Desktop profiles

Save where your apps are and bring them back later. Loading puts open
windows back in place, starts the missing apps and places them as they open.

```sh
pzwm msg profile save work     # or bind it: "Super+F1" = "profile save work"
pzwm msg profile load work
pzwm msg profile list
```

Profiles are stored in `~/.local/state/pzwm/profiles/`. A profile keeps,
for each window, the app, workspace, position, size and maximized state, plus
the view of each workspace and which workspace each monitor shows. Missing
apps are started with `gtk-launch`, so their app id must match a `.desktop`
file. To load one at login, add it to `autostart`:

```toml
[general]
autostart = ["pzwm msg profile load work"]
```

### Scripting

`pzwm msg` sends any action from the `[binds]` list to the running pzwm:

```sh
pzwm msg workspace 3
pzwm msg fit
pzwm msg windows           # list windows
pzwm msg outputs           # list monitors
```

Useful commands:

```sh
pzwm --check-config            # check the config for errors
pzwm --check-config FILE       # check another file
journalctl --user -t pzwm -b   # logs
```

## Similar projects

See [docs/similar-projects.md](docs/similar-projects.md).

## Credits

pzwm is built on these open-source projects. Thank you to their authors.

| Project | Used for | License |
|---|---|---|
| [Smithay](https://github.com/Smithay/smithay) | the base of the compositor | MIT |
| [smithay-drm-extras](https://github.com/Smithay/smithay) | monitor info | MIT |
| [calloop](https://github.com/Smithay/calloop) | event loop | MIT |
| [winit](https://github.com/rust-windowing/winit) | `--nested` mode | Apache-2.0 |
| [serde](https://github.com/serde-rs/serde), [toml](https://github.com/toml-rs/toml) | reading the config | MIT or Apache-2.0 |
| [serde_json](https://github.com/serde-rs/json) | profiles, bar feed | MIT or Apache-2.0 |
| [PyGObject](https://gitlab.gnome.org/GNOME/pygobject), [GTK](https://gtk.org), [libadwaita](https://gitlab.gnome.org/GNOME/libadwaita) | the pzwm config app (used, not included) | LGPL-2.1-or-later |
| [tracing](https://github.com/tokio-rs/tracing) | logs | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | errors | MIT or Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | cursor themes | MIT |

All other Rust libraries pzwm uses are listed in [`Cargo.lock`](Cargo.lock).
Their licenses (MIT, Apache-2.0, BSD, ISC, Zlib and similar) all allow
use in a GPL program. pzwm does not include their source code; Cargo
downloads it when you build.

The programs in [Works with](#works-with) (DMS, waybar, xwayland-satellite
and others) are separate projects with their own licenses. pzwm only talks
to them through Wayland protocols and, for the DMS bar, a local socket; it
does not include their code.

## License

pzwm is free software under the **GNU General Public License, version 2
or (at your option) any later version** (GPL-2.0-or-later), the same family
of license as the Linux kernel. See [LICENSE](LICENSE).
