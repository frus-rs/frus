# Milestone 768 — A restored scroll offset waits for its content

## Objective

Milestone 762 left one thing open on the Android 10 phone: after the system ended the demo in
the background, the Tasks page came back **at the top** although it had been scrolled, while
the draft typed on the same page came back. The demo's test of that path
(`the_tasks_page_comes_back_scrolled_where_it_was_left`) passed, so the cause was something
only the device did. This milestone finds it, fixes it, and checks three milestones just merged
(766: the floating action button's placements; 767: autocomplete and table widths; 770: a
field above a landscape keyboard on the Web) on the phone.

## Root cause

Reproduced on the phone (Huawei STK-L21, Android 10, release APK with temporary traces):

```
android-activity: Creating: Activity = 0x2c46, saved state size = 84
frus_shell::app: restoring the state the system kept (84 bytes)
J768TRACE apply_restored scroll WidgetId(5105654141966622073) (0.0, 47.0)
J768TRACE clamp WidgetId(5105654141966622073) (0.0, 47.0) -> (0.0, 0.0) max (0, 0) viewport …
frus_demo::demo: J768TRACE replace_all 2 items
```

The restoration itself worked: the offset was read back under the right id and handed to the
scroll view before the first layout. But the demo **loads its tasks on another thread** at
start-up (`on_start` → `Demo::load` → `host::spawn`), and they arrive a frame or two after the
first one. The first frames lay out the Tasks page without its tasks; the page is then too
short to scroll as far as it was left (here, not at all: `max (0, 0)`), and the runtime keeps
every offset nobody owns inside its content (`keep_scroll_in_range`, milestone 533). The
restored offset was clamped to 0 for good; the tasks arrived one frame later, to a page
already at the top.

Why the test stayed green: the test driver never runs the application's `init` (so never its
`on_start`), and the demo's tasks are never loaded there. Both processes of the test have the
same, empty list, so the restored offset always fits. On the phone the second process starts
with *fewer* tasks than the first had on screen, for the frames until the load finishes.

This is not peculiar to the demo. Any application whose content comes after the first frame
— read from a file or a database, fetched, decoded — loses a restored offset the same way.

## Design

A restored offset the content cannot take yet is **kept as the goal** instead of being clamped
away:

- `Runtime::apply_restored` records each restored scroll offset as a `RestoringScroll` — the
  goal, and where the region was last put on its way there.
- While a region has a goal, it is drawn as far along it as the content allows
  (`scroll_offset_within` clamps the goal rather than the current offset), and
  `keep_scroll_in_range` moves it there. The frame in which the content grows enough is
  already at the restored offset; the goal is then reached and forgotten.
