# The web

A frus application runs in a browser as WebAssembly, drawing into one canvas with WebGPU where
the browser has it and WebGL 2 everywhere else. The same code runs; this page covers what is
different in a browser: building and hosting the site, the address bar, text input, fonts,
screen readers, and what a browser does not allow.

## Running and building

```sh
frus run -d web          # build, serve on http://localhost:8080, open the browser
frus build web           # the site, in target/frus-web/<package>/, ready for any static host
```

`frus run -d web` relays the browser's console to the terminal and patches each saved change
into the page (see [Running and hot reload](running.md)). `frus build web` compiles and shrinks
the application into `web/pkg/`, then copies `web/` to `target/frus-web/<package>/`: that copy is
the site, and its `index.html` gets the first screen's text for search engines and link
previews (and, with path addresses, every route a page of its own: see
[below](#find-printing-and-search-engines)), so the build never writes into your page. Copy that folder to any static host: GitHub
Pages, a bucket, nginx. `web/` itself still runs as a site too, only without that text. `frus create` makes the page with the
application's name, a loading indicator, and a message for a browser that can draw neither with
WebGPU nor with WebGL 2; edit it freely, keeping the canvas styles.

The application needs nothing of its own for the web beyond the `wasm-bindgen` dependency that
`frus create` writes: `frus::main!` makes the entry point the page calls.

## The address bar

With `FrusApp::router`, the router's location **is** the address. `https://example.com/#/users/42`
opens the application at that page with its parents under it; moving between pages changes the
address and adds a history entry; Back and Forward in the browser move the router; a reload comes
back to the same page. There is nothing to write for it.

By default the location is written after a `#`, which every static host serves. Plain paths read
better, and need the server to answer every address with `index.html`:

```rust
use frus::{text, FrusApp, GoRoute, GoRouter, LocationStrategy};

fn app() -> FrusApp {
    let router = GoRouter::new(vec![
        GoRoute::new("/", |_, _| text("Home")),
        GoRoute::new("/users/:id", |_, state| text(format!("User {}", state.param("id").unwrap_or_default()))),
    ]);
    // https://example.com/users/42 rather than https://example.com/#/users/42
    FrusApp::router(router).location_strategy(LocationStrategy::Path)
}
# let _ = app;
```

A page served under a folder (`https://example.com/app/`) sets `<base href="/app/">` in
`index.html`, and the paths are read under it. Routes and redirects are in
[Screens, navigation and dialogs](navigation.md).

## Find, printing and search engines

The canvas has pixels, not words, so frus gives the page its text three ways, with nothing to
write in the application:

- **Find in page.** A transparent layer over the canvas holds each text where it is drawn, the
  ones scrolled out of view included. The browser's find bar (Ctrl+F, Cmd+F) searches it,
  draws its highlight over the canvas, and a match out of view scrolls the application to it.
  A lazy `ListView` that says what its rows say (`ListView::item_text`) has every row found,
  built or not.
- **Printing.** Ctrl+P prints the page's text as a document: headings, paragraphs and every row
  of such a list, black on white, not a picture of the window.
- **Search engines and link previews.** `frus build web` opens the built site in Chrome or Edge
  with no window and writes what the application shows into the site's `index.html`, with a
  description made of its first words when the page has none.

**Every route gets a page of its own** when the addresses are paths
(`LocationStrategy::Path`): the build opens the application at each route and writes
`settings/index.html` for `/settings`, with that page's text, a title (its first heading, which
an `AppBar`'s title is, before the site's title) and a description. A static host
serves those files as they are, so a search engine that reads `/settings` without running
anything finds the words of that page. A route with a parameter (`/products/:id`) cannot be
listed from its pattern; list the locations worth a page on the router:

```rust
use frus::{text, GoRoute, GoRouter};

fn router() -> GoRouter {
    GoRouter::new(vec![
        GoRoute::new("/", |_, _| text("Home")),
        GoRoute::new("/products/:id", |_, state| text(format!("Product {}", state.param("id").unwrap_or_default()))),
    ])
    .prerender_paths((1..=20).map(|id| format!("/products/{id}")))
}
# let _ = router;
```

A route that redirects elsewhere (a guard sending an empty cart from `/checkout` to `/cart`)
gets no page, and neither does a location the site already has a file for. Each list gives
its first 200 rows to these pages, and a page holds at most 256 KB of text. Addresses that come
after a `#` (the default) are all one page to a server: only the first screen is written then.

A host that answers every unknown address with `index.html` (a "single-page application"
rule, or GitHub Pages' `404.html`) keeps working: the files the build wrote are found first,
and any other address gets `index.html`, which then needs a `<base href="/">` (or the site's
folder) so its relative URLs still resolve. `frus run -d web` answers every address that is not
a file with the page too, so a reload at `/settings` works there.

`frus build web --no-page-text` skips all of it, and a machine with neither Chrome nor Edge
builds the site without it, with a warning. On a build machine with no GPU, the browser draws
with its software renderer, which is enough to read the text.

## Text input

A browser gives a soft keyboard, an input method for Chinese or Japanese, dictation, autocorrect
and a password manager only to its own input elements. So while a frus `TextField` has focus, an
invisible `<input>` (a `<textarea>` for a multi-line field) sits over it and takes the typing;
frus draws the field as it does everywhere. Its `keyboard_type` and `autofill` hints become the
element's `inputmode`, `type` and `autocomplete`, so a phone shows the right keyboard and a
password manager offers to fill the field. Nothing is asked of the application.

## Fonts

A page cannot reach the computer's fonts. frus draws with the fonts it bundles (see
[Text and fonts](text.md)) and the ones the application registers; a character none of them has
is drawn as a box. An application that shows Japanese, Chinese, Korean, Thai or another script
the bundled faces lack registers a face for it, ideally a subset: every byte of a font is a byte
each visitor downloads.

## Screen readers

Every frame, frus writes the tree a screen reader needs as ordinary HTML elements inside the
canvas: a `<button>` for what can be pressed, other elements with their ARIA role, name and state
for the rest, and a live region for announcements. NVDA, JAWS, VoiceOver on a Mac and ChromeVox
read them, and Tab moves through them. A phone's screen reader cannot explore the page by touch,
because those elements have no position. See [Accessibility](accessibility.md).

## Browsers

A frus page runs in Chrome, Edge, Firefox and Safari, on the desktop and on phones, with or
without WebGPU:

- **The GPU.** WebGPU where the browser has it and it works, WebGL 2 everywhere else: no
  `navigator.gpu`, a GPU the browser blocks, no device, or a device that cannot draw in a canvas
  (it is tried on a canvas of its own before the page's is touched). The browser's console says
  which: `GPU interface: WebGPU`, or `GPU interface: WebGL 2 (why)`.
- **The platform's conventions.** A page follows the system the browser runs on, not the
  browser: in a browser on Windows, Linux or a Mac, lists stop at their ends as they do on that
  desktop (and bounce on a Mac); on an Android phone they stretch, on an iPhone or an iPad they
  bounce. `Theme::platform` and `ScrollConfiguration` change it like anywhere else.
- **The page's own effects.** frus turns off the browser's end-of-scroll effects on the page
  (`overscroll-behavior: none`): the elastic bounce of a touchpad, a sideways swipe back through
  the history, a phone's pull to refresh. A page that sets `overscroll-behavior` keeps its own.

`scripts/web-browsers-check.mjs` checks a built site in all of them that the computer can run:
Edge with WebGPU, without it, and with a WebGPU that fails at the canvas; Firefox; WebKit, the
engine of Safari; and both as phones.

## What a browser does not allow

- **Files and dialogs.** The desktop's file dialogs, tray, taskbar and global shortcuts do not
  exist in a page; [the system around the application](platform.md) has a table of what each
  platform offers. Preferences are kept in the browser's storage.
- **The network.** Requests go through the browser, so the server must allow the page's origin
  (CORS). See [Talking to a server](network.md).
- **Web views and native views** are `<iframe>`s and other elements, limited by what the browser
  allows across origins: see [Web pages inside the page](webview.md).
- **Threads.** Work that would run on a background thread elsewhere runs on the page's thread
  between frames; keep long computations short or split them.
- **Size.** The application is downloaded on every first visit. `frus build web` builds for size;
  `frus size web` says what the binary is made of. The fonts are the largest part, and a release
  `frus build web` cuts them to the characters the application can show (see
  [Text and fonts](text.md#the-release-build-carries-only-the-characters-you-can-show)).
