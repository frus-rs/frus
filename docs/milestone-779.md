# Milestone 779 — Four bugs the phone found

## Objective

Phone checks of milestones merged just before (Huawei STK-L21, Android 10; Chrome on the same
phone for the Web build) found four bugs that every test had let through. Milestone 768 recorded
them; this milestone finds each root cause and fixes it.

1. The tags `Autocomplete` in the demo's About tab, in a row beside the "Info" button, showed
   only its label "Tags", lower than the button's text: no outline, no placeholder, and no way
   to focus it.
2. The demo's Save wrote to `std::env::temp_dir()`, which on Android is `/data/local/tmp`, a
   folder the application cannot write. The error was dropped, and the snack bar said "Saved".
3. On the phone, the Web demo did not scroll under a finger at all, even on a freshly loaded
   page. Taps worked.
4. On the Web in landscape, with the keyboard up, Chrome left the page about 16 CSS pixels. The
   focused field, about 45 pixels tall, never came into view, and once the keyboard closed the
   page stayed scrolled with the field just out of sight.

## 1. A lone child's default size had nowhere to go

### Root cause

Reproduced through the demo's own `Application::view` at the phone's width (360) and at the
demo tests' 411: the field's box was **0 pixels wide** at both, its label painted anyway. It was
not the screen, the density, the text scale or the release build. The demo's test of the About
tab (milestone 759) only checked that the label was painted.

Milestone 767 made an unsized `TextField` take the room on offer, with 260 pixels as a default
where there is none (a soft size, milestone 590). A container that holds a soft-sized child
**alone** clears the child's default and has it fill the container, because a lone child is
handed its parent's constraints. Milestone 767 covered one container, the portal, which is what
an `Autocomplete` is **once it has suggestions to hang**: the portal takes the anchor's default
over. With nothing typed there are no suggestions, the autocomplete is a plain box around its
field, and the default was cleared inside a box that its own parent, a row of several children,
gives no room. The box was as wide as its content: a field with no width.

The same happened to any soft-sized widget alone inside a padding, a decorated box or any other
single-child wrapper placed in a row beside other children.

### Fix

`build_layout_scoped` (`ui.rs`): a container whose lone child gives up a default size on an axis
**takes that default over** (the child's size and margins, plus the container's padding), when
the container's own size on that axis is unset, and marks its own size there as soft. Its parent
then does what it does for any soft size: a parent that grants the room clears it and the
container fills; a parent that cannot (a row of several children) leaves the default, and the
child fills the container. This is what milestone 767 did for the portal, now for every
single-child container. `build_layout` clears such an inherited default at the root, which has
the whole room to give.

In the demo the tags field is now `Expanded` in its row: at 360 pixels a 260-pixel field beside
the button would run past the tab's edge.

## 2. The demo saves in the application's data folder, and says when it cannot

- New `frus::app_data_dir()` (`frus_shell::app_data_dir`, `data_dir.rs`): the folder where an
  application keeps what it must find again, created if missing. On Android it is the
  application's private `files` folder (`AndroidApp::internal_data_path`, recorded by
  `android_main`); on Windows `%APPDATA%\<program>`; on macOS and iOS
  `~/Library/Application Support/<program>`; elsewhere `$XDG_DATA_HOME/<program>` or
  `~/.local/share/<program>`. `None` on the Web, which has no file system. A preferences API was
  being written in parallel and was not available to build on; this is the plain folder.
- The demo's `todos_path()` is a file in that folder (the page's `localStorage` on the Web, as
  before). `Demo::save` toasts when the write is done: "Saved", or "Could not save: <the
  error>".

## 3. Drags on the Web demo: what the phone check saw was the injected input

### What the phone shows

Logged in Chrome on the phone through its DevTools: the drags made with `adb shell input swipe`
(and `input touchscreen swipe`) reach the page as pointer events whose `pointerType` is the
**empty string**. Android's `input` command injects events with an unknown tool type, and Chrome
reports "a pointer of unknown kind". winit takes every pointer that is not `"touch"` for a
mouse, so the shell heard a mouse press, moves and release: a mouse drag, which does not scroll
a list here (as on the desktop), while a mouse click is a tap. That is exactly what milestone 768
saw: taps worked, drags did nothing. The browser did not cancel anything (no `pointercancel`).

The same drag made as a **touch** (`Input.dispatchTouchEvent` from DevTools, `pointerType`
`"touch"`) scrolls the demo's Settings page on the phone, and so does a touch scroll gesture in
headless Edge (`Input.synthesizeScrollGesture`, touch source). A finger reports `"touch"`, so
this is not what a person sees; the phone check needs a touch source to test drags on the Web.

### Change

The shell still sets `touch-action: none` on the canvas when the window is made
(`web_page::keep_touches`). winit gives the canvas none and neither did the pages, so the
browser kept its own gestures over an application that draws its own: a downward drag at the top
of a page can pull to refresh (reloading the application), a double tap zooms, and a pan the
browser starts cancels the pointer for the application. It changes nothing for a mouse.

## 4. A landscape keyboard on the Web: the field's line in the strip, the scroll put back

### Root cause

Milestone 770 decides from one frame laid out above the keyboard whether the application can be
laid out there (a resize) or must keep its whole size and have the page panned to the field (a
pan). That frame is a keyboard frame like any other: the shell brings the focused field back
into view by scrolling its region (`reveal_after_keyboard`). In landscape that region was a few
pixels tall, so the field was scrolled to its top. When the page then panned, the application
got its whole size back but **kept that scroll**: the field was scrolled out of its own region,
under the app bar, and the browser panned to the part of the page where the field's element was,
over nothing of the field. After the keyboard closed, nothing scrolled it back: the "page stayed
scrolled" of the phone check was the application's own list (the page's header was in place).

