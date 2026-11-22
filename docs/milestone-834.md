# Milestone 834 — Animations keep real time on slow frames

## Objective

An animation written to last 300 ms should be over 300 ms after it starts, however many frames
the device manages to draw in that time. On a fast screen this was already true. On a slow one
it was not: the shell moved every animation by the time since the last frame, **capped at
50 ms** (or one and a half periods of the decoration's rate). A page drawn at four frames a
second therefore moved its animations 50 ms per frame while 250 ms really passed, so they ran
five times slower than written. Milestone 831 measured frames 300 ms apart on the test phone,
with frames still arriving 9 s after a swipe. At that rate the cap plays every animation,
the scroll's settle included, six times slower than written.

The cap had a purpose: a gap that is not a slow frame at all (a debugger stopped on a
breakpoint, a window held while it is dragged, a frozen process) must not end every animation
in one jump. This milestone keeps that protection and drops the slow motion.

## The change

### How far a frame moves the animations

`pacing::frame_step(gap, running, period)` in frus-shell decides, as a pure function:

- **The first frame of a motion** moves by one frame (`FRAME_STEP`, 1/60 s). Before it the
  screen was at rest, and the gap is time nobody was animating through. Counting it, as the old
  clamp did up to 50 ms, made an animation started by a tap skip its first three frames. Moving
  by one frame rather than nothing matters too: an animation started in this very frame has to
  see time move to report that it is moving and ask for the next frame. The very first frame of
  a window still moves nothing.
- **A frame of a running motion** moves by the whole gap, however slow, up to `STALL` (half a
  second), or one and a half decoration periods when the decoration runs slower than that.
- **A longer gap is a stall**: it moves by one frame, the decoration period or `FRAME_STEP`, and
  the motion goes on from where it was.

"Running" is a new field of the window, `clock_running`. It is set when the frame before asked
for this one (`NextFrame::Now` or `NextFrame::At`) and cleared when it went `Idle`. The parked
applications' clock (milestone 827) goes through the same function, so a parked application
and the one in front keep the same time.

### A spring stays stable on a long step

Taking the whole gap exposed a second problem. `spring_step` integrated the spring with one
semi-implicit Euler step per frame. At 1/60 s that is stable. At a quarter of a second, a stiff
spring like the scroll's settle overshoots its target and swings wider every frame: the test
below reached 12.5 times its target. `spring_step` now cuts a long `dt` into steps of at most
`SPRING_SUBSTEP` (20 ms, more than a 60 Hz frame, so an ordinary frame is still exactly one
step), with at most a thousand steps per call. The constant is public, beside `spring_step`.

## How it was tested

- `pacing::tests::a_slow_frame_moves_animations_by_the_time_that_passed`: frames 200 ms apart
  end a 300 ms animation in three frames (a first frame, then two of 200 ms); 450 ms and 1/120 s
  gaps are taken whole.
- `pacing::tests::a_stall_counts_as_one_frame`: a minute's gap and a 600 ms gap move one frame;
  decoration at one frame a second is not a stall at its own rate, and a stall in it moves one
  of its periods.
- `pacing::tests::a_motion_starts_with_one_frame`: the window's first frame moves nothing; the
  first after rest moves one frame however long the rest was; a clock that went backwards or a
  NaN moves nothing.
- `runtime::tests::a_spring_stays_stable_through_slow_frames`: the scroll's spring, stepped in
  quarter-second frames, stays bounded, comes to rest within three seconds, and ends where the
  same motion in 1/60 s frames ends; one 1/60 s frame is still a single Euler step.
- **The four tests fail without the change.** With the old clamp put back in `frame_step`,
  the three pacing tests fail (the slow animation is still at 0.25 s of 0.3 after five frames;
  a stall moves 0.05 s; the first frame after rest moves the whole 0.03 s gap). With the old
  single Euler step, the spring test fails with "the spring blew up: 12.5".
- `cargo test --workspace --exclude frus-gpu --exclude frus-test`, the `animation_speed`
  test, `cargo clippy --workspace --all-targets -D warnings` and the frus-widgets and
  frus-shell docs with `-D warnings` pass.
- The goldens (frus-test, all targets, under WSL, fresh build) pass with this change applied:
  106 of 106 images.

## Not done

- **Not measured on the phone.** The phone was not connected when the change was made. The
  expected effect is that the scroll's settle and page transitions end on time on a slow page,
  so the frames after a swipe stop sooner; this is to be measured with the home screen's frame
  time work that follows (milestone 835).
