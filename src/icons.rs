// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! App icons for semantic zoom: app id → `.desktop` file → `Icon=` →
//! a PNG in the hicolor theme or `pixmaps`. Looked up once per app and
//! kept; SVG-only icons are not decoded (the card shows without one).

use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
};

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Logical, Size, Transform},
};

/// A decoded icon and its size in pixels.
pub type Icon = (MemoryRenderBuffer, Size<i32, Logical>);

/// Theme sizes tried, best first.
const SIZES: [u32; 7] = [256, 128, 512, 96, 64, 48, 32];
/// Larger images are skipped instead of uploaded.
const MAX_SIDE: u32 = 1024;

#[derive(Debug, Default)]
pub struct Icons {
    cache: HashMap<String, Option<Icon>>,
}

impl Icons {
    pub fn get(&mut self, app_id: &str) -> Option<Icon> {
        self.cache
            .entry(app_id.to_string())
            .or_insert_with(|| load(app_id))
            .clone()
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        .into_iter()
        .collect();
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(PathBuf::from));
    if let Some(h) = home {
        dirs.push(h.join(".local/share/flatpak/exports/share"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs
}

/// The `Icon=` of the `[Desktop Entry]` group, and its `StartupWMClass=`.
fn desktop_keys(text: &str) -> (Option<String>, Option<String>) {
    let (mut icon, mut class) = (None, None);
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some(v) = line.strip_prefix("Icon=") {
                icon = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
                class = Some(v.trim().to_string());
            }
        }
    }
    (icon, class)
}

fn icon_name(app_id: &str, dirs: &[PathBuf]) -> String {
    let lower = app_id.to_lowercase();
    for dir in dirs.iter().map(|d| d.join("applications")) {
        for name in [app_id, lower.as_str()] {
            if let Ok(text) = std::fs::read_to_string(dir.join(format!("{name}.desktop"))) {
                if let (Some(icon), _) = desktop_keys(&text) {
                    return icon;
                }
            }
        }
    }
    // X11 and older apps: a desktop file naming the app id as its class.
    for dir in dirs.iter().map(|d| d.join("applications")) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if let (Some(icon), Some(class)) = desktop_keys(&text) {
                if class.eq_ignore_ascii_case(app_id) {
                    return icon;
                }
            }
        }
    }
    app_id.to_string()
}

/// The `Name=` (not a translated one) of the `[Desktop Entry]` group.
fn desktop_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if let Some(v) = line.strip_prefix("Name=").filter(|_| in_entry) {
            return Some(v.trim().to_string()).filter(|v| !v.is_empty());
        }
    }
    None
}

/// A readable name for an app id, for lists such as find: the `.desktop`
/// file's `Name=`, "Web app" for browser app windows (their ids are long
/// generated strings), else the id itself.
pub fn app_name(app_id: &str) -> String {
    const WEB_APPS: [&str; 4] = ["chrome-", "chromium-", "msedge-", "brave-"];
    if WEB_APPS.iter().any(|p| app_id.starts_with(p)) && app_id.contains('_') {
        return "Web app".into();
    }
    let lower = app_id.to_lowercase();
    data_dirs()
        .iter()
        .map(|d| d.join("applications"))
        .flat_map(|dir| {
            [
                dir.join(format!("{app_id}.desktop")),
                dir.join(format!("{lower}.desktop")),
            ]
        })
        .find_map(|path| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|t| desktop_name(&t))
        })
        .unwrap_or_else(|| app_id.to_string())
}

fn icon_file(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return (path.extension().is_some_and(|e| e == "png") && path.exists())
            .then(|| path.to_path_buf());
    }
    let name = name.strip_suffix(".png").unwrap_or(name);
    if name.contains('/') {
        return None;
    }
    let themed = dirs.iter().flat_map(|d| {
        SIZES
            .iter()
            .map(move |s| d.join(format!("icons/hicolor/{s}x{s}/apps/{name}.png")))
    });
    let pixmaps = dirs.iter().map(|d| d.join(format!("pixmaps/{name}.png")));
    themed.chain(pixmaps).find(|p| p.exists())
}

fn load(app_id: &str) -> Option<Icon> {
    if app_id.is_empty() {
        return None;
    }
    let dirs = data_dirs();
    let path = icon_file(&icon_name(app_id, &dirs), &dirs)?;
    let (w, h, rgba) = decode(&path)
        .inspect_err(|err| tracing::debug!("icon {}: {err}", path.display()))
        .ok()?;
    let size = Size::from((w as i32, h as i32));
    let buffer = MemoryRenderBuffer::from_slice(
        &rgba,
        Fourcc::Abgr8888,
        (w as i32, h as i32),
        1,
        Transform::Normal,
        None,
    );
    Some((buffer, size))
}

/// Decodes a PNG into premultiplied RGBA bytes.
fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(File::open(path).map_err(|e| e.to_string())?);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.width > MAX_SIDE || info.height > MAX_SIDE {
        return Err("too large".into());
    }
    let pixels = &buf[..info.buffer_size()];
    Ok((
        info.width,
        info.height,
        to_premultiplied_rgba(pixels, info.color_type),
    ))
}

fn to_premultiplied_rgba(pixels: &[u8], color: png::ColorType) -> Vec<u8> {
    let rgba: Vec<[u8; 4]> = match color {
        png::ColorType::Rgba => pixels
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect(),
        png::ColorType::Rgb => pixels
            .chunks_exact(3)
            .map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks_exact(2)
            .map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        _ => pixels.iter().map(|&g| [g, g, g, 255]).collect(),
    };
    rgba.into_iter()
        .flat_map(|[r, g, b, a]| {
            let k = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
            [k(r), k(g), k(b), a]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_icon_and_class_from_the_main_group_only() {
        assert_eq!(
            desktop_name("[Desktop Entry]\nName[uk]=Фу\nName=Foo\n[Desktop Action a]\nName=Bar\n"),
            Some("Foo".into())
        );
        assert_eq!(app_name("chrome-__tmp_demo_stats.html-Default"), "Web app");
        let text = "[Desktop Entry]\nName=Foo\nIcon=foo-icon\nStartupWMClass=Foo\n\
                    [Desktop Action new]\nIcon=other\n";
        assert_eq!(
            desktop_keys(text),
            (Some("foo-icon".into()), Some("Foo".into()))
        );
    }

    #[test]
    fn premultiplies_alpha() {
        let out = to_premultiplied_rgba(&[255, 128, 0, 128], png::ColorType::Rgba);
        assert_eq!(out, vec![128, 64, 0, 128]);
        let gray = to_premultiplied_rgba(&[10, 20], png::ColorType::Grayscale);
        assert_eq!(gray, vec![10, 10, 10, 255, 20, 20, 20, 255]);
    }

    #[test]
    fn relative_paths_are_not_icon_names() {
        assert_eq!(
            icon_file("../etc/passwd", &[PathBuf::from("/usr/share")]),
            None
        );
    }
}
