# Milestone 777 — Haptic feedback and connectivity

## Objective

Two platform services every phone application reaches for, which a frus application could only
get by writing JNI by hand:

1. **Haptic feedback**: the short vibrations that answer a gesture or mark an outcome.
2. **Connectivity**: which network the device is on, and being told when that changes.

Both as one function each, re-exported from `frus`, working with no setup.

## The API

```rust
// Haptics: each returns Result<(), frus::HapticsError> at once.
frus::haptics::light();
frus::haptics::medium();
frus::haptics::heavy();
frus::haptics::selection();
frus::haptics::success();
frus::haptics::warning();
frus::haptics::error();
frus::haptics::vibrate(std::time::Duration::from_millis(400));

// Connectivity.
let net: frus::Connectivity = frus::connectivity(); // None | Wifi | Cellular | Ethernet | Other | Unknown
let net = cx.use_connectivity();                    // in a component: rebuilt on each change
net.is_connected();

// The widgets' own feedback, off for an application.
Theme::default().with_feedback(false);
```

`HapticsError` is `Unsupported` (nothing to vibrate with) or `Failed(reason)` (no permission, no
vibrator, the browser refused).

## Design

### Haptics

The kinds and their Android effects are the leading toolkit's: light is
`HapticFeedbackConstants.VIRTUAL_KEY`, medium `KEYBOARD_TAP`, heavy `CONTEXT_CLICK`, selection
`CLOCK_TICK`, success `CONFIRM`, warning `KEYBOARD_TAP`, error `REJECT`. `CONFIRM` and `REJECT`
arrived in Android 11; the toolkit does nothing below it, frus uses `CONTEXT_CLICK` and
`LONG_PRESS` there instead of nothing.

- **Android**: `View.performHapticFeedback` on the window's decor view, posted to the Java UI
  thread (`FrusHaptics` in the bundled dex). It needs no permission and follows each device's
  tuning. When it performs nothing (it answers `false`), the vibrator stands in for as long as
  the web would vibrate for that kind (10 to 50 ms); that needs `android.permission.VIBRATE`,
  which is checked first, so a missing one is an error value, never a `SecurityException`.
  **The person's setting** (`Settings.System.HAPTIC_FEEDBACK_ENABLED`) is read before
  anything else. `performHapticFeedback` obeys it by itself, but the vibrator does not, and
  must not get around it: with touch feedback off the call does nothing and answers `Ok`.
  `vibrate(duration)` is an alert, not touch feedback, and goes straight to the vibrator,
  whose own settings apply.
- **Web**: `navigator.vibrate` for the toolkit's web durations (light and selection 10 ms,
  medium 20, heavy 30, long press 50). Its `false` (Chrome before the person has touched the
  page) is `Failed`; a browser without it is `Unsupported`.
- **Desktops**: `Unsupported`. There is no haptic hardware to drive; the value says so instead
  of pretending, and an application ignores it as it ignores any feedback result.
- **iOS**: `Unsupported` for now.

**The widgets' own feedback.** No frus widget gave any before. The leading toolkit's defaults
were checked one by one:

- A long press that selects text (`TextSelectionGestureDetectorBuilder.onSingleLongTapStart`)
  calls `Feedback.forLongPress`: Android's `LONG_PRESS` effect, a heavy impact on iOS, nothing
  on the desktops. frus now does the same where a hold selects a word: in a text field, and in
  a selection area. It is the system's platform that decides, not the theme's (a desktop
  pretending to be a phone does not vibrate), as the toolkit does.
- Switches and checkboxes: on Android the toolkit gives **no** haptic feedback for them (a tap
  plays the click sound through `Feedback.forTap`; the toggles do not call it at all). Only its
  iOS-style switch gives a light impact on iOS. frus therefore adds none on Android, which would
  have been a departure from what the platform's applications feel like. iOS haptics are not
  wired yet, so the iOS switch's impact waits for them.

`Theme::enable_feedback` (default `true`, `Theme::with_feedback`) turns the widgets' feedback
off for the application. It is read from the application's theme when the hold is decided.

### Connectivity

The value lives in `frus-widgets` (`frus_widgets::connectivity`), where the build context can
read it, in an atomic shared by every thread, because the systems report from threads of their
own. The platform layer installs the function that starts watching; the first read calls it, so
an application that never asks pays nothing. A report that changes the value sets a flag and
asks for a frame (the same wake the async tasks use); the frame sees the flag and builds the
interface again. So `use_connectivity()` needs no hook slot: every component that reads the
value reads the new one, and an `Application`'s `view` that calls `frus::connectivity()` follows
it the same way.

- **Android**: `ConnectivityManager.registerDefaultNetworkCallback` (`FrusConnectivity` in the
  dex: a `NetworkCallback` is a class, which JNI cannot subclass). The capabilities' transport
  names the kind (Wi-Fi first, then cellular, ethernet, anything else is `Other`); losing the
  default network is `None`. Needs `ACCESS_NETWORK_STATE`; without it the value stays `Unknown`
  and the log says what to declare.
