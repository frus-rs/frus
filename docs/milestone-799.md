# Milestone 799 — Web views

## Objective

An application that shows a web page needs more than an embedded view: it loads addresses
with headers, shows its own HTML, moves in the history, runs scripts and reads their results,
hears the page (a channel the page posts to), decides which links the page may follow, follows
its progress and its failures, and sets cookies. This milestone gives frus that API on top of
the native views of milestone 794, in the shape application developers already know from the
leading toolkit: a controller, a navigation delegate, a widget.

## What the application writes

```rust
let web = WebViewController::new(); // kept in the page's state
web.set_navigation_delegate(
    NavigationDelegate::new()
        .on_progress(|p| println!("{p}%"))
        .on_navigation_request(|r| {
            if r.url.starts_with("https://example.com") {
                NavigationDecision::Navigate
            } else {
                NavigationDecision::Prevent
            }
        }),
);
web.add_javascript_channel("Toaster", |m| println!("{}", m.message))?;
web.load_request(WebRequest::new("https://example.com").header("Accept-Language", "en"));

WebViewWidget::new(&web).semantic_label("Help"); // in the tree

let title = web.run_javascript_returning_result("document.title").await?; // JSON
web.go_back();
web.current_url(); web.title(); web.progress(); web.can_go_back();
WebViewCookieManager::new().set_cookie(WebViewCookie::new("session", "abc", "example.com"));
```

The full list: `load_request` (GET with headers, POST with a body), `load_html_string`,
`load_file`, `load_asset`, `reload`, `go_back`, `go_forward`, `can_go_back`,
`can_go_forward`, `current_url`, `title`, `progress`, `is_loading`, `run_javascript`,
`run_javascript_returning_result`, `add_javascript_channel`, `remove_javascript_channel`,
`set_javascript_mode`, `set_user_agent`, `user_agent`, `set_background_color`, `enable_zoom`,
`clear_cache`, `clear_local_storage`, `set_navigation_delegate`. `NavigationDelegate`:
`on_navigation_request`, `on_page_started`, `on_page_finished`, `on_progress`,
`on_web_resource_error`, `on_http_error`, `on_url_change`. `WebViewCookieManager`:
`set_cookie`, `clear_cookies`. Guide: `docs/guide/webview.md` (doctest-compiled).

## Design

### The controller owns the page

