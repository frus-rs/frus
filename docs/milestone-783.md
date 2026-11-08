# Milestone 783 — The window remembers where it was; decoration at its own frame rate

## Objective

Two things a desktop application is expected to do without being asked, and that frus did not:

1. **The window comes back as it was left.** Close an application with its window moved to the
   second screen, made bigger, or maximized, and it opened again at its default size in the
   system's default place. Every desktop application of quality keeps that, and every
   application written on frus would have had to write it again, with the same traps: a
   monitor unplugged since, a lower resolution, a new scale factor, a window closed maximized
   or minimized.
2. **A spinner alone does not need 120 frames a second.** A continuous animation asks for every
   frame the display can show. For a scroll or a transition that is right; for an indeterminate
   progress indicator, a placeholder's pulse or a breathing halo — decoration that only says
   "something is going on" — it keeps the GPU and the processor busy, and the battery draining,
   for nothing the eye can tell from 30.

## What changed

### The window's placement, kept between runs

`Application::remember_window()` (default `true`) and `FrusApp::remember_window(bool)`. The
existing `window_size` is now documented as the size of the first run.

- **Where:** `window.json` in the application's data folder (`app_data_dir()`, milestone 779),
  which is already one per program. A small JSON object: `x`, `y` (the frame's top-left corner,
  physical pixels of the desktop), `width`, `height` (the content, logical pixels), `scale`,
  `frame_width`, `frame_height` (what the frame adds, physical) and `maximized`, plus a
  `version`. A file that is not one of ours, or of another version, is ignored. It is written
  whole or not at all, through the same temporary-file-and-rename the preferences use.
- **When it is read:** in `resumed`, before the window is created. The window is created hidden
  (the accessibility bridge must exist before it is shown), so it appears in its place, at its
  size, with no jump. A window closed maximized is created at its normal bounds and maximized
  just before it is shown, after the accessibility bridge: on Windows, maximizing a window shows
  it.
- **When it is written:** when the event loop ends (`exiting`). Between, a `Moved` or a
  `Resized` has the placement noted in memory once the loop has run out of events (see the
  evidence for why not at once). The tracker keeps the **normal** bounds: while the
  window is maximized only the flag changes, and while it is minimized nothing does. So a window
  closed maximized opens maximized and goes back to its own size when restored, and one closed
  minimized (from the taskbar) opens as it was before it was minimized. Full screen is not
  recorded either.
- **Fitting to today's screens** (`window_memory::fit`, pure and unit-tested). The screens are
  asked for when the window is about to be made, the main one first:
  - the window stays on the screen that shows the most of it, as long as that is at least
    100 by 50 physical pixels — enough to take hold of. It is then moved, and if need be
    shrunk, until the whole of it (frame included) is on that screen;
  - on no screen any more (a monitor unplugged, or a sliver left at the edge), it opens centred
    on the main screen, at a size that fits there;
  - the size is kept in logical pixels, so a new scale factor keeps the window the same size
    to the eye; where that no longer fits the screen, what fits is kept. The minimum window
    size (360 by 280) holds unless the screen itself is smaller;
  - with no position known (Wayland does not tell an application where its window is) only the
    size is fitted, and the system places the window.
- **Not restored:** for a simulated device (`FRUS_DEVICE`, whose window has the device's
  shape, and which is not followed either), and on a restart under `frus run`, which hands the
  window's placement over itself. On Android, iOS and the Web the code does not exist.

### Decoration at its own frame rate

