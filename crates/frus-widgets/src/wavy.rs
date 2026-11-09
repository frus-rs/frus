// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The **wavy** (expressive) progress indicators: [`LinearWavyProgressIndicator`] and
//! [`CircularWavyProgressIndicator`], and the wavy track a slider can take,
//! [`WavySliderTrackShape`].
//!
//! The line that has been covered is drawn as a wave that flows along it; what is left is
//! a flat track, separated from the wave by a gap, ending in a stop dot. Near the start and
//! the end of the work the wave flattens out, so the indicator reads as a plain line when
//! there is little to show and when it is about to finish.
//!
//! An indeterminate indicator, and a determinate one whose wave is flowing, is
//! **decoration**: it moves only to say something is under way, so it is drawn at the
//! decorative frame rate (milestone 783), not at the display's.

use std::f32::consts::{PI, TAU};

use frus_core::{Color, Path, Point, Rect, Scene, Stroke, StrokeCap};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::slidershape::{SliderPaint, SliderTrackShape};
use crate::theme::Theme;
use crate::widget::Widget;

/// The linear indicator's box height: room for the wave's crest and trough around a 4 px
/// line.
pub const WAVY_LINEAR_HEIGHT: f32 = 10.0;
/// The line's thickness, the wave's and the track's.
pub const WAVY_STROKE_WIDTH: f32 = 4.0;
/// How far the wave swings either side of the line's centre on a linear indicator.
pub const WAVY_LINEAR_AMPLITUDE: f32 = 3.0;
/// The distance from one crest to the next on a determinate linear indicator.
pub const WAVY_LINEAR_WAVELENGTH: f32 = 40.0;
/// The same on an indeterminate linear indicator, whose waves are tighter.
pub const WAVY_LINEAR_INDETERMINATE_WAVELENGTH: f32 = 20.0;
/// The circular indicator's side, its padding included.
pub const WAVY_CIRCULAR_SIZE: f32 = 48.0;
/// How far the wave swings either side of the circle on a circular indicator.
pub const WAVY_CIRCULAR_AMPLITUDE: f32 = 1.6;
/// The distance from one crest to the next along a circular indicator's circle.
pub const WAVY_CIRCULAR_WAVELENGTH: f32 = 15.0;
/// The gap between the wave and the track.
pub const WAVY_TRACK_GAP: f32 = 4.0;
/// The stop dot's diameter at the end of a determinate linear track.
pub const WAVY_STOP_SIZE: f32 = 4.0;
/// One cycle of the indeterminate linear indicator's two lines, in seconds.
const LINEAR_INDETERMINATE_PERIOD: f32 = 1.75;

/// Theme defaults for the wavy progress indicators. Every field is `None` until set; a
/// widget asks its own builder first, then this, then the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WavyProgressTheme {
    /// The wave's colour. Unset, `primary`.
    pub color: Option<Color>,
    /// The flat track's colour. Unset, `secondary_container`.
    pub track_color: Option<Color>,
    /// The stop dot's colour. Unset, the wave's colour.
    pub stop_indicator_color: Option<Color>,
    /// The wave's thickness. Unset, four.
    pub stroke_width: Option<f32>,
    /// The track's thickness. Unset, four.
    pub track_stroke_width: Option<f32>,
    /// The gap between the wave and the track. Unset, four.
    pub track_gap: Option<f32>,
    /// A linear indicator's amplitude. Unset, three.
    pub linear_amplitude: Option<f32>,
    /// A determinate linear indicator's wavelength. Unset, forty.
    pub linear_wavelength: Option<f32>,
    /// An indeterminate linear indicator's wavelength. Unset, twenty.
    pub linear_indeterminate_wavelength: Option<f32>,
    /// A linear indicator's box height. Unset, ten.
    pub linear_height: Option<f32>,
    /// A circular indicator's side. Unset, forty-eight.
    pub circular_size: Option<f32>,
    /// A circular indicator's amplitude. Unset, 1.6.
    pub circular_amplitude: Option<f32>,
    /// A circular indicator's wavelength along its circle. Unset, fifteen.
    pub circular_wavelength: Option<f32>,
    /// How many wavelengths the wave travels in a second. Unset, one. Zero holds it still.
    pub wave_speed: Option<f32>,
}

/// **How much of its amplitude the wave has at `progress`**: none at the very start and
/// near the end, all of it in between, with a short ramp at each side rather than a jump.
///
/// ```
/// use frus_widgets::wavy_amplitude;
/// assert_eq!(wavy_amplitude(0.05), 0.0);
/// assert_eq!(wavy_amplitude(0.5), 1.0);
/// assert_eq!(wavy_amplitude(0.99), 0.0);
/// ```
pub fn wavy_amplitude(progress: f32) -> f32 {
    const RAMP: f32 = 0.05;
    let rise = ((progress - 0.1) / RAMP).clamp(0.0, 1.0);
    let fall = ((0.95 - progress) / RAMP).clamp(0.0, 1.0);
    let t = rise.min(fall);
    // Smoothstep, so the wave grows in without a kink.
    t * t * (3.0 - 2.0 * t)
}

