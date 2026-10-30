// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Groups: named sets of canvas windows that belong together, drawn as a
//! tinted frame around them like a Figma frame.
//!
//! A group is the windows sharing a `group` name on one workspace. It can be
//! fitted on screen in one go, moved as a whole (`move-group` mouse bind or
//! `group move DIR`), and collapsed: its windows are put away and only the
//! frame stays, a click on it brings them back.

use std::collections::BTreeMap;

use smithay::{
    output::Output,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    action::Dir,
    bindings::shell_quote,
    canvas::{Camera, bounding_box},
    profiles::valid_name,
    shell::Placement,
    state::State,
};

/// Space between the windows and their group's frame, canvas units.
pub const GROUP_PADDING: f64 = 28.0;

/// Frame colors, picked by name so a group keeps its color.
const PALETTE: [[f32; 3]; 8] = [
    [0.36, 0.55, 0.98],
    [0.95, 0.52, 0.25],
    [0.30, 0.78, 0.50],
    [0.86, 0.38, 0.72],
    [0.98, 0.78, 0.25],
    [0.35, 0.80, 0.85],
    [0.92, 0.36, 0.38],
    [0.62, 0.50, 0.95],
];

pub fn group_color(name: &str) -> [f32; 3] {
    // FNV-1a: stable across runs, unlike the std hasher.
    let hash = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    PALETTE[hash as usize % PALETTE.len()]
}

/// A group as drawn on one workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupFrame {
    pub name: String,
    /// Canvas rectangle, padding included.
    pub rect: Rectangle<f64, Logical>,
    pub collapsed: bool,
    pub members: usize,
}

impl State {
    /// Indices of the canvas windows of group `name` on workspace `ws`.
    pub fn group_members(&self, name: &str, ws: usize) -> Vec<usize> {
        (0..self.windows.len())
            .filter(|&i| {
                let m = &self.windows[i];
                m.mapped
                    && m.ws == ws
                    && m.placement == Placement::Canvas
                    && m.group.as_deref() == Some(name)
            })
            .collect()
    }

    /// Frames of every group on workspace `ws`, sorted by name.
    pub fn group_frames(&self, ws: usize) -> Vec<GroupFrame> {
        let mut rects: BTreeMap<&str, Vec<Rectangle<f64, Logical>>> = BTreeMap::new();
        for m in &self.windows {
            if let Some(name) = m.group.as_deref() {
                if m.mapped && m.ws == ws && m.placement == Placement::Canvas {
                    rects.entry(name).or_default().push(self.canvas_frame(m));
                }
            }
        }
        rects
            .into_iter()
            .filter_map(|(name, frames)| {
                let bbox = bounding_box(&frames)?;
                let pad = Point::from((GROUP_PADDING, GROUP_PADDING));
                Some(GroupFrame {
                    name: name.to_string(),
                    rect: Rectangle::new(bbox.loc - pad, bbox.size + pad.to_size().upscale(2.0)),
                    collapsed: self.collapsed_groups.contains(name),
                    members: frames.len(),
                })
            })
            .collect()
    }

    /// Names of all groups with their workspace, for `find` and `group list`.
    pub fn group_names(&self) -> Vec<(usize, String)> {
        let mut names: Vec<(usize, String)> = self
            .windows
            .iter()
            .filter(|m| m.mapped)
            .filter_map(|m| Some((m.ws, m.group.clone()?)))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Collapsed group whose frame is under `local` (output-local).
    pub fn collapsed_group_at(
        &self,
        output: &Output,
        local: Point<f64, Logical>,
    ) -> Option<String> {
        let p = self.camera(output).to_canvas(local);
        self.group_frames(self.output_ws(output))
            .into_iter()
            .find(|f| f.collapsed && f.rect.contains(p))
            .map(|f| f.name)
    }

    fn focused_group(&self) -> Option<(String, usize)> {
        let m = &self.windows[self.focused_index()?];
        Some((m.group.clone()?, m.ws))
    }

    /// The named group (on the active workspace, else wherever it is), or
    /// the focused window's group when `name` is empty.
    fn resolve_group(&self, name: Option<&str>) -> Result<(String, usize), String> {
        let Some(name) = name else {
            return self
                .focused_group()
                .ok_or_else(|| "the focused window is in no group".into());
        };
        let active = self.active_output().map(|o| self.output_ws(&o));
        let names = self.group_names();
        names
            .iter()
            .find(|(ws, n)| n == name && Some(*ws) == active)
            .or_else(|| names.iter().find(|(_, n)| n == name))
            .map(|(ws, n)| (n.clone(), *ws))
            .ok_or_else(|| format!("no group {name}"))
    }

    /// `group add NAME | remove | fit [NAME] | toggle [NAME] | move DIR | list`.
    pub fn group_command(&mut self, args: &[&str]) -> Result<String, String> {
        match args {
            ["add", name] => {
                if !valid_name(name) {
                    return Err(format!(
                        "bad group name `{name}`: use letters, digits, - and _"
                    ));
                }
                let idx = self.focused_index().ok_or("no focused window")?;
                if self.windows[idx].placement != Placement::Canvas {
                    return Err("only windows on the canvas can join a group".into());
                }
                self.windows[idx].group = Some(name.to_string());
                self.queue_redraw_all();
                Ok(format!("added to group {name}"))
            }
            ["add"] => {
                self.prompt_group_name()?;
                Ok("ok".into())
            }
            ["remove"] => {
                let idx = self.focused_index().ok_or("no focused window")?;
                let old = self.windows[idx].group.take().ok_or("not in a group")?;
                self.queue_redraw_all();
                Ok(format!("removed from group {old}"))
            }
            ["fit"] => self.group_fit(None),
            ["fit", name] => self.group_fit(Some(name)),
            ["toggle"] => self.group_toggle(None),
            ["toggle", name] => self.group_toggle(Some(name)),
            ["move", dir] => {
                let dir = Dir::parse(dir).ok_or("direction: left, right, up or down")?;
                let (name, ws) = self.resolve_group(None)?;
                let (dx, dy) = dir.vector();
                let zoom = self.workspace(ws).target().zoom;
                let step = self.config.canvas.window_step as f64 / zoom;
                self.move_group_by(&name, ws, Point::from((dx * step, dy * step)));
                Ok("ok".into())
            }
            ["list"] => Ok(self
                .group_names()
                .into_iter()
                .map(|(ws, name)| {
                    let n = self.group_members(&name, ws).len();
                    let state = if self.collapsed_groups.contains(&name) {
                        " collapsed"
                    } else {
                        ""
                    };
                    format!("{} {name} {n} windows{state}", ws + 1)
                })
                .collect::<Vec<_>>()
                .join("\n")),
            _ => Err(
                "usage: group add NAME | remove | fit [NAME] | toggle [NAME] | move DIR | list"
                    .into(),
            ),
        }
    }

    pub fn group_fit(&mut self, name: Option<&str>) -> Result<String, String> {
        let (name, ws) = self.resolve_group(name)?;
        self.collapsed_groups.remove(&name);
        if !self.outputs.iter().any(|o| self.output_ws(o) == ws) {
            self.switch_workspace(ws);
        }
        let output = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == ws)
            .cloned()
            .ok_or("no output")?;
        let frame = self
            .group_frames(ws)
            .into_iter()
            .find(|f| f.name == name)
            .ok_or_else(|| format!("no group {name}"))?;
        let area = self.usable_area(&output).to_f64();
        let canvas = &self.config.canvas;
        let cam = Camera::fitting(frame.rect, area, 0.0, canvas.min_zoom, canvas.max_zoom);
        self.remember_camera(&output);
        self.fly_to(&output, cam);
        Ok("ok".into())
    }

