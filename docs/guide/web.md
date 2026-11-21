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
previews, so the build never writes into your page. Copy that folder to any static host: GitHub
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