/// A horizontal wave from `x0` to `x1` about `cy`: `amplitude` either side, crest to crest
/// `wavelength`, shifted along by `phase` wavelengths. Sampled finely enough that the
/// segments cannot be seen.
pub(crate) fn wave_path(
    x0: f32,
    x1: f32,
    cy: f32,
    amplitude: f32,
    wavelength: f32,
    phase: f32,
) -> Path {
    let len = x1 - x0;
    if amplitude <= 0.0 || wavelength <= 0.0 {
        return Path::new()
            .move_to(Point::new(x0, cy))
            .line_to(Point::new(x1, cy));
    }
    let step = (wavelength / 24.0).clamp(0.5, 2.0);
    let n = ((len.abs() / step).ceil() as usize).max(1);
    let y = |x: f32| cy + amplitude * (TAU * (x / wavelength - phase)).sin();
    let mut path = Path::new().move_to(Point::new(x0, y(x0)));
    for i in 1..=n {
        let x = x0 + len * i as f32 / n as f32;
        path = path.line_to(Point::new(x, y(x)));
    }
    path
}

/// An arc around `center` whose radius swings by `amplitude`, `waves` whole waves to the
/// turn, from `start` sweeping `sweep` radians. `phase` turns the waves along it.
pub(crate) fn wavy_arc_path(
    center: Point,
    radius: f32,
    start: f32,
    sweep: f32,
    amplitude: f32,
    waves: f32,
    phase: f32,
) -> Path {
    let n = ((sweep.abs() * radius / 1.0).ceil() as usize).clamp(2, 720);
    let at = |a: f32| {
        let r = radius + amplitude * (waves * a - TAU * phase).sin();
        Point::new(center.x + r * a.cos(), center.y + r * a.sin())
    };
    let mut path = Path::new().move_to(at(start));
    for i in 1..=n {
        path = path.line_to(at(start + sweep * i as f32 / n as f32));
    }
    path
}

/// A plain arc, for the track.
fn arc_path(center: Point, radius: f32, start: f32, sweep: f32) -> Path {
    Path::new()
        .move_to(Point::new(
            center.x + radius * start.cos(),
            center.y + radius * start.sin(),
        ))
        .arc_to(center, radius, start, start + sweep)
}

/// The resolved look of a wavy indicator, for one paint.
struct Look {
    color: Color,
    track: Color,
    stop: Color,
    stroke: f32,
    track_stroke: f32,
    gap: f32,
    speed: f32,
}

fn look(
    theme: &Theme,
    color: Option<Color>,
    track: Option<Color>,
    stroke: Option<f32>,
    track_stroke: Option<f32>,
    gap: Option<f32>,
    speed: Option<f32>,
) -> Look {
    let t = &theme.widgets.wavy_progress;
    let color = color.or(t.color).unwrap_or(theme.scheme.primary);
    Look {
        color,
        track: track
            .or(t.track_color)
            .unwrap_or(theme.scheme.secondary_container),
        stop: t.stop_indicator_color.unwrap_or(color),
        stroke: stroke.or(t.stroke_width).unwrap_or(WAVY_STROKE_WIDTH),
        track_stroke: track_stroke
            .or(t.track_stroke_width)
            .unwrap_or(WAVY_STROKE_WIDTH),
        gap: gap.or(t.track_gap).unwrap_or(WAVY_TRACK_GAP),
        speed: speed.or(t.wave_speed).unwrap_or(1.0),
    }
}

/// What a progress indicator says to a screen reader: the value from 0 to 100, or only
/// that it is busy.
fn progress_semantics(
    value: Option<f32>,
    label: &Option<String>,
) -> Option<frus_core::SemanticsProperties> {
    let mut s = frus_core::SemanticsProperties::new(frus_core::Role::ProgressBar);
    if let Some(label) = label {
        s = s.label(label.clone());
    }
    if let Some(v) = value {
        let pct = (v * 100.0).round();
        s = s.range(0.0, pct, 100.0).value(format!("{pct}"));
    }
    Some(s)
}

