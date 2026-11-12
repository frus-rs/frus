// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The widgets that show a [`VideoPlayerController`]: [`VideoPlayer`] (the picture),
//! [`VideoProgressIndicator`] (where the playback is, and a bar to drag it along) and
//! [`VideoControls`] (an overlay of both, with a play button, that hides itself while the clip
//! plays). Each is a component: `.into_widget()` makes it something a `column![..]` holds.

use std::rc::Rc;
use std::time::Duration;

use frus_widgets::{
    row, text, BoxFit, BuildContext, Callback, CircularProgressIndicator, Container,
    GestureDetector, IconButton, Icons, LayoutBuilder, Listener, ListenerEventKind, Positioned,
    Semantics, Stack, StackFit, StatelessWidget, Texture, Theme, UseRef, Widget,
};
use frus_widgets::{Color, Role, SemanticsProperties};

use super::controller::{VideoPlayerController, VideoPlayerValue};

/// **The picture of a clip**: its controller's texture, fitted into the room it is given.
///
/// Before the clip is open it shows its background only (none by default). The picture keeps
/// its proportions ([`BoxFit::Contain`]) unless another fit is asked for.
///
/// ```no_run
/// use frus_shell::{VideoPlayer, VideoPlayerController};
/// use frus_widgets::{BoxFit, Color, StatelessWidget};
///
/// let controller = VideoPlayerController::asset(include_bytes!("../../../frus-demo/assets/texture-demo.mp4"));
/// let picture = VideoPlayer::new(&controller)
///     .fit(BoxFit::Cover)
///     .background(Color::BLACK)
///     .into_widget();
/// # let _ = picture;
/// ```
pub struct VideoPlayer {
    controller: VideoPlayerController,
    fit: BoxFit,
    background: Option<Color>,
    label: Option<String>,
}

impl VideoPlayer {
    /// The picture of `controller`'s clip.
    pub fn new(controller: &VideoPlayerController) -> Self {
        Self {
            controller: controller.clone(),
            fit: BoxFit::Contain,
            background: None,
            label: None,
        }
    }

    /// How the picture fills the room: [`BoxFit::Contain`] unless told otherwise.
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// What is painted behind the picture, and around it when its proportions are not the
    /// room's: nothing unless told.
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// What a screen reader says of the picture. "Video" unless told otherwise.
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl StatelessWidget for VideoPlayer {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let label = self.label.clone().unwrap_or_else(|| "Video".into());
        match self.controller.texture_id() {
            Some(id) => {
                let mut picture = Texture::new(id).fit(self.fit).semantic_label(label);
                if let Some(color) = self.background {
                    picture = picture.background(color);
                }
                Box::new(picture)
            }
            None => {
                let mut empty = Container::new().flex(1.0);
                if let Some(color) = self.background {
                    empty = empty.color(color);
                }
                Box::new(Semantics::new(
                    SemanticsProperties::new(Role::Image).label(label),
                    empty,
                ))
            }
        }
    }
}

/// The colours of a [`VideoProgressIndicator`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoProgressColors {
    /// The part already played.
    pub played: Color,
    /// The part downloaded ahead.
    pub buffered: Color,
    /// The rest of the track.
    pub background: Color,
}

impl VideoProgressColors {
    /// The theme's: its primary colour for what was played, the same, fainter, for what is
    /// downloaded, and its muted colour, fainter still, for the track.
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            played: theme.primary,
            buffered: theme.primary.with_alpha(0.35),
            background: theme.muted.with_alpha(0.3),
        }
    }
}

