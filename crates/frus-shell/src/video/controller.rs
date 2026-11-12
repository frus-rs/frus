// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`VideoPlayerController`]: one clip, its platform player, and what is known of it.
//!
//! The controller lives on the interface's thread. The platform's player reports from threads
//! of its own (Android's main thread, Media Foundation's work queues), so what it says goes
//! through a [`Sink`]: a queue whose waker is a task of the interface's thread
//! ([`spawn_local`]), which applies each report to the controller's value and tells the
//! listeners. A report asks for a frame; no report, no frame.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use frus_widgets::{spawn_local, Listenable, Subscription, TaskHandle, TextureId, ValueNotifier};

/// Where a clip comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataSource {
    /// Bytes built into the application, usually with `include_bytes!`.
    Asset(&'static [u8]),
    /// A file on the device.
    File(PathBuf),
    /// An `http://` or `https://` address: a file the player streams, or an HLS playlist
    /// (`.m3u8`) where the platform plays them.
    Network(String),
}

/// A stretch of the clip, from `start` to `end`: what has been downloaded, for instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DurationRange {
    /// Where it begins.
    pub start: Duration,
    /// Where it ends.
    pub end: Duration,
}

impl DurationRange {
    /// A range from `start` to `end`.
    pub fn new(start: Duration, end: Duration) -> Self {
        Self { start, end }
    }

    /// Where it begins, as a fraction of `duration` (0 to 1).
    ///
    /// ```
    /// use std::time::Duration;
    /// use frus_shell::DurationRange;
    ///
    /// let r = DurationRange::new(Duration::from_secs(5), Duration::from_secs(10));
    /// assert_eq!(r.start_fraction(Duration::from_secs(20)), 0.25);
    /// assert_eq!(r.end_fraction(Duration::from_secs(20)), 0.5);
    /// ```
    pub fn start_fraction(&self, duration: Duration) -> f32 {
        fraction(self.start, duration)
    }

    /// Where it ends, as a fraction of `duration` (0 to 1).
    pub fn end_fraction(&self, duration: Duration) -> f32 {
        fraction(self.end, duration)
    }
}

fn fraction(at: Duration, of: Duration) -> f32 {
    if of.is_zero() {
        0.0
    } else {
        (at.as_secs_f64() / of.as_secs_f64()).clamp(0.0, 1.0) as f32
    }
}

/// Why a clip cannot be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoError {
    /// This platform has no player frus can drive (macOS, Linux and iOS for now), or not for
    /// this kind of source (a file on the Web).
    Unsupported(&'static str),
    /// The source could not be opened or read: no such file, an address that does not answer,
    /// a format the platform does not play. The platform's own words.
    Source(String),
    /// The controller was disposed of.
    Disposed,
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoError::Unsupported(what) => write!(f, "video playback is unsupported: {what}"),
            VideoError::Source(why) => write!(f, "the video could not be played: {why}"),
            VideoError::Disposed => write!(f, "the video controller was disposed of"),
        }
    }
}

impl std::error::Error for VideoError {}

/// Everything known of a clip at one moment: what [`VideoPlayerController::value`] answers,
/// and what changed when its listeners are told.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoPlayerValue {
    /// How long the clip is; zero until it is initialised, and for a live stream.
    pub duration: Duration,
    /// Where the playback is.
    pub position: Duration,
    /// The picture's size in pixels; zero until initialised, and for a clip with no picture.
    pub width: u32,
    /// The picture's height in pixels.
    pub height: u32,
    /// What has been downloaded or decoded ahead, as the platform reports it.
    pub buffered: Vec<DurationRange>,
    /// Whether the clip is open and its duration and size known.
    pub is_initialized: bool,
    /// Whether it plays (or will, as soon as it is initialised).
    pub is_playing: bool,
    /// Whether it starts again from the beginning when it ends.
    pub is_looping: bool,
    /// Whether it waits for data.
    pub is_buffering: bool,
    /// Whether it reached its end (and is not looping).
    pub is_completed: bool,
    /// The sound's volume, 0 to 1.
    pub volume: f32,
    /// How fast it plays: 1 is normal speed.
    pub playback_speed: f32,
    /// What went wrong, in the platform's words, if anything did.
    pub error_description: Option<String>,
}

