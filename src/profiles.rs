//! Desktop profiles: named snapshots of where applications sit.
//!
//! `profile save work` records every canvas or maximized window (by app id)
//! with its workspace, position and size, plus the workspace cameras and
//! which workspace each monitor shows. `profile load work` puts open
//! windows back, starts the missing apps with `gtk-launch` and places their
//! windows as they appear. Files live in `$XDG_STATE_HOME/pzwm/profiles`.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use smithay::utils::{Logical, Point, Size};
use tracing::info;

use crate::{
    canvas::{Camera, WORKSPACES},
    shell::Placement,
    state::{State, output_state},
};

/// How long a loaded profile waits for the windows of apps it started.
const PENDING_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub app_id: String,
    pub ws: usize,
    pub x: f64,
    pub y: f64,
    pub w: i32,
    pub h: i32,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedCamera {
    ws: usize,
    x: f64,
    y: f64,
    zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedOutput {
    name: String,
    ws: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Profile {
    windows: Vec<SavedWindow>,
    cameras: Vec<SavedCamera>,
    outputs: Vec<SavedOutput>,
    /// Drawings on the canvas; profiles saved before drawing existed have
    /// none, and loading them keeps the current drawings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drawings: Option<Vec<crate::draw::SavedStroke>>,
}

/// Name of the profile `[general] autosave` keeps up to date.
pub const AUTOSAVE: &str = "autosave";
/// How often the autosave profile is written.
pub const AUTOSAVE_EVERY: Duration = Duration::from_secs(120);

/// Windows of a loaded profile still waiting for their app to open them.
#[derive(Debug, Default)]
pub struct PendingPlacements {
    slots: Vec<SavedWindow>,
    until: Option<Instant>,
}

impl PendingPlacements {
    fn slots(&self) -> &[SavedWindow] {
        if self.until.is_some_and(|t| Instant::now() > t) {
            &[]
        } else {
            &self.slots
        }
    }

    fn take(&mut self, app_id: &str) -> Option<SavedWindow> {
        let i = self.slots().iter().position(|w| w.app_id == app_id)?;
        Some(self.slots.remove(i))
    }
}

/// `$XDG_STATE_HOME/pzwm`: profiles, bookmarks.
pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("pzwm")
}

fn dir() -> PathBuf {
    state_dir().join("profiles")
}

/// Profile names become file names, so only plain words are allowed.
/// Bookmark and group names follow the same rule.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn path(name: &str) -> Result<PathBuf, String> {
    if valid_name(name) {
        Ok(dir().join(format!("{name}.json")))
    } else {
        Err(format!(
            "bad profile name `{name}`: use letters, digits, - and _"
        ))
    }
}

