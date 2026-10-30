# pzwm - Pan And Zoom Window Manager

![pzwm](docs/screenshot.png)

pzwm is a Wayland compositor (window manager) for Linux, written in Rust on
top of [Smithay](https://github.com/Smithay/smithay). Windows live on one
**infinite canvas** that you pan and zoom like a Figma or Miro board. There
is no tiling mode: put windows anywhere, zoom out to see all of them, and
line some up on a grid over the screen when you want to (`Super+T`).

[Українською →](README.uk.md)

| | |
|---|---|
| **Type** | Wayland compositor, floating windows on an infinite canvas |
| **Language** | Rust 2024 (compositor), Python + GTK 4 (settings app) |
| **Runs on** | Linux, from a login manager or a TTY; `--nested` inside another desktop |
| **Config** | `~/.config/pzwm/config.toml`, or the **pzwm config** app |
| **Status** | Personal project, version 0.1, used daily |
| **License** | GPL-2.0-or-later |

### Two ways to open windows

| Mode | What happens when a window opens | Set it |
|---|---|---|
| **center** (default) | it appears at once in the middle of the view, 90 % × 90 % of the screen | `new_window_placement = "center"` |
| **pick** | it waits until you drag out its area with the mouse, then takes exactly that place and size (Escape: the middle) | `new_window_placement = "pick"` |

Switch in **pzwm config** → Canvas → *Where new windows open*, or in
`~/.config/pzwm/config.toml` under `[canvas]`; it applies as soon as the
file is saved. In either mode, `Super+O` (or `Super+Ctrl` + left drag)
picks the place of just the next window, and dialogs and an app's own
further windows always open by the window they belong to. Details:
[Where new windows open](#where-new-windows-open).

## Contents

1. [What's new](#whats-new)
2. [Features](#features)
3. [Install](#install)
4. [First steps](#first-steps)
5. [How it works](#how-it-works)
6. [Where new windows open](#where-new-windows-open) (size hints, window numbers)
7. [Putting windows on the screen: tile grid, halves, corners](#putting-windows-on-the-screen-tile-grid-halves-corners)
8. [Shortcuts](#shortcuts)
9. [Helper apps](#helper-apps)
10. [Settings app: pzwm config](#settings-app-pzwm-config)
11. [Configuration file](#configuration-file)
12. [Workspaces and the bar](#workspaces-and-the-bar)
13. [Monitors](#monitors)
14. [Desktop profiles](#desktop-profiles)
15. [Groups, bookmarks and find](#groups-bookmarks-and-find)
16. [Drawing, presentation, arrange, semantic zoom](#drawing-presentation-arrange-semantic-zoom)
17. [Screen sharing and screenshots](#screen-sharing-and-screenshots)
18. [Scripting: pzwm msg](#scripting-pzwm-msg)
19. [Works with](#works-with)
20. [Troubleshooting](#troubleshooting)
21. [For developers](#for-developers)
22. [Similar projects](#similar-projects)
23. [Credits](#credits)
24. [License](#license)

## What's new

| Feature | Keys / option | More |
|---|---|---|
| Pick where a new window opens: drag out its area and it takes exactly that area | `Super+O`, `Super+Ctrl` + left drag, `Super+Shift+Return`; every window: `[canvas] new_window_placement = "pick"` | [Where new windows open](#where-new-windows-open) |
| Windows an app opens by itself (Zoom's meeting window, dialogs) open by the one they came from, and the camera fits them; windows you launch are never put there | `[canvas] place_near_siblings` | [Where new windows open](#where-new-windows-open) |
| Ideal size: the display's resolution at 100 % (a bar left out if there is one) or your own `ideal_sizes`; again: back | `Super+Shift+F` (fullscreen moved to `Super+Ctrl+F`) | [Shortcuts](#windows) |
| Size hint: the ideal size (the display's resolution, without a bar if there is one, or your own sizes) pulls a dragged area or a resized window in and shows as an outline | hold `Alt` for a free size; `[canvas] size_hints`, `ideal_sizes` | [Size hints](#size-hints) |
| Tile grid over the screen: drag across cells, any mix of halves, thirds, quarters | `Super+T`; `[canvas] tile_grid` | [Tile grid](#putting-windows-on-the-screen-tile-grid-halves-corners) |
| Tiling at 100 %: tiled windows get real-pixel sizes whatever the zoom (or are pinned) | `[canvas] tile_mode` | [Tile grid](#putting-windows-on-the-screen-tile-grid-halves-corners) |
| Snap thirds and corner quarters | `Super+[` / `]` again: ⅓, ⅔; drop in a corner | [Tile grid](#putting-windows-on-the-screen-tile-grid-halves-corners) |
| Pinning keeps a window's place and visible size; pinned windows stay pinned when tiled | `Super+P` | [Tile grid](#putting-windows-on-the-screen-tile-grid-halves-corners) |
| Window numbers by place (top left first, or top right, columns, opening order), more than 10 | `Super+1` … `0`, hold `Super` for 11, 12, …; `[general] window_numbers` | [Window numbers](#window-numbers) |
| Screen sharing in Discord and browsers keeps streaming (ext-image-copy-capture) | | [Screen sharing](#screen-sharing-and-screenshots) |
| Window rules match apps that set their title late (Chrome, Electron) | `[[rule]]` | [Configuration file](#configuration-file) |
| Find shows readable names (`Infra Monitor · Web app · workspace 3`, the app's name from its `.desktop` file) and zooms in on the window it flies to; `Super+G` again expands the group it just collapsed; the Desktop Profiles name field has focus | | [Groups, bookmarks and find](#groups-bookmarks-and-find) |

## Features

**Canvas and windows**
- Infinite canvas with smooth zoom around the cursor; pan with the mouse,
  touchpad or keyboard. Each workspace remembers its own view.
- `Super+C` fits the focused window: centers it and zooms until it fills
  the screen.
- New windows open at the same size (90 % × 90 % of the screen by default),
  not at whatever size the app remembered.
- **Open a window where you want it**: drag out an area (`Super+O`, or
  `Super+Ctrl` + left drag) and the next window fills it;
  `Super+Shift+Return` does the same and starts a terminal there.
- **Pick mode** (`[canvas] new_window_placement = "pick"`): every new
  window waits until you drag out its area and then takes exactly that
  area; Escape puts it in the middle. Dialogs and an app's further
  windows skip it and open by their parent.
- **Size hints**: while you drag out an area or resize a window, the
  ideal size, the whole screen at 100 % (the display's resolution, the
  bar left out, so the app's pixels match the screen's), pulls it in and
  shows as an outline. A notification names the size; hold Alt for a free
  size.
- **Window numbers by place**: `Super+1` … `Super+0` count the windows
  from the top left (or top right, by columns, or in opening order); hold
  `Super` and type more digits for 11, 12 and on.
- **An app's windows stay together**: a window an app opens by itself
  (Zoom's meeting window, a dialog) opens beside or over the one it came
  from, and the camera fits it, at most at 100 %. A window you launch
  yourself is never put there, even of an app that is already running.
- **Ideal size** (`Super+Shift+F`): the window takes the display's
  resolution at 100 % (a bar left out if there is one), or the first of
  your own `[canvas] ideal_sizes`; again: back to its size and place.
- **Tile grid** (`Super+T`): a 6 × 4 grid over the screen; drag across the
  cells and the window fills them: a half, a third, a quarter, a sixth,
  any block. Several windows side by side however you like.
- Maximize, fullscreen, pin a window to the screen, magnetic edges.
  `Super+[` / `Super+]` snap to the left / right, pressed again ½ → ⅓ →
  ⅔; drop a window on a screen edge for a half, in a corner for a quarter.
- Overview of all windows, and a minimap with live window previews.
- **Wallpaper parallax** (on by default): the wallpaper shifts a little
  against the camera when you pan the canvas, like a far-away backdrop.
  The slow left-right swing on its own is optional (`swing = true`).
- **Drawing on the canvas** (`Super+Shift+D`): freehand notes and arrows
  between windows, in canvas coordinates, saved per workspace.
- **Semantic zoom**: zoomed out below 30 %, windows turn into cards in
  their app's color with the app icon.
- **Presentation** (`Super+F5`): the camera flies through the workspace's
  bookmarks one by one, like slides.
- **Arrange** (`Super+Shift+A` / `Super+Alt+A`): line up the focused group,
  or the whole workspace, as a grid or a row, once. Not a tiling mode.
- **Find** (`Super+/`): search windows, groups and bookmarks by name and
  fly there (zooming in when the window would stay a thumbnail), with
  rofi, fuzzel or wofi as the picker.
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
  that are not running are started and put back in place. The
  **Desktop Profiles** window (`Super+Shift+P`) manages them, and the
  `autosave` profile keeps the last desktop for after a crash.
- Several monitors with their own mode, scale and position, or mirroring
  one monitor onto another (a mirror is scaled to fill a TV of another
  resolution, wallpaper included).
- Screen sharing and screenshots (ext-image-copy-capture and wlr-screencopy,
  xdg-desktop-portal-wlr), live in Discord, browsers and OBS.
- System tray icons in the bar (the DankMaterialShell System Tray widget,
  or waybar's tray), for apps that close to the tray.
- X11 apps through xwayland-satellite.
- Taskbars and docks see and control the windows (foreign-toplevel).
- Monitor tools work live: kanshi, wdisplays, wlr-randr
  (wlr-output-management); night light with wlsunset or gammastep.
- Input methods (fcitx5) and drawing tablets with pressure and tilt.

**Setup**
- **pzwm config**: a settings app for every option, shortcuts included,
  plus the helper launchers **Display Settings** and **Desktop Profiles**.
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
| `/usr/local/share/applications/pzwm-profiles.desktop` | **Desktop Profiles** in your app launcher (only under pzwm) |
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
   With `new_window_placement = "pick"` the window waits: drag out its
   area (Escape: the middle).
4. **Hold `Super` and scroll** to zoom; **hold the middle mouse button and
   drag** to move around.
5. Put windows side by side with `Super+T` (drag across the grid cells),
   or give one the display's resolution at 100 % with `Super+Shift+F`.
6. Open **pzwm config** from the launcher to change settings and shortcuts.

## How it works

| Idea | What it means |
|---|---|
| **Canvas** | An endless 2D plane. Windows have a position on it and keep their real size; zooming only changes how big everything looks. |
| **Camera** | What part of the canvas a monitor shows: a position and a zoom level. Panning and zooming move the camera, not the windows. |
| **Workspace** | One of 10 separate canvases. Each monitor shows one workspace; each workspace keeps its own camera. |
| **Window states** | *Canvas* (normal, moves with the canvas), *pinned* (fixed on screen while the canvas moves under it), *maximized* (fills the screen minus gaps), *fullscreen*. |
| **Size and zoom** | A window's size is in logical pixels (the monitor's resolution divided by its scale). At 100 % zoom one logical pixel of the app is one of the screen, so text is sharp and true to size; zoomed out the same window just looks smaller. That is why the tile grid, `Super+Shift+F` and the size hints work at 100 %: windows placed one after another fit together whatever zoom you were at. |
| **New windows** | Open in the middle of the view, or (pick mode) wait until you drag out their area. Dialogs and an app's own further windows open by the window they belong to. See [Where new windows open](#where-new-windows-open). |
| **Overview** | Zooms out until every window of the workspace is visible; press again to go back. |
| **Minimap** | A small map of the whole workspace with live previews; click a window to jump to it. |
| **Bar, launcher, notifications** | Not part of pzwm. They come from other programs such as DankMaterialShell (see [Works with](#works-with)). |

## Where new windows open

| Window | Opens |
|---|---|
| after you picked an area | filling the area: `Super+O`, then drag a rectangle (Esc cancels); or `Super+Ctrl` + left drag straight away; `Super+Shift+Return` picks an area and starts a terminal in it |
| any other window, with `new_window_placement = "pick"` | nowhere yet: it waits until you drag out its area (the cursor is a crosshair) and then takes exactly that area, size included; a click without dragging keeps it waiting; Escape puts it in the middle of the view. Several new windows wait in turn |
| a dialog (it has a parent window) | centered over its parent; the camera fits it, at most at 100 % |
| a further window an app opens by itself (the same process as a window on the workspace, and you launched nothing just before) | beside that window (right, else below, left, above: the first free side), and the camera fits it, at most at 100 %; a window at most 60 % of the other one's size opens over it, like a dialog. A window you launch (a bind, the launcher, `pzwm msg spawn`, within 20 s) or a new process of the same app is yours and opens like any new window |
| anything else (`new_window_placement = "center"`, the default) | in the middle of the view, 90 % × 90 % of the screen |

- The picked area waits up to a minute for its window and is drawn on the
  canvas until then. Dialogs and fixed-size windows do not take it.
- Window rules with a `position` or `size` still apply; a picked area wins
  over them.
- Side by side placement is on by default; `[canvas] place_near_siblings =
  false` turns it off.
- Picking an area never changes the zoom: you drag at the view you have.
  Each workspace starts at `[canvas] start_zoom` (0.667 by default, so
  about 1.5 screens show), which leaves room around a screen-sized
  window; `Super+Home` goes back to it.
- Pick mode skips windows placed by a rule with a `position` or a
  `workspace`, windows restored from a desktop profile, the scratchpad,
  dialogs and fixed-size windows.
- From scripts: `pzwm msg area`, `area spawn <command>`, `area cancel`,
  `area rect X,Y WxH` (canvas coordinates, no mouse needed). In pick mode
  `area rect` places the waiting window and `area cancel` puts it in the
  middle. `pzwm msg windows --json` marks waiting windows with
  `"awaiting_pick": true`.

### Size hints

While you drag out an area (`Super+O`, pick mode) or resize a window by
an edge, the **ideal size** shows as an outline once you get close, and
within about 24 screen pixels the size snaps onto it. A notification then
names it, for example `1707 × 960 (2560 × 1440 px)`.

By default the ideal size is the display's resolution at 100 % zoom: with
a bar that reserves space (waybar, DMS) its height is left out, without
one it is the whole display. The window then fills the screen and the
app's pixels are the screen's. The notification shows both sizes, for
example `1707 × 960 (2560 × 1440 px)`: logical pixels and the display's
own. When only one edge is dragged, only that side snaps (the full width,
or the full height).

Your own ideal sizes go in `[canvas] ideal_sizes`:

```toml
[canvas]
ideal_sizes = ["screen", "1920x1080", "1280x720"]   # default: ["screen"]
```

`"screen"` is the display as above; `"WIDTHxHEIGHT"` is a window whose
content is that big, in logical pixels (the same as `size` in window
rules). The hints pull to any of them; `Super+Shift+F` uses the first
one, in the middle of the screen when it is smaller. Hold `Alt` while dragging for a free size; `[canvas] size_hints =
false` turns the hints off.

### Window numbers

`Super+1` … `Super+9`, `Super+0` (window 10) jump to a window of the
workspace by number. Keep `Super` held and type more digits for 11, 12
and on: `Super`+`1`, `2` is window 12 (if there are fewer, the last digit
alone counts). `[general] window_numbers` sets the counting:

| `window_numbers` | Counting |
|---|---|
| `"rows"` (default) | rows from the top, each left to right |
| `"rows-rtl"` | rows from the top, each right to left |
| `"columns"` | columns from the left, each top to bottom |
| `"opened"` | the order the windows opened in |

Windows whose tops are within half a window of each other count as one
row. From scripts: `pzwm msg window 12`.

## Putting windows on the screen: tile grid, halves, corners

The windows still live on the canvas; these place one on part of what the
screen shows. So that the result does not depend on how far you zoomed,
`[canvas] tile_mode` decides how:

| `tile_mode` | What happens |
|---|---|
| `"zoom"` (default) | the camera first goes to 100 % (the middle of the screen stays put), then the window gets the cells' size in real pixels: text at 1:1, sharp at the monitor's scale, and windows tiled one after another fit together |
| `"pin"` | the window is pinned to the screen at 1:1 on those cells; the canvas moves under it |
| `"canvas"` | at the current zoom, as before (zoomed out, the window gets a bigger logical size) |

A pinned window stays pinned when tiled or snapped, and `Super+P` keeps a
window where it is and as big as it looks, in both directions.

| How | Result |
|---|---|
| `Super+T`, then drag across cells | the focused window fills the cells. Without the mouse: arrows move the selected block, `Shift`+arrows grow it, `Enter` applies, `Esc` (or right click) closes the grid |
| `Super+[` / `Super+]` | left / right half; pressed again within 3 s: a third, then two thirds |
| drop a dragged window on the left / right edge | that half |
| drop it within 120 px of a corner | that quarter |
| drop it on the top edge | maximize |

Examples with the default 6 × 4 grid: three cells across = a half, two =
a third, three across and two down = a quarter, the whole top row = a
wide strip. So one window can take the right half, another the top-left
quarter, a third the bottom-left quarter, or any other mix.
`[canvas] tile_grid = "4x4"` (up to 12x12) changes the grid.
From scripts: `pzwm msg tile`, then `tile left|right|up|down`,
`tile grow <dir>`, `tile apply`, `tile cancel`.

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
| `Super+Home` | go back to the start of the canvas, at `[canvas] start_zoom` |
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
| `Super+Shift+F` | ideal size: the display's resolution at 100 %, without a bar if there is one, or the first of `[canvas] ideal_sizes` (again: back) |
| `Super+Ctrl+F` | fullscreen / restore |
| `Super+P`, pin button | pin to the screen / back to the canvas |
| `Super+T` | tile grid: drag across cells (or arrows, Shift+arrows, Enter) |
| `Super+[` / `Super+]` | left / right half; again: ⅓, then ⅔ |
| drop on the left or right screen edge / in a corner | fill that half / quarter of the screen |
| drop on the top screen edge | maximize |
| `Super+Q` | close |
| `Super+O`, then drag / `Super+Ctrl` + left drag | pick the area the next window opens in |
| `Super+Shift+Return` | pick an area, then open a terminal in it |
| `Super+Ctrl+arrows` | move the window |
| `Super+Shift+arrows` | resize the window |
| ``Super+` `` | scratchpad: call up or put away the drop-down terminal |

### Groups

| Shortcut | Action |
|---|---|
| `Super+Ctrl+G` | add the window to a group (the picker asks for its name) |
| `Super+Alt+G` | take the window out of its group |
| `Super+G` | collapse / expand the group (a click on a collapsed frame expands it too; pressed again right away it expands the group it just collapsed) |
| `Super+Shift+G` | fit the whole group on screen |
| `Super+Alt` + left drag | move the whole group |

### Draw, present, arrange

| Shortcut | Action |
|---|---|
| `Super+Shift+D` | pen on / off |
| while the pen is on: left drag / `Shift` + left drag / right drag | draw / draw an arrow / erase strokes |
| while the pen is on: `1` … `9`, `Ctrl+Z`, `Delete`, `Esc` | pick a color, undo, clear the workspace, pen off |
| `Super+Alt+D` | hide / show the drawings |
| `Super+F5` | start / end the presentation through the bookmarks |
| `Super+Page Down` / `Super+Page Up` | next / previous slide |
| `Super+Shift+A` / `Super+Alt+A` | arrange the group (or the workspace) as a grid / a row |

### Focus and workspaces

| Shortcut | Action |
|---|---|
| `Super+arrows`, `Super+H` / `J` / `K` | focus the window in that direction |
| `Alt+Tab` / `Alt+Shift+Tab` | next / previous window |
| `Super+1` … `Super+0` | go to window 1 … 10, counted from the top left (`[general] window_numbers`); hold `Super` and type more digits for 11, 12, … |
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
| `Super+Shift+P` | **Desktop Profiles** window |
| `Super+Shift+R` | reload the config |
| `Super+Ctrl+Q` | quit pzwm |

## Helper apps

pzwm comes with three launcher entries. All three are the same program,
`pzwm-config`, opened on a different page; they are listed in your app
launcher (rofi, fuzzel, DMS) and the last two only show up under pzwm.

| Name in the launcher | Command | What it is for |
|---|---|---|
| **pzwm config** | `pzwm-config` | every setting and shortcut, see [below](#settings-app-pzwm-config) |
| **Display Settings** | `pzwm-config --page monitors` | resolution, refresh rate, scale, position and mirroring of monitors, see [Monitors](#monitors) |
| **Desktop Profiles** | `pzwm-config --page profiles`, `Super+Shift+P` | save, load, rename and delete desktop profiles, see [Desktop profiles](#desktop-profiles) |

`--page` takes any page name (`general`, `canvas`, `parallax`, `draw`,
`shortcuts`, …). If the app is already open, it switches to that page.

## Settings app: pzwm config

Open **pzwm config** from your app launcher, or run `pzwm-config`. It edits
`~/.config/pzwm/config.toml` with switches, sliders, color pickers and lists.

| Page | What you can change |
|---|---|
| General | autostart commands, scale, cursor theme and size, X11 apps, title bars, desktop autosave, the find picker, how windows are numbered |
| Input | keyboard layouts and options, key repeat, touchpad, pointer speed |
| Canvas | size of new windows, where they open (center or pick), side by side placement, tile grid and tile mode (100 %, pin, current zoom), size hints, zoom step and limits, mouse behavior, grid, semantic zoom |
| Monitors | connected monitors with their current mode; mode (picked from what the monitor offers), scale, position, mirroring, disabling |
| Appearance | gaps, borders, corners, title bars, shadows, colors |
| Animation | on/off and durations |
| Parallax | on/off, strength, following the camera, the swing on its own (off by default) and its time |
| Drawing | pen width and colors |
| Presentation | seconds per slide (0: by key) |
| Minimap | position, size, colors |
| Shortcuts | your key bindings and the built-in ones: add, change, turn off, or press *Record* and the keys |
| Mouse | mouse bindings (`Super+scroll`, `Super+left`, …) |
| Profiles | desktop profiles: save the desktop under a name, load, replace, rename, delete; acts at once, no Save needed |

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
| `[general]` | autostart commands, scale, cursor, X11 apps, systemd session, auto reload, `window_numbers` |
| `[input]` | keyboard layouts (at most 4), XKB options, key repeat, touchpad, pointer speed |
| `[canvas]` | zoom, panning, snapping, edge tiling, grid, new window size, `new_window_placement` (`center`/`pick`), `place_near_siblings`, `tile_grid`, `tile_mode`, `size_hints`, `ideal_sizes`, `start_zoom` |
| `[[output]]` | one block per monitor: mode, scale, position, mirror, disable |
| `[appearance]` | gaps, borders, corners, title bars, shadows, colors (`"#RRGGBB"` or `"#RRGGBBAA"`) |
| `[animation]` | animations on/off and their length |
| `[parallax]` | wallpaper parallax: `enabled`, `strength`, `follow_camera`, `swing`, `period`, see below |
| `[draw]` | pen `width` and `colors` |
| `[presentation]` | `interval`: seconds per slide, 0 waits for the key |
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
| `ideal-size` | the first ideal size (`[canvas] ideal_sizes`, by default the display without a bar) at 100 %; again: back to the size and place it had |
| `overview`, `minimap`, `home` | overview, minimap, back to the start |
| `zoom-in`, `zoom-out`, `zoom-reset` | zoom |
| `focus left\|right\|up\|down\|next\|prev` | move focus |
| `move-window <dir>`, `resize-window <dir>`, `pan <dir>` | keyboard move / resize / pan |
| `snap left\|right\|up\|down` | left / right (again: ⅓, ⅔), maximize, restore |
| `tile [left\|right\|up\|down\|grow <dir>\|apply\|cancel]` | the tile grid, see [Putting windows on the screen](#putting-windows-on-the-screen-tile-grid-halves-corners) |
| `workspace <1-10>`, `move-to-workspace <1-10>` | workspaces |
| `window <N>` | the n-th window, counted as `[general] window_numbers` says (no upper limit) |
| `profile save\|load\|delete <name>`, `profile rename <old> <new>` | [desktop profiles](#desktop-profiles) |
| `find` | search windows, groups and bookmarks |
| `camera back\|forward` | camera history |
| `bookmark save\|go\|delete <name>` | camera bookmarks (per workspace) |
| `group add [<name>]\|remove\|toggle [<name>]\|fit [<name>]\|move <dir>` | groups |
| `scratchpad` | drop-down window |
| `draw [on\|off\|undo\|clear\|hide\|color N\|color next]` | drawing on the canvas |
| `arrange grid\|row\|column` | line up the focused group or the workspace |
| `area [spawn <command>]\|cancel\|rect X,Y WxH` | pick the area the next window opens in, see [Where new windows open](#where-new-windows-open) |
| `present [start\|stop\|next\|prev]` | presentation through the bookmarks |
| `layout next\|<index>` | keyboard layout |
| `reload-config`, `quit`, `vt <1-12>` | system |

Mouse actions for `[mouse]`: `zoom`, `pan`, `pan-horizontal` (for
`scroll`); `pan`, `move-window`, `resize-window`, `move-group`,
`select-area` (for buttons).

**Window rules** decide how matching windows open. `app_id` and `title`
are patterns where `*` matches anything and `?` one character, ignoring
case; every matching rule applies and later ones win. `pzwm msg windows`
shows app ids. Rules are checked when the window opens and again during
its first 5 seconds (until you move it), so apps that set their title
late, such as Chrome and Electron apps, match too.

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

These are the defaults:

```toml
[parallax]
enabled = true
strength = 0.15       # how far it travels, share of the screen width (0.01 .. 0.15)
follow_camera = true  # shift against the canvas while panning
swing = false         # also sway slowly left and right on its own
period = 60.0         # seconds for one full swing (only with swing = true)
```

The wallpaper (any `background` layer surface: awww, swaybg, hyprpaper…)
is zoomed in by `strength`, so its edges never show. With `follow_camera`,
panning the canvas moves it a little the other way, like a far-away
backdrop; this costs nothing extra, because panning repaints anyway.
`swing = true` adds a slow sway on two mixed sine waves, mostly left and
right with a small vertical drift; only then does pzwm repaint about 30
times a second. `swing = false` with `follow_camera = true` (the default)
keeps only the movement that follows the canvas.

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

**The Desktop Profiles window** (`Super+Shift+P`, **Desktop Profiles** in
the launcher, or the Profiles page of pzwm config):

1. Type a name in *Save the desktop* and press Enter: the desktop as it
   is now is saved under that name.
2. Each saved profile shows its windows, apps, workspaces, drawings and
   when it was saved. **Load** puts it back. The buttons next to it
   replace it with the current desktop, rename it, or delete it (both ask
   first).
3. The page talks to the running pzwm directly; there is nothing to
   Save in the header. When the window opens the name field has focus,
   so `Super+Shift+P`, a name and Enter save the desktop.

From a terminal or a script:

```sh
pzwm msg profile save work          # save the current arrangement as "work"
pzwm msg profile load work          # put everything back
pzwm msg profile list               # names; add --json for details
pzwm msg profile rename work office
pzwm msg profile delete office
```

- **Saved**: for each window its app, workspace, position, size,
  maximized state and group; the view of every workspace; which workspace
  each monitor shows; the drawings on the canvas.
- **Loading** moves open windows back, starts the apps that are not running
  (with `gtk-launch`, so the app id must match a `.desktop` file) and places
  their windows as they appear, for up to a minute.
- **Stored in** `~/.local/state/pzwm/profiles/<name>.json`. Names may use
  letters, digits, `-` and `_`.
- Bind them to keys (`"Super+F1" = "profile save work"`), or load one at
  login: `autostart = ["pzwm msg profile load work"]`.
- **Autosave**: with `[general] autosave = true` (the default) pzwm saves
  the profile `autosave` every 2 minutes and when it quits, but never an
  empty desktop. After a crash, `pzwm msg profile load autosave` (or Load
  in the window) brings your apps back.

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
  picker`, else rofi, fuzzel or wofi. Each line reads `Title · App ·
  workspace N`, the app named as in its `.desktop` file (browser app
  windows show as "Web app"). Jumping to a window that would stay
  a thumbnail (from the overview, say) zooms in on it.
- **Collapse and expand**: `Super+G` collapses the focused window's group
  and hands focus to another window; pressed again right away it expands
  that same group, not the new window's.
- **Scratchpad**: the first press starts `[scratchpad] command` (foot by
  default; for ghostty use `ghostty --class=pzwm-scratchpad`). Any window
  can become it with a `scratchpad = true` rule.

## Drawing, presentation, arrange, semantic zoom

**Drawing** turns the canvas into a whiteboard: notes and arrows that
point from one window to another.

1. `Super+Shift+D` turns the pen on; the cursor becomes a crosshair.
2. Left drag draws; `Shift` + left drag draws a straight arrow; right drag
   erases every stroke it touches. The pen draws over windows too; the bar
   and the minimap still work.
3. `1` … `9` pick a color from `[draw] colors`, `Ctrl+Z` undoes the last
   stroke, `Delete` clears the workspace, `Esc` (or `Super+Shift+D`) turns
   the pen off. While the pen is on, these keys do not reach apps.
4. `Super+Alt+D` hides or shows all drawings.

Strokes live on the canvas, so they pan and zoom with the windows. The
pen width is in screen pixels at the zoom you draw at: zoomed out you
draw thick lines for the big picture, zoomed in thin notes. Drawings are
kept per workspace in `~/.local/state/pzwm/drawings.json` and saved in
desktop profiles.

```toml
[draw]
width = 4.0
colors = ["#f38ba8", "#fab387", "#f9e2af", "#a6e3a1", "#89b4fa", "#cdd6f4"]
```

**Presentation**: save a few bookmarks on a workspace
(`Super+Alt+Shift+1…5`, or `pzwm msg bookmark save intro`), then press
`Super+F5`. The camera flies to each bookmark in turn: numbered ones
first (1, 2, … 10), then the others by name. `Super+Page Down` /
`Super+Page Up` step, `Super+F5` ends the show and flies back to where it
started. With `[presentation] interval = 8` it moves on every 8 seconds
and ends after the last slide.

**Arrange**: `Super+Shift+A` lays out the focused window's group as a
grid (about √n per row), `Super+Alt+A` as one row (`arrange column` is a
column). Outside a group it arranges every canvas window of the
workspace. Windows keep their size and their reading order (top to
bottom, left to right), start at the top-left of where they were, and the
camera fits the result. It happens once; drag windows freely afterwards.

**Semantic zoom**: when you zoom out below `[canvas] semantic_zoom`
(0.3, that is 30 %), the unreadable miniature of each window fades into a
card in its app's color with the app icon in the middle. Icons come from
the app's `.desktop` file and the hicolor theme (PNG only; apps with only
an SVG icon get the card without one). `semantic_zoom = 0` turns it off.

## Screen sharing and screenshots

- **Screen sharing** (browsers, Discord, OBS): install
  `xdg-desktop-portal-wlr`. The installed `pzwm-portals.conf` sends screen
  sharing and screenshot requests to it. pzwm offers
  **ext-image-copy-capture**, which the portal prefers: its older
  wlr-screencopy path in xdg-desktop-portal-wlr 0.8.4 stops after the
  first frame (upstream issue #411). Captures are XRGB8888 or ARGB8888
  shm buffers, and every frame is reported as changed.
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
| `pzwm msg windows --json`, `outputs --json` | the same as JSON (windows also with title, group, hidden, scratchpad, `awaiting_pick` for a window waiting for its place) |
| `pzwm msg subscribe` | stays connected and prints one JSON event per line (see below) |
| `pzwm msg profile save\|load\|delete\|rename\|list …` | see [Desktop profiles](#desktop-profiles); `profile list --json` gives windows, apps, workspaces and save time |
| `pzwm msg draw …`, `present …`, `arrange …` | see [Drawing, presentation, arrange](#drawing-presentation-arrange-semantic-zoom); errors are printed, not only notified |
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
wlr-screencopy, ext-image-copy-capture, ext-image-capture-source,
wlr-data-control, primary selection, xdg-activation,
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
| A new window does not appear | Pick mode is on (`new_window_placement = "pick"`): the window waits for its place. Drag out an area (a click is not enough), or press Escape to put it in the middle |
| The cursor is a crosshair and clicks do not reach apps | An area is being picked or the tile grid is open: drag, or press Escape |
| `Super+Shift+F` no longer makes a window fullscreen | It is the ideal size now; fullscreen is `Super+Ctrl+F` (or bind `"Super+Shift+F" = "fullscreen"` in `[binds]`) |
| A second window of a running app asks for its place | It was launched by you (a bind, the launcher, `pzwm msg spawn`) or is a new process: those always open like a new window. Only windows the app opens by itself go beside their sibling |
| Tiled windows end up different sizes | Use `[canvas] tile_mode = "zoom"` (the default): tiling goes to 100 % first. With `"canvas"` the size depends on the zoom you tile at |
| Window numbers are not in the order I expect | Set `[general] window_numbers` to `rows`, `rows-rtl`, `columns` or `opened` |
| Config changes are ignored | `pzwm --check-config` shows the error; pzwm keeps the old settings until it is fixed |
| No bar after login | `systemctl --user add-wants pzwm-session.target dms.service`, then log in again |
| Bar shows no workspaces | Check that `umbriel` is installed (`which umbriel`) and that DMS was started by pzwm |
| A keyboard layout is missing | XKB allows at most 4 layouts; the 5th in `layouts` is ignored |
| Screen sharing shows nothing | Install `xdg-desktop-portal-wlr`, then log in again |
| Screen sharing (Discord, browser) shows one still picture or nothing | Fixed in pzwm: it now offers ext-image-copy-capture, so xdg-desktop-portal-wlr no longer uses its wlr-screencopy path, which in 0.8.4 stalls after the first frame; captures also come in XRGB8888, the format PipeWire clients ask for. Rebuild and log in again. Check with `journalctl --user -u xdg-desktop-portal-wlr`: it should say `using ext_image_copy_capture` |
| Tray icons are missing in the DMS bar | Add the **System Tray** widget to the bar (DMS settings → Widgets), or `{"id": "systemTray", "enabled": true}` to `rightWidgets` in `~/.config/DankMaterialShell/settings.json`. Apps that close to the tray (Discord, Telegram) show there |
| X11 apps do not start | Install `xwayland-satellite` |
| Chrome/Chromium leaves fullscreen video after ~1.5 s | Fixed in pzwm: frames now report presentation feedback (Chromium 151+ quits fullscreen when every frame reads as discarded). Rebuild and log in again |
| Wallpaper on a mirrored TV leaves bars on the right and bottom | Fixed in pzwm: a mirror now draws surfaces at its own scale. Rebuild and log in again |
| A monitor change from kanshi/wdisplays is gone | Runtime changes last until the config is reloaded; put them in `[[output]]` to keep them |
| Displays Settings (nwg-displays) quits at once | It supports only sway, Hyprland, niri and Mango; use **Display Settings** (pzwm config) or wdisplays |
| A window card shows no icon when zoomed out | The app only ships an SVG icon (or none); semantic zoom reads PNG icons. Set `[canvas] semantic_zoom = 0` to turn the cards off |
| The mouse draws lines instead of clicking | The pen is on: press `Esc` or `Super+Shift+D` |
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
| `src/area.rs` | picked area for the next window, pick mode, placing an app's windows side by side |
| `src/tile.rs` | tile grid, snap ½ → ⅓ → ⅔, corner quarters |
| `src/draw.rs`, `src/render/drawings.rs` | drawing on the canvas: strokes, eraser, saving, rendering |
| `src/present.rs`, `src/arrange.rs` | presentation through bookmarks, one-shot layouts |
| `src/icons.rs`, `src/render/semantic.rs` | app icon lookup, semantic-zoom cards |
| `src/render/parallax.rs` | wallpaper parallax |
| `src/groups.rs`, `src/jumps.rs`, `src/find.rs` | groups, camera history and bookmarks, find |
| `src/events.rs` | `pzwm msg subscribe` event stream |
| `src/render/` | drawing: element list per monitor, GLSL shaders |
| `src/backend/udev.rs`, `src/backend/udev/` | real session: DRM/KMS monitors, libinput, mirroring, live monitor changes, gamma |
| `src/backend/winit.rs` | `--nested` mode |
| `src/minimap.rs` | the minimap |
| `src/profiles.rs` | desktop profiles |
| `src/bar.rs` | workspace feed for DankMaterialShell |
| `src/ipc.rs` | `pzwm msg` socket |
| `src/screencopy.rs`, `src/image_copy.rs` | wlr-screencopy and ext-image-copy-capture for screenshots and screen sharing |
| `resources/default-config.toml` | documented default config (built into the binary) |
| `resources/pzwm-config/` | the settings app (Python, GTK 4, libadwaita); `profiles_page.py` is the Desktop Profiles page |
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
| [png](https://github.com/image-rs/image-png) | app icons for semantic zoom | MIT or Apache-2.0 |
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
