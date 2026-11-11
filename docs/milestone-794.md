# Milestone 794 — Native Android views inside a frus page

## Objective

frus draws every pixel of its window itself, so an application could not show the things only
the system draws: a web page, a map, an advertisement, a vendor's sign-in button. This
milestone embeds a real `android.view.View` in a frus page. frus lays it out like any other
widget, draws it with the frame, and routes touches to it. The view keeps its own focus,
keyboard and screen-reader behaviour.

## What the application writes

```rust
let web = PlatformViewController::new(); // kept in the application's state

AndroidView::new("frus/webview", "<h1>Help</h1>") // or an address
    .height(280.0)
    .controller(&web)
    .semantic_label("Help page")
    .on_event(|event: &PlatformViewEvent| {
        (event.name == "pageFinished").then(|| Msg::Loaded(event.data.clone()))
    });

web.invoke("evaluateJavascript", "document.title"); // answered by a "javascriptResult" event
```

Two view types are built in: `frus/webview` and `frus/edittext`. A plugin adds others: it
writes a factory in Java and registers it once:

```rust
frus::register_android_view_factory("com.example/map", "com.example.map.MapFactory", Some(MAP_DEX))?;
```

```java
public final class MapFactory implements dev.frus.platform.PlatformViewFactory {
    public PlatformView create(Context context, int id, String params, PlatformViewChannel channel) { ... }
}
```

On the desktops, the web and iOS the widget shows its `fallback(...)`. The default fallback
is an `UnsupportedView`: a labelled box in a debug build, nothing in a release build. Its
colours and text style default to the theme's, and each can be set. A widget of an
application's own can embed a view too, by returning a `PlatformViewSpec` from the new
`Widget::platform_view` hook.

## Design

### Why the view is drawn into an image

The leading toolkit has two Android modes. In the first, the view sits in the window's own
view hierarchy, over or under the toolkit's surface, and the toolkit's content that must
appear above it moves to extra overlay surfaces. In the second, the view sits in the
hierarchy but its host redirects drawing into a texture, which the toolkit composites with
its frame.

frus runs in a `NativeActivity`, which calls `getWindow().takeSurface()`. The window's views
are measured and laid out, but **never drawn**: the native side owns the surface. This was
already observed in milestone 782: TalkBack's focus box never appears. So the first mode is
not available as it stands. A view added to the hierarchy would be invisible, and a view put
in a separate window raises its own problems. A window under frus's surface needs a
translucent swapchain and a hole punched in the frame. Its view cannot take the keyboard
focus from the activity's window, and the screen reader drops a window that a touchable one
covers whole. A window over the surface hides frus's dialogs, so it needs a second overlay
surface. Both kinds of window move through the window manager, a frame or two behind frus's
own drawing.

The second mode fits exactly. The view lives in the activity's hierarchy, inside a `Host`
`FrameLayout` (`FrusPlatformViews.java`). Focus, the IME and accessibility therefore work as
for any view of the window: the view is a sibling of the keyboard bridge and of the screen
reader's view. The host overrides `invalidateChildInParent` (which a window without
hardware acceleration calls) and `onDescendantInvalidated`. When anything inside the view
invalidates, the host schedules one draw on the next `Choreographer` frame, with
`Surface.lockHardwareCanvas()` on an `ImageReader` of the view's size. The newest image is
kept and the native side is told (`nativeFrame`). The loop wakes, copies the image, and
draws it where the widget was laid out, as an ordinary image primitive.

What follows from that:

- **Overlays need nothing.** A dialog, a menu or a snack bar over the view is drawn over its
  image, in the same frame.
- **Scrolling has no lag at all.** The image is drawn at the widget's position in the frame
  that has that position. The native view's own place in the hierarchy (moved with
  `setTranslationX/Y`, only on frames where the box changed) matters only to the screen
  reader and to IME popups.
- **An idle view costs nothing.** Nothing is drawn or copied until the view invalidates.

The copy is on the CPU: frus has no way yet to sample an Android `HardwareBuffer` in wgpu.
The image is RGBA8. It is copied in Java straight into a buffer the Rust side owns, packed
row by row, then made straight-alpha (Android draws premultiplied; only pixels that are not
opaque are touched). Milestone 793's external textures are the natural next step: the same
`ImageReader`, imported instead of copied.

### The split

- **`frus-widgets/platformview.rs`**: `AndroidView`, `PlatformViewController` (a queue of
  calls, bound to a view by the shell), `PlatformViewEvent`, `PlatformViewSpec`,
  `UnsupportedView`, and the latest image of each view (`set_platform_view_frame`).
