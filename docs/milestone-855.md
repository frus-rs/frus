# Milestone 855 — A file tree of any size, driven from the keyboard

L-048: `FileTree` (milestone 811) built every visible row, scrolled into view or not, and its
keyboard stopped at Right opening a closed folder and Left closing an open one. This milestone
makes it a tree an explorer can be built on: rows built as they scroll into view, the keys of a
desktop tree view, multiple selection, and the semantics a screen reader expects of one.

## What was wrong

- **Every row was built.** The tree flattened the open part of the hierarchy into a `Flex`
  column, one row widget per visible node. Ten thousand open rows were ten thousand rows built,
  laid out and walked every frame.
- **A row could not move the focus.** Up and Down reached the next row only through the shell's
  geometric focus search, which needs both rows built and on screen. Left to the parent, Right
  to the first child, Home, End and type-ahead all mean sending the focus to a row the widget
  names, and a widget had no way to do that.
- **A row in a virtualised list could not take keys at all.** This is the root cause, and it is
  wider than the tree: a list builds its rows for the frame they show in and lets them go, and
  the shell answers a key by finding the focused widget in the retained tree
  (`find_widget`), which never contains a list's rows. A click works because the frame keeps
  each target's message; nothing kept a row's answer to a key. So the tree could not simply
  move into a `ListView`: its rows would have lost Left, Right and Enter.
- **No modifiers reached a click.** A click sent the message the frame kept for its target; the
  shell knew Shift and Ctrl were held and told nobody.

## What was done

### Two hooks the frame keeps (frus-widgets, frus-shell)

- **`Widget::on_row_key`** returns a closure (`RowKeyHandler`) the frame keeps beside the focus
  stop, as it keeps a click's message. While that stop has the focus the shell calls it with a
  `RowKey` — the `KeyStroke` as pressed, the keyboard's platform (so `RowKey::primary()` is Cmd
  on an Apple keyboard and Ctrl elsewhere, as milestone 852's shortcuts are) and the time on the
  click clock — after the application's shortcuts and before its own use of the key (Tab, the
  arrows' focus search, Enter's activation). The answer, `RowKeyAnswer`, holds messages to send
  and optionally a `RowTarget { index, key }`: a row of the same list.
- **`Ui::row_key`** resolves that target without the row being built. A row's identity is its
  list's identity keyed by the row's key (`child_id`), so the shell sets the focus to it at
  once; `Ui::reveal_list_item` works out where item `index` is from the list's shape (the
  measured extents for a list of any extent) and returns the scrolls that bring it into view —
  the least movement, through the enclosing scroll areas, as `reveal` does for a built stop;
  the shell moves them at once rather than gliding, as a tree view does.
  The next frame builds the row where the focus already is. A held Enter or Space activates once.
- **`Widget::on_modified_click`** returns a closure the frame keeps on the hit target; the shell
  calls it on a click with the modifiers held, `ClickModifiers { shift, primary, alt }`
  (`Ui::msg_for_click`). When it answers nothing, the target's plain message is sent as before.
- A repaint boundary holding a stop with `on_row_key` is not replayed from the paint cache: the
  closures are not part of what a boundary keeps.

### The tree

- **Flattened once, shown by a `ListView`.** The open part of the hierarchy becomes a vector of
  small lines (the folder's line, the index among its siblings, the depth, the id, open or not,
  the place "3 of 5"), in one walk, `O(rows)`; the place among siblings was computed by a second
  pass that rescanned each run, now it falls out of the walk. A row widget is built for a line
  only when the list asks for it, which is when it is in view. The tree takes the room on offer
  as a list does, or `.height(..)` / `.flex(..)`; `.shrink_wrap(true)` makes it as tall as its
  rows for a short tree in a page that scrolls.
- **The keys of a tree view**, from the row's `on_row_key`: Up and Down to the previous and
  next row (past a loading row, staying put at the ends); Right opens a closed folder and goes
  into an open one; Left closes an open folder and goes up to the parent of anything else (Left
  and Right swap in a right-to-left theme); Home and End; Enter activates (`on_activate`, or what
  a click does); Space selects; letters typed within a second go to the next row whose name
  starts with them, the same letter again goes on to the next one, and a pause starts a new
  name (`type_ahead_pause`). The tree keeps the key: an arrow at an end does not wander off to a
  widget beside the tree.
