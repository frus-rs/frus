# Milestone 832 — A line with an expanded child fills its axis

The demo's conversation screen was wrong on a phone. The list had a fixed, short height under
the app bar, with its newest messages cut off. The composer sat below it in the middle of the
screen, over a large empty area, and its field stopped two thirds of the way across.

The screen's body is the usual chat layout:

```rust
Flex::column()
    .child(Expanded::new(list))
    .child(Divider::new())
    .child(SafeArea::new(composer))
```

## What was wrong

A test that drives `Application::view` on a 411 × 869 surface, with a 27-px status bar and a
48-px navigation bar, found the same picture: the composer's field at y = 335. The layout
inspector showed three faults, each hiding the next.

1. **The body column shrink-wrapped.** `Flex` laid its line out as flexbox does: along its
   axis it was as big as its children, and an `Expanded` in it had no free room to take. The
   column was 332 px tall in a 778-px body. The list kept the default height a list has when
   it cannot fill (200 px, milestone 590), and the composer came right after it. This is the
   "fixed height" that was reported.
2. **The composer's row shrink-wrapped, for the same reason.** `ChatComposer` lays its buttons
   and its `Expanded` field out in a `Flex::row()`. The field kept its own default width
   (220 px), and the composer was 348 px wide on a 411-px screen, wherever it was placed.
3. **A safe area grew along a line shared with siblings.** `SafeArea` said "I fill what I am
   given" with `flex_grow: 1.0`. In a column that fills, it grew like a second `Expanded` and
   split the free height with the list. The first fault hid this one; with the column fixed,
   the composer would have sat in the middle of the lower half.

### Was the screen ever right?

No. `Flex` has shrink-wrapped since it was written, and its file has not changed in this
respect for months. The screen came in with this layout in milestone 815, whose note says it
was not run on a phone. No recent change caused it.

## The choice: fix the line, not the screen

The screen could have been fixed by itself: a `Column` (which fills its main axis by default)
for the body, a `Row` in the composer. That would leave the trap for everyone else. Writing a
`Flex` column with an `Expanded` child is the obvious way to say "this child takes the rest",
and it silently did nothing. It did nothing in a frus widget too (the composer), which shows
how easy it is to fall into.

The behaviour wanted is the one leading toolkits give a line with a flexible child: when the
line has room along its axis, a child that must fill its share takes all of the free room, so
the line ends up as long as the room. Without a flexible child the line keeps its children's
size. When the room along the axis is unbounded, there is nothing to fill.

So **`Flex` asks to fill its own axis when it holds a tight flexible child** (an `Expanded`, or
a `Flexible` with a tight fit), through `Widget::fill_axes`. The layout already answers such a
request the right way:

- alone in a box (a screen, a scaffold body, a padding), the line fills the box;
- across its parent's line, it stretches across;
- along its parent's line, there is no maximum to take, so the request is not granted and the
  line keeps its children's size, as before.

Left as they were:

- a `Flex` with no tight flexible child, including one holding only loose `Flexible`s, which
  need not fill their share;
- a wrapping `Flex`, whose room is divided into lines of its own;
- a `Flex` given a size on its axis, which already has its answer.

A debug warning for an `Expanded` in a shrink-wrapped line was the other option. With the line
filling wherever it has room, the case that remains is a line along its parent's line, where
nothing could be granted; that is listed under "Not done".

With this, the composer's row and the screen's column both fill. Neither needed to change.

## The safe area

`SafeArea` now **asks** to fill, through `fill_axes` (both axes), and declares no growth. It still
fills a box it is alone in and stretches across a row or a column. Along a line shared with
siblings it is its content plus its padding. To make one take the rest of a line, wrap it in
`Expanded`, as with any widget.

Other uses of `SafeArea::new` in the workspace and the examples were read:

- the demo's browser screen keeps its footer clear of the navigation bar with a safe area under
  a growing web view, in a column that fills; the footer took a share of the web view's room
  the same way, and the fix covers it (not checked on a device);
- the scaffold's persistent footer stacks a divider over a safe area in a column of unbounded
  height, where there was nothing to grow into, so it was not affected;
- the full-screen uses (a sized container holding a safe area, a scaffold body that is a safe
  area, the app bar's safe area around its toolbar) hold the safe area alone, where it still
  fills.

## How it was tested

- `tests::the_conversation_fills_the_screen_above_its_composer` (frus-demo) drives the
  application's view on a phone-sized surface with both bars. The composer's safe area reaches
  the bottom edge, the field sits just above the navigation bar and is as wide as the buttons
  leave it, and the list is taller than 500 px and reaches the rule. Before the fixes the field
  was at y = 335, and the inspector showed the 332-px column, the 200-px list and the 220-px
  field.
- `flex::tests::a_line_with_an_expanded_child_fills_its_box`: a column of an `Expanded` and a
  20-px row, alone in a 100-px window: the expanded child is 80 px tall. With a loose
  `Flexible`, or no flexible child, the column keeps its children's size.
- `flex::tests::a_line_with_an_expanded_child_beside_siblings_keeps_its_size`: the same column
  beside a sibling in a column is not stretched.
- `safearea::tests::beside_siblings_it_takes_only_its_content`: a 360 × 780 column of an
  `Expanded` and a safe area holding a 40-px block, with a 28-px status bar and a 16-px
  navigation bar. The block is at y = 724 (780 − 16 − 40). Before the fix it was at y = 376.
- `safearea::tests::alone_it_fills_the_box`: a safe area alone fills its box less the bars.
- `chat::tests::the_composer_sits_at_the_bottom_of_a_column`: the chat layout in a column of a
  definite size, with Android and Windows themes. Before the `SafeArea` fix the newest message
  was at y = 286 on an 835-px column; after it, at 661, just above the composer.
- The frus-widgets tests (2556) and the frus-demo tests (99), fmt and clippy.
- The frus-test goldens under WSL: two of them moved, `search_bars` and `search_view`, and were
  re-recorded. In both, the clear button of a search bar now sits at the bar's trailing end;
  before, it sat in the middle of the bar, right after the text. The bar lays its field out in
  a `Flex` row with an `Expanded` field, so the field kept its own width there too: the same
  fault, found by the same rule. Nothing else moved.
- The eight example applications build and pass their tests, and the widget gallery builds for
  the web.
- **On the phone** (a release APK of the demo, 1080 × 2340): the conversation's list runs from
  the app bar down to the rule, and the composer sits on the bottom edge, above the navigation
  bar, the whole width of the screen. After a burst of messages, with the keyboard up, the
  composer sits right above the keyboard and the list fills the room above it.

## Not done

- An `Expanded` in a line that is itself laid along its parent's line (a column in a column
  with siblings, with no `Expanded` around the inner one) still gets no room, silently. No
  room can be granted there; a debug warning for it is not written.
- `Navigator` and `Placeholder` also grow with a raw `flex_grow` along a shared line. They are
  left as they are. Changing them changes where existing applications place a navigator beside
  other children, and nothing reported a problem with them.