/// **Where the playback is**: a thin track, what was played and what is downloaded drawn
/// along it. With scrubbing allowed (the default), a press or a drag on it moves the
/// playback there, pausing the clip while the finger is down.
///
/// ```no_run
/// use frus_shell::{VideoPlayerController, VideoProgressColors, VideoProgressIndicator};
/// use frus_widgets::{Color, StatelessWidget};
///
/// let controller = VideoPlayerController::network_url("https://example.com/clip.mp4");
/// let bar = VideoProgressIndicator::new(&controller)
///     .colors(VideoProgressColors {
///         played: Color::rgb(1.0, 0.0, 0.0),
///         buffered: Color::rgba(1.0, 1.0, 1.0, 0.4),
///         background: Color::rgba(1.0, 1.0, 1.0, 0.15),
///     })
///     .bar_height(4.0)
///     .into_widget();
/// # let _ = bar;
/// ```
pub struct VideoProgressIndicator {
    controller: VideoPlayerController,
    colors: Option<VideoProgressColors>,
    allow_scrubbing: bool,
    bar_height: f32,
    padding: f32,
}

impl VideoProgressIndicator {
    /// The progress of `controller`'s clip.
    pub fn new(controller: &VideoPlayerController) -> Self {
        Self {
            controller: controller.clone(),
            colors: None,
            allow_scrubbing: true,
            bar_height: 5.0,
            padding: 5.0,
        }
    }

    /// Its colours; the theme's ([`VideoProgressColors::from_theme`]) unless told.
    pub fn colors(mut self, colors: VideoProgressColors) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Whether a press or a drag moves the playback. On by default.
    pub fn allow_scrubbing(mut self, allow: bool) -> Self {
        self.allow_scrubbing = allow;
        self
    }

    /// How thick the track is, in logical pixels: 5 by default.
    pub fn bar_height(mut self, height: f32) -> Self {
        self.bar_height = height.max(0.0);
        self
    }

    /// The room above and below the track, which a finger also presses: 5 by default.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.max(0.0);
        self
    }
}

/// What the scrubbing keeps while the finger is down: whether the clip was playing.
#[derive(Clone, Copy, Default)]
struct Scrub {
    active: bool,
    was_playing: bool,
}

fn seek_fraction(controller: &VideoPlayerController, fraction: f32) {
    let duration = controller.with_value(|v| v.duration);
    if duration.is_zero() {
        return;
    }
    controller.seek_to(duration.mul_f64(fraction.clamp(0.0, 1.0) as f64));
}

impl StatelessWidget for VideoProgressIndicator {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let scrub = cx.use_ref(Scrub::default);
        self.track(cx.theme(), scrub, None)
    }
}

