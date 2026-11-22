# Milestone 839 — Calling the platform yourself: `frus::platform`

Part of #216.

## Objective

When an application needs a platform API that frus does not wrap (a sensor, Bluetooth,
biometrics), it had to write raw JNI by hand: find the Java VM, attach the thread, keep track of
local references, clear exceptions before they take the process down, and, for any callback,
ship a Java class and a dex of its own, since JNI cannot define a class. This milestone builds
the bridge #216 asks for: a public, documented way into the platform on every target, and
callbacks from Java into Rust with no dex of the caller's own. The convention for publishing
such code as a plugin crate (manifest entries, permissions, Java or Swift sources picked up by
the build) needs a discussion first, as the issue says, and is not part of it.

## What was built

### `frus::platform` (every target)

- `channel()` → `(Sender<T>, Receiver<T>)`. The sender is `Send` and goes into a system
  callback; the receiver is awaited by a task of the interface's thread (`spawn_local`,
  `StateHandle::spawn`). A send queues the value and wakes that task, and the task's waker is
  the one every local task has: it asks the window for a frame (`Window::request_redraw`,
  which winit on Android honours where a bare looper wake is dropped), and the frame runs the
  task before it builds. Nothing runs, and no frame is asked for, while nothing is sent.
- `PlatformError` (`Unavailable`, `Exception`, `Binding`) and `BRIDGE` (`Android`, `Web`,
  `Native`), so code can tell which case a build is in.

### `frus::platform::android`

- `with_env(|env, activity| ...)`: a `JNIEnv` and the activity, on any thread. The VM and the
  activity come from the native activity's recorded context, so nothing is set up for it. A
  thread that is not attached is attached for the call and detached when it returns
  (`attach_current_thread`, which leaves an already attached thread, such as the interface's,
  as it was). The closure runs in a JNI local frame, so its local references are freed when it
  returns, which matters on the interface's thread: it never returns to Java, so locals made
  there would otherwise pile up until the reference table overflows. A Java exception left
  pending is cleared and returned as `PlatformError::Exception` with its `toString()`.
- `run_on_main_thread(|env, activity| ...)`: posts work to Android's main looper, through a
  `Runnable` implemented with `JavaProxy`. frus runs on a thread of its own, not on Android's
  main thread, and several APIs (toasts, views) must be called there. The proxy is kept until
  it has run, and kept *before* it is posted, since the main thread may run it before `post`
  returns.
- `JavaProxy::new(env, &["some.Interface"], |env, call| ...)`: any Java interface implemented
  in Rust with `java.lang.reflect.Proxy`. The interfaces are looked up with `Class.forName`
  through the activity's class loader, so the platform's and the package's interfaces are both
  found from any thread (JNI's `FindClass` on a natively attached thread sees only the system
  loader).
- `JavaProxy::broadcast_receiver(env, |env, call| ...)`: a `BroadcastReceiver`, the class
  most of Android's system events arrive through, which a proxy cannot extend.
- `ProxyCall`: the method's name, the object called, the arguments, and `int_arg`,
  `long_arg`, `float_arg`, `double_arg`, `bool_arg`, `string_arg` to unbox them.
- `jni` is re-exported, so a crate calling Java needs no `jni` version of its own.

### `frus::platform::web`

`window()` and `document()`, and `web_sys`, `js_sys`, `wasm_bindgen` and
`wasm_bindgen_futures` re-exported. A `web-sys` interface frus does not use is turned on by the
crate that needs it, in its own `web-sys` line, and Cargo merges the features into the one
`web-sys` of the build. `PlatformError` converts from a thrown `JsValue`.

### Windows, macOS, Linux and iOS

No module: their system interfaces are C, COM or Objective-C, which Rust calls directly, with
no VM to attach to and no thread rule of the bridge's own. `BRIDGE` is `Native` there, the
channel works the same, and the rustdoc of `frus::platform` says so. Every target compiles.

## Design

### One dex class, for every callback

JNI calls Java but cannot define a class; `Proxy` defines an implementation of any interface
at run time, but needs an `InvocationHandler`, which is a class. So exactly one class is
needed, and it is generic: `dev.frus.input.FrusProxy` (in `crates/frus-shell/java`), compiled
into the dex frus already carries (`assets/frus_input.dex`, built by
`scripts/build-input-dex.sh`; 2.2 KB more). It forwards every call to a single native,
`nativeInvoke(long id, Object proxy, String method, Object[] args)`, with the number the Rust
side keeps the handler under. Its nested `Receiver` does the same for broadcasts.

