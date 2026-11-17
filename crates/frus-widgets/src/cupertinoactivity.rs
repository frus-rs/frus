// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoActivityIndicator`]: the Apple-style spinner — eight ticks round a circle,
//! their brightness turning clockwise once a second (milestone 812). And
//! [`CircularProgressIndicator::adaptive`], which is this spinner under an Apple platform's
//! theme and the circular one everywhere else.

use frus_core::{Color, Path, Point, Rect, Scene, Stroke, StrokeCap};
use frus_layout::{Dimension, Style};

use crate::cupertinocolors::CupertinoDynamicColor;
use crate::interaction::Status;
use crate::spinner::CircularProgressIndicator;
use crate::theme::Theme;
use crate::widget::Widget;

/// The spinner's radius unless told (`activity_indicator.dart:12`).
pub const CUPERTINO_ACTIVITY_RADIUS: f32 = 10.0;
/// A turn of the brightness, in seconds (`:94`).
const TURN_SECONDS: f32 = 1.0;
/// Each tick's opacity, counted back from the brightest (`:139`), out of 255: the native
/// spinner's own, the same in the light and the dark.
const TICK_ALPHAS: [u8; 8] = [47, 47, 47, 47, 72, 97, 122, 147];
/// A partly revealed spinner's ticks, all alike (`:142`).
const REVEALED_ALPHA: u8 = 147;

/// The ticks' colour unless told (`:15`): a dark grey in the light, a pale one in the dark.
pub const CUPERTINO_ACTIVITY_COLOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xFF3C3C44),
    Color::from_argb_u32(0xFFEBEBF5),
);

/// **The Apple-style activity indicator**: eight ticks round a circle, the brightest one
/// turning clockwise once a second.
///
/// ```
/// use frus_widgets::CupertinoActivityIndicator;
///
/// let _busy = CupertinoActivityIndicator::new();
/// let _large = CupertinoActivityIndicator::new().radius(20.0);
/// let _half = CupertinoActivityIndicator::partially_revealed(0.5);
/// ```
///
/// [`animating(false)`](Self::animating) holds it still. A
/// [partially revealed](Self::partially_revealed) one draws only the first ticks of the
/// circle, all alike — what a pull to refresh shows as the pull grows.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoActivityIndicator {
    radius: f32,
    color: Option<Color>,
    animating: bool,
    progress: f32,
    semantics_label: Option<String>,
}

impl Default for CupertinoActivityIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl CupertinoActivityIndicator {
    /// A spinner of radius 10, turning.
    pub fn new() -> Self {
        Self {
            radius: CUPERTINO_ACTIVITY_RADIUS,
            color: None,
            animating: true,
            progress: 1.0,
            semantics_label: None,
        }
    }

    /// A spinner showing `progress` of its ticks (`0.0..=1.0`), still.
    pub fn partially_revealed(progress: f32) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            animating: false,
            ..Self::new()
        }
    }

    /// Its radius; the box is twice as wide.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius.max(0.0);
        self
    }

    /// The ticks' colour, in place of the adaptive grey.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Whether it turns. Still, it is drawn as it would be at the start of a turn.
    #[must_use]
    pub fn animating(mut self, animating: bool) -> Self {
        self.animating = animating;
        self
    }

    /// What a screen reader says it is.
    #[must_use]
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// The opacity of tick `i` (clockwise from the top), the brightest at `active` — out of
    /// 255 (`:177`).
    fn alpha(&self, i: usize, active: usize) -> u8 {
        if self.progress < 1.0 {
            REVEALED_ALPHA
        } else {
            TICK_ALPHAS[(i + TICK_ALPHAS.len() - active) % TICK_ALPHAS.len()]
        }
    }

    /// Which tick is the start of the turn `time` seconds in.
    fn active_tick(&self, time: f32) -> usize {
        if !self.animating {
            return 0;
        }
        let turn = (time / TURN_SECONDS).rem_euclid(1.0);
        ((turn * TICK_ALPHAS.len() as f32).floor() as usize).min(TICK_ALPHAS.len() - 1)
    }
}

impl<Msg> Widget<Msg> for CupertinoActivityIndicator {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(self.radius * 2.0),
            height: Dimension::Length(self.radius * 2.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        self.animating
    }