/// A **linear wavy progress indicator**: the work done as a wave that flows along the line,
/// the rest as a flat track ending in a stop dot — determinate ([`new`](Self::new)) or
/// indeterminate ([`indeterminate`](Self::indeterminate)).
///
/// ```
/// use frus_widgets::LinearWavyProgressIndicator;
/// let _done = LinearWavyProgressIndicator::new(0.6).width(240.0);
/// let _busy = LinearWavyProgressIndicator::indeterminate();
/// ```
///
/// **Themed** through [`WavyProgressTheme`] (`theme.widgets.wavy_progress`); every value
/// has a builder of the same name that outranks the theme.
pub struct LinearWavyProgressIndicator {
    value: Option<f32>,
    width: Dimension,
    height: Option<f32>,
    color: Option<Color>,
    track_color: Option<Color>,
    stroke_width: Option<f32>,
    track_stroke_width: Option<f32>,
    amplitude: Option<f32>,
    wavelength: Option<f32>,
    track_gap: Option<f32>,
    wave_speed: Option<f32>,
    stop_indicator: bool,
    semantics_label: Option<String>,
}

impl LinearWavyProgressIndicator {
    /// A **determinate** indicator at `value`, clamped to `0.0..=1.0`.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.3);
    /// ```
    pub fn new(value: f32) -> Self {
        Self::with_value(Some(value))
    }

    /// An **indeterminate** indicator: two wavy lines run along the track.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::indeterminate();
    /// ```
    pub fn indeterminate() -> Self {
        Self::with_value(None)
    }

    /// At `value`, or indeterminate for `None`.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::with_value(None);
    /// ```
    pub fn with_value(value: Option<f32>) -> Self {
        Self {
            value: value.map(|v| v.clamp(0.0, 1.0)),
            width: Dimension::Length(240.0),
            height: None,
            color: None,
            track_color: None,
            stroke_width: None,
            track_stroke_width: None,
            amplitude: None,
            wavelength: None,
            track_gap: None,
            wave_speed: None,
            stop_indicator: true,
            semantics_label: None,
        }
    }

    /// The indicator's width; 240 unless told.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).width(320.0);
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self
    }

    /// Fills the width its parent offers.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).fill_width();
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.width = Dimension::Percent(1.0);
        self
    }

    /// The box's height, room for the wave included; ten unless told.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).height(14.0);
    /// ```
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height.max(0.0));
        self
    }

    /// The wave's colour.
    ///
    /// ```
    /// # use frus_widgets::{Color, LinearWavyProgressIndicator};
    /// let _p = LinearWavyProgressIndicator::new(0.5).color(Color::rgb(0.8, 0.1, 0.3));
    /// ```
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The flat track's colour.
    ///
    /// ```
    /// # use frus_widgets::{Color, LinearWavyProgressIndicator};
    /// let _p = LinearWavyProgressIndicator::new(0.5).track_color(Color::rgb(0.9, 0.9, 0.9));
    /// ```
    pub fn track_color(mut self, color: Color) -> Self {
        self.track_color = Some(color);
        self
    }

    /// The wave's thickness.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).stroke_width(6.0);
    /// ```
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width.max(0.0));
        self
    }

    /// The track's thickness.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).track_stroke_width(2.0);
    /// ```
    pub fn track_stroke_width(mut self, width: f32) -> Self {
        self.track_stroke_width = Some(width.max(0.0));
        self
    }

    /// How far the wave swings either side of the line; zero draws a flat line.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).amplitude(2.0);
    /// ```
    pub fn amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = Some(amplitude.max(0.0));
        self
    }

    /// Crest to crest, in logical pixels.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).wavelength(24.0);
    /// ```
    pub fn wavelength(mut self, wavelength: f32) -> Self {
        self.wavelength = Some(wavelength.max(1.0));
        self
    }

    /// The gap between the wave and the track.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).track_gap(8.0);
    /// ```
    pub fn track_gap(mut self, gap: f32) -> Self {
        self.track_gap = Some(gap.max(0.0));
        self
    }

    /// How many wavelengths the wave flows in a second; zero holds it still.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).wave_speed(0.0);
    /// ```
    pub fn wave_speed(mut self, speed: f32) -> Self {
        self.wave_speed = Some(speed.max(0.0));
        self
    }

    /// Whether a determinate track ends in a stop dot; it does unless told.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).stop_indicator(false);
    /// ```
    pub fn stop_indicator(mut self, show: bool) -> Self {
        self.stop_indicator = show;
        self
    }

    /// What the indicator is the progress of, for screen readers.
    ///
    /// ```
    /// # use frus_widgets::LinearWavyProgressIndicator;
    /// let _p = LinearWavyProgressIndicator::new(0.5).semantics_label("Upload");
    /// ```
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    fn box_height(&self, theme: &Theme) -> f32 {
        self.height
            .or(theme.widgets.wavy_progress.linear_height)
            .unwrap_or(WAVY_LINEAR_HEIGHT)
    }

    fn amplitude_in(&self, theme: &Theme) -> f32 {
        self.amplitude
            .or(theme.widgets.wavy_progress.linear_amplitude)
            .unwrap_or(WAVY_LINEAR_AMPLITUDE)
    }

    fn speed_in(&self, theme: &Theme) -> f32 {
        self.wave_speed
            .or(theme.widgets.wavy_progress.wave_speed)
            .unwrap_or(1.0)
    }

    /// Does anything on it move by itself?
    fn moves(&self, theme: &Theme) -> bool {
        match self.value {
            None => true,
            Some(v) => {
                self.speed_in(theme) > 0.0
                    && self.amplitude_in(theme) > 0.0
                    && wavy_amplitude(v) > 0.0
            }
        }
    }
}

