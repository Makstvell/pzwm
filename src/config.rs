//! User configuration: `~/.config/pzwm/config.toml`.
//!
//! Every field has a default, so an empty file (or no file at all) gives a
//! working compositor. Unknown keys are ignored to keep old configs loading.

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};
use tracing::warn;

use crate::{
    action::Action,
    input::{BTN_BACK, BTN_FORWARD, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT},
    rules::WindowRule,
};

pub const DEFAULT_CONFIG: &str = include_str!("../resources/default-config.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub input: Input,
    pub canvas: Canvas,
    pub animation: Animation,
    pub parallax: Parallax,
    pub draw: Draw,
    pub presentation: Presentation,
    pub appearance: Appearance,
    pub minimap: Minimap,
    pub scratchpad: Scratchpad,
    #[serde(rename = "output")]
    pub outputs: Vec<OutputConfig>,
    #[serde(rename = "rule")]
    pub rules: Vec<WindowRule>,
    pub binds: HashMap<String, String>,
    #[serde(skip)]
    pub keybinds: Vec<Keybind>,
    pub mouse: HashMap<String, String>,
    #[serde(skip)]
    pub mousebinds: Vec<MouseBind>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct General {
    /// Commands run once when the compositor starts (through `sh -c`).
    pub autostart: Vec<String>,
    /// Start `pzwm-session.target` so systemd user services bound to
    /// `graphical-session.target` (DankMaterialShell, portals, ...) come up.
    pub systemd_session: bool,
    /// Start `xwayland-satellite` for X11 apps when it is installed.
    pub xwayland: bool,
    pub cursor_theme: String,
    pub cursor_size: u32,
    /// Ask clients to drop their own titlebars so pzwm draws its own.
    pub prefer_server_decorations: bool,
    /// Scale (DPI factor) for outputs without their own `[[output]] scale`.
    pub scale: f64,
    /// Re-read the config as soon as the file is saved.
    pub auto_reload: bool,
    /// dmenu-style picker for `find` (reads lines, prints the chosen one).
    /// Empty: rofi, else fuzzel, else wofi.
    pub picker: String,
    /// Save the desktop as the profile `autosave` every few minutes, so it
    /// can be put back after a crash.
    pub autosave: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Scratchpad {
    /// Started by `scratchpad` when no scratchpad window exists yet.
    pub command: String,
    /// Windows with this app id become the scratchpad (rules can add more).
    pub app_id: String,
    /// Size as a share (0.1 … 1.0) of the usable screen area.
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Input {
    pub layouts: String,
    pub variant: String,
    pub options: String,
    pub model: String,
    pub repeat_rate: i32,
    pub repeat_delay: i32,
    pub natural_scroll: bool,
    pub tap_to_click: bool,
    pub tap_and_drag: bool,
    pub disable_while_typing: bool,
    /// Pointer speed from -1.0 to 1.0.
    pub accel_speed: f64,
    pub mouse_natural_scroll: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Canvas {
    /// Zoom multiplier per mouse-wheel notch.
    pub zoom_step: f64,
    pub min_zoom: f64,
    pub max_zoom: f64,
    /// Mouse wheel over empty canvas zooms (true) or pans (false).
    pub wheel_zooms: bool,
    /// Left-drag on empty canvas pans like the middle button does.
    pub left_drag_pans: bool,
    /// Middle-drag pans even over windows; a middle click without moving
    /// still reaches the app (paste, open link in new tab).
    pub middle_drag_over_windows: bool,
    /// Distance in screen pixels at which dragged windows snap to edges.
    pub snap_distance: f64,
    /// Dragging a window to the left/right screen edge tiles it to a half.
    pub edge_tiling: bool,
    /// Pixels the canvas moves per keyboard pan step.
    pub pan_step: f64,
    /// Pixels a window moves/resizes per keyboard step.
    pub window_step: i32,
    /// Grid drawn on the canvas: "dots", "lines" or "none".
    pub grid: String,
    pub grid_spacing: f64,
    /// Each workspace remembers its own camera (true) or all share one.
    pub per_workspace_camera: bool,
    /// Dragging the titlebar moves the window. Off: windows stay where they
    /// are and only move with the `move-window` mouse bind.
    pub titlebar_drag: bool,
    /// Invisible grab zone outside window edges for resizing, screen pixels.
    pub resize_margin: f64,
    /// Length of the diagonal (corner) resize zone along each edge.
    pub resize_corner: f64,
    /// Size new windows open at, as a fraction (0.1 .. 1.0) of the usable
    /// screen area. Dialogs and fixed-size windows keep their own size.
    pub new_window_width: f64,
    pub new_window_height: f64,
    /// Below this zoom windows turn into cards with their app icon
    /// (0 turns it off).
    pub semantic_zoom: f64,
    /// A new window of an app that already has one on the workspace opens
    /// beside it (dialog-sized ones over it) instead of in the middle.
    pub place_near_siblings: bool,
    /// Where a new window opens: "center" (the middle of the view) or
    /// "pick" (drag out its place first, see area.rs).
    pub new_window_placement: String,
    /// Cells of the `tile` grid, "COLSxROWS".
    pub tile_grid: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Minimap {
    pub enabled: bool,
    /// "cursor" (hidden until called, opens under the pointer),
    /// "bottom-right", "bottom-left", "top-right" or "top-left" (always on).
    pub position: String,
    /// Width in logical pixels; height follows the screen's aspect ratio.
    pub width: f64,
    /// Width of the map called up at the cursor.
    pub popup_width: f64,
    #[serde(deserialize_with = "de_color")]
    pub background: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub window_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub focused_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub viewport_color: [f32; 4],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Animation {
    pub enabled: bool,
    /// Window open/close/maximize animation length.
    pub window_ms: u64,
    /// Camera (zoom/fit/center) animation length.
    pub camera_ms: u64,
}

/// The wallpaper drifts slowly left and right behind the windows.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Parallax {
    pub enabled: bool,
    /// How far the wallpaper may travel, as a share of the screen width
    /// (it is zoomed in by as much so no edge ever shows).
    pub strength: f64,
    /// Slow left-right swing on its own, independent of the camera.
    pub swing: bool,
    /// Seconds for one full swing left, right and back.
    pub period: f64,
    /// Also shift a little against the camera when the canvas is panned.
    pub follow_camera: bool,
}

/// Drawing on the canvas, see draw.rs.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Draw {
    /// Pen width in screen pixels at the zoom you draw at.
    pub width: f64,
    /// Pen colors; keys 1 … 9 pick one while drawing.
    #[serde(deserialize_with = "de_colors")]
    pub colors: Vec<[f32; 4]>,
}

/// `present`: the camera walks through the workspace's bookmarks.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Presentation {
    /// Seconds per slide before moving on by itself; 0 waits for a key.
    pub interval: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub gaps: i32,
    pub border_width: f32,
    pub corner_radius: f32,
    pub titlebar: bool,
    pub titlebar_height: i32,
    pub shadow_size: f32,
    #[serde(deserialize_with = "de_color")]
    pub shadow_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub background: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub grid_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub active_border_start: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub active_border_end: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub inactive_border: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub titlebar_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub titlebar_active_color: [f32; 4],
    #[serde(deserialize_with = "de_color")]
    pub pinned_accent: [f32; 4],
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub name: String,
    pub scale: Option<f64>,
    /// "2560x1440@165" or "2560x1440"; preferred mode when absent.
    pub mode: Option<String>,
    /// "x,y" in logical pixels; outputs are laid out left to right when absent.
    pub position: Option<String>,
    pub disable: bool,
    /// Name of another output to show a copy of instead of its own area.
    pub mirror: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Keybind {
    pub mods: Mods,
    pub sym: Keysym,
    pub action: Action,
}

/// What a mouse bind reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTrigger {
    Button(u32),
    Scroll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// Drag: pan the canvas. Scroll: pan along the scroll direction.
    Pan,
    /// Scroll: pan sideways with a plain wheel.
    PanHorizontal,
    /// Scroll: the wheel zooms around the cursor, a touchpad pans.
    Zoom,
    MoveWindow,
    ResizeWindow,
    /// Drag: move the window's whole group (the window alone without one).
    MoveGroup,
    /// Drag: pick the area the next window opens in, see area.rs.
    SelectArea,
}

impl MouseAction {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_lowercase().as_str() {
            "pan" => Self::Pan,
            "pan-horizontal" => Self::PanHorizontal,
            "zoom" => Self::Zoom,
            "move-window" | "move" => Self::MoveWindow,
            "resize-window" | "resize" => Self::ResizeWindow,
            "move-group" => Self::MoveGroup,
            "select-area" | "area" => Self::SelectArea,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MouseBind {
    pub mods: Mods,
    pub trigger: MouseTrigger,
    pub action: MouseAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub logo: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub fn from_state(state: &ModifiersState) -> Self {
        Self {
            logo: state.logo,
            ctrl: state.ctrl,
            alt: state.alt,
            shift: state.shift,
        }
    }
}

/// Field-level defaults used by serde for keys missing from a file. The
/// complete built-in setup (with key bindings) is [`Config::builtin`].
impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            input: Input::default(),
            canvas: Canvas::default(),
            animation: Animation::default(),
            parallax: Parallax::default(),
            draw: Draw::default(),
            presentation: Presentation::default(),
            appearance: Appearance::default(),
            minimap: Minimap::default(),
            scratchpad: Scratchpad::default(),
            outputs: Vec::new(),
            rules: Vec::new(),
            binds: HashMap::new(),
            keybinds: Vec::new(),
            mouse: HashMap::new(),
            mousebinds: Vec::new(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: Vec::new(),
            systemd_session: true,
            xwayland: true,
            cursor_theme: std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into()),
            cursor_size: std::env::var("XCURSOR_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            prefer_server_decorations: true,
            scale: 1.0,
            auto_reload: true,
            picker: String::new(),
            autosave: true,
        }
    }
}

impl Default for Scratchpad {
    fn default() -> Self {
        Self {
            command: "foot --app-id pzwm-scratchpad".into(),
            app_id: "pzwm-scratchpad".into(),
            width: 0.6,
            height: 0.5,
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Self {
            layouts: "us".into(),
            variant: String::new(),
            options: String::new(),
            model: String::new(),
            repeat_rate: 25,
            repeat_delay: 600,
            natural_scroll: true,
            tap_to_click: true,
            tap_and_drag: true,
            disable_while_typing: true,
            accel_speed: 0.0,
            mouse_natural_scroll: false,
        }
    }
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            zoom_step: 1.12,
            min_zoom: 0.08,
            max_zoom: 3.0,
            wheel_zooms: true,
            left_drag_pans: true,
            middle_drag_over_windows: true,
            snap_distance: 14.0,
            edge_tiling: true,
            pan_step: 160.0,
            window_step: 40,
            grid: "dots".into(),
            grid_spacing: 32.0,
            per_workspace_camera: true,
            titlebar_drag: true,
            resize_margin: 12.0,
            resize_corner: 28.0,
            new_window_width: 0.9,
            new_window_height: 0.9,
            semantic_zoom: 0.3,
            place_near_siblings: true,
            new_window_placement: "center".into(),
            tile_grid: "6x4".into(),
        }
    }
}

impl Default for Minimap {
    fn default() -> Self {
        Self {
            enabled: true,
            position: "cursor".into(),
            width: 220.0,
            popup_width: 560.0,
            background: rgba(0x0f1117d9),
            window_color: rgba(0x4b5263ff),
            focused_color: rgba(0x22d3eeff),
            viewport_color: rgba(0x8b5cf6ff),
        }
    }
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            enabled: true,
            window_ms: 130,
            camera_ms: 110,
        }
    }
}

