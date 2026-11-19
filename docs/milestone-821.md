# Milestone 821 — The selection highlight in pictures, web restoration in a browser, the Apple look decided

## Objective

Three issues had been closed on less evidence than they asked for, or not closed at all:

- **#24** (selectable text) asked for "a pixel test that pins the highlight". The highlight was
  tested through the scene (a rect of the right colour, two handle paths) and checked by eye on
  a phone, but no image held it.
- **#57** (state restoration) was proven on Android (milestones 753 to 768). The web path —
  the tab's session storage, one entry per history entry — had unit tests and had never run in
  a browser.
- **#50** asked for a decision on an Apple look, written into `ROADMAP.md`. Apple-style
  widgets shipped in milestone 812, but the decision itself was never written down.

## What was done

### #24: two goldens, and the bug the second one showed

`crates/frus-test/tests/selection.rs` (new) puts a selection in the runtime the way the shell
does after a long press, through `frus_test::Stage`, and photographs it:

- **`selection_text_touch`** (300 × 110): a `Text::selectable()` line, "Select part of this
  line", with "part of" selected by a finger: the `Edit` (anchor 7, cursor 14), the focus and
  the handle mark. The highlight and both handles, with a second, unselected text under it.
- **`selection_area_rich`** (340 × 170): a `SelectionArea` over a plain text, a `RichText`
  ("then a **bold** and a big ending", the "and a big" span at 24 px in amber) and a last line.
  A touch selection (`RegionSelection` with its handles, no bar) runs from "words" in the
  first line to the end of "big" in the paragraph: one highlight per text, the second as tall
  as the big run, and a handle at each end. The last line stays unselected.

Beside each golden the test checks the colour **by number**, not only by picture:

- the highlight rect in the scene is `theme.widgets.text_selection.selection_color_in(theme)`,
  which is the theme's `selection` token when nothing overrides it; the handles are
  `handle_color_in`, the scheme's `primary`;
- at least a third of the pixels inside each highlight rect (the rest is glyph ink) are the
  token **blended over the background in linear light**, within 2 per channel, and **none** is
  the sRGB lerp. For the dark theme that is `rgba(0.35, 0.62, 0.95, 0.40)` over
  `rgb(18, 20, 24)`: `rgb(59, 105, 162)` in linear light, against `rgb(46, 75, 111)` for the
  sRGB lerp — far enough apart that the check cannot pass by accident.

Both images were read by eye before they were accepted.

**The bug.** The first rendering of `selection_area_rich` had the start handle half covered:
it hangs below "words", over the next line, and the next text's highlight was painted over its
lower half (sampled: `rgb(96, 200, 130)` — the primary — down to y = 48, then
`rgb(94, 185, 186)` below, the highlight's blue laid over it).

*Root cause.* A selection area's handles were painted by each text, in its own `paint`,
as the walk reached it. A handle hangs past its text, and everything the walk paints after
that text — the next text's highlight, any card or image below — goes over it. The handles of a
single field or selectable text have the same shape of problem in principle, but nothing in
their own widget is painted after them; an area's run through several widgets by design.

*Fix* (`crates/frus-widgets/src/ui.rs`):

- The walk paints an area's handles itself, from the text's `SelectableText::handle` — the
  same answer the shell hit-tests against — in the text's theme handle colour and opacity, and
  **lifts** them: they are set aside with the scene position they were painted at.
- They are put back **at the end of the area's subtree**, over everything it painted.
- Every place that takes a range of primitives out of the scene — a layer for an opacity, a
  press fade, a pixel effect, a clip shape, a transform, a hidden barrier, a hero — now goes
  through `Builder::split_scene`, which first puts back the lifted handles painted in that
  range. A handle stays inside the group it was painted in: faded with its opacity, clipped by
  its clip, turned by its transform, and dropped with a hidden subtree.
- The paint cache captures the lifted handles of a repaint boundary with its primitives
  (positions relative to the boundary) and puts them back when the boundary is replayed, so a
  replayed frame keeps its handles on top.
- `Text` and `RichText` no longer paint an area's handles themselves (`text::paint_grips` keeps
  the widget's own selection's handles). A widget of words of its own that implements
  `SelectableText` now gets an area's handles too, which it did not before.