impl VideoProgressIndicator {
    /// The track itself: a layout builder (for the width a press is a fraction of) holding
    /// the bars, announced as a slider, heard by a listener when scrubbing is allowed. `grow`
    /// is its flex factor, for a place in a row.
    fn track(&self, theme: &Theme, scrub: UseRef<Scrub>, grow: Option<f32>) -> Box<dyn Widget> {
        let colors = self
            .colors
            .unwrap_or_else(|| VideoProgressColors::from_theme(theme));
        let controller = self.controller.clone();
        let (bar, padding, scrubbing) = (self.bar_height, self.padding, self.allow_scrubbing);
        let value = controller.value();
        // The width the layout gave the track, for what fraction of it a press is at.
        let laid_out = Rc::new(std::cell::Cell::new(0.0f32));
        let measured = laid_out.clone();
        let mut track = LayoutBuilder::new(move |size| {
            let semantics = SemanticsProperties::new(Role::Slider)
                .label("Video progress")
                .value(format!(
                    "{} of {}",
                    format_duration(value.position),
                    format_duration(value.duration)
                ));
            let width = size.width.max(0.0);
            measured.set(width);
            let (played, buffered) = fractions(&value);
            let mut layers = Stack::new().fit(StackFit::Expand).height(bar);
            layers = layers.layer(Container::new().height(bar).color(colors.background));
            for (start, end) in buffered.iter().copied() {
                layers = layers.layer(
                    Positioned::new(
                        Container::new()
                            .width((end - start) * width)
                            .height(bar)
                            .color(colors.buffered),
                    )
                    .left(start * width)
                    .top(0.0),
                );
            }
            layers = layers.layer(
                Positioned::new(
                    Container::new()
                        .width(played * width)
                        .height(bar)
                        .color(colors.played),
                )
                .left(0.0)
                .top(0.0),
            );
            let padded = Container::new()
                .padding_each(padding, 0.0, padding, 0.0)
                .child(layers);
            Semantics::new(semantics, padded)
        });
        if let Some(grow) = grow {
            track = track.flex(grow);
        }
        if !scrubbing {
            return Box::new(track);
        }
        Box::new(Listener::new(track, move |event| {
            let width = laid_out.get();
            let fraction = if width > 0.0 {
                event.local.x / width
            } else {
                0.0
            };
            let (controller, scrub) = (controller.clone(), scrub.clone());
            match event.kind {
                ListenerEventKind::Down => Some(Callback::new(move || {
                    let playing = controller.with_value(|v| v.is_playing);
                    scrub.set(Scrub {
                        active: true,
                        was_playing: playing,
                    });
                    if playing {
                        controller.pause();
                    }
                    seek_fraction(&controller, fraction);
                })),
                ListenerEventKind::Move if scrub.with(|s| s.active) => {
                    Some(Callback::new(move || seek_fraction(&controller, fraction)))
                }
                ListenerEventKind::Up | ListenerEventKind::Cancel => {
                    Some(Callback::new(move || {
                        let was = scrub.with(|s| s.active && s.was_playing);
                        scrub.set(Scrub::default());
                        if was && !controller.with_value(|v| v.is_completed) {
                            controller.play();
                        }
                    }))
                }
                _ => None,
            }
        }))
    }
}

/// What was played, as a fraction, and the downloaded stretches, as fractions.
fn fractions(value: &VideoPlayerValue) -> (f32, Vec<(f32, f32)>) {
    let buffered = value
        .buffered
        .iter()
        .map(|r| {
            (
                r.start_fraction(value.duration),
                r.end_fraction(value.duration),
            )
        })
        .filter(|(s, e)| e > s)
        .collect();
    (value.progress(), buffered)
}

/// `m:ss`, or `h:mm:ss` from an hour.
///
/// ```ignore
/// assert_eq!(format_duration(Duration::from_secs(75)), "1:15");
/// ```
pub(crate) fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// How a [`VideoControls`] overlay looks and behaves. Every field has a default; change the
/// ones that matter: `VideoControlsStyle { scrim: Color::TRANSPARENT, ..Default::default() }`.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoControlsStyle {
    /// The icons' and the times' colour. White by default.
    pub foreground: Color,
    /// What dims the picture while the controls show. Black at 35% by default.
    pub scrim: Color,
    /// The play button's icon size. 56 by default.
    pub icon_size: f32,
    /// The progress bar's colours; the theme's unless set.
    pub progress: Option<VideoProgressColors>,
    /// Whether the position and the duration are written beside the bar. On by default.
    pub show_time: bool,
    /// How long the controls stay while the clip plays, once nothing touched them; `None`
    /// keeps them. Three seconds by default.
    pub hide_after: Option<Duration>,
    /// The room around the bottom bar. 12 by default.
    pub padding: f32,
}

impl Default for VideoControlsStyle {
    fn default() -> Self {
        Self {
            foreground: Color::WHITE,
            scrim: Color::rgba(0.0, 0.0, 0.0, 0.35),
            icon_size: 56.0,
            progress: None,
            show_time: true,
            hide_after: Some(Duration::from_secs(3)),
            padding: 12.0,
        }
    }
}

type PartBuilder = Rc<dyn Fn(&VideoPlayerController) -> Box<dyn Widget>>;

