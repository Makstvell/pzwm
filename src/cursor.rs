// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! XCursor theme loading with a per-(name, size) cache.

use std::collections::HashMap;

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::pointer::CursorIcon,
    utils::{Logical, Point, Transform},
};
use tracing::warn;

pub struct CursorImage {
    pub buffer: MemoryRenderBuffer,
    /// Hotspot in logical pixels.
    pub hotspot: Point<i32, Logical>,
}

pub struct CursorTheme {
    theme: xcursor::CursorTheme,
    size: u32,
    cache: HashMap<(CursorIcon, i32), Option<CursorImage>>,
}

impl CursorTheme {
    pub fn load(name: &str, size: u32) -> Self {
        Self {
            theme: xcursor::CursorTheme::load(name),
            size: size.max(8),
            cache: HashMap::new(),
        }
    }

    /// Image for `icon` at the integer `scale` (ceil of the output scale).
    pub fn get(&mut self, icon: CursorIcon, scale: i32) -> Option<&CursorImage> {
        let scale = scale.max(1);
        if !self.cache.contains_key(&(icon, scale)) {
            let image = self
                .load_icon(icon, scale)
                .or_else(|| self.load_icon(CursorIcon::Default, scale))
                .or_else(|| Some(fallback_arrow(scale)));
            self.cache.insert((icon, scale), image);
        }
        self.cache.get(&(icon, scale)).and_then(Option::as_ref)
    }

    fn load_icon(&self, icon: CursorIcon, scale: i32) -> Option<CursorImage> {
        let names = std::iter::once(icon.name()).chain(icon.alt_names().iter().copied());
        let path = names.into_iter().find_map(|n| self.theme.load_icon(n))?;
        let data = std::fs::read(&path)
            .inspect_err(|err| warn!("cursor {}: {err}", path.display()))
            .ok()?;
        let images = xcursor::parser::parse_xcursor(&data)?;
        let want = self.size as i32 * scale;
        let nearest = images
            .iter()
            .min_by_key(|img| (img.size as i32 - want).abs())?
            .size;
        // Animated cursors: use the first frame of the best size.
        let img = images.iter().find(|img| img.size == nearest)?;
        let buffer = MemoryRenderBuffer::from_slice(
            &img.pixels_rgba,
            Fourcc::Argb8888,
            (img.width as i32, img.height as i32),
            scale,
            Transform::Normal,
            None,
        );
        Some(CursorImage {
            buffer,
            hotspot: Point::from((img.xhot as i32 / scale, img.yhot as i32 / scale)),
        })
    }
}

/// A small arrow so there is always a visible pointer, even with no theme.
fn fallback_arrow(scale: i32) -> CursorImage {
    const W: usize = 12;
    const H: usize = 18;
    let s = scale as usize;
    let (w, h) = (W * s, H * s);
    let mut pixels = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x / s, y / s);
            let inside = fx <= fy && fy < 16 && fx < 11 && (fy < 12 || fx < 4 || fx + 4 > fy);
            let edge = inside && (fx == 0 || fx == fy || fy == 15);
            let px = &mut pixels[(y * w + x) * 4..][..4];
            if edge {
                px.copy_from_slice(&[0, 0, 0, 255]);
            } else if inside {
                px.copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    CursorImage {
        buffer: MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (w as i32, h as i32),
            scale,
            Transform::Normal,
            None,
        ),
        hotspot: Point::from((0, 0)),
    }
}
