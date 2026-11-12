# Milestone 802 — Native views on Windows and the web

## Objective

Milestone 794 embedded native Android views in a frus page; on the desktops, the web and iOS
the widget showed a stand-in. This milestone hosts native views on **Windows** (a child
window: a system `EDIT` control, a WebView2) and on **the web** (an element of the page: an
`<iframe>`, an `<input>`, a `<video>`), through the same widget, the same controller and the
same calls and events, and lays the code out so that macOS, Linux and iOS can get a host the
same way.

## What the application writes

```rust
let web = PlatformViewController::new(); // kept in the application's state

PlatformView::new("frus/webview", "<h1>Help</h1>") // or an address
    .height(280.0)
    .controller(&web)
    .on_event(|event: &PlatformViewEvent| {
        (event.name == "pageFinished").then(|| Msg::Loaded(event.data.clone()))
    });

web.invoke("evaluateJavascript", "document.title"); // answered by a "javascriptResult" event
```

`PlatformView` is the widget for every platform. `AndroidView` stays as it was: Android only.

| View type | Android | Windows | Web | macOS, Linux, iOS |
| --- | --- | --- | --- | --- |
| `frus/webview` | `WebView` | WebView2 (feature `webview2`) | `<iframe>` | fallback |
| `frus/edittext` | `EditText` | `EDIT` control | `<input>` | fallback |
| `frus/video` | fallback | fallback | `<video>` | fallback |

The calls and events are Android's (`loadUrl`, `loadHtml`, `evaluateJavascript` answered by
`javascriptResult`, `pageFinished`; `setText`, `setHint`, `changed`, `submitted`; `frus.focus`,
`frus.error`), plus `goBack`, `goForward`, `reload` and `stop` for the web view on Windows and
the web. A controller written against Android's web view drives the Windows and web ones with
no change.

A plugin adds a type on Windows and the web from Rust:

```rust
frus::register_platform_view_factory("com.example/chart", |context| {
    // Windows: a child window of `context.parent`; the web: an element in `context.container`.
    Ok(Box::new(Chart::new(context)) as Box<dyn frus::NativeView>)
})?;
```

## Design

### Which types a platform hosts

The widget has to know, when it is built, whether a view will be made or the fallback shown.
`frus-widgets` keeps a list of hosted view types (`register_platform_view_type`,
`platform_view_type_available`). The shell fills it at start-up with its built-in types (the
Windows web view only with the `webview2` feature), and `register_platform_view_factory` adds
a plugin's. On Android every type is available, as before: its factories are Java classes, and
a type without one says so with a `frus.error` event. On macOS, Linux and iOS the list is
empty, so every `PlatformView` shows its fallback there.

`AndroidView` and `PlatformView` share one implementation (`Embedded` in `platformview.rs`);
the only difference is which platforms host them.

### Hosted over the surface: the airspace problem

On Android frus draws the view's image into its own frame (milestone 794). On Windows and the
web that is not possible in general: a child window and a page element are drawn by the system
**above** frus's swap chain or canvas. (frus's DirectComposition target is made with
`topmost = false`, so child windows stay above it.) Three things follow, handled by the pure
`Shape` of `platform_view.rs`:

- **Clipping.** The view's container is cut to the part of its box inside every clip around
  it: a view half scrolled out of a list shows only its visible half. Out of sight it is
  hidden.
- **What frus draws over it.** The walk records a new registry, the boxes overlays paint
  (`overlay_paint`: each overlay's box, a tooltip's too, and the whole window under a scrim).
  `Ui::platform_view_covers` gives, for a view, the overlay boxes and the press targets
  registered after it (a button laid over it in a `Stack`), without a menu's barrier, which
  takes presses but paints nothing. These are cut out of the container (a window region on
  Windows, a `clip-path` on the web), so a menu or a button over the view shows.