In frus the tree is built again on every change, and a widget's identity follows its place in
the tree: a screen pushed over the page and popped, or a tab switched away and back, gives the
web view widget a new identity, and milestone 794 made a new native view for it. For a web
page that loses the history, the scroll and the page itself. So the page belongs to the
controller, as in the leading toolkit: the creation parameters carry the controller's number,
the Java side keeps one `WebView` per number, and a view made for a number that already has
one is handed that `WebView`, taken from the host that showed it before. The page is
destroyed when the last copy of the controller is dropped (`web.release`, sent after the
frame's own commands so that the view goes first).

`PlatformViews::sync` used to unbind a controller from a view that went. With two widgets of
one controller in a frame (the new one made, the old one dropped, in that order) that unbound
the new view: the controller is now unbound only from the view it is bound to.

### One channel, named

The controller talks to its view only through `PlatformViewController::invoke` and the view's
events. Arguments and event data are JSON; the names are public, in `frus::web_view_wire`
(`web.loadRequest`, `web.goBack`, `web.pageStarted`, `web.javascriptResult`, …), so another
host that makes `"frus/webview"` views and speaks them drives the same controller. Milestone
802 builds those hosts for the desktop and the web. The plain `AndroidView("frus/webview",
address)` of milestone 794 still works: parameters that are not a JSON object make the old
view, with its old methods and events.

Calls made before the view exists wait in the controller, in order. Settings (JavaScript, user
agent, background, zoom, channels, whether navigations are decided) are also kept in the
controller and passed as creation parameters, so a view made later starts with them.

Calls that are not for one view (cookies, a page let go) go through a queue the shell drains
each frame, and their answers (`web.cookiesCleared`) through a native method of their own.
Draining an empty queue is one thread-local borrow; the answers are read only when a flag says
there are some.

### What the page says

Every event goes to the widget, which hands it to its controller (`handle_event`). The
controller keeps the address, title, progress, loading state and history flags, notifies its
listeners (and asks for a rebuild), and calls the delegate. `run_javascript_returning_result`
returns a future, filled when the `web.javascriptResult` event with its number arrives; it is
awaited in a task on the interface's thread. `clear_cookies` works the same way.

A navigation decision cannot be answered synchronously: `shouldOverrideUrlLoading` runs on
Android's UI thread, the delegate on frus's. As the leading toolkit does, the view holds every
main-frame navigation back while a delegate decides them, sends `web.navigationRequest`, and
the controller loads the address if the delegate lets it go. A POST that goes ahead is loaded
again as a GET.

### The system's Back

`PlatformViewController` gained `set_handles_back`: the web controller sets it whenever its
history says it can go back. On Back, after the selection bar and the overlays and before a
screen is popped, the shell asks `Ui::platform_view_taking_back` for the topmost view in sight
that takes it and is not covered in its middle by something frus drew over it, and sends it
`web.goBack`. The flag is cleared until the page reports its new history, so a second Back
pressed before the page answered pops the screen rather than going back twice.

### A drag on the page inside a frus list (the 794 open item)

Milestone 794 gave the whole gesture to the view, so a frus list could not be scrolled by a
drag that started on a web view. Now:

- The Java host checks, each time it draws the view (scrolling redraws it), which ways the
  view's content can still scroll (`canScrollVertically(±1)`, `canScrollHorizontally(±1)`), and
  sends the four bits to the native side when they change (`nativeScroll`). No wake: they
  matter only to the next touch.
- When a press lands on a view, the shell records which ways frus can scroll around it (the
  scroll areas under the press with room to move).
- `TouchRouter`: once a one-finger drag passes the touch slop (8 dp), its axis is fixed. If
  frus scrolls that way and the content cannot move that way any further, the view is sent a
  `CANCEL`, frus is pressed where the finger went down, and the rest of the drag is frus's
  (`Routed::Handover`). A drag the content can follow stays the view's, but if the content
  reaches its edge mid-drag and the finger keeps going, frus takes it from there. A pinch, a
  drag across the list's axis, and a view outside any list are never handed over.

So a page that does not scroll (a banner, a form that fits) scrolls the frus list, and a long
page scrolls itself and then the list, as nested scrolling does.

### A native field above the keyboard

When the keyboard takes room and a native view holds the focus, the shell brings the view's
box above the keyboard with `Ui::reveal_rect`, the same least scroll that `Ui::reveal` makes for
a frus field.

### Zero cost

No new thread, no polling, no GPU resource. With no web view: one empty queue checked per
frame and an atomic flag. The Java side (a 12 KB larger bundled dex) is loaded only with the
first native view. No new crate.

## How it was tested

**Unit tests** (Windows):

- `frus-widgets/webview.rs`: a request goes out with its method, headers and base64 body; the
  base64 encoder against the standard's vectors; HTML, assets, history and scripts carry their
  names; the page state follows the events and notifies (address, title, progress, history,
  `handles_back`), the delegate hears started, progress and finished in order, and another
  view's event is ignored; a navigation is decided by the delegate (prevented: nothing loads;
  allowed: a load of its address; the view is told when the delegate stops deciding);
  channels need an identifier and a free name, are passed at creation, and are heard by name;
  errors carry their type (from Android's code or a name); settings are kept for a view made
  later and each controller has its own identity; a script's result waits for its answer (ok
  and failed); a cookie answer fills its own future; off Android a result answers
  `Unsupported` at once and the widget shows its stand-in; sizing.
- `frus-widgets/platformview.rs`: Back goes to a view in sight that takes it, not before it has
  history, and not under a dialog; a view holding the keyboard is scrolled into sight.
