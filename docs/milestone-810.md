# Milestone 810 — Input widgets: one-time codes, fields, swipe actions, wheels, empty states

## Objective

Real applications need input widgets that frus did not have yet: the row of boxes a code
from an SMS is typed into, a label and an error around any control, a field joined to its
prefix and button, a list row that slides aside to show what can be done with it, the
spinning wheels a time or a date is picked from, and the screen a list shows before it has
anything. Each one should look like the platform's own, be fully themable, and work with a
keyboard and a screen reader as well as with a finger.

## Change

### New widgets (frus-widgets)

- **`OtpInput`**: a row of boxes for a one-time code. Typing moves to the next box, backspace
  to the one before, and a paste fills them all. `.on_completed` hears the edit that fills the
  last box. Options: `.groups([3, 3])`, `.masked(true)`, `.kind(OtpKind::Alphanumeric)`,
  `.error(..)`, `.enabled(..)`. It reports the `OneTimeCode` autofill hint, so Android's
  autofill and the browser offer the code from the message, and a phone shows its number
  keypad.
- **`Field`**: a label, a description, an error and a required marker around any control. Its
  error is a **live region**: a screen reader reads it out when it appears.
- **`InputGroup`** with **`Addon`** (text, icon or any widget): a field and what is attached to
  it in one bordered box that lights up as a whole while the field has the focus.
- **`SwipeActions`** with **`SwipeAction`**: a row that rests open on its actions, springs
  closed short of half of them, and performs the first action of a side on a swipe all the
  way. Rows of the same `.group` are open one at a time; any tap closes an open row. Leading
  actions follow the reading direction. A focused row opens with the arrow keys and closes
  with Escape, and a screen reader offers every action by name.
- **`WheelPicker`** with **`WheelColumn`**: one or more wheels behind one band.
  `WheelColumn::numbers(range, ..)` builds a column of numbers.
- **`EmptyState`**: an icon or illustration, a title, a description and an action.

Each has a theme in `theme.widgets` (`otp_input`, `field`, `input_group`, `swipe_actions`,
`wheel_picker`, `empty_state`), and each widget's own builders win over the theme.

### Changes under them

- **`ListWheel`** can loop (`.looping(true)`) and tick (`.haptic_ticks(true)`). A looping wheel
  lays out many turns of its rows and rests on the copy of the chosen row nearest to what is
  on show; `selected` and `on_selected` still speak of rows `0..count`.
- **A fling carries on while the application echoes each value.** A wheel reports each row
  as it passes the middle; the application answering with that row made the next build ask to
  go there, and the fling stopped at the first value it passed. The runtime now ignores a
  requested page that is already the page on show.
- **Haptic tick on wheels**: `Widget::page_tick` asks the shell for the platform's selection
  tick as each value passes the middle (Android and iOS, nothing on a desktop), unless the
  theme's `enable_feedback` is off.
- **Swipe dismissal** (`dismiss.rs`) learned a reveal mode for `SwipeActions` (`RevealSpec`):
  the row rests open on its strip of actions instead of leaving. A cancelled swipe never
  dismisses or performs an action, however far it had travelled.

### Accessibility (frus-core, frus-shell)

These are additions only: new fields, a new action, a new module.

- `SemanticsProperties` gained **`live`** and **`custom_actions`**. Merging keeps `live` from
  either node and the first non-empty list of actions.
- **Live regions**: `live_region.rs` works out which live nodes appeared or changed their
  words each frame, and the shell announces them through the bridge it already uses. The
  window's first frame says nothing, so the opening interface is not read twice.
- **Named actions**: AccessKit gets `CustomAction` (Windows, macOS, Linux), Android gets custom
  `AccessibilityAction`s (TalkBack's actions menu) with ids from `CUSTOM_BASE`. Performing one
  sends `Widget::on_custom_action(index)`, dispatched like a press.

### Autofill on the web

`AutofillHint::web()` gives the `autocomplete` token, and the web editing element now carries
it, so a browser offers a code from an SMS or a saved password. A field without a hint gets
`off`. `AutofillHint::ios()` holds the `UITextContentType` names for the iOS bridge that
frus does not have yet.

### Demo and guides

- The demo has five new screens (`/otp`, `/fields`, `/empty`, `/swipe`, `/wheel`), one per
  widget, linked from the to-do screen.
- `docs/guide/input.md` covers `OtpInput`, `Field` and `InputGroup`; `docs/guide/lists.md`
  covers `SwipeActions`, `WheelPicker` and `EmptyState`.

## How it was tested

- Unit tests in each new module: `otpinput` (8), `swipeactions` (6), `field` (3),
  `wheelpicker` (3), `emptystate` (2) and the shell's `live_region` (3). They cover typing,
  backspace, paste and completion; opening, snapping, the full swipe, one open row per group,
  keyboard and named actions; the field's live error; a looping wheel's row arithmetic; and
  live regions saying nothing on the first frame and speaking on a change.
- A runtime test covers the fling carrying on while the application echoes each value, and
  a wheel test covers looping.
- Doc tests on `SemanticsProperties::live`, `custom_action` and `AutofillHint::web`/`ios`.
- `cargo fmt --all`, workspace clippy with `-D warnings`, Android (`aarch64-linux-android`)
  and `wasm32-unknown-unknown` clippy with `-D warnings`, and the workspace tests.

## Not done

- Not run on a phone: the haptic tick, TalkBack's actions menu and the SMS-code autofill
  offer were checked only through the code paths that set them up.
- iOS gets no `UITextContentType` yet, because frus has no iOS text editing bridge; the table
  is there for it.
- `WheelPicker` has no ready-made date column (days that follow the month); a date is three
  columns the application keeps consistent.
