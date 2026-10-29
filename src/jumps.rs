//! Camera jumps: a back/forward history like a browser's and named
//! bookmarks of camera spots, both per workspace.
//!
//! Every jump (fit, center, overview, home, bookmark, find, minimap click)
//! first remembers where the camera was, so `camera back` returns there.
//! Plain panning and zooming are not jumps: the spot you panned to is
//! remembered when the next jump leaves it. Bookmarks are kept in
//! `$XDG_STATE_HOME/pzwm/bookmarks.json`.

use std::{collections::BTreeMap, path::PathBuf, time::Instant};

use serde::{Deserialize, Serialize};
use smithay::{output::Output, utils::Point};
use tracing::warn;

use crate::{
    canvas::{Camera, WORKSPACES},
    profiles::{state_dir, valid_name},
    state::State,
};

/// Jumps kept per workspace in each direction.
const HISTORY_LIMIT: usize = 50;

#[derive(Debug, Default)]
pub struct Jumps {
    back: [Vec<Camera>; WORKSPACES],
    forward: [Vec<Camera>; WORKSPACES],
    /// Per workspace slot: name → camera.
    bookmarks: [BTreeMap<String, Camera>; WORKSPACES],
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedBookmark {
    ws: usize,
    name: String,
    x: f64,
    y: f64,
    zoom: f64,
}

/// Two cameras showing practically the same thing.
fn same_spot(a: Camera, b: Camera) -> bool {
    let d = (a.pos - b.pos).upscale(a.zoom);
    d.x.abs() < 2.0 && d.y.abs() < 2.0 && (a.zoom / b.zoom - 1.0).abs() < 0.01
}

impl Jumps {
    /// Records `from` before a jump; a new jump forgets the forward list.
    pub fn push(&mut self, slot: usize, from: Camera) {
        let back = &mut self.back[slot];
        if back.last().is_none_or(|&last| !same_spot(last, from)) {
            back.push(from);
            if back.len() > HISTORY_LIMIT {
                back.remove(0);
            }
        }
        self.forward[slot].clear();
    }

    /// The camera to go back to from `current`, if any.
    pub fn back(&mut self, slot: usize, current: Camera) -> Option<Camera> {
        Self::step(&mut self.back[slot], &mut self.forward[slot], current)
    }

    pub fn forward(&mut self, slot: usize, current: Camera) -> Option<Camera> {
        Self::step(&mut self.forward[slot], &mut self.back[slot], current)
    }

    fn step(from: &mut Vec<Camera>, to: &mut Vec<Camera>, current: Camera) -> Option<Camera> {
        // Entries equal to where we are would make the key look dead.
        while from.last().is_some_and(|&c| same_spot(c, current)) {
            from.pop();
        }
        let target = from.pop()?;
        if to.last().is_none_or(|&last| !same_spot(last, current)) {
            to.push(current);
        }
        Some(target)
    }

    pub fn bookmark(&self, slot: usize, name: &str) -> Option<Camera> {
        self.bookmarks[slot].get(name).copied()
    }

    /// Bookmarks of one camera slot, in name order.
    pub fn slot_bookmarks(&self, slot: usize) -> Vec<(String, Camera)> {
        self.bookmarks[slot]
            .iter()
            .map(|(n, c)| (n.clone(), *c))
            .collect()
    }

    /// `(slot, name)` of every bookmark.
    pub fn bookmark_names(&self) -> Vec<(usize, String)> {
        (0..WORKSPACES)
            .flat_map(|ws| self.bookmarks[ws].keys().map(move |n| (ws, n.clone())))
            .collect()
    }

    fn path() -> PathBuf {
        state_dir().join("bookmarks.json")
    }

    pub fn load() -> Self {
        let mut jumps = Self::default();
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            return jumps;
        };
        match serde_json::from_str::<Vec<SavedBookmark>>(&text) {
            Ok(saved) => {
                for b in saved.into_iter().filter(|b| b.ws < WORKSPACES) {
                    if valid_name(&b.name) && b.zoom.is_finite() && b.zoom > 0.0 {
                        let cam = Camera {
                            pos: Point::from((b.x, b.y)),
                            zoom: b.zoom,
                        };
                        jumps.bookmarks[b.ws].insert(b.name, cam);
                    }
                }
            }
            Err(err) => warn!("{}: {err}", Self::path().display()),
        }
        jumps
    }

    fn save(&self) -> Result<(), String> {
        let saved: Vec<SavedBookmark> = (0..WORKSPACES)
            .flat_map(|ws| {
                self.bookmarks[ws]
                    .iter()
                    .map(move |(name, cam)| SavedBookmark {
                        ws,
                        name: name.clone(),
                        x: cam.pos.x,
                        y: cam.pos.y,
                        zoom: cam.zoom,
                    })
            })
            .collect();
        let text = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())?;
        let path = Self::path();
        std::fs::create_dir_all(state_dir()).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl State {
    /// Remembers where `output`'s camera is, before jumping somewhere else.
    pub fn remember_camera(&mut self, output: &Output) {
        let ws = self.output_ws(output);
        let slot = self.camera_slot(ws);
        let cam = self.workspace(ws).target();
        self.jumps.push(slot, cam);
    }

    /// Like [`State::remember_camera`] for the output showing `ws`.
    pub fn remember_camera_of_ws(&mut self, ws: usize) {
        if let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.output_ws(o) == ws)
            .cloned()
        {
            self.remember_camera(&output);
        }
    }

