// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Size hints while a window's size is chosen (dragging out an area,
//! resizing): "ideal" sizes pull the size in when it comes near and show
//! as an outline before that. The main one is the whole screen at 100 %
//! zoom, where the app's pixels match the screen's. Holding Alt turns the
//! pull off.
//! pzwm draws no text, so the size snapped to is named in a notification.

use smithay::{
    output::Output,
    utils::{Logical, Rectangle, Size},
};

use crate::state::State;

/// Closer than this (screen pixels) the size snaps to the ideal one.
pub const SNAP_PX: f64 = 24.0;
/// Closer than this the ideal size shows as an outline.
pub const SHOW_PX: f64 = 120.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Ideal {
    /// Frame size (title bar included), canvas units at 100 % zoom.
    pub size: Size<f64, Logical>,
    pub name: String,
}

/// What the hint shows on an output during a drag.
pub struct SizeHint {
    pub output: Output,
    /// Outline of the ideal frame, output-local screen units.
    pub rect: Rectangle<f64, Logical>,
    pub ideal: Ideal,
    /// The size is on the ideal one now.
    pub snapped: bool,
}

/// Ideal frame sizes for a usable screen area of `area` (logical pixels,
/// which are canvas units at 100 %), with `gap` around windows and a
/// title bar of `title`.
/// An entry of `[canvas] ideal_sizes`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IdealSpec {
    /// The display's resolution, without a bar that reserves space.
    Screen,
    /// A window whose content is this big, logical pixels.
    Content(f64, f64),
}

/// Parses "screen" or "WIDTHxHEIGHT"; `None` for anything else.
pub fn parse_ideal(s: &str) -> Option<IdealSpec> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("screen") {
        return Some(IdealSpec::Screen);
    }
    let (w, h) = s.split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
    (w >= 1.0 && h >= 1.0 && w.is_finite() && h.is_finite()).then_some(IdealSpec::Content(w, h))
}

/// Ideal frame sizes for a usable screen `area` (logical pixels, which
/// are canvas units at 100 %) and a title bar of `title`. No valid entry:
/// the screen alone.
pub fn ideal_sizes(area: Size<f64, Logical>, title: f64, specs: &[IdealSpec]) -> Vec<Ideal> {
    let one = |spec: &IdealSpec| match *spec {
        IdealSpec::Screen => Ideal {
            size: area,
            name: "the whole screen at 100 %".into(),
        },
        IdealSpec::Content(w, h) => Ideal {
            size: Size::from((w, h + title)),
            name: format!("{w} × {h}"),
        },
    };
    if specs.is_empty() {
        return vec![one(&IdealSpec::Screen)];
    }
    specs.iter().map(one).collect()
}