- The goal is **let go**, the region staying where it is drawn, as soon as:
  - something else owns the region (a finger, a fling, a glide, a programmatic scroll);
  - something else moved the offset (it is no longer where the goal last put it — a
    correction that keeps the reader's place, a jump to an item);
  - the user acts at all: a press, a key or a wheel anywhere
    (`Runtime::forget_restored_scroll`, called by the shell). Content that arrives after the
    reader started using the page must not move the page under them.
- What restoration keeps for a region still waiting is the **goal**, not the clamped offset, so
  a process ended again before the content came back keeps the reader's place.

Regions that never had a restored offset behave exactly as before: content that shrinks still
pulls an offset at rest back inside it at once.

*Considered*:

- **Applying the offset later**, once the page "has its content" — there is no such moment the
  framework can see; an application may load in several steps, and the first frame is the
  only one known to come.
- **Not clamping at all** while restoring — the first frames would then draw the page past
  its end, an empty area where the content stops.
- **A time limit** on the goal — loading times vary by orders of magnitude between a file and
  the network; ending the wait on the user's first action is what decides whether a later jump
  would surprise anyone.

## How it was tested

- **Demo tests** (`frus-demo`), through the shell's frame and the demo's own `view`:
  - `the_tasks_page_comes_back_scrolled_when_its_tasks_arrive_late`: twelve tasks, the page
    dragged to its end, the data taken; a fresh demo restored from it runs two frames
    **without** its tasks (as the phone does), then the tasks arrive (`Demo::replace_all`,
    what the load does). The frame they arrive in already draws the page where the first
    process left it, and the strip of showcases near the top is drawn where it was.
  - **Mutation**: with `apply_restored` no longer recording the goal, this test fails ("the
    page is where it was left": the offset is the short page's end, 58 px, instead of 916);
    the other two pass, as expected — the press test guards the letting go, which a missing
    goal satisfies trivially.
  - `a_restored_offset_waiting_for_its_content_is_let_go_on_a_press`: the same, with a tap on
    the page before the tasks arrive: the page stays where it was when the reader tapped.
  - milestone 762's `the_tasks_page_comes_back_scrolled_where_it_was_left` still passes.
- **Runtime tests** (`frus-widgets`): `a_restored_offset_waits_for_its_content` (drawn at the
  content's end, kept as the goal in the restoration data, reached once the content is long
  enough, clamped as ever afterwards) and `a_restored_offset_is_let_go_when_the_region_is_used`
  (a user action, a finger, another move each let it go); milestone 753's
  `a_scroll_offset_is_restored_and_kept` still passes.

## Checked on the phone

Huawei STK-L21, Android 10, release APK of `frus-demo` built from this branch after merging
master. The process was ended the way the system ends it: Home, then `adb shell am kill`
(the process id was gone before reopening), and the app was reopened from the recents screen.
The log showed the saved state handed back ("restoring the state the system kept").

- **Tasks arriving late, twice over.** Eight tasks on the Tasks page, scrolled to its end, then
  ended. The demo's Save writes nowhere on Android (see *Not done*), so the second process
  loaded only the two tasks of its old file: the page came back drawn at the end of that
  short page, not at the top, with no user action in between. With nine tasks then put in the
  file, the process was ended **again** without touching the screen, and reopened: the page
  came back exactly where the first process had left it (the same rows at the same pixels as
  the screenshot taken before the first kill). The goal survived a process death while it
  was still waiting, as designed.
- **Mid-page.** Scrolled by hand to a middle offset, ended, reopened: the page came back at
  the same offset, row for row.
- Before this milestone, the same steps brought the page back at the top (milestone 762's
  open point, reproduced with traces at the start of this milestone).

## Phone checks of milestones 766, 767 and 770

- **Milestone 766, the floating action button.** On the Tasks page the button floats at the
  end above the bottom navigation bar; the "Saved" snack bar lifted it above itself and let it
  down again when it left. On a task's page the button is docked (`EndDocked`) astride the
  bottom app bar's top edge, in a notch cut around it with an even gap (zoomed screenshot).
  The demo has no page with both a button and a persistent footer, and no top or mini
  placement, so those were not seen on the phone; they are covered by the 766 tests only.
- **Milestone 767, tables.** The About tab's two-column `Table` ("Metric" / "Value") has its
  labels well apart, each column starting at the same place in every row.
- **Milestone 767, autocomplete: a bug.** In the About tab, the tags `Autocomplete` sits in a
  row beside the "Info" button. On the phone it shows only its label "Tags", lower than the
  button's text, with **no field outline and no placeholder**, and it cannot be focused: taps
  on the label, to its right and below it raised no keyboard (`dumpsys input_method`:
  `mInputShown=false`). Milestone 767 expects the field to keep its 260 px fallback there,
  and the demo's test through `Application::view` asserts that; the phone draws something
  else. The cause was not looked into here.
- **Milestone 770, a field above a landscape keyboard on the Web** (web release build served
  locally, Chrome on the phone through `adb reverse`). In landscape, Chrome's address bar and
  its "No internet connection" strip leave the page about 16 CSS px between them and the
  keyboard. Tapping the task field brought the keyboard up and the page was panned down to
  the top of the card that holds the field, but a field about 45 CSS px tall cannot fit in
  16 px, so the field itself was **not visible** while typing. With the keyboard closed, the
  page stayed scrolled with the field just above the visible area. Whether that strip is
  the whole room a real user would have (without the offline strip it would be about 35 CSS
  px, still less than the field) was not settled. Separately, touch drags (`adb input
  swipe`) did not scroll the demo's page in Chrome at all, even on a freshly loaded page, so
  the field could not be brought back by hand; taps worked. Not investigated.

## Not done

- **The demo's Save on Android writes nowhere.** `todos_path()` is
  `std::env::temp_dir()`, which on Android is `/data/local/tmp`, a directory the app cannot
  write; the error is dropped and the snack bar still says "Saved". The demo's file there
  (two tasks, written by the shell) is what every start loads. A demo bug, not changed here.
- **The tags autocomplete in the About tab** (above): no outline and no focus on the phone.
- **Milestone 770's landscape check** did not show the focused field above the keyboard
  (above); and touch drags did not scroll the Web demo in Chrome on the phone.
- The floating action button's top, mini and footer placements were not seen on the phone.
- An application whose content stays shorter after a restore (the second process with fewer
  items than the first) keeps waiting for its goal until the user acts; then the region is
  left where it was drawn. No time limit was added (see *Considered*).
