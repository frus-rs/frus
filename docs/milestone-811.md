# Milestone 811 — Display widgets: Marquee, RollingDigits, KpiCard, FileTree, Sidebar

## Objective

Five widgets that application screens keep rebuilding by hand, each one widget with sensible
defaults and every colour, size, duration and slot overridable from the widget or the theme:

1. **`Marquee`** — text, or any child, that scrolls sideways in a loop when it does not fit.
2. **`RollingDigits`** — a number whose digits roll to each new value.
3. **`KpiCard`** — a dashboard figure: label, value, change coloured by the status colours,
   optional trend line and slots. It brings a small **`Sparkline`** with it.
4. **`FileTree`** — folders and files opened and closed in place, with lazy children, keyboard
   and screen-reader support.
5. **`Sidebar`** — side navigation that folds into a rail of icons, and becomes a drawer on a
   narrow window.

## API

```rust
Marquee::text("A long song title").velocity(30.0).gap(48.0).pause(1.2).reverse(false);
Marquee::builder(|| Row::new()...);              // any child, built twice (content + copy)

RollingDigits::new(1299.5).decimals(2).prefix("$").suffix(" USD")
    .group_separator(" ").decimal_separator(",").duration(0.6).curve(Curve::Linear);

KpiCard::new("Revenue").icon(Icons::BAR_CHART)
    .value("$48,210") | .number(48210.0) | .digits(RollingDigits::new(..)) | .value_widget(w)
    .change(12.5) | .delta("−320 today", KpiTrend::Down) | .delta_widget(w)
    .lower_is_better(true).sparkline([..]) | .footer(w)
    .label_widget(w).trailing(w).on_tap(msg).background(c);

Sparkline::new([3.0, 5.0, 4.0]).color(c).stroke_width(2.0).fill_opacity(0.12).height(32.0);

FileTree::new(vec![FileNode::folder(1, "src", [..]), FileNode::lazy_folder(5, "assets"),
                   FileNode::file(6, "Cargo.toml")])
    .expanded([1]).selected(Some(2))
    .on_toggle(|id, open| ..).on_select(|id| ..)
    .icon(|node, open| ..).label("Project files").loading_label("Loading…");

Sidebar::new(selected, on_select)
    .group("Workspace").item(Icons::DASHBOARD, "Overview").destination(SidebarItem::new(..))
    .expanded(open).on_toggle(msg).header(w).footer(w)
    .body(page).breakpoint(600.0);
```

Theme: `theme.widgets.{marquee, rolling_digits, kpi_card, sparkline, file_tree, sidebar}`, one
`Option` per setting, resolved widget first, theme second, default last.

## Design

### Marquee: a walk hook rather than a timer

A marquee has to move a whole subtree with the clock, and nothing in the walk did that: a
paint offset (`transform_translate`) is a fixed number, and only a widget's own `paint` is
told the time. Rebuilding the view every frame to move it would cost the whole tree.

So the marquee is a `ConstraintsTransformBox` (width unbounded, height loose, start-aligned
in the reading direction, cut to its box) — the box is the content's natural width held to
what is offered — plus one new hook, `Widget::marquee() -> Option<MarqueeMotion>`. In the
constraints-transform branch of the paint walk, when the child came out wider than the box
and motion is allowed (`Runtime::still` and `MediaQuery::reduce_motion()` both off), the walk:

- reads how long this marquee has been moving (`Runtime::marquee_elapsed`, a small table of
  start times keyed by widget identity, forgotten after a second unseen), so each marquee
  starts with its own pause instead of wherever the application clock is;
- slides the child by `MarqueeMotion::travel(elapsed, width + gap)` — zero during the pause,
  then the speed times the time, up to one lap — towards where reading starts (left in LTR,
  right in RTL, the other way with `reverse`);
- walks the second child, a copy wrapped in `ExcludeSemantics` and `IgnorePointer`, one lap
  behind, so the loop has no seam: at the end of a lap the copy stands where the content
  began, and the next lap starts there;
- sets `wants_animation`.

A marquee whose content fits never takes that branch: it is walked like any box and asks for
no frame — the "no cost when static" requirement. The copy is built from the same text (or
from a builder closure for a child marquee, called twice) and is only walked while moving, so
a screen reader hears the content once and a tap lands on the content.

Defaults follow the platform marquee: 30 px/s, a 1.2 s pause at the start of each lap, a
48 px gap.

### RollingDigits: one animated value, odometer wheels

The widget declares one implicit animated value, its count (the value in units of the last
decimal, as `anim_target`), so the runtime's existing machinery rolls it, rebases it when the
value changes mid-roll, and completes it at once when motion is off. The duration and the
curve are resolved from the theme's motion tokens in `build_themed` (`motion.long2`,
`emphasized_decelerate`) because `anim_duration` is asked without a theme; the widget's own
`.duration()`/`.curve()` win.

Each column is an odometer wheel computed from the current count `a`: the units wheel at `a`,
and every wheel above at `floor(a / 10^p)` plus the part of a turn the wheel below makes from
9 to 10. So 19 → 20 turns the units from 9 to 0 *and* the tens from 1 to 2 together, and a
number going up always rolls upwards. Every digit sits in a cell as wide as the font's widest
digit (with `tnum` asked for as well), so nothing shimmies; a leading wheel still under one is
hidden and fades in as it turns. At rest the exact value (from the `f64`) is drawn, since the
runtime carries the value as `f32`. A screen reader hears the formatted value
(`Role::Label`). The thousands separator defaults to the reader's language
(`Localizations::group_digits`).