impl Default for Parallax {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 0.15,
            swing: false,
            period: 60.0,
            follow_camera: true,
        }
    }
}

impl Default for Draw {
    fn default() -> Self {
        Self {
            width: 4.0,
            colors: [
                0xf38ba8ff, 0xfab387ff, 0xf9e2afff, 0xa6e3a1ff, 0x89b4faff, 0xcdd6f4ff,
            ]
            .into_iter()
            .map(rgba)
            .collect(),
        }
    }
}

impl Default for Presentation {
    fn default() -> Self {
        Self { interval: 0.0 }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            gaps: 10,
            border_width: 2.0,
            corner_radius: 12.0,
            titlebar: true,
            titlebar_height: 30,
            shadow_size: 26.0,
            shadow_color: [0.0, 0.0, 0.0, 0.55],
            background: rgba(0x0b0d12ff),
            grid_color: rgba(0xffffff1c),
            active_border_start: rgba(0x8b5cf6ff),
            active_border_end: rgba(0x22d3eeff),
            inactive_border: rgba(0x353b48ff),
            titlebar_color: rgba(0x14161dff),
            titlebar_active_color: rgba(0x1b1e29ff),
            pinned_accent: rgba(0xf59e0bff),
        }
    }
}

fn rgba(v: u32) -> [f32; 4] {
    [
        ((v >> 24) & 0xff) as f32 / 255.0,
        ((v >> 16) & 0xff) as f32 / 255.0,
        ((v >> 8) & 0xff) as f32 / 255.0,
        (v & 0xff) as f32 / 255.0,
    ]
}

