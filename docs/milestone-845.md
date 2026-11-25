# Milestone 845 — A screen reader on the web reads the page and reaches every node

## Objective

Issue #18 asks that a frus application on the web work with a real screen reader. Milestone 564
built the bridge: one DOM element per semantics node, nested inside the `<canvas>` as its
fallback content. This milestone read what that bridge actually gives an assistive technology —
Chromium's computed accessibility tree, the tree NVDA, JAWS, Narrator, VoiceOver and TalkBack
are handed — and fixed what was wrong in it.

## What the computed tree showed

Read from the demo's home and settings screens on master (headless Edge,
`Accessibility.getFullAXTree`):

```
application "frus — Todo" {"focusable": true, "focused": true}
  heading "Mes tâches" {"focusable": true, "level": 2}
  image "" {"focusable": true}
  generic "Nothing to show for this filter." {"focusable": true}
  tab "Tasks, Onglet 1 sur 3" {"focusable": true, "selected": false}
  ...
  tab "Controls" {"focusable": true, "selected": false}
  generic "Notifications" {"focusable": true}
```

and `DOM.getBoxModel` on any node: *Could not compute box model*.

| Gap | Cause |
|---|---|
| A reader browsing the page reads none of the texts and headings | The canvas had `role="application"`. A reader in its browse mode presents an application as one item and does not walk into it: only what Tab reaches could be read, and a text is not a Tab stop. |
| A text is an element with no role and a name | It was given `role="text"`, which is not an ARIA role. The browser drops it, and a name on an element with no role is not read in browse mode. |
| The tab showing is never said to be selected | A tab's state was written as `aria-checked`, which a tab does not have; a tab is `aria-selected`. |
| No node has a box | Fallback content is never laid out. A reader that explores by touch (VoiceOver on a phone, TalkBack in Chrome) finds only the canvas under the finger, and a desktop reader draws its highlight around nothing. |
| A task added to the list is read last | A new node's element was appended to the end; the DOM order is the reading order. |
| Unnamed icons read as "image" | An image with no label was announced as just that. |
| Every text, heading and progress bar announced as focusable | Every node that was not a button got `tabindex="-1"`. |
| A reader's "increment" on a slider without the focus moved nothing | A browser sends an increment as an arrow key on the slider's element. The arrow bubbled to the canvas, where the shell gave it to whatever had the shell's focus. |
| The stepper and the rating could not be incremented at all, on any platform | Both say they are sliders (a range and a value) and neither answered an arrow: their buttons and stars take the focus, the node itself has no `on_key`. Android's increment reached the same nothing. |
| A filter chip never said it was the one on | Its state was `aria-checked` on a `button`, which the browser drops; a button that stays down is `aria-pressed`. |
| A progress bar of nothing out of nothing said "NaN" | The demo's 0/0 reached the bar's semantics as a value and as `aria-valuenow="NaN"`. |
| A dialog does not say it is modal | No `aria-modal`. |
| A password field reads as a plain field of dots | Nothing said it was a password. |

The bridge's own unit tests never ran: the module was compiled for the web only, and the tests
run on the host.

## What was done

