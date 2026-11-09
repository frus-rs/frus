// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`LoadingIndicator`]: the expressive loading indicator — a shape that **morphs through
//! a sequence of rounded polygons** while it turns, for waits too short for a progress
//! indicator to say anything useful.
//!
//! Each morph takes 650 ms on a bouncy spring, and every morph turns the shape a further
//! quarter turn on top of a slow steady spin. It is decoration: it moves at the decorative
//! frame rate (milestone 783).

use std::cell::OnceCell;
use std::f32::consts::{FRAC_PI_2, TAU};

use frus_core::{Color, Rect, Scene};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::shapes::{MaterialShapes, Morph, RoundedPolygon};
use crate::theme::Theme;
use crate::widget::Widget;

/// The indicator's box.
pub const LOADING_INDICATOR_SIZE: f32 = 48.0;
/// The shape's size inside it.
pub const LOADING_INDICATOR_ACTIVE_SIZE: f32 = 38.0;
/// One morph, in seconds.
const MORPH_SECONDS: f32 = 0.65;
/// One turn of the steady spin, in seconds.
const SPIN_SECONDS: f32 = 4.666;

/// Theme defaults for [`LoadingIndicator`]; each `None` falls through to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadingIndicatorTheme {
    /// The shape's colour. Unset, `primary` (`on_primary_container` when contained).
    pub color: Option<Color>,
    /// The circle behind a contained indicator. Unset, `primary_container`.
    pub container_color: Option<Color>,
    /// The box's side. Unset, forty-eight.
    pub size: Option<f32>,
    /// The shape's size. Unset, thirty-eight.
    pub active_size: Option<f32>,
}

/// How far along a morph is, `t` seconds into it: the step response of a spring with a
/// damping ratio of 0.6 and a stiffness of 200, which overshoots by about a tenth and has
/// settled by the end of the 650 ms.
///
/// ```
/// use frus_widgets::loading_spring;
/// assert_eq!(loading_spring(0.0), 0.0);
/// assert!(loading_spring(0.25) > 1.0); // the overshoot
/// assert!((loading_spring(0.65) - 1.0).abs() < 0.01);
/// ```
pub fn loading_spring(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    let omega = 200f32.sqrt();
    let zeta = 0.6f32;
    let wd = omega * (1.0 - zeta * zeta).sqrt();
    1.0 - (-zeta * omega * t).exp() * ((wd * t).cos() + zeta * omega / wd * (wd * t).sin())
}

/// The shapes an indeterminate indicator morphs through, in order.
pub fn loading_shapes() -> Vec<RoundedPolygon> {
    vec![
        MaterialShapes::soft_burst(),
        MaterialShapes::cookie9(),
        MaterialShapes::pentagon(),
        MaterialShapes::pill(),
        MaterialShapes::sunny(),
        MaterialShapes::cookie4(),
        MaterialShapes::oval(),
    ]
}

/// The **expressive loading indicator**: a shape morphing through the Material 3 shapes as
/// it turns — indeterminate ([`new`](Self::new)) or following a value
/// ([`determinate`](Self::determinate)).
///
/// ```
/// use frus_widgets::LoadingIndicator;
/// let _busy = LoadingIndicator::new();
/// let _contained = LoadingIndicator::new().contained(true);
/// let _half = LoadingIndicator::determinate(0.5);
/// ```
///
/// **Themed** through [`LoadingIndicatorTheme`] (`theme.widgets.loading_indicator`).
pub struct LoadingIndicator {
    value: Option<f32>,
    shapes: Option<Vec<RoundedPolygon>>,
    contained: bool,
    color: Option<Color>,
    container_color: Option<Color>,
    size: Option<f32>,
    semantics_label: Option<String>,
    morphs: OnceCell<Vec<Morph>>,
}

impl LoadingIndicator {
    /// An indeterminate indicator.
    pub fn new() -> Self {
        Self {
            value: None,
            shapes: None,
            contained: false,
            color: None,
            container_color: None,
            size: None,
            semantics_label: None,
            morphs: OnceCell::new(),
        }
    }

    /// A determinate indicator: the first shape turning into the second as `value` goes
    /// from zero to one.
    pub fn determinate(value: f32) -> Self {
        let mut s = Self::new();
        s.value = Some(value.clamp(0.0, 1.0));
        s
    }

    /// The shapes to morph through, in order (at least two).
    ///
    /// ```
    /// use frus_widgets::{LoadingIndicator, MaterialShapes};
    /// let _l = LoadingIndicator::new().shapes(vec![MaterialShapes::circle(), MaterialShapes::heart()]);
    /// ```
    pub fn shapes(mut self, shapes: Vec<RoundedPolygon>) -> Self {
        if shapes.len() >= 2 {
            self.shapes = Some(shapes);
        }
        self
    }

    /// On a filled circle (`primary_container`), the shape in `on_primary_container`.
    pub fn contained(mut self, contained: bool) -> Self {
        self.contained = contained;
        self
    }

    /// The shape's colour.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The container's colour, when contained.
    pub fn container_color(mut self, color: Color) -> Self {
        self.container_color = Some(color);
        self
    }

