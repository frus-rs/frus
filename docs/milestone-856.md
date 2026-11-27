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

NUMBERS

## Tests

TESTS

## Not done

NOTDONE
