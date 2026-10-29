//! Window rules: `[[rule]]` blocks in the config that pick where and how a
//! window opens, matched on its app id and title.
//!
//! ```toml
//! [[rule]]
//! app_id = "pavucontrol"
//! size = "700x500"
//! pin = true
//! ```

use serde::Deserialize;
use smithay::utils::{Logical, Point, Size};

use crate::{profiles::valid_name, shell::Managed, state::State};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WindowRule {
    /// Pattern for the app id; `*` matches any run of characters, `?` one.
    /// Matching ignores case. Empty matches every window.
    pub app_id: String,
    /// Pattern for the title, same syntax as `app_id`.
    pub title: String,
    /// Workspace 1 … 10 the window opens on.
    pub workspace: Option<usize>,
    /// "x,y": top-left of the window on the canvas.
    pub position: Option<String>,
    /// "WxH" in logical pixels.
    pub size: Option<String>,
    pub pin: bool,
    pub maximize: bool,
    pub fullscreen: bool,
    /// Group the window joins (see groups.rs).
    pub group: Option<String>,
    /// The window becomes the scratchpad: hidden until `scratchpad` is pressed.
    pub scratchpad: bool,
}

/// What all matching rules ask for, later rules winning.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleEffect {
    pub workspace: Option<usize>,
    pub position: Option<Point<f64, Logical>>,
    pub size: Option<Size<i32, Logical>>,
    pub pin: bool,
    pub maximize: bool,
    pub fullscreen: bool,
    pub group: Option<String>,
    pub scratchpad: bool,
}

impl WindowRule {
    pub fn matches(&self, app_id: &str, title: &str) -> bool {
        (self.app_id.is_empty() || glob_match(&self.app_id, app_id))
            && (self.title.is_empty() || glob_match(&self.title, title))
    }

    /// Problems a user should hear about at config load.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.app_id.is_empty() && self.title.is_empty() {
            out.push("rule without app_id or title matches every window".into());
        }
        if let Some(ws) = self.workspace.filter(|w| !(1..=10).contains(w)) {
            out.push(format!("rule workspace must be 1 … 10, not {ws}"));
        }
        if let Some(p) = self
            .position
            .as_deref()
            .filter(|p| parse_pair(p, ',').is_none())
        {
            out.push(format!("rule position `{p}` is not \"x,y\""));
        }
        if let Some(g) = self.group.as_deref().filter(|g| !valid_name(g.trim())) {
            out.push(format!("rule group `{g}`: use letters, digits, - and _"));
        }
        if let Some(s) = self.size.as_deref().filter(|s| parse_size(s).is_none()) {
            out.push(format!("rule size `{s}` is not \"WxH\""));
        }
        out
    }
}

/// Combines every rule that matches the window.
pub fn effect(rules: &[WindowRule], app_id: &str, title: &str) -> Option<RuleEffect> {
    let mut found = false;
    let mut e = RuleEffect::default();
    for rule in rules.iter().filter(|r| r.matches(app_id, title)) {
        found = true;
        if let Some(ws) = rule.workspace.filter(|w| (1..=10).contains(w)) {
            e.workspace = Some(ws - 1);
        }
        if let Some((x, y)) = rule.position.as_deref().and_then(|p| parse_pair(p, ',')) {
            e.position = Some(Point::from((x, y)));
        }
        if let Some(size) = rule.size.as_deref().and_then(parse_size) {
            e.size = Some(size);
        }
        e.pin |= rule.pin;
        e.maximize |= rule.maximize;
        e.fullscreen |= rule.fullscreen;
        e.scratchpad |= rule.scratchpad;
        if let Some(g) = rule
            .group
            .as_deref()
            .map(str::trim)
            .filter(|g| valid_name(g))
        {
            e.group = Some(g.to_string());
        }
    }
    found.then_some(e)
}

