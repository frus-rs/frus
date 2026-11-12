# Web pages inside the page

`WebViewWidget` shows a web page inside a frus page, and `WebViewController` drives it: what
it loads, its history, its scripts, the messages the page sends back.

| | Android | Desktop | Web | iOS |
| --- | --- | --- | --- | --- |
| `WebViewWidget` | ✓ | stand-in | stand-in | stand-in |

Elsewhere than on Android the widget shows its fallback for now (an `UnsupportedView`: a
labelled box in a debug build, nothing in a release build), and the calls that need an answer
answer `WebViewError::Unsupported`.

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
web.load_asset("help/index.html"); // the application's assets/ folder (on Android, the APK's)
web.load_file("/data/user/0/com.example/files/page.html");
web.reload();
```

## History, state and the Back button

`go_back()`, `go_forward()`, `can_go_back()`, `can_go_forward()`, `current_url()`,
`title()`, `progress()` (0 to 100) and `is_loading()`. The controller is `Listenable`.

The system's Back goes back in the page's history while the page in sight has somewhere to go
back to, as a browser does. Once it has not, Back does what it always does: closes the screen.

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
address.

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

On Android a script that throws gives `null`, as the platform reports it.

The controller holds its hooks, so a hook that calls the controller back holds a weak handle
(`web.downgrade()`, then `weak.upgrade()`): a hook holding the controller itself would keep the
page alive for ever.

## Settings and cookies

```rust
use frus::{Color, WebViewController, WebViewCookie, WebViewCookieManager};

let web = WebViewController::new();
web.set_user_agent(Some("MyApp/1.0"));
web.set_background_color(Color::WHITE);
web.enable_zoom(false);
web.clear_cache();
web.clear_local_storage();

let cookies = WebViewCookieManager::new();
cookies.set_cookie(WebViewCookie::new("session", "abc", "example.com"));
let _cleared = cookies.clear_cookies(); // await it: whether there were any
```

Settings made before the page exists are kept and applied when it is made.

## How it behaves among frus widgets

The page is a native view (see [Native views](native-views.md)): drawn by frus where the
widget is, under any dialog or menu frus draws over it, read by the screen reader in its
place.

- **Scrolling.** A drag that starts on the page scrolls the page while the page can scroll
  that way. Once the page is at its edge, or when the page cannot scroll at all, the drag
  goes to the frus list around it, as nested scrolling does. A pinch stays the page's.
- **The keyboard.** A field of the page that takes the keyboard brings the web view above it,
  as a frus field is brought.
- **Cost.** Nothing runs while no web view exists. A page that does not change costs nothing:
  frus copies its image only when it draws a new one.

## Another host for the same controller

The controller speaks to its view only through `PlatformViewController::invoke` and the
view's events, with the names in `frus::web_view_wire`. A host that makes views of type
`"frus/webview"` and speaks those names drives the same controller.
