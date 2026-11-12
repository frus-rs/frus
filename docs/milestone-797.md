# Milestone 797 — Video playback

## Objective

Playing a video must be as simple as making a controller and putting a widget on the screen:
a clip built into the application, a file or an address on the network, with sound, seeking,
looping, speed and volume, a progress bar that can be dragged, and controls an application
can use as they are or replace. It builds on the external textures of milestone 793.

## API

```rust
use frus::{
    spawn_local, Listenable, StatelessWidget, VideoControls, VideoControlsStyle, VideoPlayer,
    VideoPlayerController, VideoProgressIndicator,
};

let c = VideoPlayerController::network_url("https://example.com/clip.mp4");
// or ::asset(include_bytes!("intro.mp4")), ::file(path)
spawn_local({ let c = c.clone(); async move { c.initialize().await } });
c.play(); c.pause(); c.seek_to(d); c.set_volume(0.5); c.set_looping(true);
c.set_playback_speed(1.5)?;
let v = c.value();   // duration, position, width/height, aspect_ratio(), buffered,
                     // is_initialized/is_playing/is_looping/is_buffering/is_completed,
                     // volume, playback_speed, error_description
let _s = c.add_listener(|| ..);            // every change also asks for a rebuild
c.dispose();                               // or drop the last handle

VideoPlayer::new(&c).fit(BoxFit::Contain).background(Color::BLACK).into_widget();
VideoProgressIndicator::new(&c).colors(..).allow_scrubbing(true).into_widget();
VideoControls::new(&c).style(VideoControlsStyle { .. }).center(|c| ..).bottom(|c| ..)
```

Behind the `video` feature of `frus` (and `frus-shell`), off by default.

## Design

### The controller (`frus-shell/src/video/controller.rs`)

The controller lives on the interface's thread: an `Rc` of the source, a `ValueNotifier` of
`VideoPlayerValue`, and the platform's player behind a small trait (`play`, `pause`,
`seek_to`, `set_volume`, `set_looping`, `set_playback_speed`; dropping it releases the
player). The platform players report from their own threads, so they report into a **sink**:
a queue whose waker is a task of the interface's thread (`spawn_local`, milestone 714). A
report wakes the task, which asks the shell for a frame; the frame runs the task before it
builds; the task applies the reports to the value, which tells the listeners and asks for a
rebuild. No report, no frame. Positions coalesce in the queue (only the newest is applied).

Behaviour follows the leading toolkit's package:

- settings and `play()` made before the clip is open are applied once it is;
- `play()` on a clip at its end starts from the beginning;
- the end stops the playback and marks the value completed, unless it loops;
- `seek_to` and `set_volume` stay within bounds; a speed that is not above zero is refused;
- `initialize()` starts at once and its future answers when the duration and size are known,
  or with why not (also kept as `error_description`);
