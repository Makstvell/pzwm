//! User configuration: `~/.config/pzwm/config.toml`.
//!
//! Every field has a default, so an empty file (or no file at all) gives a
//! working compositor. Unknown keys are ignored to keep old configs loading.

use std::{collections::HashMap, path::PathBuf};

use serde::Deserialize;
use smithay::input::keyboard::{Keysym, ModifiersState, xkb};
use tracing::warn;

use crate::action::Action;

pub const DEFAULT_CONFIG: &str = include_str!("../resources/default-config.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub input: Input,
    pub canvas: Canvas,
    pub animation: Animation,
    pub appearance: Appearance,
    #[serde(rename = "output")]
    pub outputs: Vec<OutputConfig>,
    pub binds: HashMap<String, String>,
    #[serde(skip)]
    pub keybinds: Vec<Keybind>,
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
}

#[derive(Debug, Clone)]
pub struct Keybind {
    pub mods: Mods,
    pub sym: Keysym,
    pub action: Action,
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
            appearance: Appearance::default(),
            outputs: Vec::new(),
            binds: HashMap::new(),
            keybinds: Vec::new(),
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
        self
    }

    pub fn output(&self, name: &str) -> Option<&OutputConfig> {
        self.outputs.iter().find(|o| o.name == name)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DefaultBinds {
    binds: HashMap<String, String>,
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

/// Parses "Super+Shift+Return" into modifiers and a keysym.
pub fn parse_combo(combo: &str) -> Option<(Mods, Keysym)> {
    let mut mods = Mods::default();
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let key = parts.pop()?;
    for m in parts {
        match m.to_lowercase().as_str() {
            "super" | "mod" | "logo" | "win" => mods.logo = true,
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "mod1" => mods.alt = true,
            "shift" => mods.shift = true,
            _ => return None,
        }
    }
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
    fn missing_fields_fall_back_to_defaults() {
        let cfg = Config::parse("[canvas]\nmin_zoom = 0.2\n").unwrap();
        assert_eq!(cfg.canvas.min_zoom, 0.2);
        assert_eq!(cfg.canvas.max_zoom, Canvas::default().max_zoom);
    }
}
