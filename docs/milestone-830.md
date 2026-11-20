# Milestone 830 — The page has text: find-in-page, printing, search engines

## Objective

Issue #212: on the web a frus page paints its words into a canvas, and as far as the browser is
concerned the page has no text. Find-in-page (Ctrl+F) finds nothing, a search engine or a link
preview indexes an empty page, and Ctrl+P prints a picture of the canvas cut at the window. That
is the usual reason a canvas-drawn toolkit is called fine for web *applications* and wrong for web
*sites*. The issue asked for a discussion first: the shape, its cost per frame and per node, and
what happens with a list of 10,000 rows, most of which is never built. The discussion is the
first half of this note; the second half is what was built and how it was checked.

"Done when": on the web demo, Ctrl+F finds a word on screen and one scrolled out of view, a search
engine fetching the page sees its text, and Ctrl+P prints readable pages.

## What existed

Milestone 564 projects the semantics tree into the DOM as **canvas fallback content**: one element
per node, diffed every frame, read by screen readers. Fallback content is never laid out or
painted, so the browser knows the words exist but not where. Chrome's find skips it, it does not
print, and it holds only what **shows**: the walk registers a semantics node only for a widget
with a visible box, so text scrolled out of view is not in it at all.

## Design

### Three shapes, three jobs

1. **A positioned, transparent text layer over the canvas** for find-in-page.
2. **A plain document** (headings, paragraphs, lists, in reading order) for printing and for
   search engines, written into `index.html` at build time.
3. **On-demand rows** for lists that build only what is in view.

The issue proposed all three as alternatives; they are not. Find needs positions (the browser
draws the highlight and scrolls to the match), a printed page needs flow (absolute boxes paginate
badly and cut lines in half), and an indexer needs the text without running anything. One source
feeds all three: a new registry the frame fills while it is walked.

### The registry: `Ui::page_text` and `Ui::page_lists`

`Widget::page_words()` (answered by `Text` and `RichText`, forwarded by every transparent
wrapper) gives the words, the font size, the line height and whether the text is a heading. The
walk records a `PageText` for every text it lays out **that shows, or that sits in a scroll
region** — the same rule the focus registry already uses so Tab can reach a field below the fold.
Each entry carries its laid-out rect (out of view when scrolled out of view) and its innermost
scroll region. It is cached and replayed with repaint boundaries, mapped through transforms and
withdrawn by barriers that hide paint (a layer of an `IndexedStack` that is not shown), like
every other registry.

It is recorded only when `Runtime::page_text` is set. The web shell sets it when it creates the
window; no other platform does, so a native frame never calls `page_words` and allocates
nothing. **Zero cost on native**: one `bool` test per laid-out widget.

### The layer

`crates/frus-shell/src/web_text.rs`. A `<div id="frus-text">` inserted right after the canvas,
`position: fixed` over the canvas's box, `pointer-events: none` (the canvas keeps every press and
the wheel), `aria-hidden="true"` (the bridge of milestone 564 already reads these words to a
screen reader; it must not read them twice), `color: transparent`. One `<div>` per text,
absolutely placed, with the text's font size and line height, so the browser's highlight lands
where the canvas drew the word, approximately: the browser lays the words out in its own font.
`::search-text` makes the highlight translucent and drops the browser's own copy of the matched
words, so the canvas's words show through it; a browser without that pseudo-element draws its
default highlight with the words in its own font, which is still readable.

**Scroll regions are boxes.** Each `Scrollable` of the frame is a box in the layer
(as big as its viewport) holding a sizer as big as its content, with the region's texts inside
the sizer at their **content** position: the rect, less the content's origin. Nested regions
nest. The box is scrolled (`scrollLeft`/`scrollTop`) as far as the application's region is,
within what the box can scroll (a region pulled past its end by an overscroll puts the box at
that end, so the browser's own stop is not read as a request); a reversed region counts from
its far end.

**The box scrolls, the reader cannot scroll it.** It is `overflow: auto` with its scroll bar
hidden. The first version had `overflow: hidden`, which a desktop browser's `scrollIntoView`
scrolls, but **Chrome's find bar on Android does not**: on the phone it found "Vibrate 400 ms"
(1/1) and left the box at 0, so the application never moved. A box that can scroll is scrolled
by it. The layer lets the pointer through (`pointer-events: none`, inherited by every box), so
the wheel, the touchpad and the finger land on the canvas, and a scroll goes up the target's
ancestors, which the boxes are not: the reader's scrolling still goes to the application alone.
Chrome on Android then scrolls the box but draws the match's highlight only once the box is
painted again; a find's scroll (one that is not this module's echo) is followed, 150 ms later,
by the box's scroll-bar style changed and changed back, which nobody sees and which has it
painted.

