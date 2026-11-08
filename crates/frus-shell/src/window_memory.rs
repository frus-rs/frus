// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The desktop window comes back where it was left** (milestone 783): its size, its place
//! and whether it was maximized, kept in the application's data folder
//! ([`crate::app_data_dir`], one per program) as `window.json`.
//!
//! - What is kept is the window's **normal** placement: the bounds it had before it was
//!   maximized, so that a window closed maximized opens maximized and goes back to its own
//!   size when it is restored. A minimized window is never restored minimized.
//! - It is read before the window is shown, which is created hidden: the window appears in
//!   its place, with no jump.
//! - The screens may have changed since: a monitor unplugged, a resolution lowered, a scale
//!   changed. The placement is fitted to the screens there are now ([`fit`]): a window on a
//!   screen that is gone opens centred on the main one, and one bigger than its screen is
//!   shrunk to it.
//! - The size is kept in logical pixels, which a new scale factor converts; the place in
//!   physical ones, which is how the desktop numbers its screens.
//!
//! The application turns it off with `Application::remember_window` returning `false`, or
//! `FrusApp::remember_window(false)`.

use std::path::{Path, PathBuf};

use frus_widgets::__serde_json as json;

/// The file, in the application's data folder.
pub(crate) const FILE: &str = "window.json";

/// How much of a window a screen must still show for the window to stay on it, in physical
/// pixels: enough of it to take hold of. Less than that, and the screen it was on is gone.
const GRIP: (i64, i64) = (100, 50);

/// Where a window is, and how big.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// The top-left corner of the window's frame, in physical pixels of the desktop. `None`
    /// where the system does not tell an application where its window is (Wayland).
    pub(crate) position: Option<(i32, i32)>,
    /// The size of its content, in logical pixels.
    pub(crate) size: (f64, f64),
    /// The scale factor it was measured at.
    pub(crate) scale: f64,
    /// What the frame adds around the content, in physical pixels: borders, a title bar.
    pub(crate) frame: (u32, u32),
    /// Whether it is maximized. [`size`](Self::size) and [`position`](Self::position) are then
    /// the bounds it goes back to.
    pub(crate) maximized: bool,
}

/// A screen of the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Screen {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) scale: f64,
}

impl Placement {
    /// The window as JSON.
    pub(crate) fn encode(&self) -> String {
        let mut map = json::Map::new();
        map.insert("version".into(), 1.into());
        if let Some((x, y)) = self.position {
            map.insert("x".into(), x.into());
            map.insert("y".into(), y.into());
        }
        map.insert("width".into(), self.size.0.into());
        map.insert("height".into(), self.size.1.into());
        map.insert("scale".into(), self.scale.into());
        map.insert("frame_width".into(), self.frame.0.into());
        map.insert("frame_height".into(), self.frame.1.into());
        map.insert("maximized".into(), self.maximized.into());
        json::to_string_pretty(&json::Value::Object(map)).unwrap_or_default()
    }

    /// What [`encode`](Self::encode) wrote, or `None` for anything else: a file from another
    /// version, a size of nothing, a value that is not a number.
    pub(crate) fn decode(text: &str) -> Option<Placement> {
        let value: json::Value = json::from_str(text).ok()?;
        let map = value.as_object()?;
        if map.get("version")?.as_u64()? != 1 {
            return None;
        }
        let number = |key: &str| map.get(key).and_then(json::Value::as_f64);
        let int = |key: &str| {
            map.get(key)
                .and_then(json::Value::as_i64)
                .and_then(|n| i32::try_from(n).ok())
        };
        let size = (number("width")?, number("height")?);
        let scale = number("scale").unwrap_or(1.0);
        let sane = |n: f64| n.is_finite() && n > 0.0 && n < 1.0e6;
        if !(sane(size.0) && sane(size.1) && sane(scale)) {
            return None;
        }
        let position = match (int("x"), int("y")) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };
        let frame = |key: &str| {
            map.get(key)
                .and_then(json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n < 10_000)
                .unwrap_or(0)
        };
        Some(Placement {
            position,
            size,
            scale,
            frame: (frame("frame_width"), frame("frame_height")),
            maximized: map
                .get("maximized")
                .and_then(json::Value::as_bool)
                .unwrap_or(false),
        })
    }

    /// Its frame's rectangle on the desktop, at `scale`: `(x, y, width, height)`.
    fn rect_at(&self, (x, y): (i32, i32), scale: f64) -> (i64, i64, i64, i64) {
        (
            x as i64,
            y as i64,
            (self.size.0 * scale).round() as i64 + self.frame.0 as i64,
            (self.size.1 * scale).round() as i64 + self.frame.1 as i64,
        )
    }
}