impl<Msg> Widget<Msg> for LinearWavyProgressIndicator {
    fn style(&self) -> Style {
        Widget::<Msg>::style_themed(self, &Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        Style {
            width: self.width,
            height: Dimension::Length(self.box_height(theme)),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        self.moves(&Theme::default()) || self.value.is_none()
    }

    /// Its wave only says that work is under way: drawn at the decorative rate.
    fn decorative(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = &theme.widgets.wavy_progress;
        let l = look(
            theme,
            self.color,
            self.track_color,
            self.stroke_width,
            self.track_stroke_width,
            self.track_gap,
            self.wave_speed,
        );
        let o = status.opacity;
        let rtl = theme.direction.is_rtl();
        let cy = bounds.y + bounds.height * 0.5;
        // The lines' round caps reach half a stroke past their ends, so the ends are held in
        // by that much to stay inside the box.
        let inset = l.stroke.max(l.track_stroke) * 0.5;
        let x0 = bounds.x + inset;
        let span = (bounds.width - 2.0 * inset).max(0.0);
        let x_at = |f: f32| {
            if rtl {
                x0 + span * (1.0 - f)
            } else {
                x0 + span * f
            }
        };
        let max_amp = ((bounds.height - l.stroke) * 0.5).max(0.0);
        let amp = self.amplitude_in(theme).min(max_amp);
        let wave = Stroke::new(l.color.fade(o), l.stroke).with_cap(StrokeCap::Round);
        let track = Stroke::new(l.track.fade(o), l.track_stroke).with_cap(StrokeCap::Round);
        // The gap counts the caps of both lines, as the lines meet cap to cap.
        let gap_f = if span > 0.0 {
            (l.gap + inset * 2.0) / span
        } else {
            0.0
        };
        let draw_track = |scene: &mut Scene, a: f32, b: f32| {
            if b - a > 0.0 {
                let (p, q) = (x_at(a), x_at(b));
                scene.paint_path(
                    &Path::new()
                        .move_to(Point::new(p, cy))
                        .line_to(Point::new(q, cy)),
                    None,
                    Some(track),
                );
            }
        };
        let phase = status.time * l.speed;
        let draw_wave = |scene: &mut Scene, a: f32, b: f32, wavelength: f32, scale: f32| {
            if b - a > 0.0 {
                let (p, q) = (x_at(a), x_at(b));
                // The wave is anchored to the box, not to the line's end, so it flows
                // through the line rather than being dragged by it.
                let ph = if rtl { -phase } else { phase };
                let path = wave_path(p, q, cy, amp * scale, wavelength, ph);
                scene.paint_path(&path, None, Some(wave));
            }
        };
        match self.value {
            Some(v) => {
                let wavelength = self
                    .wavelength
                    .or(t.linear_wavelength)
                    .unwrap_or(WAVY_LINEAR_WAVELENGTH);
                let start = if v > 0.0 { v + gap_f } else { 0.0 };
                draw_track(scene, start, 1.0);
                if self.stop_indicator && start < 1.0 {
                    let r = (WAVY_STOP_SIZE * 0.5).min(bounds.height * 0.5);
                    let c = Point::new(x_at(1.0), cy);
                    scene.draw_rect(
                        Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r),
                        l.stop.fade(o),
                        r,
                        0.0,
                        Color::TRANSPARENT,
                    );
                }
                if v > 0.0 {
                    draw_wave(scene, 0.0, v, wavelength, wavy_amplitude(v));
                }
            }
            None => {
                let wavelength = self
                    .wavelength
                    .or(t.linear_indeterminate_wavelength)
                    .unwrap_or(WAVY_LINEAR_INDETERMINATE_WAVELENGTH);
                let anim = (status.time / LINEAR_INDETERMINATE_PERIOD).rem_euclid(1.0);
                let [head1, tail1, head2, tail2] = crate::progressbar::indeterminate_ends(anim);
                // The track: whatever no line covers, kept a gap away from each line.
                let mut covered: Vec<(f32, f32)> = Vec::new();
                for (tail, head) in [(tail2, head2), (tail1, head1)] {
                    if head - tail > 0.0 {
                        covered.push((tail, head));
                    }
                }
                covered.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut from = 0.0f32;
                for &(a, b) in &covered {
                    let end = if a > 0.0 { a - gap_f } else { 0.0 };
                    draw_track(scene, from, end);
                    from = if b < 1.0 { b + gap_f } else { 1.0 };
                }
                draw_track(scene, from, 1.0);
                for (a, b) in covered {
                    draw_wave(scene, a, b, wavelength, 1.0);
                }
            }
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        progress_semantics(self.value, &self.semantics_label)
    }
}

/// A **circular wavy progress indicator**: the arc of the work done drawn as a wave around
/// the circle, the rest as a flat track — determinate ([`new`](Self::new)) or
/// indeterminate ([`indeterminate`](Self::indeterminate)).
///
/// ```
/// use frus_widgets::CircularWavyProgressIndicator;
/// let _done = CircularWavyProgressIndicator::new(0.6);
/// let _busy = CircularWavyProgressIndicator::indeterminate();
/// ```
///
/// **Themed** through [`WavyProgressTheme`]; each value has a builder that outranks it.
pub struct CircularWavyProgressIndicator {
    value: Option<f32>,
    size: Option<f32>,
    color: Option<Color>,
    track_color: Option<Color>,
    stroke_width: Option<f32>,
    track_stroke_width: Option<f32>,
    amplitude: Option<f32>,
    wavelength: Option<f32>,
    track_gap: Option<f32>,
    wave_speed: Option<f32>,
    semantics_label: Option<String>,
}

impl CircularWavyProgressIndicator {
    /// A **determinate** indicator at `value`, clamped to `0.0..=1.0`.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.3);
    /// ```
    pub fn new(value: f32) -> Self {
        Self::with_value(Some(value))
    }

    /// An **indeterminate** indicator: a wavy arc that grows, shrinks and turns.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::indeterminate();
    /// ```
    pub fn indeterminate() -> Self {
        Self::with_value(None)
    }

    /// At `value`, or indeterminate for `None`.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::with_value(Some(0.5));
    /// ```
    pub fn with_value(value: Option<f32>) -> Self {
        Self {
            value: value.map(|v| v.clamp(0.0, 1.0)),
            size: None,
            color: None,
            track_color: None,
            stroke_width: None,
            track_stroke_width: None,
            amplitude: None,
            wavelength: None,
            track_gap: None,
            wave_speed: None,
            semantics_label: None,
        }
    }

    /// The side of the indicator's square; forty-eight unless told.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).size(64.0);
    /// ```
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size.max(0.0));
        self
    }

