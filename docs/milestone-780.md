# Milestone 780 — Slivers

## Objective

frus had lists, grids, paged views and single scroll areas, but no way to scroll several of them
as one: an app bar that collapses over a grid and then a list, a section header that sticks
while the list under it goes by, a footer that fills what is left of a short page. Apps written
elsewhere do this with a scroll view made of slivers. This milestone adds it:

- `CustomScrollView`, holding slivers;
- `SliverList` (fixed extent, any extent, or given children) and `SliverGrid` (a column count,
  or a maximum tile width), both lazy;
- `SliverToBoxAdapter`, `SliverPadding`, `SliverFillRemaining`;
- `SliverPersistentHeader` (scrolling, pinned, floating, floating and pinned, snap);
- `SliverAppBar` (pinned / floating / snap, `expanded_height`, `collapsed_height`,
  `toolbar_height`) with `FlexibleSpaceBar` (a title that shrinks into the toolbar, a background
  that moves up at half pace and fades out) or a flexible space of your own.

## Design

### One walk branch, two passes

A `CustomScrollView` is a leaf of the layout, like `ListView`: its children are not laid out
with the page, they are built and laid out in the walk (`ui/slivers.rs`), where the offset is
known. A new structural hook, `Widget::sliver_view`, says a widget is one. It is forwarded by
every transparent wrapper (`transparent.rs`, `Responsive`) and checked wherever `virtual_list`
is (paint cache, layout leaf, relayout fingerprint).

1. **Extent pass.** Each sliver says how far it scrolls without the offset: a list counts its
   items (measured ones as measured, the others at the mean), a grid its rows, a box adapter
   is measured, a header is its largest extent. The sum, less the viewport, is the maximum the
   offset is kept inside of, so restored offsets, a shrinking list and the scrollbar all work
   as for the other scroll areas.
2. **Layout pass.** The sliver protocol: each sliver is given its scroll offset (how far into
   it the view has scrolled), the paint extent left in the viewport, the overlap the slivers
   before it paint over, the preceding scroll extent and the cross extent, and answers its
   scroll, paint and layout extents and its paint origin. The geometry is pure functions in
   `sliver.rs` (`box_geometry`, `header_layout`, `padded_constraints`/`padded_geometry`,
   `fill_extent`, `grid_metrics`, `floating_effective`, `advance_float`), tested on their own.

The view registers a `Scrollable` exactly as the virtual list does, so the platform physics,
overscroll glow/stretch, the scrollbar, keyboard and wheel scrolling, `ScrollTo` through a key,
`on_scroll`, keyboard dismissal and `restoration_id` all come from the existing machinery.

### Lazy

Lists and grids build only items in `[offset − cache, offset + remaining viewport + cache]`,
the cache being 250 px by default (`cache_extent`) and shared along the view, so a sliver past
the viewport only gets what is left of it. Built items are rows: built in their own row scope
(state follows the row, `KeepAlive` honoured through the existing `keep_rows_alive`), in a
restoration scope per row, measured once for a list of any extent and remembered between
frames.

### Headers are painted over what scrolls under them

A pinned or floating header paints more than it takes, so later slivers are laid out under
it. The walk records each header's segment and moves it after everything that followed —
primitives and the registries a pointer is matched against last-first — the same move a
`StickyHeader` already did; that code is now one helper, `raise_segment`, used by both.
Focus and semantics keep reading order.

### Floating and snap

A floating header keeps, per header, the scroll offset it was last laid out at and how far it is
scrolled away (`Runtime::floating_headers`): scrolling back brings it back by as much, scrolling
on takes it away by as much. With snap, when the view comes to rest (`scroll_at_rest`: no
finger, no fling, no glide) with the header half shown, it finishes the way the scroll last
went over 200 ms, ease-out, and any new movement cancels it.

### SliverAppBar

The bar is a header whose child is built every frame from a `SliverHeaderState` (shrink offset,
current extent, whether content is under it). The toolbar is the existing `AppBar` (transparent,
`primary` keeps the status bar room, implied leading, actions), at the top of a box of the
current extent; the flexible space sits behind it. Widgets given to the bar are shared between
the frames' bars through a transparent `Rc` wrapper. Minimum extent is the toolbar plus the top
inset for a primary bar.

