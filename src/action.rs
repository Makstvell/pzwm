//! Actions that key bindings can trigger, parsed from strings like
//! `"spawn ghostty"`, `"workspace 3"` or `"pan left"`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "left" | "l" => Some(Self::Left),
            "right" | "r" => Some(Self::Right),
            "up" | "u" => Some(Self::Up),
            "down" | "d" => Some(Self::Down),
            _ => None,
        }
    }

    /// Unit vector in screen coordinates (y grows downwards).
    pub fn vector(self) -> (f64, f64) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::Up => (0.0, -1.0),
            Self::Down => (0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Spawn(String),
    Close,
    /// Fill the usable screen area; toggling back restores the old place.
    ToggleMaximize,
    /// Cover the whole output, above the bar.
    ToggleFullscreen,
    /// Keep the window fixed on screen while the canvas moves under it.
    TogglePin,
    CenterWindow,
    /// `profile save|load NAME`: desktop profiles, see profiles.rs.
    Profile(String),
    /// Center the window and zoom so it fills the visible area.
    FitWindow,
    FocusDir(Dir),
    FocusNext,
    FocusPrev,
    MoveWindow(Dir),
    ResizeWindow(Dir),
    /// Tile to the left/right half (up = maximize, down = restore).
    Snap(Dir),
    Pan(Dir),
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Zoom out so every window of the workspace is visible.
    Overview,
    Home,
    /// Show or hide the workspace minimap.
    ToggleMinimap,
    /// Jump to the n-th window (0-based) in the order windows were opened.
    FocusNth(usize),
    Workspace(usize),
    MoveToWorkspace(usize),
    NextLayout,
    SetLayout(u32),
    ReloadConfig,
    Quit,
    SwitchVt(i32),
}

impl Action {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (name, arg) = match s.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (s, ""),
        };
        let dir = || Dir::parse(&arg.to_lowercase());
        let num = || arg.parse::<usize>().ok();
        Some(match name.to_lowercase().as_str() {
            "spawn" | "exec" if !arg.is_empty() => Self::Spawn(arg.to_string()),
            "close" | "kill" => Self::Close,
            "maximize" | "toggle-maximize" => Self::ToggleMaximize,
            "fullscreen" | "toggle-fullscreen" => Self::ToggleFullscreen,
            "pin" | "toggle-pin" => Self::TogglePin,
            "center" | "center-window" => Self::CenterWindow,
            "fit" | "fit-window" => Self::FitWindow,
            "profile" if !arg.is_empty() => Self::Profile(arg.to_string()),
            "focus" => match arg {
                "next" => Self::FocusNext,
                "prev" | "previous" => Self::FocusPrev,
                _ => Self::FocusDir(dir()?),
            },
            "move-window" | "move" => Self::MoveWindow(dir()?),
            "resize-window" | "resize" => Self::ResizeWindow(dir()?),
            "snap" => Self::Snap(dir()?),
            "pan" => Self::Pan(dir()?),
            "zoom-in" => Self::ZoomIn,
            "zoom-out" => Self::ZoomOut,
            "zoom-reset" => Self::ZoomReset,
            "overview" | "zoom-fit" => Self::Overview,
            "home" => Self::Home,
            "minimap" | "toggle-minimap" => Self::ToggleMinimap,
            "window" => Self::FocusNth(num().filter(|n| (1..=10).contains(n))? - 1),
            "workspace" => Self::Workspace(num().filter(|n| (1..=10).contains(n))? - 1),
            "move-to-workspace" => {
                Self::MoveToWorkspace(num().filter(|n| (1..=10).contains(n))? - 1)
            }
            "layout" => match arg {
                "next" => Self::NextLayout,
                _ => Self::SetLayout(arg.parse().ok()?),
            },
            "reload-config" | "reload" => Self::ReloadConfig,
            "quit" | "exit" => Self::Quit,
            "vt" => Self::SwitchVt(arg.parse().ok().filter(|n| (1..=12).contains(n))?),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions_with_arguments() {
        assert_eq!(
            Action::parse("spawn ghostty +new-window"),
            Some(Action::Spawn("ghostty +new-window".into()))
        );
        assert_eq!(Action::parse("workspace 1"), Some(Action::Workspace(0)));
        assert_eq!(Action::parse("window 10"), Some(Action::FocusNth(9)));
        assert_eq!(Action::parse("window 0"), None);
        assert_eq!(
            Action::parse("move-to-workspace 10"),
            Some(Action::MoveToWorkspace(9))
        );
        assert_eq!(Action::parse("pan Left"), Some(Action::Pan(Dir::Left)));
        assert_eq!(Action::parse("focus next"), Some(Action::FocusNext));
        assert_eq!(Action::parse("layout 3"), Some(Action::SetLayout(3)));
        assert_eq!(Action::parse("vt 2"), Some(Action::SwitchVt(2)));
    }

    #[test]
    fn rejects_bad_actions() {
        assert_eq!(Action::parse("spawn"), None);
        assert_eq!(Action::parse("workspace 0"), None);
        assert_eq!(Action::parse("workspace 11"), None);
        assert_eq!(Action::parse("pan sideways"), None);
        assert_eq!(Action::parse("dance"), None);
    }
}