    /// The wave's colour.
    ///
    /// ```
    /// # use frus_widgets::{Color, CircularWavyProgressIndicator};
    /// let _p = CircularWavyProgressIndicator::new(0.5).color(Color::rgb(0.8, 0.1, 0.3));
    /// ```
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The flat track's colour.
    ///
    /// ```
    /// # use frus_widgets::{Color, CircularWavyProgressIndicator};
    /// let _p = CircularWavyProgressIndicator::new(0.5).track_color(Color::rgb(0.9, 0.9, 0.9));
    /// ```
    pub fn track_color(mut self, color: Color) -> Self {
        self.track_color = Some(color);
        self
    }

    /// The wave's thickness.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).stroke_width(6.0);
    /// ```
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width.max(0.0));
        self
    }

    /// The track's thickness.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).track_stroke_width(2.0);
    /// ```
    pub fn track_stroke_width(mut self, width: f32) -> Self {
        self.track_stroke_width = Some(width.max(0.0));
        self
    }

    /// How far the wave swings either side of the circle; zero draws a plain arc.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).amplitude(2.0);
    /// ```
    pub fn amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = Some(amplitude.max(0.0));
        self
    }

    /// Crest to crest along the circle, rounded so that a whole number of waves go round.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).wavelength(12.0);
    /// ```
    pub fn wavelength(mut self, wavelength: f32) -> Self {
        self.wavelength = Some(wavelength.max(1.0));
        self
    }

    /// The gap between the wave and the track.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).track_gap(6.0);
    /// ```
    pub fn track_gap(mut self, gap: f32) -> Self {
        self.track_gap = Some(gap.max(0.0));
        self
    }

    /// How many wavelengths the wave flows in a second; zero holds it still.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).wave_speed(0.0);
    /// ```
    pub fn wave_speed(mut self, speed: f32) -> Self {
        self.wave_speed = Some(speed.max(0.0));
        self
    }

    /// What the indicator is the progress of, for screen readers.
    ///
    /// ```
    /// # use frus_widgets::CircularWavyProgressIndicator;
    /// let _p = CircularWavyProgressIndicator::new(0.5).semantics_label("Sync");
    /// ```
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    fn side(&self, theme: &Theme) -> f32 {
        self.size
            .or(theme.widgets.wavy_progress.circular_size)
            .unwrap_or(WAVY_CIRCULAR_SIZE)
    }
}