*Test that fails without the fix*: `region::tests::frame::the_handles_are_painted_over_the_texts_after_them`
— three texts in an area, the first inside a repaint boundary, a touch selection from the first
to the third; both handle paths must come after the last highlight rect in the scene, on the
frame that paints the boundary and on the frame that replays it. With the walk pushing the
handles straight into the scene instead of lifting them (the old order), it fails at
`region.rs:931`; with the fix, the 2529 tests of `frus-widgets` pass.
`richtext::tests::the_handles_of_an_area_hang_from_a_paragraph` now says the paragraph's own
paint has none, since the walk paints them.

The golden was regenerated after the fix: the handle is the primary down to its tip, and
`selection_text_touch` came out byte-identical to its first rendering.

### #57: restoration on the web, driven in headless Edge

The demo opts into restoration (`restoration_scope_id("demo")`): the task draft
(`use_restorable_text_controller` + the field's `restoration_id`), the Tasks page's scroll, the
Settings page's scroll. It was built with `frus build web` (the worktree's own `frus`), served
by a static server on 127.0.0.1, and driven in headless Edge (`--headless=new`, WebGPU, a
504 × 678 viewport) over the DevTools protocol: mouse events, `Input.insertText`, wheel events,
`Page.reload`, `Page.navigate`, `Page.navigateToHistoryEntry`, and `Runtime.evaluate` to read
`sessionStorage`, `history.state` and `performance.timeOrigin` (a new value is a new document).

One change in the shell: a page restored from the tab now says so in the console, as Android
does — `restoring the state the tab kept (N bytes, history entry I)` — so the evidence below is
the application's own word and a developer can tell a restored page from a fresh one.

Evidence (in order, one tab):

1. **Opened** at `/#/`: `sessionStorage` holds `frus-restoration:0` with an empty draft.
2. **Typed** "Buy oat milk" in the New task field: the entry becomes
   `demo/route 0 \//draft	Buy oat milk` and `draft-field/selection	12`.
3. **Scrolled** the Tasks page with the wheel: `demo/route 0 \//tasks	0,76` (76 px is as far as
   the page scrolls with no tasks).
4. **Reloaded** (`timeOrigin` 1795053137383.3 → 1795053213024.5, a new document). Console:
   `[INFO frus_shell::app] restoring the state the tab kept (121 bytes, history entry 0)`.
   The screenshot after the reload is the one before it: "Buy oat milk" in the field, the page
   scrolled the same 76 px (the tip card cut at the top identically).
5. **Navigated** in the application to Settings (the drawer): the address becomes
   `/#/settings`, `history.state` 1, and a second entry `frus-restoration:1` appears. Scrolled:
   `demo/route 1 \/settings/scroll	0,600`.
6. **Reloaded on Settings** (`timeOrigin` → 1795053327110.5). Console: `restoring the state the
   tab kept (201 bytes, history entry 1)`. The page is Settings, scrolled to the same place
   (the screenshot matches the one before, the calendar at the same rows).
7. **The application's back arrow** from there: Tasks, with "Buy oat milk" in the field and
   the page scrolled the same 76 px — the values of the page beneath, restored with the reload of
   the page above it, claimed when it was built again.
8. **Left the page and came back**: an `unload` listener was added first so that the browser
   could not keep the page in its back-forward cache (the first attempt without it came back
   with the same `timeOrigin` — a page that never left, which proves nothing); then
   `about:blank`, then back through the tab's history. New document (`timeOrigin`
   1795053418683.4), console `restoring the state the tab kept (153 bytes, history entry 2)`,
   and the draft and the scroll are back. This is what a tab the browser discarded and opened
   again goes through; a tab **closed and reopened** by the user (Ctrl+Shift+T) keeps its
   session storage the same way, but there is no protocol command to reopen a closed tab, so
   that exact gesture was not driven.
