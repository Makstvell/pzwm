# pzwm - Pan And Zoom Window Manager

![pzwm](docs/screenshot.png)

## What is it

pzwm is a Wayland compositor (a window manager) for Linux, written in Rust.

Your windows live on one big **infinite canvas**. You move around it and
zoom in and out, like in Figma or Miro. There is no tiling grid: you put
windows wherever you want, and zoom out to see all of them at once.

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

Recommended extras:
`xwayland-satellite grim slurp wl-clipboard xdg-desktop-portal-gtk`.

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
| `Super+C` | center on the focused window |
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

## Config

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

Useful commands:

```sh
pzwm --check-config            # check the config for errors
journalctl --user -t pzwm -b   # logs
```

## Credits

pzwm is built on these open-source projects. Thank you to their authors.

| Project | Used for | License |
|---|---|---|
| [Smithay](https://github.com/Smithay/smithay) | the base of the compositor | MIT |
| [smithay-drm-extras](https://github.com/Smithay/smithay) | monitor info | MIT |
| [calloop](https://github.com/Smithay/calloop) | event loop | MIT |
| [winit](https://github.com/rust-windowing/winit) | `--nested` mode | Apache-2.0 |
| [serde](https://github.com/serde-rs/serde), [toml](https://github.com/toml-rs/toml) | reading the config | MIT or Apache-2.0 |
| [tracing](https://github.com/tokio-rs/tracing) | logs | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | errors | MIT or Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | cursor themes | MIT |

All other Rust libraries pzwm uses are listed in [`Cargo.lock`](Cargo.lock).
Their licenses (MIT, Apache-2.0, BSD, ISC, Zlib and similar) all allow
use in a GPL program. pzwm does not include their source code; Cargo
downloads it when you build.

The programs in [Works with](#works-with) (DMS, waybar, xwayland-satellite
and others) are separate projects with their own licenses. pzwm only talks
to them through standard Wayland protocols and does not include their code.

## License

pzwm is free software under the **GNU General Public License, version 2
or (at your option) any later version** (GPL-2.0-or-later), the same family
of license as the Linux kernel. See [LICENSE](LICENSE).