    /// Its turning only says that something is under way.
    fn decorative(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let colour = self
            .color
            .unwrap_or_else(|| CUPERTINO_ACTIVITY_COLOR.resolve(theme));
        let centre = Point::new(
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        );
        // A tick is a rounded bar from a third of the radius out to the radius, a fifth of a
        // tenth of it wide (`:150`): a line with round ends, its ends pulled in by half its
        // width so that the caps land where the bar's ends are.
        let r = self.radius;
        let half = r / CUPERTINO_ACTIVITY_RADIUS;
        let tick = Path::new()
            .move_to(Point::new(centre.x, centre.y - r + half))
            .line_to(Point::new(centre.x, centre.y - r / 3.0 - half));
        let count = TICK_ALPHAS.len();
        let active = self.active_tick(status.time);
        let shown = (count as f32 * self.progress).ceil() as usize;
        for i in 0..shown.min(count) {
            let alpha = f32::from(self.alpha(i, active)) / 255.0;
            let angle = std::f32::consts::TAU * i as f32 / count as f32;
            scene.paint_path(
                &tick.rotated(centre, angle),
                None,
                Some(
                    Stroke::new(colour.with_alpha(colour.a * alpha).fade(o), half * 2.0)
                        .with_cap(StrokeCap::Round),
                ),
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut s = frus_core::SemanticsProperties::new(frus_core::Role::ProgressBar);
        if let Some(label) = &self.semantics_label {
            s = s.label(label.clone());
        }
        Some(s)
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoActivityIndicator"
    }
}

/// **A progress indicator that looks like its platform's**: the Apple-style spinner under a
/// theme whose platform is iOS or macOS, the [`CircularProgressIndicator`] everywhere else.
/// Made by [`CircularProgressIndicator::adaptive`].
pub struct AdaptiveProgressIndicator {
    material: CircularProgressIndicator,
    cupertino: CupertinoActivityIndicator,
}

impl CircularProgressIndicator {
    /// **A spinner in its platform's style**: the Apple-style one under a theme whose
    /// platform is iOS or macOS, this one everywhere else. With a value, the Apple-style one
    /// is revealed that far, as the native one is.
    pub fn adaptive(value: Option<f32>) -> AdaptiveProgressIndicator {
        AdaptiveProgressIndicator {
            material: CircularProgressIndicator::with_value(value),
            cupertino: match value {
                Some(v) => CupertinoActivityIndicator::partially_revealed(v),
                None => CupertinoActivityIndicator::new(),
            },
        }
    }
}

impl AdaptiveProgressIndicator {
    /// The colour of both: the circular one's arc and the spinner's ticks.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.material = self.material.color(color);
        self.cupertino = self.cupertino.color(color);
        self
    }

    /// What a screen reader says it is.
    #[must_use]
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.material = self.material.semantics_label(label.clone());
        self.cupertino = self.cupertino.semantics_label(label);
        self
    }

    fn apple(theme: &Theme) -> bool {
        theme.platform.is_apple()
    }
}

