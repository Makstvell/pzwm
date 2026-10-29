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
    pub fn parse(s: &str) -> Option<Self> {
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
    /// `bookmark save|go|delete NAME`: camera spots, see jumps.rs.
    Bookmark(String),
    /// `group add NAME | remove | fit | toggle | move DIR`, see groups.rs.
    Group(String),
    /// Back / forward through the camera's jumps.
    CameraBack,
    CameraForward,
    /// Call up or put away the drop-down scratchpad window.
    Scratchpad,
    /// Search windows, groups and bookmarks with a picker.
    Find,
    /// `draw [on|off|undo|clear|hide|color N|color next]`, see draw.rs.
    Draw(String),
    /// Line up the focused group (or the workspace) once, see arrange.rs.
    Arrange(crate::arrange::Arrangement),
    /// `present [start|stop|next|prev]`: fly through the bookmarks.
    Present(String),
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
            "bookmark" if !arg.is_empty() => Self::Bookmark(arg.to_string()),
            "group" if !arg.is_empty() => Self::Group(arg.to_string()),
            "camera" => match arg {
                "back" => Self::CameraBack,
                "forward" => Self::CameraForward,
                _ => return None,
            },
            "jump-back" => Self::CameraBack,
            "jump-forward" => Self::CameraForward,
            "scratchpad" | "toggle-scratchpad" => Self::Scratchpad,
            "find" | "search" => Self::Find,
            "draw" => Self::Draw(arg.to_string()),
            "arrange" => Self::Arrange(crate::arrange::Arrangement::parse(arg)?),
            "present" | "presentation" => Self::Present(arg.to_string()),
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
        assert_eq!(
            Action::parse("bookmark save 1"),
            Some(Action::Bookmark("save 1".into()))
        );
        assert_eq!(
            Action::parse("group add work"),
            Some(Action::Group("add work".into()))
        );
        assert_eq!(Action::parse("camera back"), Some(Action::CameraBack));
        assert_eq!(Action::parse("jump-forward"), Some(Action::CameraForward));
        assert_eq!(Action::parse("scratchpad"), Some(Action::Scratchpad));
        assert_eq!(Action::parse("find"), Some(Action::Find));
        assert_eq!(Action::parse("draw"), Some(Action::Draw(String::new())));
        assert_eq!(
            Action::parse("draw color 2"),
            Some(Action::Draw("color 2".into()))
        );
        assert_eq!(
            Action::parse("arrange row"),
            Some(Action::Arrange(crate::arrange::Arrangement::Row))
        );
        assert_eq!(
            Action::parse("present next"),
            Some(Action::Present("next".into()))
        );
    }

    #[test]
    fn rejects_bad_actions() {
        assert_eq!(Action::parse("spawn"), None);
        assert_eq!(Action::parse("workspace 0"), None);
        assert_eq!(Action::parse("workspace 11"), None);
        assert_eq!(Action::parse("pan sideways"), None);
        assert_eq!(Action::parse("dance"), None);
        assert_eq!(Action::parse("camera sideways"), None);
        assert_eq!(Action::parse("bookmark"), None);
        assert_eq!(Action::parse("group"), None);
        assert_eq!(Action::parse("arrange spiral"), None);
    }
}
