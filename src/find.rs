//! `find`: search windows, groups and bookmarks by name and fly there.
//!
//! pzwm draws no text, so the list is shown by a dmenu-style picker (rofi,
//! fuzzel, wofi or `[general] picker`): `find` runs
//! `pzwm msg find-list | PICKER`, and the chosen line comes back as
//! `pzwm msg find-select LINE`. Each line ends with a tag (`#w12`,
//! `#g1:work`, `#b1:home`) naming what it points to.

use std::path::Path;

use crate::{bindings::shell_quote, shell::Placement, state::State};

/// Pickers tried in order when `[general] picker` is empty.
const PICKERS: [(&str, &str); 3] = [
    ("rofi", "rofi -dmenu -i -p find"),
    ("fuzzel", "fuzzel --dmenu --prompt 'find '"),
    ("wofi", "wofi --dmenu --prompt find"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindTarget {
    Window(u64),
    Group(usize, String),
    Bookmark(usize, String),
}

/// Reads the tag at the end of a picked line.
pub fn parse_tag(line: &str) -> Option<FindTarget> {
    let tag = line.trim().rsplit_once('#')?.1;
    let (kind, rest) = tag.split_at_checked(1)?;
    let named = |rest: &str| -> Option<(usize, String)> {
        let (ws, name) = rest.split_once(':')?;
        let ws: usize = ws.parse().ok()?;
        ((1..=10).contains(&ws) && !name.is_empty()).then(|| (ws - 1, name.to_string()))
    };
    match kind {
        "w" => rest.parse().ok().map(FindTarget::Window),
        "g" => named(rest).map(|(ws, n)| FindTarget::Group(ws, n)),
        "b" => named(rest).map(|(ws, n)| FindTarget::Bookmark(ws, n)),
        _ => None,
    }
}

/// One line of the picker: no tabs or newlines that would split it.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

/// This binary, quoted for `sh`, so spawned helpers call the same pzwm.
pub fn own_exe() -> String {
    let exe = std::env::current_exe()
        .ok()
        .filter(|p| p.is_file())
        .unwrap_or_else(|| Path::new("pzwm").to_path_buf());
    shell_quote(&exe.to_string_lossy())
}

impl State {
    pub fn picker(&self) -> Option<String> {
        let own = self.config.general.picker.trim();
        if !own.is_empty() {
            return Some(own.to_string());
        }
        PICKERS
            .iter()
            .find(|(program, _)| on_path(program))
            .map(|(_, command)| command.to_string())
    }

    /// The `find` action: opens the picker.
    pub fn find(&mut self) {
        let Some(picker) = self.picker() else {
            self.spawn(
                "notify-send -a pzwm 'pzwm: find' 'Install rofi, fuzzel or wofi, or set [general] picker'",
            );
            return;
        };
        let exe = own_exe();
        self.spawn(&format!(
            "sel=$({exe} msg find-list | {picker}) && [ -n \"$sel\" ] && {exe} msg find-select \"$sel\""
        ));
    }

    /// Lines for the picker: windows (most recently used first), then
    /// groups and bookmarks.
    pub fn find_list(&self) -> String {
        let mut lines = Vec::new();
        for m in self.windows.iter().filter(|m| m.mapped) {
            let title = clean(&self.title(m).unwrap_or_default());
            let app = clean(&crate::icons::app_name(&self.app_id(m).unwrap_or_default()));
            let place = match m.placement {
                _ if m.scratchpad => "scratchpad".to_string(),
                Placement::Pinned => "pinned".to_string(),
                _ => format!("workspace {}", m.ws + 1),
            };
            // "Title · App · workspace 2", the app left out when the title says it.
            let line = if title.is_empty() || title.eq_ignore_ascii_case(&app) {
                format!("{app}  ·  {place}")
            } else {
                format!("{title}  ·  {app}  ·  {place}")
            };
            lines.push(format!("{line}  #w{}", m.id));
        }
        for (ws, name) in self.group_names() {
            let n = self.group_members(&name, ws).len();
            lines.push(format!(
                "▣ group {name}  ({n} windows, workspace {})  #g{}:{name}",
                ws + 1,
                ws + 1
            ));
        }
        for (slot, name) in self.jumps.bookmark_names() {
            lines.push(format!(
                "⌖ bookmark {name}  (workspace {})  #b{}:{name}",
                slot + 1,
                slot + 1
            ));
        }
        lines.join("\n")
    }

    pub fn find_select(&mut self, line: &str) -> Result<String, String> {
        match parse_tag(line).ok_or("nothing to go to in that line")? {
            FindTarget::Window(id) => {
                let idx = self.index_of_id(id).ok_or("that window is gone")?;
                self.go_to_window(idx);
                Ok("ok".into())
            }
            FindTarget::Group(_, name) => self.group_fit(Some(&name)),
            FindTarget::Bookmark(ws, name) => {
                self.switch_workspace(ws);
                self.bookmark_command(&["go", &name])
            }
        }
    }

    /// Brings a window into view wherever it is: its workspace, its
    /// collapsed group, the scratchpad.
    pub fn go_to_window(&mut self, idx: usize) {
        let id = self.windows[idx].id;
        if self.windows[idx].scratchpad {
            if self.windows[idx].hidden {
                self.toggle_scratchpad();
            }
            return;
        }
        // Minimized from a taskbar: bring it back.
        self.windows[idx].hidden = false;
        self.expand_group_of(idx);
        let ws = self.windows[idx].ws;
        let placement = self.windows[idx].placement;
        // Switching workspaces refocuses and reorders the window list, so
        // `idx` is not used past this point.
        if placement != Placement::Pinned && !self.outputs.iter().any(|o| self.output_ws(o) == ws) {
            self.switch_workspace(ws);
        }
        if placement == Placement::Canvas {
            self.remember_camera_of_ws(ws);
        }
        self.focus_and_center(id);
        // From an overview the window would stay a thumbnail: zoom in on it.
        if let Some(idx) = self.index_of_id(id).filter(|&i| self.too_small_to_use(i)) {
            self.fit_window(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_read_from_the_end_of_the_line() {
        assert_eq!(
            parse_tag("C# notes — vim  (foot, workspace 2)  #w12"),
            Some(FindTarget::Window(12))
        );
        assert_eq!(
            parse_tag("▣ group work  (3 windows, workspace 4)  #g4:work"),
            Some(FindTarget::Group(3, "work".into()))
        );
        assert_eq!(
            parse_tag("⌖ bookmark home  (workspace 1)  #b1:home\n"),
            Some(FindTarget::Bookmark(0, "home".into()))
        );
    }

    #[test]
    fn bad_tags_are_rejected() {
        assert_eq!(parse_tag("no tag here"), None);
        assert_eq!(parse_tag("#wx"), None);
        assert_eq!(parse_tag("#g11:work"), None);
        assert_eq!(parse_tag("#b1:"), None);
        assert_eq!(parse_tag("#"), None);
        assert_eq!(parse_tag("#ж"), None);
    }

    #[test]
    fn lines_have_no_control_characters() {
        assert_eq!(clean(" a\tb\nc "), "a b c");
    }
}