impl<Msg> Widget<Msg> for CircularWavyProgressIndicator {
    fn style(&self) -> Style {
        Widget::<Msg>::style_themed(self, &Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        let side = self.side(theme);
        Style {
            width: Dimension::Length(side),
            height: Dimension::Length(side),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        match self.value {
            None => true,
            Some(v) => self.wave_speed != Some(0.0) && wavy_amplitude(v) > 0.0,
        }
    }

    /// Decoration: drawn at the decorative rate.
    fn decorative(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = &theme.widgets.wavy_progress;
        let l = look(
            theme,
            self.color,
            self.track_color,
            self.stroke_width,
            self.track_stroke_width,
            self.track_gap,
            self.wave_speed,
        );
        let o = status.opacity;
        let amp = self
            .amplitude
            .or(t.circular_amplitude)
            .unwrap_or(WAVY_CIRCULAR_AMPLITUDE);
        let center = Point::new(
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        // The wave and its stroke stay inside the box.
        let radius = bounds.width.min(bounds.height) * 0.5 - l.stroke * 0.5 - amp;
        if radius <= 0.0 {
            return;
        }
        let wavelength = self
            .wavelength
            .or(t.circular_wavelength)
            .unwrap_or(WAVY_CIRCULAR_WAVELENGTH);
        let waves = (TAU * radius / wavelength).round().max(1.0);
        let phase = status.time * l.speed;
        let wave = Stroke::new(l.color.fade(o), l.stroke).with_cap(StrokeCap::Round);
        let track = Stroke::new(l.track.fade(o), l.track_stroke).with_cap(StrokeCap::Round);
        let start = -PI / 2.0;
        match self.value {
            Some(v) => {
                // The track: the rest of the circle, a stroke and a gap clear of each end.
                let gap = (l.stroke + l.gap) / radius;
                let sweep = if v > 0.0 {
                    TAU * (1.0 - v) - 2.0 * gap
                } else {
                    TAU - 0.001
                };
                if sweep > 0.0 {
                    let from = if v > 0.0 {
                        start + TAU * v + gap
                    } else {
                        start
                    };
                    scene.paint_path(&arc_path(center, radius, from, sweep), None, Some(track));
                }
                if v > 0.0 {
                    let path = wavy_arc_path(
                        center,
                        radius,
                        start,
                        TAU * v,
                        amp * wavy_amplitude(v),
                        waves,
                        phase,
                    );
                    scene.paint_path(&path, None, Some(wave));
                }
            }
            None => {
                let (from, sweep) = crate::spinner::arc(None, status.time);
                let path = wavy_arc_path(center, radius, from, sweep, amp, waves, phase);
                scene.paint_path(&path, None, Some(wave));
            }
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        progress_semantics(self.value, &self.semantics_label)
    }
}

/// A slider track whose **travelled part is a wave**, the rest a flat line: the expressive
/// look of a media player's seek bar.
///
/// ```
/// use frus_widgets::{Slider, WavySliderTrackShape};
/// let _seek: Slider<()> = Slider::new(0.4).track_shape(WavySliderTrackShape::default());
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WavySliderTrackShape {
    /// How far the wave swings either side of the track's centre.
    pub amplitude: f32,
    /// Crest to crest.
    pub wavelength: f32,
}

impl Default for WavySliderTrackShape {
    fn default() -> Self {
        Self {
            amplitude: WAVY_LINEAR_AMPLITUDE,
            wavelength: WAVY_LINEAR_WAVELENGTH,
        }
    }
}

impl SliderTrackShape for WavySliderTrackShape {
    fn paint(
        &self,
        scene: &mut Scene,
        track: Rect,
        thumbs: &[f32],
        _secondary: Option<f32>,
        paint: &SliderPaint<'_>,
    ) {
        let Some(&thumb) = thumbs.first() else {
            return;
        };
        let h = track.height.max(1.0);
        let o = paint.opacity;
        let cy = track.y + track.height * 0.5;
        let rtl = paint.is_rtl();
        let (left, right) = (track.x, track.x + track.width);
        let flat = Stroke::new(paint.palette.inactive_track.fade(o), h).with_cap(StrokeCap::Round);
        let active = Stroke::new(paint.palette.active_track.fade(o), h).with_cap(StrokeCap::Round);
        let (a0, a1, i0, i1) = if rtl {
            (thumb, right, left, thumb)
        } else {
            (left, thumb, thumb, right)
        };
        if i1 - i0 > 0.0 {
            scene.paint_path(
                &Path::new()
                    .move_to(Point::new(i0, cy))
                    .line_to(Point::new(i1, cy)),
                None,
                Some(flat),
            );
        }
        if a1 - a0 > 0.0 {
            let path = wave_path(a0, a1, cy, self.amplitude, self.wavelength, 0.0);
            scene.paint_path(&path, None, Some(active));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::{PathVerb, Primitive};

    fn strokes(widget: &dyn Widget<()>, bounds: Rect, time: f32) -> Vec<(Path, Stroke)> {
        let mut scene = Scene::new();
        let status = Status {
            time,
            ..Default::default()
        };
        widget.paint(bounds, status, &Theme::default(), &mut scene);
        scene
            .primitives()
            .iter()
            .filter_map(|p| match p {
                Primitive::Path {
                    path,
                    stroke: Some(s),
                    ..
                } => Some((path.clone(), *s)),
                _ => None,
            })
            .collect()
    }

    fn ys(path: &Path) -> Vec<f32> {
        path.verbs()
            .iter()
            .filter_map(|v| match v {
                PathVerb::MoveTo(p) | PathVerb::LineTo(p) => Some(p.y),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_amplitude_is_flat_at_the_ends_and_full_in_the_middle() {
        assert_eq!(wavy_amplitude(0.0), 0.0);
        assert_eq!(wavy_amplitude(0.1), 0.0);
        assert_eq!(wavy_amplitude(0.15), 1.0);
        assert_eq!(wavy_amplitude(0.5), 1.0);
        assert_eq!(wavy_amplitude(0.9), 1.0);
        assert_eq!(wavy_amplitude(0.95), 0.0);
        assert_eq!(wavy_amplitude(1.0), 0.0);
        let mid = wavy_amplitude(0.125);
        assert!(mid > 0.0 && mid < 1.0, "ramps in: {mid}");
    }

    #[test]
    fn a_determinate_bar_is_a_wave_then_a_gap_then_a_flat_track_and_a_dot() {
        let bar = LinearWavyProgressIndicator::new(0.5).width(200.0);
        let bounds = Rect::new(0.0, 0.0, 200.0, 10.0);
        let s = strokes(&bar, bounds, 0.0);
        assert_eq!(s.len(), 2, "track then wave");
        let theme = Theme::default();
        let (track, wave) = (&s[0], &s[1]);
        assert_eq!(track.1.color, theme.scheme.secondary_container);
        assert_eq!(wave.1.color, theme.scheme.primary);
        assert_eq!((wave.1.width, wave.1.cap), (4.0, StrokeCap::Round));
        // The track is flat; the wave swings 3 px either side of the centre line.
        assert!(ys(&track.0).iter().all(|y| (*y - 5.0).abs() < 1e-4));
        let w = ys(&wave.0);
        let (lo, hi) = w
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
        assert!(
            (hi - 8.0).abs() < 0.05 && (lo - 2.0).abs() < 0.05,
            "{lo}..{hi}"
        );
        // The wave ends where the value is; the track starts a gap (and two half caps) on.
        let end = match wave.0.verbs().last() {
            Some(PathVerb::LineTo(p)) => p.x,
            _ => unreachable!(),
        };
        let start = match track.0.verbs()[0] {
            PathVerb::MoveTo(p) => p.x,
            _ => unreachable!(),
        };
        assert!((end - (2.0 + 196.0 * 0.5)).abs() < 1e-3);
        assert!(
            (start - end - (4.0 + 4.0)).abs() < 1e-3,
            "gap {}",
            start - end
        );
        // And the stop dot at the far end.
        let mut scene = Scene::new();
        Widget::<()>::paint(&bar, bounds, Status::default(), &theme, &mut scene);
        assert!(scene
            .primitives()
            .iter()
            .any(|p| matches!(p, Primitive::Rect { rect, .. } if (rect.x + rect.width * 0.5 - 198.0).abs() < 1e-3)));
    }

    #[test]
    fn near_the_start_the_wave_is_a_flat_line() {
        let bar = LinearWavyProgressIndicator::new(0.05).width(200.0);
        let s = strokes(&bar, Rect::new(0.0, 0.0, 200.0, 10.0), 0.3);
        let wave = s.last().unwrap();
        assert!(ys(&wave.0).iter().all(|y| (*y - 5.0).abs() < 1e-4));
    }

    #[test]
    fn the_wave_flows_with_time() {
        let bar = LinearWavyProgressIndicator::new(0.5).width(200.0);
        let b = Rect::new(0.0, 0.0, 200.0, 10.0);
        let a = ys(&strokes(&bar, b, 0.0)[1].0);
        let c = ys(&strokes(&bar, b, 0.25)[1].0);
        assert_ne!(a, c, "a quarter second on, the wave has moved");
        // A whole period later it is where it was.
        let d = ys(&strokes(&bar, b, 1.0)[1].0);
        for (x, y) in a.iter().zip(&d) {
            assert!((x - y).abs() < 1e-3);
        }
        assert!(Widget::<()>::continuous(&bar));
        assert!(Widget::<()>::decorative(&bar));
        assert!(!Widget::<()>::continuous(
            &LinearWavyProgressIndicator::new(0.5).wave_speed(0.0)
        ));
    }

    #[test]
    fn an_indeterminate_bar_is_decoration_and_moves() {
        let bar = LinearWavyProgressIndicator::indeterminate();
        assert!(Widget::<()>::continuous(&bar));
        assert!(Widget::<()>::decorative(&bar));
        let s = strokes(&bar, Rect::new(0.0, 0.0, 240.0, 10.0), 0.4);
        assert!(s
            .iter()
            .any(|(_, st)| st.color == Theme::default().scheme.primary));
        let circle = CircularWavyProgressIndicator::indeterminate();
        assert!(Widget::<()>::continuous(&circle));
        assert!(Widget::<()>::decorative(&circle));
    }

    #[test]
    fn a_circular_wave_closes_on_a_whole_number_of_waves() {
        let ring = CircularWavyProgressIndicator::new(0.999).wave_speed(0.0);
        let b = Rect::new(0.0, 0.0, 48.0, 48.0);
        let s = strokes(&ring, b, 0.0);
        // At the very end the wave is flat: every point on the circle.
        let wave = s.last().unwrap();
        let r = 24.0 - 2.0 - WAVY_CIRCULAR_AMPLITUDE;
        for v in wave.0.verbs() {
            if let PathVerb::LineTo(p) | PathVerb::MoveTo(p) = v {
                let d = ((p.x - 24.0).powi(2) + (p.y - 24.0).powi(2)).sqrt();
                assert!((d - r).abs() < 1e-3);
            }
        }
        // Half way, the wave swings by its amplitude about that circle.
        let s = strokes(&CircularWavyProgressIndicator::new(0.5), b, 0.0);
        let wave = s.last().unwrap();
        let ds: Vec<f32> = wave
            .0
            .verbs()
            .iter()
            .filter_map(|v| match v {
                PathVerb::LineTo(p) => Some(((p.x - 24.0).powi(2) + (p.y - 24.0).powi(2)).sqrt()),
                _ => None,
            })
            .collect();
        let max = ds.iter().cloned().fold(f32::MIN, f32::max);
        let min = ds.iter().cloned().fold(f32::MAX, f32::min);
        assert!((max - (r + WAVY_CIRCULAR_AMPLITUDE)).abs() < 0.05);
        assert!((min - (r - WAVY_CIRCULAR_AMPLITUDE)).abs() < 0.05);
    }

    #[test]
    fn the_theme_and_the_caller_are_heard() {
        let mut theme = Theme::default();
        theme.widgets.wavy_progress.color = Some(Color::rgb(1.0, 0.0, 0.0));
        theme.widgets.wavy_progress.linear_height = Some(16.0);
        let bar = LinearWavyProgressIndicator::new(0.5);
        assert_eq!(
            Widget::<()>::style_themed(&bar, &theme).height,
            Dimension::Length(16.0)
        );
        let mut scene = Scene::new();
        Widget::<()>::paint(
            &bar.color(Color::rgb(0.0, 0.0, 1.0)),
            Rect::new(0.0, 0.0, 240.0, 16.0),
            Status::default(),
            &theme,
            &mut scene,
        );
        assert!(scene.primitives().iter().any(|p| matches!(p,
            Primitive::Path { stroke: Some(s), .. } if s.color == Color::rgb(0.0, 0.0, 1.0))));
    }

    #[test]
    fn a_reader_hears_the_value() {
        let s = Widget::<()>::semantics(
            &LinearWavyProgressIndicator::new(0.42).semantics_label("Copy"),
        )
        .unwrap();
        assert_eq!(s.value.as_deref(), Some("42"));
        assert_eq!(s.label.as_deref(), Some("Copy"));
        assert_eq!(
            Widget::<()>::semantics(&CircularWavyProgressIndicator::indeterminate())
                .unwrap()
                .value,
            None
        );
    }
}
