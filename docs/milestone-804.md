# Milestone 804 — One web view API on every host

## Objective

Milestone 799 gave frus a web view API (`WebViewController`, `NavigationDelegate`,
`WebViewWidget`, `WebViewCookieManager`) and built it on Android alone; elsewhere the widget
showed a stand-in. Milestone 802 hosted native views on Windows (a child window: WebView2) and
on the web (an element of the page: an `<iframe>`), speaking the plain web view's older calls.
This milestone puts the web view API on top of those hosts and teaches them the controller's
protocol, so that one application's code shows and drives a web page on Android, Windows and
the web, in the shape a mature cross-platform web view plugin has: one controller API, one
implementation of the protocol per host.

## What the application writes

The same as on Android, with nothing new to learn:

```rust
let web = WebViewController::new(); // kept in the page's state
web.add_javascript_channel("Frus", |m| println!("{}", m.message))?;
web.set_navigation_delegate(NavigationDelegate::new().on_progress(|p| println!("{p}%")));
web.load_html_string(PAGE, None);

WebViewWidget::new(&web) // in the tree: Android's WebView, WebView2, or an <iframe>

if !web_views_available() { /* macOS, Linux, iOS: the stand-in */ }
web.set_user_agent(Some("MyApp/1.0"))?; // Err(Unsupported) on the web
```

What changed in the API (all unreleased, from milestone 799):

- `WEB_VIEWS_SUPPORTED` (a constant, true on Android) is now `web_views_available()`, a
  function: true on Android, on Windows (the shell registers WebView2 at start-up) and on the
  web.
