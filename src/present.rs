// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Maksym Shymchenko (Makstvell)

//! Presentation mode: `present` walks the camera through the bookmarks of
//! the current workspace, one slide each, like a Prezi or a Figma
//! prototype. Bookmarks named with numbers come first, in number order,
//! then the others by name. `present next|prev` steps, `present stop` (or
//! `present` again) ends the show and flies back to where it started.

use std::{cmp::Ordering, time::Duration};

use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::{canvas::Camera, state::State};

/// Shortest automatic slide time; faster would only be a blur.
const MIN_INTERVAL_SECS: f64 = 1.0;

#[derive(Debug)]
pub struct Show {
    slot: usize,
    slides: Vec<(String, Camera)>,
    index: usize,
    /// Where the camera was before the show.
    start: Camera,
    /// Tells this show's auto-advance timer apart from older ones.
    generation: u64,
}

/// `1 < 2 < 10 < intro < outro`.
fn slide_order(a: &str, b: &str) -> Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

fn sorted_slides(mut slides: Vec<(String, Camera)>) -> Vec<(String, Camera)> {
    slides.sort_by(|a, b| slide_order(&a.0, &b.0));
    slides
}

impl State {
    /// `present [start|stop|next|prev]`; plain `present` starts or stops.
    pub fn present_command(&mut self, args: &[&str]) -> Result<String, String> {
        match args {
            [] if self.presentation.is_some() => self.present_stop(),
            [] | ["start"] => self.present_start(),
            ["stop"] => self.present_stop(),
            ["next"] => self.present_step(1),
            ["prev" | "previous"] => self.present_step(-1),
            _ => Err("usage: present [start | stop | next | prev]".into()),
        }
    }

    fn present_start(&mut self) -> Result<String, String> {
        let output = self.active_output().ok_or("no output")?;
        let ws = self.output_ws(&output);
        let slot = self.camera_slot(ws);
        let slides = sorted_slides(self.jumps.slot_bookmarks(slot));
        if slides.is_empty() {
            return Err(format!(
                "no bookmarks on workspace {}: save some with Super+Alt+Shift+1 … 5",
                ws + 1
            ));
        }
        let generation = self.presentation.as_ref().map_or(0, |s| s.generation + 1);
        let start = self.workspace(ws).target();
        let count = slides.len();
        self.presentation = Some(Show {
            slot,
            slides,
            index: 0,
            start,
            generation,
        });
        self.show_slide();
        self.schedule_next_slide(generation);
        Ok(format!(
            "presentation: {count} slides (Super+Page Down / Page Up, Super+F5 ends it)"
        ))
    }

    fn present_stop(&mut self) -> Result<String, String> {
        let show = self.presentation.take().ok_or("no presentation running")?;
        if let Some(output) = self
            .outputs
            .iter()
            .find(|o| self.camera_slot(self.output_ws(o)) == show.slot)
            .cloned()
        {
            self.fly_to(&output, show.start);
        }
        Ok("presentation ended".into())
    }

    fn present_step(&mut self, by: isize) -> Result<String, String> {
        let show = self
            .presentation
            .as_mut()
            .ok_or("no presentation running")?;
        let last = show.slides.len() as isize - 1;
        let next = (show.index as isize + by).clamp(0, last) as usize;
        show.index = next;
        self.show_slide();
        Ok(format!("slide {}", next + 1))
    }

    fn show_slide(&mut self) {
        let Some(show) = &self.presentation else {
            return;
        };
        let (slot, cam) = (show.slot, show.slides[show.index].1);
        let output = self
            .outputs
            .iter()
            .find(|o| self.camera_slot(self.output_ws(o)) == slot)
            .cloned();
        if let Some(output) = output {
            self.fly_to(&output, cam);
        }
    }

    /// With `[presentation] interval`, moves on by itself; the show ends
    /// after its last slide has had its time.
    fn schedule_next_slide(&mut self, generation: u64) {
        let interval = self.config.presentation.interval;
        if interval <= 0.0 {
            return;
        }
        let every = Duration::from_secs_f64(interval.max(MIN_INTERVAL_SECS));
        let timer = Timer::from_duration(every);
        let added = self.loop_handle.insert_source(timer, move |_, _, state| {
            let Some(show) = state.presentation.as_mut() else {
                return TimeoutAction::Drop;
            };
            if show.generation != generation {
                return TimeoutAction::Drop;
            }
            if show.index + 1 >= show.slides.len() {
                let _ = state.present_stop();
                return TimeoutAction::Drop;
            }
            show.index += 1;
            state.show_slide();
            TimeoutAction::ToDuration(every)
        });
        if let Err(err) = added {
            tracing::warn!("presentation timer: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_slides_come_first_in_number_order() {
        let cam = Camera::default();
        let names = ["outro", "10", "2", "intro", "1"];
        let slides = sorted_slides(names.iter().map(|n| (n.to_string(), cam)).collect());
        let order: Vec<&str> = slides.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(order, ["1", "2", "10", "intro", "outro"]);
    }
}
