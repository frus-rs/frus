# Milestone 833 — Page text everywhere: the real find bar, Firefox, WebKit, every route

## Objective

Milestone 830 gave a frus page its text on the web: a transparent layer over the canvas for
find-in-page, a plain document for printing, and the first screen's words written into the
site `frus build web` makes. Its "Not done" listed four limits, and this milestone takes them on:

1. **The browser's real find bar was not tested on a desktop.** Headless, `window.find` selects
   a match and scrolls nothing, so the check script scrolled the match into view itself.
2. **Firefox, Safari and printing outside Chrome were not tried.**
3. **Search engines saw the first screen only**, not the application's other routes.
4. **Sites built by hand** (the gallery's Pages workflow, the `release.yml` that `frus create`
   writes) got no page text at all.

## 1. The real find bar, and the bug it found

`scripts/web-find-bar-check.py` opens a real, windowed browser on Windows, presses Ctrl+F on
the keyboard (`SendInput`) and types the words into the browser's own find bar, then reads the
page over the DevTools protocol (Edge, Chrome) or WebDriver BiDi (Firefox) and screenshots the
window before and after. Keys go to the browser's window only: it is found by its process id
(the largest visible window of the browser's process tree, not a bubble over it), kept on top
while the keys are sent, and each key is refused unless that window, or another window of the
same browser (the find bar is a window of its own), is the one in front. Edge runs InPrivate
and Chrome as a guest, so no account is signed in and no sync prompt covers the page.

**On the settings screen it passed at once**: Edge's find bar found "Vibrate 400" in a row
scrolled out of view, scrolled the region's box (0 to 977 px), the application followed, and
the highlight shows over the button.

**On the journal it failed.** The journal is a lazy list of 5000 rows; Ctrl+F lays every row in
(milestone 830). Typing "Row 4321", the find bar said 1/1, but the application stopped at row
39 in one run and at row 428 in the next, never at 4321. A trace of every write to a box's
`scrollTop` and every `scroll` event showed why:

- The find bar searches as each key is typed: "Row 4" (row 4), "Row 43" (row 43), "Row 432",
  "Row 4321". Each new match is scrolled into view, and on a desktop **smoothly**: the box
  moves over several frames.
- Each step of that animation changes `scrollTop`, but its `scroll` event is dispatched in a
  later rendering update. The frame in between saw the box away from where the application was,
  and put it back (`scrollTop = ` the application's offset). That ended the animation, and the
  event that finally came read the box where this module had just put it: an echo, ignored.
- So the application followed the first part of the first scroll, and every later scroll was
  undone before anyone heard of it. On the phone, in milestone 830, the find bar's scroll was
  not animated, which is why that check passed.

**The fix** (`web_text.rs`): before it writes a box's position, a frame compares where the box
is with where this module last left it. A box the browser moved since (more than a pixel, and
not just stopped short because its content shrank) is a request **now**: it is queued like a
`scroll` event's, the box is left where the browser has it, so its animation goes on, and the
event that comes later matches and is an echo. The application then follows the animation
frame by frame and ends at the match. The rule is `moved_by_browser`, a pure function with a
unit test (a move part way; a pixel of rounding; a box stopped at a shorter content's end).
Without the fix, the real find bar leaves the journal at row 39 or 428; with it, at row 4321
(see "How it was tested").

## 2. Firefox and WebKit

**Firefox 160, windowed, real Ctrl+F** (the same script, read over WebDriver BiDi): the layer is
over the canvas (WebGPU is offered), the find bar finds "Vibrate 400" out of view, scrolls the
box (0 to 947 px), the application follows, and the match is highlighted. Firefox has no
`::search-text`, so it draws its own green highlight with the matched words in its own font
over the canvas's words, slightly offset; readable, not as clean as Chrome's. Printing through
BiDi's `browsingContext.print` (which fires `beforeprint`, like a print from the menu) gives
123 pages of text with "Row 1", "Row 2500" and "Row 5000"; the printed document is gone after.

**WebKit**, the engine of Safari, through Playwright 1.65 (WebKit 27.2, Linux, under WSL):
`scripts/web-webkit-check.mjs`. The page starts and draws with WebGL 2 (headless WebKit there
offers no WebGPU adapter), the text written in at build time goes away once the layer shows,
`window.find` reaches "Volume" on screen and "Vibrate 400 ms" out of view, and, unlike a
headless Chrome, **WebKit's find scrolls the region's box itself** (0 to 730 px); the
application follows. For printing, Playwright prints to PDF in Chromium only, so the check
emulates the print media and sends `beforeprint`: the printed document holds all 5000 rows of
the journal and the print style shows it alone, the canvas hidden; `afterprint` takes it out.

Nothing needed fixing in either engine.

## 3. Every route has its own page

### How the routes are known

The router knows its routes. `GoRouter::paths()` lists every route's full path as declared
(sub-routes and a shell's branches included, a shell itself not being a place), and
`GoRouter::prerendered_paths()` the ones a build can write a page for: every path without a
parameter or `*`, then the locations the application lists with
`GoRouter::prerender_paths(..)`, each once. A route with a parameter (`/products/:id`) stands
for locations nobody can enumerate from its pattern; the application names the ones worth a
page (the navigation example lists its catalogue).

`Application::prerendered_paths()` (default: none) carries them to the shell; `FrusApp`
answers its router's. **Nothing runs in a released application**: the page `frus build web`
serves defines `window.__frusPrerender` before the application starts; the web shell, when it
reads its launch address, answers that object with the strategy and the paths, and only when
it exists. Any other page costs one property lookup at startup.

### What the build does

The probe script the build's server adds to the page now posts JSON: the first screen (text,
title, location, the strategy and the paths), then, for an application whose addresses are
paths, each location in turn. It moves the application there the way the browser's back button
would (the address changed with `pushState`, then a `popstate` event, which the shell already
handles as "the address was changed outside the application"), waits for the text to hold
still, and posts it with where the application ended up. One load of the module for all the
routes: a page reloaded per route would compile the module again each time.

`frus build web` then writes, for each location:

- `settings/index.html` for `/settings` (`products/3/index.html` for `/products/3`), made from
  `web/index.html` as it was before the first screen's text went in;
- a `<base href="../">` (as deep as the page) when the page has no `<base>` of its own, so
  `./pkg/app.js` and every relative URL resolve as from the site's root, whether the host serves
  the page at `/settings/` or `/settings`;
- the page's `<title>`: the one the application gave it (when the document's title differs
  from the site's), or its first heading before the site's title ("Cart | navigation"), or the
  site's title;
- `<meta name="description">`: its first words, cut at a word to 155 characters. The site's own
  `index.html` gets one too when it has none, and keeps its own title.

A location gets no page when it **redirects** (the navigation example's guard sends an empty
cart from `/checkout` to `/cart`; a page of `/cart`'s words at `/checkout` would mislead), when
the site already has a file there, when it showed no text, or when it is not a plain path (a
query, a `..`, a character a file name cannot hold). The build says which and why.

**Caps.** The page gives at most 200 rows per list to the build (`__frusPageText(200)`; a
print still takes them all), a page holds at most 256 KB of text (cut at the end of an element,
its list and `<main>` closed), at most 500 locations are visited, and each has 20 s to settle.

**Hash addresses.** With the default `LocationStrategy::Hash`, every address is the one page
to a server: a crawler does not fetch `/#/settings` as a page. The build writes the first screen
only, as before, and says that path addresses would give each route its own page. The demo is
such an application: its build says "26 other routes" would get pages.

### The fallback keeps working

A host's single-page rule (every unknown address answered with `index.html`) or GitHub Pages'
`404.html` still applies: a static host serves the files that exist first, so the written pages
are found, and any other address gets `index.html`. That page, answered at a deeper address,
needs a `<base href="/">` (or the site's folder) for its relative URLs; the build says so when
the page has none.

`frus run -d web` now behaves like such a host: an address with no file whose last segment has
no extension (`/settings`, `/products/3/`) is answered with `index.html`, given a
`<base href="/">` when it has no base of its own, so a reload at a path address works while
developing. A missing file (`/pkg/missing.js`, `/favicon.ico`) is still a 404.

The navigation example now writes its addresses as paths and lists its products:
`frus build web` writes pages for `/search`, `/cart`, `/order-placed` and `/products/1` to
`/products/N`, and none for `/checkout`.

### An app bar's title is a heading on the page

A route's title comes from its first heading, and the plain document took headings only from
`Text::heading()`. An `AppBar` already announces its title to a screen reader as the screen's
heading (`Semantics::heading` around it), and that is what the title of a page is. The walk now
carries a heading scope like its focus scopes: a widget answering the new
`Widget::page_heading()` (a `Semantics` whose role is `Heading`; forwarded by `Box`, the
transparent wrappers and `Responsive`) marks the texts inside it as headings on the page. It is
asked only while the page's text is recorded, so native frames never call it. The printed
page and the indexed text get the screen's title as a heading too.

## 4. The sites built by hand

- **The gallery** (`.github/workflows/pages.yml`) is now built with `frus build web`, using
  the command line of the same checkout (`cargo build --release -p frus-cli`, a minute: it has
  two dependencies), and publishes `target/frus-web/widgets/`. It thereby gets the release
  build's font subsetting too, which the hand-written steps skipped. The runner's Chrome reads
  the page with no GPU; a missing text is a warning on the run, not a failure.
- **The `release.yml` that `frus create` writes** (and forge's copy) installs the frus command
  line at the version of `frus` the application resolves (`cargo install frus-cli --version`,
  falling back to the latest) and runs `frus build web`, which installs the wasm-bindgen it
  needs, then zips `target/frus-web/<name>/`.
- `frus build web` adds `--enable-unsafe-swiftshader` to the browser it starts: recent Chrome
  no longer falls back to the software renderer for WebGL on its own, which a build machine
  with no GPU needs.

Neither workflow runs for a pull request: the Pages workflow runs on pushes to master, the
release on a tag. CI is no slower.

## Cost

- **A released application**: nothing new per frame. At startup, one `Reflect::get` of
  `window.__frusPrerender`; the routes are listed only when a build asks. `GoRouter` holds one
  more `Vec` (empty unless `prerender_paths` is called). The fix in the layer is two property
  reads per region per published frame, which the frame already made for the write it decides.
  `ci/size-budget.toml` is unchanged.
- **The build**: one load of the module, then about a second per route (the text has to hold
  still for three quarters of a second). The demo's build (hash addresses) is as before.

## How it was tested

- **Unit tests.** `frus-widgets`: the paths of nested routes, a shell's branches and
  parameters; what is prerendered with and without listed locations. `frus-shell`:
  `moved_by_browser`; the plain document caps each list's rows. `frus-cli`: a route's head gets
  its title, description and base (a base of its own kept, a `<title>` in a style sheet not
  taken for the real one); the file for each location, and the locations that have none; a long
  document cut at an element; the description and title read from a document; the posts read in
  order; `write_routes` writes each route's page, none for a redirect, none over the site's own
  file and none for hash addresses; the server answers an address with the page (with a root
  base) and a missing file with 404; `--no-page-text` is parsed.
- **Real browsers, on the demo built with this branch's `frus build web`, served from
  `target/frus-web/frus-demo/`:**
  - Edge 158, windowed, real Ctrl+F: settings, "Vibrate 400" found out of view, box 0 to 977 px,
    on screen afterwards, highlighted (3601 highlight pixels in the screenshot). Journal, "Row
    4321": **failed before the fix** (the application at row 39, then row 428), passes after.
    Chrome is not installed on this machine.
  - Firefox 160, windowed, real Ctrl+F: as above; printing to PDF: 123 pages, every row.
  - WebKit 27.2 (Playwright, WSL): starts with WebGL 2, the layer, find on and off screen, the
    print document; see section 2.
  - A headless Chromium 159 on Linux with `--disable-gpu --enable-unsafe-swiftshader` (what a
    CI runner has): the page answers `__frusPageText` with the settings screen's text.
- **Every route**, on the navigation example built with this branch's `frus build web` (about
  40 s for 11 locations after the compile): "Wrote a page with its own text for 10 more
  addresses: /search, /cart, /order-placed, /products/1, ... /products/7. No page written for:
  /checkout (it goes to /cart)." `scripts/web-routes-check.py`, with the site served by
  `python -m http.server` (no fallback): each route's page, fetched without running anything,
  carries its words, its title ("Cart | navigation", "Search | navigation") and description
  and a `<base>` as deep as the page; `/checkout/` is a 404; `/products/3/` opened in headless
  Edge starts the application at that product, and the written text gives way to the layer.
- **The checks of milestone 830** (`web-page-text-check.py`: the served text, find on and off
  screen, the wheel going to the canvas, no loop between box and application, 5000 rows, the
  PDF) pass on the new build.

## Not done

- **Chrome on the desktop and Safari itself were not driven.** Chrome is not installed here;
  it shares Edge's engine and find bar. Safari runs only on Apple's systems: WebKit through
  Playwright is the same engine, but not Safari's find bar, its print dialog or its WebGPU. A
  macOS job running Safari was not added: driving Safari's own find bar needs UI scripting and
  accessibility permissions a hosted runner does not grant, and a WebKit job would repeat what
  the Playwright check does. The check script can run on a Mac by hand.
- **Firefox's highlight** is its own (green, its own copy of the words, slightly offset from
  the canvas's); it has no `::search-text` to make it translucent.
- **Printing in Firefox and WebKit** was checked through the protocol (`browsingContext.print`,
  and the print media with `beforeprint` in WebKit), not through the browser's print dialog.
- **Parameterised routes** are written only for the locations the application lists with
  `prerender_paths`; nothing discovers them by following links.
- **Hash addresses** still get the first screen only; there is no single-URL way to give them
  more.
- **No sitemap** is written: it needs the site's absolute address, which the build does not
  know.
- **The workflows were not run**: the Pages workflow runs on a push to master and the release
  on a tag. The headless, GPU-less Chromium run above is the closest stand-in.
- **The routes are visited one after another in one page**, not each from a fresh start: a
  route whose words depend on what an earlier route left in the application's state shows that
  state.