/// Parses `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<[f32; 4]> {
    let hex = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(rgba((v << 8) | 0xff)),
        8 => Some(rgba(v)),
        _ => None,
    }
}

fn de_color<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; 4], D::Error> {
    let s = String::deserialize(d)?;
    parse_color(&s).ok_or_else(|| {
        serde::de::Error::custom(format!("bad color `{s}`, use #RRGGBB or #RRGGBBAA"))
    })
}

fn de_colors<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<[f32; 4]>, D::Error> {
    let list = Vec::<String>::deserialize(d)?;
    list.iter()
        .map(|s| {
            parse_color(s).ok_or_else(|| {
                serde::de::Error::custom(format!("bad color `{s}`, use #RRGGBB or #RRGGBBAA"))
            })
        })
        .collect()
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("pzwm").join("config.toml")
    }

    /// Loads the user config, falling back to defaults on any error so a
    /// typo never leaves the user without a desktop.
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match Self::parse(&text) {
                Ok(cfg) => cfg,
                Err(err) => {
                    warn!("config {}: {err}; using defaults", path.display());
                    Self::builtin()
                }
            },
            Err(_) => Self::builtin(),
        }
    }

    /// Like [`Config::load`], but reports a broken file instead of falling
    /// back, so a reload can keep the config that is running now.
    pub fn try_load() -> Result<Self, String> {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|err| format!("{}: {err}", path.display())),
            Err(_) => Ok(Self::builtin()),
        }
    }

    pub fn builtin() -> Self {
        Self::parse(DEFAULT_CONFIG).expect("built-in default config must parse")
    }

    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Config>(text).map(Config::finish)
    }

    /// Binds from the file are layered over the built-in ones, so a user only
    /// lists what they change. A bind set to "none" removes the default.
    fn finish(mut self) -> Self {
        let mut binds: HashMap<String, String> = toml::from_str::<DefaultBinds>(DEFAULT_CONFIG)
            .map(|d| d.binds)
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (normalize_combo(&k), v))
            .collect();
        for (k, v) in std::mem::take(&mut self.binds) {
            binds.insert(normalize_combo(&k), v);
        }

        self.keybinds = binds
            .iter()
            .filter_map(|(combo, action)| {
                let (mods, sym) = parse_combo(combo).or_else(|| {
                    warn!("unknown key combination `{combo}`");
                    None
                })?;
                let action = Action::parse(action).or_else(|| {
                    if action.trim() != "none" {
                        warn!("unknown action `{action}` for `{combo}`");
                    }
                    None
                })?;
                Some(Keybind { mods, sym, action })
            })
            .collect();
        self.binds = binds;

        let mut mouse: HashMap<String, String> = toml::from_str::<DefaultBinds>(DEFAULT_CONFIG)
            .map(|d| d.mouse)
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (normalize_combo(&k), v))
            .collect();
        for (k, v) in std::mem::take(&mut self.mouse) {
            mouse.insert(normalize_combo(&k), v);
        }
        self.mousebinds = mouse
            .iter()
            .filter_map(|(combo, action)| {
                let (mods, trigger) = parse_mouse_combo(combo).or_else(|| {
                    warn!("unknown mouse combination `{combo}`");
                    None
                })?;
                let action = MouseAction::parse(action).or_else(|| {
                    if action.trim() != "none" {
                        warn!("unknown mouse action `{action}` for `{combo}`");
                    }
                    None
                })?;
                Some(MouseBind {
                    mods,
                    trigger,
                    action,
                })
            })
            .collect();
        self.mouse = mouse;
        for problem in self.rules.iter().flat_map(WindowRule::problems) {
            warn!("{problem}");
        }
        self
    }

    pub fn output(&self, name: &str) -> Option<&OutputConfig> {
        self.outputs.iter().find(|o| o.name == name)
    }

    /// Scale for an output: its own `scale`, else `[general] scale`.
    pub fn output_scale(&self, name: &str) -> f64 {
        self.output(name)
            .and_then(|o| o.scale)
            .unwrap_or(self.general.scale)
            .clamp(0.5, 4.0)
    }

    pub fn mouse_bind(&self, mods: Mods, trigger: MouseTrigger) -> Option<MouseAction> {
        self.mousebinds
            .iter()
            .find(|b| b.mods == mods && b.trigger == trigger)
            .map(|b| b.action)
    }
}