- **Multiple selection**, `.multi_select(true)` with `.selection(ids)` and
  `.on_selection_change(ids)`: the platform's own modifier with a click or Space adds a row or
  takes it out, Shift with a click, an arrow, Home, End or Space selects the range from the
  anchor (the row last chosen without Shift; with none, the first selected row shown), and the
  modifier with A selects every row shown. The primary modifier with an arrow moves the focus
  alone. `.selection_follows_focus(true)` selects the row an arrow lands on, as an explorer's
  list does; it is off by default, so a selection that opens a document does not open every one
  passed on the way. The anchor and the letters typed live in the runtime, under the tree's
  identity.
- **Semantics.** The tree is the `Tree` node itself (the list is told not to be a `List`), and
  says `multiselectable` when it is: `SemanticsProperties::multiselectable`, AccessKit's
  `set_multiselectable`, `aria-multiselectable` on the web. Each row keeps its role, name,
  level, place among its siblings, `expanded` for a folder and `selected`.
- The demo's "Display widgets" screen has a folder of five thousand files in a 320 px tree with
  multiple selection; the guide (`lists.md`) describes the keys and the selection.

## Measured

`build_ui` of a tree with one open folder of `n` files in a 400 × 600 window, release build,
median of the timed frames after two warm-up frames, on the development machine (a temporary
test, not committed):

FRAME_TABLE

## How it was tested

- `frus-widgets` (`filetree.rs`): the lines know their folder, depth and place; only the open
  part is shown; the loading row; roles, level, position, selected, expanded, no `List` between
  the tree and its items; chevron toggles and body selects through `Ui::hit` and
  `msg_for_click`; every arrow, Home, End, Enter, Space and Tab through `Ui::row_key`; Left and
  Right swapped right to left; `on_activate`; type-ahead (next match, a repeated letter, a
  pause, wrapping, the row in focus kept, Ctrl+letter not typing); clicks with modifiers
  building a selection and keeping the anchor; Shift+arrows, Shift+End, Space, Ctrl+A, Cmd+A on
  an Apple keyboard and Ctrl+A not on one; selection following the focus; the tree's height
  unsized, given and shrink-wrapped; the theme and the icon slot.
- **Fifty thousand rows**: a folder of 50,000 files open in a 400 px window builds 14 to 16
  rows (counted through the icon slot); End from the top sends the focus to the identity of the
  last row, unbuilt, with the scroll to the bottom; at that offset the next frame builds no more
  than 16 rows and the last row has the identity the focus was given.
- `frus-shell` (`file_tree_tests.rs`), through the shell's own key and pointer handling: a tree
  of 10,020 open rows; a click focuses and selects a row, Down moves the focus, End reaches the
  last row and scrolls it into view, Left goes up to its folder and then closes it, Home goes
  back, a letter jumps; Ctrl-click adds a row, Shift-click selects the range from the row
  Ctrl-clicked last, Shift+Down extends it.
- `a11y.rs` and `a11y_web.rs`: a multiselectable tree on AccessKit and in ARIA.

## Not done

- **Drag and drop** of rows: the drag-and-drop widgets take a dragged widget and drop zones
  laid out in the tree, and a row moved into a folder that is not built yet, or a drop between
  two rows that scroll, needs a list-level drop target; left for a milestone of its own.
- **Double-click to activate**, PageUp and PageDown by a window of rows, and `*` to open every
  sibling folder: not done. Enter activates.
- **Android's semantics** do not say the tree is multiselectable (its `ListView` mapping has no
  flag for it); each row's selected state is said.
- Other widgets' rows in a virtualised list are as they were: the shell's Enter, Left/Right
  (`on_key`) and Home/End paths look the focused widget up in the retained tree, which does not
  hold a list's rows (read from the code, not reproduced by a test here). The hooks are public;
  the existing list widgets were not moved onto them.
- A tree row scrolled out of view with the wheel while it has the focus is not built, so its
  keys are not answered until it is back in view; a desktop tree view would scroll back to it.
- Not run on a device or in a browser: the tests drive the walk, the runtime and the shell.
