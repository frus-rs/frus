# Native views inside the page

Some things are best left to the system: a web page, a map, an advertisement, a video
player, a sign-in button from a vendor's SDK. `AndroidView` puts a real Android `View` inside
a frus page. frus lays it out like any other widget, draws it with the rest of the frame, and
gives it the touches that land on it. The view keeps its own keyboard, focus and
screen-reader behaviour.

Two views are built in: `"frus/webview"` (a `WebView`) and `"frus/edittext"` (an
`EditText`). Anything else comes from a plugin that registers a factory written in Java or
Kotlin (see [Writing a factory](#writing-a-factory)).

| | Android | Desktop | Web | iOS |
| --- | --- | --- | --- | --- |
| `AndroidView` | ✓ | stand-in | stand-in | stand-in |

On every platform but Android the widget shows its fallback. By default that is an
`UnsupportedView`: a labelled box in a debug build, and nothing at all in a release build.

## Embedding a view

```rust
use frus::{column, component, AndroidView, Button, PlatformViewController, Text, Widget};

fn help_page() -> Box<dyn Widget> {
    Box::new(component(|cx| {
        // The controller is kept across rebuilds; the view is made once and kept while
        // the widget stays.
        let web = cx.use_memo((), PlatformViewController::new);
        let title = cx.use_state(String::new);
        let shown = title.clone();
        let ask = (*web).clone();
        Box::new(column![
            AndroidView::new("frus/webview", "<h1>Help</h1><p>Local HTML, no network.</p>")
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
- **Lifetime.** The view is made the first time the widget is laid out. It is moved or resized
  only on the frames where its box changes, and dropped when the widget leaves the tree. A
  view scrolled out of sight is kept. A list that builds only the rows on screen drops the
  view with its row, as it drops any other row.
- **Calls and events.** `PlatformViewController::invoke(method, arguments)` calls the view.
  Calls made before the view exists wait for it, in order. The view answers through events:
  `on_event` turns each `PlatformViewEvent` (a name and its data, both text) into a message.
  frus sends two events of its own. `frus.focus` (`"true"` or `"false"`) says the view took or
  let go of the keyboard focus. `frus.error` says the view could not be made, and why.
- **Pause and resume.** The views hear when the activity is paused and resumed.

### What each built-in view understands

| View type | Parameters | Calls | Events |
| --- | --- | --- | --- |
| `frus/webview` | An address, or HTML when it starts with `<` | `loadUrl`, `loadHtml`, `evaluateJavascript` | `pageFinished` (the address), `javascriptResult` (the value) |
| `frus/edittext` | The hint | `setText`, `setHint` | `changed` (the text), `submitted` (the text) |

The web view runs JavaScript. Loading an address from the internet needs the
`android.permission.INTERNET` permission in the application's manifest.

## How the view behaves among frus widgets

- **Drawing.** The view is drawn where the widget is, as part of the frame. Anything frus
  draws over it covers it: a dialog, a menu, a snack bar, a floating button. A page scrolling
  under it moves it in the same frame, with no lag.
- **Touches.** A press that lands on the view goes to the view, unless something frus drew over
  it takes presses there (a dialog's barrier, a button laid over the view). The whole gesture
  is then the view's, from the press to the last finger lifted: a drag that starts on a web
  view scrolls the web page, not the frus list around it. A gesture that starts in frus stays in
  frus, even when it moves over the view.
- **Keyboard.** A native text field, or a field in a web page, opens the keyboard as it would
  in any Android application. A press anywhere in frus takes the focus away from it, and a
  frus text field taking the focus takes the keyboard back.
- **Screen readers.** TalkBack reads the view's own content where the view is drawn, in the
  order of the page. `.semantic_label()` names the view first.

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
    // Elsewhere than on Android this is `Err(PlatformViewError::Unsupported)`, and the
    // widget shows its fallback.
    let _ = frus::register_android_view_factory(
        "com.example/map",
        "com.example.map.MapFactory",
        Some(MAP_DEX),
    );
}
# install();
```

The application then writes `AndroidView::new("com.example/map", "48.85,2.35")`. The view
types of a plugin should start with its own name (`com.example/…`): `frus/` is frus's.

## What it costs

Nothing, until the first `AndroidView` is laid out: no class is loaded, no thread runs and no
memory is taken. Each view then holds two images of its size: the one Android draws into and
the copy frus draws. A view that does not change is not drawn again: an idle web page costs
no frame and no CPU time. A view that animates is copied once for each frame it draws.

## Not yet

- Native views exist on Android only. On the desktops, the web and iOS the widget shows its
  fallback.
- The image Android draws is copied on the CPU before frus draws it. That is cheap for a page,
  a form or a map at rest, and noticeable for a full-screen video.
- A view drawn inside a frus transform (rotated, scaled by an animation) is drawn
  transformed, but its touches and its place for the screen reader follow its untransformed
  box.