- **`Widget::decorative()`** (default `false`): a widget's continuous animation is decoration.
  `CircularProgressIndicator` and `LinearProgressIndicator` (indeterminate), `Skeleton` and a
  chart's pulsing halo say `true`. Every wrapper that forwards `continuous` forwards it too
  (`Box`, the transparent wrappers' macro, the drag wrappers, `Hero`, `Responsive`); one that
  does not simply leaves the animation at full rate.
- **`AnimationHandle::decorative()`** and `AnimationController::set_decorative`: one call marks
  an application's own controller, `cx.use_animation_controller(AnimationController::unit)
  .decorative()`. `tick_animations` still moves it, but answers only for the others;
  `decorative_animations_moving()` answers for these.
- **`Ui::wants_full_rate_animation()`**: `wants_animation()` without decoration.
  `wants_animation()` keeps its meaning (anything moving), so nothing that read it changes.
- **The rate:** `Application::decorative_frame_rate()` / `FrusApp::decorative_frame_rate`,
  `Some(DECORATIVE_FRAME_RATE)` (30) by default, `None` for the display's rate. At the end of
  a frame, `pacing::next_frame` decides: anything other than decoration moving (an animation
  the runtime keeps, a widget that is not decoration, the application's tick, an image or a
  task in flight) asks for the next frame at once, as before; decoration alone sets a deadline
  one period after the frame began, which the loop's idle policy waits for (`WaitUntil`, like
  the caret's blink) and which asks for a frame when it is reached. A frame that waited for the rate takes
  its whole time step (the usual 50 ms cap on a step would slow a spinner below 20 frames a
  second), and the wait is never longer than a second. A frame drawn earlier for
  any other reason draws the decoration too, and moves the deadline. Since the indicators are
  driven by time, not by frame count, they turn at the same speed, with fewer frames.
- **Frame statistics** (`FRUS_FRAME_STATS=1`) name the new reason: `next frame asked by
  decoration`.

The rate sits on the application rather than the theme: it is about how the program uses the
machine, not how it looks, and a theme is copied into subtrees where a different value would
mean nothing.

## How it was tested

- Unit tests, `frus-shell`: `window_memory` (encoding read back, foreign files ignored, a
  window kept on its screen and on a second one, a screen unplugged, a window barely on a
  screen brought back whole and a sliver treated as lost, a lower resolution, a higher scale,
  the minimum size, no position, no screens, maximized over normal bounds, the tracker under
  maximize and minimize, a file saved and loaded); `pacing` (decoration waits for its rate,
  anything else asks at once, no rate or a meaningless one means every frame, nothing moving
  asks nothing).
- Unit tests, `frus-widgets`: a decorative controller is moved and answered for apart; doctests
  on `Ui::wants_full_rate_animation` (a spinner alone is decoration) and `Widget::decorative`.
- **Windows, for real** — see the evidence below.
- macOS and Linux: compile checks only (see Not done).
- Full suites: `frus-shell` and `frus-widgets` on Windows (524 and 2194 unit tests plus
  doctests), and every `frus-test` target (goldens included) under WSL with the CI's software
  renderer. No golden changed: the harness's `Stage::advance` counts a decorative loop as
  moving, so it renders the same frames as before. A first run of the goldens on Windows showed
  pixel differences in the decoration, widgets and goldens targets. The goldens are kept for
  the CI's software renderer, and on the final code every `frus-test` target passes under WSL
  with it (decoration 7, goldens 106, widgets 50, no failure). The Windows run was not
  repeated on clean master, so it is not proven there that the same differences appear
  without this change. In that first run, which
  shared the machine with a long build, `instance::tests::a_second_instance_with_no_link_only_asks_for_the_window`
  (a localhost hand-off with a 2-second wait, untouched here) failed once. It passed on the
  rerun of the full suite.
- Cross-target: clippy for `wasm32-unknown-unknown` and `aarch64-linux-android`, `cargo check`
  for `x86_64-apple-darwin` and `aarch64-apple-ios`.

## Evidence

Windows 11, a 1920 by 1080 screen at 150%, Intel Iris Xe, a 60 Hz display. A small check program
(a `FrusApp` with a `CircularProgressIndicator` alone, `window_size(640, 480)`) was started,
its window found by process id and title, moved, resized, maximized and minimized with
`SetWindowPos` and `ShowWindow`, and closed with `WM_CLOSE`. Window rectangles below are physical
pixels, from `GetWindowRect` and `GetWindowPlacement`.

| Run | Done before closing | Opened as | `window.json` after |
| --- | --- | --- | --- |
| 1 (no file) | moved to (300, 225), 1350 by 1050; maximized | 640 by 480 at the system's place | `maximized: true`, `x: 300, y: 225`, 885.3 by 662.7 logical |
| 2 | restored; moved to (450, 300), 1200 by 900; minimized | **maximized**, normal bounds (300, 30) 1350 by 1050 | `maximized: false`, `x: 450, y: 300` |
| 3 | — | (450, 180), 1200 by 900, not minimized | |
| 4 (`x` edited to -5000) | — | (360, 90): centred on the main screen | |
| 5 | maximized | (360, 90), 1200 by 900 | `maximized: true` |
| 6 | restored | **maximized**; restored to (360, 90) 1200 by 900 | `maximized: false` |

In run 2 the window came back 195 px higher than it was left, and in run 3 120 px higher: the
windows had been left hanging off the bottom of the screen (225 + 1050 > 1080), and were moved
until they were whole on it, which is what the fit is for.

The first version noted the placement on each `Moved` event, and run 1 kept the maximized
window's bounds as the normal ones: winit sends `Moved` (from `WM_WINDOWPOSCHANGED`) before its
`WM_SIZE` handler records the window as maximized, so the window was measured as a normal
window the size of the screen. The placement is now noted once the loop has run out of events,
when winit's state is whole; run 1 above is after that fix.

Frames per second, the spinner alone, `FRUS_FRAME_STATS=1`, three 2-second windows each:

| Rate | Frames in 2 s | Interval (median/95th) | Render (median) |
| --- | --- | --- | --- |
| `None` (as before this milestone) | 121, 122, 122 | 16.7 / 19.7 ms | 16.0 ms |
| default, 30 | 60, 60, 61 | 33.9 / 34.4 ms | 9.0 ms |
| 20 | 41, 41, 41 | 50.6 / 51.0 ms | 9.5 ms |

Half the frames at the default (a quarter on a 120 Hz display), and each frame's render shorter
too, since the GPU is no longer kept busy back to back. The first measurement of the default
showed no frames after the first one: the deadline is set by the frame, after the loop's turn
had already chosen how long to wait, so the loop slept until the next input. `about_to_wait`
now chooses again while a frame of decoration is owed. It also showed every paced frame
counted "over budget" in the statistics; a frame that waited for decoration's rate on purpose
is no longer counted late.

## Not done

- Run on macOS and Linux: the code is compile-checked for them, not run. Wayland gives no
  position, so there only the size and the maximized state come back.
- The fit uses each monitor's whole area: winit does not say where the taskbar or the dock
  is, so a window may be placed under it (Windows itself then keeps it reachable).
- The placement is written when the application ends normally. A process that is killed
  keeps the placement of its previous run.
- One window: frus has one window per application today. The file is per program, not per
  window.
- A theme-level rate for decoration: the rate is the application's (see above).
- The goldens' pixel differences on the Windows GPU renderer were not compared against clean
  master; they pass under WSL with the CI's renderer.
- No Android or Web run: the window memory does not exist there, and the decoration rate was
  not measured on a phone or in a browser (compile and clippy checks only).