**A layer over the canvas** (`crates/frus-shell/src/a11y_web.rs`). The elements leave the
canvas for a `<div id="frus-semantics" role="region">`, a sibling of it, laid exactly over it
(`position: fixed`, the canvas's box, moved with it), each element placed at its node's
rectangle scaled to CSS pixels. Everything in it is transparent and `pointer-events: none`: a
press goes through to the canvas as before (`elementFromPoint` at a button's centre is still the
canvas), while a reader's touch lands on the element, and a reader's "activate" is a `click` on
it. The layer clips rather than hides its overflow: an element out of view that a reader moves
to cannot scroll the layer and shift every node. The browser's focus is moved with
`preventScroll`.

**The words a node says are generated content** (`::before { content: attr(data-t) }`) for
text, headings and a field's contents. The accessibility tree reads generated content as text
(a `StaticText` node), and the browser's find-in-page does not search it — the page's words are
already found once, in the text layer of milestone 830, and would otherwise be counted twice.
Checked in a probe page before it was used: `window.find` finds plain text in such a layer and
not generated content; both are `StaticText` in the tree.

**Mapping fixes**, in a pure function (`node_dom`) that turns a node's annotation into a tag and
its attributes, so they are unit-tested on the host:

- no `role` for static text, its words as content;
- a heading's words as content;
- `aria-selected` for a tab (and a tree item, as before);
- an unlabelled image is `aria-hidden`;
- `tabindex="-1"` only on nodes the shell may focus — not on text, headings, images, progress
  bars;
- `aria-modal` on a dialog, `aria-roledescription="password"` on a masked field;
- `aria-pressed` for a button that toggles;
- a range that is not a number is left out;
- numbers written as a reader says them (`1`, not `1.0`).

**The stepper and the rating answer the arrows** (`crates/frus-widgets/src/stepper.rs`,
`rating.rs`): Left and Right step the value as their buttons and stars do, clamped, and nothing
when disabled. That is what a screen reader's increment and decrement reach on the web and on
Android. **A progress bar's value that is not a number reads as none done**
(`progressbar.rs`).

**The canvas has no role** — only the application's name — and the region is named after the
application, so a reader can jump to it as a landmark.

**Keys.** The elements are no longer inside the canvas, so a key pressed while one has the
browser's focus is routed (`key_route`, unit-tested): Enter and Space on a button stay the
browser's (they become the `click`); the browser's own shortcuts stay its own; a slider's or a
splitter's arrows become `A11yAction::Adjust` on **that** widget — the same action Android's
increment and decrement use, now shared (`adjust_for_assistive_tech`); every other key is handed
on to the canvas through the editing element's forwarder (milestone 711), with the browser's
default cancelled, so Tab moves the shell's focus and the bridge moves the browser's after it.

**Order.** After the diff, the layer's children are put in the tree's order when it changed.

**The editing element** (milestone 711) recognised a field's own node, as somewhere the focus has
gone "nowhere in particular", by its being inside the canvas; it now looks for it in the layer.

**Live regions** were already right in one place: the status element milestone 564 put outside
the canvas, rendered and visually hidden, carries every announcement, a widget's own and the
words of nodes marked live (milestone 810). The canvas's fallback content can no longer hold a
second one, and none of the layer's elements is `aria-live`: a node's words would be announced
twice.

## Cost

Nothing tells a page whether an assistive technology is running, so the bridge runs on every
page. Per frame it compares each node's annotation and rectangle with the last frame's and
writes to the DOM only what changed: a node whose annotation changed has the attributes that
changed written, a node that moved has its `style` written. A frame that changes nothing writes
nothing — measured: no mutation of the layer over 30 hover frames on the demo (the check's
`MutationObserver`). The canvas's box is read once a frame (`getBoundingClientRect`), as the text
layer already did. While a list scrolls, each node in it is moved once per frame, one attribute
write each.

## How it was tested

- Unit tests in `a11y_web.rs` (15), run on the host now that the module compiles there: the role
  map, text as content, headings, tabs selected, pressed buttons, checkboxes and switches
  checked, tree items, sliders' range and words, a range that is not a number, fields and
  passwords, disabled and expanded, hidden images, dialogs and descriptions, placement, and
  where each key goes. In `frus-widgets`: the stepper's and the rating's arrows, and a progress
  bar of NaN.
- `scripts/web-accessibility-firefox-check.py`, new: the same bridge in Firefox, below.
- `scripts/web-accessibility-check.py`, rewritten: headless Edge over the DevTools protocol, the
  computed accessibility tree, on the demo. Its output is below.
- The other browser checks that read the bridge were moved from `canvas [aria-label]` to the
  layer (`web-find-bar-check.py`, `web-page-text-check.py`, `web-frame-focus-check.py`,
  `web-webkit-check.mjs`) and run again: see below.

### The check, after (this branch)

```
the page is read as a page: a landmark named after the application
  ok   no role=application (a reader browsing the page skipped every text in it): None
    landmark: region 'frus — Todo' {}
text is text, and a tab says which one is showing
    empty-state text: StaticText 'Nothing to show for this filter.' {}
    tab: tab 'Tasks, Onglet 1 sur 3' {'selected': True}
    filter chip: button 'Toutes' {'pressed': 'true'}
    progress: progressbar '' {'valuemin': 0, 'valuemax': 100, 'valuetext': ''} value=0
  ok   unnamed images (each read as just 'image'): 0
every node has a box over the place it is drawn
    menu: button 'Menu' {} box=(4, 44, 48, 48)
    add: button 'Add' {} box=(380, 328, 76, 48)
  ok   a press there still lands on the canvas (the layer takes no pointer): CANVAS
adding a task announces it and names the checkbox after it
    checkbox: checkbox 'Wash the dog' {'checked': 'false'} box=(103, 517, 240, 48)
  ok   ...read where the list has it, before 'Clear completed', not appended last
    live region: status '' {'live': 'polite', 'atomic': True, 'relevant': 'additions text'} box=(0, 0, 1, 1)
    its words: StaticText 'Wash the dog added' {}
focusing and activating the checkbox through the accessibility tree ticks it
  ok   ...and is now checked: true
the browser's focus follows the application's
    focus: 'Wash the dog' -> Tab -> 'Delete task'
    focused in the accessibility tree: button 'Delete task' {'focused': True}
frames that change nothing write nothing to the layer
  ok   mutations of the layer over 30 hover frames: 0
settings: a switch, a radio, a slider, an assistive technology's increment
    heading: heading 'Settings' {'level': 2} box=(0, 36, 504, 56)
  ok   the 'Controls' tab is selected: True
    switch after a reader's click: switch '' {'checked': 'true'}
  ok   a reader's click picks 'Medium': true
    volume after ArrowRight twice: slider '' {'focused': True, ...} value=0.10000000149011612
    less: button 'Moins' {'disabled': True}
    quantity after an increment: slider '' {... 'valuemax': 20 ...} value=1
  ok   ...and 'Less' is no longer disabled: None
all good
```

(The demo ran in the machine's French locale: "Onglet 1 sur 3", "Toutes", "Moins".)

### The same check, on master

14 lines fail: the `application` role, no landmark, text not read as text, no tab selected
(both screens), no pressed chip, 2 unnamed images, no box for any node, the new task's checkbox
read after "Clear completed", the increment on the quantity doing nothing (and so "Less" staying
disabled), and the focus line (which looks for the layer). The idle-frame line passes vacuously
there (there is no layer to watch); watching the canvas's fallback content instead, master wrote
12 mutations over the same 30 hover frames, this branch none.

### The other browser checks, on this branch

| Check | Result |
|---|---|
| `web-text-editing-check.py` (711) | all good |
| `web-keys-check.py` | all good |
| `web-caret-keys-check.py` | all good |
| `web-frame-focus-check.py` | all good |
| `web-selection-bar-check.py` (568) | all good with the browser in English (`--lang=en-US`); in the machine's French locale its 8 lines looking for "Paste" and "Select all" fail, as the bar says "Coller" and "Tout sélectionner" |
| `web-page-text-check.py` (830) | all good but the first line, which needs the site `frus build web` writes, not the bare demo served here |
| `web-find-bar-check.py` (edge) | all passed |

### Firefox

Firefox gives automation no accessibility tree (WebDriver BiDi has none), so
`scripts/web-accessibility-firefox-check.py` (new) checks what Firefox builds its tree from, in a
real Firefox 160 window with WebGPU: the layer, a region named after the application, the
canvas without a role; a text without a role whose words are its generated content
(`getComputedStyle(el, '::before').content` is `"Notifications"`); the heading's words; the
switch's element with a box of its own and a press there landing on the canvas; the tab showing
selected; a click on the switch's element turning it on; an increment (the arrow event) on the
volume's element, unfocused, moving it to 0.05; real Right arrows with it focused moving it on to
0.15 and the focus staying on it; and a find reaching "Notifications" (in the text layer). All
good, twice in a row.

### A trap in the checks themselves

On Windows the process `subprocess.Popen` starts for Edge is a launcher: the browser outlives it,
and `kill()` — or `taskkill /T` on that process — leaves it serving the debugging port. Every
later check then opened its tab in the **first** browser left running, whatever flags it asked
for (the selection bar's run "in English" was in a French one, the frame check's 1000-pixel
window was a 420-pixel one). The accessibility check now closes its browser through the protocol
(`Browser.close`), which leaves nothing behind; the other checks were run one at a time with the
leftovers removed between them.

## Not done

- **Heard.** Still the computed tree a reader is given, not a reader listened to. No screen
  reader was run.
- **Browsers.** Chromium (Edge) was read through its computed accessibility tree; Firefox was
  driven at the DOM level (no tree is offered to automation). Safari/WebKit was not run: no
  WebKit build is installed here (Playwright's WebKit is not), so `web-webkit-check.mjs` was not
  run either. The WebGL 2 path (no WebGPU) was not run; the bridge does not depend on the
  renderer. No phone was used: touch exploration is now possible (every node has a box) and
  was not tried with a finger.
- **The tree is flat** (L-040): a list's rows are not inside a list, a dialog's contents are not
  inside the dialog, a tab is not inside a tab list. The browser accepts each, but a reader
  cannot say "list, 5 items" or "3 of 5".
- **Custom actions** (a row's Archive and Delete) have no web equivalent and are not offered.
- **Unnamed controls in the demo** — the settings switch and slider, the stars of the rating —
  are named by nothing: their caption is a separate text. That is the application's to merge
  (`Semantics::merge`), on every platform, and was left.
- **An announcement repeated word for word** is not said again: the live region speaks a change
  of its text, and the text did not change. As on the desktop.
- **A heading's level** is always 2: the semantics carry no level.
- **An increment in a right-to-left layout** is sent as the Right arrow, as on Android; a widget
  that reverses its arrows there would move the other way.
- **Not every check of the web was run**: the WebKit check (`web-webkit-check.mjs`) and the
  routes, address and permission checks, which do not read the bridge, were not.