impl Default for VideoPlayerValue {
    fn default() -> Self {
        Self {
            duration: Duration::ZERO,
            position: Duration::ZERO,
            width: 0,
            height: 0,
            buffered: Vec::new(),
            is_initialized: false,
            is_playing: false,
            is_looping: false,
            is_buffering: false,
            is_completed: false,
            volume: 1.0,
            playback_speed: 1.0,
            error_description: None,
        }
    }
}

impl VideoPlayerValue {
    /// The picture's width over its height; 1 until the size is known.
    ///
    /// ```
    /// use frus_shell::VideoPlayerValue;
    ///
    /// let value = VideoPlayerValue { width: 1920, height: 1080, ..Default::default() };
    /// assert!((value.aspect_ratio() - 16.0 / 9.0).abs() < 1e-6);
    /// assert_eq!(VideoPlayerValue::default().aspect_ratio(), 1.0);
    /// ```
    pub fn aspect_ratio(&self) -> f32 {
        if self.width == 0 || self.height == 0 {
            1.0
        } else {
            self.width as f32 / self.height as f32
        }
    }

    /// Whether something went wrong.
    pub fn has_error(&self) -> bool {
        self.error_description.is_some()
    }

    /// How far the playback is, 0 to 1; 0 while the duration is unknown.
    pub fn progress(&self) -> f32 {
        fraction(self.position, self.duration)
    }
}

// ---------------------------------------------------------------------------------------
// What the platform's player reports, and the queue it goes through
// ---------------------------------------------------------------------------------------

/// A report from the platform's player.
// Only a platform with a player (Android, Windows, the web) and the tests report events.
#[cfg_attr(
    not(any(target_os = "android", windows, target_arch = "wasm32")),
    allow(dead_code)
)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    /// The clip is open: its duration (zero for a live stream), its picture's size, and the
    /// texture the pictures go to.
    Ready {
        duration: Duration,
        width: u32,
        height: u32,
        texture: Option<TextureId>,
    },
    /// The picture changed size.
    Size { width: u32, height: u32 },
    /// Where the playback is.
    Position(Duration),
    /// What is downloaded.
    Buffered(Vec<DurationRange>),
    /// Waiting for data, or no longer.
    Buffering(bool),
    /// The platform started or stopped the playback of its own accord (a browser refusing to
    /// start a clip with sound before the page was touched).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Playing(bool),
    /// The end was reached.
    Completed,
    /// Something went wrong.
    Error(String),
}

#[derive(Default)]
struct SinkState {
    events: Vec<Event>,
    waker: Option<Waker>,
}

/// Where the platform's player reports, from any thread.
#[derive(Default)]
pub(crate) struct Sink {
    state: Mutex<SinkState>,
}

impl Sink {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Queues `event` and wakes the interface's thread. Positions coalesce: only the latest
    /// one not yet applied is kept.
    #[cfg_attr(
        not(any(target_os = "android", windows, target_arch = "wasm32")),
        allow(dead_code)
    )]
    pub(crate) fn send(&self, event: Event) {
        let waker = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if let (Event::Position(_), Some(Event::Position(_))) = (&event, state.events.last()) {
                state.events.pop();
            }
            state.events.push(event);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    pub(crate) fn poll_take(&self, cx: &mut Context<'_>) -> Poll<Vec<Event>> {
        let Ok(mut state) = self.state.lock() else {
            return Poll::Pending;
        };
        if state.events.is_empty() {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(std::mem::take(&mut state.events))
        }
    }
}

/// What the controller asks of the platform's player. Dropping it releases the player.
pub(crate) trait Player {
    fn play(&self);
    fn pause(&self);
    fn seek_to(&self, position: Duration);
    fn set_volume(&self, volume: f32);
    fn set_looping(&self, looping: bool);
    fn set_playback_speed(&self, speed: f32);
}

