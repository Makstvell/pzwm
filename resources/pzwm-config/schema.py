# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Maksym Shymchenko (Makstvell)

"""The options pzwm config can edit, grouped into pages.

Defaults come from `pzwm --print-default-config`; this file only adds
labels, help text and limits. Keep it in sync with src/config.rs.
"""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Option:
    key: str
    title: str
    kind: str  # bool | int | float | str | choice | color | list
    subtitle: str = ""
    low: float = 0
    high: float = 0
    step: float = 1
    choices: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True)
class Page:
    section: str
    title: str
    icon: str
    options: tuple[Option, ...]


PAGES = (
    Page(
        "general",
        "General",
        "preferences-system-symbolic",
        (
            Option("autostart", "Autostart", "list", "Commands run once when pzwm starts"),
            Option("scale", "Scale", "float", "DPI factor for monitors without their own scale", 0.5, 4, 0.05),
            Option("cursor_theme", "Cursor theme", "str"),
            Option("cursor_size", "Cursor size", "int", "", 8, 128),
            Option("xwayland", "X11 apps", "bool", "Run xwayland-satellite when it is installed"),
            Option("prefer_server_decorations", "pzwm draws title bars", "bool", "Apps that refuse keep their own"),
            Option("systemd_session", "Start the systemd session", "bool", "Brings up the bar and portals"),
            Option("auto_reload", "Apply changes on save", "bool"),
            Option("picker", "Picker", "str", "dmenu-style program for Find and new groups; empty: rofi, fuzzel or wofi"),
            Option("window_numbers", "Window numbers (Super+1 … 0)", "choice", "rows: from the top left; rows-rtl: from the top right; columns; opened: in opening order", choices=("rows", "rows-rtl", "columns", "opened")),
            Option("autosave", "Autosave the desktop", "bool", "Profile “autosave” every 2 minutes and on quit, to recover after a crash"),
        ),
    ),
    Page(
        "input",
        "Input",
        "input-keyboard-symbolic",
        (
            Option("layouts", "Keyboard layouts", "str", "Comma separated, at most 4, e.g. us,ua,de"),
            Option("variant", "Layout variants", "str"),
            Option("options", "XKB options", "str", "e.g. grp:alt_shift_toggle,caps:escape"),
            Option("model", "Keyboard model", "str"),
            Option("repeat_rate", "Key repeat rate", "int", "Repeats per second", 1, 100),
            Option("repeat_delay", "Key repeat delay", "int", "Milliseconds", 100, 2000, 10),
            Option("natural_scroll", "Natural scrolling (touchpad)", "bool"),
            Option("mouse_natural_scroll", "Natural scrolling (mouse)", "bool"),
            Option("tap_to_click", "Tap to click", "bool"),
            Option("tap_and_drag", "Tap and drag", "bool"),
            Option("disable_while_typing", "Disable touchpad while typing", "bool"),
            Option("accel_speed", "Pointer speed", "float", "-1 slow … 1 fast", -1, 1, 0.05),
        ),
    ),
    Page(
        "canvas",
        "Canvas",
        "zoom-fit-best-symbolic",
        (
            Option("new_window_width", "New window width", "float", "Share of the screen", 0.1, 1, 0.05),
            Option("new_window_height", "New window height", "float", "Share of the screen", 0.1, 1, 0.05),
            Option("start_zoom", "Start zoom", "float", "Zoom each workspace starts at and Super+Home goes back to; 0.667 shows about 1.5 screens", 0.1, 2, 0.05),
            Option("semantic_zoom", "Semantic zoom below", "float", "Zoom under which windows become app cards; 0 turns it off", 0, 1, 0.05),
            Option("new_window_placement", "Where new windows open", "choice", "pick: drag out the place of each new window; dialogs and an app's further windows open by it", choices=("center", "pick")),
            Option("tile_grid", "Tile grid (Super+T)", "str", "Columns x rows, e.g. 6x4"),
            Option("tile_mode", "Tile and snap at", "choice", "zoom: go to 100 % first (real pixels); pin: pin at 1:1; canvas: the current zoom", choices=("zoom", "pin", "canvas")),
            Option("ideal_sizes", "Ideal sizes", "list", "screen (the display, without a bar) or WIDTHxHEIGHT; Super+Shift+F uses the first"),
            Option("size_hints", "Size hints", "bool", "The ideal size (the whole screen at 100 %) pulls dragged areas and resized windows in; hold Alt for a free size"),
            Option("place_near_siblings", "Open an app's windows side by side", "bool", "A second window of an app opens next to the first; dialogs over it"),
            Option("zoom_step", "Zoom step", "float", "Zoom factor per wheel notch", 1.01, 2, 0.01),
            Option("min_zoom", "Minimum zoom", "float", "", 0.01, 1, 0.01),
            Option("max_zoom", "Maximum zoom", "float", "", 1, 10, 0.1),
            Option("wheel_zooms", "Wheel on empty canvas zooms", "bool", "Off: it pans"),
            Option("left_drag_pans", "Left drag on empty canvas pans", "bool"),
            Option("middle_drag_over_windows", "Middle drag pans over windows", "bool"),
            Option("titlebar_drag", "Drag title bars to move windows", "bool"),
            Option("edge_tiling", "Tile by dropping on a screen edge", "bool"),
            Option("snap_distance", "Magnetic edges", "float", "Screen pixels", 0, 100),
            Option("per_workspace_camera", "Each workspace keeps its own view", "bool"),
            Option("pan_step", "Keyboard pan step", "float", "Pixels", 10, 1000, 10),
            Option("window_step", "Keyboard move / resize step", "int", "Pixels", 1, 500),
            Option("grid", "Background grid", "choice", choices=("dots", "lines", "none")),
            Option("grid_spacing", "Grid spacing", "float", "", 4, 256),
            Option("resize_margin", "Resize grab zone", "float", "Pixels outside window edges", 0, 64),
            Option("resize_corner", "Corner resize zone", "float", "", 0, 128),
        ),
    ),
    Page(
        "appearance",
        "Appearance",
        "applications-graphics-symbolic",
        (
            Option("gaps", "Gaps", "int", "Around maximized and tiled windows", 0, 100),
            Option("border_width", "Border width", "float", "", 0, 20, 0.5),
            Option("corner_radius", "Corner radius", "float", "", 0, 50),
            Option("titlebar", "Title bars", "bool"),
            Option("titlebar_height", "Title bar height", "int", "", 16, 80),
            Option("shadow_size", "Shadow size", "float", "", 0, 100),
            Option("shadow_color", "Shadow", "color"),
            Option("background", "Background", "color"),
            Option("grid_color", "Grid", "color"),
            Option("active_border_start", "Active border (start)", "color"),
            Option("active_border_end", "Active border (end)", "color"),
            Option("inactive_border", "Inactive border", "color"),
            Option("titlebar_color", "Title bar", "color"),
            Option("titlebar_active_color", "Active title bar", "color"),
            Option("pinned_accent", "Pinned window accent", "color"),
        ),
    ),
    Page(
        "scratchpad",
        "Scratchpad",
        "utilities-terminal-symbolic",
        (
            Option("command", "Command", "str", "Started by the scratchpad key when it is not running"),
            Option("app_id", "App id", "str", "Windows with this app id become the scratchpad"),
            Option("width", "Width", "float", "Share of the screen", 0.1, 1, 0.05),
            Option("height", "Height", "float", "Share of the screen", 0.1, 1, 0.05),
        ),
    ),
    Page(
        "animation",
        "Animation",
        "media-playback-start-symbolic",
        (
            Option("enabled", "Animations", "bool"),
            Option("window_ms", "Window animations", "int", "Open, close, maximize (ms)", 0, 1000, 10),
            Option("camera_ms", "Camera animations", "int", "Zoom, overview, center (ms)", 0, 1000, 10),
        ),
    ),
    Page(
        "parallax",
        "Parallax",
        "preferences-desktop-wallpaper-symbolic",
        (
            Option("enabled", "Wallpaper parallax", "bool", "The background moves a little behind the canvas"),
            Option("strength", "Strength", "float", "Travel, share of the screen width", 0.01, 0.15, 0.01),
            Option("follow_camera", "Follow the camera", "bool", "Shift against the canvas while panning"),
            Option("swing", "Swing", "bool", "Also sway slowly left and right on its own"),
            Option("period", "Swing time", "float", "Seconds for one full swing", 10, 300, 5),
        ),
    ),
    Page(
        "draw",
        "Drawing",
        "document-edit-symbolic",
        (
            Option("width", "Pen width", "float", "Screen pixels at the zoom you draw at", 1, 30, 0.5),
            Option("colors", "Pen colors", "list", "#RRGGBB each; keys 1 … 9 pick one while drawing"),
        ),
    ),
    Page(
        "presentation",
        "Presentation",
        "x-office-presentation-symbolic",
        (
            Option("interval", "Seconds per slide", "float", "0 waits for Super+Page Down", 0, 600, 1),
        ),
    ),
    Page(
        "minimap",
        "Minimap",
        "find-location-symbolic",
        (
            Option("enabled", "Minimap", "bool"),
            Option(
                "position",
                "Position",
                "choice",
                "cursor: Super+M opens it at the pointer",
                choices=("cursor", "bottom-right", "bottom-left", "top-right", "top-left"),
            ),
            Option("width", "Corner map width", "float", "", 80, 800, 10),
            Option("popup_width", "Map at the cursor width", "float", "", 160, 1600, 10),
            Option("background", "Background", "color"),
            Option("window_color", "Windows", "color"),
            Option("focused_color", "Focused window", "color"),
            Option("viewport_color", "Visible area", "color"),
        ),
    ),
)

