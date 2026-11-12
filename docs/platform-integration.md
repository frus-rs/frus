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
| Android — APIs frus does not wrap | sometimes | — | via `jni` |
| iOS — once the shell exists | — | never required | ✅ |

Two things that table does not show, and that a developer does meet:

**Configuration is not Rust.** `AndroidManifest.xml` carries permissions, the
application name, the icon, deep links — and the activity theme, which is not
cosmetic: an application must ask for a *NoActionBar* theme, because an action bar
takes 56 dp out of `content_rect` and every subsequent layout inherits the error.
iOS will bring `Info.plist` and a launch configuration. This is XML and plist, not
a programming language, but it is not Rust either, and it must be understood rather
than copied.

**Coverage, not language, is the real limit.** A developer who needs the camera,
biometrics or in-app purchases today writes the JNI themselves. That is possible and unpleasant. It is discussed under
[the real gap](#the-real-gap-coverage-not-language) below.

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
- **Video, camera and native pictures**: `frus::textures().create()` gives a texture any
  thread pushes RGBA, BGRA, NV12 or I420 frames into, and `frus::Texture::new(id)` shows it
  (every platform). On Android, `frus::TextureSurface` is a `Surface` for `MediaCodec`,
  Camera2 or a media player, from the NDK or from Java, with no JNI of the application's own
  beyond handing it over; with the `hardware-buffers` feature, on a Vulkan phone its frames
  are shown where the decoder wrote them, without a copy. See [the guide](guide/textures.md).
- **Frames spent on decoration**: a spinner, a placeholder's pulse or any animation marked
  decorative (`Widget::decorative`, `AnimationHandle::decorative()`) is drawn at 30 frames a
  second when nothing else moves, not at the display's rate.
  `FrusApp::decorative_frame_rate(Some(fps))` changes the rate, `None` removes the limit.

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

That is the rule to carry: **interfaces are free, classes cost a dex.**

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

An application needing biometrics or in-app purchases gets no help from frus today (a
camera or a video decoder at least has a surface to draw into, `TextureSurface`). Its author
writes JNI by hand — reachable, verbose, easy to get wrong, and repeated in every application
that needs the same thing. That is the actual obstacle for someone shipping a real
application, and it is not solved by adding another widget.

What it argues for is a platform-services layer: a small set of Rust APIs with
per-target implementations behind them, following the same rule as the rest of the
architecture — the portable crates state the intent, `frus-shell` knows the
platform. The shape is not settled and no milestone claims it. It is written down
here so the next person to feel the friction knows it was seen, not missed.

---

## What this means for the project's claims

- **"No embedded VM"** — structural. Rust compiles ahead of time to native code and
  to wasm, and [`reload.rs`](../crates/frus-shell/src/reload.rs) achieves
  state-preserving hot reload by serialising one struct and relaunching the binary,
  which is the usual reason a framework ends up embedding an interpreter. On
  Android the process hosts ART because `NativeActivity` is a Java class; that is
  the platform's runtime, not one frus ships.
- **"No second language for app logic"** — true, and the qualifier is load-bearing.
  The framework contains one Java file and will likely contain a second; an
  application contains none.
- **"No platform channel between your code and the pixels"** — true, and scoped on
  purpose. The render path is application → widget tree → scene → `wgpu` → native
  API, with no serialisation boundary. The JNI bridge sits on the *input* path.
  Restated as "no platform channel", the claim is false.
