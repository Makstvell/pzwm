// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Light or dark apps, `[appearance] color_scheme`.
//!
//! GTK 4 / libadwaita apps, Firefox, Chromium and Electron read the
//! `color-scheme` setting (through the GTK portal); older GTK 3 apps read
//! the GTK theme name. Qt apps follow neither unless a platform theme such
//! as qt6ct is loaded, so pzwm sets `QT_QPA_PLATFORMTHEME` before anything
//! starts (`[appearance] qt_platform_theme`).

use std::path::Path;

use tracing::info;

use crate::state::State;

/// What `color_scheme` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Dark,
    Light,
    /// Leave the setting alone.
    System,
}

impl Scheme {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "dark" | "prefer-dark" => Self::Dark,
            "light" | "prefer-light" => Self::Light,
            _ => Self::System,
        }
    }
}

/// Shell commands that switch GTK apps (and the DankMaterialShell bar) to
/// `scheme`. The GTK theme only changes between Adwaita and Adwaita-dark,
/// so a custom theme stays.
pub fn scheme_command(scheme: Scheme) -> Option<String> {
    let (gnome, from, to, dms) = match scheme {
        Scheme::Dark => ("prefer-dark", "Adwaita", "Adwaita-dark", "dark"),
        Scheme::Light => ("prefer-light", "Adwaita-dark", "Adwaita", "light"),
        Scheme::System => return None,
    };
    let key = "org.gnome.desktop.interface";
    Some(format!(
        "command -v gsettings >/dev/null && {{ \
         gsettings set {key} color-scheme '{gnome}'; \
         case \"$(gsettings get {key} gtk-theme)\" in \"'{from}'\"|\"''\") \
         gsettings set {key} gtk-theme '{to}';; esac; }}; \
         command -v dms >/dev/null && for _ in $(seq 1 20); do \
         dms ipc call theme {dms} >/dev/null 2>&1 && break; sleep 1; done"
    ))
}

/// The Qt platform theme to export: `setting` itself, or for "auto" qt6ct
/// when it is installed. `None`: leave the environment alone (also when
/// the user already set one).
pub fn qt_platform_theme(setting: &str, current: Option<&str>) -> Option<String> {
    if current.is_some_and(|c| !c.is_empty()) {
        return None;
    }
    match setting.trim() {
        "" => None,
        "auto" => qt6ct_installed().then(|| "qt6ct".into()),
        name => Some(name.into()),
    }
}

fn qt6ct_installed() -> bool {
    ["/usr/bin/qt6ct", "/usr/local/bin/qt6ct"]
        .iter()
        .any(|p| Path::new(p).exists())
}

impl State {
    /// Applies `[appearance] color_scheme` (at start and when it changes).
    pub fn apply_color_scheme(&mut self) {
        let scheme = Scheme::parse(&self.config.appearance.color_scheme);
        if Some(scheme) == self.applied_scheme {
            return;
        }
        self.applied_scheme = Some(scheme);
        if let Some(cmd) = scheme_command(scheme) {
            info!("color scheme: {scheme:?}");
            self.spawn(&cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_parse() {
        assert_eq!(Scheme::parse("dark"), Scheme::Dark);
        assert_eq!(Scheme::parse(" Light "), Scheme::Light);
        assert_eq!(Scheme::parse("system"), Scheme::System);
        assert_eq!(Scheme::parse("whatever"), Scheme::System);
    }

    #[test]
    fn dark_sets_prefer_dark_and_keeps_custom_gtk_themes() {
        let cmd = scheme_command(Scheme::Dark).unwrap();
        assert!(cmd.contains("color-scheme 'prefer-dark'"));
        assert!(cmd.contains("\"'Adwaita'\"|\"''\") gsettings set org.gnome.desktop.interface gtk-theme 'Adwaita-dark'"));
        assert!(cmd.contains("dms ipc call theme dark"));
        assert_eq!(scheme_command(Scheme::System), None);
    }

    #[test]
    fn qt_theme_respects_what_is_set() {
        assert_eq!(qt_platform_theme("kvantum", None), Some("kvantum".into()));
        assert_eq!(qt_platform_theme("kvantum", Some("qt5ct")), None);
        assert_eq!(qt_platform_theme("", None), None);
    }
}
