# Playing video

A `VideoPlayerController` plays one clip — built into the application, a file, or an address
on the network — with the platform's own player, which plays the sound, keeps the picture in
step with it, seeks and streams. A `VideoPlayer` shows the picture; a `VideoProgressIndicator`
and `VideoControls` drive it. It is the `video` feature of the `frus` crate:

```toml
frus = { version = "0.5", features = ["video"] }
```

| | Android | Windows | Web | macOS, Linux, iOS |
| --- | --- | --- | --- | --- |
| Player | `MediaPlayer` | Media Foundation | `<video>` | — (`VideoError::Unsupported`) |
| Built-in clip, file | ✓, ✓ | ✓, ✓ | ✓, — | |
| `http(s)` file, HLS playlist | ✓, ✓ | ✓, ✓ (Windows 10) | ✓, where the browser plays HLS | |

What a platform plays — H.264 and AAC everywhere, VP9, HEVC, AV1 on some — is the platform's.
On Android, video needs Android 8 or later.

## A clip on a screen

Make the controller where the screen keeps its state, start it with `initialize`, and drop it
(or call `dispose`) when the screen goes: that releases the player.

```rust,no_run
use frus::{
    spawn_local, Color, Positioned, Stack, StackFit, State, StateContext, StatefulWidget,
    StatelessWidget, VideoControls, VideoPlayer, VideoPlayerController, Widget,
};

struct Clip;

struct ClipState {
    controller: VideoPlayerController,
}

impl StatefulWidget for Clip {
    type State = ClipState;

    fn create_state(&self) -> ClipState {
        let controller = VideoPlayerController::network_url("https://example.com/clip.mp4");
        // Asked before it is open: applied as soon as it is.
        controller.set_looping(true);
        controller.play();
        let opening = controller.clone();
        spawn_local(async move {
            if let Err(err) = opening.initialize().await {
                frus::log::warn!("{err}");
            }
        });
        ClipState { controller }
    }
}

impl State for ClipState {
    type Widget = Clip;

    fn build(&self, _cx: &StateContext<Self>) -> Box<dyn Widget> {
        let c = &self.controller;
        Box::new(
            Stack::new()
                .fit(StackFit::Expand)
                .layer(VideoPlayer::new(c).background(Color::BLACK).into_widget())
                .layer(Positioned::fill(VideoControls::new(c).into_widget())),
        )
    }
}
```

`initialize` starts opening at once; its future only says when the clip is open (its duration
and size known) or why it cannot be. Dropping the future does not stop it.

Every change the player reports — the position, four times a second while it plays; waiting
for data; the end; an error — updates the controller's `value()` and asks for a rebuild, so a
screen that reads the value while it builds follows it. To react to changes in code, listen:

```rust,no_run
use frus::{Listenable, VideoPlayerController};

let controller = VideoPlayerController::file("/sdcard/Movies/clip.mp4");
let watched = controller.clone();
let subscription = controller.add_listener(move || {
    let v = watched.value();
    if v.is_completed {
        frus::log::info!("finished after {:?}", v.duration);
    }
});
// Keep `subscription` for as long as you listen.
# drop(subscription);
```

## Sources

```rust,no_run
use frus::VideoPlayerController;

// Built into the application: the bytes are written once to the cache folder (a blob on the
// Web) and played from there.
static INTRO: &[u8] = &[]; // include_bytes!("../assets/intro.mp4")
let built_in = VideoPlayerController::asset(INTRO);

// A file on the device (not on the Web).
let file = VideoPlayerController::file("/data/user/0/com.example/files/clip.mp4");

// The network: a file streamed, or an HLS playlist.
let stream = VideoPlayerController::network_url("https://example.com/live/master.m3u8");
# let _ = (built_in, file, stream);
```

On Android, network clips need the `INTERNET` permission in the application's manifest
(`frus build` and `frus run` declare it for every application; an APK built by hand with
cargo-apk needs `[[package.metadata.android.uses_permission]] name = "android.permission.INTERNET"`),
and a plain `http://` address needs `uses_cleartext_traffic`. On the Web, a clip from another origin
is read only if its server allows the page (CORS headers).

## Driving the playback

```rust,no_run
use std::time::Duration;
use frus::VideoPlayerController;

let c = VideoPlayerController::network_url("https://example.com/clip.mp4");
c.play();                                   // from the start again once it has ended
c.pause();
c.seek_to(Duration::from_secs(30));         // kept within the clip
c.set_volume(0.5);                          // 0 to 1
c.set_looping(true);
c.set_playback_speed(1.5).unwrap();         // above zero; Android 6 and later

let v = c.value();
let _ = (v.position, v.duration, v.aspect_ratio(), v.is_buffering, v.buffered);
```

`VideoPlayerValue` holds the duration, the position, the picture's size and aspect ratio, the
downloaded stretches (`buffered`), and whether it is initialised, playing, looping, waiting
for data or completed, with the volume, the speed and the error, if any.

## The look

`VideoPlayer` fits the picture into the room it is given, keeping its proportions
(`BoxFit::Contain`); `fit`, `background` and `semantic_label` change that.

`VideoProgressIndicator` is the track along the bottom: what was played, what is downloaded,
and — unless `allow_scrubbing(false)` — a bar that moves the playback where it is pressed or
dragged, pausing the clip while the finger is down. Its colours are the theme's unless given:

```rust,no_run
use frus::{Color, StatelessWidget, VideoPlayerController, VideoProgressColors, VideoProgressIndicator};

let c = VideoPlayerController::network_url("https://example.com/clip.mp4");
let bar = VideoProgressIndicator::new(&c)
    .colors(VideoProgressColors {
        played: Color::rgb(0.9, 0.1, 0.1),
        buffered: Color::rgba(1.0, 1.0, 1.0, 0.4),
        background: Color::rgba(1.0, 1.0, 1.0, 0.15),
    })
    .bar_height(3.0)
    .into_widget();
# let _ = bar;
```

`VideoControls` is a default overlay: a play, pause or replay button in the middle (a spinner
while it waits for data), the bar with the position and the duration at the bottom, over a
scrim; a tap shows or hides it, and while the clip plays it hides itself after three seconds.
Its `style` sets the colours, the sizes and the timing; `center` and `bottom` replace either
part; or leave it out and build controls of your own from the controller.

```rust,no_run
use std::time::Duration;
use frus::{
    text, Color, StatelessWidget, VideoControls, VideoControlsStyle, VideoPlayerController,
};

let c = VideoPlayerController::network_url("https://example.com/clip.mp4");
let controls = VideoControls::new(&c)
    .style(VideoControlsStyle {
        scrim: Color::TRANSPARENT,
        hide_after: Some(Duration::from_secs(5)),
        ..Default::default()
    })
    .bottom(|c| Box::new(text(format!("{:?}", c.position())).color(Color::WHITE)))
    .into_widget();
# let _ = controls;
```

## The application in the background

A clip playing pauses when the application goes to the background and plays again when it
comes back. `VideoPlayerController::allow_background_playback(true)` keeps it playing (sound
only, while the system lets the application run).

## What it costs

Nothing while no clip is open: no thread, no timer, nothing at start-up but, on Android, one
reference to the class loader. While a clip plays, the platform's player decodes it; each new
picture is copied into the texture and asks for one frame, and the controller hears of the
position four times a second. Paused, nothing runs (on Windows, the clip's thread sleeps until
it is told something).
