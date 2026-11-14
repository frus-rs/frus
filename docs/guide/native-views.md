# Native views inside the page

Some things are best left to the system: a web page, a map, an advertisement, a video
player, a sign-in button from a vendor's SDK. `PlatformView` puts a real native view inside a
frus page: an Android `View`, a Windows child window, an element of the web page. frus lays it
out like any other widget, keeps it where the widget is on every frame, and lets it have the
input that lands on it. The view keeps its own keyboard, focus and screen-reader behaviour.

frus's own view types are made on each platform by what that platform has, and understand the
same calls and send the same events:

| View type | Android | Windows | Web | macOS, Linux, iOS |
| --- | --- | --- | --- | --- |
| `"frus/webview"` | `WebView` | WebView2 (feature `webview2`) | `<iframe>` | stand-in |
| `"frus/edittext"` | `EditText` | `EDIT` control | `<input>` | stand-in |
| `"frus/video"` | stand-in | stand-in | `<video>` | stand-in |

Anything else comes from a plugin that registers a factory: a Java or Kotlin class on Android
(see [Writing a factory](#writing-a-factory)), Rust on Windows and the web (see
[A factory for Windows or the web](#a-factory-for-windows-or-the-web)).

Where a view type has no factory, the widget shows its fallback. By default that is an
`UnsupportedView`: a labelled box in a debug build, and nothing at all in a release build.

`AndroidView` is the same widget for Android alone: it shows its fallback everywhere else,
even where a `PlatformView` of the same type is hosted.

The web view on Windows is behind the `webview2` feature of `frus`, off by default (the
`webview` feature of [web views](webview.md) turns it on too):

```toml
frus = { version = "0.5", features = ["webview2"] }
```

It uses the Edge WebView2 runtime, which Windows 11 ships with. Where the runtime is missing,
the view sends a `frus.error` event that says so.

## Embedding a view

```rust
use frus::{column, component, Button, PlatformView, PlatformViewController, Text, Widget};

fn help_page() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        // The controller is kept across rebuilds; the view is made once and kept while
        // the widget stays.
        let web = cx.use_memo((), PlatformViewController::new);
        let title = cx.use_state(String::new);
        let shown = title.clone();
        let ask = (*web).clone();
        Box::new(column![
            PlatformView::new("frus/webview", "<h1>Help</h1><p>Local HTML, no network.</p>")
                .height(240.0)
                .controller(&web)
                .semantic_label("Help page")
                .on_event(move |event| {
                    if event.name == "javascriptResult" {
                        shown.set(event.data.clone());
                    }
                    None
                }),
            Button::new("Read the title")
                .on_press(move || ask.invoke("evaluateJavascript", "document.title")),
            Text::new(format!("Title: {}", title.get())),
        ]) as Box<dyn Widget>
    }))
}
# let _ = help_page;
```

- **Size.** The view takes the whole width it is given and 300 pixels of height, unless
  `.width()` and `.height()` say otherwise. It is sized like any other box, so it can sit in a
  `Row`, an `Expanded` or a scrolling list.
- **Lifetime.** The view is made the first time the widget is laid out. It is resized on the
  frame its size changes and moved with its box — on Android once the box has stopped moving,
  since frus draws its image where the frame has it and a view moved every frame of a scroll
  would be drawn again every frame — and dropped when the widget leaves the tree. A view
  scrolled out of sight is kept. A list that builds only the rows on screen drops the
  view with its row, as it drops any other row.
- **Calls and events.** `PlatformViewController::invoke(method, arguments)` calls the view.
  Calls made before the view exists wait for it, in order. The view answers through events:
  `on_event` turns each `PlatformViewEvent` (a name and its data, both text) into a message.
  frus sends two events of its own. `frus.focus` (`"true"` or `"false"`) says the view took or
  let go of the keyboard focus. `frus.error` says the view could not be made, and why.
- **Pause and resume.** On Android the views hear when the activity is paused and resumed.

### What each built-in view understands

| View type | Parameters | Calls | Events |
| --- | --- | --- | --- |
| `frus/webview` | An address, or HTML when it starts with `<` | `loadUrl`, `loadHtml`, `evaluateJavascript`; on Windows and the web also `goBack`, `goForward`, `reload`, `stop` | `pageFinished` (the address), `javascriptResult` (the value, as JSON); on Windows also `pageStarted` |
| `frus/edittext` | The hint | `setText`, `setHint` | `changed` (the text), `submitted` (the text) |
| `frus/video` (web) | The video's address | `play`, `pause`, `setSource`, `seekTo` (seconds), `setMuted`, `setLooping` (`"true"` or `"false"`) | `loaded` (the duration), `playing`, `paused`, `ended` (the time), `error` |

The web view runs JavaScript. Loading an address from the internet needs the
`android.permission.INTERNET` permission in the application's manifest, which `frus build` and
`frus run` declare for every application.

On the web, `frus/webview` is an `<iframe>`, so the browser's rules for frames apply: many sites
refuse to be shown in a frame of another site, and a page from another site keeps its scripts
and its history to itself (`evaluateJavascript` then answers `null`, and `goBack` does
nothing). HTML given to the view, and pages of the application's own site, have none of these
limits.

For a web page, `WebViewController` and `WebViewWidget` do all of this and more, with one
API on Android, Windows and the web: history, navigation decisions, JavaScript channels,
cookies, and Back going back in the page on Android. See
[Web pages inside the page](webview.md).

## How the view behaves among frus widgets

### On Android

- **Drawing.** The view is drawn where the widget is, as part of the frame. Anything frus
  draws over it covers it: a dialog, a menu, a snack bar, a floating button. A page scrolling
  under it moves it in the same frame, with no lag.
- **Touches.** A press that lands on the view goes to the view, unless something frus drew over
  it takes presses there (a dialog's barrier, a button laid over the view). The gesture is
  then the view's. With the `webview` feature, a web view inside a frus scrolling list keeps
  a one-finger drag only while its page can follow it: a page at its top pulled down, or a
  page that does not scroll at all, gives the drag to the list, and so does a page that
  reaches its edge mid-drag. A pinch stays the view's. A gesture that starts in frus stays in
  frus, even when it moves over the view.
- **Keyboard.** A native text field, or a field in a web page, opens the keyboard as it would
  in any Android application (with the `webview` feature, the view is scrolled above the
  keyboard as a frus field is).
  A press anywhere in frus takes the focus away from it, and a frus text field taking the
  focus takes the keyboard back.
- **Screen readers.** TalkBack reads the view's own content where the view is drawn, in the
  order of the page. `.semantic_label()` names the view first.

### On Windows and the web

There the view is not drawn by frus. It is a child window of frus's window, or an element of
the page, and the system draws it **above** everything frus draws. frus keeps it in step with
the frame:

- **Moving.** The view is moved and resized on every frame its box changes, right after the
  frame is presented, so it scrolls with the page around it. A view scrolled partly out of a
  scrolling area is clipped to it, and a view out of sight is hidden.
- **Drawing over it.** What frus draws over the view is cut out of it, so that it shows: a
  menu, a tooltip, a button laid over the view in a `Stack`. A dialog's scrim covers the view
  whole: on Windows the view is hidden and a picture of it, taken as the dialog opens, is drawn
  in its place, under the scrim; on the web it is hidden (a page cannot take a picture of a
  frame from another site).
- **Input.** The mouse, the touches and the keyboard go to the view by themselves. The whole
  gesture is the view's: a wheel or a drag over a web view scrolls the web page. A press on
  frus takes the keyboard focus back from the view.
- **What is not cut out.** frus cuts out only what it knows is drawn over the view: an
  overlay's box and the targets of presses. A decoration with no press target drawn over the
  view (a frus shadow, a painted badge in a `Stack`) is hidden under the view.

## Writing a factory

A factory is a Java (or Kotlin) class that implements `dev.frus.platform.PlatformViewFactory`.
frus asks it for a `PlatformView` each time a widget of its type appears. The three
interfaces are in
[`crates/frus-shell/java/dev/frus/platform`](../../crates/frus-shell/java/dev/frus/platform).
A plugin compiles against them. At run time its classes are loaded with frus's own as their
parent, so frus's copies are the ones used, even if the plugin's dex carries copies too.

```java
package com.example.map;

import android.content.Context;
import android.view.View;
import dev.frus.platform.PlatformView;
import dev.frus.platform.PlatformViewChannel;
import dev.frus.platform.PlatformViewFactory;

public final class MapFactory implements PlatformViewFactory {
    @Override
    public PlatformView create(Context context, int id, String params,
            final PlatformViewChannel channel) {
        final MapView map = new MapView(context);
        map.setOnMarkerClick(marker -> channel.send("markerTapped", marker.getTitle()));
        return new PlatformView() {
            @Override public View getView() { return map; }
            @Override public void dispose() { map.onDestroy(); }
            @Override public void onMethodCall(String method, String arguments) {
                if ("zoomTo".equals(method)) map.zoomTo(Float.parseFloat(arguments));
            }
            @Override public void onPause() { map.onPause(); }
            @Override public void onResume() { map.onResume(); }
        };
    }
}
```

Every method is called on Android's UI thread. `PlatformViewChannel.send` can be called from
any thread. `onMethodCall`, `onPause` and `onResume` have empty defaults.

The plugin compiles its Java into a dex in its build script, as the `frus create --template
plugin` template does. The simplest way to compile against the interfaces is to copy the three
files into the plugin's own Java sources. It then registers the factory once, before the
first view of its type is laid out:

```rust
/// The plugin's compiled Java: `include_bytes!(concat!(env!("OUT_DIR"), "/map.dex"))`.
static MAP_DEX: &[u8] = &[];

pub fn install() {
    // Elsewhere than on Android this is `Err(PlatformViewError::Unsupported)`.
    let _ = frus::register_android_view_factory(
        "com.example/map",
        "com.example.map.MapFactory",
        Some(MAP_DEX),
    );
}
# install();
```

The application then writes `PlatformView::new("com.example/map", "48.85,2.35")`. The view
types of a plugin should start with its own name (`com.example/…`): `frus/` is frus's.

## A factory for Windows or the web

On Windows and the web a factory is a Rust function. frus makes a container where the widget
is laid out and keeps it in place; the factory puts its view inside, and the view fills it.

```rust
struct Chart {
    channel: frus::PlatformViewChannel,
}

impl frus::NativeView for Chart {
    fn invoke(&mut self, method: &str, arguments: &str) {
        if method == "select" {
            self.channel.send("selected", arguments);
        }
    }
    fn resized(&mut self, _width: i32, _height: i32) {
        // Lay the view out again: the container is this many physical pixels now.
    }
}

pub fn install() {
    let _ = frus::register_platform_view_factory("com.example/chart", |context| {
        // Windows: `context.parent` is the container's HWND; make the view's window a child
        // of it at (0, 0). The web: append an element to `context.container`, at 100%.
        Ok(Box::new(Chart { channel: context.channel.clone() }) as Box<dyn frus::NativeView>)
    });
}
# install();
```

The view is dropped when its widget leaves. On macOS, Linux and iOS the call answers
`Err(PlatformViewError::Unsupported)`. The same plugin registers a Java factory for Android
under the same type, and one `PlatformView` shows it everywhere.

## What it costs

Nothing, until the first native view is laid out: no class is loaded, no window or element is
made, no thread runs and no memory is taken. On Windows and the web an idle view costs frus
nothing either: it is moved only on the frames its box changes.

On Android each view holds two images of its size: the one Android draws into and the copy
frus draws. A view that does not change is not drawn again: an idle web page costs no frame
and no CPU time. A view that animates is copied once for each frame it draws.

## Not yet

- macOS, Linux and iOS have no host yet: a `PlatformView` shows its fallback there.
- On Android the image the view draws is copied on the CPU before frus draws it. That is cheap
  for a page, a form or a map at rest, and noticeable for a full-screen video.
- A view drawn inside a frus transform (rotated, scaled by an animation) is drawn
  transformed on Android, but its touches and its place for the screen reader follow its
  untransformed box; on Windows and the web it keeps its untransformed box.