- the application going to the background pauses every clip playing (unless
  `allow_background_playback(true)`), and coming back plays them again — but not one the
  person paused (the shell's lifecycle calls `video::lifecycle`).

### Android: `MediaPlayer` through JNI, not `MediaCodec` through the NDK

The choice was between the NDK's `AMediaExtractor` + `AMediaCodec` (into the texture
surface's `ANativeWindow`) with sound through AAudio, and the platform's `MediaPlayer` given the
texture surface's `android.view.Surface`.

| | NDK codec + AAudio | `MediaPlayer` |
| --- | --- | --- |
| Sound | a second codec, AAudio stream, resampling | built in |
| A/V sync | ours: a clock from AAudio's position, frames released against it | built in |
| Seeking | extractor seek to a sync frame, decode forward to the target, flush both codecs | `seekTo(ms, SEEK_CLOSEST)` |
| Network | `AMediaExtractor_setDataSource` reads progressive HTTP; **no HLS** | HTTP(S) and HLS |
| Buffering, errors | ours | listeners |
| Speed | time-stretching the sound ourselves | `PlaybackParams` (Android 6) |
| Code | about 1500 lines of Rust, estimated | 270 lines of Java, 380 of Rust |

`MediaPlayer` gives synchronisation, seeking and HLS with the least code; the library gains a
7.2 KB dex of its own (`frus_video.dex`, built by `scripts/build-video-dex.sh`), embedded only
with the `video` feature and loaded, through its own in-memory class loader, the first time a
clip opens. It is not part of the bundled `frus_input.dex`, so an application without the
feature carries none of it. Its pictures go through the same path as
milestone 793's decoder: the image reader's planes are copied into the texture.

`FrusVideo.java` (`dev.frus.video`) wraps one `MediaPlayer`: `open` sets the source and prepares it
asynchronously; `onPrepared` reports the duration and the picture's size. On that report (on
the main thread) the Rust side makes a `TextureSurface` **at the picture's size** (BT.709 from
720 lines up, BT.601 below), gives its Java surface to the player (`setSurface`), and reports
the clip open with the texture's id. A `Handler` on the main looper reports the position every
250 ms while the clip plays, and only then. Buffering (`MEDIA_INFO_BUFFERING_START/END`), the
downloaded percentage, the end, errors and size changes are reported as they come. Releasing
the controller calls `release()` before the surface is dropped, so nothing draws into a
surface that is gone. The dex is loaded, and the native wired, the first time a clip opens; at
start-up only a reference to the application's class loader is kept.

`TextureSurface::java_surface` needs Android 8 (API 26), so does video.

### Windows: Media Foundation's media engine