This gives two properties that make the whole design affordable:

- **Scrolling costs one write.** The application scrolling a region moves no text: in content
  coordinates they have not moved. Only the box's `scrollTop` changes.
- **Find scrolls the application for free.** A find that lands on a text out of view does what a
  browser does for any match: it scrolls the match's scrollable ancestors to show it. The box's
  `scroll` event, when it is not the echo of a scroll this module made (it remembers what it set,
  within a pixel), is queued and the loop woken; the shell turns it into
  `Runtime::scroll_to(region, Offset)` without animation. The next frame sets the box where the
  region now is, which is where the browser put it. No find event exists in any browser; this is
  the channel there is.

**Diffed, not rebuilt.** Each live element keeps the style string and the words it was last
given; a frame writes only what changed. Elements are created for new texts and removed for gone
ones; a region gone takes its box out.

### The list of 10,000 rows

A `ListView` builds the rows in view and a few around them. Its other rows exist nowhere — no
widget, no text — so neither the semantics tree nor the layer can have them, and a frame that
built them all to find out would defeat the list.

The answer is an opt-in the application can give at no cost: **`ListView::item_text(|i| ..)`**,
what row `i` says without building it. The frame records the list (`PageList`: its id, its
length, the closure, where it comes in reading order) and never calls the closure. It is called:

