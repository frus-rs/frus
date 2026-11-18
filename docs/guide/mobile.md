# Android and iOS

On a phone a frus application is the same code as on the desktop: the same widgets drawn by the
same renderer, with the platform's conventions where it has them (its scrolling, its page
transitions, its back gesture, its text selection handles). This page covers what is particular
to phones: the Android project, the back button, links and shares, and where iOS stands.
[Starting a frus application](../getting-started.md#android) covers installing the tools and the
first run.

## The Android project

`frus create` writes everything Android needs into the application's `Cargo.toml` and a `res/`
folder; there is no Gradle project to keep. The parts worth knowing:

```toml
[package.metadata.android]
package = "com.example.notes"           # the application's id on the device and in the store
build_targets = ["aarch64-linux-android"]
resources = "res"                       # the launcher icon and the launch theme

[package.metadata.android.sdk]
min_sdk_version = 24
target_sdk_version = 34

[package.metadata.android.application]
label = "Notes"
icon = "@mipmap/ic_launcher"
theme = "@style/LaunchTheme"            # keep its NoActionBar parent

[package.metadata.frus]
platforms = ["android", "web", "windows"]
```

- **Permissions** are declared once under `[package.metadata.frus.permissions]`, and frus writes
  the Android entries: see [The system around the application](platform.md#permissions).
  Internet access is always declared.
- **The launch theme** paints the window before the first frame: give it the colour the
  application opens with. Its `NoActionBar` parent matters: a theme with an action bar makes the
  safe area 56dp too short.
- **`config_changes`** on the activity lists every change Android may make while the
  application runs (rotation, a keyboard, the font size, dark mode). Keep the whole list: for a
  change not on it, Android starts the activity again, and the application closes.
- **The launcher icon** is the `res/mipmap-*` set; frus's logo is the default. See
  [The application's icon](platform.md#the-applications-icon).
- **Signing** a release, App Bundles and one APK per processor are in
  [Build modes, flavors and releases](build-variants.md).

`frus run -d android` and `frus build apk` add frus's own activity, which keeps one instance of
the application: a link, a share or a tapped notification reaches the running application rather
than starting a second one.

## The back button and the back gesture

Back pops the router's top page, closes an open dialog, sheet, drawer or menu first, and on the
first page of the application leaves it, as Android expects. A tab bar sends Back to the first
tab before leaving. Nothing is written for any of this.

A component that shows something of its own that Back should close first says so on each build:

```rust
use frus::{column, text, Button, BuildContext, Widget};

fn editor(cx: &BuildContext) -> Box<dyn Widget> {
    let panel_open = cx.use_state(|| false);
    // While the panel is open, Back is not the router's.
    cx.block_back(panel_open.get());
    let toggle = panel_open.clone();
    Box::new(column![
        text(if panel_open.get() { "Formatting panel" } else { "Text" }),
        Button::new("Format").on_press(move || toggle.update(|open| *open = !*open)),
    ])
}
# let _ = editor;
```

The router's `redirect` keeps a reader out of a page; [Screens, navigation and
dialogs](navigation.md) has it, and how Back behaves in nested navigation.

## Links into the application

`frus create --deeplink myapp` (or `--deeplink https://example.com` for links to a web site you
own) declares the scheme or the host, and a link that opens the application lands on the page
it names: `myapp://orders/42` and `https://example.com/orders/42` are both the router's
`/orders/42`. `location_of_link` is that translation, for an application that handles links
itself:

```rust
assert_eq!(frus::location_of_link("myapp://orders/42").as_deref(), Some("/orders/42"));
assert_eq!(
    frus::location_of_link("https://example.com/orders/42?tab=items").as_deref(),
    Some("/orders/42?tab=items"),
);
```

## What other applications share

An application that appears in the system's share sheet receives what is shared into it: text,
a subject, files (copied into its cache folder). Uncomment the `SEND` intent filter the template
has in `Cargo.toml`, then:

```rust
use frus::{text, FrusApp, SharedContent};

fn app() -> FrusApp {
    FrusApp::from_fn(|_| Box::new(text("Notes"))).on_share(|shared: SharedContent| {
        if let Some(text) = shared.text {
            frus::log::info!("shared with us: {text}");
        }
        for file in shared.files {
            frus::log::info!("a file: {}", file.display());
        }
    })
}
# let _ = app;
```

Sharing **out** of the application, opening a link in the browser, haptics, notifications,
secrets in the Android Keystore and the network's state are in
[The system around the application](platform.md).

## The screen

Safe areas, the keyboard, the system bars' colours and locking the orientation are in
[The screen](screen.md). Screen readers (TalkBack) are in [Accessibility](accessibility.md).

## Speed and battery

A frus application draws nothing while nothing moves: an idle screen costs no frames. An
animation that only decorates (a shimmer, a spinner) is drawn at 30 frames a second rather than
the display's rate; `FrusApp::decorative_frame_rate` changes that. Measure on a real phone with
`frus run -d android --profile` and the `P` panel; an emulator's GPU says little about a phone's.
A release APK is a few megabytes; a debug build is hundreds, and is not what to measure or ship.

## iOS

frus does not run on iOS yet: there is no iOS shell. What exists today:

- **A simulated iPhone or iPad** on the desktop: `frus run -d iphone-15` shows the application
  in a window with the device's size, notch, safe area and the iOS look and behaviour, and
  `--web` shows it in a browser. See [The screen](screen.md#seeing-other-screens-without-the-devices).
- **The iOS look** anywhere: `o` under `frus run` switches the platform the widgets behave as,
  and the Apple-style widgets (`CupertinoButton`, `CupertinoSwitch`, `CupertinoNavigationBar`…)
  work on every platform. See [Text, colour and the theme](style.md).

See LIMITATIONS.md for the state of the iOS port.
