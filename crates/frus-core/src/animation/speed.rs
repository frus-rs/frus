// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **How fast animated time runs**: one process-wide factor that every frame's time step
//! is multiplied by before any animation sees it (milestone 778).
//!
//! A debugging aid. At `0.1` every transition, spring, fling and ripple takes ten times
//! as long, which is how a motion that is over in 200 ms is looked at; at `10` a demo
//! that waits on animations runs through them. Input is not slowed: a drag still follows
//! the finger, and timers fire on the wall clock.

use std::sync::atomic::{AtomicU32, Ordering};

/// The slowest speed accepted: a hundredth of normal.
pub const MIN_ANIMATION_SPEED: f32 = 0.01;
/// The fastest speed accepted: a hundred times normal.
pub const MAX_ANIMATION_SPEED: f32 = 100.0;

/// The factor, as the bits of an `f32`. `1.0` until somebody says otherwise.
static SPEED: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// The speed `speed` is stored as: clamped to
/// [`MIN_ANIMATION_SPEED`]..=[`MAX_ANIMATION_SPEED`], and `1.0` for a value that is not a
/// number at all.
fn clamp_speed(speed: f32) -> f32 {
    if speed.is_nan() {
        1.0
    } else {
        speed.clamp(MIN_ANIMATION_SPEED, MAX_ANIMATION_SPEED)
    }
}

/// **Slows down or speeds up every animation**, for the whole application.
///
/// `1.0` is normal; `0.1` makes every animation ten times slower, `4.0` four times faster.
/// The value is clamped to `0.01..=100`. It applies to every animation the frames drive:
/// implicit transitions, animation controllers, springs, scroll flings, page transitions,
/// ripples and the theme's fade. It does not slow input or timers.
///
/// ```
/// // Look at the motion in slow motion while debugging it.
/// frus_core::set_animation_speed(0.1);
/// assert_eq!(frus_core::animation_speed(), 0.1);
/// frus_core::set_animation_speed(1.0);
/// ```
pub fn set_animation_speed(speed: f32) {
    SPEED.store(clamp_speed(speed).to_bits(), Ordering::Relaxed);
}

/// The factor set by [`set_animation_speed`]: `1.0` unless it was changed.
///
/// ```
/// assert!(frus_core::animation_speed() > 0.0);
/// ```
pub fn animation_speed() -> f32 {
    f32::from_bits(SPEED.load(Ordering::Relaxed))
}

/// A frame's time step, `dt` seconds of wall clock, as the animations should see it:
/// multiplied by [`animation_speed`]. The shell calls this once a frame.
///
/// ```
/// let dt = frus_core::dilate_frame_time(1.0 / 60.0);
/// assert!(dt > 0.0);
/// ```
pub fn dilate_frame_time(dt: f32) -> f32 {
    dt * animation_speed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speed_is_clamped_to_its_range() {
        assert_eq!(clamp_speed(0.1), 0.1);
        assert_eq!(clamp_speed(0.0), MIN_ANIMATION_SPEED);
        assert_eq!(clamp_speed(-3.0), MIN_ANIMATION_SPEED);
        assert_eq!(clamp_speed(1000.0), MAX_ANIMATION_SPEED);
        assert_eq!(clamp_speed(f32::NAN), 1.0);
        assert_eq!(clamp_speed(f32::INFINITY), MAX_ANIMATION_SPEED);
    }

    #[test]
    fn the_default_is_normal_speed() {
        assert_eq!(f32::from_bits(0x3F80_0000), 1.0);
    }
}
