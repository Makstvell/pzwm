# pzwm

**An infinite-canvas Wayland compositor, written in Rust.** Windows float on
an endless canvas that you pan and zoom like a design tool — hold the mouse
wheel and drag, scroll to zoom around the cursor, zoom out to see everything.

[Українською →](README.uk.md)

![pzwm](docs/screenshot.png)

## Features

- **Infinite canvas** with a dot grid that stays readable at any zoom
  (spacing adapts like in Figma).
- **Pan like a designer**: hold the middle button and drag — over empty
  space *and* over windows. A plain middle click still reaches the app
  (paste, open link in a new tab). Left-drag on empty canvas pans too.
- **Zoom around the cursor** with the wheel on empty canvas or `Super+wheel`
  anywhere; pinch on a touchpad; three-finger swipe pans.
- **Snappy, not floaty**: short ease-out animations (110–130 ms), no inertia.
- **Maximize / fullscreen and back**: the window returns to exactly where it
  was on the canvas, with the same size.
- **Move, resize, pin, snap**: drag the titlebar or `Super+drag`; resize from
  any edge; magnetic edges between windows; drop on the left/right screen
  edge to tile a half (top edge maximizes); **pin** a window to the screen so
  the canvas slides under it.
- **Design**: rounded corners (content is clipped too), gradient borders on
  the focused window, soft shadows, a slim titlebar with close / maximize /
  pin buttons. Apps that draw their own titlebar (GTK4) keep it.
- **Works with your bar/shell**: wlr-layer-shell, so waybar,
  DankMaterialShell, rofi, mako, swaync, awww/swaybg work. Systemd units
  bound to `graphical-session.target` start automatically.
- Lock screens (ext-session-lock: hyprlock, DMS), idle (hypridle,
  swayidle), clipboard managers (wlr-data-control: cliphist), screenshots
  (wlr-screencopy: grim, slurp), fractional scaling, pointer lock for games,
  multiple monitors and hybrid GPU laptops, 10 workspaces with their own camera.