9. **Control**: `sessionStorage.clear()` and a reload: no restoring line, the draft empty
   (`demo/route 0 \//draft	`), the page at the top. What came back before came from the tab's
   storage and from nothing else.

Screenshots and the console log are kept outside the repository (the agent's scratch folder).
Edge was stopped with `taskkill /T` by its PID, and no Edge process with this run's profile was
left afterwards.

Two things seen on the way, both by design and unchanged:

- After a reload on Settings, the application's back arrow made a **new** history entry for
  `/` (`history.state` 2) rather than going back to entry 0: the shell does not know what the
  entry before a reloaded one holds, and pushing is the answer milestone 561 chose for that
  case (`history::tests::a_page_reloaded_partway_finds_its_place_from_its_state`).
- The entry written while the Settings page was sliding away held a line under a
  `route leaving` scope (`demo/route leaving/scroll	0,600`): the outgoing page's scroll,
  registered while it was still being drawn, and still in the data written after the page
  had gone. Nothing claims it on a restore, so it costs a line of storage and nothing on
  screen; whether the restoration should forget the leaving scope was not looked into here.

### #50: the decision, in `ROADMAP.md`

Under *Framework depth*, a new entry: frus has one design language by default and an
Apple-style set beside it, adaptive where the difference is glaring.

- **What frus offers**: the behaviour that follows `Theme::platform` without asking (scroll
  physics, scroll behaviour and scrollbars, the selection bar's desktop form, page transitions
  and the iOS edge swipe, the centred app bar title, the back glyph, the caret's corners, Cmd as
  the modifier on an Apple keyboard); the Apple palette and theme; the seven Apple-style widgets
  of milestone 812; `Switch::adaptive` and `CircularProgressIndicator::adaptive`.
- **Welcome**: an adaptive constructor or an Apple-style counterpart for a control whose
  difference is visible and cheap — the checkbox, the radio, the slider, the alert dialog, the
  pickers, the context menu — themed, overridable, accessible and right-to-left like the rest.
- **Out of scope**: a complete second widget library kept in lockstep with the first; frus
  switching an application's look on its own (the application asks for the Apple look); pixel
  parity with the system's own controls (the system font, materials and vibrancy, haptics on
  every control).

## How it was tested

- `cargo test -p frus-widgets --lib`: 2529 passed, including the new paint-order test, which
  was seen to fail against the old order first.
- The goldens under WSL (Mesa, Vulkan, offline vendored dependencies, `CARGO_INCREMENTAL=0`):
  every `frus-test` target passes, the two new images included.
- The web evidence above, in headless Edge.
- `cargo fmt --all`; clippy with 1.100.0 and `-D warnings` for the host (all targets),
  `aarch64-linux-android` (NDK clang for `ring`) and `wasm32-unknown-unknown`: clean.
- `cargo test --workspace` on Windows: 5120 passed; the 19 failures are all `frus-test`
  goldens, whose images are made under WSL's Mesa and differ on this machine's GPU — the same
  targets pass under WSL.

## Not done

- **The default themes' selection colour is not the primary's**: `Theme::dark()` and
  `Theme::light()` keep their historical blue selection (`rgba(0.35, 0.62, 0.95, 0.40)` dark)
  while their primary — and so the handles — is green. A theme made with `Theme::from_seed`
  derives both from the primary, as the leading toolkit does. Changing the two default themes
  moves every golden with a text field in it and is a design decision of its own; the goldens
  here pin what the token is today.
- **A field's or a selectable text's own handles are not lifted**: only a selection area's.
  Their widget paints nothing after them, but a sibling painted after the widget could still
  cover a handle that hangs past it. Lifting them needs the widget's own paint to hand its
  handles to the walk, which is a change to `TextField`'s paint and was left for a milestone of
  its own.
- **A tab closed by the user and reopened** was not driven (see step 8); the discarded-and-
  reloaded path it shares was.
- Restoration on the web was not run in Firefox or Safari, nor on a phone's browser.
