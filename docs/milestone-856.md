# Milestone 856 — A change paints what changed, and the rest is given back

## Objective

L-042: every frame walks the whole widget tree. Milestone 822 made a component's own state
build that component alone, and milestone 853 made the layout that follows lay out the path
from the change to its root. What was left was the walk that paints and registers what can
be touched (hit targets, focus stops, accessibility nodes, selection stops), which reused
nothing unless a subtree was an explicit repaint boundary and no build had run. At a thousand
rows it was most of the frame, and it grew faster than the rows did.

This milestone measured that walk, found where its time went, and makes a component that was
not built again give back what its subtree painted and registered the frame before, moved if
its place moved: the way retained render objects keep their layers and only the dirty ones
paint again.

## What the frame cost, and why

A new measurement (`crates/frus-shell/src/repaint_tests.rs`, `one_row_cost_*`): a screen of
`ListTile` rows, each a component with a state of its own, in a `SingleChildScrollView`; one
row's state changes each frame, and the frame is timed in phases through the shell's own
frame (`Driver::costs`, and the new `Driver::layout_time`).

A profile of the thousand-row frame (`perf`, release build with line tables, under WSL)
showed no walk at all at the top. **58 % of the samples were text shaping** (`harfrust`,
`cosmic_text::shape`, `unicode_bidi`, called from `frus_text::measure_at`), 7 % the
allocator, and 8 % frus's own walk and layout together. The
rows' subtitles are cut with an ellipsis, and the cut was found by measuring the line with
one more grapheme cluster dropped each time, from the end: one measurement per cluster
dropped, per subtitle, every paint. The measurements are cached, two generations of 2 048
entries each. The candidates of forty rows fit in the cache and are shaped once; those of a
thousand rows do not, each generation is retired before the next frame asks again, and every
candidate is shaped again every frame. That is why the walk cost 0.02 ms a row at forty rows
and 0.26 ms a row at a thousand.

## Design

### The cut is found by halving

`truncate` and `ellipsise` (`text.rs`) find the cut by halving the grapheme boundaries rather
than by walking them back: a candidate with more of the line is never narrower than one with
less, so the ones that fit come first, and halving finds the same cut with five or six
measurements instead of thirty. A test checks it against the walk, at sixty widths, for
Latin, combining marks, emoji sequences, flags and trailing spaces.

### A kept component gives back what it painted

Every component is now a boundary of the paint cache of milestone 88, beside the explicit
`repaint_boundary()`.

- **Whether its configuration is the same.** A component keeps a *built mark*
  (`Widget::built_mark`, hidden from the docs, forwarded by every transparent wrapper): a
  number that moves whenever anything in its subtree is built. The walk that builds
  components counts the builds on the thread (`component::builds`) and, coming back up from a
  node under which something was built, sets that node's mark to the count. A count is never
  reused, so while a component's mark stands its subtree is the same objects with the same
  configuration, and a component built anew — by its own state, by a parent built again, by a
  new theme — has a mark no entry was taken under. An explicit repaint boundary that is not a
  component keeps the old rule: it stands until the next build of anything.
- **What it was painted from beside its configuration**, checked every frame before an entry
  is used, in one pass over the subtree (`Builder::survey`) that also says whether the
  subtree is plain and how many rectangles it takes:
  - each node's whole `Status` — the fields the fingerprint of milestone 88 left out are in
    now: the caret's blink, the caret's affinity, a drag over it, the colour and the corner
    radius in flight, the scroll offset, whether a value was placed;
  - what a node reads from outside the tree (`paint_source`), its live ink, and the animated
    values the walk reads itself: a scale or a turn in flight, an anchor or a slide moving, a
    text style in flight;
  - the subtree's rectangles, relative to its own box;
  - what the walk carries into it from above (`Builder::hash_context`): the clip, the
    direction, the touch clip, the focus scopes (excluded, skipped, order, group, form), the
    selection area and whether its bar was placed, the hover region it is in, the scroll and
    refresh hosts, the hero screen, the backdrop group, the page heading, whether the page's
    text is recorded, whether focus is shown, the scrolling behaviour in force;
  - the theme in force (compared once per pair of themes and frame, eleven kilobytes being
    worth not comparing a thousand times), the surface (`MediaQuery`) and the reader's text
    scale.
