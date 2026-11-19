# Milestone 824 — The last of #52: an app bar, a body and an intrinsic box that measure

Issue #52 ("A widget cannot measure its children") listed six widgets and one API wart, all
with one cause. Milestones 592 to 602 built the mechanism (children laid out apart, in the
walk, through the relayout cache) and used it for `AnimatedSize`, `AnimatedCrossFade`,
`SizeTransition`, the `Aligned` factors, `LimitedBox`, `CustomMultiChildLayout` and
`StickyHeader`. Milestone 766 used it for the scaffold: the shell became one layout that
measures its slots, the floating button is measured (`Scaffold::fab_size` does nothing since),
and every `*Top` placement exists, standing on the app bar's measured height.

Three items were still open:

1. **`AppBar` decided what fits from a width it was told, not one it was given.** Milestone
   716 had made the builder a widget (a `.build()` was no longer needed), but the bar still
   read the surface's width when it was built (`MediaQuery::of`) and folded its actions
   against that. A bar beside a rail, in a pane or in a dialog folded against the screen, not
   against its own box, unless the caller worked its width out and passed it with
   `AppBar::width`. And an action's width was read off its style, so an action whose width
   depends on its content counted as nothing.
2. **A scaffold told an extended body what it runs under from declared numbers.** Behind the
   app bar (`extend_body_behind_app_bar`) the body was told the status bar only; under the
   bottom widgets (`extend_body`) it was told the bottom bar's declared height and nothing of
   the footer. A body that pads itself by those heights, a `SafeArea` in it, had to know them
   itself.
3. **`Intrinsic` measured its content on one endless line.** An `IntrinsicHeight` around a
   paragraph was one line tall whatever its width, and the paragraph ran out of it; and the
   box was the content's natural size even where that was wider than the room.

## What changed

### The app bar decides when it is laid out

`appbar.rs`. The bar's toolbar row is now a `CustomMultiChildLayout` (milestone 601) whose
function is handed the toolbar's own box:

- Every piece that might be shown is built: the leading, the title, each action, and one
  overflow menu per number of foldable actions that can stay inline (each menu holds the
  actions from that one on). Building a menu is cheap, and only the one the layout picks is
  laid out, walked and painted.
- The function measures the leading, the title (under the type it will wear) and every
  action, then shares the width out with the arithmetic the bar already used: the title is
  reserved its natural width up to half the bar, at least 64 px; the actions take what is
  left; the last foldable ones fold into the menu when they do not fit; the title gets what
  the actions leave, cut with an ellipsis.
- A folded action is taken back (`ChildLayout::forget_child`, crate-internal), so it is not
  shown, not focusable and not announced. The menu's button is measured the same way and taken
  back when nothing folds.
- The pieces are placed as the old flex row placed them: the leading centred in its slot, the
  spacer, a spring either side of a centred title or one after a flush one, the actions at
  the end, padded by `actions_padding`, everything centred down the toolbar, on whole pixels,
  and mirrored in a right-to-left script.
- Pieces that run past the bar even with everything folded (free widgets never fold) are
  reported with the debug band, as the row reported them (`ChildLayout::report_spills`,
  crate-internal).

So the bar takes the width it is given (`width_fraction(1.0)`, as it already did when no
surface was described) and folds against it. `AppBar::width` stays, as a width of its own.
The bar's own size never depends on its children (its height is fixed and its width is the
room), so the layout entry that holds it is cached as before; each piece is a root of its own
in the relayout cache.

**`AppBar::build` is deprecated.** It returns the same bar in a box and there is nothing left
for it to finish. Every call in the workspace and the examples now passes the bar as it is
(about ninety calls), or `Box::new(bar)` where a box is needed. The navigation example's shell
no longer works out the width a rail leaves the bar, and the widget gallery's folding bar no
longer says it is 320 px wide: both fold against the box they are in.

### The body is told what the shell measured

`scaffold.rs`. The shell's layout (milestone 766) measures the app bar and the bottom widgets
before it lays the body out. It now leaves their heights in a cell shared with the body's
`MediaScope`, which reads them:

- behind the app bar, the top is `max(status bar, the bar's measured height)`: the bar's
  `bottom` and a status bar the bar consumed included;
- under the bottom widgets, the bottom is `max(gesture bar, bottom bar + footer)`, less what a
  keyboard the shell resizes for already took.

The body is fingerprinted, laid out and walked after the shell's layout has run, so the three
see the same numbers. The values decided before layout stay as the fallback.

### `Intrinsic` is a measured box

`constraints.rs`, `ui.rs`, `relayout.rs`. `Intrinsic` used to measure its content unconstrained
before layout and write the answer into its own style, both axes alike. It is now a box on the
`ConstraintsTransformBox` path, its content laid out apart and handed the room on the other
axis:

- **`IntrinsicHeight`** asks its content how tall it would like to be at the width the box is
  given, and the box is that height, rounded up to the step and held to the room. A paragraph
  under it wraps at the width it is shown at.
