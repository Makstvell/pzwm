# Changelog

All notable changes to pzwm. Newest first; dates are when the change
landed on `main`. pzwm has no tagged release yet, so everything is under
**Unreleased** (version 0.1).

## Unreleased

### 2026-11-03: splits

**Added**
- Splits keep their windows: a member that is maximized, fullscreen or
  at its ideal size leaves its place free and goes back there when
  restored. `Super+C` on a member fits the whole split, again the window.
- A thin outline a little outside each split, brighter while one of its
  windows is focused.
- Drag any window by its title over a split's window: the half it would
  take lights up, and dropping it there adds it to the split. The same
  rearranges members; `Super+Ctrl` + arrows swap a member with its
  neighbour on that side.
- **Splits**: windows tiled together over one area of the canvas, in
  columns that each hold one or more rows. Drag the gap (or the shared
  edge) between two and both resize, like snapped windows in Windows;
  double-click it to share evenly; drag an outer edge to grow the whole
  split; `Super+Alt` + left drag moves it as one piece.
- `Super+S` (`split`) turns the focused window into a split filling the
  view; pressed again on a member it takes the window out.
- While a new window waits for its place, an arrow or `h j k l` puts it
  into the focused window's split on that side: a new column left or
  right, a new row above or below.
- Windows snapped next to each other (halves, thirds, quarters, the tile
  grid) join up into a split by themselves.
- A window that closes, is pinned, moved or dragged out by its title
  leaves its split, and the others take its space.