- The settings a platform may not honour return `Result<(), WebViewError>`:
  `set_user_agent`, `enable_zoom`, `clear_cache`, `clear_local_storage`, and
  `WebViewCookieManager::set_cookie`. `clear_cookies` answers `Result<bool, WebViewError>`.
  They answer `WebViewError::Unsupported` at once, and change nothing, where the platform
  cannot do it (the web's frame, or no web views at all).
- `run_javascript_returning_result` answers `WebViewError::Unsupported` for a script the
  platform cannot run (a page of another origin in a browser's frame).
- The `webview` cargo feature now turns on `webview2` too: one feature gives an application
  web views on every platform that has them.

## Design

### One controller, one protocol, a host per platform

The controller (`frus-widgets/webview.rs`) talks to its view only through
`PlatformViewController::invoke` and the view's events, with the public names of
`frus::web_view_wire` (milestone 799). This milestone makes two more hosts speak those names,
so the controller learns nothing about platforms beyond what each one cannot do:

- `WebViewWidget` asks for a `"frus/webview"` native view wherever
  `platform_view_type_available("frus/webview")` says one is hosted: Android always, Windows
  once the shell registered WebView2 at start-up, the web always. Elsewhere it shows its
  fallback. Windows and the web place it as milestone 802 places any `PlatformView` (a child
  window, a `<div>` over the canvas, moved and clipped with the frame, cut where frus draws over
  it, pictured or hidden under a dialog).
- `frus-shell/web_view_protocol.rs` (new, pure, tested without a window) reads the calls
  (`Call::parse`, `Global::parse`, and `Params::parse` for the creation parameters) and writes
  the events, so both new hosts say exactly what Android's Java says. It also holds what a host
  needs that its platform does not do for it: `NavigationGate` (which navigations to hold back
  for the delegate) and `FrameHistory` (an `<iframe>`'s history, which the browser does not
  tell the page around it).
- A view whose creation parameters are a JSON object with a `controller` number is a
  controller's page and speaks `web.*`; any other parameters (an address, HTML) make milestone
  802's plain web view, unchanged, with its older names.

### Windows: WebView2

`webview2.rs` maps each call to WebView2: `Navigate`, `NavigateWithWebResourceRequest` (a
request with headers or a `POST` body, built with `CreateWebResourceRequest`),
`NavigateToString`, `Reload`, `GoBack`/`GoForward`, `ExecuteScript` (its JSON result is the
`web.javascriptResult` value), the settings (`IsScriptEnabled`, `IsZoomControlEnabled`,
`IsPinchZoomEnabled`, `UserAgent`, restored to the platform's own for `None`),
`DefaultBackgroundColor`, and the profile's `ClearBrowsingData` for the cache and the storage.

- **Events.** `NavigationStarting` is `web.pageStarted`, `SourceChanged` is `web.urlChange`,
  `NavigationCompleted` is `web.pageFinished` with `web.resourceError` (the WebView2 error
  status mapped to the error types) or `web.httpError` (its `HttpStatusCode`), and
  `HistoryChanged` and `DocumentTitleChanged` send `web.state` (address, title, `CanGoBack`,
  `CanGoForward`). WebView2 reports no progress: frus reports 10 at the start, 50 when content
  starts arriving (`ContentLoading`), 80 when the document is read (`DOMContentLoaded`) and 100
  when it is loaded.
- **Decided navigations.** While the delegate decides, `NavigationStarting` of a navigation the
  page started (a link, a script, a form) is cancelled and sent as `web.navigationRequest`; its
  cancelled completion is not reported, as Android reports nothing for a navigation it held
  back. The navigations the application asked for are never held: the gate remembers the last
  one asked for (its address when known, any address for back, forward, reload and HTML), and a
  server's redirect of a navigation let go goes too.
- **JavaScript channels** are scripts added with `AddScriptToExecuteOnDocumentCreated`, so they
  exist from a document's first line; `Name.postMessage(text)` posts `{frusChannel, message}`
  through `window.chrome.webview.postMessage`, and the `WebMessageReceived` handler hands it to
  the controller only for a channel the controller has open. A channel opened while a page is
  shown is also run in that page.
- **HTML with a base address** is served for that address through `WebResourceRequested`, so
  its relative addresses and its origin are the base's, as Android's `loadDataWithBaseURL` makes
  them.
- **Assets** come from the `assets` folder beside the executable; files by their `file:`
  address.
- **Cookies** go through the profile's `CookieManager` (`CreateCookie` and `AddOrUpdateCookie`;
  `GetCookies` to answer whether there were any, then `DeleteAllCookies`), from whichever web
  view is ready. Cookie calls made before any web view is ready wait in a thread-local queue and
  are carried out by the first one.

The handlers hold the page's state weakly, so a page does not keep itself alive through
WebView2's reference counts.

### The web: an `<iframe>`

`web_frame.rs` (feature `webview`) speaks the protocol within what a browser lets a page do
with a frame:

- **Loading.** An address is the frame's `src`; HTML its `srcdoc` (with a `<base>` for a base
  address, put after the doctype so that the page keeps its standards mode); an asset is the
  `assets/` address beside the application's page. A request with headers or a body is made
  with `fetch` and its HTML shown through `srcdoc`, resolved against its address; the server
  must allow it (CORS), and a refusal is a `connect` error. A file of the device is a `file`
  error.
- **Pages of the application's origin** (HTML given, or a same-site address): their address and
  title are read from the frame's document, scripts run through the frame's `eval` (an
  exception is `WebViewError::JavaScript` with its message), back and forward are the frame's
  `history`, and on each load the frame gets its channels' scripts (a `postMessage` to the
  parent) and a capture listener on links: while the delegate decides, a link is held back and
  asked about; otherwise its address is noted, so that the next page, if of another origin, is
  still named. HTML given gets its channels' scripts at its start, so a channel exists from the
  first line, as on Android. Messages are taken only from the frame's own window, and only for
  open channels.
- **Pages of another origin**: the address is what frus loaded or the link followed, the title
  is the address, the history stops at what frus knows, and a script's result answers
  `WebViewError::Unsupported`.
- **Scripts off** is the frame's `sandbox` without `allow-scripts`, from the next load.
- **What a frame cannot do** at all (a user agent of its own, zoom off, the cache, the storage,
  cookies) is refused by the controller before anything is sent: those calls return
  `Err(WebViewError::Unsupported)`. The storage especially: a page of the application's origin
  shares the application's own `localStorage`, which clearing it would wipe.