- **Where it is.** At the same place, what was kept is given back as it was. Moved — a row
  above grew, the list scrolled — it is moved with everything it registered, when the move is
  whole physical pixels and everything it drew is well inside the clip, both where it was and
  where it is: then moving its output is exactly painting it there. A primitive keeps the clip
  of the walk around the boundary, as a fresh paint would, and a clip narrowed inside it moves
  with it. A subtree holding a transformed layer, a path clip, a scroll area or a touch clip is
  never moved, only given back in place.
- **What is given back**: the primitives and the lifted selection handles, and every registry
  the walk fills: hit targets and long presses, back dismissals, edit actions, focus stops,
  scroll areas, drag sources and drop zones, inks, pans, pointer listeners, hover regions,
  selection stops, the page's text, accessibility nodes with their shape, system bar styles,
  the shortcut and action scopes and keystroke listeners (with their focus ranges moved to
  where the stops now are), the overflow bands, and whether the subtree asked for the next
  frame. The scopes, the listeners, the edit actions, the interactives, the overflows and the
  animation requests were not kept by milestone 88; a boundary around a shortcut lost it.
- **What is not kept**: a subtree that opened an overlay, walked a hover card, touched the
  modal focus scope, registered a hero, a scrollbar, a selection bar, a refresh area, a sheet,
  a dismissible item, a page list, a reorderable or a native view, or passed a filter (which
  may read the clock), is painted every frame, as before. So is one that is not plain: a scroll
  area, a list, a stack, an overlay, a layout builder, a custom layout (a `Scaffold`), a
  dismissible, a scroll overlay, a continuous animation.

### Memory

A component nested in another is kept by both, so what the cache holds is bounded: four times
what the frame itself painted and registered, and at least 4 096 items. Past it, the largest
entries — the outermost components, which hold the most — are dropped and not taken again
until their subtree changes, so what is left is the finer grain a change in one place needs.
An entry lives as long as its component is reached, as before.

### The paint contract

`Widget::paint` now says it: what a widget paints must follow from the widget and its
arguments, and one whose picture also follows something else says so with `paint_source` or
`continuous`. A component given back is not asked to paint.

## Alternatives rejected

- **Sharing nested entries by reference** instead of copying a child's output into its
  parent's. It saves the copies the budget pays for, but an ancestor rewrites what its
  children registered on the way out (a tap area takes their words, a barrier drops them, a
  transform maps them), so the parent's output is not a sequence of its children's.
- **Keeping only the innermost boundaries.** Then everything above them is walked every
  frame; the budget keeps as much of the outer ones as it can afford instead.
- **A generation per subtree bumped by the shell.** The shell does not know which subtree a
  build changed; the walk that builds components does, as it comes back up.
- **Caching measurements longer** (a bigger text cache). It would have hidden the cost at a
  thousand rows and brought it back at five thousand; the cut found by halving asks a sixth of
  the questions, and a kept row asks none.

## Numbers