/// **A default overlay of controls** for a clip: a play/pause button in the middle (a replay
/// button at the end, a spinner while it waits for data), the progress bar with the position
/// and the duration along the bottom, over a scrim. A tap shows or hides them; while the clip
/// plays they hide themselves after three seconds.
///
/// Put it over a [`VideoPlayer`] in a stack. Everything can be changed: the look with
/// [`style`](Self::style), the middle and the bottom with [`center`](Self::center) and
/// [`bottom`](Self::bottom) — or leave this widget out and build controls of your own from the
/// controller.
///
/// ```no_run
/// use frus_shell::{VideoControls, VideoControlsStyle, VideoPlayer, VideoPlayerController};
/// use frus_widgets::{Positioned, Stack, StackFit, StatelessWidget};
///
/// let controller = VideoPlayerController::network_url("https://example.com/clip.mp4");
/// let player = Stack::new()
///     .fit(StackFit::Expand)
///     .layer(VideoPlayer::new(&controller).into_widget())
///     .layer(Positioned::fill(
///         VideoControls::new(&controller)
///             .style(VideoControlsStyle { show_time: false, ..Default::default() })
///             .into_widget(),
///     ));
/// # let _ = player;
/// ```
pub struct VideoControls {
    controller: VideoPlayerController,
    style: VideoControlsStyle,
    center: Option<PartBuilder>,
    bottom: Option<PartBuilder>,
}

impl VideoControls {
    /// The default controls of `controller`'s clip.
    pub fn new(controller: &VideoPlayerController) -> Self {
        Self {
            controller: controller.clone(),
            style: VideoControlsStyle::default(),
            center: None,
            bottom: None,
        }
    }

    /// The look and the timing.
    pub fn style(mut self, style: VideoControlsStyle) -> Self {
        self.style = style;
        self
    }

    /// Builds the middle of the overlay instead of the play button.
    pub fn center(
        mut self,
        build: impl Fn(&VideoPlayerController) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.center = Some(Rc::new(build));
        self
    }

    /// Builds the bottom of the overlay instead of the progress bar and the times.
    pub fn bottom(
        mut self,
        build: impl Fn(&VideoPlayerController) -> Box<dyn Widget> + 'static,
    ) -> Self {
        self.bottom = Some(Rc::new(build));
        self
    }
}

impl StatelessWidget for VideoControls {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let style = &self.style;
        let c = &self.controller;
        let v = c.value();
        let shown = cx.use_state(|| true);
        let scrub = cx.use_ref(Scrub::default);
        // While it plays and shows, the controls go after a while; a tap brings them back.
        if v.is_playing && shown.get() {
            if let Some(after) = style.hide_after {
                let hide = shown.clone();
                cx.use_interval(after, move || hide.set(false));
            }
        }
        let visible = shown.get() || !v.is_playing;
        let toggle = {
            let shown = shown.clone();
            Callback::new(move || shown.set(!shown.get()))
        };
        let mut stack = Stack::new().fit(StackFit::Expand);
        let mut scrim = Container::new();
        if visible {
            scrim = scrim.color(style.scrim);
        }
        stack = stack.layer(Positioned::fill(scrim));
        if v.is_buffering && !v.has_error() {
            stack = stack.layer(frus_widgets::Center::new(
                CircularProgressIndicator::new().color(style.foreground),
            ));
        } else if visible {
            let middle = match &self.center {
                Some(build) => build(c),
                None => Box::new(play_button(c, &v, style)),
            };
            stack = stack.layer(frus_widgets::Center::new(middle));
        }
        if visible {
            let bottom = match &self.bottom {
                Some(build) => build(c),
                None => bottom_bar(c, &v, style, cx.theme(), scrub),
            };
            // A tap on the bar is the bar's: it does not also hide the controls.
            let bottom = GestureDetector::new(bottom).on_tap(Callback::new(|| {}));
            stack = stack.layer(Positioned::new(bottom).left(0.0).right(0.0).bottom(0.0));
        }
        // The whole area hears the tap that shows or hides the controls; the buttons inside
        // it take their own taps first.
        Box::new(GestureDetector::new(stack).on_tap(toggle))
    }
}