- **when the reader opens find-in-page.** The capture-phase key listener that already hands
  Ctrl+F to the browser (`web_keys`) now also calls `web_text::find_opened()`, which flags the
  next frame and wakes the loop. That frame lays every row into the list's box, at the place the
  list itself would scroll to for it (`Runtime::list_item_spans`: measured extents for the rows
  seen, the list's own guess for the others — the same numbers `ScrollTo::Item` uses). While they
  are laid in, the list's built rows are left out of the layer, so a word is not found twice.
  Find then reaches row 4321, scrolls the box to it, and the list scrolls there. The rows stay
  until the list goes or changes length, and are laid again at the next Ctrl+F.
- **when the page is printed**, and **when `frus build web` takes the page's text**: the plain
  document lists every row.

Without `item_text`, a lazy list's unbuilt rows are not found, printed or indexed; its built rows
are, like any text. That is stated in the method's documentation. Opening find from the
browser's menu instead of the keyboard does not lay the rows in (no event says it happened);
pressing Ctrl+F again does.

### Printing

`beforeprint` puts a `<div id="frus-print">` holding the plain document in the page;
`afterprint` takes it out. A print style hides everything else in the body — the canvas, the
layer — and lets the document flow: headings as `<h2>`, texts as paragraphs, a list's rows as a
`<ul>`, black on white. The rule is written `body:has(> #frus-print)`, so a page printed without
the event (none was found) prints as before rather than blank.

### Search engines

The web shell defines `window.__frusPageText()`, which returns the plain document.
`frus build web` now runs the built site in Chrome or Edge with no window (it serves it on a free
port, with a small script that waits for the answer to hold still for three quarters of a second
and posts it back to the server), and writes the document into the site's `index.html` between
`<!-- frus:page-text -->` and `<!-- /frus:page-text -->`, right after the `<body>` start tag, or
in place of the block a page already has. The page is read as markup, not searched as a string:
the demo's style sheet says "winit appends its canvas to `<body>`" in a CSS comment, and the
first version put the block there, inside `<style>`, where a crawler reads nothing. Comments,
`<style>`, `<script>` and the other raw-text elements are stepped over, and so are quoted
attribute values; markers outside the body are not the block.

**The site is a copy, added beside what was there.** `frus build web` used to name `web/`
itself as the site, so writing the text into its `index.html` changed a file of the sources on
every build (the first version did exactly that). Two ways out were weighed: keep `web/` as the
only output and not write the text at all, or add an output. The build now does everything it
did (the application compiled into `web/pkg/`, `web/` a working site) **and** then copies
`web/` (the page, its assets and `pkg/`, without dotfiles) to `target/frus-web/<package>/`,
emptied and made again on each build; the text is written into the copy only. This breaks
nothing: whatever reads `web/` reads what it read before.

What reads the old place, checked:

- `frus run -d web` and the hot-reload server serve `web/` and build `web/pkg/` themselves: unchanged.
- `frus clean` removes `web/pkg/`: unchanged; the copy is under `target/`, which `cargo clean` removes.
- The gallery's Pages workflow (`.github/workflows/pages.yml`) and the template's and forge's
  `release.yml` run `wasm-bindgen` into `web/pkg/` and copy `web/` by hand, without
  `frus build web`: unchanged, and they ship no first-screen text (they never did).
- The template's `.gitignore` already ignores `/target`: the copy never shows in `git status`.
- The browser check scripts serve a directory given to them; `web-page-text-check.py` now says
  to serve the copy.
- Docs that say where the site is, updated: the CLI's help, its README, the template's README,
  the eight examples' READMEs, `frus-hello`'s, the getting-started guide and the web guide.
- **frus-rs/frus-website, not edited here**: `docs/getting-started.md` line 492 and
  `docs/tooling/cli.md` line 197 say `frus build web` makes `web/`; they need the same line
  once this ships (the same wording as the CLI's README).

The
holder is out of sight (a clipped 1 px box, the usual "visually hidden" pattern), and the
application removes it as soon as its layer has text, so the words are never found twice. No
browser, or no answer within 60 s: a warning, and the page is left as it was.
`--no-page-text` skips the step. A crawler that runs the page gets the live layer instead.

## Cost

Measured in headless Edge on the demo, release build (`web-release`), with
`FRUS_MEASURE_PAGE_TEXT=1` at compile time, which compiles in a counter
(`window.__frusPageTextCost()`); without it the counter is not compiled.

Two runs of the check below (settings screen, then the journal and its Ctrl+F), on this
Windows machine's headless Edge:

| | run 1 | run 2 |
| --- | --- | --- |
| frames the layer published | 32 | 35 |
| time in the layer, all frames | 155.7 ms | 143.2 ms |
| of which laying in 5000 rows on Ctrl+F (one frame) | 110.1 ms | 101.6 ms |
| every other frame, on average | 1.47 ms | 1.25 ms |

The other frames include the first ones, which create every element; a frame where nothing
changed compares strings and writes nothing. Laying in the 5000 rows is a one-off ~100 ms
pause when Ctrl+F is pressed on a screen with such a list, and costs nothing until then. The
layer then holds 5004 texts.

- **Per frame**: one `getBoundingClientRect` of the canvas, a pass over the frame's texts and
  regions comparing strings, and a DOM write only for what changed. Scrolling writes one
  property per moved region.
- **Per node**: one `<div>` and one text node, plus a `String` of the words and of its style kept
  on the Rust side (about as much again as the semantics node the bridge already keeps per
  visible text). Per scroll region: two `<div>`s and a listener.
- **Native**: nothing; the registry stays empty and `page_words` is never called.
- **Size**: the web build grows by the layer's code; the Android budget is untouched (the
  widgets' share is a few small functions).

## How it was tested

- **Unit tests.** `frus-widgets` (`pagetext.rs`): nothing is recorded unless asked; texts
  scrolled out of view are recorded with their region and their laid-out place, which the
  semantics tree does not hold; a list with `item_text` is recorded where it comes in reading
  order and its closure is not called by a frame; `list_item_spans` places row 4321 of 10,000;
  a layer of an `IndexedStack` that is not shown is not on the page. `frus-shell`
  (`web_text.rs`): a box scrolls as far as its region and back, a reversed list included; a text
  keeps its place in the content wherever the region is scrolled; unbuilt rows are laid where the
  list puts them, from either end; the plain document reads in order, escapes, lists every row
  once and leaves out the rows built; `web_keys`: Ctrl+F opens find. `frus-cli` (`pagetext.rs`):
  the text goes after `<body>` the first time, replaces itself after, and a page without a body
  is left alone; with the demo's shape (a `<body>` in a CSS comment, in a script and in an HTML
  comment, a `>` in a quoted attribute) the block goes after the real tag, once, and a second
  build replaces it; markers in a script are not the block.
- **A real browser.** `scripts/web-page-text-check.py`, headless Edge over the DevTools
  protocol, on the demo built with `frus build web`:

  - **The fetched page** (`urllib`, nothing run) carries the first screen's text between the
    markers: 23 paragraphs and a heading. An HTML parser reads those words as the page's text,
    outside any `<style>`, `<script>` or comment (the check the first version failed: its block
    sat in the style sheet). Once the layer shows, the holder is gone.
  - **Find on screen**: `window.find` reaches "Volume: 0%", and its highlight lies over the
    canvas.
  - **Find out of view**: "Vibrate 400 ms" is not on screen (the accessibility tree does not
    have it); find reaches it, and the application scrolls to it: the tree then has it.
  - **5000 rows, a dozen built**: "Row 4321" is not found before; after Ctrl+F the layer holds
    5004 texts, find reaches it, and the list scrolls there and builds it.
  - **Printing**: `Page.printToPDF` gives 120 pages whose text has "Row 1", "Row 2500" and
    "Row 5000"; the printed document is gone after the print.

  `window.find` in a headless browser selects the match but scrolls nothing (measured: no box
  moved, no scroll event). The find bar scrolls the match into view through its scrollable
  boxes, so the script does that step itself (`scrollIntoView` on the match); it moves the
  region's box, fires its scroll event, and the application follows. The page was served by
  `python -m http.server` from `target/frus-web/frus-demo/`, and `git status` shows
  `crates/frus-demo/web/` unchanged after the build.

  The script also checks that the reader's scrolling never reaches a box: over a region's box,
  `elementFromPoint` is the canvas, four wheel turns all target the canvas, the application
  scrolls and the box follows it (0 to 480 px), and once the application stops the box gets no
  further scroll event (no loop between the box's event and the application's scroll). The
  screen-reader bridge's check (`web-accessibility-check.py`, milestone 564) and the text
  editing one (`web-text-editing-check.py`, milestone 711) pass on the same build.
- **A phone, with Chrome's own find bar.** A Huawei STK-L21 (Android, Chrome 157), the demo
  served from the built copy through `adb reverse`, opened at the settings screen; find opened
  from Chrome's menu and the words typed into its bar; the page read over the DevTools
  protocol (`adb forward`) and the screen captured.
  - **On screen**: "Volume" is found (1/1) and highlighted on the canvas's word (77 highlight
    pixels, on the word's line).
  - **Out of view**: with the first version's `overflow: hidden` box, "Vibrate 400 ms" was
    found (1/1) but the box stayed at 0 and the application did not move. With the scrolling
    box, the find scrolls it to 823 px, the application follows (the accessibility tree has
    "Vibrate 400 ms" afterwards) and the match is highlighted over the button (446 highlight
    pixels). Without the repaint after the scroll, the button showed no highlight.
  - **A finger** dragged over a region (`input motionevent`): every pointer and touch event
    targets the canvas (1 down, 14 moves, 1 up), and the box and the page do not scroll.
    The demo did not scroll for that injected drag either, with the layer hidden too, so the
    application's own response to it is not shown here.
- **The routine check**, on Windows: `cargo fmt --check`; clippy `-D warnings` on the host
  (all targets) and for wasm32; the tests of `frus-shell`, `frus-widgets`, `frus-cli`, `frus`
  and `frus-demo`; `cargo doc` with `-D warnings`; the eight frus-examples and the gallery's
  wasm build: all pass. Clippy for Android stops in `frus-core`, which this milestone does not
  touch (`missing_const_for_thread_local` on a `thread_local!` that is already `const`, from the
  local toolchain); with that one lint allowed it passes, `frus-shell` and `frus-widgets`
  included. `cargo check` for iOS could not run here: `ring`'s build script needs a `clang`
  this machine does not have. The new code is `cfg(web)` or platform-free.

## Not done

- **A desktop's find bar was not driven.** Chrome's bar on Android was (above); on the desktop
  the script's `scrollIntoView` stands in for the bar. Firefox and Safari were not tried
  (Firefox has no `::search-text`, so its default highlight shows), nor printing on the phone.
- **The phone's drag.** An injected drag (`adb input`) did not scroll the demo in Chrome, with
  or without the layer; whether that is the injection or the application's touch handling on
  the web is for another look. The layer is not in its way: every event went to the canvas.
- **Opening find from the browser's menu** does not lay a lazy list's rows in: no event says
  it happened. Ctrl+F (or Cmd+F) does.
- **Highlights are approximate.** The browser lays the words out in its own font, so a long
  line's highlight drifts from the canvas's glyphs.
- **The first screen only, for search engines.** `frus build web` writes the text of the page
  the application opens on, not of every route; a crawler that runs the page gets the live
  layer for the others.
- **Sites built by hand get no first-screen text.** The gallery's Pages workflow and the
  `release.yml` that `frus create` writes copy `web/` after a bare `wasm-bindgen`; moving them
  to `frus build web` would need a browser on the runner, and was left for later.
- **Laying in a huge list is one long frame**: about 100 ms for 5000 rows here. Rows are not
  laid in by parts.
- **Rich text is one paragraph.** A `RichText` is recorded as its plain text at its base
  style's size: its spans' sizes do not reach the layer, and it is never a heading (only
  `Text::heading()` is).