/// Opens a source, the player reporting to the sink.
pub(crate) type Opener = fn(&DataSource, Arc<Sink>) -> Result<Box<dyn Player>, VideoError>;

// ---------------------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------------------

struct Inner {
    source: DataSource,
    opener: Opener,
    allow_background: Cell<bool>,
    value: ValueNotifier<VideoPlayerValue>,
    texture: Cell<Option<TextureId>>,
    player: RefCell<Option<Box<dyn Player>>>,
    sink: Arc<Sink>,
    pump: RefCell<Option<TaskHandle<()>>>,
    started: Cell<bool>,
    disposed: Cell<bool>,
    /// Paused because the application went to the background; played again on its return.
    resume: Cell<bool>,
    waiting: RefCell<Vec<Waker>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(pump) = self.pump.get_mut().take() {
            pump.cancel();
        }
        // Releases the platform's player and its texture.
        drop(self.player.get_mut().take());
    }
}

/// **Plays one video**: from the application's bytes, a file or the network — opened with
/// [`initialize`](Self::initialize), shown by a [`VideoPlayer`](crate::VideoPlayer), driven by
/// [`play`](Self::play), [`pause`](Self::pause), [`seek_to`](Self::seek_to) and the setters.
///
/// Its [`value`](Self::value) says where the playback is; listeners
/// ([`Listenable::add_listener`]) are told of every change, and each change asks for a rebuild,
/// so a tree that reads the value while it is built follows it with nothing more.
///
/// It lives on the interface's thread; clones are handles to the same clip. The player is
/// released when the last clone is dropped, or at once by [`dispose`](Self::dispose). It pauses
/// when the application goes to the background and plays again when it comes back, unless
/// [`allow_background_playback`](Self::allow_background_playback) says otherwise.
///
/// ```no_run
/// use frus_shell::VideoPlayerController;
/// use frus_widgets::spawn_local;
///
/// let controller = VideoPlayerController::network_url("https://example.com/clip.mp4");
/// let c = controller.clone();
/// spawn_local(async move {
///     if c.initialize().await.is_ok() {
///         c.set_looping(true);
///         c.play();
///     }
/// });
/// ```
#[derive(Clone)]
pub struct VideoPlayerController {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for VideoPlayerController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoPlayerController")
            .field("source", &self.inner.source)
            .field("value", &self.inner.value.with(|v| v.clone()))
            .finish()
    }
}

impl PartialEq for VideoPlayerController {
    /// The same clip: two handles of one controller.
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl VideoPlayerController {
    /// A clip built into the application: `VideoPlayerController::asset(include_bytes!("intro.mp4"))`.
    ///
    /// The bytes are written to the application's cache folder the first time they are played
    /// (the platform players read files, not memory); on the Web they become a `blob:` address.
    pub fn asset(bytes: &'static [u8]) -> Self {
        Self::new(DataSource::Asset(bytes))
    }

    /// A file on the device.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::new(DataSource::File(path.into()))
    }

    /// An address on the network: `http://` or `https://`, a file or an HLS playlist.
    ///
    /// On Android the application needs the `INTERNET` permission, and `http://` (not `https`)
    /// addresses need the manifest's `usesCleartextTraffic`. On the Web the server must allow
    /// the page's origin (CORS), or the pictures cannot be read.
    pub fn network_url(url: impl Into<String>) -> Self {
        Self::new(DataSource::Network(url.into()))
    }

    /// A controller for `source`. Nothing is opened before [`initialize`](Self::initialize).
    pub fn new(source: DataSource) -> Self {
        Self::with_opener(source, super::platform::open)
    }