Styling is themed and overridable: the surface is `AppBarTheme::background`, else
`scheme.surface`; once content is under it (or the pinned bar has collapsed),
`AppBarTheme::scrolled_under_background` (new), else `scheme.surface_container`, at
`AppBarTheme::scrolled_under_elevation` (new), else 3 — a shadow only where a shadow colour is
named, as for `AppBar`. Each has a builder method of the same name. A bar that is not pinned
fades its toolbar over its last toolbar height of travel.

## API

```rust
CustomScrollView::new()
    .sliver(SliverAppBar::new().pinned(true).expanded_height(220.0)
        .flexible_space(FlexibleSpaceBar::new().title(Text::new("Lisbon")).background(image)))
    .sliver(SliverPadding::new(SliverGrid::extent(160.0, 24, tile).gap(8.0)).padding(12.0))
    .sliver(SliverPersistentHeader::new(40.0, 40.0, |state| header(state)).pinned(true))
    .sliver(SliverList::new(10_000, 56.0, |i| row(i)))
    .sliver(SliverFillRemaining::new(footer).has_scroll_body(false));
```

## How it was tested

- Geometry unit tests (`sliver::tests`): box paint extents; scrolling header shrinks then leaves
  with its bottom on its paint extent; pinned header stays at its smallest extent, paints but
  takes no room, a second pinned header is laid out under the first; floating header comes back
  by as much as scrolled back; floating + pinned keeps its smallest extent; snap finishes the way
  the scroll went, does nothing without snap, is cancelled by movement; fill remaining with and
  without scroll body; padding; grid metrics; window arithmetic.
- View tests through `build_ui` (`sliver::view_tests`): **laziness** — a 10 000-row list builds
  rows 0–16 at the top (400 px viewport + 250 px cache) and rows 4 993–5 016 at offset 200 000;
  a list of any extent builds only what it measures and reports the measured mean on the next
  frame; pinned header positions at three offsets, drawn over the rows and taking the tap where
  it covers them; scrolling header leaving; floating header coming back over the rows; padded
  grid in LTR and RTL; fill remaining; horizontal axis; app bar surface rule; a pinned
  `SliverAppBar` painted at 200 then at its toolbar height in the scrolled-under colour.
- Goldens (`frus-test/tests/slivers.rs`, rendered under WSL with the CI's mesa):
  `sliver_app_bar_expanded` (the title at one and a half times over its background, the first
  rows under the bar) and `sliver_app_bar_collapsed` (the toolbar's height, the title at
  toolbar size, rows going under it). Every other golden still matches.
- Real window on Windows: `cargo run -p frus-demo --example slivers`, driven with the mouse
  wheel and photographed. Pinned: the bar expanded over its gradient, half collapsed, collapsed
  to a toolbar with the two pinned section headers stacked under it over a grid, then thousands
  of rows down, then back to the top fully expanded again. With `FRUS_SLIVERS=floating`: deep in
  the list the bar is gone; a scroll up brings it back over the rows and the pinned sections
  lay themselves out under it, and it snaps to fully shown; a scroll down hides it again.
- Clippy (`--workspace --all-targets -D warnings`), the whole `frus-widgets` suite and its
  doctests.

## Not done

- `reverse` and a horizontal view in right-to-left (it scrolls from the left, as `ListView`
  does); a vertical view in right-to-left mirrors grid columns.
- `ScrollTo::index` (it needs a list shape; offsets work).
- Keeping the reader's place when a measured row above the viewport changes size (the
  `ListView` correction of milestone 717); here the view may move by that difference.
- `SliverAppBar::bottom` (a tab bar under the toolbar), `stretch`, and the large/medium
  variants; `SliverFillViewport`, `SliverAnimatedList`, nested scroll views.
- The layout's scroll extent is computed before the layout pass, so a row of any extent measured
  for the first time is counted at the estimate for one frame (the next frame corrects it).