/// How much of the rectangle `a` the screen shows: `(width, height)` of the overlap.
fn overlap(a: (i64, i64, i64, i64), screen: &Screen) -> (i64, i64) {
    let (sx, sy) = (screen.x as i64, screen.y as i64);
    let (sw, sh) = (screen.width as i64, screen.height as i64);
    let w = (a.0 + a.2).min(sx + sw) - a.0.max(sx);
    let h = (a.1 + a.3).min(sy + sh) - a.1.max(sy);
    (w.max(0), h.max(0))
}

/// `saved`, fitted to the `screens` there are now — the main one first — for a window no
/// smaller than `min` logical pixels.
///
/// The window stays on the screen that shows the most of it, as long as that is enough to
/// take hold of; it is then moved, and if need be shrunk, until all of it is on that screen.
/// On no screen any more, it opens centred on the main one, at a size that fits there. With no
/// screens to ask about, it is left as it was.
pub(crate) fn fit(saved: &Placement, screens: &[Screen], min: (f64, f64)) -> Placement {
    let Some(main) = screens.first() else {
        return *saved;
    };
    // The screen it is still on, if any.
    let kept = saved.position.and_then(|at| {
        let rect = saved.rect_at(at, saved.scale);
        screens
            .iter()
            .map(|screen| (overlap(rect, screen), screen))
            .filter(|((w, h), _)| *w >= GRIP.0.min(rect.2) && *h >= GRIP.1.min(rect.3))
            .max_by_key(|((w, h), _)| w * h)
            .map(|(_, screen)| screen)
    });
    let screen = kept.unwrap_or(main);
    // As big as it was, but no bigger than the screen, frame included.
    let scale = screen.scale.max(0.1);
    let room = (
        (screen.width.saturating_sub(saved.frame.0)) as f64 / scale,
        (screen.height.saturating_sub(saved.frame.1)) as f64 / scale,
    );
    let size = (
        saved.size.0.min(room.0).max(min.0.min(room.0)),
        saved.size.1.min(room.1).max(min.1.min(room.1)),
    );
    let fitted = Placement { size, ..*saved };
    let position = saved.position.map(|at| {
        let (_, _, w, h) = fitted.rect_at(at, scale);
        let (sx, sy) = (screen.x as i64, screen.y as i64);
        let (sw, sh) = (screen.width as i64, screen.height as i64);
        let (x, y) = if kept.is_some() {
            (
                (at.0 as i64).clamp(sx, (sx + sw - w).max(sx)),
                (at.1 as i64).clamp(sy, (sy + sh - h).max(sy)),
            )
        } else {
            (sx + (sw - w).max(0) / 2, sy + (sh - h).max(0) / 2)
        };
        (x as i32, y as i32)
    });
    Placement { position, ..fitted }
}

/// What the window's placement is, followed as it is moved, resized, maximized and
/// minimized: the normal bounds, and whether it is maximized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tracker {
    placement: Placement,
}

impl Tracker {
    /// Following a window placed as `placement`.
    pub(crate) fn new(placement: Placement) -> Self {
        Self { placement }
    }

    /// The window moved or changed size. A minimized window changes nothing: it opens as it
    /// was before. A maximized one keeps the bounds it had before; `normal` is asked for its
    /// bounds only when it is neither.
    pub(crate) fn note(
        &mut self,
        minimized: bool,
        maximized: bool,
        normal: impl FnOnce() -> Option<Placement>,
    ) {
        if minimized {
            return;
        }
        self.placement.maximized = maximized;
        if maximized {
            return;
        }
        if let Some(now) = normal() {
            self.placement = Placement {
                maximized: false,
                ..now
            };
        }
    }

    /// What is kept.
    pub(crate) fn placement(&self) -> Placement {
        self.placement
    }
}

/// The file in the application's data folder, if it has one.
pub(crate) fn path() -> Option<PathBuf> {
    crate::app_data_dir().map(|dir| dir.join(FILE))
}

/// The placement kept in `path`, if there is one that can be read.
pub(crate) fn load(path: &Path) -> Option<Placement> {
    Placement::decode(&std::fs::read_to_string(path).ok()?)
}

/// Keeps `placement` in `path`, whole or not at all.
pub(crate) fn save(path: &Path, placement: &Placement) -> std::io::Result<()> {
    crate::storage::write_atomically(path, placement.encode().as_bytes())
}

/// The window's placement now, as a normal (not maximized) window.
#[cfg(desktop)]
pub(crate) fn measure(window: &winit::window::Window) -> Option<Placement> {
    let scale = window.scale_factor();
    let inner = window.inner_size();
    if inner.width == 0 || inner.height == 0 {
        return None;
    }
    let outer = window.outer_size();
    let size = inner.to_logical::<f64>(scale);
    Some(Placement {
        position: window.outer_position().ok().map(|p| (p.x, p.y)),
        size: (size.width, size.height),
        scale,
        frame: (
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        ),
        maximized: false,
    })
}

