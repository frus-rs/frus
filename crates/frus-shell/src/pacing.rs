// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Decoration does not need every frame the display can show** (milestone 783).
//!
//! An indeterminate indicator turning, a placeholder shimmering, a pulse: they say that
//! something is going on, and nobody follows them frame by frame. Drawn at 120 frames a second,
//! a spinner alone keeps the GPU and the processor busy, and the battery draining, for nothing
//! the eye can tell from 30. So when the only thing that moves is decoration
//! ([`Widget::decorative`](frus_widgets::Widget::decorative),
//! [`AnimationHandle::decorative`](frus_widgets::AnimationHandle::decorative)), the next
//! frame waits for the application's rate for decoration
//! ([`Application::decorative_frame_rate`](crate::Application::decorative_frame_rate)).
//! Anything else that moves — a scroll, a gesture, a transition, an image arriving — asks for
//! the next frame at once, as it always did, and the decoration is drawn in those frames too.

use std::time::Duration;
// The browser has no `std` clock: the same one the shell keeps time with.
use web_time::Instant;

/// How many frames a second decoration is drawn at by default, when nothing else moves: 30.
///
/// An application chooses its own with
/// [`Application::decorative_frame_rate`](crate::Application::decorative_frame_rate) or
/// [`FrusApp::decorative_frame_rate`](crate::FrusApp::decorative_frame_rate).
///
/// ```
/// assert_eq!(frus_shell::DECORATIVE_FRAME_RATE, 30.0);
/// ```
pub const DECORATIVE_FRAME_RATE: f32 = 30.0;

/// What a frame that has just been drawn asks of the next one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum NextFrame {
    /// Nothing moves: no frame until something happens.
    Idle,
    /// As soon as the display can show it.
    Now,
    /// Not before this moment, this many seconds after the frame began: decoration alone
    /// moves.
    At(Instant, f32),
}

/// The next frame, after one that began at `began`: `full` when something that is not
/// decoration moves, `decoration` when decoration does, at `rate` frames a second for it
/// (`None`, or a rate that is not a positive number, for no limit).
pub(crate) fn next_frame(
    full: bool,
    decoration: bool,
    rate: Option<f32>,
    began: Instant,
) -> NextFrame {
    if full {
        return NextFrame::Now;
    }
    if !decoration {
        return NextFrame::Idle;
    }
    match rate.filter(|fps| fps.is_finite() && *fps > 0.0) {
        Some(fps) => {
            // A frame a second at least: a rate of almost nothing is not a frozen screen.
            let period = (1.0 / fps).min(1.0);
            NextFrame::At(began + Duration::from_secs_f32(period), period)
        }
        None => NextFrame::Now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoration_alone_waits_for_its_rate() {
        let began = Instant::now();
        assert_eq!(
            next_frame(false, true, Some(30.0), began),
            NextFrame::At(began + Duration::from_secs_f32(1.0 / 30.0), 1.0 / 30.0)
        );
        assert_eq!(
            next_frame(false, true, Some(10.0), began),
            NextFrame::At(began + Duration::from_secs_f32(0.1), 0.1)
        );
        assert_eq!(
            next_frame(false, true, Some(1.0e-30), began),
            NextFrame::At(began + Duration::from_secs(1), 1.0),
            "never less than a frame a second"
        );
    }

    #[test]
    fn anything_else_moving_asks_for_the_next_frame_at_once() {
        let began = Instant::now();
        assert_eq!(next_frame(true, true, Some(30.0), began), NextFrame::Now);
        assert_eq!(next_frame(true, false, Some(30.0), began), NextFrame::Now);
    }

    #[test]
    fn no_rate_means_every_frame() {
        let began = Instant::now();
        for rate in [
            None,
            Some(0.0),
            Some(-5.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ] {
            assert_eq!(
                next_frame(false, true, rate, began),
                NextFrame::Now,
                "{rate:?}"
            );
        }
    }

    #[test]
    fn nothing_moving_asks_for_nothing() {
        assert_eq!(
            next_frame(false, false, Some(30.0), Instant::now()),
            NextFrame::Idle
        );
    }
}