### Fix

- The scrolls a keyboard sets off are recorded with their offsets before
  (`web_page::KeyboardReveal`), and put back at once when the choice is a pan
  (`App::fit_web_keyboard`). Under a resize they stay, as before.
- When the strip the keyboard leaves is shorter than the field, the editing element the browser
  pans to is a band as tall as the strip around **the line being typed in**
  (`web_editing::pan_band`): the middle of a single-line field, where its text is, or the
  caret's line in a multi-line one. The browser centres the element in the strip, so it is that
  line that shows, instead of whatever was in the middle of a field taller than the strip.

## How it was tested

- **Reproduced through the demo's own view** (`Application::view`, the demo's `Bench`, About
  tab): at widths 360, 380 and 411 the tags field's box was 0 wide, its label painted at
  (121, 224) as on the phone.
- **Tests that fail before the fix:**
  - `autocomplete::tests::in_a_row_an_empty_field_falls_back_to_its_default` (frus-widgets): run
    with `ui.rs` put back as on master, it fails: the field has no rectangle at all ("the field
    alone: []"). With the fix, 260 wide.
  - `tests::the_tags_field_beside_the_info_button_is_a_field` (frus-demo): the field under the
    "Tags" label is more than 150 wide, beside the button, inside the tab, outlined, at 360 and
    411. Before: 0 wide (the probe above).
  - `tests::a_save_that_fails_says_so` (frus-demo): a save to a folder that does not exist
    toasts "Could not save: …" and not "Saved". Before, the toast was "Saved" whatever happened.
  - `tests::the_tasks_are_kept_in_the_applications_data_folder`, `data_dir::tests::*`
    (each platform's folder from its environment, no folder without a full path, the folder is
    made and written to on the machine running the tests).
  - `web_page::tests::the_scrolls_a_keyboard_set_off_are_taken_back_for_a_pan`,
    `web_editing::tests::a_strip_shorter_than_the_field_shows_the_line_being_typed_in`,
    `web_page::tests::the_browser_keeps_no_touch_for_itself`. These are unit tests of the pieces;
    the shell's web frame cannot run outside a browser, so the behaviour was checked on the phone.
- `an_empty_field_takes_the_width_offered` guards the other side: offered room, the field takes
  it.
- `cargo test -p frus-widgets --lib` (2169 pass), `cargo test -p frus-demo --lib` (92),
  `cargo test -p frus-shell --lib` (487), `cargo clippy --workspace --all-targets -D warnings`,
  `cargo clippy -p frus-shell -p frus-demo --target wasm32-unknown-unknown -D warnings`,
  `cargo clippy -p frus-shell --target aarch64-linux-android --no-deps -D warnings`,
  `cargo fmt --all`.
- **Headless Edge** (CDP, touch emulation, the demo's web release build): a touch scroll gesture
  scrolls the Tasks page; no `pointercancel` either way there.

## Checked on the phone

Huawei STK-L21, Android 10 (density 408, 423 logical pixels wide), release APK of `frus-demo`
built from this branch; Chrome for the Web build, served from the PC through `adb reverse`.

- **The tags field**: Settings, About tab: the field is outlined beside "Info", takes the rest of
  the row, and a tap focuses it: the keyboard came up (`mInputShown=true`), "ap" typed showed
  "apple" and "apricot" under it at the field's width.
- **Save**: a task added and saved: the snack bar said "Saved". After `am force-stop` and a new
  start, the task came back from the file (the old file in `/data/local/tmp`, which holds two
  other tasks, was not read).
- **Web drags**: see section 3. With `adb input swipe` (pointerType "") the page does not move,
  with or without `touch-action`; with DevTools touches (pointerType "touch") the Settings page
  scrolls, `touch-action` `auto` or `none`.
- **Web landscape keyboard**: rotated to landscape, the task field tapped: the keyboard left a
  17.25-pixel strip, the editing element was a 17.25-pixel band over the field's text line, and
  the visual viewport was panned to it (`offsetTop` 166.7, the element's top 166.6): the typed
  "Hello" and the caret showed in the strip. With the keyboard closed, the page was back as
  before the tap (the field in place, nothing scrolled). Before this milestone the strip showed
  the empty top of the card, and the page stayed scrolled afterwards (milestone 768's f42, f45).
- Afterwards: the original 0.2.1 demo reinstalled, rotation and `/data/local/tmp/frus-todos.txt`
  as they were, the `adb` mappings removed.

## Not done

- **Goldens not run.** The layout change in `ui.rs` touches every single-child container that
  holds a soft-sized child; the unit tests (2169) all pass, but the golden images were not
  compared: drive C: of the build machine was full and WSL, where the goldens run, failed to
  start. They should be run before merging.
- **A pointer of unknown kind is a mouse on the Web**: winit decides it, and a device that
  reports no `pointerType` cannot drag-scroll. Not changed.
- **A column of several children in a row** (a label above a text field, beside a button)
  still gives up the field's default inside the column, which its row gives no room: the field
  is as wide as the label. Only single-child containers take a default over.
- **The caret line of a multi-line field** is placed from an estimate of the field's top padding
  (taken as even around the lines); only single-line fields were checked on the phone.
- The demo's task file moved: tasks saved before this change, in the temporary folder, are not
  read any more.