/// The screens of the desktop, the main one first.
#[cfg(desktop)]
pub(crate) fn screens(event_loop: &winit::event_loop::ActiveEventLoop) -> Vec<Screen> {
    let main = event_loop.primary_monitor();
    let mut all: Vec<_> = event_loop.available_monitors().collect();
    if let Some(main) = &main {
        if let Some(at) = all.iter().position(|monitor| monitor == main) {
            let main = all.remove(at);
            all.insert(0, main);
        }
    }
    all.into_iter()
        .map(|monitor| {
            let at = monitor.position();
            let size = monitor.size();
            Screen {
                x: at.x,
                y: at.y,
                width: size.width,
                height: size.height,
                scale: monitor.scale_factor(),
            }
        })
        .filter(|screen| screen.width > 0 && screen.height > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: (f64, f64) = (360.0, 280.0);

    fn screen(x: i32, y: i32, width: u32, height: u32, scale: f64) -> Screen {
        Screen {
            x,
            y,
            width,
            height,
            scale,
        }
    }

    fn placed(x: i32, y: i32, w: f64, h: f64, scale: f64) -> Placement {
        Placement {
            position: Some((x, y)),
            size: (w, h),
            scale,
            frame: (16, 39),
            maximized: false,
        }
    }

    #[test]
    fn what_is_written_is_read_back() {
        let kept = Placement {
            maximized: true,
            ..placed(-1500, 120, 1024.5, 700.0, 1.5)
        };
        assert_eq!(Placement::decode(&kept.encode()), Some(kept));
        let wayland = Placement {
            position: None,
            ..kept
        };
        assert_eq!(Placement::decode(&wayland.encode()), Some(wayland));
    }

    #[test]
    fn a_file_that_is_not_ours_is_ignored() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"version":2,"width":800,"height":600}"#,
            r#"{"version":1,"width":0,"height":600}"#,
            r#"{"version":1,"width":"800","height":600}"#,
            r#"{"version":1,"width":800,"height":-1}"#,
        ] {
            assert_eq!(Placement::decode(text), None, "{text}");
        }
        // The fields a later version may add, or an older one left out, are not a reason to
        // forget the window.
        assert_eq!(
            Placement::decode(r#"{"version":1,"width":800,"height":600,"extra":true}"#),
            Some(Placement {
                position: None,
                size: (800.0, 600.0),
                scale: 1.0,
                frame: (0, 0),
                maximized: false,
            })
        );
    }

    #[test]
    fn a_window_on_a_screen_that_is_still_there_stays_put() {
        let screens = [screen(0, 0, 1920, 1080, 1.0)];
        let saved = placed(200, 150, 900.0, 640.0, 1.0);
        assert_eq!(fit(&saved, &screens, MIN), saved);
    }

    #[test]
    fn a_window_on_a_second_screen_stays_on_it() {
        let screens = [
            screen(0, 0, 1920, 1080, 1.0),
            screen(-2560, -200, 2560, 1440, 1.25),
        ];
        let saved = placed(-2000, 100, 1000.0, 700.0, 1.25);
        assert_eq!(fit(&saved, &screens, MIN), saved);
    }

    #[test]
    fn a_window_whose_screen_was_unplugged_opens_centred_on_the_main_one() {
        let screens = [screen(0, 0, 1920, 1080, 1.0)];
        let saved = placed(-2000, 100, 1000.0, 700.0, 1.25);
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.size, (1000.0, 700.0));
        let (w, h) = (1000 + 16, 700 + 39);
        assert_eq!(fitted.position, Some(((1920 - w) / 2, (1080 - h) / 2)));
    }

    #[test]
    fn a_window_barely_on_a_screen_is_brought_back_whole() {
        let screens = [screen(0, 0, 1920, 1080, 1.0)];
        // Only 200 by 100 px of it left at the bottom right corner: enough to hold, so the
        // screen is the same, and the window is moved until all of it is on it.
        let saved = placed(1720, 980, 800.0, 600.0, 1.0);
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.size, (800.0, 600.0));
        assert_eq!(fitted.position, Some((1920 - 816, 1080 - 639)));
        // A sliver of 20 px is not: the window is treated as lost, and centred.
        let lost = fit(&placed(1900, 300, 800.0, 600.0, 1.0), &screens, MIN);
        assert_eq!(lost.position, Some(((1920 - 816) / 2, (1080 - 639) / 2)));
    }

    #[test]
    fn a_lower_resolution_shrinks_the_window_to_its_screen() {
        // Saved on a 2560 by 1440 screen, the screen now 1280 by 720.
        let screens = [screen(0, 0, 1280, 720, 1.0)];
        let saved = placed(100, 100, 2400.0, 1300.0, 1.0);
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.size, (1280.0 - 16.0, 720.0 - 39.0));
        assert_eq!(fitted.position, Some((0, 0)));
    }

    #[test]
    fn a_higher_scale_keeps_the_logical_size_when_it_fits() {
        // The same 1920 by 1080 screen, its scale raised from 100% to 150%: 1000 logical px
        // are 1500 physical ones now, still on the screen once moved left.
        let screens = [screen(0, 0, 1920, 1080, 1.5)];
        let saved = placed(800, 100, 1000.0, 600.0, 1.0);
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.size, (1000.0, 600.0));
        assert_eq!(fitted.position, Some((1920 - 1516, 100)));
        // And at 200%, 1000 logical px no longer fit: what does is kept.
        let screens = [screen(0, 0, 1920, 1080, 2.0)];
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.size, ((1920.0 - 16.0) / 2.0, (1080.0 - 39.0) / 2.0));
    }

    #[test]
    fn the_minimum_size_holds_unless_the_screen_is_smaller() {
        let screens = [screen(0, 0, 1920, 1080, 1.0)];
        let tiny = placed(10, 10, 50.0, 40.0, 1.0);
        assert_eq!(fit(&tiny, &screens, MIN).size, MIN);
        let small_screen = [screen(0, 0, 300, 200, 1.0)];
        assert_eq!(
            fit(&tiny, &small_screen, MIN).size,
            (300.0 - 16.0, 200.0 - 39.0)
        );
    }

    #[test]
    fn with_no_position_only_the_size_is_fitted() {
        let screens = [screen(0, 0, 1280, 720, 1.0)];
        let saved = Placement {
            position: None,
            ..placed(0, 0, 2000.0, 600.0, 1.0)
        };
        let fitted = fit(&saved, &screens, MIN);
        assert_eq!(fitted.position, None);
        assert_eq!(fitted.size, (1280.0 - 16.0, 600.0));
    }

    #[test]
    fn with_no_screens_known_nothing_changes() {
        let saved = placed(-5000, -5000, 800.0, 600.0, 1.0);
        assert_eq!(fit(&saved, &[], MIN), saved);
    }

    #[test]
    fn maximized_is_kept_over_the_normal_bounds() {
        let screens = [screen(0, 0, 1920, 1080, 1.0)];
        let saved = Placement {
            maximized: true,
            ..placed(300, 200, 900.0, 600.0, 1.0)
        };
        let fitted = fit(&saved, &screens, MIN);
        assert!(fitted.maximized);
        assert_eq!(fitted.size, (900.0, 600.0));
        assert_eq!(fitted.position, Some((300, 200)));
    }

    #[test]
    fn the_tracker_keeps_the_normal_bounds_under_maximized_and_minimized() {
        let first = placed(100, 100, 800.0, 600.0, 1.0);
        let mut tracker = Tracker::new(first);
        let moved = placed(250, 180, 820.0, 610.0, 1.0);
        tracker.note(false, false, || Some(moved));
        assert_eq!(tracker.placement(), moved);

        // Maximized: the bounds it had are kept, not the screen's.
        tracker.note(false, true, || panic!("not asked while maximized"));
        assert_eq!(
            tracker.placement(),
            Placement {
                maximized: true,
                ..moved
            }
        );
        // Minimized from there: still maximized, with the same bounds behind it.
        tracker.note(true, false, || panic!("not asked while minimized"));
        assert!(tracker.placement().maximized);
        assert_eq!(tracker.placement().size, moved.size);

        // Restored: normal again, where it was.
        tracker.note(false, false, || Some(moved));
        assert_eq!(tracker.placement(), moved);
        // A window that cannot be measured keeps what was known.
        tracker.note(false, false, || None);
        assert_eq!(tracker.placement(), moved);
    }

    #[test]
    fn a_placement_is_saved_and_loaded_from_a_file() {
        let dir = std::env::temp_dir().join(format!("frus-window-{}", std::process::id()));
        let path = dir.join(FILE);
        assert_eq!(load(&path), None, "nothing saved yet");
        let kept = Placement {
            maximized: true,
            ..placed(40, 60, 1111.0, 777.0, 1.25)
        };
        save(&path, &kept).unwrap();
        assert_eq!(load(&path), Some(kept));
        std::fs::write(&path, "{ broken").unwrap();
        assert_eq!(load(&path), None, "a broken file is no placement");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