- **Handlers live in a table, not behind a pointer.** Java holds a number, and Rust looks it
  up under a lock and clones the handler's `Arc` before calling it. Dropping a `JavaProxy`
  removes the entry; a call that arrives after (the listener was not unregistered, or a call
  was already on its way) finds nothing and answers the default. A raw pointer in the Java
  object would have been a use-after-free in exactly that race.
- **Nothing is thrown into the system's code.** A handler's error or panic is logged, the
  exception cleared, and the call answers `null`, which `FrusProxy` turns into the return
  type's default (`false`, `0`), since a proxy returning `null` for a primitive throws a
  `NullPointerException` in its caller. `equals`, `hashCode` and `toString` are answered by
  identity in Java, so a proxy works in a listener list or as a key.
- **Loaded on first use.** At start-up the shell only keeps a global reference to the dex's
  class loader (it already makes it for the keyboard bridge); `FrusProxy` is loaded and its
  native registered by the first `JavaProxy`. An application that never makes one pays one
  global reference.

### Callbacks reach the interface's thread through a channel, not a hook

A handler runs on the thread Java calls it on, and must read its Java arguments there, while
they are valid local references; what it learned then crosses to the interface's thread as a
Rust value. The crossing is a channel awaited by an ordinary local task rather than a callback
registry of its own, so it needs no new hook in the frame: the local tasks already run at the
start of each frame, and their wakers already ask the window for one.

### Threading rules, stated

The rules are in the rustdoc of `frus::platform::android` and in
`docs/platform-integration.md`: any thread may call `with_env`; the interface's thread is not
Android's main thread; neither may block waiting for the other; local references end with the
call; exceptions are caught.

## The example

`crates/frus-platform-example` shows the battery's charge and whether it is charging. Its
`battery` module uses `frus::platform` alone, as a third-party crate would: a
`BroadcastReceiver` for `ACTION_BATTERY_CHANGED` on Android (sticky, so it hears the current
state at once, then each change), `navigator.getBattery()` on the web (reached by name with
`js_sys::Reflect`, since `web-sys` has no binding for it), and `Unavailable` elsewhere. A
button shows a toast through `run_on_main_thread`, which is the `Proxy` path on an interface
(`Runnable`).

## How it was tested

- **Unit tests.** `platform::channel`: a value sent from another thread wakes the receiver; a
  send wakes a task of the interface's thread; a send with no receiver says so; the last sender
  dropped ends the receiver. `platform`: a desktop test build reports no bridge, and an error
  says what failed. The example's `battery` module: the extras of a battery intent read as a
  reading, a reading describes itself, and a desktop build says the battery is unavailable.
  Run with `cargo test -p frus-shell --lib platform` (22 tests matching `platform`, all green)
  and `cargo test -p frus-platform-example` (3, green); `frus-widgets`'s `core_exports` guard
  is green too.
- **Every target compiles and lints clean.** `cargo check` of `frus` and the example for the
  host (all targets), `aarch64-linux-android`, `wasm32-unknown-unknown` and
  `aarch64-apple-ios`; `cargo clippy -D warnings` for the workspace (all targets), and for
  `frus-shell` with the example on Android (`--no-deps`, since the JNI code is behind
  `cfg(android)` and linted by nothing else) and on wasm; `cargo doc` of `frus-shell` and
  `frus` with `-D warnings`; `cargo fmt --all`.
- **Not run on a device.** No phone was connected when this was checked, so the JNI half
  (`with_env`, `JavaProxy`, `broadcast_receiver`, `run_on_main_thread`) and the rebuilt dex
  were compiled and linted but not run. The web half was compiled, not run in a browser.

## Not done

- **The plugin crate convention** (#216's third point): how a crate declares permissions,
  manifest entries and Java or Swift sources, and how `frus build` and the iOS build pick them
  up. It needs the discussion the issue asks for. Until then a crate using `frus::platform`
  documents the manifest lines an application must add itself.
- **A class that is not `BroadcastReceiver`** still needs a dex: a `Service`, a `View`
  subclass, an `AccessibilityNodeProvider`. Interfaces and broadcast receivers cover most
  callbacks; the rest is the plugin convention's business.
- **iOS** has no helper (no `with_ui_thread`, no `objc2` re-export). Rust calls UIKit through
  `objc2` directly; whether frus should re-export it is part of the plugin discussion.
- **Arrays and return values** are left to `jni`: `ProxyCall` unboxes primitives and reads
  strings, but a handler that returns a primitive boxes it itself.
- **No run on a phone or in a browser.** The Android bridge and the example have not been run
  on a device (none was connected), and the web half has not been run in a browser. Both are
  owed before the bridge is relied on: the battery reading and its changes, the toast, a
  proxy called after its `JavaProxy` is dropped, and the reference table staying flat over
  many calls on the interface's thread.
