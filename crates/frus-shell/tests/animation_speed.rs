// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The animation speed reaches every frame's animations** (milestone 778).
//!
//! Its own test binary, so its own process: the speed is process-wide, and the shell's
//! other tests time their animations at the normal speed.

use frus_shell::testing::Driver;
use frus_shell::FrusApp;
use frus_widgets::{
    set_animation_speed, AnimationController, AnimationHandle, Curve, Text, Widget,
};

const FRAME: f32 = 1.0 / 60.0;

/// Frames of `FRAME` until `fade`, started now over `seconds`, reaches its end.
fn frames_to_finish(driver: &mut Driver<FrusApp>, fade: &AnimationHandle, seconds: f32) -> u32 {
    fade.set_value(0.0);
    fade.forward(seconds, Curve::Linear);
    let mut frames = 0;
    while fade.is_animating() {
        driver.frame(FRAME);
        frames += 1;
        assert!(frames < 10_000, "it never finished");
    }
    frames
}

#[test]
fn a_tenth_of_the_speed_takes_ten_times_as_long() {
    let mut driver = Driver::new(
        FrusApp::from_fn(|_| Box::new(Text::new("Speed")) as Box<dyn Widget>),
        200.0,
        100.0,
    );
    driver.frame(0.0);
    let fade = AnimationHandle::new(AnimationController::unit());

    let normal = frames_to_finish(&mut driver, &fade, 0.5);
    assert!(
        (29..=31).contains(&normal),
        "half a second is 30 frames: {normal}"
    );

    set_animation_speed(0.1);
    let slow = frames_to_finish(&mut driver, &fade, 0.5);
    // Half way through the slowed run, the value is where a normal run is after 1/20th.
    fade.set_value(0.0);
    fade.forward(0.5, Curve::Linear);
    for _ in 0..30 {
        driver.frame(FRAME);
    }
    let after_half_a_second = fade.value();

    set_animation_speed(4.0);
    let fast = frames_to_finish(&mut driver, &fade, 0.5);
    set_animation_speed(1.0);

    assert!(
        (slow as f32 / normal as f32 - 10.0).abs() < 0.5,
        "ten times as long: {slow} frames against {normal}"
    );
    assert!(
        (after_half_a_second - 0.1).abs() < 0.01,
        "half a second at a tenth of the speed is a tenth of the way: {after_half_a_second}"
    );
    assert!(
        (8..=9).contains(&fast),
        "four times as fast is a quarter of the frames: {fast}"
    );
}