    pub(crate) fn with_opener(source: DataSource, opener: Opener) -> Self {
        Self {
            inner: Rc::new(Inner {
                source,
                opener,
                allow_background: Cell::new(false),
                value: ValueNotifier::new(VideoPlayerValue::default()),
                texture: Cell::new(None),
                player: RefCell::new(None),
                sink: Sink::new(),
                pump: RefCell::new(None),
                started: Cell::new(false),
                disposed: Cell::new(false),
                resume: Cell::new(false),
                waiting: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Keeps playing while the application is in the background (sound-only playback). Off by
    /// default: the clip pauses, and plays again when the application comes back.
    pub fn allow_background_playback(self, allow: bool) -> Self {
        self.inner.allow_background.set(allow);
        self
    }

    /// Where the clip comes from.
    pub fn data_source(&self) -> &DataSource {
        &self.inner.source
    }

    /// **Opens the clip.** It starts at once; the future answers when the clip's duration and
    /// size are known, or with why it cannot be played (the reason is also the value's
    /// [`error_description`](VideoPlayerValue::error_description)). Dropping the future does
    /// not stop the opening, and a second call opens nothing more.
    ///
    /// Settings made before — [`set_looping`](Self::set_looping), the volume, the speed,
    /// [`play`](Self::play) — are applied once it is open.
    pub fn initialize(&self) -> impl Future<Output = Result<(), VideoError>> + 'static {
        self.start();
        Initialized {
            inner: self.inner.clone(),
        }
    }

    fn start(&self) {
        let inner = &self.inner;
        if inner.started.replace(true) || inner.disposed.get() {
            return;
        }
        register(&self.inner);
        let weak = Rc::downgrade(&self.inner);
        let sink = inner.sink.clone();
        *inner.pump.borrow_mut() = Some(spawn_local(pump(weak, sink)));
        match (inner.opener)(&inner.source, inner.sink.clone()) {
            Ok(player) => *inner.player.borrow_mut() = Some(player),
            Err(err) => {
                log::warn!("video: {err}");
                fail(inner, err.to_string());
            }
        }
    }

    /// What is known of the clip now.
    pub fn value(&self) -> VideoPlayerValue {
        self.inner.value.get()
    }

    /// Reads the value without copying it.
    pub fn with_value<R>(&self, read: impl FnOnce(&VideoPlayerValue) -> R) -> R {
        self.inner.value.with(read)
    }

    /// Where the playback is.
    pub fn position(&self) -> Duration {
        self.inner.value.with(|v| v.position)
    }

    /// The texture the pictures go to, once the clip is open and has a picture: what
    /// [`VideoPlayer`](crate::VideoPlayer) shows.
    pub fn texture_id(&self) -> Option<TextureId> {
        self.inner.texture.get()
    }

    /// **Plays**, from where it is — from the beginning again once it has reached its end.
    /// Called before the clip is open, it plays as soon as it is.
    pub fn play(&self) {
        play(&self.inner);
    }

    /// **Pauses.**
    pub fn pause(&self) {
        let inner = &self.inner;
        inner.resume.set(false);
        inner.value.update(|v| v.is_playing = false);
        if let Some(player) = inner.player.borrow().as_ref() {
            player.pause();
        }
    }

    /// **Moves the playback to `position`**, kept within the clip.
    pub fn seek_to(&self, position: Duration) {
        seek(&self.inner, position);
    }

    /// The sound's volume, 0 (silent) to 1 (full); kept within those.
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_nan() {
            0.0
        } else {
            volume.clamp(0.0, 1.0)
        };
        self.inner.value.update(|v| v.volume = volume);
        if let Some(player) = self.inner.player.borrow().as_ref() {
            player.set_volume(volume);
        }
    }

    /// Whether the clip starts again from the beginning when it ends.
    pub fn set_looping(&self, looping: bool) {
        self.inner.value.update(|v| v.is_looping = looping);
        if let Some(player) = self.inner.player.borrow().as_ref() {
            player.set_looping(looping);
        }
    }

    /// How fast it plays: 1 is normal, 2 twice as fast, 0.5 half. A speed that is not above
    /// zero is refused. Android plays at another speed from Android 6.
    ///
    /// ```
    /// use frus_shell::{VideoError, VideoPlayerController};
    ///
    /// let c = VideoPlayerController::file("clip.mp4");
    /// assert!(c.set_playback_speed(1.5).is_ok());
    /// assert!(c.set_playback_speed(0.0).is_err());
    /// ```
    pub fn set_playback_speed(&self, speed: f32) -> Result<(), VideoError> {
        if speed.is_nan() || speed <= 0.0 {
            return Err(VideoError::Unsupported(
                "a playback speed must be above zero",
            ));
        }
        self.inner.value.update(|v| v.playback_speed = speed);
        if let Some(player) = self.inner.player.borrow().as_ref() {
            player.set_playback_speed(speed);
        }
        Ok(())
    }

    /// **Releases the player** now, rather than when the last handle is dropped. The value
    /// keeps what it said; nothing plays any more.
    pub fn dispose(&self) {
        let inner = &self.inner;
        inner.disposed.set(true);
        if let Some(pump) = inner.pump.borrow_mut().take() {
            pump.cancel();
        }
        let player = inner.player.borrow_mut().take();
        drop(player);
        inner.texture.set(None);
        inner.value.update(|v| v.is_playing = false);
        for waker in inner.waiting.borrow_mut().drain(..) {
            waker.wake();
        }
    }

    /// Whether [`dispose`](Self::dispose) was called.
    pub fn is_disposed(&self) -> bool {
        self.inner.disposed.get()
    }
}

impl Listenable for VideoPlayerController {
    fn add_listener(&self, listener: impl Fn() + 'static) -> Subscription {
        self.inner.value.add_listener(listener)
    }
}

fn play(inner: &Inner) {
    let (initialized, completed, at_end) = inner.value.with(|v| {
        (
            v.is_initialized,
            v.is_completed,
            !v.duration.is_zero() && v.position >= v.duration,
        )
    });
    if initialized && (completed || at_end) {
        seek(inner, Duration::ZERO);
    }
    inner.value.update(|v| {
        v.is_playing = true;
        v.is_completed = false;
    });
    if initialized {
        if let Some(player) = inner.player.borrow().as_ref() {
            player.play();
        }
    }
}

fn seek(inner: &Inner, position: Duration) {
    let position = inner.value.with(|v| {
        if v.duration.is_zero() {
            position
        } else {
            position.min(v.duration)
        }
    });
    inner.value.update(|v| {
        v.position = position;
        v.is_completed = false;
    });
    if let Some(player) = inner.player.borrow().as_ref() {
        player.seek_to(position);
    }
}

fn fail(inner: &Inner, why: String) {
    inner.value.update(|v| {
        v.error_description = Some(why);
        v.is_playing = false;
        v.is_buffering = false;
    });
    for waker in inner.waiting.borrow_mut().drain(..) {
        waker.wake();
    }
}

/// Applies one report of the platform's player.
fn apply(inner: &Inner, event: Event) {
    match event {
        Event::Ready {
            duration,
            width,
            height,
            texture,
        } => {
            if texture.is_some() {
                inner.texture.set(texture);
            }
            inner.value.update(|v| {
                v.is_initialized = true;
                v.duration = duration;
                v.width = width;
                v.height = height;
            });
            let wanted = inner.value.get();
            if let Some(player) = inner.player.borrow().as_ref() {
                player.set_looping(wanted.is_looping);
                if wanted.volume != 1.0 {
                    player.set_volume(wanted.volume);
                }
                if wanted.playback_speed != 1.0 {
                    player.set_playback_speed(wanted.playback_speed);
                }
                if !wanted.position.is_zero() {
                    player.seek_to(wanted.position);
                }
                if wanted.is_playing {
                    player.play();
                }
            }
            for waker in inner.waiting.borrow_mut().drain(..) {
                waker.wake();
            }
        }
        Event::Size { width, height } => inner.value.update(|v| {
            v.width = width;
            v.height = height;
        }),
        Event::Position(position) => {
            let changed = inner.value.with(|v| {
                let position = if v.duration.is_zero() {
                    position
                } else {
                    position.min(v.duration)
                };
                (v.position != position).then_some(position)
            });
            if let Some(position) = changed {
                inner.value.update(|v| v.position = position);
            }
        }
        Event::Buffered(ranges) => {
            if inner.value.with(|v| v.buffered != ranges) {
                inner.value.update(|v| v.buffered = ranges);
            }
        }
        Event::Buffering(buffering) => {
            if inner.value.with(|v| v.is_buffering != buffering) {
                inner.value.update(|v| v.is_buffering = buffering);
            }
        }
        Event::Playing(playing) => {
            if inner.value.with(|v| v.is_playing != playing) {
                inner.value.update(|v| v.is_playing = playing);
            }
        }
        Event::Completed => inner.value.update(|v| {
            if !v.is_looping {
                v.is_playing = false;
                v.is_completed = true;
                if !v.duration.is_zero() {
                    v.position = v.duration;
                }
            }
            v.is_buffering = false;
        }),
        Event::Error(why) => fail(inner, why),
    }
}

/// The task that applies the player's reports on the interface's thread, until the
/// controller is gone.
async fn pump(inner: Weak<Inner>, sink: Arc<Sink>) {
    loop {
        let events = std::future::poll_fn(|cx| sink.poll_take(cx)).await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        for event in events {
            apply(&inner, event);
        }
    }
}

/// Answers once the clip is open, or cannot be.
struct Initialized {
    inner: Rc<Inner>,
}

impl Future for Initialized {
    type Output = Result<(), VideoError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = &self.inner;
        if inner.disposed.get() {
            return Poll::Ready(Err(VideoError::Disposed));
        }
        let (ready, error) = inner
            .value
            .with(|v| (v.is_initialized, v.error_description.clone()));
        if let Some(why) = error {
            return Poll::Ready(Err(VideoError::Source(why)));
        }
        if ready {
            return Poll::Ready(Ok(()));
        }
        inner.waiting.borrow_mut().push(cx.waker().clone());
        Poll::Pending
    }
}

// ---------------------------------------------------------------------------------------
// The application's lifecycle
// ---------------------------------------------------------------------------------------

thread_local! {
    /// The controllers opened on this thread, for the application's lifecycle.
    static LIVE: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
}

#[inline(never)]
fn register(inner: &Rc<Inner>) {
    LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.retain(|w| w.strong_count() > 0);
        live.push(Rc::downgrade(inner));
    });
}