- **`ui.rs`**: the walk records a `PlatformViewPlacement` for each widget whose
  `platform_view()` answers. A placement is recorded even when the widget is scrolled out of
  sight, so the view lives as long as its widget. It also records how many press targets of
  each kind had been registered so far. `Ui::platform_view_at(point)` answers a press with
  the topmost view under it, unless a target registered *after* the view covers the point:
  a hit, a long press, a drag, a listener, a scrollable area, or an overlay's cover (a
  dialog's barrier is one). A subtree holding a view is never replayed from the paint cache,
  because its image changes without a rebuild. Layer transforms move placements as they move
  the other registries.
- **`frus-shell/platform_view.rs`** (pure, tested on every host):
  - `PlatformViews::sync` turns placements into `Create`, `Place` (only when the box changed
    by a physical pixel), `Dispose` and `Invoke` commands. A widget painted twice in a frame
    is one view. A new view type in the same place is a new view. Queued calls follow the
    creation, in order.
  - `TouchRouter` routes touches. A press on a view makes the whole gesture the view's, with
    stable pointer ids, `POINTER_DOWN`/`POINTER_UP` for other fingers, and positions in the
    view's pixels. A gesture that starts in frus stays in frus, and the view is not even
    asked.
  - `packed_straight` unpremultiplies the pixels.
- **`frus-shell/android_platform_view.rs`** (JNI): at start-up it keeps only the class loader
  and the activity, as two global references. The class is loaded, its natives wired, and its
  container added to the activity the first time a view is asked for.
- **Java** (`FrusPlatformViews.java`, and `dev.frus.platform.{PlatformViewFactory,
  PlatformView, PlatformViewChannel}`): every command is posted to the UI thread. Touches are
  stamped with `SystemClock.uptimeMillis()` when the native side hands them over, not when
  the UI thread runs them, so fling speeds are read from real spacing. Factories from a
  plugin's dex are loaded with an `InMemoryDexClassLoader` whose parent is frus's own, so a
  plugin compiles against the interfaces and does not ship them. The dex is now built with
  `--min-api 24`, the lowest frus supports, so `PlatformView`'s default methods stay real
  default methods: a plugin compiled with or without desugaring finds them.

### Focus and the keyboard

A native view taking the focus is seen by a global focus listener and sent as a
`frus.focus` event. frus then drops its own field's focus *without* closing the keyboard:
`FrusTextBridge.stopInput` no longer hides the keyboard while a native view holds the focus.
A press that goes to frus while a native view has the focus clears that focus and hides the
keyboard, as a tap outside a native field does. A frus field taking the focus takes the
keyboard back (the bridge view requests focus).

### Screen readers

The view's host is left out of the window's own accessibility tree: its container's
`addChildrenForAccessibility` adds nothing. It is shown instead inside the frus node that
stands where the view is drawn. `AndroidNode::embed` names the view, the published tree
carries one more array, and `FrusAccessibility` calls `info.addChild(host)`. The host's own
node names that virtual node as its parent. An `AndroidView` with no `semantic_label` still
gets a node (role none) for this.

### Pause and resume

`set_lifecycle` tells the views when the activity pauses and resumes
(`PlatformView.onPause/onResume`; the web view pauses its timers).

## How it was tested

**Unit tests** (run on Windows):

- `frus-widgets/platformview.rs`: queued calls wait for the view in order and go once; images
  kept by widget and forgotten; events become messages; default sizing; the fallback stands in
  elsewhere and a custom one replaces it; the stand-in draws in debug only (unless asked). With
  a test widget that embeds a view on every host: the walk records the view's box and gives it
  a screen-reader node; a button drawn over the view takes the presses it covers, while a
  pressable parent drawn under it does not; an open dialog takes every press; a view scrolled
  out of sight is kept, one in sight inside a scrolling area keeps its presses; a page held in a
  repaint boundary still reports the view on every frame.
- `frus-shell/platform_view.rs`: made once, moved only when it moves, dropped when the widget
  leaves; one view for a widget painted twice; a new type is a new view; queued calls follow
  the creation and the controller is bound and unbound; a box scrolled by fractions of a pixel
  keeps its size; whole gestures go to the view (stable pointer ids, second finger, lifts in
  any order); a gesture that starts in frus stays there; a cancel; a dropped view forgets its
  gesture; premultiplied pixels made straight; registering elsewhere answers `Unsupported`.
- `a11y_android.rs`: a native view is read inside the node it is drawn in, and its arrival is
  a change for the screen reader.

**On the phone** (Huawei STK-L21, Android 10, Mali-G51, release APK of `frus-demo`, the new
`/native` screen, opened with `am start -d frusdemo://native`):

- The local HTML page shows in the `WebView`, drawn at its place in the page, and a `frus/edittext`
  under it (screenshots).
