# Milestone 801 — frus's own Android activity: links, shares, notifications and sign-ins reach the running application

## Objective

On Android, a frus application must come back to **the same running activity** when something
opens it again: a link (an app link or the application's own scheme), a share from another
application, a notification's tap, the redirect at the end of a sign-in.

Milestone 795 measured why it did not. Chrome hands a custom-scheme redirect back with
`FLAG_ACTIVITY_NEW_TASK | FLAG_ACTIVITY_CLEAR_TOP` (`flg=0x14000000`), from a Custom Tab and
from an ordinary tab alike. With the native activity's standard launch mode, `CLEAR_TOP`
**finishes the running activity and creates another**; a frus application cannot survive that in
its process (one event loop per process, milestones 755 and 763), so it started over and the
sign-in was lost. `frus::auth_session` therefore answered `Unsupported` for custom-scheme
redirects on Android, and a link opened while the application ran only worked through milestone
763's detour: a second activity starts, reads its link, hands it to the first and finishes.

The fix asked for: an activity class of frus's own, with a launch mode that keeps one instance,
whose `onNewIntent` forwards the intent to Rust — packaged by the build with no step for the
developer — then custom-scheme redirects enabled, and links, shares and notification taps
delivered through it.

## Design

### The activity (`crates/frus-cli/java/dev/frus/FrusActivity.java`)

```java
public class FrusActivity extends NativeActivity {
    private native void nativeNewIntent(Intent intent);

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        try {
            nativeNewIntent(intent);
        } catch (UnsatisfiedLinkError notYet) {
            // The native side has not started: it reads getIntent() when it does.
        }
    }
}
```

Everything else is the system's native activity: the library is loaded and run exactly as before,
the `android.app.func_name` entry point of milestone 762 still works, nothing depends on frus's
other Java (the bridge dex loaded later from `assets`).

- **Declared `launchMode="singleTop"`.** `CLEAR_TOP` then removes what is above the running
  activity (the sign-in's Custom Tab) and delivers the intent to it instead of recreating it; an
  intent with `NEW_TASK` finds the running task and, the activity being on top, hands it over as
  `onNewIntent` too. `singleTask` was weighed: it also routes an intent from another
  application's task to the single instance, but tapping the launcher icon then clears whatever
  the application started in its task (a sign-in tab, a file picker). `singleTop` is the common
  choice for a single-activity application, and the rare intent that still asks for a second
  instance (one from another application's task with none of the flags that find the running
  one) keeps milestone 763's hand-over. A manifest that chose a launch mode keeps it.
- **The native method is registered by Rust** (`RegisterNatives` on the activity's runtime
  class, then its superclasses up to the native activity), not found by symbol name: the native
  activity loads the library itself, outside the class loader's library list. An intent that
  arrives before registration is not lost: `setIntent` makes it the activity's own, and the
  native side reads `getIntent()` at its first frame.

### Packaging (`crates/frus-cli`)

The class must be in the package's own `classes.dex`: the system instantiates the activity
before any native code runs, so the bridge dex frus loads at startup cannot hold it. cargo-apk
packages no code (`android:hasCode="false"`) and names `android.app.NativeActivity`.

- The class is compiled once (`scripts/build-activity-dex.sh`, `javac --release 8` + `d8
  --min-api 24`) into `crates/frus-cli/assets/frus_activity.dex`, **892 bytes**, checked in and
  embedded in the CLI with `include_bytes!`, so building an application never needs `javac`.
- After cargo-apk, `app_links::finish_package` (milestone 791's step that adds `autoVerify`,
  generalised) rewrites the manifest — `android:name="dev.frus.FrusActivity"`,
  `launchMode="singleTop"` unless one is set, `android:hasCode="true"` — and makes the package
  again with the SDK's `aapt package`, the libraries and `classes.dex`, `zipalign`, `apksigner`
  with the key cargo-apk was handed (`android_pack::signing_key`, so `frus run --release` without
  a release key still signs with the debug key). The rewritten manifest and the dex are left
  beside cargo-apk's package, so a flavor's APK, `--split-per-abi` and the App Bundle
  (`dex/classes.dex`) carry them too. A failure here fails the build: without the activity a
  sign-in's redirect would end the application.
- `frus run` starts `<package>/dev.frus.FrusActivity` (it named the native activity before).
- The hot-reload path (`frus run --hot`, the engine's Gradle project) writes its `MainActivity.kt`
  with the same `onNewIntent` and `external fun nativeNewIntent`, and declares it `singleTop`.
- **Opting out:** an application whose manifest names an activity
  (`[package.metadata.android.application.activity] name = "…"`) is left exactly as cargo-apk
  made it, `android.app.NativeActivity` included. A package made by hand with `cargo apk` runs in
  the native activity; the shell sees that (no `nativeNewIntent` to register) and behaves as
  before: links through a second activity, custom-scheme sign-ins `Unsupported` with a message
  that says why.
- Existing applications need no change: nothing in the template named the activity, so the next
  `frus run` or `frus build` packages frus's.

### The shell (`android_link.rs`, `received_share.rs`, `auth.rs`)

- `android_link::listen` registers the native method at startup; `native_new_intent` runs on the
  Java main thread and reads the intent: `ACTION_SEND` / `ACTION_SEND_MULTIPLE` become a share,
  anything with data a link. A link joins the queue milestone 763's hand-over already fills
  (`android_lifecycle::queue_link`), so the next frame gives it to a waiting sign-in
  (`auth::offer`) or opens it as a location — the same path for app links (milestone 791),
  custom schemes and redirects. The loop is woken with `Window::request_redraw` (winit on
  Android drops a bare wake).
- A launcher intent with no data — a notification's tap — carries nothing to deliver; what
  matters is that it no longer recreates anything, and the notification's own answer
  (`NotificationResponse::Clicked`) is read as before.
- **Receiving shares is new:** `frus::SharedContent { text, subject, mime_type, files }`, handed
  to `Application::on_share` (default: ignored) or to the handler given `FrusApp::on_share`.
  Files arrive as `content://` addresses only the receiving activity may read; each is copied
  (name from `_display_name`, made safe) into `<cache>/shared/` on a thread of its own, so a
  large video does not stall the main thread, and the share is delivered once the copies are
  done. A share that starts the application is delivered after the first frame. The
  application declares what it takes with an `ACTION_SEND` intent filter (commented in the
  template's `Cargo.toml`, used by `frus-demo` and `frus-data-example`).
- **Sign-in:** on Android a custom-scheme redirect now waits for its link like a desktop's,
  with the page in a Custom Tab (`android_share::open_auth_tab`) over the application; the
  browser's `CLEAR_TOP` closes the tab and hands the link to the running activity. Without
  frus's activity the answer is still `AuthError::Unsupported`, now saying how to get it.
  Loopback redirects are unchanged.

### Process death

If Android ends the process while the browser is in front, the sign-in is lost: the PKCE
verifier lives only in the waiting future, on purpose, and the application's code that awaited
it is gone too. Writing the verifier to storage was considered "cheap" only on the surface:
restoring it would need an API for the application to resume a sign-in it no longer awaits, and a
verifier at rest weakens what PKCE protects. Instead the behaviour is documented
(`docs/guide/data.md`): the browser's link starts the application afresh, at the redirect's
location (`/callback?code=…` for `com.example.app:/callback`), where the application should
offer to sign in again.

## How it was tested

### Without a device

- `android_activity` (frus-cli): the native activity becomes frus's, with `singleTop` and
  `hasCode="true"`, the rest of the manifest kept; a launch mode the manifest chose is kept; an
  activity of the application's own is left alone; patching twice changes nothing; the embedded
  dex holds `Ldev/frus/FrusActivity;` and `nativeNewIntent`.
- `hot` (frus-cli): the engine's project declares `singleTop` and its `MainActivity.kt` forwards
  `onNewIntent`.
- `received_share` (frus-shell): shares are taken once and in order, empty ones dropped; shared
  file names cannot leave their folder (`../../etc/passwd` → `passwd`, `..` → `shared`).
- `create` (frus-cli): the template's commented share filter is not taken for a declared one.
- Clippy with `-D warnings`: host (frus-shell with `net,json,sqlite,auth,secure-store,
  notifications`, frus-cli, frus-demo, all targets) and `aarch64-linux-android --no-deps`.

### On the phone (Huawei STK-L21, Android 10)

`frus-data-example`, built with the worktree's own `frus build apk` (release, debug key). The
manifest in the package names `dev.frus.FrusActivity` with `launchMode="singleTop"` and
`hasCode="true"`; `classes.dex` is the 892-byte activity. The launcher noticed the change
("activity changed. the old: android.app.NativeActivity, the new: …FrusActivity").

- **Cold start**: `Displayed com.frus.data/dev.frus.FrusActivity: +999ms`, first frame at 514 ms
  as before; `dev.frus.FrusActivity hands new intents to the running application`.
- **A share while running** (`am start -a android.intent.action.SEND -t text/plain --es
  android.intent.extra.TEXT …`): "intent has been delivered to currently running top-most
  instance", `the running activity was handed a share (text/plain, 0 file(s), text)`, the text
  became the note being written. Same pid (2214).
- **Deep links while running** (`am start -a android.intent.action.VIEW -d
  com.frus.data://signin`, then `…://notes`): delivered to the running instance, `location
  /signin`, `location /notes`; same pid, and the note being written was still there after the
  round trip (screenshot `p3-notes`).
- **Sign-in with a custom scheme** against the mock server through `adb reverse tcp:8795`: the
  Custom Tab opened over the application, **Approve** sent Chrome's link back with
  `flg=0x14000000` (`NEW_TASK | CLEAR_TOP`, as in milestone 795) — this time to the running
  activity: `the running activity was handed a link (com.frus.data:/callback)`, then `auth:
  Signed in: access token …, refresh token kept, ID token checked`. Same pid; the server counted
  one authorization and one token exchange, no PKCE failure.
- **A notification's tap**: **Notify** posted one, the application went to the background
  (Home), and a tap in the shade started the launcher's intent with
  `NEW_TASK | SINGLE_TOP` — `result: 2`, delivered to the running activity, no new process (pid
  18148 before and after) — and the application read `Ok(Clicked)`. The example had no launcher
  icon, and Android refuses a notification whose small icon is 0 (`Java exception was thrown` on
  the first try): it now ships the frus logo as `frus create` does.
- **Idle after all of it**: 0.0 % CPU (`top`), 125 MB total PSS.
- **Size**: the package frus makes again is 9,138,662 bytes against cargo-apk's 9,129,282
  (+9.4 KB: the dex and the signature blocks). The first device build stored the library
  uncompressed, as milestone 791's re-packaging always had (18,997,734 bytes: every application
  with an `https` link paid that); the libraries are now compressed in a release build as
  cargo-apk compresses them, and stored in a debug build, as cargo-apk stores them. Nothing runs
  for the feature until an intent arrives: no thread, no polling.

## Not done

- **Shared files on the phone.** The text share was verified on the device; a share with files
  (`EXTRA_STREAM`, copied into `cache/shared/`) was not driven there (it needs another
  application to grant the `content://` read), only written and linted for Android.
- **A link while the sign-in tab is open**, from another application's task without `CLEAR_TOP`:
  `singleTop` sees the tab on top and starts a second activity, which milestone 763's hand-over
  answers. Not seen in practice; `singleTask` would remove it at the cost described above.
- **Process death during a sign-in** is documented, not recovered (see Design).
- **The hot-reload path** (`frus run --hot`, the engine's Gradle project with `MainActivity.kt`)
  is covered by a unit test of the files it writes, not run on the phone in this milestone.
- **iOS** has no activity; nothing changes there. The web and the desktops are untouched apart
  from `on_share`, which nothing calls there.
- **A notification's own payload**: a tap still reaches the application only as
  `NotificationResponse::Clicked`; carrying a link in the notification is left for later.