- **Covered whole.** A dialog's scrim covers the view entirely. On Windows the view is then
  pictured with `PrintWindow(PW_RENDERFULLCONTENT)`, which renders DirectComposition content
  too (WebView2's), and the picture is drawn by frus in the view's place, under the dialog: the
  scrim dims it like the rest of the page. The window is hidden one frame later, once the frame
  that draws the picture is on the screen, so the place is never empty (`Covers` in
  `view_host.rs`). On the web a page cannot picture a frame from another site, so the element
  is just hidden.

Shapes are computed in the host's pixels (Windows' physical pixels, the page's CSS pixels) and
compared from frame to frame: a view is reshaped only when its shape changes, and a view in
sight whole is its whole box whatever the rounding of the clip, so scrolling by fractions of a
pixel never reshapes it.

**Why not DirectComposition ordering.** The title line (milestone 643) uses DirectComposition,
and WebView2 can be hosted as a composition visual. Putting the view's visual *under* frus's
would require a see-through swap chain on every frame and holes punched in it where the view
is, and the view's input would then have to be forwarded by hand (visual hosting gets no
window messages). Cutting the window is cheaper and keeps the view's own input, focus and
accessibility exactly as the system gives them. The cost is that frus content drawn over the
view that is neither an overlay nor a press target (a shadow, a badge) is hidden under it.

### The hosts

- **`view_host.rs`** (every platform): the `ViewHost` trait a platform implements (create,
  place, shape, dispose, invoke, snapshot, clear the focus), the event queue the views send
  through (`PlatformViewChannel`, usable from any thread, waking the loop), the plugin
  factories, and `Covers`. A host for macOS (an `NSView` in the content view), Linux (a
  `GtkWidget`) or iOS (a `UIView`) implements the trait and registers its types; nothing else
  changes.
- **`windows_platform_view.rs`**: a container child window per view (`WS_CHILD |
  WS_CLIPCHILDREN`), moved with `SetWindowPos` only on frames the box changed, shaped with
  `SetWindowRgn`, shown and hidden with `ShowWindow(SW_SHOWNA/SW_HIDE)`. The `EDIT` control is
  centred in a white box with a frame, in Segoe UI at 15 logical pixels, its font made again
  when the window's scale changes. `EN_CHANGE`, `EN_SETFOCUS`, `EN_KILLFOCUS` become `changed`
  and `frus.focus`; Enter (a subclass of the control) becomes `submitted`.
- **`webview2.rs`** (feature `webview2`, crate `webview2-com` 0.39, MIT, over the `windows`
  crate frus already uses): the environment and the controller are made asynchronously, both
  answered from winit's own message loop. Calls, size and visibility wait for the controller,
  in order. The browser data lives in the application's data folder, not beside the
  executable. `SetIsVisible(false)` while hidden lets WebView2 stop drawing.
- **`web_platform_view.rs`**: a `<div>` per view, `position: fixed` over the canvas like the
  editing element of milestone 711, and the element inside it at 100%.

### Focus and the keyboard

The system gives the view the input that lands on it. A view taking the focus says so
(`frus.focus`): frus's own focused field lets go. A mouse press or a touch on frus while a
view holds the focus gives the focus back to frus's window (Windows) or the canvas (the web).
On Windows, the top-level window losing the focus to one of its own children is not the
application going inactive: `WindowEvent::Focused(false)` is ignored when the focus went to a
window inside frus's.

### DPI

