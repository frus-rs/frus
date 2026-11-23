# Platform integration: what a developer writes, and where Rust stops

frus claims one language, top to bottom. This note states precisely what that claim
covers, where it stops, and what it would take to move the boundary. It is written
for two readers: someone deciding whether to build an application on frus, and
whoever implements the iOS shell.

The short version: **an application developer writes Rust, and no Java, Kotlin,
Swift or Objective-C.** That holds today on desktop and Android, and it will hold
on iOS. It is not the same statement as "the artifact contains only Rust", and the
difference is worth understanding before it is discovered in a comment thread.

---

## What a developer writes

| | Java / Kotlin | Swift / Obj-C | Rust alone |
| --- | :---: | :---: | :---: |
| UI, layout, state, animation, gestures | — | — | ✅ |
| Desktop, in its entirety | — | — | ✅ |
| Android — APIs frus wraps | — | — | ✅ |
| Android — APIs frus does not wrap | — | — | via `frus::platform` |
| iOS — once the shell exists | — | never required | ✅ |

Two things that table does not show, and that a developer does meet:

**Configuration is not Rust.** `AndroidManifest.xml` carries permissions, the
application name, the icon, deep links — and the activity theme, which is not
cosmetic: an application must ask for a *NoActionBar* theme, because an action bar
takes 56 dp out of `content_rect` and every subsequent layout inherits the error.
iOS will bring `Info.plist` and a launch configuration. This is XML and plist, not
a programming language, but it is not Rust either, and it must be understood rather
than copied.