fn parse_pair(s: &str, sep: char) -> Option<(f64, f64)> {
    let (a, b) = s.split_once(sep)?;
    let a: f64 = a.trim().parse().ok()?;
    let b: f64 = b.trim().parse().ok()?;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

fn parse_size(s: &str) -> Option<Size<i32, Logical>> {
    let (w, h) = parse_pair(&s.to_lowercase(), 'x')?;
    (w >= 1.0 && h >= 1.0 && w <= 16384.0 && h <= 16384.0)
        .then(|| Size::from((w.round() as i32, h.round() as i32)))
}

/// Case-insensitive glob with `*` and `?`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    // Last `*` seen and the text position it was tried at, for backtracking.
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

impl State {
    /// Rules matching a window; the scratchpad app id counts as a rule.
    pub fn rule_effect(&self, m: &Managed) -> Option<RuleEffect> {
        let app_id = self.app_id(m).unwrap_or_default();
        let title = self.title(m).unwrap_or_default();
        let mut e = effect(&self.config.rules, &app_id, &title);
        let pad = &self.config.scratchpad.app_id;
        if !pad.is_empty() && app_id == *pad {
            e.get_or_insert_with(RuleEffect::default).scratchpad = true;
        }
        e
    }

    /// Applies the rules to a window that just mapped. Returns false when no
    /// rule matched and the window should be focused as usual.
    pub fn apply_rules(&mut self, idx: usize) -> bool {
        let Some(e) = self.rule_effect(&self.windows[idx]) else {
            return false;
        };
        let id = self.windows[idx].id;
        if let Some(group) = e.group {
            self.windows[idx].group = Some(group);
        }
        if e.scratchpad {
            self.adopt_scratchpad(idx);
            return true;
        }
        if let Some(pos) = e.position {
            let title = self.title_height(&self.windows[idx]);
            self.windows[idx].pos = Point::from((pos.x, pos.y + title));
        }
        if let Some(ws) = e.workspace {
            self.windows[idx].ws = ws;
            if !self.outputs.iter().any(|o| self.output_ws(o) == ws) {
                // Opens in the background, on a workspace nobody looks at.
                return true;
            }
        }
        let window = self.windows[idx].window.clone();
        self.focus_window(Some(window));
        let Some(idx) = self.index_of_id(id) else {
            return true;
        };
        if e.fullscreen {
            self.fullscreen(idx);
        } else if e.maximize {
            self.maximize(idx);
        } else if e.pin {
            self.toggle_pin(idx);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app_id: &str, title: &str) -> WindowRule {
        WindowRule {
            app_id: app_id.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn glob_handles_stars_and_question_marks() {
        assert!(glob_match("firefox", "Firefox"));
        assert!(glob_match("*fox", "firefox"));
        assert!(glob_match("org.*.Nautilus", "org.gnome.Nautilus"));
        assert!(glob_match("*picture*in*", "Picture-in-Picture"));
        assert!(glob_match("f?ot", "foot"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("foot", "footclient"));
        assert!(!glob_match("?", ""));
    }

    #[test]
    fn rule_needs_both_patterns_to_match() {
        let r = rule("firefox", "*Picture*");
        assert!(r.matches("firefox", "Picture-in-Picture"));
        assert!(!r.matches("firefox", "GitHub"));
        assert!(rule("", "*Picture*").matches("chromium", "Picture"));
    }

    #[test]
    fn effects_combine_with_later_rules_winning() {
        let rules = vec![
            WindowRule {
                workspace: Some(2),
                size: Some("800x600".into()),
                ..rule("foot", "")
            },
            WindowRule {
                workspace: Some(5),
                pin: true,
                group: Some(" dev ".into()),
                ..rule("f*", "")
            },
        ];
        let e = effect(&rules, "foot", "~").unwrap();
        assert_eq!(e.workspace, Some(4));
        assert_eq!(e.size, Some(Size::from((800, 600))));
        assert!(e.pin);
        assert_eq!(e.group.as_deref(), Some("dev"));
        assert!(effect(&rules, "kitty", "~").is_none());
    }

    #[test]
    fn bad_values_are_reported_and_ignored() {
        let r = WindowRule {
            workspace: Some(11),
            position: Some("left".into()),
            size: Some("0x10".into()),
            ..rule("foot", "")
        };
        assert_eq!(r.problems().len(), 3);
        let spaced = WindowRule {
            group: Some("my group".into()),
            ..rule("foot", "")
        };
        assert_eq!(spaced.problems().len(), 1);
        assert_eq!(effect(&[spaced], "foot", "").unwrap().group, None);
        let e = effect(&[r], "foot", "").unwrap();
        assert_eq!(e, RuleEffect::default());
    }

    #[test]
    fn position_and_size_parse() {
        let r = WindowRule {
            position: Some("-100, 250.5".into()),
            size: Some("1280X720".into()),
            ..rule("x", "")
        };
        let e = effect(&[r], "x", "").unwrap();
        assert_eq!(e.position, Some(Point::from((-100.0, 250.5))));
        assert_eq!(e.size, Some(Size::from((1280, 720))));
    }
}