Positions are physical pixels on Windows, computed from the logical layout with the window's
scale (and a simulated device's zoom and offset). WebView2 follows the monitor's scale by
itself; the `EDIT` control's font is made for the scale.

## How it was tested

**Unit tests** (Windows):

- `frus-widgets/platformview.rs`: a `PlatformView` is hosted where its type is registered and
  falls back elsewhere; an `AndroidView` of a registered type is still Android only. What frus
  paints over a view: a button laid over it, a dialog's scrim (the whole window), a menu's box
  (and not its barrier); a pressable parent drawn under it covers nothing.
- `frus-shell/platform_view.rs`: a view in sight is whole, also scrolled by fractions of a
  pixel; half under a scrolling area's edge; out of sight hidden; a menu over a corner leaves
  two parts; a scrim covers it. Cutting a box leaves every pixel but the cut exactly once
  (checked pixel by pixel). A hosted view is reshaped only when its shape changes; a view frus
  draws itself (Android) is never clipped.
- `frus-shell/view_host.rs`: a covered view is pictured, then hidden one frame later, and its
  picture released when shown again; a host that cannot picture hides at once; commands keep
  their order; events queue until taken; a registered factory makes its type available.

**A real Windows window** (Windows 11, Intel Iris Xe, DirectX 12; `frus-demo` debug build with
`webview2`, opened on `frusdemo://native`, driven by a script that finds the window by its
process and title and refuses any click that does not land on it; screenshots kept with the
milestone's working files):

- The WebView2 shows the local HTML page at its place among the frus cards, and the `EDIT`
  control under it. `pageFinished` reached the application ("The web view said:
  pageFinished: about:blank": the address of HTML given to `NavigateToString`).
- **Keyboard**: a click on the `EDIT` control, then `hello802` and Enter typed through the
  system's input: the control shows it, and its `changed` events made frus show "The native
  field holds: hello802".
- **Scrolling**: the mouse wheel over a frus card scrolled the page; the web view and the field
  moved with it, clipped under the app bar at the top of the scrolling area and at the
  window's bottom edge.
- **Dialog**: "Open a dialog" drew the frus dialog over both views, with the scrim dimming the
  pictures of the web page and of the field (the windows hidden, `PrintWindow` pictures drawn
  by frus). "Close" brought the live windows back, the field still holding its text.
- **Idle**: 0 ms of CPU time in 10 s with both views on screen and nothing moving.
- **Frames while scrolling** (debug build, `FRUS_FRAME_STATS=1`, the machine shared with
  several other builds): interval median 16.7 to 23 ms, 95th 21 to 38 ms; layout and paint
  0.4 to 0.7 ms; render 12 to 16 ms. The same page before the views were in sight scrolled at
  19.1 ms median in the same session: the views add no measurable cost to a frame, as frus
  draws nothing for them and moves two windows on frames the page moved.

**The web** (the demo built for `wasm32-unknown-unknown`, bound with `wasm-bindgen` 0.2.127,
served locally, headless Edge driven over the DevTools protocol at 900 x 1000, on `#/native`):

- The `<iframe>` shows the HTML page and the `<input>` sits under it, each in a `<div>` placed
  over its widget (`pageFinished: about:srcdoc` reached the application).
- A click on the `<input>` and `hello802` typed: "The native field holds: hello802".
- "Ask the page its title": `javascriptResult: "A native web view"` (a script run in the
  frame, its value as JSON).
- A wheel over the frus page scrolled it and moved both elements by the same 240 pixels. Scrolled
  further, the frame half under the app bar was cut to its visible part
  (`clip-path: path('M0 64h868v216h-868Z')`), the app bar drawn whole above it.
- "Open a dialog": both elements hidden (`visibility: hidden`) under the dialog and its scrim.

The elements of the page were checked through its DOM, the pixels through screenshots.

**Lints and builds**: `cargo clippy --workspace --all-targets -- -D warnings` (Windows),
`cargo clippy -p frus-shell -p frus-widgets --no-deps --target wasm32-unknown-unknown`,
`cargo clippy -p frus-shell --no-deps --target aarch64-linux-android` (`frus-widgets` there
trips a newer `missing_const_for_thread_local` in files this milestone does not touch), and
`cargo check -p frus-shell -p frus-widgets --target aarch64-apple-ios`. CI on the branch:
clippy, rustdoc, the wasm and iOS builds and the Android build.

**Size**: SIZE

**Not measured on a phone**: this milestone changes nothing Android draws or runs; the Android
path of milestone 794 is unchanged (`android_platform_view.rs` is as it was).


## Not done

- **macOS, Linux and iOS** have no host: the code is laid out for one (`ViewHost`), and a
  `PlatformView` shows its fallback there.
- **Undeclared paint over a view.** On Windows and the web, frus content drawn over a view that
  is neither an overlay nor a press target is hidden under the view.
- **A covered view is a still picture.** A video or an animation under a Windows dialog stops
  in the picture; on the web the covered element is hidden.
- **Transforms.** A view under a scaling or rotating frus transform keeps its untransformed box
  on Windows and the web.
- **Gestures are the view's.** A wheel or a drag over a web view scrolls the web page, never the
  frus list around it.
- **Screen readers.** On Windows the child windows are in the window's own UI Automation tree,
  but AccessKit's tree does not name them; their order relative to frus's nodes was not
  checked. On the web the elements are read by the browser where they sit in the page, after
  the canvas.
- **Keyboard navigation.** Tab does not move from frus's focus into a native view or back.
- **The web view on the web** is an `<iframe>`: sites that refuse framing show nothing, and a
  page from another site gives no script result or history.
- **A video on Windows** has no built-in type.
- The web view protocol of milestone 799 (`web.*` names, JSON arguments) was not merged when
  this was written: the Windows and web hosts speak milestone 794's names.