- **Windows**: `NetworkInformation.GetInternetConnectionProfile`, read again on each
  `NetworkStatusChanged`. No profile, or a connectivity level of `None`, is `None`; a WLAN or
  WWAN profile is Wi-Fi or cellular; otherwise the adapter's IANA type (6 ethernet, 71 Wi-Fi,
  243/244 mobile broadband).
- **macOS**: `NWPathMonitor` through the Network framework's C API, its update handler a
  block. A satisfied path's interface type names the kind.
- **Linux**: the interface of the default route (`/proc/net/route`, then `/proc/net/ipv6_route`)
  and what `/sys/class/net/<if>` says of it (`wireless`, `DEVTYPE=wlan`/`wwan`, ARP type 1),
  looked at every two seconds on a thread. NetworkManager over D-Bus was weighed and set aside:
  not every machine runs it, and reading the kernel's tables needs no service and no
  dependency.
- **Web**: `navigator.onLine` with the `online`/`offline` events, and `navigator.connection`'s
  type and `change` event where the browser has them (Chrome on Android). A browser that only
  says "online" gives `Other`.
- **iOS**: `Unknown`.

**Connected is not online**, and every doc that names the API says so: a captive portal or a
router with no line behind it leave the device on a network and every request failing.

### Permissions, with no setup

`VIBRATE` and `ACCESS_NETWORK_STATE` are normal permissions: granted at install, never asked.
`frus build` and `frus run` now add both to an application's `Cargo.toml` (cargo-apk's
`[[package.metadata.android.uses_permission]]`) like the declared ones, the `frus create`
template carries them, and the hot-reload engine's manifest names them. The demo, built with
cargo-apk directly, declares them by hand.

### Dependencies

The `windows` crate (already in the tree, through the tray and the notifications) is now always
a Windows dependency of `frus-shell`, with its `Foundation` and `Networking_Connectivity`
features; it was optional, behind `taskbar`. Nothing new is downloaded.

## How it was tested

Unit tests:

- `haptics`: each kind's Android effect, before and after Android 11; the web durations; the
  long press felt on Android and iOS only; the theme's switch; the desktops answering
  `Unsupported`.
- Through the shell (`Driver`): a hold that selects a word in a field gives the long-press
  feedback on Android and a tap does not; the same hold gives none on a desktop system, nor
  under a theme with feedback off; a hold that selects words in a selection area is felt, a
  hold on a button is not.
- `connectivity` (widgets): the value survives its code; a change is reported once, a repeat is
  not.
- `connectivity` (shell): the default route in IPv4 and IPv6 tables, interfaces classified
  from what the kernel says, the web's online/type pair, Windows' IANA types.
- `frus-cli`: an application that declares nothing still gets `VIBRATE` and
  `ACCESS_NETWORK_STATE`, once; the hot-reload manifest names them.

On machines:

- **Windows** (this machine, on Wi-Fi): `cargo test -p frus-shell --lib reads_this_machine --
  --ignored --nocapture` installs the watch and reads through `frus_widgets::connectivity()`:
  `connectivity now: Wifi`. Wi-Fi was **not** turned off to see a change: the machine is shared
  with other running work that needs the network, so the change path was left to the unit
  tests and the phone.
- **Linux** (WSL2, Ubuntu 24.04): the same test, built and run there, reads `connectivity now:
  Ethernet` — its `/proc/net/route` names `eth0` with a default route, its `uevent` has no
  `DEVTYPE` and its ARP type is 1. No change was provoked.
- **Clippy** with `-D warnings` for `aarch64-linux-android`, `wasm32-unknown-unknown` and
  `x86_64-apple-darwin` (which compiles the `NWPathMonitor` code), and `cargo check` for
  `aarch64-apple-ios`.

## Not done

- **Phone check not run: phone disconnected.** The release APK of the demo (Settings screen: the
  network and a button per haptic) was built for `aarch64-linux-android` with this branch and
  `cargo clippy --target aarch64-linux-android` is clean, but the test phone was not attached
  (`adb devices` empty) for the length of the milestone. So neither `performHapticFeedback` nor
  the default-network callback was seen running, and airplane mode was not toggled.
- **iOS**: haptics answer `Unsupported` and connectivity `Unknown`. The `UIImpactFeedbackGenerator`
  family and `NWPathMonitor` (the macOS code would serve) are the way.
- **macOS** connectivity is compiled for the target but was not run on a Mac.
- **Linux** change detection was not seen on a machine (only the first read). It looks every two seconds rather than listening to netlink: a change takes up to two
  seconds to show.
- **Web**: not run in a browser in this milestone.
- No **switch or checkbox** feedback on Android, deliberately (see above); the iOS switch's
  light impact waits for iOS haptics.
- No per-widget `enable_feedback`: the switch is the theme's, for the whole application.