# Actions offered when editing a key binding (see the [binds] docs).
KEY_ACTIONS = (
    "spawn ",
    "close",
    "fit",
    "center",
    "maximize",
    "fullscreen",
    "pin",
    "overview",
    "minimap",
    "home",
    "zoom-in",
    "zoom-out",
    "zoom-reset",
    "focus next",
    "focus left",
    "move-window left",
    "resize-window right",
    "snap left",
    "workspace 1",
    "move-to-workspace 1",
    "window 1",
    "layout next",
    "profile save work",
    "profile load work",
    "find",
    "scratchpad",
    "camera back",
    "camera forward",
    "bookmark save 1",
    "bookmark go 1",
    "group add",
    "group remove",
    "group toggle",
    "group fit",
    "draw",
    "draw hide",
    "draw undo",
    "draw clear",
    "arrange grid",
    "arrange row",
    "arrange column",
    "area",
    "area spawn ",
    "area cancel",
    "tile",
    "ideal-size",
    "present",
    "present next",
    "present prev",
    "profile delete work",
    "spawn pzwm-config --page profiles",
    "reload-config",
    "none",
)

MOUSE_ACTIONS = ("zoom", "pan", "pan-horizontal", "move-window", "resize-window", "move-group", "select-area", "none")

OUTPUT_KEYS = ("name", "mode", "scale", "position", "mirror", "disable")
