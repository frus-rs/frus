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

/// The longest time between two frames of one motion that animations still take whole, in
/// seconds: half a second (milestone 834).
///
/// Below it, a slow frame moves every animation by the time that really passed, so that a
/// page drawn at four frames a second still ends a 300 ms transition after 300 ms, in fewer
/// steps, rather than playing it in slow motion. Above it, the gap is not a slow frame but a
/// **stall**: a debugger stopped on a breakpoint, a window held by the system while it is
/// dragged, a process frozen. Taking it whole would end every animation in one jump, so a
/// stall counts as a single frame and the motion goes on from where it was.
pub(crate) const STALL: f32 = 0.5;

/// One frame, in seconds, when no rate for the frames is known: 60 Hz. What a stall counts
/// as, and the first frame of a motion.
pub(crate) const FRAME_STEP: f32 = 1.0 / 60.0;

/// How far a frame moves the animations, in seconds (milestone 834).
///
/// `gap` is the time since the frame before it (`None` for the first frame of a window, or
/// the first after it was hidden), `running` whether that frame asked for this one
/// ([`NextFrame::Now`] or [`NextFrame::At`]), and `period` the time between two frames of
/// decoration while it alone moves.
///
/// - The first frame of a motion moves by one frame, [`FRAME_STEP`]. A frame that follows a
///   screen at rest has a gap that is time spent at rest, which no animation was running
///   through; counting it would make an animation started by a tap skip its first frames.
///   One frame and not nothing: what a tap started in this very frame has to see time move
///   to say that it is moving, and ask for the next frame. The very first frame of a window
///   moves nothing.
/// - A frame of a running motion moves by the whole gap, however slow it was, up to
///   [`STALL`] (or one and a half decoration periods, for decoration slower than that).
/// - A longer gap is a stall, and moves by one frame: a decoration period, or
///   [`FRAME_STEP`].
pub(crate) fn frame_step(gap: Option<f32>, running: bool, period: Option<f32>) -> f32 {
    let Some(gap) = gap.filter(|g| g.is_finite() && *g > 0.0) else {
        return 0.0;
    };
    if !running {
        return FRAME_STEP;
    }
    let longest = period.map_or(STALL, |p| (1.5 * p).max(STALL));
    if gap <= longest {
        gap
    } else {
        period.unwrap_or(FRAME_STEP).min(gap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slow_frame_moves_animations_by_the_time_that_passed() {
        // Frames 200 ms apart, as a heavy page on a slow phone draws them: a 300 ms
        // animation has to be over after the second one, not six frames later.
        let mut t = 0.0;
        let mut frames = 0;
        let mut running = false;
        while t < 0.3 {
            t += frame_step(Some(0.2), running, None);
            running = true;
            frames += 1;
            assert!(frames < 5, "still at {t} s of 0.3 after {frames} frames");
        }
        assert_eq!(frames, 3, "a first frame, then two steps of 200 ms");
        assert_eq!(frame_step(Some(0.45), true, None), 0.45);
        assert_eq!(frame_step(Some(1.0 / 120.0), true, None), 1.0 / 120.0);
    }

    #[test]
    fn a_stall_counts_as_one_frame() {
        // A breakpoint held for a minute: the motion goes on from where it was.
        assert_eq!(frame_step(Some(60.0), true, None), FRAME_STEP);
        assert_eq!(frame_step(Some(0.6), true, None), FRAME_STEP);
        // Decoration at one frame a second is not stalled by its own rate...
        assert_eq!(frame_step(Some(1.0), true, Some(1.0)), 1.0);
        // ...and a stall in it moves one of its frames.
        assert_eq!(frame_step(Some(30.0), true, Some(1.0)), 1.0);
        assert_eq!(frame_step(Some(30.0), true, Some(0.1)), 0.1);
    }

    #[test]
    fn a_motion_starts_with_one_frame() {
        // The first frame of the window moves nothing; the first after a screen at rest
        // moves one frame, however long the rest: that time is nobody's.
        assert_eq!(frame_step(None, false, None), 0.0);
        assert_eq!(frame_step(None, true, None), 0.0);
        assert_eq!(frame_step(Some(0.03), false, None), FRAME_STEP);
        assert_eq!(frame_step(Some(5.0), false, None), FRAME_STEP);
        // A clock that went backwards, or nothing at all.
        assert_eq!(frame_step(Some(-1.0), true, None), 0.0);
        assert_eq!(frame_step(Some(f32::NAN), true, None), 0.0);
    }

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
