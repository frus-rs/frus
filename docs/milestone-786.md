# Milestone 786 — Material 3 widgets, batch 1

## Objective

Eight Material 3 components that applications reach for and frus did not have:

1. a **split button** — a leading action and a trailing menu toggle, in the expressive sizes
   and shapes;
2. a **button group**, standard and connected, single or multiple choice, with the shape
   morph on press;
3. a **floating action button menu** — a button that opens a list of labelled actions over
   a scrim, closed by the keyboard and by Back;
4. the **docked and floating toolbars**, lying down or standing up, that step out of the way
   while the page scrolls;
5. a **side sheet**, standard and modal, from the end edge, with a drag to dismiss the modal
   one;
6. a **full-screen dialog** with a close button, a title and a confirming action;
7. a **rich tooltip** — title, lines and actions, hovered or persistent;
8. **selection mode** — a long press puts a list into multi-select, the app bar becomes a
   contextual bar ("3 selected"), checks appear and Back leaves the mode.

None of them existed in any form: the crate, the demo and the examples were searched for each
before starting. Each is built from the framework's own parts (`Button`, `PopupMenuButton`,
`FloatingActionButton`, `Drawer`, `Dialog`, `AppBar`, `AnimatedSlide`) rather than beside
them, and lives in a module of its own.

## What changed

### The widgets

| Widget | Module | Built on |
|---|---|---|
| `SplitButton`, `ButtonSize` | `splitbutton.rs` | two `Button`s, `PopupMenuButton` |
| `ButtonGroup`, `GroupSelection` | `buttongroup.rs` | `Button`s with state-dependent shapes |
| `FabMenu`, `FabMenuColor` | `fabmenu.rs` | `FloatingActionButton`, `Button`, a new placement |
| `Toolbar`, `ToolbarKind`, `HideOnScroll` | `toolbar.rs` | a surface, `AnimatedSlide` |
| `SideSheet` | `sidesheet.rs` | `Drawer` (end edge), now draggable |
| `FullScreenDialog` | `fullscreendialog.rs` | `Dialog` with no inset and no corner |
| `RichTooltip` | `richtooltip.rs` | the tooltip placement, now pinnable |
| `SelectionController`, `SelectableItem`, `SelectionAppBar` | `selectionmode.rs` | `AppBar`, a new Back hook |

Every one is a builder that is also the widget it builds (the `Lazy` arrangement `AppBar`
uses, written once as `composite_widget!` in `composite.rs`), assembled under the theme of the
place it lands in. Every colour, corner, size and type is a builder, then a field of a new
`*Theme` in `WidgetThemes` (`split_button`, `button_group`, `fab_menu`, `toolbar`,
`side_sheet`, `full_screen_dialog`, `rich_tooltip`, `selection`), then the scheme's role or
the specification's number.

**The numbers.** They are the Material 3 specification's. The full-screen dialog's tokens (`surface`, square, a 56 header, `title_large` on `on_surface`, the
action in `primary` `label_large`) are the published ones; the expressive components take the
specification's values:

- the five expressive button sizes (`ButtonSize`): 32 / 40 / 56 / 96 / 136 high, glyphs of
  20 / 20 / 24 / 32 / 40, 12 / 16 / 24 / 48 / 64 of room, `label_large` up to `title_medium`,
  `headline_small` and `headline_large`; the square corner 12 / 12 / 16 / 28 / 28 and the
  pressed corner 8 / 8 / 12 / 16 / 16;
- the split button: two apart, round outside, inner corners 4 / 4 / 4 / 8 / 12 tightening to
  8 / 8 / 12 / 20 / 20 when pressed, the trailing half becoming a pill with an upward chevron
  while its menu is open;
- the button group: standard gaps 18 / 12 / 8, connected gap 2, connected inner corners
  4 / 8 / 8 / 16 / 20; unselected `secondary_container`, selected `secondary`; a standard
  button squares its corners when selected, a connected one becomes a pill;
- the FAB menu: actions 56 high, pill-shaped, `primary_container` with `title_medium`, four
  apart and eight above a round 56 close button in `primary`; secondary and tertiary
  families; a scrim at 32 %;
- the toolbars: 64 thick; docked in `surface_container` with 16 at the ends, the actions
  spread; floating as a pill with eight of room, actions four apart, six off the page, in
  `surface_container` or `primary_container` when vibrant;
- the side sheet: 256 wide; standard in `surface` beside a hairline; modal in
  `surface_container_low`, one off the page, 16 on its inner corners; the heading in
  `title_large` on `on_surface_variant`, 24 of room;
- the rich tooltip: `surface_container`, a 12 corner, three off the page, `title_small` and
  `body_medium` on `on_surface_variant`, the actions as text buttons in `primary`, at most
  320 wide.

### Framework changes the widgets needed