`cargo test -p frus-shell --release _cost -- --ignored --nocapture --test-threads=1`, Windows
laptop, release profile without LTO (`CARGO_PROFILE_RELEASE_LTO=false`,
`CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, as milestone 853). "Before" is `c3f14b71` with only
the new layout timer (`LayoutCache::last_frame_time`) added, so that both binaries report the
same phases. Medians; the two binaries were run alternately, twice each.

**The machine was not quiet**: other builds ran on it throughout, and the same binary's
numbers moved by a factor of two to three between runs (the thousand-row "before" frame was
323 ms in milestone 853 and 1.4 to 3.1 s here). Read the ratios, not the absolute numbers.

One row's own state changes each frame (`one_row_cost_*`), a `ListTile` per row, titles and
ellipsised subtitles:

| rows | | build_ui | layout (fingerprints) | paint and register | kept / painted |
|---|---|---|---|---|---|
| 40 | before | 2.9 / 2.6 ms | 1.0 / 0.7 ms | 1.8 / 1.6 ms | — |
| 40 | after | 1.9 / 0.6 ms | 1.1 / 0.4 ms | 0.8 / 0.25 ms | 40 / 1 |
| 1000 | before | 1 364 / 3 081 ms | 161 / 353 ms | 1 206 / 2 726 ms | — |
| 1000 | after | 54 / 17 ms | 32 / 11 ms | 21 / 6.4 ms | 1000 / 1 |

The thousand-row frame is 25 to 180 times cheaper; the walk that paints and registers, 57 to
425 times. The layout's share fell too: its fingerprints measure the subtitles, and those
measurements now stay in the cache.

The counter ticking above the rows of milestones 822 and 853 (`rebuild_cost_*`, whose
`layout` is the whole of `build_ui`):

| rows | before | after |
|---|---|---|
| 40 | 6.8 / 9.5 ms | 5.3 / 7.3 ms |
| 1000 | 1 470 / 1 514 ms | 128 / 66 ms |

What is left at a thousand rows is the fingerprint walk of the layout root (the rows' content,
every widget of it, each frame) and the walk down to the rows and the survey of each kept row
(its statuses hashed, node by node): both proportional to the screen, both cheap per node.

The profile behind the diagnosis is of the "before" binary, under WSL (`perf record -g`,
237 519 samples over the thousand-row leaf-tick measurement).

## Tests

Each test changes one thing a kept subtree is painted from and compares the frame with the
same frame painted from scratch (the paint cache emptied): the primitives, the hit targets,
the focus stops and their traversal order, the accessibility nodes with their shape, the
selection stops, the hover regions, the inks, the shortcut scopes, the animation request. A
stale reuse shows as a difference.

- `frus-widgets` (`ui/reuse_tests.rs`): one row of fifty changes and grows, and the rows
  below it are moved, not painted (49 kept, 1 painted); a new theme with nothing built paints
  everything, and an equal theme as a new value does not; a new surface paints everything; a
  hovered row and a row with a value in flight are painted again, the others kept; a scroll by
  whole pixels moves the rows well inside the viewport and paints those across its edges, and
  a scroll by a quarter of a pixel paints them all; an ancestor's transform applies to what is
  given back; the selection stops of a selection area are given back, and a selection over
  kept rows paints them; eight levels of nested components keep a bounded amount.
- `frus-widgets` (`text.rs`): halving cuts where walking back did, at sixty widths, for
  Latin, combining marks, emoji sequences, flags and trailing spaces.
- `frus-widgets` (`paintcache.rs`): the lock is kept, vanished boundaries are evicted, the
  largest entries go first past the budget, the counters freeze.
- `frus-shell` (`repaint_tests.rs`, through the shell's frame): one row of forty changes, it
  alone is painted and the frame is the one painted from scratch; a tap on a row that was
  given back reaches it; a new theme and a new surface reach the kept rows. And the
  measurements above. `Driver` gained `layout_time`, `paint_reuse`, and, for tests,
  `forget_paint` and `frame_digest`.
- The milestone 88 tests of explicit repaint boundaries pass unchanged.
- `cargo test -p frus-widgets -p frus-layout -p frus-shell --lib`: 2 639, 17 and 677 passed (5 ignored, the measurements); the `thread_locals` guard passes with `CustomPaint`'s new counter.

## Not done

- **The layout's fingerprint walk** still visits every widget of a root that changed, and at
  a thousand rows it is now the larger half of the frame. A kept component's mark says its
  configuration did not change, but the fingerprint also reads what the runtime adds (an
  animated size or padding, a dismissal in flight, the scoped surface), and nothing yet says
  cheaply that none of that moved in a subtree.
- **The walk down to the kept rows**, and the survey of each one (every node's status
  hashed), are still proportional to the screen. A subtree whose nodes have no interaction
  state at all could be given back without asking each node, if the runtime said which
  widgets it holds state for.
- **What is not kept**: a component whose built child answers one of the walk's own branches
  (a semantics description, a barrier, an opacity, a clip, a transform, a filter) is not a
  boundary at its own level, only the components inside it are; a subtree holding a scroll
  area, a list, a stack, a custom layout (a `Scaffold`), an overlay or a continuous animation
  is painted every frame, as is one that walked a hover card or a selection bar.
- **A move is only followed by whole physical pixels, well inside the clip.** A list scrolled
  by a fraction of a pixel, or a row crossing the viewport's edge, is painted again.
- **The contract on `paint`** is documented, not enforced: a hand-written widget whose paint
  reads shared state without saying so through `paint_source` or `continuous` will be given
  back as it was painted. The built-in widgets were reviewed for it (by reading what their
  paints read, not exhaustively by test). `CustomPaint`'s painter is the application's
  closure and may read anything, so it answers a new `paint_source` every frame: a component
  holding one is painted every frame, as before.
- The numbers are from a loaded laptop; the phone was not used.
- The goldens were not run for this milestone; they are advisory and should be run before merging, since what reaches the scene is now partly given back rather than painted.