    pub fn group_toggle(&mut self, name: Option<&str>) -> Result<String, String> {
        // Without a name and a grouped window, the collapsed frame under
        // the pointer is meant.
        let under_cursor = || {
            let output = self.active_output()?;
            let local = self.cursor_local(&output);
            self.collapsed_group_at(&output, local)
        };
        // Collapsing hands focus to another window, often of another group:
        // pressing the key again still means the group just collapsed.
        let focused_id = self.focused_index().map(|i| self.windows[i].id);
        let just_collapsed = self
            .last_collapsed
            .as_ref()
            .filter(|(g, handed)| *handed == focused_id && self.collapsed_groups.contains(g))
            .map(|(g, _)| g.clone());
        let name = match name {
            Some(n) => Some(n.to_string()),
            None => under_cursor().or(just_collapsed),
        };
        let (name, ws) = self.resolve_group(name.as_deref())?;
        self.last_collapsed = None;
        if self.collapsed_groups.remove(&name) {
            // Expanded: focus its top window.
            if let Some(&i) = self.group_members(&name, ws).first() {
                let w = self.windows[i].window.clone();
                self.focus_window(Some(w));
            }
            self.queue_redraw_all();
            return Ok(format!("group {name} expanded"));
        }
        self.collapsed_groups.insert(name.clone());
        let focused_inside = self
            .focused_index()
            .is_some_and(|i| self.windows[i].group.as_deref() == Some(&name));
        if focused_inside {
            let next = self
                .windows
                .iter()
                .find(|m| self.shown(m) && m.ws == ws)
                .map(|m| m.window.clone());
            self.focus_window(next);
        }
        let handed = self.focused_index().map(|i| self.windows[i].id);
        self.last_collapsed = Some((name.clone(), handed));
        self.queue_redraw_all();
        Ok(format!("group {name} collapsed"))
    }

    /// `group add` without a name: asks for one with the picker, offering
    /// the existing groups.
    fn prompt_group_name(&mut self) -> Result<(), String> {
        self.focused_index().ok_or("no focused window")?;
        let picker = self
            .picker()
            .ok_or("install rofi, fuzzel or wofi, or set [general] picker")?;
        let mut names: Vec<String> = self.group_names().into_iter().map(|(_, n)| n).collect();
        names.dedup();
        let exe = crate::find::own_exe();
        let list = shell_quote(&names.join("\n"));
        self.spawn(&format!(
            "name=$(printf '%s\\n' {list} | {picker}) && [ -n \"$name\" ] && {exe} msg group add \"$name\""
        ));
        Ok(())
    }

    /// Opens the collapsed group a window belongs to, so it can be shown.
    pub fn expand_group_of(&mut self, idx: usize) {
        if let Some(g) = self.windows[idx].group.clone() {
            if self.collapsed_groups.remove(&g) {
                self.queue_redraw_all();
            }
        }
    }

    /// Moves every window of the group by a canvas delta.
    pub fn move_group_by(&mut self, name: &str, ws: usize, delta: Point<f64, Logical>) {
        for i in self.group_members(name, ws) {
            self.windows[i].pos += delta;
        }
        self.queue_redraw_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_color_is_stable_and_from_the_palette() {
        let a = group_color("work");
        assert_eq!(a, group_color("work"));
        assert!(PALETTE.contains(&a));
    }

    #[test]
    fn different_names_spread_over_the_palette() {
        let colors: std::collections::HashSet<[u32; 3]> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|n| group_color(n).map(f32::to_bits))
            .collect();
        assert!(colors.len() >= 3);
    }
}