/// The application went to the background (`foreground == false`) or came back: the clips
/// playing pause, and play again on the way back, unless they may play in the background.
#[inline(never)]
pub(crate) fn lifecycle(foreground: bool) {
    let live: Vec<Rc<Inner>> = LIVE.with(|live| {
        let mut live = live.borrow_mut();
        live.retain(|w| w.strong_count() > 0);
        live.iter().filter_map(Weak::upgrade).collect()
    });
    for inner in live {
        if inner.allow_background.get() || inner.disposed.get() {
            continue;
        }
        if foreground {
            if inner.resume.replace(false) {
                play(&inner);
            }
        } else if inner.value.with(|v| v.is_playing) {
            inner.value.update(|v| v.is_playing = false);
            if let Some(player) = inner.player.borrow().as_ref() {
                player.pause();
            }
            inner.resume.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    thread_local! {
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static SINK: RefCell<Option<Arc<Sink>>> = const { RefCell::new(None) };
    }

    static RELEASED: AtomicUsize = AtomicUsize::new(0);

    struct Fake;

    fn log(call: String) {
        CALLS.with(|c| c.borrow_mut().push(call));
    }

    fn calls() -> Vec<String> {
        CALLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
    }

    impl Player for Fake {
        fn play(&self) {
            log("play".into());
        }
        fn pause(&self) {
            log("pause".into());
        }
        fn seek_to(&self, position: Duration) {
            log(format!("seek {}", position.as_millis()));
        }
        fn set_volume(&self, volume: f32) {
            log(format!("volume {volume}"));
        }
        fn set_looping(&self, looping: bool) {
            log(format!("looping {looping}"));
        }
        fn set_playback_speed(&self, speed: f32) {
            log(format!("speed {speed}"));
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            RELEASED.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn open_fake(_: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
        SINK.with(|s| *s.borrow_mut() = Some(sink));
        Ok(Box::new(Fake))
    }

    fn open_nothing(_: &DataSource, _: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
        Err(VideoError::Unsupported("not here"))
    }

    fn send(event: Event) {
        SINK.with(|s| s.borrow().as_ref().expect("opened").send(event));
    }

    fn ready() -> Event {
        Event::Ready {
            duration: Duration::from_secs(10),
            width: 640,
            height: 360,
            texture: Some(TextureId::from_u64(77)),
        }
    }

    /// Polls a future once, by hand.
    fn poll_once<F: Future>(future: &mut Pin<Box<F>>) -> Poll<F::Output> {
        let waker = Waker::noop();
        future.as_mut().poll(&mut Context::from_waker(waker))
    }

    fn controller() -> VideoPlayerController {
        calls();
        VideoPlayerController::with_opener(DataSource::File("clip.mp4".into()), open_fake)
    }

    #[test]
    fn nothing_is_opened_before_initialize() {
        let c = controller();
        c.play();
        c.set_volume(0.5);
        assert!(calls().is_empty());
        assert!(c.value().is_playing);
        assert_eq!(c.value().volume, 0.5);
        assert!(!c.value().is_initialized);
    }

    #[test]
    fn initialize_answers_when_the_player_is_ready_and_applies_what_was_asked() {
        let c = controller();
        c.set_looping(true);
        c.set_volume(0.25);
        c.play();
        let mut init = Box::pin(c.initialize());
        assert!(poll_once(&mut init).is_pending());
        send(ready());
        frus_widgets::run_local_tasks();
        assert_eq!(poll_once(&mut init), Poll::Ready(Ok(())));
        let v = c.value();
        assert!(v.is_initialized);
        assert_eq!(v.duration, Duration::from_secs(10));
        assert_eq!((v.width, v.height), (640, 360));
        assert!((v.aspect_ratio() - 640.0 / 360.0).abs() < 1e-6);
        assert_eq!(c.texture_id(), Some(TextureId::from_u64(77)));
        assert_eq!(calls(), ["looping true", "volume 0.25", "play"]);
    }

    #[test]
    fn an_unsupported_platform_fails_the_initialization_with_its_reason() {
        let c = VideoPlayerController::with_opener(DataSource::File("x".into()), open_nothing);
        let mut init = Box::pin(c.initialize());
        match poll_once(&mut init) {
            Poll::Ready(Err(VideoError::Source(why))) => assert!(why.contains("not here")),
            other => panic!("{other:?}"),
        }
        assert!(c.value().has_error());
        assert!(!c.value().is_playing);
    }

    #[test]
    fn an_error_reported_later_fails_the_initialization() {
        let c = controller();
        let mut init = Box::pin(c.initialize());
        assert!(poll_once(&mut init).is_pending());
        send(Event::Error("no such file".into()));
        frus_widgets::run_local_tasks();
        assert_eq!(
            poll_once(&mut init),
            Poll::Ready(Err(VideoError::Source("no such file".into())))
        );
    }

    #[test]
    fn listeners_hear_each_report_and_positions_coalesce() {
        let c = controller();
        drop(c.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        let heard = Rc::new(Cell::new(0));
        let h = heard.clone();
        let _sub = c.add_listener(move || h.set(h.get() + 1));
        send(Event::Position(Duration::from_millis(250)));
        send(Event::Position(Duration::from_millis(500)));
        frus_widgets::run_local_tasks();
        assert_eq!(heard.get(), 1, "two positions queued, one applied");
        assert_eq!(c.position(), Duration::from_millis(500));
        // The same position again changes nothing and tells nobody.
        send(Event::Position(Duration::from_millis(500)));
        frus_widgets::run_local_tasks();
        assert_eq!(heard.get(), 1);
        send(Event::Buffering(true));
        frus_widgets::run_local_tasks();
        assert!(c.value().is_buffering);
        assert_eq!(heard.get(), 2);
    }

    #[test]
    fn the_end_stops_the_playback_and_play_starts_over() {
        let c = controller();
        drop(c.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        c.play();
        send(Event::Completed);
        frus_widgets::run_local_tasks();
        let v = c.value();
        assert!(v.is_completed && !v.is_playing);
        assert_eq!(v.position, Duration::from_secs(10));
        calls();
        c.play();
        assert_eq!(calls(), ["seek 0", "play"]);
        assert!(!c.value().is_completed);
        assert_eq!(c.position(), Duration::ZERO);
    }

    #[test]
    fn a_looping_clip_does_not_complete() {
        let c = controller();
        drop(c.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        c.set_looping(true);
        c.play();
        send(Event::Completed);
        frus_widgets::run_local_tasks();
        assert!(c.value().is_playing && !c.value().is_completed);
    }

    #[test]
    fn seeking_and_volume_stay_within_bounds() {
        let c = controller();
        drop(c.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        calls();
        c.seek_to(Duration::from_secs(99));
        c.set_volume(3.0);
        c.set_volume(-1.0);
        assert_eq!(calls(), ["seek 10000", "volume 1", "volume 0"]);
        assert_eq!(c.position(), Duration::from_secs(10));
        assert!(c.set_playback_speed(-1.0).is_err());
        assert!(c.set_playback_speed(f32::NAN).is_err());
        assert!(c.set_playback_speed(2.0).is_ok());
        assert_eq!(c.value().playback_speed, 2.0);
    }

    #[test]
    fn the_background_pauses_and_the_return_plays_again() {
        let c = controller();
        drop(c.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        c.play();
        calls();
        lifecycle(false);
        assert!(!c.value().is_playing);
        lifecycle(true);
        assert!(c.value().is_playing);
        assert_eq!(calls(), ["pause", "play"]);
        // A clip the person paused stays paused on the way back.
        c.pause();
        calls();
        lifecycle(false);
        lifecycle(true);
        assert!(!c.value().is_playing);
        assert!(calls().is_empty());
        // One allowed in the background goes on.
        let b = controller().allow_background_playback(true);
        drop(b.initialize());
        send(ready());
        frus_widgets::run_local_tasks();
        b.play();
        lifecycle(false);
        assert!(b.value().is_playing);
    }

    #[test]
    fn the_last_handle_or_dispose_releases_the_player() {
        let before = RELEASED.load(Ordering::SeqCst);
        let c = controller();
        drop(c.initialize());
        let other = c.clone();
        drop(c);
        assert_eq!(RELEASED.load(Ordering::SeqCst), before);
        drop(other);
        assert_eq!(RELEASED.load(Ordering::SeqCst), before + 1);

        let d = controller();
        let mut init = Box::pin(d.initialize());
        d.dispose();
        assert_eq!(RELEASED.load(Ordering::SeqCst), before + 2);
        assert!(d.is_disposed());
        assert_eq!(poll_once(&mut init), Poll::Ready(Err(VideoError::Disposed)));
        // Nothing reaches a released player.
        calls();
        d.play();
        assert!(calls().is_empty());
    }

    #[test]
    fn a_report_from_another_thread_wakes_the_interface() {
        let c = controller();
        drop(c.initialize());
        frus_widgets::run_local_tasks();
        let sink = SINK.with(|s| s.borrow().clone()).expect("opened");
        std::thread::spawn(move || sink.send(ready()))
            .join()
            .expect("the thread");
        frus_widgets::run_local_tasks();
        assert!(c.value().is_initialized);
    }
}