impl<Msg> Widget<Msg> for AdaptiveProgressIndicator {
    fn style(&self) -> Style {
        Widget::<Msg>::style_themed(self, &Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        if Self::apple(theme) {
            Widget::<Msg>::style(&self.cupertino)
        } else {
            Widget::<Msg>::style_themed(&self.material, theme)
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        Widget::<Msg>::continuous(&self.material) || Widget::<Msg>::continuous(&self.cupertino)
    }

    fn decorative(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        if Self::apple(theme) {
            Widget::<Msg>::paint(&self.cupertino, bounds, status, theme, scene);
        } else {
            Widget::<Msg>::paint(&self.material, bounds, status, theme, scene);
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        Widget::<Msg>::semantics(&self.material)
    }

    fn debug_name(&self) -> &'static str {
        "AdaptiveProgressIndicator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cupertinobutton::AllPrimitives;
    use crate::{build_ui, Runtime};
    use frus_core::{Primitive, Size, TargetPlatform};

    /// Each tick's stroke, in painting order.
    fn ticks(widget: &dyn Widget<()>, theme: &Theme) -> Vec<Stroke> {
        build_ui(widget, Size::new(100.0, 100.0), &Runtime::default(), theme)
            .scene()
            .all()
            .into_iter()
            .filter_map(|p| match p {
                Primitive::Path {
                    stroke: Some(s), ..
                } => Some(*s),
                _ => None,
            })
            .collect()
    }

    /// **Twice its radius square**: 20 by default.
    #[test]
    fn it_is_twice_its_radius_square() {
        let s = Widget::<()>::style(&CupertinoActivityIndicator::new());
        assert_eq!(s.width, Dimension::Length(20.0));
        let s = Widget::<()>::style(&CupertinoActivityIndicator::new().radius(15.0));
        assert_eq!(s.height, Dimension::Length(30.0));
    }

    /// **Eight ticks, a fifth of a tenth of the radius wide, their opacities the native
    /// spinner's**, the brightest just before the turn's start.
    #[test]
    fn eight_ticks_at_the_native_opacities() {
        let theme = Theme::light();
        let painted = ticks(&CupertinoActivityIndicator::new().animating(false), &theme);
        assert_eq!(painted.len(), 8);
        assert!(painted.iter().all(|s| s.width == 2.0));
        let base = CUPERTINO_ACTIVITY_COLOR.resolve(&theme);
        let alphas: Vec<u8> = painted
            .iter()
            .map(|s| (s.color.a / base.a * 255.0).round() as u8)
            .collect();
        assert_eq!(alphas, TICK_ALPHAS.to_vec());
    }

    /// **The brightness turns clockwise once a second**: half a second in, the brightest
    /// tick is four places on.
    #[test]
    fn the_brightness_turns_once_a_second() {
        let s = CupertinoActivityIndicator::new();
        assert_eq!(s.active_tick(0.0), 0);
        assert_eq!(s.active_tick(0.5), 4);
        assert_eq!(s.active_tick(1.0), 0);
        assert_eq!(s.alpha(3, 4), 147);
        assert_eq!(s.alpha(4, 4), 47);
        // Still, it is the start of a turn whatever the time.
        assert_eq!(s.clone().animating(false).active_tick(0.5), 0);
        assert!(Widget::<()>::continuous(&s));
        assert!(!Widget::<()>::continuous(&s.animating(false)));
    }

    /// **Partly revealed, it draws that share of the ticks, all alike.**
    #[test]
    fn partly_revealed_it_draws_a_share() {
        let theme = Theme::light();
        let painted = ticks(&CupertinoActivityIndicator::partially_revealed(0.5), &theme);
        assert_eq!(painted.len(), 4);
        let first = painted[0].color;
        assert!(painted.iter().all(|s| s.color == first));
    }

    /// **Its grey adapts to the dark**, and a colour of the caller's replaces it.
    #[test]
    fn its_grey_adapts_to_the_dark() {
        let dark = Theme::dark();
        let painted = ticks(&CupertinoActivityIndicator::new().animating(false), &dark);
        let pale = Color::from_argb_u32(0xFFEBEBF5);
        assert!((painted[7].color.r - pale.r).abs() < 1e-6);
        let red = Color::rgb(1.0, 0.0, 0.0);
        let painted = ticks(
            &CupertinoActivityIndicator::new()
                .color(red)
                .animating(false),
            &dark,
        );
        assert_eq!(painted[7].color.with_alpha(1.0), red);
        let s =
            Widget::<()>::semantics(&CupertinoActivityIndicator::new().semantics_label("Loading"))
                .unwrap();
        assert_eq!(s.role, frus_core::Role::ProgressBar);
        assert_eq!(s.label.as_deref(), Some("Loading"));
    }

    /// **The adaptive indicator is the spinner on Apple's platforms**, the circle elsewhere.
    #[test]
    fn the_adaptive_indicator_follows_the_platform() {
        let side = |platform: TargetPlatform| {
            let theme = Theme::light().with_platform(platform);
            match Widget::<()>::style_themed(&CircularProgressIndicator::adaptive(None), &theme)
                .width
            {
                Dimension::Length(w) => w,
                _ => 0.0,
            }
        };
        assert_eq!(side(TargetPlatform::Ios), 20.0);
        assert_eq!(side(TargetPlatform::MacOs), 20.0);
        assert_eq!(side(TargetPlatform::Android), 36.0);
        let theme = Theme::light().with_platform(TargetPlatform::Ios);
        assert_eq!(
            ticks(&CircularProgressIndicator::adaptive(None), &theme).len(),
            8
        );
    }
}