A huge jump (0 → 1,000,000) spins the low wheels fast, as an odometer would; per-digit rolling
from the old digit to the new one would need the start of the animation, which the runtime
does not hand to the paint.

### KpiCard and Sparkline

A composed card (`Container` with the theme's `surface_container_low`, `outline_variant`
border, radius 12, padding 16): the label line (icon, label, trailing slot), the figures line
(value, then the change: an arrow and the text), and an optional footer or sparkline. The
change's colour comes from the scheme's status colours — `success` for good news, `error` for
bad, `on_surface_variant` for none — and `lower_is_better` swaps the first two. The sparkline
takes the change's colour. The card is a `Semantics::merge`, so it reads as one element:
"Revenue / $48,210 / +12.5 %".

`Sparkline` is a leaf: points scaled to the box (a flat series across the middle), a filled
area at 12 % and a 2 px line, decoration unless given a `label`.

### FileTree

Controlled like the existing `Tree`, but over a node hierarchy: the application passes
`FileNode`s and the set of open folders; the tree flattens the visible part into rows. A
`FileNode::lazy_folder` has no children yet: opened, a loading row stands under it until the
application gives it children (the demo loads them after 900 ms).

Each row is one widget (`FileTreeRow`): the hit target, the focus stop, and the semantic node
— `Role::TreeItem` with its name, `expanded` for a folder, and `toggled` for the selection.
The chevron is its own `GestureDetector`, so a click on it opens or closes and a click on the
rest of the row selects; it takes no focus, so Up and Down go from row to row through the
framework's directional focus. Right on a closed folder opens it and Left on an open one
closes it (`on_key`); Enter and Space select. The rows are keyed by node id, so focus and
hover follow a node when rows above it come and go. The tree itself is `Role::Tree`.

Two roles were added to `frus_core::Role`, mapped on every bridge: `Tree` / `TreeItem` to
AccessKit's `Tree` / `TreeItem`, ARIA `tree` / `treeitem`, and on Android a `ListView` and a
`View`. A selected tree item is *selected*, not ticked: AccessKit `set_selected`,
`aria-selected`, Android's `SELECTED` flag (as a tab).

### Sidebar

Open, a column of labelled items under group titles; folded, a 72 px rail of icons with the
names in tooltips and dividers for groups; an `AnimatedSize` moves the width between the two
over `motion.medium2`. The item in use wears a pill in `secondary_container`. Each item is one
widget (`SidebarItem` in the widget tree): hit target, focus stop and `Role::Tab` with
`toggled`, so an assistive technology's activation reaches the message.

With a `body`, the sidebar fills its parent and gives the page the rest of the row. Under the
breakpoint (600 px of window) it becomes a drawer: hidden while folded; opened, it slides in
over the page at full width (`AnimatedSlide`) above a scrim that closes it, and Back or Escape
close it too (`back_dismiss`). Closed, the panel takes no touches, no focus and is not read.

## How it was tested

Unit tests in each module (`cargo test -p frus-widgets`):

- **Marquee**: the lap arithmetic (wait, run, restart); a text that fits is drawn once and
  asks for no frame; an overflowing one waits, then runs left 50 px after one second of travel
  with its copy one lap behind, and asks for frames; reduced motion keeps it still and asks for
  nothing; reversed it runs right; the theme sets the motion; the copy is out of the
  semantics tree.
- **RollingDigits**: formatting (groups, decimals, prefix, suffix, sign, custom separators);
  odometer carries; every digit in a cell of one width; a change rolls through the runtime
  (half way two glyphs share the units column, the new one coming up from below) and ends on
  the exact value; a leading wheel fades in; reduced motion = zero duration; the theme times
  and sizes it; it reads as its formatted value.
- **KpiCard**: rise = success, fall = error, flat = on-surface-variant; lower-is-better swaps;
  the theme recolours; every slot replaces its default; the card reads as one element; the
  sparkline wears the trend colour.
- **Sparkline**: scaling, flat and empty series, semantics only when labelled, themed height
  and colour.
- **FileTree**: only the open part is shown, in order; a lazy folder shows a loading row;
  roles and expanded state, each name read once; a click on the chevron toggles and on the
  body selects (through `Ui::hit`); Right/Left open and close, Down moves the focus to the next
  row; the icon slot and the themed row height.
- **Sidebar**: open wide / folded rail and the page beside it; items select (groups not
  counted) and read as tabs, the one in use toggled; folded items draw no label; narrow it is
  a drawer (closed: nothing read; open: full width, Back closes); the theme sets the widths.
- **Shell**: AccessKit maps the new roles and a selected tree item is `selected`, not
  `toggled`; the ARIA roles.

The demo has a new screen, "Display widgets" (`/display`, linked from the drawer menu).

## Not done

- **Per-digit rolling** from the old digit to the new one (each wheel taking the short way):
  needs the animation's start value at paint; big jumps spin the low wheels as an odometer.
- **ARIA's Left-to-parent and Right-to-first-child** in the file tree: a widget cannot move the
  focus to another row yet, so Left on a closed node and Right on an open one do nothing. No
  type-ahead, no Home/End, no multi-selection, no drag and drop, and no virtualisation: every
  visible row is built (fine for hundreds, not tens of thousands).
- **Tree level and position** (`aria-level`, "3 of 5") are not exposed: the semantics tree is
  flat.
- **A marquee during its pause** still asks for frames (it is moving, in the walk's eyes); a
  timer-driven wake for the pause would make it free.
- **The sidebar's own scroll**: a list of items taller than the window is cut, not scrolled.
- Not run on a device or in a browser: the tests drive the walk and the runtime; the demo was
  built, not looked at on a phone.