impl OutputConfig {
    /// Parses `position = "x,y"`.
    pub fn position(&self) -> Option<(i32, i32)> {
        let (x, y) = self.position.as_deref()?.split_once(',')?;
        Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DefaultBinds {
    binds: HashMap<String, String>,
    mouse: HashMap<String, String>,
}

/// Canonical form so "Shift+Super+q" and "super + shift + Q" are one key.
fn normalize_combo(combo: &str) -> String {
    let mut parts: Vec<String> = combo.split('+').map(|p| p.trim().to_string()).collect();
    let key = parts.pop().unwrap_or_default();
    let mut mods: Vec<String> = parts.iter().map(|m| m.to_lowercase()).collect();
    mods.sort();
    let key = if key.chars().count() == 1 {
        key.to_lowercase()
    } else {
        key
    };
    mods.push(key);
    mods.join("+")
}

fn parse_mods<'a>(parts: impl Iterator<Item = &'a str>) -> Option<Mods> {
    let mut mods = Mods::default();
    for m in parts {
        match m.to_lowercase().as_str() {
            "super" | "mod" | "logo" | "win" => mods.logo = true,
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "mod1" => mods.alt = true,
            "shift" => mods.shift = true,
            _ => return None,
        }
    }
    Some(mods)
}

/// Parses "Super+Shift+left" or "Super+scroll" into modifiers and a trigger.
pub fn parse_mouse_combo(combo: &str) -> Option<(Mods, MouseTrigger)> {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let key = parts.pop()?;
    let mods = parse_mods(parts.into_iter())?;
    let trigger = match key.to_lowercase().as_str() {
        "left" => MouseTrigger::Button(BTN_LEFT),
        "right" => MouseTrigger::Button(BTN_RIGHT),
        "middle" => MouseTrigger::Button(BTN_MIDDLE),
        "back" | "side" => MouseTrigger::Button(BTN_BACK),
        "forward" | "extra" => MouseTrigger::Button(BTN_FORWARD),
        "scroll" | "wheel" => MouseTrigger::Scroll,
        _ => return None,
    };
    Some((mods, trigger))
}

/// Parses "Super+Shift+Return" into modifiers and a keysym.
pub fn parse_combo(combo: &str) -> Option<(Mods, Keysym)> {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let key = parts.pop()?;
    let mods = parse_mods(parts.into_iter())?;
    let mut sym = xkb::keysym_from_name(key, xkb::KEYSYM_NO_FLAGS);
    if sym.raw() == 0 {
        sym = xkb::keysym_from_name(key, xkb::KEYSYM_CASE_INSENSITIVE);
    }
    if sym.raw() == 0 {
        return None;
    }
    // Bindings are matched on the unshifted (lowercase) symbol.
    Some((mods, keysym_to_lower(sym)))
}

fn keysym_to_lower(sym: Keysym) -> Keysym {
    match sym.raw() {
        raw @ 0x41..=0x5a => Keysym::from(raw + 0x20),
        _ => sym,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses_with_binds() {
        let cfg = Config::builtin();
        assert!(cfg.keybinds.len() > 20);
        assert!(cfg.appearance.corner_radius > 0.0);
    }

    #[test]
    fn colors_parse_with_and_without_alpha() {
        assert_eq!(parse_color("#ff0000"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("#00000000"), Some([0.0, 0.0, 0.0, 0.0]));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn combos_parse_case_insensitively() {
        let (mods, sym) = parse_combo("Super+Shift+Return").unwrap();
        assert!(mods.logo && mods.shift && !mods.ctrl);
        assert_eq!(sym, Keysym::Return);
        let (_, q) = parse_combo("SUPER+Q").unwrap();
        assert_eq!(q, Keysym::q);
        assert!(parse_combo("Hyper+x").is_none());
    }

    #[test]
    fn user_binds_override_and_remove_defaults() {
        let cfg =
            Config::parse("[binds]\n\"super + q\" = \"none\"\n\"Super+Y\" = \"close\"\n").unwrap();
        let (m, q) = parse_combo("Super+q").unwrap();
        assert!(!cfg.keybinds.iter().any(|k| k.mods == m && k.sym == q));
        let (m, y) = parse_combo("Super+y").unwrap();
        assert!(
            cfg.keybinds
                .iter()
                .any(|k| k.mods == m && k.sym == y && k.action == Action::Close)
        );
    }

    #[test]
    fn mouse_binds_parse_and_layer_over_defaults() {
        let cfg = Config::parse(
            "[mouse]\n\"Super+left\" = \"none\"\n\"Alt+right\" = \"resize-window\"\n",
        )
        .unwrap();
        let sup = Mods {
            logo: true,
            ..Mods::default()
        };
        let sup_shift = Mods { shift: true, ..sup };
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(cfg.mouse_bind(sup, MouseTrigger::Button(BTN_LEFT)), None);
        assert_eq!(
            cfg.mouse_bind(sup_shift, MouseTrigger::Button(BTN_LEFT)),
            Some(MouseAction::MoveWindow)
        );
        assert_eq!(
            cfg.mouse_bind(sup, MouseTrigger::Scroll),
            Some(MouseAction::Zoom)
        );
        assert_eq!(
            cfg.mouse_bind(alt, MouseTrigger::Button(BTN_RIGHT)),
            Some(MouseAction::ResizeWindow)
        );
        assert!(parse_mouse_combo("Super+wheel").is_some());
        assert!(parse_mouse_combo("Super+button9").is_none());
    }

    #[test]
    fn output_scale_falls_back_to_general() {
        let cfg = Config::parse(
            "[general]\nscale = 1.5\n[[output]]\nname = \"HDMI-A-1\"\nscale = 1.0\nposition = \"1707, 0\"\n",
        )
        .unwrap();
        assert_eq!(cfg.output_scale("eDP-1"), 1.5);
        assert_eq!(cfg.output_scale("HDMI-A-1"), 1.0);
        assert_eq!(cfg.output("HDMI-A-1").unwrap().position(), Some((1707, 0)));
    }

    #[test]
    fn output_mirror_is_parsed() {
        let cfg = Config::parse("[[output]]\nname = \"HDMI-A-1\"\nmirror = \"eDP-1\"\n").unwrap();
        assert_eq!(
            cfg.output("HDMI-A-1").unwrap().mirror.as_deref(),
            Some("eDP-1")
        );
        assert!(cfg.output("eDP-1").is_none());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let cfg = Config::parse("[canvas]\nmin_zoom = 0.2\n").unwrap();
        assert_eq!(cfg.canvas.min_zoom, 0.2);
        assert_eq!(cfg.canvas.max_zoom, Canvas::default().max_zoom);
    }
}