    pub fn fly_to(&mut self, output: &Output, cam: Camera) {
        let ws = self.output_ws(output);
        let canvas = &self.config.canvas;
        let cam = Camera {
            pos: cam.pos,
            zoom: cam.zoom.clamp(canvas.min_zoom, canvas.max_zoom),
        };
        let dur = self.camera_duration();
        self.workspace_mut(ws).animate_to(cam, dur, Instant::now());
        self.queue_redraw(output);
    }

    pub fn camera_history(&mut self, output: &Output, back: bool) {
        let ws = self.output_ws(output);
        let slot = self.camera_slot(ws);
        let current = self.workspace(ws).target();
        let target = if back {
            self.jumps.back(slot, current)
        } else {
            self.jumps.forward(slot, current)
        };
        if let Some(cam) = target {
            self.pre_overview = None;
            self.fly_to(output, cam);
        }
    }

    /// `bookmark save|go|delete NAME`, `bookmark list`.
    pub fn bookmark_command(&mut self, args: &[&str]) -> Result<String, String> {
        let output = self.active_output().ok_or("no output")?;
        let ws = self.output_ws(&output);
        let slot = self.camera_slot(ws);
        let check = |name: &str| {
            valid_name(name)
                .then_some(())
                .ok_or_else(|| format!("bad bookmark name `{name}`: use letters, digits, - and _"))
        };
        match args {
            ["save", name] => {
                check(name)?;
                let cam = self.workspace(ws).target();
                self.jumps.bookmarks[slot].insert(name.to_string(), cam);
                self.jumps.save()?;
                Ok(format!("bookmark {name} saved on workspace {}", ws + 1))
            }
            ["go", name] => {
                let cam = self
                    .jumps
                    .bookmark(slot, name)
                    .ok_or_else(|| format!("no bookmark {name} on workspace {}", ws + 1))?;
                self.remember_camera(&output);
                self.fly_to(&output, cam);
                Ok("ok".into())
            }
            ["delete", name] => {
                self.jumps.bookmarks[slot]
                    .remove(*name)
                    .ok_or_else(|| format!("no bookmark {name} on workspace {}", ws + 1))?;
                self.jumps.save()?;
                Ok(format!("bookmark {name} deleted"))
            }
            ["list"] => Ok(self
                .jumps
                .bookmark_names()
                .into_iter()
                .map(|(ws, name)| format!("{} {name}", ws + 1))
                .collect::<Vec<_>>()
                .join("\n")),
            _ => Err("usage: bookmark save NAME | go NAME | delete NAME | list".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(x: f64, zoom: f64) -> Camera {
        Camera {
            pos: Point::from((x, 0.0)),
            zoom,
        }
    }

    #[test]
    fn back_and_forward_walk_the_history() {
        let mut j = Jumps::default();
        j.push(0, cam(0.0, 1.0));
        j.push(0, cam(100.0, 1.0));
        // Now at 200: back goes to 100, then 0; forward returns.
        assert_eq!(j.back(0, cam(200.0, 1.0)), Some(cam(100.0, 1.0)));
        assert_eq!(j.back(0, cam(100.0, 1.0)), Some(cam(0.0, 1.0)));
        assert_eq!(j.back(0, cam(0.0, 1.0)), None);
        assert_eq!(j.forward(0, cam(0.0, 1.0)), Some(cam(100.0, 1.0)));
        assert_eq!(j.forward(0, cam(100.0, 1.0)), Some(cam(200.0, 1.0)));
        assert_eq!(j.forward(0, cam(200.0, 1.0)), None);
    }

    #[test]
    fn a_new_jump_clears_forward_and_skips_duplicates() {
        let mut j = Jumps::default();
        j.push(0, cam(0.0, 1.0));
        j.push(0, cam(0.5, 1.0)); // same spot, not recorded twice
        assert_eq!(j.back[0].len(), 1);
        assert!(j.back(0, cam(50.0, 1.0)).is_some());
        assert_eq!(j.forward[0].len(), 1);
        j.push(0, cam(0.0, 1.0));
        assert!(j.forward[0].is_empty());
    }

    #[test]
    fn back_skips_entries_equal_to_where_we_are() {
        let mut j = Jumps::default();
        j.push(0, cam(0.0, 1.0));
        j.push(0, cam(300.0, 1.0));
        assert_eq!(j.back(0, cam(300.0, 1.0)), Some(cam(0.0, 1.0)));
    }

    #[test]
    fn history_is_bounded_and_per_workspace() {
        let mut j = Jumps::default();
        for i in 0..(HISTORY_LIMIT + 10) {
            j.push(1, cam(i as f64 * 100.0, 1.0));
        }
        assert_eq!(j.back[1].len(), HISTORY_LIMIT);
        assert!(j.back[0].is_empty());
    }

    #[test]
    fn zoom_changes_count_as_a_different_spot() {
        assert!(!same_spot(cam(0.0, 1.0), cam(0.0, 2.0)));
        assert!(same_spot(cam(0.0, 1.0), cam(1.0, 1.001)));
    }
}
