# Similar projects

Compositors and window managers that also put windows on an infinite,
pannable (and often zoomable) plane. Checked 2026-10-28; maintenance status
was not verified, so look at each repository before relying on it.

| Project | Base | How it differs from pzwm |
|---|---|---|
| [driftwm](https://github.com/malbiruk/driftwm) | Rust, smithay | Closest match. Trackpad-first: momentum panning, pinch zoom, 3-finger pan. GPL-3.0-or-later, in the AUR (`driftwm-git`). Described as experimental. |
| [Nourish](https://phoronix.com/news/Nourish-Wayland-Compositor) | Rust, smithay + wgpu + iced | Infinite zoom and pan, Vulkan renderer. MIT/Apache-2.0. New (June 2026). |
| [hevel](https://zff.dev/derivelinux/hevel-site) | — | Scrollable floating plane in every direction, all commands by mouse chords. |
| [phantomat](https://github.com/kaolti/phantomat/wiki) | Hyprland | One endless zoomable plane instead of workspaces, inside Hyprland. |
| [CanvasWM](https://codeberg.org/mickkc/CanvasWM) | wlroots | Every workspace is an infinite canvas. Experimental. |
| [OceanWM](https://codeberg.org/jake-c/OceanWM) | — | Infinite canvas compositor, alpha. |
| [niri](https://github.com/YaLTeR/niri) | Rust, smithay | Not a canvas: scrollable tiling (an endless strip of columns). Worth reading for smithay patterns. |

What pzwm has that these mostly do not: per-workspace cameras, a minimap
with live previews, desktop profiles (`profile save|load`), monitor
mirroring, a DankMaterialShell workspace feed, drawing on the canvas,
semantic zoom, presentations through bookmarks and one-shot arrange.

Ideas worth borrowing: driftwm's pan momentum for touchpads, and niri's IPC
(a JSON event stream many bars already support).