- `frus-shell/platform_view.rs`: a drag the page cannot follow is handed to the list (cancel to
  the view, frus pressed where the finger went down, the rest frus's); a drag the page can
  follow stays its until its edge, then is handed over from where the page stopped; a
  sideways drag, a pinch and a view outside a list are never handed over.

**On the phone** (Huawei STK-L21, Android 10, Mali-G51, release APK of `frus-demo`, opened with
`am start -d frusdemo://browser` and `frusdemo://native`):

- **The browser** (`/browser`): the start page (local HTML) fills the room between the address
  bar and the footer, and its title ("frus start page") reached the app bar through the
  controller. A first build showed the page with no height at all, because the demo's column had
  none to give; the widget now starts from 300 pixels and grows or shrinks from there
  (`WEB_VIEW_DEFAULT_HEIGHT`), and the demo's column takes the screen.
- **JavaScript channel, both ways**: the page's "Send to frus" button posted to the `Frus`
  channel; frus showed "The page posted: Hello from the page at 23:56:50" and answered with
  `run_javascript`, and the page showed "frus heard: Hello from the page at 23:56:50".
- **Navigation**: following the page's link to example.com showed the progress bar part way,
  then the page, with the address bar and the title ("Example Domain") following the page
  through `on_url_change` and the history state.
- **Back**: the system's Back went back to the start page and the screen stayed. A second Back,
  with no history left, closed the screen.
- **The 794 open item**: on `/native`, a vertical drag that starts on the web page (which does
  not scroll itself) scrolled the frus list by about 700 pixels. Two taps on the page's button
  still reached it ("Pressed 2 time(s)").
- **The keyboard**: tapping the native field low on `/native` opened the keyboard, and the page
  scrolled so that the field sits right above it.
- **Scrolling with web views on screen** (`/native`, frame statistics, 12 s of drags): median
  frame interval 53 to 63 ms, 95th percentile 65 to 67 ms in a first run; 67 to 75 ms and 73 to
  88 ms in a second run with the phone warmer. The plain `/licenses` page: 48 to 50 ms, 95th
  50 to 62 ms. The cost is the GPU's (render 46 to 70 ms; layout and paint under 5 ms): the
  same as milestone 794 measured, nothing new on the frame path.
- **Idle**: 0 CPU ticks in 10 s on `/browser` and on `/native`, page loaded, nothing moving.
- **Memory**: 114 MB PSS on `/browser`, 111 to 121 MB on `/native`, 133 MB on `/licenses`
  (PSS moves by tens of MB between runs on this phone; no web-view cost stands out).
- **Size**: the bundled dex grew from 61 328 to 73 300 bytes. The release APK of the demo is 8 295 227 bytes,
  against 8 237 883 built the same way from `master`: 57 344 bytes more, the demo's new screen
  included (the native library is 125 KB larger before compression, 58 KB after).

Not checked on the phone: the demo's footer is now kept above the navigation bar (a
`SafeArea`), after the screenshots showed it under it; checked by build and the demo's tests.

**Checks** (Windows unless said): `cargo fmt --all -- --check`; `cargo clippy --workspace
--all-targets -- -D warnings`; `cargo clippy -p frus-shell -p frus-widgets --no-deps` for
`aarch64-linux-android` and `wasm32-unknown-unknown` with `-D warnings` (WSL); `cargo doc
--workspace --no-deps --all-features` with `-D warnings`; `cargo test --workspace`: frus-widgets
2 356, frus-shell 622, the frus doctests (the new guide included) 102, and every other suite
pass. The golden comparisons of `frus-test` (19 of them) differ on this Windows machine's GPU;
they are checked under WSL's software renderer, and this milestone changes no drawing off
Android, so they were not run there.

## Not done

- **Windows, macOS, Linux, the web and iOS.** `WebViewWidget` shows its stand-in there, and the
  calls that need an answer answer `Unsupported`. Milestone 802 (branch
  `feat/desktop-web-views`) owns the desktop and web hosts (a WebView2 child window, an
  `<iframe>` beside the canvas); they speak the names of `web_view_wire`, so this controller
  drives them once both are merged.
- **A script that throws** gives `null` on Android, as the platform reports it; it is not
  turned into an error.
- **A POST that the delegate lets go** is loaded again as a GET of its address.
- **Pages that scroll inside an element** (a `div` with `overflow: auto` rather than the page
  itself) report to Android that they cannot scroll, so a drag on them goes to the frus list
  when the page sits in one. The leading toolkit has the same limit with its gesture
  recognisers.
- **The list hands nothing back to the page**: a drag frus has taken stays frus's to the lift.
- **HTTP authentication, file choosers, permission requests from the page (camera, location),
  full-screen video, downloads, console messages and scroll position APIs** are not covered.