**Changed**
- A window an app opens by itself keeps the size the app gave it again
  (it was enlarged to the ideal size, which turned helper windows such as
  Zoom's into big empty windows).

### 2026-11-03: moving around with the keyboard

**Added**
- Hold `Super+Alt` + arrows, or `h j k l` like in vim, and the view glides
  across the canvas for as long as the key is down, speeding up a little;
  `Shift` goes three times faster. Two keys at once move diagonally.
  `[canvas] pan_speed` sets the speed (screen pixels per second).
- `Super+N` (`navigate`): a mode where the plain arrows and `h j k l` move
  the view the same way, `+` / `-` zoom, `0` goes home, and `Escape` or
  `Enter` leave it.
- While the map at the cursor is open (`Super+M`), the plain arrows and
  `h j k l` pick windows: left / right step to the previous / next window
  by number (like `Super+1` … `0`, wrapping around), up / down go to the
  window above / below, digits jump straight to that window (typed one
  after another for 11, 12, 122 …, as with `Super`), `Enter`
  closes the map. Also `window next` / `window prev` as actions.

### 2026-11-03: games and calmer window opening

**Added**
- `Super+Escape` always works, even while a game holds the keyboard and
  the mouse: it takes keyboard shortcuts and the pointer back from the
  game (keyboard-shortcuts-inhibit and pointer lock/confine). Click the
  game to give them back.
- `[canvas] super_navigation`: pan and zoom only with Super held
  (Super+scroll, Super+middle or Super+left drag). Plain scrolling and
  middle or left drags then always go to the app or game under the cursor.
- Windows pushed aside to make room for a new one move back when that
  window closes (unless you moved them since).

**Changed**
- Opening a window never changes the zoom. The camera only pans, and only
  when the new window would be completely out of sight.
- While picking a place, **Enter** opens the window in the middle of the
  view at the ideal size, without moving other windows. The arrows and
  `h j k l` still put it right next to the focused window on that side.
- With `new_window_placement = "beside"`, a fullscreen, maximized or
  fitted (`Super+C`) focused window counts as filling the view: the new
  window opens in the middle instead.
- A window an app opens by itself (a second window of the same app) takes
  the ideal size beside the first one; dialog-sized windows still open
  centered over it.

**Fixed**
- The camera flew away when a new window opened, most visibly after
  `fit`: apps such as Ghostty ask to activate a new window before it is
  shown, and pzwm revealed it at its not-yet-chosen position. Activation
  and focus now wait until the window is on screen.

### 2026-10-30: idle, dark mode, placing windows by keyboard

**Added**
- **Idle and sleep** (`[idle]`), replacing hypridle or swayidle: dim,
  screens off (DRM DPMS), lock, and suspend on battery only; the screens
  turn off soon after you lock; a playing video (idle inhibitor) holds it
  all back. pzwm locks before any sleep, the lid and the power menu
  included, through a logind delay inhibitor; `loginctl lock-session`
  locks too. New actions `lock` (`Super+L`) and `dpms on|off|toggle`
  (`Super+Shift+Escape`).
- **Dark mode** for apps (`[appearance] color_scheme = "dark"`): the GTK
  color scheme (GTK 4, libadwaita, browsers, Electron), Adwaita-dark for
  GTK 3, the DankMaterialShell mode, and `QT_QPA_PLATFORMTHEME=qt6ct` for
  Qt apps (`qt_platform_theme`).
- **Place a window beside the focused one** while picking its place: the
  arrows or `h j k l` put it right next to it on that side, at the ideal
  size; the windows already there are pushed along. The same in reading
  order with `new_window_placement = "beside"`.
- **Pick queue card**: while several new windows wait for their place, a
  card next to the cursor shows a live miniature of the current one with
  its app icon, plus one dot per waiting window; the notification says
  "2 of 3" and which apps come next.
- A left click on the bare canvas unfocuses the window.
- `pzwm msg area beside [left|right|up|down]`.

**Fixed**
- Closing the laptop lid did not suspend it when an NVIDIA HDMI port
  stayed "connected" after unplugging: pzwm lit that phantom output, so
  logind counted the laptop as docked. Outputs where no monitor answers
  (no preferred mode) now stay off, and light up once a monitor is there.
- A window waiting for its place no longer takes the keyboard focus.

**Docs and project**
- Relicensed under the **Apache License 2.0**, with a `NOTICE` file and an
  SPDX header in every source file.
- Credits for every dependency (`docs/THIRD-PARTY.md`) and for what the
  demo clips show; ten short demo clips at the top of the README.

### 2026-10-29: picking places, tile grid, ideal size, screen sharing

**Added**
- `new_window_placement = "pick"`: each new window waits until you drag
  out its area and takes exactly that place and size. `Super+O` /
  `Super+Ctrl` + left drag pick the place of just the next window;
  `Super+Shift+Return` picks an area and opens a terminal in it.
- An app's own further windows (and dialogs) open by the window they
  belong to; windows you launch yourself never do.
- Tile grid (`Super+T`, `[canvas] tile_grid`), snapping ½ → ⅓ → ⅔
  (`Super+[` / `Super+]`), corner drops for quarters, `[canvas] tile_mode`.
- Ideal size (`Super+Shift+F`; fullscreen moved to `Super+Ctrl+F`): the
  display's resolution at 100 %, or your own `[canvas] ideal_sizes`, with
  size hints that pull a dragged size onto it (hold Alt for a free size).
- Window numbers by place (`Super+1` … `0`, more digits with Super held),
  `[general] window_numbers`.
- `[canvas] start_zoom` (0.667 by default).
- Drawing on the canvas, semantic zoom (app cards when zoomed far out),
  presentation through bookmarks, arrange, the Desktop Profiles window,
  wallpaper parallax.

**Fixed**
- Screen sharing (Discord, browsers, OBS) keeps streaming: pzwm supports
  ext-image-copy-capture, and wlr-screencopy offers XRGB8888.

### 2026-10-28: first version

- The infinite-canvas Wayland compositor: pan and zoom, floating windows,
  overview, minimap, groups, bookmarks, find, scratchpad, window rules,
  desktop profiles, monitor mirroring, DankMaterialShell workspaces,
  `pzwm msg` scripting, and the **pzwm config** settings app.
