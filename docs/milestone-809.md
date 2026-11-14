# Milestone 809 — Overlays and panels

## Objective

Four widgets that web component kits offer and frus did not:

- **`CommandPalette`**: a modal, filterable list of commands, opened by a key the application
  chooses, with fuzzy matching, the matched letters marked, groups, keys shown on the right,
  Up/Down/Enter/Escape, an empty state, and a way to show results the application fetches.
- **`HoverCard`**: a card that opens after the pointer rests on its anchor, stays while the
  pointer is on the card, closes after a delay, opens on a long press on touch, and is placed
  against the anchor with flip and shift.
- **`ContextMenu`**: a menu at the pointer on a right-click or a long press, with submenus,
  separators, unavailable rows, keys, checkbox and radio rows, and the keyboard.
- **`ResizablePanels`**: panels side by side or stacked, with handles moved by drag or by the
  keyboard, least and largest sizes, collapsing panels, and the sizes reported back.

## Design

### Where the state lives

As everywhere in frus, in the application: `CommandPaletteState`, `ContextMenuState` and
`ResizablePanelsState` are values the application keeps and is sent, changed, through one
message. Each has the moves it needs (`opened`, `with_query`, `open_at`, `sizes`,
`is_dragging`). A row of a menu or a command of the palette is one message, the row's own, so
the application closes the menu when it handles it — the rule the menu bar already has.

The **hover card is the exception**: whether the pointer has rested long enough is a matter of
time, and an application should not need a timer to show a preview. The walk that builds a
frame now knows where the pointer is (`InputState::pointer`, `pointer_touch`, set by the shell
on every pointer event) and keeps one record per card in the runtime: whether the pointer is
over the anchor or the card, since when, and whether it is open (`hovercard::step`). While a
delay runs the frame asks for the next one; when nothing runs nothing is drawn. The frame also
says whether a pointer that moved changes a card (`Ui::hover_cards_changed`), and the shell
draws a frame only then — a mouse moving over anything else costs a comparison of rectangles.
A subtree holding a hover card is never replayed from the paint cache, since the card is
decided as the walk passes it. `HoverCard::open(bool)` lets an application say it itself.

On touch, the finger on the anchor is the pointer: held for the touch delay (0.5 s) the card
opens; it stays when the finger lifts, and the next touch outside the anchor and the card
closes it at once.

### Placement

Two placements were added to the overlay system:

- `Placement::Card(side, align)`: against the anchor on a side (top, bottom, start, end) and
  lined up at the start, centre or end; flipped to the other side when it does not fit and the
  other side does; then moved along to stay in the window. Start and end follow the text
  direction. The card keeps its gap from the anchor as transparent room round itself, so the
  gap counts as "on the card" and crossing it does not close it.
- `Placement::Pointer`: the overlay's start corner on a point the widget names
  (`Widget::overlay_at`), opening towards the end and down, and towards the start or up where
  the window has no room.

### Where a press landed

A context menu opens where the press landed, which the widget could not learn: a secondary tap
and a long press were messages built before the press. Two methods, `on_secondary_tap_at` and
`on_long_press_at`, are asked with the point by the shell, and default to the plain ones.

### Reuse

The context menu is the menu bar's panel (`menubar::panel`), now told which theme it answers
to: a new `PanelKind::Context` resolves the caller's `MenuTheme`, then
`theme.widgets.context_menu`, then a desktop's menus on a desktop theme, then the
application's `MenuTheme`. Radio rows are a new leading mark (`MenuItem::radio`), read as radio
buttons.

The command palette and the hover card float on one shared surface (`hovercard::Surface`): a
rounded panel with a shadow and a hairline, each word the caller's, then the widget's theme,
then the framework's.

The panels lay themselves out in a layout builder (their sizes are shares of the room they are
given), and learn the text direction, the handle size and the keyboard step from the theme in
`Widget::expand`, which runs before the build.

### Naming

A palette's row is a `PaletteCommand`, not a `Command`. `frus` re-exports the widgets and the
shell side by side, and the shell's `Command` (the effect `update` returns) names the type in
`frus`, so a palette type called `Command` could not be reached there. The guide's doctest
caught it.

### Keyboard

- Palette: the field keeps the focus; Up and Down (bound on the palette) move the highlight
  round the ends and past unavailable commands; Enter runs the highlighted command (the field's
  submit, and the binding); Escape and a press outside close it. The highlighted row is kept in
  view through the scroll's `keep_visible`.
- Context menu: the menu bar's: the first row takes the focus, the arrows move between rows and
  into and out of submenus, Escape closes one level.
- Panels: a handle takes the focus; the arrows move it by the step (mirrored right to left),
  Home and End take the panel before it to its least and largest share, Enter collapses and
  opens a collapsible panel beside it.

### Accessibility

Two roles were added to `frus_core::Role` and mapped on every bridge: `Dialog` (AccessKit
`Dialog`, ARIA `dialog`) for the palette, and `Splitter` (AccessKit `Splitter`, ARIA
`separator` with its value, Android `SeekBar`) for a handle, whose range is the share of the
panel before it in per cent. A context menu's anchor and a hover card the application opens
report `expanded`. Group headings in the palette are headings.

### Right to left

The four mirror: the card's start side and alignment, the menu opening towards the left of the
pointer, the palette's rows, a handle's drag and arrows. Mirroring the context menu showed that
**a menu row drew its words on the left in right-to-left text** — on every menu in frus: the
label, the mark, the keys and the chevron are now mirrored in the row.

## How it was tested

Unit tests through the existing harness (`build_ui` frames, scene primitives, semantics, hits
and the keystroke resolution), in the four modules:

- placement: sides, alignment, flip and shift, right to left (`place_card`, `place_at_point`);
- hover timing: a mouse resting, passing over, leaving and coming back; a long press, a lifted
  finger, a touch elsewhere; then through the walk, the card appearing once the time has come,
  frames asked for only while a delay runs, and the pointer on the card keeping it open;
- the context menu: the messages for a right-click and a long press at a point, the rows hanging
  from the point, the submenu beside its row with its radio rows, right to left, the theme;
- the panels: limits, collapsing and reopening measured from the start of a drag, the layout
  of the shares, right to left, a drag, the keyboard, the theme;
- the palette: fuzzy matching and ranking, groups, the key that opens it, the arrows and Enter,
  the empty and loading states, the match colour.

The demo has a new page, "Overlays and panels", with all four.

## Not done

- The hover card's delay is measured on the frame clock: while a delay runs, frames are drawn
  at the display's rate (at most 0.7 s per card). A wake at a deadline would cost less.
- On touch, a long press on an anchor that is itself a button also releases as a tap on it.
- A hover card that the pointer opened does not say `expanded` to a screen reader (its state is
  the runtime's, and `semantics()` is asked of the widget); its content is read once shown.
- In a menu opened right to left, Left and Right still mean what they mean left to right for
  opening and leaving submenus (as in the menu bar).
- A context menu is not opened from the keyboard (the Menu key, Shift+F10).
- A row the caller draws in a menu keeps its padding on the physical sides right to left.