    /// The box's side; the shape keeps its proportion of it.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size.max(0.0));
        self
    }

    /// What is loading, for screen readers.
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    fn morphs(&self) -> &[Morph] {
        self.morphs.get_or_init(|| {
            let shapes = self.shapes.clone().unwrap_or_else(loading_shapes);
            let n = shapes.len();
            (0..n)
                .map(|i| Morph::new(&shapes[i], &shapes[(i + 1) % n]))
                .collect()
        })
    }

    fn side(&self, theme: &Theme) -> f32 {
        self.size
            .or(theme.widgets.loading_indicator.size)
            .unwrap_or(LOADING_INDICATOR_SIZE)
    }
}

impl Default for LoadingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

/// Which morph, how far into it, and the turn, `time` seconds in: pure, so the timing is
/// tested on its own.
pub(crate) fn loading_phase(time: f32, morphs: usize) -> (usize, f32, f32) {
    let time = time.max(0.0);
    let step = (time / MORPH_SECONDS).floor();
    let into = time - step * MORPH_SECONDS;
    let p = loading_spring(into);
    let index = (step as usize) % morphs.max(1);
    let rotation = TAU * (time / SPIN_SECONDS) + FRAC_PI_2 * (step + p);
    (index, p, rotation.rem_euclid(TAU))
}

impl<Msg> Widget<Msg> for LoadingIndicator {
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
        self.value.is_none()
    }

    /// It only says that something is under way.
    fn decorative(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = theme.widgets.loading_indicator;
        let o = status.opacity;
        let side = bounds.width.min(bounds.height);
        let base = self.side(theme).max(1.0);
        let active = t.active_size.unwrap_or(LOADING_INDICATOR_ACTIVE_SIZE) / base * side;
        let (cx, cy) = (
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        let color = if self.contained {
            let container = self
                .container_color
                .or(t.container_color)
                .unwrap_or(theme.scheme.primary_container);
            scene.draw_rect(
                Rect::new(cx - side * 0.5, cy - side * 0.5, side, side),
                container.fade(o),
                side * 0.5,
                0.0,
                Color::TRANSPARENT,
            );
            self.color
                .or(t.color)
                .unwrap_or(theme.scheme.on_primary_container)
        } else {
            self.color.or(t.color).unwrap_or(theme.scheme.primary)
        };
        let shape_box = Rect::new(cx - active * 0.5, cy - active * 0.5, active, active);
        let morphs = self.morphs();
        let (morph, progress, rotation) = match self.value {
            Some(v) => (&morphs[0], v, v * std::f32::consts::PI),
            None => {
                let (i, p, r) = loading_phase(status.time, morphs.len());
                (&morphs[i], p, r)
            }
        };
        scene.fill_path(&morph.path_at(progress, shape_box, rotation), color.fade(o));
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut s = frus_core::SemanticsProperties::new(frus_core::Role::ProgressBar);
        if let Some(label) = &self.semantics_label {
            s = s.label(label.clone());
        }
        if let Some(v) = self.value {
            let pct = (v * 100.0).round();
            s = s.range(0.0, pct, 100.0).value(format!("{pct}"));
        }
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::Primitive;

    #[test]
    fn the_spring_overshoots_then_settles() {
        let peak = (1..65)
            .map(|i| loading_spring(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.05 && peak < 1.15, "{peak}");
        assert!((loading_spring(MORPH_SECONDS) - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_phase_walks_the_shapes_and_turns() {
        let (i, p, _) = loading_phase(0.0, 7);
        assert_eq!((i, p), (0, 0.0));
        let (i, _, _) = loading_phase(MORPH_SECONDS * 3.5, 7);
        assert_eq!(i, 3);
        let (i, _, _) = loading_phase(MORPH_SECONDS * 7.2, 7);
        assert_eq!(i, 0, "round again");
        // At the end of each morph a further quarter turn has been added.
        let (_, _, r0) = loading_phase(0.0, 7);
        let (_, _, r1) = loading_phase(MORPH_SECONDS - 1e-4, 7);
        let spin = TAU * MORPH_SECONDS / SPIN_SECONDS;
        assert!(((r1 - r0) - (FRAC_PI_2 + spin)).abs() < 0.02, "{}", r1 - r0);
    }

    #[test]
    fn it_paints_a_shape_and_a_container_when_contained() {
        let theme = Theme::light();
        let mut scene = Scene::new();
        let ind = LoadingIndicator::new().contained(true);
        Widget::<()>::paint(
            &ind,
            Rect::new(0.0, 0.0, 48.0, 48.0),
            Status::default(),
            &theme,
            &mut scene,
        );
        let prims = scene.primitives();
        assert!(
            matches!(&prims[0], Primitive::Rect { color, .. } if *color == theme.scheme.primary_container)
        );
        assert!(prims.iter().any(|p| matches!(p,
            Primitive::Path { fill: Some(c), .. } if *c == theme.scheme.on_primary_container)));
        assert!(Widget::<()>::continuous(&ind));
        assert!(Widget::<()>::decorative(&ind));
        assert!(!Widget::<()>::continuous(&LoadingIndicator::determinate(
            0.3
        )));
    }
}