/// App ids passed to `gtk-launch` must not need shell quoting.
fn launchable(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Pairs saved windows with open ones of the same app, in order. Returns
/// (saved index, open index) pairs and the saved windows left without one.
fn assign(saved: &[SavedWindow], open: &[String]) -> (Vec<(usize, usize)>, Vec<usize>) {
    let mut taken = vec![false; open.len()];
    let mut pairs = Vec::new();
    let mut missing = Vec::new();
    for (s, w) in saved.iter().enumerate() {
        match (0..open.len()).find(|&o| !taken[o] && open[o] == w.app_id) {
            Some(o) => {
                taken[o] = true;
                pairs.push((s, o));
            }
            None => missing.push(s),
        }
    }
    (pairs, missing)
}

impl State {
    /// Handles `profile save|load|delete NAME`, `profile rename OLD NEW`
    /// and `profile list [--json]`.
    pub fn profile_command(&mut self, args: &[&str]) -> Result<String, String> {
        match args {
            ["save", name] => self.profile_save(name),
            ["load", name] => self.profile_load(name),
            ["delete", name] => profile_delete(name),
            ["rename", old, new] => profile_rename(old, new),
            ["list"] => Ok(profile_list()),
            ["list", "--json"] => Ok(profile_list_json()),
            _ => Err(
                "usage: profile save NAME | load NAME | delete NAME | rename OLD NEW | list [--json]"
                    .into(),
            ),
        }
    }

    /// Saves the `autosave` profile, unless there is nothing to save: an
    /// empty desktop right after login must not overwrite the last one.
    pub fn autosave_profile(&mut self) {
        if !self.config.general.autosave {
            return;
        }
        if (0..self.windows.len()).all(|i| self.saved_window(i).is_none()) {
            return;
        }
        if let Err(err) = self.profile_save(AUTOSAVE) {
            tracing::warn!("autosave: {err}");
        }
    }

    fn profile_save(&mut self, name: &str) -> Result<String, String> {
        let path = path(name)?;
        let windows: Vec<SavedWindow> = (0..self.windows.len())
            .rev()
            .filter_map(|i| self.saved_window(i))
            .collect();
        let cameras = (0..WORKSPACES)
            .map(|ws| {
                let cam = self.workspace(ws).target();
                SavedCamera {
                    ws,
                    x: cam.pos.x,
                    y: cam.pos.y,
                    zoom: cam.zoom,
                }
            })
            .collect();
        let outputs = self
            .outputs
            .iter()
            .map(|o| SavedOutput {
                name: o.name(),
                ws: self.output_ws(o),
            })
            .collect();
        let profile = Profile {
            windows,
            cameras,
            outputs,
            drawings: Some(self.draw.saved()),
        };
        let text = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir()).map_err(|e| format!("{}: {e}", dir().display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        info!("profile {name} saved ({} windows)", profile.windows.len());
        Ok(format!(
            "saved profile {name}: {} windows",
            profile.windows.len()
        ))
    }

    fn saved_window(&self, idx: usize) -> Option<SavedWindow> {
        let m = &self.windows[idx];
        if !m.mapped || m.window.toplevel()?.parent().is_some() {
            return None;
        }
        let g = self.canvas_geometry(idx)?;
        Some(SavedWindow {
            app_id: self.app_id(m).filter(|a| !a.is_empty())?,
            ws: g.ws,
            x: g.pos.x,
            y: g.pos.y,
            w: g.size.w,
            h: g.size.h,
            maximized: m.placement == Placement::Maximized,
            group: m.group.clone(),
        })
    }

    fn profile_load(&mut self, name: &str) -> Result<String, String> {
        let path = path(name)?;
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let profile: Profile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;

        for cam in profile.cameras.iter().filter(|c| c.ws < WORKSPACES) {
            let zoom = cam
                .zoom
                .clamp(self.config.canvas.min_zoom, self.config.canvas.max_zoom);
            self.workspace_mut(cam.ws).set(Camera {
                pos: Point::from((cam.x, cam.y)),
                zoom,
            });
        }
        self.restore_output_workspaces(&profile.outputs);
        if let Some(drawings) = profile.drawings {
            self.draw.replace_all(drawings);
            self.draw.save();
        }

        let ids: Vec<u64> = self.windows.iter().map(|m| m.id).collect();
        let open: Vec<String> = self
            .windows
            .iter()
            .map(|m| {
                let top_level = m.window.toplevel().is_some_and(|t| t.parent().is_none());
                let movable = matches!(m.placement, Placement::Canvas | Placement::Maximized);
                if m.mapped && top_level && movable {
                    self.app_id(m).unwrap_or_default()
                } else {
                    String::new()
                }
            })
            .collect();
        let saved: Vec<SavedWindow> = profile
            .windows
            .into_iter()
            .filter(|w| w.ws < WORKSPACES)
            .collect();
        let (pairs, missing) = assign(&saved, &open);
        for &(s, o) in &pairs {
            if let Some(idx) = self.windows.iter().position(|m| m.id == ids[o]) {
                self.place_saved(idx, &saved[s]);
            }
        }

        let missing: Vec<SavedWindow> = missing.into_iter().map(|s| saved[s].clone()).collect();
        for w in &missing {
            if launchable(&w.app_id) {
                self.spawn(&format!("gtk-launch {}", w.app_id));
            }
        }
        let started = missing.len();
        self.profile_pending = PendingPlacements {
            slots: missing,
            until: Some(Instant::now() + PENDING_TTL),
        };
        self.queue_redraw_all();
        info!("profile {name} loaded");
        Ok(format!(
            "loaded profile {name}: {} windows placed, {started} apps started",
            pairs.len()
        ))
    }

    fn restore_output_workspaces(&mut self, saved: &[SavedOutput]) {
        for s in saved.iter().filter(|s| s.ws < WORKSPACES) {
            let Some(output) = self.output_by_name(&s.name) else {
                continue;
            };
            // Keep one workspace per monitor: swap with whoever shows it.
            let current = self.output_ws(&output);
            if let Some(other) = self
                .outputs
                .iter()
                .find(|o| *o != &output && self.output_ws(o) == s.ws)
            {
                output_state(other).ws = current;
            }
            output_state(&output).ws = s.ws;
        }
    }

    fn place_saved(&mut self, idx: usize, w: &SavedWindow) {
        if self.windows[idx].placement != Placement::Canvas {
            self.restore(idx);
        }
        let m = &mut self.windows[idx];
        m.ws = w.ws;
        m.pos = Point::from((w.x, w.y));
        m.output = None;
        m.group = w.group.clone();
        self.configure(
            idx,
            Some(Size::from((w.w.max(1), w.h.max(1)))),
            false,
            false,
        );
        if w.maximized && self.outputs.iter().any(|o| self.output_ws(o) == w.ws) {
            self.maximize(idx);
        }
    }

    /// Saved size for a window that is about to open, if a loaded profile
    /// is waiting for one of its app.
    pub fn profile_size(&self, app_id: &str) -> Option<Size<i32, Logical>> {
        self.profile_pending
            .slots()
            .iter()
            .find(|w| w.app_id == app_id)
            .map(|w| Size::from((w.w.max(1), w.h.max(1))))
    }

    /// Takes the profile slot for a window that just mapped and puts the
    /// window there. Returns false when no slot was waiting for it.
    pub fn place_from_profile(&mut self, idx: usize) -> bool {
        let m = &self.windows[idx];
        if m.window.toplevel().is_none_or(|t| t.parent().is_some()) {
            return false;
        }
        let Some(app_id) = self.app_id(m) else {
            return false;
        };
        let Some(w) = self.profile_pending.take(&app_id) else {
            return false;
        };
        self.place_saved(idx, &w);
        true
    }
}

fn profile_delete(name: &str) -> Result<String, String> {
    let path = path(name)?;
    if !path.exists() {
        return Err(format!("no profile {name}"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(format!("deleted profile {name}"))
}

fn profile_rename(old: &str, new: &str) -> Result<String, String> {
    let (from, to) = (path(old)?, path(new)?);
    if !from.exists() {
        return Err(format!("no profile {old}"));
    }
    if to.exists() {
        return Err(format!("profile {new} already exists"));
    }
    std::fs::rename(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
    Ok(format!("renamed profile {old} to {new}"))
}

/// Every profile with what it holds, for the profiles page of pzwm config.
fn profile_list_json() -> String {
    let profiles: Vec<serde_json::Value> = profile_list()
        .lines()
        .filter_map(|name| {
            let path = path(name).ok()?;
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let profile: Profile = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            let mut apps: Vec<&str> = profile.windows.iter().map(|w| w.app_id.as_str()).collect();
            apps.sort_unstable();
            apps.dedup();
            let mut workspaces: Vec<usize> = profile.windows.iter().map(|w| w.ws + 1).collect();
            workspaces.sort_unstable();
            workspaces.dedup();
            Some(serde_json::json!({
                "name": name,
                "windows": profile.windows.len(),
                "apps": apps,
                "workspaces": workspaces,
                "drawings": profile.drawings.as_ref().map_or(0, Vec::len),
                "modified": modified,
                "autosave": name == AUTOSAVE,
            }))
        })
        .collect();
    serde_json::Value::from(profiles).to_string()
}

fn profile_list() -> String {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return String::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".json")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    names.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(app_id: &str) -> SavedWindow {
        SavedWindow {
            app_id: app_id.into(),
            ws: 0,
            x: 0.0,
            y: 0.0,
            w: 800,
            h: 600,
            maximized: false,
            group: None,
        }
    }

    #[test]
    fn assign_pairs_windows_of_the_same_app_in_order() {
        let saved = [saved("firefox"), saved("ghostty"), saved("firefox")];
        let open = ["ghostty".to_string(), "firefox".into(), "firefox".into()];
        let (pairs, missing) = assign(&saved, &open);
        assert_eq!(pairs, vec![(0, 1), (1, 0), (2, 2)]);
        assert!(missing.is_empty());
    }

    #[test]
    fn assign_reports_apps_that_are_not_open() {
        let saved = [saved("firefox"), saved("code")];
        let open = ["firefox".to_string()];
        let (pairs, missing) = assign(&saved, &open);
        assert_eq!(pairs, vec![(0, 0)]);
        assert_eq!(missing, vec![1]);
    }

    #[test]
    fn profile_names_cannot_escape_the_directory() {
        assert!(valid_name("work-2"));
        assert!(!valid_name("../etc"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(""));
    }

    #[test]
    fn only_plain_app_ids_are_launched() {
        assert!(launchable("org.mozilla.firefox"));
        assert!(!launchable("x; rm -rf ~"));
        assert!(!launchable(""));
    }

    #[test]
    fn profile_round_trips_through_json() {
        let p = Profile {
            windows: vec![saved("ghostty")],
            ..Default::default()
        };
        let back: Profile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.windows, p.windows);
    }
}