`IMFMediaEngine` in frame-server mode (no window; `MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT` =
BGRA) opens files and `http(s)` addresses, plays the sound and keeps the clock. One thread per
clip owns the engine and Media Foundation's start-up; the engine's events
(`IMFMediaEngineNotify`, on Media Foundation's threads) are sent to it. While the clip plays,
the thread asks every 8 ms whether a picture is due (`OnVideoStreamTick`, its `S_OK` and not
its `S_FALSE`), has it drawn into a WIC bitmap (`TransferVideoFrame`) and pushes it as BGRA,
alpha set opaque. Paused, the thread blocks on its channel: it costs nothing. After a seek or
the first load it ticks for half a second so that a paused clip shows the picture it is on.

New dependency: `windows-core` 0.62, already in the tree through `windows`, named because
`#[implement]` expands to it. New `windows` features: `Win32_Media_MediaFoundation`,
`Win32_Graphics_Imaging` (only with `video`).

### The Web: `<video>` drawn through a canvas

A `<video>` element (one transparent pixel in the page; some browsers do not decode a
detached one) plays the clip. While it plays, each animation frame checks whether its time
moved on and, when it did, draws it into a 2D canvas and pushes `getImageData`'s pixels.
Paused, no animation frame is asked for. A built-in clip becomes a `blob:` address; a file is
`Unsupported`. A network clip is asked for anonymously (`crossOrigin`), and a server that does
not allow the page leaves the canvas unreadable, which is reported as an error. A browser that
refuses to start a clip with sound before the page was touched is reported as the clip not
playing.

`copyExternalImageToTexture` would skip the canvas and the CPU copy on WebGPU; it is not done
(below).

### macOS, Linux, iOS

`initialize` answers `VideoError::Unsupported`. AVFoundation (`AVPlayerItemVideoOutput`) and
GStreamer (`appsink`) are the paths; neither was cheap enough for this milestone.

### The widgets (`frus-shell/src/video/widgets.rs`)

- `VideoPlayer`: the controller's `Texture`, `BoxFit::Contain` by default, an optional
  background, a semantic label ("Video"); before the clip is open, its background only.
- `VideoProgressIndicator`: a track (the theme's muted colour), the downloaded stretches and the
  part played (the theme's primary colour), all overridable (`VideoProgressColors`), the bar's
  height and padding too. Scrubbing (on by default): a press pauses the clip if it played and
  seeks there, a drag seeks along, the release plays again. Announced as a slider with
  "0:12 of 1:00".
- `VideoControls`: a scrim, a play / pause / replay button in the middle (a spinner while
  waiting for data), the bar with the position and the duration at the bottom. A tap shows or
  hides them; while the clip plays they hide after three seconds. `VideoControlsStyle` holds
  every colour, size and timing; `center` and `bottom` replace either part.

Each is a `StatelessWidget` (`.into_widget()`), reading the controller when it builds.

### Cost when nothing plays

No thread, no timer, nothing at start-up but (Android) one global reference to the class
loader. The `video` feature compiles nothing in but the bindings; without it, nothing at all.
The lifecycle hook walks an empty list.

## How it was tested

See the sections below; numbers are from the runs described.

### Unit tests

- `controller.rs` (11): nothing is opened before `initialize`; it answers when the player is
  ready and applies what was asked before (looping, volume, play, in that order); an
  unsupported platform fails it with its reason; an error reported later fails it; listeners
  hear each report, two queued positions are applied as one, an unchanged position tells nobody;
  the end stops the playback and `play` starts over from 0; a looping clip does not complete;
  seeking and the volume stay in bounds, invalid speeds are refused; the background pauses and
  the return plays again, a clip the person paused stays paused, one allowed in the background
  goes on; the last handle or `dispose` releases the player, and `initialize` then answers
  `Disposed`; a report from another thread reaches the value.
- `platform/mod.rs`: file extensions from the first bytes; an asset is written once and reused.
- `windows.rs`: file addresses escaped; infinite and NaN durations are zero.
- `widgets.rs`: the time format; the track's played and downloaded fractions.
- The guide (`docs/guide/video.md`) compiles as doctests (`cargo test -p frus --features video
  --doc`).
- `widgets.rs`, through the shell's windowless driver (`Driver`): the play button takes its tap
  and pauses (a tap area under it took it before), the bar has the room between the two times
  and seeks where it is pressed (it had none, the times touching, and seeked to 0), and once the
  controls have hidden, a tap brings them back and a second tap on the button pauses.

### Windows

An ignored test opens a real clip through Media Foundation: 120 pictures of 320 x 240 reach the
texture, and the clip ends.

### On the test phone (Mali-G51), release APK

- The built-in clip plays at 34 fps, the app taking 52% of one core; paused, idle on the home
  screen, and after leaving the player: 0.0%.
- PSS 134 MB on the home screen, 291 MB playing (200 MB of it graphics), 285 MB after.
- An HLS stream at 1080p plays (77-88% of a core). In the background the clip pauses; it plays
  again on return.
- Two bugs were found there and not by the tests, fixed and each covered by a driver test since:
  the centre button did not pause (a tap area took its tap), and the progress bar had zero
  width inside the controls' row.
- A third report, the second tap of "show the controls, then pause" seemingly lost, was the
  script: each `adb shell input` takes about 1.4 s to start on this phone, so the second tap
  landed after the controls had hidden again and showed them instead. Both taps sent in one
  shell pause the clip; the driver test keeps the sequence.

### Size

An application without the `video` feature pays nothing: `frus-hello`'s release library for
Android is 14,456,328 bytes on this branch and on master. The Java side is a dex of its own
(`assets/frus_video.dex`, 7,224 bytes), loaded only by the feature, so `frus_input.dex` is
unchanged.

## Not done

- macOS, Linux and iOS (`VideoError::Unsupported`), see above.
- Pictures are copied through the CPU on Windows and the web; a GPU-to-GPU path (shared
  textures, `importExternalTexture`) would cut that copy.
- Subtitles, audio tracks, DRM and picture-in-picture.
- A large network MP4 (158 MB, no fast start) did not open within 25 s on the phone: `MediaPlayer`
  reads up to the index at the file's end first. The demo uses a small clip.