**Coverage, not language, is the real limit.** A developer who needs an API frus does not
wrap (biometrics, a sensor, Bluetooth) calls it from Rust through `frus::platform`, described
[below](#calling-the-platform-yourself): JNI with the threading and exception rules handled on
Android, `web-sys` on the web. That is the bridge; a convention for publishing such code as a
plugin crate is not settled yet, see [the real gap](#the-real-gap-coverage-not-language).

## What frus already wraps

These need no JNI, no platform code and no setup; each is one call, re-exported from `frus`:

- **Opening an address**: `frus::open_url("https://…")` opens the browser, and `mailto:`,
  `tel:` and `sms:` open the mail, dialer and messages applications (Windows, macOS, Linux,
  Android, the web).
- **Sharing**: `frus::share("text")`, or `frus::Share::text(..).subject(..).url(..).file(path)`
  for more, shows the system's share sheet (Android, Windows, macOS, the web). Where a desktop
  cannot show one, the text is copied to the clipboard and the call says so.
- **Receiving a share**: an application that declares an `ACTION_SEND` intent filter (commented
  out in the template's `Cargo.toml`) is listed in other applications' share sheets on Android;
  what they share — text, a subject, files copied into the cache folder — reaches
  `Application::on_share`, or the handler given to `FrusApp::on_share`, whether it starts the
  application or comes while it runs.
- **Haptic feedback**: `frus::haptics::light()` and its siblings give the system's touch
  feedback (Android, the web; the desktops answer `Unsupported`).
- **Which network**: `frus::connectivity()`, and `cx.use_connectivity()` in a component, say
  whether the device is on Wi-Fi, mobile data, a cable or nothing, and follow changes
  (Windows, macOS, Linux, Android, the web). Connected does not mean the internet answers.
- **Native views inside the page**: `AndroidView::new("frus/webview", html_or_url)` embeds an
  Android `WebView`, `"frus/edittext"` an `EditText`, and a plugin's factory any other `View`
  (a map, an ad), laid out, drawn, scrolled and covered like a frus widget (Android; a
  stand-in elsewhere). See [the guide](guide/native-views.md).
- **Notifications** (`SystemNotification`), **permissions** (`Permission`), **file dialogs**
  (`FileDialog`, desktop), **HTTP** (`fetch`).
- **Preferences**: `frus::prefs()` keeps small settings across restarts, and
  `cx.use_pref(key, default)` shows one in a component (every platform).
- **Secrets**: `frus::secure_store()` (feature `secure-store`) keeps them with the system's
  credential store (Windows, macOS, iOS, Linux, Android; `Unsupported` on the web).
- **Network images**: `Image::network(url)` keeps what it downloads in memory and in the
  application's cache folder, following the server's `Cache-Control`, `ETag` and
  `Last-Modified`, so a picture seen once is shown again with the network off;
  `frus::image_cache()` clears and sizes it (feature `net`; the web uses the browser's cache).
- **A database**: `frus::database("app.db")` (feature `sqlite`) opens SQLite in the
  application's data folder, with versioned migrations, typed rows, transactions, and every
  call off the interface's thread (Windows, macOS, Linux, Android; `Unsupported` on the web).
- **Signing in**: `frus::auth_session(AuthRequest::new(..))` (feature `auth`) runs OAuth 2 /
  OpenID Connect with PKCE through the system's browser: a loopback listener on the desktops
  and Android (in a Custom Tab there), a popup on the web.
- **The window's place**: on Windows, macOS and Linux the window comes back at the size, the
  place and the maximized state it was closed with, fitted to the screens there are at the
  next start. Nothing to write; `FrusApp::remember_window(false)` turns it off, and
  `.window_size(w, h)` is the size of the first run.
- **Playing video**: `frus::VideoPlayerController::network_url(..)` (or `asset`, `file`;
  feature `video`) plays a clip with the platform's player — `MediaPlayer` on Android, Media
  Foundation on Windows, `<video>` on the web — and `VideoPlayer`, `VideoProgressIndicator` and
  `VideoControls` show and drive it. No JNI to write. macOS, Linux and iOS answer
  `VideoError::Unsupported` for now. See [the guide](guide/video.md).
- **Video, camera and native pictures**: `frus::textures().create()` gives a texture any
  thread pushes RGBA, BGRA, NV12 or I420 frames into, and `frus::Texture::new(id)` shows it
  (every platform). On Android, `frus::TextureSurface` is a `Surface` for `MediaCodec`,
  Camera2 or a media player, from the NDK or from Java, with no JNI of the application's own
  beyond handing it over; with the `hardware-buffers` feature, on a Vulkan phone its frames
  are shown where the decoder wrote them, without a copy. See [the guide](guide/textures.md).
- **The camera and code scanning** (features `camera`, `scanner`): `frus::available_cameras()`,
  `frus::CameraController` (preview, pictures, frames, flash, zoom, focus, the other camera)
  and `frus::CameraPreview`; `frus::Scanner` reads QR codes and barcodes with
  `on_detect(|capture| ..)`. Android (Camera2), Windows (webcams) and the web. See
  [the guide](guide/camera.md).
- **Frames spent on decoration**: a spinner, a placeholder's pulse or any animation marked
  decorative (`Widget::decorative`, `AnimationHandle::decorative()`) is drawn at 30 frames a
  second when nothing else moves, not at the display's rate.
  `FrusApp::decorative_frame_rate(Some(fps))` changes the rate, `None` removes the limit.

---

## Calling the platform yourself

`frus::platform` (milestone 839) is the documented way into the system for an API frus does
not wrap. Nothing in it costs an application that does not call it: no class is loaded, no
thread started, no listener registered.

**Android.** `frus::platform::android::with_env(|env, activity| ...)` runs a closure with a
`JNIEnv` and the activity, and `jni` is re-exported as `frus::platform::android::jni`:

```rust
use frus::platform::android::{jni::objects::JValue, with_env};

let percent = with_env(|env, activity| {
    let name = env.new_string("batterymanager")?;
    let manager = env
        .call_method(activity, "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;", &[JValue::Object(&name)])?
        .l()?;
    Ok(env.call_method(&manager, "getIntProperty", "(I)I", &[JValue::Int(4)])?.i()?)
});
```

The rules, stated in its documentation:

- **Any thread.** The interface's thread is attached to the Java VM for its whole life; any
  other thread is attached for the call and detached after it, unless it was attached already.
- **The interface's thread is not Android's main thread.** An API that must run on the main
  thread (a `Toast`, a `View`) goes through `run_on_main_thread(|env, activity| ...)`, which
  posts a `Runnable` to the main looper. Neither thread may block waiting for the other.
- **Local references are freed when the closure returns** (it runs in a local frame); a Java
  object kept beyond the call is a global reference.
- **A Java exception is caught**: cleared, and returned as `PlatformError::Exception` with its
  text, where a pending one would end the process at the next JNI call.

`JavaProxy::new(env, &["android.hardware.SensorEventListener"], |env, call| ...)` implements
any Java interface in Rust; `JavaProxy::broadcast_receiver(env, |env, call| ...)` makes a
`BroadcastReceiver`. The handler runs on the thread Java calls it on, sees the method's name and
its arguments (`call.int_arg(env, 0)` unboxes a primitive), and answers a value for methods that
return one. An error or a panic in it is logged and never thrown into the system's code.
Dropping the proxy drops the handler; Java calls that still come answer the return type's
default.

**The web.** `frus::platform::web::window()` and `document()`, with `web_sys`, `js_sys`,
`wasm_bindgen` and `wasm_bindgen_futures` re-exported. An interface frus does not use itself is
turned on by the crate that needs it, with its own `web-sys` line listing that feature; Cargo
merges it into the one frus re-exports. An API the bindings lack is reached by name with
`js_sys::Reflect`.

**Windows, macOS, Linux and iOS.** No bridge, because none is needed: their interfaces are C,
COM or Objective-C, which Rust calls directly (`windows`, `objc2`, D-Bus crates), with no VM
to attach to. `frus::platform::BRIDGE` says which case a build is in.

**Back to the interface's thread.** A system callback runs on a system thread. It sends what it
learned through `frus::platform::channel()`; a task of the interface's thread (`spawn_local`,
`StateHandle::spawn`) awaits `Receiver::recv`, and the send wakes it through the same path every
task woken from another thread takes: a request for a frame (a redraw request, which winit on
Android honours where a bare wake is dropped), and the frame runs the task before it builds.

`crates/frus-platform-example` is the worked example: the battery's charge and charging state,
read with a broadcast receiver on Android and `navigator.getBattery()` on the web, in one file
that uses `frus::platform` alone, as a third-party crate would.

---

## Desktop

Nothing to qualify. Win32, X11, Wayland and AppKit are all reachable through
C-callable interfaces, and `winit` plus `wgpu` already stand in front of them.
`accesskit` and `accesskit_winit` expose the semantic tree to UIA, AT-SPI and the
macOS screen reader; the dependency block in
[`crates/frus-shell/Cargo.toml`](../crates/frus-shell/Cargo.toml) scopes them to
desktop deliberately, because neither has a UIKit backend and the Android and Web
providers are separate implementations.

---

## Android: JNI can call, but it cannot subclass

This is the only platform where a wall exists, and the shape of the wall is
specific.

JNI lets Rust **invoke** any Java API without a line of Java: look up the class,
look up the method, call it. Most of the Android SDK is reachable this way, and
everything frus needs from the platform beyond the NDK's C surface goes through it.

What JNI cannot do is **define** a class. Subclassing an abstract or concrete Java
class requires a class definition, which requires bytecode, which requires either a
compiler or a bytecode emitter. So wherever Android's design says *"extend this
class and override these methods"*, pure Rust cannot answer.

frus has met this twice so far: the keyboard, below, and the screen reader.
[`FrusTextBridge.java`](../crates/frus-shell/java/dev/frus/input/FrusTextBridge.java)
is 209 lines and exists because `NativeActivity` provides no `InputConnection`.
Without it, every IME degrades to `TYPE_NULL` — Latin keys only, no composition, no
swipe typing, no CJK — which is not a rough edge but a broken text field for most of
the world's writing systems. The bridge is a focusable 1×1 `View` laid over the
native content; it supplies a real `InputConnection` and relays each IME operation
to natives registered from
[`android_ime.rs`](../crates/frus-shell/src/android_ime.rs) through
`RegisterNatives`. See [milestone 81](milestone-81.md) for the full reasoning.

The packaging decision matters as much as the shim: it is compiled to a dex by
[`scripts/build-input-dex.sh`](../scripts/build-input-dex.sh) and loaded at runtime
with `InMemoryDexClassLoader`. An application's build never learns that Java was
involved — no Gradle, no source set, no toolchain in the dependency graph. `cargo
apk run` remains the whole story.

### The escape hatch, and its limit

`java.lang.reflect.Proxy` is itself invocable over JNI, and it implements an
**interface** dynamically at runtime. Every listener-shaped API — callbacks,
observers, `OnClickListener`-style contracts — is therefore reachable with no `.java`
file at all. It does not extend classes, so it does not help with `View`,
`Service`, `BroadcastReceiver` or `AccessibilityNodeProvider`.

That is the rule to carry: **interfaces are free, classes cost a dex.** Since milestone 839
the trick is built, as `frus::platform::android::JavaProxy` (below): the one
`InvocationHandler` every proxy needs lives in the dex frus already carries, so a crate that
implements a listener ships no Java and no dex. `BroadcastReceiver`, the class most callbacks
on Android arrive through, is answered the same way by one forwarding class in that dex.

### Screen readers on Android

TalkBack reads a frus window through `FrusAccessibility`, the second class in the same dex
([milestone 782](milestone-782.md)): one view, as large as the content, whose
`AccessibilityNodeProvider` shows one virtual view per semantic node of the frame, with
its role, bounds, words, states and actions. Nothing is asked of the application: the
semantics its widgets already declare are what TalkBack reads, swiping moves through
them, a double tap presses, and fields, sliders and scrolling areas answer TalkBack's
editing, adjusting and scrolling. Not answered yet: exploring by touch, and the outline
TalkBack draws around the node it is on (see the milestone's "Not done").

### Native views inside the page

`FrusPlatformViews`, in the same dex ([milestone 794](milestone-794.md)), puts real Android
views in a frus page. Under `NativeActivity` the activity's views are laid out but never
drawn, because the native side owns the window's surface. So each embedded view lives in the
activity's hierarchy, where focus, the keyboard and TalkBack treat it as any other view, and
a host view draws it into an image of its own whenever something in it changes. frus copies
that image into its frame where the widget was laid out, and forwards the touches that land
there. A plugin writes the view's factory in Java against `dev.frus.platform.PlatformViewFactory`
and registers it with `frus::register_android_view_factory`: the class is a plugin's, so the
application still writes no Java.

---

## iOS: the asymmetry runs the other way

The common assumption is that Apple's platform will be the harder one to reach from
Rust. It is the easier one, and the reason is the Objective-C runtime.

That runtime is dynamic and C-callable. `objc_allocateClassPair` and
`class_addMethod` create classes **at execution time**, from Rust, with no compiler
and no bytecode step — the very thing the JVM refuses. The `objc2` family of crates
wraps this, and `winit` already constructs its `UIView` and `UIApplicationDelegate`
this way on iOS.

So the target is genuinely zero lines of Swift and zero lines of Objective-C, for
the framework as well as for the application. UIKit, Metal, CoreText,
`UIAccessibility` and `UITextInput` are all Objective-C or C, therefore all
reachable. Nothing in the iOS integration list from
[milestone 276](milestone-276.md) — lifecycle, safe-area insets, the soft keyboard,
`os_log`, UIKit accessibility, `.ipa` packaging — requires a Swift source file.

Two honest caveats. Building and signing needs a Mac with the Xcode toolchain: that
is tooling, not language, but it is a real prerequisite. And a handful of newer
Apple frameworks are Swift-only; none of them is on the path to a working shell,
but that could change if the platform surface grows.

---

## Web

Generated JavaScript glue is unavoidable, because wasm cannot touch the DOM
directly — every call crosses an import table that `wasm-bindgen` writes. No
developer writes that JavaScript, and no developer reads it, but it is present in
the artifact and `index.html` is a real file. When web accessibility is wired up it
will mean creating DOM nodes through `web-sys`, which is Rust calling generated
bindings: more glue, still no hand-written JavaScript.

---

## The real gap: coverage, not language

The honest summary of everything above is that the language question is nearly
settled and the *coverage* question is wide open.

An application needing biometrics gets no ready-made API from frus today (video plays
through `VideoPlayerController`, purchases go through `Purchases`, and a camera has
`CameraController`). Its author calls the platform through `frus::platform`, above: the
threading, the references, the exceptions and the callbacks are handled, and no Java is
written, but the calls themselves are still written by hand, in every application that needs
the same thing.

What is missing is the convention that lets that code be published once: how a crate declares
the permissions and manifest entries it needs, the Java or Swift it might still carry, and how
`frus build` picks them up, so that an application gains a capability by adding one dependency.
That needs a discussion first (#216); the bridge it would stand on exists.

---

## What this means for the project's claims

- **"No embedded VM"** — structural. Rust compiles ahead of time to native code and
  to wasm, and [`reload.rs`](../crates/frus-shell/src/reload.rs) achieves
  state-preserving hot reload by serialising one struct and relaunching the binary,
  which is the usual reason a framework ends up embedding an interpreter. On
  Android the process hosts ART because `NativeActivity` is a Java class; that is
  the platform's runtime, not one frus ships.
- **"No second language for app logic"** — true, and the qualifier is load-bearing.
  The framework carries its Java in one prebuilt dex (the keyboard,
  the screen reader, the forwarding class behind `JavaProxy`); an application contains none.
- **"No platform channel between your code and the pixels"** — true, and scoped on
  purpose. The render path is application → widget tree → scene → `wgpu` → native
  API, with no serialisation boundary. The JNI bridge sits on the *input* path.
  Restated as "no platform channel", the claim is false.