- X11 apps through [xwayland-satellite](https://github.com/Supreeeme/xwayland-satellite) when installed.

## Install

You need Rust ≥ 1.85 and the usual Wayland compositor libraries
(libseat, libinput, libudev, mesa/gbm/egl, libdrm, libxkbcommon).

```sh
git clone https://github.com/Makstvell/pzwm
cd pzwm
./install.sh --deps   # installs packages (Arch, Fedora, Debian/Ubuntu, openSUSE, Void), builds, installs
# or, if you already have the libraries:
./install.sh
```

The script builds a release binary and installs (with `sudo`):

| file | purpose |
|---|---|
| `/usr/local/bin/pzwm` | the compositor |
| `/usr/local/bin/pzwm-session` | login wrapper (env + logging to the journal) |
| `/usr/share/wayland-sessions/pzwm.desktop` | entry for GDM/SDDM/greetd/ly… |
| `/usr/local/lib/systemd/user/pzwm-session.target` | starts `graphical-session.target` services |
| `/usr/local/share/xdg-desktop-portal/pzwm-portals.conf` | portal choice |

and copies the documented default config to `~/.config/pzwm/config.toml`
(never overwrites yours). `./install.sh --uninstall` removes it all;
`./install.sh --prefix /usr` installs elsewhere.

Recommended extras: `xwayland-satellite grim slurp wl-clipboard xdg-desktop-portal-gtk`.

### Try it without logging out

```sh
pzwm --nested        # opens pzwm in a window of your current desktop
```

### Start it

Log out and pick **pzwm** in your display manager, or run `pzwm-session`
from a TTY. If something goes wrong your previous session is still there;
logs: `journalctl --user -t pzwm -b`.

## Using it

| | |
|---|---|
| middle-drag (anywhere) / left-drag on empty canvas | pan |
| wheel on empty canvas, `Super+wheel` anywhere, pinch | zoom around the cursor |
| touchpad two fingers on empty canvas / three-finger swipe | pan |
| drag titlebar, `Super+left-drag` | move window (magnetic edges) |
| window edges, `Super+right-drag` | resize |
| drop at left/right/top screen edge | tile half / maximize |
| double-click titlebar, `Super+F` | maximize ↔ restore to the exact old place |
| `Super+Shift+F` | fullscreen ↔ restore |
| `Super+P`, pin button | pin to screen ↔ back to canvas |
| `Super+Tab` / `Super+Z` | overview (fit all windows), again to go back |
| `Super+Home`, `Super+BackSpace` | canvas home, zoom 100 % |
| `Super+=` / `Super+-` | zoom in / out |
| `Super+C` | center the camera on the focused window |
| `Super+arrows`, `Alt+Tab` | focus by direction, cycle (and center) |
| `Super+Ctrl+arrows` / `Super+Shift+arrows` | move / resize by keyboard |
| `Super+Alt+arrows` | pan by keyboard |
| `Super+[` / `Super+]` | tile left / right half |
| `Super+1..0`, `Super+Shift+1..0` | workspace, move window to workspace |
| `Super+Q` | close |
| `Super+Shift+R` | reload config |
| `Super+Ctrl+Q` | quit |
| `Ctrl+Alt+F1..F12` | switch VT |

Everything keyboard-related is configurable.

## Configuration

`~/.config/pzwm/config.toml` — every option is optional and documented in
[`resources/default-config.toml`](resources/default-config.toml)
(also `pzwm --print-default-config`). Check it with `pzwm --check-config`.

```toml
[general]
autostart = ["waybar", "swaybg -i ~/wall.jpg"]
cursor_theme = "Bibata-Modern-Ice"

[input]
layouts = "us,ua"
natural_scroll = true

[[output]]
name = "eDP-1"
scale = 1.5
mode = "2560x1440@165"

[appearance]
active_border_start = "#8b5cf6"
active_border_end = "#22d3ee"
corner_radius = 12.0

[binds]
"Super+Return" = "spawn kitty"
"Super+Q" = "none"          # remove a default
```

### Bars and shells

Anything that uses wlr-layer-shell works. Start it from `autostart`, or —
for shells that ship a systemd user unit wanted by `graphical-session.target`
(DankMaterialShell, many waybar setups) — nothing to do: pzwm starts
`pzwm-session.target`, which brings them up and stops them on exit.

### Scripting

```sh
pzwm msg workspace 3
pzwm msg zoom-in
pzwm msg overview
pzwm msg windows     # list windows
pzwm msg outputs     # outputs, camera position and zoom
```

Any action from the `[binds]` section works as a `msg` request.

## Troubleshooting

- **Logs**: `journalctl --user -t pzwm -b` (or `RUST_LOG=debug pzwm --nested`).
- **Black screen / wrong GPU** on hybrid laptops: pick the GPU explicitly,
  e.g. `export PZWM_DRM_DEVICE=/dev/dri/renderD128` in `~/.config/pzwm/env`.
- **Output names** for `[[output]]` are printed in the log at startup.
- **X11 apps don't start**: install `xwayland-satellite`.

Not implemented yet: output configuration protocol (wlr-randr/kanshi),
DPMS, mirroring, tablet/touchscreen gestures, foreign-toplevel (taskbars).

## Building from source

```sh
cargo build --release
cargo test
./target/release/pzwm --nested
```

Built on [Smithay](https://github.com/Smithay/smithay). Code layout:
`canvas.rs` (camera math), `shell.rs` (window model), `navigation.rs`
(camera, focus, workspaces), `input.rs` / `bindings.rs`, `grabs.rs`
(drag/resize/pan), `render/` (elements and GLSL shaders), `backend/`
(udev for real sessions, winit for nested), `handlers.rs` (protocols),
`screencopy.rs`, `ipc.rs`.

## License

MIT