### The page belongs to its controller, everywhere

On Android the Java side keeps a `WebView` per controller (milestone 799). Windows and the web
now do the same in the host: the view of a controller's page that goes is **parked** (its
container hidden, WebView2 told it is not visible), and a view made for the same controller,
in the same frame or later, **adopts** that container with its page, its history and its
scroll instead of making a new one; the page's events follow the new view's number. A frame
moved in the document would load again, so on the web the container is adopted, never the
frame moved. The page is dropped on `web.release`, the call the controller makes when its last
copy goes, which the shell now sends on Windows and the web after each frame's views
(`send_web_view_calls`), as it did on Android.

### Cost

- Without the `webview` feature none of this is compiled: the protocol module, the frame host,
  the drain of global calls and of their answers are all behind it.
- With it, nothing runs until a web view is made: no thread, no timer, no polling. A frame with
  no global call is one thread-local borrow. The hosts work only when a call or an event comes;
  WebView2's events arrive through winit's own message loop, the web's through the page's event
  listeners. Nothing was added to the frame path.
- One cargo feature gives an application web views everywhere: `webview` now turns on
  `webview2` (WebView2's bindings are a Windows-only dependency, compiled nowhere else).

### Fixed on the way

The web host's listeners on the page's window (a frame's `blur`) were dropped with their view
but stayed registered, so every later `blur` of the page called a freed closure, which throws.
Listeners now remove themselves when dropped (`Listener`).

## How it was tested

**Unit tests** (Windows; the platform a test stands on is a thread-local, so tests running side
by side never see each other's):

- `frus-shell/web_view_protocol.rs`: the creation parameters of a controller's page read, and
  an address or HTML is a plain view; every call of `web_view_wire` reads with its arguments
  (a `POST`'s base64 body decoded), and every event the hosts write is one the controller
  reads; channel scripts post what the host reads; the gate holds the page's own navigations
  and lets the application's go (a redirect of one let go too, a fragment load lets no link
  through, a held navigation's end is not reported); an `<iframe>`'s history follows loads,
  back, forward, reload and a new load after going back; `file:` addresses, base64, a
  `<base>` and a script put after the doctype; WebView2's error statuses.
- **A fake page**: a `WebViewController` drives a page that speaks the protocol as the hosts do
  (`FakePage`, no window): it loads, a link the delegate prevents leaves the page where it is,
  one it allows is loaded as the application's own and not asked about again, back moves the
  history, and a channel message reaches the hook only for an open channel.
- `frus-widgets/webview.rs`: where web views are hosted the widget asks for a native view with
  the controller's parameters and shows no stand-in; without web views the calls that need an
  answer answer `Unsupported` at once and the stand-in shows; in a browser's frame the user
  agent, zoom off, the cache, the storage and the cookies answer `Unsupported`, are not kept and
  send nothing, while Android and Windows take every setting; a script a frame cannot run
  answers `Unsupported`.

**A real Windows window** (Windows 11, `frus-demo` debug build, opened on
`frusdemo://browser`, driven by a script that finds the window by its process and title,
finds the controls through UI Automation, refuses any click that does not land on the window,
and kills the process tree by its number at the end):

- The start page (HTML given to `load_html_string`) shows in WebView2 between the address bar and
  the footer; the app bar shows its title, "frus start page", from `web.state`.
- The page's "Send to frus" button (inside WebView2) posted to the `Frus` channel: frus showed
  "The page posted: Hello from the page at 13:13:57" and answered with `run_javascript`; the
  page showed "frus heard: Hello from the page at 13:13:57".
- "Ask the page its title": `run_javascript_returning_result("document.title")` answered
  `"frus start page"`.
- The page's link to example.com: the progress bar part way (80, `DOMContentLoaded`), then the
  page, the address bar `https://example.com/` (`on_url_change`) and the title "Example Domain".
- frus's Back button went back to the start page; Forward to example.com again.
- **Idle**: 0 ms of CPU time in 10 s, page loaded. Working set 179 MB, private 143 MB (the
  WebView2 processes not counted).

**The web** (`frus-demo` release for `wasm32-unknown-unknown`, bound with `wasm-bindgen`
0.2.127, served locally, headless Edge over the DevTools protocol at 900 x 1000 on
`#/browser`):

- The start page shows in the frame through `srcdoc`, with its title in the app bar.
- A real click on "Send to frus" inside the frame: the `Frus` channel's `postMessage` reached
  frus ("The page posted: …"), and frus's `run_javascript` answer showed in the page ("frus
  heard: …").
- "Ask the page its title": `"frus start page"`.
- A click on the page's link to example.com (another origin): the frame showed it and the
  address bar `https://example.com/`, named from the link followed since the browser hides the
  frame's address. "Ask the page its title" then answered "the platform's web views cannot
  do this" (`WebViewError::Unsupported`).

**The phone** (Huawei STK-L21, Android 10, release APK of `frus-demo` built from this branch
merged with master, 8,807,227 bytes, opened on `frusdemo://browser`): the start page, the
channel both ways, the title script, the link to example.com with address and title, and Back
behave as milestone 799 recorded; idle 2 CPU ticks in 10 s with the page loaded; 123 MB PSS.

**Size**: `frus-hello`'s stripped `libfrus_hello.so` (aarch64, the workspace's release profile,
built offline in WSL with rustc 1.96.1, from the branch's base `d7cc1692` and from the branch
before the merge): **13,728,088 bytes on both**. Without the `webview` feature nothing of this
milestone is compiled for Android.

**Checks**: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D
warnings`; `cargo clippy -p frus-shell -p frus-widgets --features frus-shell/webview --no-deps
--target wasm32-unknown-unknown -- -D warnings` (and without the feature); `cargo clippy -p
frus-shell --no-deps --target aarch64-linux-android` with and without `webview` (`frus-widgets`
there trips `missing_const_for_thread_local` in files of earlier milestones, as milestone 802
noted); `cargo check --target aarch64-apple-ios`; `RUSTDOCFLAGS="-D warnings" cargo doc
--workspace --no-deps --all-features`; `cargo test --workspace` on Windows, where everything
passes but 19 of `frus-test`'s goldens (advisory, kept from the Linux renderer; this milestone
changes no rendering).

## Not done

- **macOS, Linux and iOS** keep the stand-in: no native view host exists there yet (milestone
  802's `ViewHost` is where one goes).
- **The system's Back** goes back in the page only on Android; Windows and the web have no
  system Back for frus to give the page (the browser's own Back moves the frame's history,
  which is the browser's).
- **A page of another origin on the web**: no script, no channel unless the page posts
  `{frusChannel, message}` to its parent itself, no title, no history beyond what frus loaded,
  no decided navigation, and no `on_page_started` for its own links before they are loaded.
  Sites that refuse framing show nothing and say nothing.
- **Decided navigations on the web** cover the links the user follows in a page of the
  application's origin; a form or a script that navigates is not asked about.
- **On the web a request with headers or a body** is fetched by the application's page, so it
  needs the server's CORS consent, and the page shown is its HTML (its own scripts run, but its
  address in the frame is the application's).
- **WebView2's progress** is four steps (10, 50, 80, 100), not a measure of bytes.
- **A script that throws** gives `null` on Android and Windows, as both report it.
- **Cookies on Windows before any web view exists** wait for the first one; with none ever
  made, `clear_cookies` does not answer.
- **The page kept for a controller** (parking and adopting) on Windows and the web is not
  exercised by a unit test (the hosts need a real window or page) and was not driven in the
  real runs, whose demo keeps one widget per controller.
- **The title fix for another origin's page on the web** (its address as its title) came after
  the headless run, which showed the previous title kept; it is covered by the wasm build and
  lints, not by a second browser run.