/// The ideal size nearest to `size` and how far it is (the larger of the
/// differences along the axes in `axes`: width, height).
pub fn nearest(
    size: Size<f64, Logical>,
    ideals: &[Ideal],
    axes: (bool, bool),
) -> Option<(Ideal, f64)> {
    let dist = |i: &Ideal| {
        let dw = if axes.0 {
            (i.size.w - size.w).abs()
        } else {
            0.0
        };
        let dh = if axes.1 {
            (i.size.h - size.h).abs()
        } else {
            0.0
        };
        dw.max(dh)
    };
    ideals
        .iter()
        .map(|i| (i.clone(), dist(i)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|_| axes.0 || axes.1)
}

/// `size` pulled onto the nearest ideal one along `axes` when it is within
/// `tolerance`; also the ideal shown, when within `show`.
pub fn pull(
    size: Size<f64, Logical>,
    ideals: &[Ideal],
    axes: (bool, bool),
    tolerance: f64,
    show: f64,
) -> (Size<f64, Logical>, Option<(Ideal, bool)>) {
    let Some((ideal, dist)) = nearest(size, ideals, axes) else {
        return (size, None);
    };
    if dist > show {
        return (size, None);
    }
    if dist > tolerance {
        return (size, Some((ideal, false)));
    }
    let pulled = Size::from((
        if axes.0 { ideal.size.w } else { size.w },
        if axes.1 { ideal.size.h } else { size.h },
    ));
    (pulled, Some((ideal, true)))
}

impl State {
    /// Ideal frame sizes for windows on `output`.
    pub fn ideal_sizes(&self, output: &Output) -> Vec<Ideal> {
        let specs: Vec<IdealSpec> = self
            .config
            .canvas
            .ideal_sizes
            .iter()
            .filter_map(|s| parse_ideal(s))
            .collect();
        let title = self.config.appearance.titlebar_height as f64;
        // "screen" is the display's resolution, without a bar that reserves space.
        ideal_sizes(self.usable_area(output).size.to_f64(), title, &specs)
    }

    /// Alt held: sizes are free, nothing pulls.
    pub fn size_hints_off(&self) -> bool {
        !self.config.canvas.size_hints
            || self
                .seat
                .get_keyboard()
                .is_some_and(|k| k.modifier_state().alt)
    }

    /// The drag ended: name the ideal size it snapped to.
    pub fn finish_size_hint(&mut self) {
        let Some(hint) = self.size_hint.take() else {
            return;
        };
        if hint.snapped {
            let quote = crate::bindings::shell_quote;
            let s = hint.ideal.size;
            let px = s.upscale(hint.output.current_scale().fractional_scale());
            self.spawn(&format!(
                "notify-send -a pzwm -t 2500 -h string:x-canonical-private-synchronous:pzwm-size {} {}",
                quote(&format!(
                    "{} × {} ({} × {} px)",
                    s.w.round(),
                    s.h.round(),
                    px.w.round(),
                    px.h.round()
                )),
                quote(&format!("Ideal size: {}. Hold Alt for a free size.", hint.ideal.name)),
            ));
        }
        self.queue_redraw_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: f64, h: f64) -> Size<f64, Logical> {
        Size::from((w, h))
    }

    #[test]
    fn by_default_the_ideal_is_the_screen() {
        let ideals = ideal_sizes(size(1707.0, 960.0), 30.0, &[]);
        assert_eq!(ideals.len(), 1);
        assert_eq!(ideals[0].size, size(1707.0, 960.0));
    }

    #[test]
    fn own_sizes_are_content_plus_the_title_bar() {
        let specs = [IdealSpec::Screen, IdealSpec::Content(1280.0, 720.0)];
        let ideals = ideal_sizes(size(1707.0, 960.0), 30.0, &specs);
        assert_eq!(ideals[1].size, size(1280.0, 750.0));
        assert_eq!(ideals[1].name, "1280 × 720");
    }

    #[test]
    fn ideal_entries_parse() {
        assert_eq!(parse_ideal("screen"), Some(IdealSpec::Screen));
        assert_eq!(
            parse_ideal(" 1920x1080 "),
            Some(IdealSpec::Content(1920.0, 1080.0))
        );
        assert_eq!(parse_ideal("0x10"), None);
        assert_eq!(parse_ideal("big"), None);
    }

    #[test]
    fn a_near_size_is_pulled_in() {
        let ideals = ideal_sizes(size(1000.0, 600.0), 0.0, &[]);
        let (pulled, hint) = pull(size(990.0, 610.0), &ideals, (true, true), 24.0, 120.0);
        assert_eq!(pulled, size(1000.0, 600.0));
        assert_eq!(hint.map(|(_, snapped)| snapped), Some(true));
    }

    #[test]
    fn a_size_on_the_way_only_shows_the_outline() {
        let ideals = ideal_sizes(size(1000.0, 600.0), 0.0, &[]);
        let (pulled, hint) = pull(size(940.0, 600.0), &ideals, (true, true), 24.0, 120.0);
        assert_eq!(pulled, size(940.0, 600.0));
        assert_eq!(hint.map(|(_, snapped)| snapped), Some(false));
    }

    #[test]
    fn a_far_size_shows_nothing() {
        let ideals = ideal_sizes(size(3000.0, 2000.0), 0.0, &[]);
        let (_, hint) = pull(size(200.0, 150.0), &ideals, (true, true), 24.0, 120.0);
        assert_eq!(hint, None);
    }

    #[test]
    fn only_the_resized_axis_snaps() {
        let ideals = ideal_sizes(size(1000.0, 600.0), 0.0, &[]);
        // Dragging the right edge: the height stays what it is.
        let (pulled, _) = pull(size(995.0, 123.0), &ideals, (true, false), 24.0, 120.0);
        assert_eq!(pulled, size(1000.0, 123.0));
    }
}
