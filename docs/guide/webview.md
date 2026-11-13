# Web pages inside the page

`WebViewWidget` shows a web page inside a frus page, and `WebViewController` drives it: what
it loads, its history, its scripts, the messages the page sends back. The same code runs on
every platform that has web views:

| | Android | Windows | Web | macOS, Linux, iOS |
| --- | --- | --- | --- | --- |
| The page | the system's `WebView` | WebView2 | an `<iframe>` | stand-in |

On macOS, Linux and iOS the widget shows its fallback (an `UnsupportedView`: a labelled box
in a debug build, nothing in a release build), `web_views_available()` is `false`, and the
calls that need an answer answer `WebViewError::Unsupported`.

WebView2 is part of Windows 11 and of every up-to-date Windows 10; on a machine without its
runtime the page sends an error event and stays empty. On the web the page is a frame of the
browser, which can do less: see [On the web](#on-the-web).

Web views are a cargo feature, off by default, so that an application without one carries
none of it:

```toml
[dependencies]
frus = { version = "0.5", features = ["webview"] }
```

`frus build` and `frus run` then declare the `android.permission.INTERNET` permission a page
from the network needs. An APK built by hand with cargo-apk needs it in `Cargo.toml`:

```toml
[[package.metadata.android.uses_permission]]
name = "android.permission.INTERNET"
```

## A page and its controller

Keep the controller in the state of the page that shows it. The controller owns the web page:
a widget built again somewhere else (a screen pushed over it and popped, a tab switched away
and back) shows the same page, with its history and its scroll. The page goes when the last
copy of the controller is dropped.

```rust
use frus::{column, component, LinearProgressIndicator, Text, WebViewController, WebViewWidget, Widget};

fn help_page() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        let web = cx.use_memo((), || {
            let web = WebViewController::new();
            web.load_request("https://example.com/help");
            web
        });
        // The controller asks for a rebuild whenever the page's address, title, progress or
        // history change: read them while building.
        let title = web.title().unwrap_or_else(|| "Help".to_owned());
        Box::new(column![
            Text::new(title),
            LinearProgressIndicator::new(f32::from(web.progress()) / 100.0),
            WebViewWidget::new(&web).semantic_label("Help page"),
        ]) as Box<dyn Widget>
    }))
}
# let _ = help_page;
```

`WebViewWidget` takes the room it is given: the whole width, and the height a column has
left, or 300 pixels where there is none to take (in a scrolling list, say). `.height(...)`
fixes it.

## Loading

```rust
use frus::{WebRequest, WebViewController};

let web = WebViewController::new();
web.load_request("https://example.com");
web.load_request(WebRequest::new("https://example.com/me").header("Authorization", "Bearer t"));
web.load_request(WebRequest::post("https://example.com/search", b"q=frus".to_vec()));
web.load_html_string("<h1>Offline</h1>", None);
web.load_asset("help/index.html"); // the application's assets/ folder (see below)
web.load_file("/data/user/0/com.example/files/page.html");
web.reload();
```

`load_asset` loads from the application's `assets/` folder: the APK's assets on Android, the
`assets` folder beside the executable on Windows, the `assets/` address beside the page on the
web. `load_file` loads a file of the device (not on the web, where the page sends a
`WebResourceErrorType::File` error).

## History, state and the Back button

`go_back()`, `go_forward()`, `can_go_back()`, `can_go_forward()`, `current_url()`,
`title()`, `progress()` (0 to 100) and `is_loading()`. The controller is `Listenable`.

On Android the system's Back goes back in the page's history while the page in sight has
somewhere to go back to, as a browser does. Once it has not, Back does what it always does:
closes the screen.

## What the page does: `NavigationDelegate`

```rust
use frus::{NavigationDecision, NavigationDelegate, WebViewController};

let web = WebViewController::new();
web.set_navigation_delegate(
    NavigationDelegate::new()
        .on_page_started(|url| println!("loading {url}"))
        .on_progress(|percent| println!("{percent}%"))
        .on_page_finished(|url| println!("loaded {url}"))
        .on_url_change(|change| println!("now at {:?}", change.url))
        .on_web_resource_error(|error| println!("{:?}: {}", error.error_type, error.description))
        .on_http_error(|error| println!("HTTP {}", error.status))
        .on_navigation_request(|request| {
            if request.url.starts_with("https://example.com") {
                NavigationDecision::Navigate
            } else {
                NavigationDecision::Prevent // say, open it in the browser with frus::open_url
            }
        }),
);
```

The hooks run on the interface's thread between frames. A component changes its state from
them through its `StateHandle` (`cx.handle()` in a `State`) or a hook's setter.

A navigation is decided between frames: the view holds it back, asks the delegate, and loads
the address if it goes ahead. A `POST` that goes ahead is loaded again as a `GET` of its
address. Loads the application asks for (`load_request`, `go_back`, `reload`, …) are never
asked about.

On Windows, WebView2 reports no progress of its own: `on_progress` hears 10 when a page
starts, 50 when its content starts arriving, 80 when its document is read and 100 when it is
loaded.

## JavaScript

```rust
use frus::{spawn_local, JavaScriptMode, WebViewController};

let web = WebViewController::new();
web.set_javascript_mode(JavaScriptMode::Unrestricted); // the default
web.run_javascript("document.body.style.background = 'papayawhip'");

// The value comes back as JSON: a string is quoted.
let asked = web.run_javascript_returning_result("document.title");
let _task = spawn_local(async move {
    if let Ok(title) = asked.await {
        println!("the page is called {title}");
    }
});

// The page posts with `Toaster.postMessage("...")`. A channel is seen from the next load.
web.add_javascript_channel("Toaster", |message| println!("{}", message.message))
    .expect("a JavaScript identifier, not yet used");
```

On Android and Windows a script that throws gives `null`, as the platform reports it; on the
web it gives `WebViewError::JavaScript` with the error's message.

The controller holds its hooks, so a hook that calls the controller back holds a weak handle
(`web.downgrade()`, then `weak.upgrade()`): a hook holding the controller itself would keep the
page alive for ever.

## Settings and cookies

```rust
use frus::{Color, WebViewController, WebViewCookie, WebViewCookieManager, WebViewError};

fn set_up(web: &WebViewController) -> Result<(), WebViewError> {
    web.set_background_color(Color::WHITE);
    web.set_user_agent(Some("MyApp/1.0"))?; // Unsupported on the web
    web.enable_zoom(false)?; // Unsupported on the web
    web.clear_cache()?; // Unsupported on the web
    web.clear_local_storage()?; // Unsupported on the web

    let cookies = WebViewCookieManager::new();
    cookies.set_cookie(WebViewCookie::new("session", "abc", "example.com"))?;
    let _cleared = cookies.clear_cookies(); // await it: Ok(whether there were any)
    Ok(())
}
# let _ = set_up;
```

The settings a platform cannot honour answer `WebViewError::Unsupported` at once and change
nothing. Settings made before the page exists are kept and applied when it is made. On
Windows the cookies go through the first web view made: cookie calls made before one exists
wait for it.

## How it behaves among frus widgets

The page is a native view (see [Native views](native-views.md)). On Android it is drawn by
frus where the widget is, under any dialog or menu frus draws over it, read by the screen
reader in its place. On Windows and the web it is a window or an element over frus's
surface, moved and clipped with the frame: a frus menu or button laid over it is cut out of
it, and under a dialog it is a picture (Windows) or hidden (the web).

On Android:

- **Scrolling.** A drag that starts on the page scrolls the page while the page can scroll
  that way. Once the page is at its edge, or when the page cannot scroll at all, the drag
  goes to the frus list around it, as nested scrolling does. A pinch stays the page's.
- **The keyboard.** A field of the page that takes the keyboard brings the web view above it,
  as a frus field is brought.
- **Cost.** Nothing runs while no web view exists. A page that does not change costs nothing:
  frus copies its image only when it draws a new one.

Everywhere, the page belongs to its controller: a widget that leaves keeps the page hidden
until a widget of the same controller comes back, with the page as it was.

## On the web

The page is an `<iframe>` of the browser, which keeps a page of another origin from the
application around it. So:

| | A page of the application's origin, or HTML given | A page of another origin |
| --- | --- | --- |
| Loading, reload, the background, scripts off (`sandbox`) | ✓ | ✓ |
| Address, title, back and forward | ✓ | the address frus loaded or the link followed; no history |
| `run_javascript`, `run_javascript_returning_result` | ✓ | `WebViewError::Unsupported` |
| JavaScript channels | ✓ (the page posts to its parent) | if the page posts `{frusChannel, message}` to `window.parent` itself |
| `on_navigation_request` | links the user follows | no |
| `on_page_started` | when frus loads, or when the next page is in | when frus loads |

- A request with headers or a body is fetched by the application's page and its HTML shown in
  the frame, so the server must allow it (CORS); a refusal is a `WebResourceErrorType::Connect`
  error.
- Many sites refuse to be framed (`X-Frame-Options`, `frame-ancestors`): the frame stays empty,
  and the browser says nothing to the page around it.
- The user agent, zoom, the cache, the storage and cookies are the browser's:
  `set_user_agent(Some(..))`, `enable_zoom(false)`, `clear_cache`, `clear_local_storage` and the
  cookie manager answer `WebViewError::Unsupported`.

## Another host for the same controller

The controller speaks to its view only through `PlatformViewController::invoke` and the
view's events, with the names in `frus::web_view_wire`. Android's, WebView2's and the web's
hosts speak exactly those names; a host that makes views of type `"frus/webview"` and speaks
them drives the same controller.
