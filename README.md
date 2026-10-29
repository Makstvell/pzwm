# pzwm - Pan And Zoom Window Manager

![pzwm](docs/screenshot.png)

pzwm is a Wayland compositor (window manager) for Linux, written in Rust on
top of [Smithay](https://github.com/Smithay/smithay). Windows live on one
**infinite canvas** that you pan and zoom like a Figma or Miro board. There
is no tiling grid: put windows anywhere and zoom out to see all of them.

| | |
|---|---|
| **Type** | Wayland compositor, floating windows on an infinite canvas |
| **Language** | Rust 2024 (compositor), Python + GTK 4 (settings app) |
| **Runs on** | Linux, from a login manager or a TTY; `--nested` inside another desktop |
| **Config** | `~/.config/pzwm/config.toml`, or the **pzwm config** app |
| **Status** | Personal project, version 0.1, used daily |
| **License** | GPL-2.0-or-later |

## Contents

1. [Features](#features)
2. [Install](#install)
3. [First steps](#first-steps)
4. [How it works](#how-it-works)
5. [Shortcuts](#shortcuts)
6. [Settings app: pzwm config](#settings-app-pzwm-config)
7. [Configuration file](#configuration-file)
8. [Workspaces and the bar](#workspaces-and-the-bar)
9. [Monitors](#monitors)
10. [Desktop profiles](#desktop-profiles)
11. [Screen sharing and screenshots](#screen-sharing-and-screenshots)
12. [Scripting: pzwm msg](#scripting-pzwm-msg)
13. [Works with](#works-with)
14. [Troubleshooting](#troubleshooting)
15. [For developers](#for-developers)
16. [Similar projects](#similar-projects)
17. [Credits](#credits)
18. [License](#license)

## Features

**Canvas and windows**
- Infinite canvas with smooth zoom around the cursor; pan with the mouse,
  touchpad or keyboard. Each workspace remembers its own view.
- `Super+C` fits the focused window: centers it and zooms until it fills
  the screen.
- New windows open at the same size (90 % × 90 % of the screen by default),
  not at whatever size the app remembered.
- Maximize, fullscreen, pin a window to the screen, half-screen tiling by
  dropping a window on a screen edge, magnetic edges.
- Overview of all windows, and a minimap with live window previews.
- **Wallpaper parallax** (`[parallax] enabled = true`): the wallpaper
  drifts slowly left and right and shifts a little against the camera when
  you pan the canvas.
- **Find** (`Super+/`): search windows, groups and bookmarks by name and
  fly there, with rofi, fuzzel or wofi as the picker.
- **Camera history**: `Super+,` / `Super+.` go back and forward through
  your jumps (fit, find, overview, bookmark, minimap click), like a browser.
- **Bookmarks**: remember a view with `Super+Alt+Shift+1…5`, fly back
  with `Super+Alt+1…5`.
- **Groups**: windows that belong together get a tinted frame; fit,
  move and collapse the whole group at once.
- **Scratchpad**: a drop-down terminal on ``Super+` ``.
- **Window rules**: open an app on a given workspace, at a given place and
  size, pinned, maximized, in a group.

**Desktop**
- 10 workspaces, shown and switched in the DankMaterialShell bar.
- Desktop profiles: save where your apps are, restore them later; apps
  that are not running are started and put back in place.
- Several monitors with their own mode, scale and position, or mirroring
  one monitor onto another (a mirror is scaled to fill a TV of another
  resolution, wallpaper included).
- Screen sharing and screenshots (wlr-screencopy, xdg-desktop-portal-wlr).
- X11 apps through xwayland-satellite.
- Taskbars and docks see and control the windows (foreign-toplevel).
- Monitor tools work live: kanshi, wdisplays, wlr-randr
  (wlr-output-management); night light with wlsunset or gammastep.
- Input methods (fcitx5) and drawing tablets with pressure and tilt.

**Setup**
- **pzwm config**: a settings app for every option, shortcuts included.
- Plain TOML config that applies as soon as you save it.
- `pzwm msg …` controls the running compositor from scripts, with JSON
  output and an event stream.

## Install

### Requirements

| What | Needed for | Arch packages |
|---|---|---|
| Rust 1.85+, pkg-config, libinput, libxkbcommon, Mesa (GBM, EGL, GLES), libdrm, libseat, libudev, Wayland | building pzwm | `rust pkgconf libinput libxkbcommon mesa libdrm seatd systemd-libs wayland` |
| Python 3.11+, PyGObject, GTK 4, libadwaita | the pzwm config app | `python-gobject gtk4 libadwaita` |
| xdg-desktop-portal-gtk | file pickers in apps | `xdg-desktop-portal-gtk` |
| xdg-desktop-portal-wlr | screen sharing | `xdg-desktop-portal-wlr` (optional) |
| xwayland-satellite | X11 apps | `xwayland-satellite` (optional) |
| grim, slurp, wl-clipboard | screenshot shortcuts | `grim slurp wl-clipboard` (optional) |

`./install.sh --deps` installs the required packages on Arch, Fedora,
Debian/Ubuntu, openSUSE and Void.

### Build and install

```sh
git clone https://github.com/Makstvell/pzwm
cd pzwm
./install.sh --deps      # or ./install.sh if the packages are already there
```

The script builds in release mode and installs with `sudo` into
`/usr/local`. It never overwrites your own config.

| Installed file | What it is |
|---|---|
| `/usr/local/bin/pzwm` | the compositor |
| `/usr/local/bin/pzwm-session` | session start script used by login managers |
| `/usr/local/bin/umbriel` | link to `pzwm`; the DMS bar calls it to switch workspaces |
| `/usr/local/bin/pzwm-config` | the settings app |
| `/usr/local/share/pzwm-config/` | settings app code |
| `/usr/local/share/applications/pzwm-config.desktop` | **pzwm config** in your app launcher |
| `/usr/local/share/applications/pzwm-displays.desktop` | **Display Settings** in your app launcher (only under pzwm) |
| `/usr/share/wayland-sessions/pzwm.desktop` | **pzwm** entry in the login screen |
| `/usr/local/lib/systemd/user/pzwm-session.target` | starts the bar, portals and other session services |
| `/usr/local/share/xdg-desktop-portal/pzwm-portals.conf` | which portal handles screen sharing |
| `~/.config/pzwm/config.toml` | your config, created once from the defaults |

Options: `--prefix DIR`, `--no-build` (install an existing build),
`--uninstall` (removes everything above except your config).

## First steps

1. **Try it inside your current desktop**, without logging out:
   `pzwm --nested` opens pzwm in a window.
2. **Start it for real**: log out and pick **pzwm** in your login screen
   (GDM, SDDM, greetd, ly…), or run `pzwm-session` from a TTY.
3. Open a terminal with `Super+Return` and the launcher with `Super+D`.
4. **Hold `Super` and scroll** to zoom; **hold the middle mouse button and
   drag** to move around.
5. Open **pzwm config** from the launcher to change settings and shortcuts.

## How it works

| Idea | What it means |
|---|---|
| **Canvas** | An endless 2D plane. Windows have a position on it and keep their real size; zooming only changes how big everything looks. |
| **Camera** | What part of the canvas a monitor shows: a position and a zoom level. Panning and zooming move the camera, not the windows. |
| **Workspace** | One of 10 separate canvases. Each monitor shows one workspace; each workspace keeps its own camera. |
| **Window states** | *Canvas* (normal, moves with the canvas), *pinned* (fixed on screen while the canvas moves under it), *maximized* (fills the screen minus gaps), *fullscreen*. |
| **Overview** | Zooms out until every window of the workspace is visible; press again to go back. |
| **Minimap** | A small map of the whole workspace with live previews; click a window to jump to it. |
| **Bar, launcher, notifications** | Not part of pzwm. They come from other programs such as DankMaterialShell (see [Works with](#works-with)). |

## Shortcuts

All shortcuts can be changed in **pzwm config** (Shortcuts and Mouse pages)
or in the `[binds]` and `[mouse]` sections of the config.

### Zoom and move around

| Shortcut | Action |
|---|---|
| `Super` + scroll | zoom around the cursor (works everywhere, also over windows) |
| scroll on empty canvas | zoom (over a window the wheel scrolls the window) |
| middle drag, anywhere | move the canvas (a plain middle click still pastes) |
| `Super` + left drag / `Super` + middle drag | move the canvas |
| left drag on empty canvas | move the canvas |
| `Super+Ctrl` + scroll / `Super+Shift` + scroll | move the canvas up-down / left-right |
| touchpad: pinch / two fingers on empty canvas / three-finger swipe | zoom / move the canvas / move the canvas anywhere |
| `Super+=` / `Super+-` | zoom in / out |
| `Super+BackSpace` | zoom to 100 % |
| `Super+Home` | go back to the start of the canvas |
| `Super+Alt+arrows` | move the canvas |
| `Super+C` | fit the focused window: center it and zoom to fill the screen |
| `Super+Tab` or `Super+Z` | overview of all windows; again to go back |
| `Super+M` | minimap at the cursor (tap: stays open, hold: closes on release) |
| `Super+/` | find a window, group or bookmark |
| `Super+,` / `Super+.` | camera back / forward |
| `Super+Alt+Shift+1` … `5` / `Super+Alt+1` … `5` | save bookmark / fly to bookmark |

### Windows

| Shortcut | Action |
|---|---|
| drag the title bar / `Super+Shift` + left drag | move a window |
| drag an edge or corner / `Super` + right drag | resize a window |
| double-click the title bar, `Super+F` | maximize / restore |
| `Super+Shift+F` | fullscreen / restore |
| `Super+P`, pin button | pin to the screen / back to the canvas |
| drop on the left or right screen edge, `Super+[` / `Super+]` | fill that half of the screen |
| drop on the top screen edge | maximize |
| `Super+Q` | close |
| `Super+Ctrl+arrows` | move the window |
| `Super+Shift+arrows` | resize the window |
| ``Super+` `` | scratchpad: call up or put away the drop-down terminal |

### Groups

| Shortcut | Action |
|---|---|
| `Super+Ctrl+G` | add the window to a group (the picker asks for its name) |
| `Super+Alt+G` | take the window out of its group |
| `Super+G` | collapse / expand the group (a click on a collapsed frame expands it too) |
| `Super+Shift+G` | fit the whole group on screen |
| `Super+Alt` + left drag | move the whole group |

### Focus and workspaces

| Shortcut | Action |
|---|---|
| `Super+arrows`, `Super+H` / `J` / `K` | focus the window in that direction |
| `Alt+Tab` / `Alt+Shift+Tab` | next / previous window |
| `Super+1` … `Super+0` | go to window 1 … 10, in the order they were opened |
| `Super+Ctrl+1` … `0` | go to workspace 1 … 10 |
| `Super+Shift+1` … `0` | move the window to workspace 1 … 10 |

### Apps and system

| Shortcut | Action |
|---|---|
| `Super+Return` | terminal (`$TERMINAL`, else foot) |
| `Super+D` | app launcher (rofi, else fuzzel) |
| `Super+E` | file manager (nautilus) |
| `Print` | screenshot to `~/Pictures/Screenshots` |
| `Super+Shift+S` | screenshot of an area to the clipboard |
| `Super+Shift+Space` | next keyboard layout |
| volume, mute, brightness, media keys | wpctl, brightnessctl, playerctl |
| `Super+Shift+R` | reload the config |
| `Super+Ctrl+Q` | quit pzwm |

## Settings app: pzwm config

Open **pzwm config** from your app launcher, or run `pzwm-config`. It edits
`~/.config/pzwm/config.toml` with switches, sliders, color pickers and lists.

| Page | What you can change |
|---|---|
| General | autostart commands, scale, cursor theme and size, X11 apps, title bars |
| Input | keyboard layouts and options, key repeat, touchpad, pointer speed |
| Canvas | size of new windows, zoom step and limits, mouse behavior, grid |
| Monitors | connected monitors with their current mode; mode (picked from what the monitor offers), scale, position, mirroring, disabling |
| Appearance | gaps, borders, corners, title bars, shadows, colors |
| Animation | on/off and durations |
| Parallax | wallpaper drift on/off, strength, swing time, following the camera |
| Minimap | position, size, colors |
| Shortcuts | your key bindings and the built-in ones: add, change, turn off, or press *Record* and the keys |
| Mouse | mouse bindings (`Super+scroll`, `Super+left`, …) |

How saving works:
- only the changed lines are rewritten, so your comments and the order of
  the file stay as they are;
- the result is checked with `pzwm --check-config` before it is written;
  a file with an error is never saved;
- the previous file is kept as `config.toml.bak`;
- pzwm applies the changes right away;
- if the file was changed by something else while the app was open, saving
  stops and asks you to reload instead of overwriting that change.

## Configuration file

`~/.config/pzwm/config.toml`, created by the installer. Every option is
optional; what you leave out keeps its default. All options with comments
are in [`resources/default-config.toml`](resources/default-config.toml)
(also printed by `pzwm --print-default-config`).

Changes apply when you save the file. If the file has an error, pzwm keeps
the old settings and shows a notification.

| Section | Controls |
|---|---|
| `[general]` | autostart commands, scale, cursor, X11 apps, systemd session, auto reload |
| `[input]` | keyboard layouts (at most 4), XKB options, key repeat, touchpad, pointer speed |
| `[canvas]` | zoom, panning, snapping, edge tiling, grid, new window size |
| `[[output]]` | one block per monitor: mode, scale, position, mirror, disable |
| `[appearance]` | gaps, borders, corners, title bars, shadows, colors (`"#RRGGBB"` or `"#RRGGBBAA"`) |
| `[animation]` | animations on/off and their length |
| `[parallax]` | wallpaper parallax: `enabled`, `strength`, `period`, `follow_camera`, see below |
| `[minimap]` | minimap position, size, colors |
| `[scratchpad]` | the drop-down window: command, app id, size |
| `[[rule]]` | window rules, see below |
| `[binds]` | keyboard shortcuts: `"Mods+Key" = "action"` |
| `[mouse]` | mouse bindings: `"Mods+Trigger" = "action"` |

Example:

```toml
[general]
autostart = ["waybar", "swaybg -i ~/wall.jpg"]
scale = 1.5

[canvas]
new_window_width = 0.9    # share of the screen, 0.1 … 1.0
new_window_height = 0.9

[binds]
"Super+Return" = "spawn kitty"
"Super+F1" = "profile save work"
"Super+Q" = "none"        # "none" removes a built-in shortcut

[mouse]
"Alt+left" = "move-window"
```

**Shortcut format**: modifiers `Super`, `Ctrl`, `Alt`, `Shift`; keys are xkb
names (`Return`, `a`, `F1`, `Print`, `XF86AudioRaiseVolume`, …). Mouse
triggers: `left`, `right`, `middle`, `back`, `forward`, `scroll`.

**Actions** for `[binds]` (and `pzwm msg`):

| Action | Does |
|---|---|
| `spawn <command>` | run a shell command |
| `close`, `maximize`, `fullscreen`, `pin` | window actions |
| `fit`, `center` | fit the focused window to the screen / center it without zooming |
| `overview`, `minimap`, `home` | overview, minimap, back to the start |
| `zoom-in`, `zoom-out`, `zoom-reset` | zoom |
| `focus left\|right\|up\|down\|next\|prev` | move focus |
| `move-window <dir>`, `resize-window <dir>`, `pan <dir>` | keyboard move / resize / pan |
| `snap left\|right\|up\|down` | half screen, maximize, restore |
| `workspace <1-10>`, `move-to-workspace <1-10>` | workspaces |
| `window <1-10>` | the n-th window in the order it was opened |
| `profile save\|load <name>` | [desktop profiles](#desktop-profiles) |
| `find` | search windows, groups and bookmarks |
| `camera back\|forward` | camera history |
| `bookmark save\|go\|delete <name>` | camera bookmarks (per workspace) |
| `group add [<name>]\|remove\|toggle [<name>]\|fit [<name>]\|move <dir>` | groups |
| `scratchpad` | drop-down window |
| `layout next\|<index>` | keyboard layout |
| `reload-config`, `quit`, `vt <1-12>` | system |

Mouse actions for `[mouse]`: `zoom`, `pan`, `pan-horizontal` (for
`scroll`); `pan`, `move-window`, `resize-window`, `move-group` (for
buttons).

**Window rules** decide how matching windows open. `app_id` and `title`
are patterns where `*` matches anything and `?` one character, ignoring
case; every matching rule applies and later ones win. `pzwm msg windows`
shows app ids.

```toml
[[rule]]
app_id = "pavucontrol"
size = "700x500"          # logical pixels
pin = true

[[rule]]
title = "Picture-in-Picture"
pin = true

[[rule]]
app_id = "org.telegram.desktop"
workspace = 3             # opens there in the background
position = "0,0"          # canvas coordinates of the window's top-left
group = "chat"
```

Options: `workspace`, `position`, `size`, `pin`, `maximize`, `fullscreen`,
`group`, `scratchpad`.

### Wallpaper parallax

```toml
[parallax]
enabled = true
strength = 0.04       # how far it travels, share of the screen width (0.01 .. 0.15)
period = 60.0         # seconds for one full swing left, right and back
follow_camera = true  # also shift against the canvas while panning
```

The wallpaper (any `background` layer surface: awww, swaybg, hyprpaper…)
is zoomed in by `strength`, so its edges never show, and sways slowly on
two mixed sine waves, mostly left and right with a small vertical drift.
With `follow_camera`, panning the canvas moves it a little the other way,
like a far-away backdrop. It repaints about 30 times a second only while
enabled; turning it off in the config stops that at once.

## Workspaces and the bar

There are 10 workspaces. Each monitor shows one; a workspace is shown on
one monitor at a time, and switching to one that is visible elsewhere swaps
them.

The **DankMaterialShell** workspace widget shows workspaces 1 … 10, marks
the ones with windows, and switches on click. pzwm speaks the small
protocol DMS already supports for the Umbriel compositor:

- pzwm exports `UMBRIEL_SOCKET` and sends the workspace and window lists
  over it whenever they change;
- DMS switches workspaces by running `umbriel msg …`, which is a link to
  `pzwm` installed by `install.sh`.

If DMS runs as a systemd service, pzwm starts it with the session. If DMS
was only set up for another compositor, run once:

```sh
systemctl --user add-wants pzwm-session.target dms.service
```

## Monitors

**Display Settings** in the app launcher (or `pzwm-config --page
monitors`) opens the Monitors page of pzwm config: it lists the connected
monitors with their current mode, offers the resolutions and refresh rates
each monitor supports, and applies the changes as soon as you save.

`nwg-displays` (also called "Displays Settings") does **not** work under
pzwm: it only talks to sway, Hyprland, niri and Mango through their own
IPC and quits with "Neither sway, Hyprland, niri nor Mango detected".
Generic tools that use the wlr-output-management protocol do work:
wdisplays, wlr-randr, kanshi.

Monitor names (`eDP-1`, `HDMI-A-1`, …) are shown by `pzwm msg outputs`, in
the log, and on the Monitors page of pzwm config.

```toml
[[output]]
name = "eDP-1"
mode = "2560x1440@165"    # empty: the monitor's preferred mode
scale = 1.5
position = "0,0"          # logical pixels; empty: right of the others

[[output]]
name = "HDMI-A-1"
scale = 1.0
position = "1707,0"       # right of the laptop: 2560 / 1.5
```

**Mirroring** (projector, TV): show a copy of one monitor instead of
extending the desktop.

```toml
[[output]]
name = "HDMI-A-1"
mirror = "eDP-1"
```

A mirror has no workspace or bar of its own and is scaled to fit its
resolution without cropping. Monitor settings apply as soon as the config
is saved (mode, scale, position, mirroring, `disable = true` to turn a
monitor off). Tools such as kanshi or wdisplays change monitors live too;
their changes last until the next config reload.

## Desktop profiles

A profile is a saved arrangement of your apps.

```sh
pzwm msg profile save work     # save the current arrangement as "work"
pzwm msg profile load work     # put everything back
pzwm msg profile list          # list saved profiles
```

- **Saved**: for each window its app, workspace, position, size and
  maximized state; the view of every workspace; which workspace each
  monitor shows.
- **Loading** moves open windows back, starts the apps that are not running
  (with `gtk-launch`, so the app id must match a `.desktop` file) and places
  their windows as they appear, for up to a minute.
- **Stored in** `~/.local/state/pzwm/profiles/<name>.json`. Names may use
  letters, digits, `-` and `_`.
- Bind them to keys (`"Super+F1" = "profile save work"`), or load one at
  login: `autostart = ["pzwm msg profile load work"]`.

## Groups, bookmarks and find

- A **group** is the windows sharing a name. Each workspace draws a
  tinted frame around its members, in a color picked from the name.
  *Collapsing* puts the windows away and leaves only the filled frame;
  click it (or `Super+G` over it) to bring them back. Groups are saved in
  desktop profiles.
- **Bookmarks** remember a camera spot per workspace and survive restarts
  (`~/.local/state/pzwm/bookmarks.json`). Names are free:
  `pzwm msg bookmark save inbox`.
- **Camera history** records where the camera was before every jump, per
  workspace. Plain panning and zooming are not jumps, but the spot you
  panned to is remembered when the next jump leaves it.
- **Find** lists every window (also on other workspaces, minimized or in
  a collapsed group), every group and every bookmark. pzwm draws no text
  itself, so the list is shown by a dmenu-style program: `[general]
  picker`, else rofi, fuzzel or wofi.
- **Scratchpad**: the first press starts `[scratchpad] command` (foot by
  default; for ghostty use `ghostty --class=pzwm-scratchpad`). Any window
  can become it with a `scratchpad = true` rule.

## Screen sharing and screenshots

- **Screen sharing** (browsers, Discord, OBS): install
  `xdg-desktop-portal-wlr`. The installed `pzwm-portals.conf` sends screen
  sharing and screenshot requests to it.
- **Screenshots**: `Print` saves the screen to `~/Pictures/Screenshots`,
  `Super+Shift+S` copies a selected area. Both use `grim` and `slurp`,
  which work through wlr-screencopy.

## Scripting: pzwm msg

`pzwm msg <request>` talks to the running pzwm over a unix socket and prints
the reply. Any [action](#configuration-file) works as a request.

| Request | Reply |
|---|---|
| `pzwm msg workspace 3`, `pzwm msg fit`, `pzwm msg spawn foot` … | runs the action, prints `ok` |
| `pzwm msg windows` | one line per window: id, app, workspace, state, position, size, focus |
| `pzwm msg outputs` | one line per monitor: name, size, position, scale, workspace, camera |
| `pzwm msg windows --json`, `outputs --json` | the same as JSON (windows also with title, group, hidden) |
| `pzwm msg subscribe` | stays connected and prints one JSON event per line (see below) |
| `pzwm msg profile save\|load\|list …` | see [Desktop profiles](#desktop-profiles) |
| `pzwm msg bookmark list`, `group list` | bookmarks and groups with their workspace |
| `pzwm msg heads --json` | every connected monitor (also disabled ones) with the modes it offers |
| `pzwm reload` | re-read the config |
| `pzwm --check-config [FILE]` | check the config (or another file) for errors |
| `pzwm --print-default-config` | print the documented defaults |

A request that fails prints a line starting with `error:` and exits with
status 1.

**Events** from `pzwm msg subscribe`: first
`{"event":"state","windows":[…],"outputs":[…]}`, then `window-opened` /
`window-changed` (with `"window"`), `window-closed` (with `"id"`),
`focus-changed` (`"id"`, null when nothing is focused) and
`workspace-changed` (`"output"`, `"workspace"`). Example:

```sh
pzwm msg subscribe | jq -r 'select(.event=="focus-changed") | .id'
```

**Environment** set for apps started in pzwm:

| Variable | Value |
|---|---|
| `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP` | `pzwm` |
| `WAYLAND_DISPLAY` | pzwm's Wayland socket |
| `PZWM_SOCKET` | `$XDG_RUNTIME_DIR/pzwm-<display>.sock`, used by `pzwm msg` |
| `UMBRIEL_SOCKET` | `$XDG_RUNTIME_DIR/pzwm-bar-<display>.sock`, the bar feed |
| `DISPLAY` | the X11 display of xwayland-satellite, when it runs |

## Works with

pzwm draws only the windows and the canvas. Everything else comes from
other programs:

| Role | Tested with |
|---|---|
| Bar, launcher, notifications, lock screen | [DankMaterialShell](https://github.com/AvengeMedia/DankMaterialShell) (with workspaces) |
| Bar | waybar |
| Launcher | rofi, fuzzel |
| Notifications | mako, swaync |
| Wallpaper | swaybg, awww |
| Lock and idle | hyprlock, hypridle, swayidle |
| Taskbar, dock | waybar `wlr/taskbar`, DankMaterialShell dock |
| Monitor setup | kanshi, wdisplays, wlr-randr, nwg-displays |
| Night light | wlsunset, gammastep |
| Input method | fcitx5 |
| Screenshots, clipboard | grim, slurp, wl-clipboard, cliphist |
| X11 apps | [xwayland-satellite](https://github.com/Supreeeme/xwayland-satellite) |

Start any of them from `[general] autostart`, or as systemd user services
bound to `pzwm-session.target`.

Supported Wayland protocols include xdg-shell, xdg-decoration,
wlr-layer-shell, ext-session-lock, ext-idle-notify, idle-inhibit,
wlr-screencopy, wlr-data-control, primary selection, xdg-activation,
fractional-scale, viewporter, linux-dmabuf, presentation-time,
pointer-constraints, relative-pointer, pointer-gestures, cursor-shape,
keyboard-shortcuts-inhibit, single-pixel-buffer, wlr-foreign-toplevel-management,
ext-foreign-toplevel-list, wlr-output-management, wlr-gamma-control,
text-input-v3, input-method-v2, virtual-keyboard, tablet-v2, xdg-dialog,
xdg-foreign, alpha-modifier, content-type and xdg-system-bell.

## Troubleshooting

| Problem | What to do |
|---|---|
| Something does not work | Read the log: `journalctl --user -t pzwm -b` |
| Config changes are ignored | `pzwm --check-config` shows the error; pzwm keeps the old settings until it is fixed |
| No bar after login | `systemctl --user add-wants pzwm-session.target dms.service`, then log in again |
| Bar shows no workspaces | Check that `umbriel` is installed (`which umbriel`) and that DMS was started by pzwm |
| A keyboard layout is missing | XKB allows at most 4 layouts; the 5th in `layouts` is ignored |
| Screen sharing shows nothing | Install `xdg-desktop-portal-wlr`, then log in again |
| X11 apps do not start | Install `xwayland-satellite` |
| Chrome/Chromium leaves fullscreen video after ~1.5 s | Fixed in pzwm: frames now report presentation feedback (Chromium 151+ quits fullscreen when every frame reads as discarded). Rebuild and log in again |
| Wallpaper on a mirrored TV leaves bars on the right and bottom | Fixed in pzwm: a mirror now draws surfaces at its own scale. Rebuild and log in again |
| A monitor change from kanshi/wdisplays is gone | Runtime changes last until the config is reloaded; put them in `[[output]]` to keep them |
| Displays Settings (nwg-displays) quits at once | It supports only sway, Hyprland, niri and Mango; use **Display Settings** (pzwm config) or wdisplays |
| `Super+/` does nothing | Install rofi, fuzzel or wofi, or set `[general] picker` |
| The scratchpad does not open | Its command needs foot by default; set `[scratchpad] command` |
| Night light does nothing | wlsunset/gammastep need a real session (not `--nested`) |

## For developers

### Project layout

| Path | Contents |
|---|---|
| `src/main.rs` | command line, startup, session environment, event loop |
| `src/state.rs` | compositor state: windows, outputs, workspaces, seat |
| `src/shell.rs` | window management: placement, focus, maximize, pin, snap, initial size |
| `src/navigation.rs` | camera: pan, zoom, fit, overview, focus by direction, workspaces |
| `src/canvas.rs` | camera math and animations |
| `src/input.rs`, `src/grabs.rs` | pointer and touchpad input, drags and resizes |
| `src/bindings.rs`, `src/action.rs` | key bindings and the action list |
| `src/config.rs` | config file parsing and defaults |
| `src/handlers.rs` | Wayland protocol handlers |
| `src/protocols.rs`, `src/tablet.rs` | input methods, tablets and other small protocols |
| `src/toplevels.rs` | window lists for taskbars (foreign-toplevel) |
| `src/output_management.rs`, `src/gamma.rs` | monitor tools and night light |
| `src/rules.rs`, `src/scratchpad.rs` | window rules, drop-down window |
| `src/groups.rs`, `src/jumps.rs`, `src/find.rs` | groups, camera history and bookmarks, find |
| `src/events.rs` | `pzwm msg subscribe` event stream |
| `src/render/` | drawing: element list per monitor, GLSL shaders |
| `src/backend/udev.rs`, `src/backend/udev/` | real session: DRM/KMS monitors, libinput, mirroring, live monitor changes, gamma |
| `src/backend/winit.rs` | `--nested` mode |
| `src/minimap.rs` | the minimap |
| `src/profiles.rs` | desktop profiles |
| `src/bar.rs` | workspace feed for DankMaterialShell |
| `src/ipc.rs` | `pzwm msg` socket |
| `src/screencopy.rs` | wlr-screencopy for screenshots and screen sharing |
| `resources/default-config.toml` | documented default config (built into the binary) |
| `resources/pzwm-config/` | the settings app (Python, GTK 4, libadwaita) |
| `resources/` (other files) | session script, login-screen entry, systemd target, portal config |
| `install.sh`, `scripts/install-deps.sh` | install and dependency scripts |

### Build and test

```sh
cargo build --release                 # compositor
cargo test                            # Rust tests
cargo clippy --all-targets            # lints
cd resources/pzwm-config && python3 -m unittest   # settings app tests
target/release/pzwm --nested          # run the build in a window
```

Logs go to stderr; `RUST_LOG=debug` shows more.

## Similar projects

Other compositors with an infinite canvas, such as driftwm, Nourish and
hevel, are compared in [docs/similar-projects.md](docs/similar-projects.md).

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
| [tracing](https://github.com/tokio-rs/tracing) | logs | MIT |
| [anyhow](https://github.com/dtolnay/anyhow) | errors | MIT or Apache-2.0 |
| [xcursor](https://github.com/esposm03/xcursor-rs) | cursor themes | MIT |
| [PyGObject](https://gitlab.gnome.org/GNOME/pygobject), [GTK](https://gtk.org), [libadwaita](https://gitlab.gnome.org/GNOME/libadwaita) | the pzwm config app (used, not included) | LGPL-2.1-or-later |

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