- **Touches**: two taps on the page's "Press me" button made it read "Pressed 2 time(s)". "Ask
  the page its title" gave `javascriptResult: "A native web view"` through `invoke` and an event.
  A drag on the frus cards scrolled the page and both views with it.
- **Keyboard**: a tap on the native field opened the keyboard (SwiftKey) and focused the
  `EditText`; `adb shell input text hello794` reached it, and its `changed` events made frus show
  "The native field holds: hello794". A drag on the frus page took the focus from the field and
  closed the keyboard.
- **Dialog**: "Open a dialog" drew the frus dialog and its scrim over the web view. A tap on the
  scrim, right over the page's button, closed the dialog and did not press the button (still
  "Pressed 2 time(s)").
- **Screen reader**: `uiautomator dump` (the tree TalkBack reads) shows the frus node "Web page"
  at `[41,1280][1040,1994]` and, right under it in reading order, the `WebView` and the page's
  own nodes: the heading, the paragraph, the three list items and the "Press me" `Button`, with
  their bounds, between the frus rows above and the frus buttons below.
- **Home and back**: the views came back drawn after the activity was paused and resumed.
- **Copying an image**: 0.2 to 7 ms for the 1000 x 714 web view (2.9 MB), 0.2 to 2.5 ms for the
  1000 x 144 field. A few copies of the small view took 50 to 80 ms while the UI thread was busy
  with the web page's first load.
- **Scrolling**: the image is drawn by frus in the frame that has the new scroll position, so
  it is never behind the page: 0 frames of lag, by construction. The screenshots during and
  after scrolling show the view at its place among the frus rows. After the size fix below, no
  image is copied while the page scrolls (the views do not change). The phone was hot (GPU at
  54 °C) during the measurement: the `/native` page scrolled with a median frame interval of
  48 to 71 ms (render 43 to 64 ms), against 42 ms (render 32 ms) for the plain `/licenses` page.
  Each view is one more full-screen-width texture to sample per frame.
- **Idle**: 1 CPU tick (10 ms) in 10 s with both views on screen and nothing moving, which is
  about 0.1 %. **Memory**: 178 MB PSS on `/native` against 130 MB on `/licenses`. Most of the
  48 MB is the WebView engine itself.
- **Size**: the bundled dex grew from 43 312 to 61 328 bytes. The release APK is 8 033 083 bytes.

**Two bugs found on the phone and fixed:**

- The size of a view was rounded as the difference of two rounded edges. A page scrolled by a
  fraction of a pixel gave 714 and 715 rows in turn, so every scroll frame resized the view,
  which made a new image reader and drew it again. The size is now rounded on its own (test
  `a_box_scrolled_by_a_fraction_keeps_its_size`).
- The `EditText` drew white text on its white background under the activity's dark theme. It
  now sets its own colours.

**Lints**: `cargo clippy` for the workspace's touched crates on Windows, and
`cargo clippy -p frus-shell -p frus-widgets --no-deps --target aarch64-linux-android` and
`--target wasm32-unknown-unknown`, all with `-D warnings`.

## Not done

- **Desktop, web and iOS native views.** `AndroidView` shows its fallback there. Embedding a
  desktop or browser view is later work.
- **No GPU path.** Each image is copied on the CPU and uploaded as a new texture. That is fine
  for a page or a form, and too slow for video or a constantly animating view. Importing the
  `ImageReader`'s `HardwareBuffer` into wgpu (milestone 793's external textures) would remove
  the copy and the upload.
- **Scrolling a page with views is slower** on the test phone (above). Not profiled further.
- **Gestures are all-or-nothing.** A drag that starts on a native view is the view's, so a frus
  list cannot be scrolled by dragging on a web view inside it. There is no way yet to let a
  frus gesture claim a vertical drag that starts on the view.
- **Transforms.** A view under a rotating or scaling frus transform is drawn transformed, but its
  touches and its place for the screen reader follow its untransformed box. Axis-aligned
  transforms are applied to the box.
- **The keyboard covers a native field.** frus does not scroll a focused native view above the
  keyboard as it does its own fields.
- **Hardware keys** go to frus first under `NativeActivity`. A native field gets the soft
  keyboard's text through its own `InputConnection`, but a key event the IME sends as a key
  (some keyboards' delete on an empty field) or a physical keyboard reaches frus, not the view.
- **TalkBack gestures** were not driven on the phone: adb's touches bypass touch exploration
  (see milestone 782). The tree TalkBack reads was checked with `uiautomator dump`.
- **No golden test**: on the hosts the tests run on, the widget draws its fallback, a debug-only
  box. The phone screenshots are the visual check.
- **Windows real-window run**: not done. On Windows the demo screen shows the stand-in box,
  checked only by the demo's screen tests.