fn play_button(
    c: &VideoPlayerController,
    v: &VideoPlayerValue,
    style: &VideoControlsStyle,
) -> IconButton {
    let (icon, label) = if v.is_completed {
        (Icons::REPLAY, "Replay")
    } else if v.is_playing {
        (Icons::PAUSE, "Pause")
    } else {
        (Icons::PLAY_ARROW, "Play")
    };
    let controller = c.clone();
    IconButton::new(icon)
        .label(label)
        .icon_size(style.icon_size)
        .size(style.icon_size + 16.0)
        .icon_color(style.foreground)
        .on_press(Callback::new(move || {
            if controller.with_value(|v| v.is_playing) {
                controller.pause();
            } else {
                controller.play();
            }
        }))
}

fn bottom_bar(
    c: &VideoPlayerController,
    v: &VideoPlayerValue,
    style: &VideoControlsStyle,
    theme: &Theme,
    scrub: UseRef<Scrub>,
) -> Box<dyn Widget> {
    let mut progress = VideoProgressIndicator::new(c);
    if let Some(colors) = style.progress {
        progress = progress.colors(colors);
    }
    // The track itself, not its component: it takes the row's room.
    let bar = progress.track(theme, scrub, Some(1.0));
    let content: Box<dyn Widget> = if style.show_time {
        Box::new(
            row![
                text(format_duration(v.position))
                    .size(13.0)
                    .color(style.foreground),
                bar,
                text(format_duration(v.duration))
                    .size(13.0)
                    .color(style.foreground),
            ]
            .gap(10.0)
            // A row hugs its children; this one spans the overlay, the bar taking the rest.
            .width_fraction(1.0),
        )
    } else {
        Box::new(row![bar].width_fraction(1.0))
    };
    Box::new(Container::new().padding(style.padding).child(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_a_clock() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_millis(75_900)), "1:15");
        assert_eq!(format_duration(Duration::from_secs(3_725)), "1:02:05");
    }

    #[test]
    fn the_track_shows_what_was_played_and_what_is_downloaded() {
        use super::super::controller::DurationRange;
        let value = VideoPlayerValue {
            duration: Duration::from_secs(100),
            position: Duration::from_secs(25),
            buffered: vec![
                DurationRange::new(Duration::ZERO, Duration::from_secs(50)),
                DurationRange::new(Duration::from_secs(60), Duration::from_secs(60)),
            ],
            ..Default::default()
        };
        let (played, buffered) = fractions(&value);
        assert_eq!(played, 0.25);
        assert_eq!(buffered, vec![(0.0, 0.5)], "an empty stretch is not drawn");
        let (played, buffered) = fractions(&VideoPlayerValue::default());
        assert_eq!(played, 0.0);
        assert!(buffered.is_empty());
    }

    use super::super::controller::{DataSource, Event, Player, Sink, VideoError};
    use crate::app::testing::Driver;
    use frus_widgets::{Point, Positioned, TextureId};
    use std::cell::RefCell;
    use std::sync::Arc;

    thread_local! {
        static SINK: RefCell<Option<Arc<Sink>>> = const { RefCell::new(None) };
    }

    /// Reports from the fake player, as the platform's thread sends them.
    fn report(event: Event) {
        SINK.with(|s| s.borrow().as_ref().expect("opened").send(event));
        frus_widgets::run_local_tasks();
    }

    /// A ten-second clip, open and playing, under the default controls in a 400 x 300 window.
    fn playing_under_controls() -> (VideoPlayerController, Driver<crate::FrusApp>) {
        use crate::FrusApp;
        struct Silent;
        impl Player for Silent {
            fn play(&self) {}
            fn pause(&self) {}
            fn seek_to(&self, _: Duration) {}
            fn set_volume(&self, _: f32) {}
            fn set_looping(&self, _: bool) {}
            fn set_playback_speed(&self, _: f32) {}
        }
        fn open(_: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
            SINK.with(|s| *s.borrow_mut() = Some(sink));
            Ok(Box::new(Silent))
        }

        let c = VideoPlayerController::with_opener(DataSource::File("x.mp4".into()), open);
        drop(c.initialize());
        report(Event::Ready {
            duration: Duration::from_secs(10),
            width: 640,
            height: 360,
            texture: Some(TextureId::from_u64(1)),
        });
        c.play();
        let shown = c.clone();
        let app = FrusApp::from_fn(move |_| {
            Box::new(
                Stack::new()
                    .fit(StackFit::Expand)
                    .width(400.0)
                    .height(300.0)
                    .layer(Positioned::fill(VideoControls::new(&shown).into_widget())),
            )
        });
        let mut driver = Driver::new(app, 400.0, 300.0);
        for _ in 0..3 {
            driver.frame(1.0 / 60.0);
        }
        (c, driver)
    }

    /// The overlay driven through the shell, as on the phone where two bugs were found: the
    /// play button must take its tap (a tap area under it took it instead), and the progress
    /// bar must have the room between the two times (it had none, the times touching).
    #[test]
    fn the_controls_pause_and_the_bar_seeks_where_it_is_pressed() {
        let (c, mut driver) = playing_under_controls();
        let texts = driver.texts();
        let rect = |label: &str| {
            texts
                .iter()
                .find(|(t, _)| t == label)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no {label:?} in {texts:?}"))
        };
        let (start, end) = (rect("0:00"), rect("0:10"));
        let gap = end.x - (start.x + start.width);
        assert!(gap > 200.0, "the bar has the room between the times: {gap}");

        // The play button, in the middle, pauses.
        let middle = Point::new(200.0, 150.0);
        driver.press(middle);
        driver.release(middle);
        driver.frame(1.0 / 60.0);
        if c.value().is_playing {
            let rects: Vec<_> = driver
                .frame_parts()
                .map(|(ui, _)| ui.hit_rects().collect())
                .unwrap_or_default();
            let by_label = driver.assistive_click("Pause");
            panic!(
                "the button did not take the tap: hit {:?}, rects {rects:?}, by label {by_label} -> playing {}",
                driver.hit(middle),
                c.value().is_playing
            );
        }

        // A press halfway along the bar seeks to the middle.
        let at = Point::new(
            start.x + start.width + gap / 2.0,
            start.y + start.height / 2.0,
        );
        driver.press(at);
        driver.release(at);
        driver.frame(1.0 / 60.0);
        let position = c.position().as_secs_f32();
        assert!(
            (position - 5.0).abs() < 0.6,
            "seeked to {position}; pressed at {at:?}, texts {:?}",
            driver.texts()
        );
    }

    /// As on the phone: the controls hidden, a tap brings them back, the clip reports its
    /// position while they show, then a tap on the button in the middle pauses.
    #[test]
    fn the_button_pauses_after_a_tap_brought_the_controls_back() {
        let (c, mut driver) = playing_under_controls();
        let corner = Point::new(30.0, 30.0);
        let middle = Point::new(200.0, 150.0);
        let tap = |driver: &mut Driver<crate::FrusApp>, at: Point| {
            driver.press(at);
            driver.frame(1.0 / 60.0);
            driver.release(at);
            driver.frame(1.0 / 60.0);
        };
        // Hidden.
        tap(&mut driver, corner);
        assert!(c.value().is_playing);
        // Back.
        tap(&mut driver, corner);
        for i in 1..=6 {
            report(Event::Position(Duration::from_millis(250 * i)));
            for _ in 0..15 {
                driver.frame(1.0 / 60.0);
            }
        }
        assert!(c.value().is_playing, "nothing paused it yet");
        tap(&mut driver, middle);
        assert!(
            !c.value().is_playing,
            "the tap on the button did not pause; hit {:?}",
            driver.hit(middle)
        );
    }
}