- **`IntrinsicWidth`** is the content's widest, which no room changes, so it is still measured
  ahead and written into the style (a box with a width of its own is not stretched across a
  column, which is the point of the widget here, where a column stretches by default). It is
  now held to the room by a ceiling of the whole of it (`max_width: 100%`).

In both, the walk hands the content the box's extent on the box's axis, so the pieces stretched
inside it match it. The step is part of the layout fingerprint.

## Not done

- **Lazy children**, the first shape #52 named, were not adopted. Every symptom the issue lists
  is answered without them, through children laid out apart.
- **The app bar's pieces are laid out twice when something folds** (once to measure, once at
  the width they are given). Each is its own cached root, so this happens on the frames the
  bar's width or content changes, not on every frame.
- **An action that does not fit even with everything folded still overflows**, as before: free
  widgets never fold.

## Verification

New tests:

- `appbar.rs`, `a_bar_folds_against_the_width_it_is_given`: the same three foldable actions in
  a 300 px box on a 1000 px surface fold (fewer than three drawn), the menu button is the only
  extra button announced (a folded action is neither drawn nor announced), and nothing runs
  past the box; given the whole 1000 px, all three show.
- `appbar.rs`, `an_action_with_no_width_of_its_own_is_measured`: a free action holding a row
  with a long label, beside two foldable ones, from 460 to 700 px: no overflow at any width.
  Read off its style, the free action counted as nothing wide and the foldable ones stayed.
- `scaffold.rs`, `a_body_behind_the_app_bar_is_told_how_tall_the_bar_is`: a 56 px bar with a
  30 px `bottom` under a 40 px status bar; a `SafeArea` body behind it starts at 126, the bar's
  measured height (it used to be told 40). A plain body still starts at 0.
- `scaffold.rs`, `a_body_under_the_bottom_widgets_is_told_how_far_they_reach`: with a footer on
  the bottom bar, a `SafeArea` body run under both stops exactly where a body that does not run
  under them stops (it used to be told the bottom bar's declared height only).
- `constraints.rs`, `an_intrinsic_height_wraps_its_paragraph_at_the_width_it_is_given`: a
  paragraph's box under `IntrinsicHeight` is several lines tall in a 160 px column and one line
  in a 2000 px one (it used to be one line in both).
- `constraints.rs`, `an_intrinsic_width_is_held_to_the_room` and
  `an_intrinsic_width_hands_its_content_the_rounded_extent`: a line wider than its 100 px
  column is given the column; a short word under a 50 px step is painted 50 px wide.

Tests changed: the two app bar tests that counted the springs of the old flex row now read
where the title is drawn; the cutout test lays the bar out under the surface it describes,
since the bar's safe area now reads the cutouts when it is laid out rather than when it is
built.

**The bar draws as it did.** A temporary test rendered 32 bars (leading, title, an icon
action, three foldable actions and an overflow; flush, centred, right to left, padded, with a
gap; at 1000, 520, 400, 300 and 200 px; medium, large, with a `bottom`, with a title spacing,
in a scaffold; an open menu; no overflow; no title; a long title flush and centred) and wrote
every text, rectangle and semantics node with its position. Before and after this change the
two dumps are identical, except one overflow amount: at 200 px with a 12 px actions padding,
where nothing fits, the band says 44 px instead of 56 (the padded group no longer counts its
own padding twice). The first version placed the pieces on half pixels where the row had
rounded them (a centred title at 306.5 against 307), and reported no overflow at all; both
were fixed before the comparison above.

**Failing before.** The new tests were not run against the previous code: a build of
`frus-widgets`' tests took about fifty minutes on this machine, shared with other builds. The
app bar's fold test reads the surface's 1000 px before the change (three inline in a 300 px
box), the two scaffold tests read the status bar and the declared bottom bar height, and the
intrinsic height test reads one line at both widths, which the first run of this milestone
showed (20 px at 160 and at 2000) while the measured box still offered its content a loose
width.

Checks, after merging master: `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -- -D warnings` clean; `frus-widgets` unit tests 2542
passed; doc tests 389 passed (115 ignored); `frus-demo` tests passed. The eight example
projects build their tests (widgets, authentication, counter, forms, hello_world, http,
navigation, websocket) and the gallery builds for `wasm32-unknown-unknown`.

**Goldens** (`frus-test`, every target, under WSL): all pass but one, which changed on
purpose. `m3_selection_mode` is the selection-mode bar of milestone 786 (a close button, "2
selected" and a delete action) at the top of a column. Its delete action was drawn right after
the title: that bar is laid out with no surface described, and the old row, given no width it
could trust, filled the room but left its spring nothing to spread. The bar now places its
actions at its end, as every other bar does; 408 pixels changed, all in the delete glyph's old
and new places (x 203 to 342, y 23 to 40). The golden was regenerated after looking at both.