- **`SemanticsProperties::expanded`** (`frus-core`): whether what a control opens is showing.
  The split button's toggle, the FAB and its close button say it. AccessKit gets
  `set_expanded` (UIA's ExpandCollapse pattern on Windows), the web DOM `aria-expanded`, and
  Android the `ACTION_EXPAND` / `ACTION_COLLAPSE` actions TalkBack reads as "collapsed" /
  "expanded" — both mapped to the press. `over` carries it through a merge.
- **`Widget::back_dismiss`**: what Back (and Escape when nothing focused answers it) sends
  while a widget is on screen, before the router is asked. It joins the overlays'
  dismissals in the same list, so an overlay opened over the page still closes first; a
  barrier that takes a subtree out of the input path (a page under another in a navigator)
  drops it too. `SelectionAppBar` uses it: Back leaves the mode.
- **`Widget::overlay_pinned`**: a tooltip-placed overlay shown while the widget says so
  rather than while the anchor is hovered — the persistent rich tooltip.
- **`Placement::Covering`**: an overlay whose bottom-end corner sits on its anchor's, behind
  a scrim, trapping the keyboard like a modal and kept inside the window. The FAB menu's
  close button lands exactly over the button it replaces.
- **`Drawer::drag_to_dismiss`**: a sideways drag moves the open modal panel's own animated
  progress with the finger (the hook the switch's thumb uses), towards the edge it lives on,
  mirrored in right-to-left; let go under half way it sends `on_dismiss`, otherwise it
  settles back. The modal side sheet turns it on.
- **`Localizations::more_options_label`** ("More options" / "Plus d'options").

## Behaviour worth knowing

- Everything is **controlled**, as every overlay in frus is: the application holds `open`,
  the selection, the chosen buttons; the widgets show them and send messages.
- **Keyboard and Back.** The split button's menu is a `PopupMenuButton`: it traps focus,
  arrows move, Escape and Back close it. The FAB menu, the modal side sheet and the
  full-screen dialog are modal overlays: focus is trapped, Escape, Back and a press on the
  scrim send their dismissal. The persistent rich tooltip closes on Escape, Back and a press
  outside it. Selection mode leaves on Back and Escape.
- **RTL.** The split button's and the connected group's corners, the side sheet's edge, its
  drag direction, the FAB menu's alignment, the vertical toolbar's hide direction and the
  selection check all follow the text direction.
- **Semantics.** A single-choice group reads as radio buttons, a multiple one as toggle
  buttons, each with its state; selectable items read as list items, ticked while the mode
  is on, and a press announces the count it leads to ("3 selected"); the contextual bar's
  title is that count; the rich tooltip's title and lines are said with its anchor; the
  sheet's and the dialog's titles are headings.

## How it was tested

- **Unit tests** in each module: the selection controller's rules; the contextual bar, the
  checks, the ticked items, the announced count and Back in a built frame; the button
  group's semantics and its corners by position, state, size and direction; the FAB menu's
  placement over its button, its semantics, its presses, the scrim and Back; the toolbars'
  boxes and `HideOnScroll`; the side sheet's edge in both directions, Back, the standard
  sheet, and the drawer drag's arithmetic in both directions; the full-screen dialog's
  header, heading, action, Back and its surface stopping presses; the persistent and the
  hovered rich tooltip; the Android bridge's expand and collapse actions.
- **Goldens** (`crates/frus-test/tests/m3_widgets_1.rs`), one per widget: split buttons,
  button groups, the open FAB menu, the toolbars, the modal and the standard side sheet, the
  full-screen dialog, the open rich tooltip, a list in selection mode.
- **A real Windows window**: `crates/frus-demo/examples/m3_widgets.rs` shows all eight on
  one page; `FRUS_M3=split|fab|sheet|dialog|tip|select` opens one at start. Each was
  captured from the running window together with its UI Automation tree.

## Evidence

- `cargo test -p frus-widgets --lib`: 2252 passed, the new modules' tests among them
  (selection controller, contextual bar and Back, button group semantics and corners, FAB
  menu placement / presses / scrim / Back, toolbars and `HideOnScroll`, side sheet edge in
  both directions and the drag arithmetic, full-screen dialog, persistent and hovered rich
  tooltip). The Android bridge test covers the expand and collapse actions.
- Goldens: the nine images of `m3_widgets_1.rs` generated and then re-verified under WSL
  (lavapipe), and each one read by eye.
- The real Windows window: `m3_widgets` was started once per `FRUS_M3` mode, captured, and
  its UI Automation tree dumped. The platform reports the split button's toggle as
  `ExpandCollapse = Collapsed`, then `Expanded` with its menu open; the FAB as `Expanded`
  with its close button `Expanded` while the menu is open; "2 selected" as the contextual
  bar's title; the sheet's and the dialog's headings, close buttons and actions by name.
- The window found three layout defects the unit tests did not: the rich tooltip did not
  wrap (the overlay offers unbounded width, so the card now states its width, up to 320);
  the standard side sheet's page did not give up its room; and a page-level `SideSheet` or
  `FullScreenDialog` inside a parent with no definite height collapsed. The last is fixed
  by letting the theme builder these widgets assemble under ask to fill the room offered
  (`ThemeBuilder::filling`, crate-private) instead of inheriting a percentage height.

## Not done

- **Motion.** The FAB menu and the split button's menu open and close at once; the
  specification's staggered entrance of the actions, the chevron's rotation and the
  button-to-close-button morph are not animated. The side sheet slides as the drawer does.
- **The width morph on press** of a standard button group (the pressed button widening and
  its neighbours giving way) is not done; the corner morph is, as a change of shape on the
  pressed state rather than an animated interpolation.
- **A hover-triggered rich tooltip with actions** cannot be reached by the pointer: like the
  plain tooltip it shows only while the anchor is hovered, so a tooltip with actions should
  be persistent (and the documentation says so).
- **Selection mode by keyboard alone**: a press (Enter, Space) toggles an item once the mode
  is on, but there is no keyboard way to start it — a long press has no key.
- The toolbars' docked variant with a floating action button beside it, and the
  `FloatingToolbar` with an attached FAB, are not built.
- The side sheet's drag is measured against its own width or 304 when the theme's drawer
  width is the only width given; a theme-only width drags slightly too fast or too slow.
- A page nested under **both** a `FullScreenDialog` and a `SideSheet` still needs a sized
  body (the demo sizes its page from the `MediaQuery`): each lays its body out as a share of
  the room it has, as `Drawer` and `Dialog` always have.
- Not run on the Android phone or in a browser: the new semantics flags are covered by unit
  tests on the bridges, not by TalkBack or a screen reader on the web.
