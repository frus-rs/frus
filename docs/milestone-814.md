# Milestone 814 — Wheels coast

## Objective

A wheel (`ListWheel`, and the `WheelPicker` time and date pickers built on it) was a snapping
paged region: a release went to `round(page ± 0.5)`, so a fling moved exactly one row however
fast it was. Real wheel pickers coast: a hard flick spins through many rows and settles on one,
and a slow release goes back to the nearest row. This milestone makes wheels do that, and
leaves `PageView`'s one-page-per-release rule exactly as it was.

## Change

- **`ScrollPhysics::item_ballistic(metrics, velocity, extent)`** (frus-widgets, `physics.rs`):
  the release of a region of rows of one height.
  1. Past an edge and heading further out: the ordinary ballistic, which brings it to the edge.
  2. The ordinary fling is worked out (`ScrollPhysics::ballistic`). If it would end on an edge
     anyway (a bouncing fling past the end), it runs unchanged.
  3. Otherwise the row nearest to where it would have stopped is chosen.
  4. Already there and still: nothing.
  5. Too slow to leave the current row: a spring back to it (the physics' own spring, mass
     0.5, stiffness 100, damping ratio 1.1).
  6. Anything else: a friction rebuilt with `FrictionSimulation::through`, its drag chosen so
     that it reaches the row exactly as its speed falls to the tolerance speed — no overshoot
     and no roll-back.

  Every motion it returns comes to rest **exactly** on its row or edge.
- **`Ballistic::Landing(Landing)`**: a new ballistic variant. It carries one of the existing
  motions (or the tuned friction) and an end; it never lets the friction past the end, and once
  the motion is done it reports the end itself rather than a point within the tolerance of it.
  `Ballistic::final_x()` says where any ballistic comes to rest. `Landing` is exported.
- **`BouncingScrollSimulation::final_x()`** (frus-core): the edge it springs back to, or where
  its friction runs out.
- **`PageSnap::coasting`** and **`PagedView::coasting`**: a snapping paged region whose release
  coasts. `ListWheel` sets it; `PageView` sets it to false, and the carousel goes through
  `PageView`. The runtime's `fling_scroll` sends a coasting region to `item_ballistic` and
  every other snapping region to `page_ballistic`, unchanged.
- A coasting release is filtered like any fling: below the physics' minimum fling speed
  (50 px/s clamping, 100 px/s bouncing) the velocity is nought, so the wheel snaps to the
  nearest row. A paged release still passes the raw velocity, since only its direction counts.
- Guide (`docs/guide/lists.md`) and the demo's wheel picker page say what a flick does.

## Design

A flag on `PageSnap` rather than a new snapping type: everything else about a wheel is the paged
machinery (the page a reader names, the requested page, the echo guard of milestone 810, page
reports and haptic ticks), and only the release differs. The paged rule stays the default.

The motion lands on the row, it is not corrected afterwards: the target is known when the
finger lifts, so the reports and ticks follow the motion, and a fling that crossed rows and was
then pulled back would be a different and worse gesture. The figures are the ones the physics
already uses: the platform's own fling (friction drag 0.135 for bouncing, the spline fling for
clamping), its spring, its tolerance (0.5 px, 2 px/s), and its fling-speed limits.

Looping wheels need nothing more: they lay out ten thousand rows and rest on the nearest copy of
the requested row, so a coast stays well inside the bounds; the bounds are respected as for any
wheel.

## How tested

On the driver clock (fixed 1/60 s frames through `Runtime::advance_scroll`), never the wall
clock:

- `physics.rs`: a flick passes several rows on both families, lands on the row nearest to the
  ordinary fling's end, exactly, and never passes it; the same flick turns a page view one page;
  a release with no fling springs exactly to the nearer row on either side, and on a row does
  nothing; a fling past either end rests exactly on the end, and an overscroll heading further
  out is brought home exactly.
- `wheel.rs`: through the runtime, a 2000 px/s flick either way passes at least three rows and
  rests exactly on a row's offset, on both families; a slow release (below the fling threshold)
  snaps to the nearer row either side; an 8000 px/s flick past either end rests on the first or
  last row; and milestone 810's echo test now asserts that after a drag with the application
  echoing each row, a flick **coasts** several rows further with the application still echoing
  each one, and rests exactly on the row the application holds.
- `pageview.rs`: an 8000 px/s flick on a page view still turns exactly one page, on both
  families.
- `simulation.rs` (frus-core): `BouncingScrollSimulation::final_x`.

## Not done

- Not checked on a phone: the feel of a coasting wheel under a finger (speed, how far a flick
  carries) has only been verified by the tests above, not by hand on the test device.
- Selection reports and haptic ticks fire once per frame in which the middle row changed, as
  before. A coast fast enough to pass several rows in one frame reports only the row it reaches
  in that frame, not each row it passed.
- `PageView` cannot opt into coasting: the flag is set by wheels only. A public builder can
  follow if a paged list of rows wants it.
- Pointer scrolling over a wheel (mouse wheel, trackpad) was not examined: this milestone
  changes only what follows the release of a drag.
