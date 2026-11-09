// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The Material motion patterns** (milestone 785): fade through, shared axis, fade and
//! scale, and the container transform, as pure functions of a progress; and the
//! **motion presets**, named timings an application uses instead of numbers.
//!
//! Each pattern says how the child **arriving** and the child **leaving** look `progress`
//! of the way through a change, `0` being the old child at rest and `1` the new one. The
//! same function draws a page transition ([`crate::PageTransitionsBuilder`]), a switch
//! between two children ([`crate::AnimatedSwitcher::pattern`]) and the content of an
//! [`crate::OpenContainer`], so the three cannot drift apart.
//!
//! The numbers are the specification's:
//!
//! | pattern | arriving | leaving |
//! |---|---|---|
//! | **fade through** | holds at 0 for 30 % of the way, then fades in and scales from 92 % on a decelerating curve | fades out over the first 30 % on an accelerating curve |
//! | **shared axis X / Y** | slides 30 px along the axis and fades in over 30–100 % | slides 30 px on along the axis and fades out over 0–30 % |
//! | **shared axis Z** | scales from 80 % (from 110 % going back) and fades in over 30–100 % | scales to 110 % (to 80 % going back) and fades out over 0–30 % |
//! | **fade scale** | fades in over the first 30 % and scales from 80 % | fades out, linearly |
//!
//! Durations come from the theme's motion tokens ([`MotionTheme`]): 300 ms
//! (`medium2`) for fade through, shared axis and the container transform, 150 ms
//! (`short3`) for a fade scale arriving and half of that leaving.

use frus_core::{Color, Curve};

use crate::motion::MotionTheme;
use crate::widget::Widget;

/// The axis a [`MotionPattern::SharedAxis`] moves along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SharedAxis {
    /// Across the reading direction: the next child comes in from the end edge. Turned
    /// round under a right-to-left layout.
    X,
    /// Down the screen: the next child comes up from below.
    Y,
    /// In depth: the next child grows into place while the one before grows past it.
    Z,
}

/// One of the Material motion patterns: how two children, or two pages, take each
/// other's place.
///
/// ```
/// use frus_widgets::{MotionPattern, SharedAxis};
///
/// // Halfway through a forward step along X: the new child is coming in from the end
/// // edge, the old one is gone.
/// let arriving = MotionPattern::SharedAxis(SharedAxis::X).pose(0.5, true, true);
/// assert!(arriving.dx > 0.0 && arriving.opacity > 0.0);
/// let leaving = MotionPattern::SharedAxis(SharedAxis::X).pose(0.5, false, true);
/// assert_eq!(leaving.opacity, 0.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionPattern {
    /// **Fade through**: between two children that are not related to each other — the
    /// destinations of a bottom navigation bar. The old one fades out, then the new one
    /// fades in as it grows from 92 %.
    FadeThrough,
    /// **Shared axis**: between two children that are steps along one axis — the pages of
    /// an onboarding flow (X), the steps of a stepper (Y), a parent and its child (Z).
    SharedAxis(SharedAxis),
    /// **Fade scale**: something appearing over the page and going away again — a
    /// dialog, a menu, a floating button. It fades in as it grows from 80 %, and simply
    /// fades out.
    FadeScale,
}

/// How one child looks at a moment of a [`MotionPattern`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionPose {
    /// Its opacity.
    pub opacity: f32,
    /// Its scale about its centre.
    pub scale: f32,
    /// How far it is moved along the reading direction, in logical pixels: positive
    /// towards the end edge (right in a left-to-right layout).
    pub dx: f32,
    /// How far it is moved down, in logical pixels.
    pub dy: f32,
}

impl MotionPose {
    /// Where a child rests: solid, at its size, in its place.
    pub const REST: MotionPose = MotionPose {
        opacity: 1.0,
        scale: 1.0,
        dx: 0.0,
        dy: 0.0,
    };
}

/// How far a shared axis slides each child, in logical pixels.
pub const SHARED_AXIS_SLIDE: f32 = 30.0;

/// `cubic-bezier(0.4, 0, 0.2, 1)`, "fast out, slow in": the curve the patterns move on.
fn legacy() -> Curve {
    Curve::Cubic {
        x1: 0.4,
        y1: 0.0,
        x2: 0.2,
        y2: 1.0,
    }
}

/// `cubic-bezier(0, 0, 0.2, 1)`: what the patterns fade and grow in on.
fn legacy_decelerate() -> Curve {
    Curve::Cubic {
        x1: 0.0,
        y1: 0.0,
        x2: 0.2,
        y2: 1.0,
    }
}

/// `cubic-bezier(0.4, 0, 1, 1)`: what the patterns fade out on.
fn legacy_accelerate() -> Curve {
    Curve::Cubic {
        x1: 0.4,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    }
}

/// `curve` at `t`, exactly `0` at the start and `1` at the end: a cubic solved numerically
/// lands a hair off its ends, and a page "gone" at 30 % must be gone, not at 0.001 %.
fn ease(curve: &Curve, t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        curve.transform(t)
    }
}

/// Where `t` is between `begin` and `end`, clamped to `0..=1`.
fn interval(begin: f32, end: f32, t: f32) -> f32 {
    if end <= begin {
        return if t >= end { 1.0 } else { 0.0 };
    }
    ((t - begin) / (end - begin)).clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl MotionPattern {
    /// How a child looks `progress` of the way through, `0` being the old child at rest
    /// and `1` the new one. `entering` is the child arriving; otherwise the one leaving.
    /// `forward` is a step forwards — a push, the next page; otherwise backwards, which
    /// turns a shared axis round.
    pub fn pose(self, progress: f32, entering: bool, forward: bool) -> MotionPose {
        let p = progress.clamp(0.0, 1.0);
        match self {
            MotionPattern::FadeThrough => fade_through(p, entering),
            MotionPattern::SharedAxis(axis) => shared_axis(axis, p, entering, forward),
            MotionPattern::FadeScale => fade_scale(p, entering),
        }
    }

    /// How long a child takes to arrive, in seconds, from the theme's tokens: `medium2`
    /// (300 ms) for fade through and shared axis, `short3` (150 ms) for fade scale.
    pub fn duration(self, motion: &MotionTheme) -> f32 {
        match self {
            MotionPattern::FadeThrough | MotionPattern::SharedAxis(_) => motion.medium2,
            MotionPattern::FadeScale => motion.short3,
        }
    }

    /// How long a child takes to leave when nothing arrives in its place — a dialog
    /// closing: the same as [`duration`](Self::duration), except for fade scale, which
    /// leaves in half the time it took to arrive (75 ms by default).
    pub fn reverse_duration(self, motion: &MotionTheme) -> f32 {
        match self {
            MotionPattern::FadeScale => motion.short3 * 0.5,
            _ => self.duration(motion),
        }
    }

    /// Whether the pattern stands its children on a fill — the surface colour by
    /// default — so that what is behind them does not show while both are faded: fade
    /// through and shared axis do, fade scale does not.
    pub fn fills(self) -> bool {
        !matches!(self, MotionPattern::FadeScale)
    }

    /// Whether the pattern's horizontal movement turns round under a right-to-left
    /// layout: the shared X axis does, so that "next" always comes from the end edge.
    pub fn mirrored(self) -> bool {
        matches!(self, MotionPattern::SharedAxis(SharedAxis::X))
    }
}

/// Fade through: the leaving child fades out over the first 30 % on the accelerating
/// curve; the arriving one waits, then fades in and grows from 92 % over the remaining
/// 70 % on the decelerating curve.
fn fade_through(p: f32, entering: bool) -> MotionPose {
    if entering {
        let t = ease(&legacy_decelerate(), interval(0.3, 1.0, p));
        MotionPose {
            opacity: t,
            scale: lerp(0.92, 1.0, t),
            ..MotionPose::REST
        }
    } else {
        MotionPose {
            opacity: 1.0 - ease(&legacy_accelerate(), interval(0.0, 0.3, p)),
            ..MotionPose::REST
        }
    }
}

/// Shared axis: both children move on the "fast out, slow in" curve, the leaving one
/// fading out over the first 30 %, the arriving one fading in over the rest.
fn shared_axis(axis: SharedAxis, p: f32, entering: bool, forward: bool) -> MotionPose {
    let moved = ease(&legacy(), p);
    let sign = if forward { 1.0 } else { -1.0 };
    if entering {
        let opacity = ease(&legacy_decelerate(), interval(0.3, 1.0, p));
        let away = SHARED_AXIS_SLIDE * sign * (1.0 - moved);
        match axis {
            SharedAxis::X => MotionPose {
                opacity,
                dx: away,
                ..MotionPose::REST
            },
            SharedAxis::Y => MotionPose {
                opacity,
                dy: away,
                ..MotionPose::REST
            },
            SharedAxis::Z => MotionPose {
                opacity,
                scale: if forward {
                    lerp(0.8, 1.0, moved)
                } else {
                    lerp(1.1, 1.0, moved)
                },
                ..MotionPose::REST
            },
        }
    } else {
        let opacity = 1.0 - ease(&legacy_accelerate(), interval(0.0, 0.3, p));
        let away = -SHARED_AXIS_SLIDE * sign * moved;
        match axis {
            SharedAxis::X => MotionPose {
                opacity,
                dx: away,
                ..MotionPose::REST
            },
            SharedAxis::Y => MotionPose {
                opacity,
                dy: away,
                ..MotionPose::REST
            },
            SharedAxis::Z => MotionPose {
                opacity,
                scale: if forward {
                    lerp(1.0, 1.1, moved)
                } else {
                    lerp(1.0, 0.8, moved)
                },
                ..MotionPose::REST
            },
        }
    }
}

/// Fade scale: arriving, a linear fade over the first 30 % and a growth from 80 % on the
/// decelerating curve; leaving, a linear fade and nothing else.
fn fade_scale(p: f32, entering: bool) -> MotionPose {
    if entering {
        MotionPose {
            opacity: interval(0.0, 0.3, p),
            scale: lerp(0.8, 1.0, ease(&legacy_decelerate(), p)),
            ..MotionPose::REST
        }
    } else {
        MotionPose {
            opacity: 1.0 - p,
            ..MotionPose::REST
        }
    }
}

/// Wraps `child` so that it is drawn as `pose` says: a fill outside the fade
/// (`outer_fill`), the fade, the move and scale, and a fill inside them
/// (`inner_fill`). The chain is the same whatever the pose, so the child keeps its
/// identity from one frame to the next and from arriving to leaving.
pub(crate) fn posed<Msg: Clone + 'static>(
    child: Box<dyn Widget<Msg>>,
    pose: MotionPose,
    rtl_mirror: bool,
    outer_fill: Color,
    inner_fill: Color,
) -> Box<dyn Widget<Msg>> {
    let dx = if rtl_mirror { -pose.dx } else { pose.dx };
    let moved = crate::Transform::translate(dx, pose.dy)
        .and_scale(pose.scale)
        .child(crate::Container::new().color(inner_fill).child(child));
    Box::new(
        crate::Container::new()
            .color(outer_fill)
            .child(crate::Opacity::new(pose.opacity.clamp(0.0, 1.0), moved)),
    )
}

// ---------------------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------------------

/// A **named timing**: a duration and an easing from the theme's motion tokens, paired
/// the way the specification pairs them. Use one instead of writing numbers, and a theme
/// with slower motion slows everything that uses it.
///
/// | preset | duration | easing |
/// |---|---|---|
/// | `Emphasized` | `long2`, 500 ms | `emphasized` |
/// | `EmphasizedEnter` | `medium4`, 400 ms | `emphasized_decelerate` |
/// | `EmphasizedExit` | `short4`, 200 ms | `emphasized_accelerate` |
/// | `Standard` | `medium2`, 300 ms | `standard` |
/// | `StandardEnter` | `medium1`, 250 ms | `standard_decelerate` |
/// | `StandardExit` | `short4`, 200 ms | `standard_accelerate` |
///
/// ```
/// use frus_widgets::{MotionPreset, Theme};
///
/// let theme = Theme::light();
/// assert_eq!(MotionPreset::EmphasizedEnter.duration(&theme.motion), 0.4);
/// let _curve = MotionPreset::EmphasizedEnter.curve(&theme.motion);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MotionPreset {
    /// Something that begins and ends on screen and matters: 500 ms, emphasized.
    Emphasized,
    /// Something important arriving: 400 ms, emphasized decelerate.
    EmphasizedEnter,
    /// Something important leaving: 200 ms, emphasized accelerate.
    EmphasizedExit,
    /// An ordinary change on screen: 300 ms, standard.
    #[default]
    Standard,
    /// Something arriving: 250 ms, standard decelerate.
    StandardEnter,
    /// Something leaving: 200 ms, standard accelerate.
    StandardExit,
}

impl MotionPreset {
    /// The preset's duration in seconds, read from `motion`.
    pub fn duration(self, motion: &MotionTheme) -> f32 {
        match self {
            MotionPreset::Emphasized => motion.long2,
            MotionPreset::EmphasizedEnter => motion.medium4,
            MotionPreset::EmphasizedExit => motion.short4,
            MotionPreset::Standard => motion.medium2,
            MotionPreset::StandardEnter => motion.medium1,
            MotionPreset::StandardExit => motion.short4,
        }
    }

    /// The preset's easing, read from `motion`.
    pub fn curve(self, motion: &MotionTheme) -> Curve {
        match self {
            MotionPreset::Emphasized => motion.emphasized.clone(),
            MotionPreset::EmphasizedEnter => motion.emphasized_decelerate.clone(),
            MotionPreset::EmphasizedExit => motion.emphasized_accelerate.clone(),
            MotionPreset::Standard => motion.standard.clone(),
            MotionPreset::StandardEnter => motion.standard_decelerate.clone(),
            MotionPreset::StandardExit => motion.standard_accelerate.clone(),
        }
    }
}

/// When a [`Transition`] runs: a preset read from the theme, or numbers of its own.
#[derive(Clone, Debug, PartialEq)]
pub enum TransitionTiming {
    /// A [`MotionPreset`], read from the theme where the transition plays.
    Preset(MotionPreset),
    /// A duration in seconds and a curve.
    Fixed {
        /// How long, in seconds.
        duration: f32,
        /// Along what.
        curve: Curve,
    },
}

/// **How something arrives or leaves** in place: what it fades from, grows from, slides
/// from and unfolds from, and when. A value, so one can be named once and reused:
///
/// ```
/// use frus_widgets::{MotionPreset, Transition};
///
/// // Rises 24 px as it fades in, on the emphasized entrance timing.
/// let rise = Transition::preset(MotionPreset::EmphasizedEnter).slide(0.0, 24.0);
/// // Shrinks a little as it fades out, on the emphasized exit timing.
/// let sink = Transition::preset(MotionPreset::EmphasizedExit).scale(0.9);
/// # let _ = (rise, sink);
/// ```
///
/// Every part is described **from the outside in**: `scale(0.9)` means the child is at
/// 90 % when it is not there and at its size when it is, whether it is arriving or
/// leaving. An [`AnimatedPresence`](crate::AnimatedPresence) plays its `enter`
/// transition forwards and its `exit` transition backwards.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    /// The opacity when not there: `0` fades completely, `1` does not fade.
    pub opacity: f32,
    /// The scale when not there, about the centre.
    pub scale: f32,
    /// The offset when not there, in logical pixels: `x` towards the end edge (turned
    /// round under a right-to-left layout), `y` down.
    pub offset: (f32, f32),
    /// Whether the room it takes unfolds with it, along the given axis — what makes the
    /// items under a list row move up as it leaves.
    pub size: Option<crate::scroll::Axis>,
    /// When it runs.
    pub timing: TransitionTiming,
}

impl Default for Transition {
    /// A fade, on [`MotionPreset::Standard`].
    fn default() -> Self {
        Self::preset(MotionPreset::Standard)
    }
}

impl Transition {
    /// A fade on `preset`'s timing — the start of most transitions. Add a slide, a scale
    /// or an unfolding with the other methods.
    pub fn preset(preset: MotionPreset) -> Self {
        Self {
            opacity: 0.0,
            scale: 1.0,
            offset: (0.0, 0.0),
            size: None,
            timing: TransitionTiming::Preset(preset),
        }
    }

    /// A fade over `duration` seconds along `curve`.
    pub fn fade(duration: f32, curve: Curve) -> Self {
        Self {
            timing: TransitionTiming::Fixed { duration, curve },
            ..Self::preset(MotionPreset::Standard)
        }
    }

    /// Nothing at all: the child is simply there, or simply gone.
    pub fn none() -> Self {
        Self {
            opacity: 1.0,
            timing: TransitionTiming::Fixed {
                duration: 0.0,
                curve: Curve::Linear,
            },
            ..Self::preset(MotionPreset::Standard)
        }
    }

    /// The opacity it fades from; `1.0` turns the fade off.
    #[must_use]
    pub fn from_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// The scale it grows from (or shrinks to), about its centre.
    #[must_use]
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Where it slides from (or to), in logical pixels: `dx` towards the end edge, `dy`
    /// down.
    #[must_use]
    pub fn slide(mut self, dx: f32, dy: f32) -> Self {
        self.offset = (dx, dy);
        self
    }

    /// Unfolds the room it takes, top to bottom, with the rest of it.
    #[must_use]
    pub fn size(mut self) -> Self {
        self.size = Some(crate::scroll::Axis::Vertical);
        self
    }

    /// Unfolds the room it takes along `axis`.
    #[must_use]
    pub fn size_along(mut self, axis: crate::scroll::Axis) -> Self {
        self.size = Some(axis);
        self
    }

    /// Runs on `preset`'s timing.
    #[must_use]
    pub fn timing(mut self, preset: MotionPreset) -> Self {
        self.timing = TransitionTiming::Preset(preset);
        self
    }

    /// Runs over `duration` seconds, on the curve it had (the preset's, read from the
    /// default tokens, when it had a preset).
    #[must_use]
    pub fn duration(mut self, duration: f32) -> Self {
        let curve = self.resolve(&MotionTheme::M3).1;
        self.timing = TransitionTiming::Fixed { duration, curve };
        self
    }

    /// Runs along `curve`, for as long as it did.
    #[must_use]
    pub fn curve(mut self, curve: Curve) -> Self {
        let duration = self.resolve(&MotionTheme::M3).0;
        self.timing = TransitionTiming::Fixed { duration, curve };
        self
    }

    /// Its duration and curve under `motion`.
    pub fn resolve(&self, motion: &MotionTheme) -> (f32, Curve) {
        match &self.timing {
            TransitionTiming::Preset(preset) => (preset.duration(motion), preset.curve(motion)),
            TransitionTiming::Fixed { duration, curve } => (*duration, curve.clone()),
        }
    }

    /// How the child looks `presence` of the way in, already eased: `0` not there, `1`
    /// in place.
    pub fn pose(&self, presence: f32) -> MotionPose {
        let p = presence.clamp(0.0, 1.0);
        MotionPose {
            opacity: lerp(self.opacity, 1.0, p),
            scale: lerp(self.scale, 1.0, p),
            dx: self.offset.0 * (1.0 - p),
            dy: self.offset.1 * (1.0 - p),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Container transform
// ---------------------------------------------------------------------------------------

/// How the two contents of a container transform give way to each other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ContainerTransitionType {
    /// The open content fades in over the closed one, which stays: for contents that
    /// look alike. The default.
    #[default]
    Fade,
    /// The closed content fades out, then the open one fades in, through a middle
    /// colour: for contents that do not.
    FadeThrough,
}

/// What a container transform paints at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerFrame {
    /// How far the container is from its closed bounds (`0`) to its open bounds (`1`),
    /// eased: what its rectangle, corners and elevation are interpolated by.
    pub bounds: f32,
    /// The scrim over the page behind, `0..=1` of its colour's strength.
    pub scrim: f32,
    /// The opacity of the closed content.
    pub closed_opacity: f32,
    /// The opacity of the open content.
    pub open_opacity: f32,
    /// The container's colour.
    pub color: Color,
}

/// The colours a container transform passes through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerColors {
    /// The closed container's.
    pub closed: Color,
    /// The open container's.
    pub open: Color,
    /// What a fade through passes through between the two.
    pub middle: Color,
}

/// **A container transform** `value` of the way open, `0` closed and `1` open:
/// where the container is, what it is painted with, and how much of each content shows.
/// `opening` says which way it is going: closing replays the same sequence in the same
/// time order, the content that was on show fading first.
///
/// The container moves on the "fast out, slow in" curve (its mirror while closing). The
/// scrim reaches its strength over the first fifth of the way while opening, and leaves
/// with the movement while closing.
///
/// ```
/// use frus_widgets::{container_transform, Color, ContainerColors, ContainerTransitionType};
///
/// let colors = ContainerColors { closed: Color::WHITE, open: Color::WHITE, middle: Color::WHITE };
/// let frame = container_transform(0.5, true, ContainerTransitionType::Fade, colors);
/// assert_eq!(frame.open_opacity, 1.0, "the open content is in by 40 %");
/// assert!(frame.bounds > 0.5, "the container is ahead of the clock: it eases out");
/// ```
pub fn container_transform(
    value: f32,
    opening: bool,
    kind: ContainerTransitionType,
    colors: ContainerColors,
) -> ContainerFrame {
    let a = value.clamp(0.0, 1.0);
    let ContainerColors {
        closed,
        open,
        middle,
    } = colors;
    if opening {
        let bounds = ease(&legacy(), a);
        let scrim = interval(0.0, 0.2, bounds);
        match kind {
            ContainerTransitionType::Fade => ContainerFrame {
                bounds,
                scrim,
                closed_opacity: 1.0,
                open_opacity: interval(0.2, 0.4, a),
                color: closed.lerp(open, interval(0.2, 0.4, a)),
            },
            ContainerTransitionType::FadeThrough => ContainerFrame {
                bounds,
                scrim,
                closed_opacity: 1.0 - interval(0.0, 0.2, a),
                open_opacity: interval(0.2, 1.0, a),
                color: if a < 0.2 {
                    closed.lerp(middle, interval(0.0, 0.2, a))
                } else {
                    middle.lerp(open, interval(0.2, 1.0, a))
                },
            },
        }
    } else {
        let bounds = ease(&Curve::Flipped(Box::new(legacy())), a);
        let scrim = bounds;
        match kind {
            ContainerTransitionType::Fade => ContainerFrame {
                bounds,
                scrim,
                closed_opacity: 1.0,
                open_opacity: interval(0.6, 0.8, a),
                color: closed.lerp(open, interval(0.6, 0.8, a)),
            },
            ContainerTransitionType::FadeThrough => ContainerFrame {
                bounds,
                scrim,
                closed_opacity: 1.0 - interval(0.0, 0.8, a),
                open_opacity: interval(0.8, 1.0, a),
                color: if a < 0.8 {
                    closed.lerp(middle, interval(0.0, 0.8, a))
                } else {
                    middle.lerp(open, interval(0.8, 1.0, a))
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-4;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    /// **Every pattern begins with the old child at rest and ends with the new one at
    /// rest**, both ways, and the child that is not on show at an end is invisible.
    #[test]
    fn every_pattern_begins_and_ends_at_rest() {
        let patterns = [
            MotionPattern::FadeThrough,
            MotionPattern::SharedAxis(SharedAxis::X),
            MotionPattern::SharedAxis(SharedAxis::Y),
            MotionPattern::SharedAxis(SharedAxis::Z),
            MotionPattern::FadeScale,
        ];
        for pattern in patterns {
            for forward in [true, false] {
                assert_eq!(
                    pattern.pose(0.0, false, forward),
                    MotionPose::REST,
                    "{pattern:?} forward={forward}: the old child at rest"
                );
                let end = pattern.pose(1.0, true, forward);
                assert!(
                    close(end.opacity, 1.0)
                        && close(end.scale, 1.0)
                        && close(end.dx, 0.0)
                        && close(end.dy, 0.0),
                    "{pattern:?} forward={forward}: the new child at rest, {end:?}"
                );
                assert_eq!(pattern.pose(0.0, true, forward).opacity, 0.0);
                assert_eq!(pattern.pose(1.0, false, forward).opacity, 0.0);
            }
        }
    }

    /// **Fade through**: the old child is gone by 30 %, the new one has not started; from
    /// there the new one fades in and grows from 92 %, on the decelerating curve.
    #[test]
    fn fade_through_fades_out_then_zooms_in_from_92_percent() {
        let p = MotionPattern::FadeThrough;
        let at = |t, entering| p.pose(t, entering, true);
        assert_eq!(at(0.3, false).opacity, 0.0, "gone by 30 %");
        assert_eq!(at(0.3, true).opacity, 0.0, "not started at 30 %");
        assert!(close(at(0.3, true).scale, 0.92), "at 92 % while it waits");
        assert!(close(at(0.0, true).scale, 0.92));
        // Halfway through the old child's fade, the accelerating curve has it mostly there.
        let fading = at(0.15, false).opacity;
        let expected = 1.0 - legacy_accelerate().transform(0.5);
        assert!(close(fading, expected), "{fading} vs {expected}");
        assert!(
            fading > 0.5,
            "an accelerating fade is slow to start: {fading}"
        );
        // Halfway through its own 70 %, the new child is on the decelerating curve.
        let mid = at(0.65, true);
        let eased = legacy_decelerate().transform(0.5);
        assert!(close(mid.opacity, eased), "{mid:?}");
        assert!(close(mid.scale, 0.92 + 0.08 * eased));
        assert!(mid.opacity > 0.5, "a decelerating fade is quick to start");
        // Fade through goes the same way back.
        assert_eq!(p.pose(0.65, true, false), mid);
    }

    /// **Shared axis X**: 30 px towards the start for the old child, from 30 px towards
    /// the end for the new one, on the "fast out, slow in" curve; the fades split at 30 %.
    /// Going back, both turn round.
    #[test]
    fn shared_axis_x_slides_thirty_pixels_each_way() {
        let p = MotionPattern::SharedAxis(SharedAxis::X);
        assert!(close(p.pose(0.0, true, true).dx, 30.0));
        assert!(close(p.pose(1.0, false, true).dx, -30.0));
        assert!(
            close(p.pose(0.0, true, false).dx, -30.0),
            "back: from the start"
        );
        assert!(
            close(p.pose(1.0, false, false).dx, 30.0),
            "back: towards the end"
        );
        let half = legacy().transform(0.5);
        assert!(close(p.pose(0.5, true, true).dx, 30.0 * (1.0 - half)));
        assert!(close(p.pose(0.5, false, true).dx, -30.0 * half));
        assert_eq!(p.pose(0.3, false, true).opacity, 0.0);
        assert_eq!(p.pose(0.3, true, true).opacity, 0.0);
        let fade_in = legacy_decelerate().transform(interval(0.3, 1.0, 0.5));
        assert!(close(p.pose(0.5, true, true).opacity, fade_in));
        assert_eq!(p.pose(0.5, true, true).dy, 0.0, "nothing vertical");
        assert!(p.mirrored(), "X turns round in right-to-left");
        assert!(!MotionPattern::SharedAxis(SharedAxis::Y).mirrored());
    }

    /// **Shared axis Y**: the same 30 px, downwards.
    #[test]
    fn shared_axis_y_slides_up_from_below() {
        let p = MotionPattern::SharedAxis(SharedAxis::Y);
        assert!(close(p.pose(0.0, true, true).dy, 30.0), "from below");
        assert!(close(p.pose(1.0, false, true).dy, -30.0), "up and away");
        assert_eq!(p.pose(0.5, true, true).dx, 0.0);
    }

    /// **Shared axis Z**: forwards, the new child grows from 80 % and the old one grows
    /// to 110 %; backwards, the new one shrinks from 110 % and the old one to 80 %.
    #[test]
    fn shared_axis_z_scales_from_80_and_to_110_percent() {
        let p = MotionPattern::SharedAxis(SharedAxis::Z);
        assert!(close(p.pose(0.0, true, true).scale, 0.8));
        assert!(close(p.pose(1.0, false, true).scale, 1.1));
        assert!(close(p.pose(0.0, true, false).scale, 1.1));
        assert!(close(p.pose(1.0, false, false).scale, 0.8));
        let half = legacy().transform(0.5);
        assert!(close(p.pose(0.5, true, true).scale, 0.8 + 0.2 * half));
        assert_eq!(p.pose(0.5, true, true).dx, 0.0);
    }

    /// **Fade scale**: in by 30 %, grown from 80 % on the decelerating curve; leaving, a
    /// linear fade at its size. 150 ms in, 75 out, from the tokens.
    #[test]
    fn fade_scale_grows_from_80_percent_and_simply_fades_out() {
        let p = MotionPattern::FadeScale;
        assert!(close(p.pose(0.15, true, true).opacity, 0.5));
        assert_eq!(p.pose(0.3, true, true).opacity, 1.0);
        assert!(close(p.pose(0.0, true, true).scale, 0.8));
        let eased = legacy_decelerate().transform(0.5);
        assert!(close(p.pose(0.5, true, true).scale, 0.8 + 0.2 * eased));
        let out = p.pose(0.25, false, true);
        assert!(close(out.opacity, 0.75) && out.scale == 1.0, "{out:?}");
        let m = MotionTheme::M3;
        assert!(close(p.duration(&m), 0.15));
        assert!(close(p.reverse_duration(&m), 0.075));
        assert!(close(MotionPattern::FadeThrough.duration(&m), 0.3));
        assert!(!p.fills() && MotionPattern::FadeThrough.fills());
    }

    /// **The durations follow the tokens**: a theme with slower motion slows the patterns.
    #[test]
    fn the_durations_follow_the_theme_s_tokens() {
        let slow = MotionTheme {
            medium2: 0.6,
            short3: 0.3,
            ..MotionTheme::M3
        };
        assert_eq!(MotionPattern::FadeThrough.duration(&slow), 0.6);
        assert_eq!(
            MotionPattern::SharedAxis(SharedAxis::Z).duration(&slow),
            0.6
        );
        assert_eq!(MotionPattern::FadeScale.reverse_duration(&slow), 0.15);
    }

    /// **The presets are the specification's pairs**, read from the tokens.
    #[test]
    fn the_presets_pair_durations_with_easings() {
        let m = MotionTheme::M3;
        for (preset, duration, curve) in [
            (MotionPreset::Emphasized, 0.5, m.emphasized.clone()),
            (
                MotionPreset::EmphasizedEnter,
                0.4,
                m.emphasized_decelerate.clone(),
            ),
            (
                MotionPreset::EmphasizedExit,
                0.2,
                m.emphasized_accelerate.clone(),
            ),
            (MotionPreset::Standard, 0.3, m.standard.clone()),
            (
                MotionPreset::StandardEnter,
                0.25,
                m.standard_decelerate.clone(),
            ),
            (
                MotionPreset::StandardExit,
                0.2,
                m.standard_accelerate.clone(),
            ),
        ] {
            assert!(close(preset.duration(&m), duration), "{preset:?}");
            assert_eq!(preset.curve(&m), curve, "{preset:?}");
        }
        let calm = MotionTheme {
            medium4: 0.8,
            ..MotionTheme::M3
        };
        let rise = Transition::preset(MotionPreset::EmphasizedEnter);
        assert_eq!(
            rise.resolve(&calm).0,
            0.8,
            "a preset is read where it plays"
        );
    }

    /// **A transition is described from the outside in**: at 0 the child is at the
    /// transition's values, at 1 in place.
    #[test]
    fn a_transition_moves_from_its_values_to_rest() {
        let t = Transition::preset(MotionPreset::StandardEnter)
            .scale(0.8)
            .slide(10.0, 20.0);
        let gone = t.pose(0.0);
        assert_eq!(
            (gone.opacity, gone.scale, gone.dx, gone.dy),
            (0.0, 0.8, 10.0, 20.0)
        );
        assert_eq!(t.pose(1.0), MotionPose::REST);
        let half = t.pose(0.5);
        assert!(close(half.opacity, 0.5) && close(half.scale, 0.9) && close(half.dy, 10.0));
        let fixed = t.clone().duration(0.7);
        assert_eq!(fixed.resolve(&MotionTheme::M3).0, 0.7);
        assert_eq!(
            fixed.resolve(&MotionTheme::M3).1,
            MotionTheme::M3.standard_decelerate,
            "the duration changed, the curve stayed"
        );
        assert_eq!(Transition::none().pose(0.0), MotionPose::REST);
    }

    fn colors() -> ContainerColors {
        ContainerColors {
            closed: Color::rgb(1.0, 0.0, 0.0),
            open: Color::rgb(0.0, 0.0, 1.0),
            middle: Color::rgb(0.0, 1.0, 0.0),
        }
    }

    /// **The container transform, opening with a fade**: the open content fades in over
    /// 20–40 % while the colour crosses over; the closed content stays; the scrim is at
    /// its strength once the container is a fifth of the way; the bounds ease out.
    #[test]
    fn the_container_opens_with_a_fade_over_20_to_40_percent() {
        let f = |a| container_transform(a, true, ContainerTransitionType::Fade, colors());
        assert_eq!(f(0.0).bounds, 0.0);
        assert!(close(f(1.0).bounds, 1.0));
        assert_eq!(f(0.2).open_opacity, 0.0);
        assert!(close(f(0.3).open_opacity, 0.5));
        assert_eq!(f(0.4).open_opacity, 1.0);
        assert_eq!(
            f(0.3).closed_opacity,
            1.0,
            "a fade keeps the closed content"
        );
        assert_eq!(f(0.1).color, colors().closed);
        assert_eq!(f(0.5).color, colors().open);
        assert!(close(f(0.5).bounds, legacy().transform(0.5)));
        let early = f(0.05);
        assert!(close(early.scrim, interval(0.0, 0.2, early.bounds)));
        assert_eq!(f(0.5).scrim, 1.0);
    }

    /// **Closing with a fade** replays it in time: the open content goes first, over
    /// 20–40 % of the way back; the bounds on the mirror curve; the scrim with them.
    #[test]
    fn the_container_closes_by_fading_the_open_content_first() {
        let f = |a| container_transform(a, false, ContainerTransitionType::Fade, colors());
        assert_eq!(f(0.9).open_opacity, 1.0, "10 % of the way back: still open");
        assert!(close(f(0.7).open_opacity, 0.5), "30 % back: half");
        assert_eq!(f(0.55).open_opacity, 0.0, "gone by 40 % back");
        let flipped = Curve::Flipped(Box::new(legacy())).transform(0.5);
        assert!(close(f(0.5).bounds, flipped));
        assert!(close(f(0.5).scrim, f(0.5).bounds));
        assert_eq!(f(0.0).bounds, 0.0);
    }

    /// **A fade through** passes through the middle colour: the closed content is gone
    /// by 20 % and the open one fades in over the rest; closing, the open content is
    /// gone within the first 20 % of the way back.
    #[test]
    fn the_container_fades_through_a_middle_colour() {
        let open = |a| container_transform(a, true, ContainerTransitionType::FadeThrough, colors());
        assert_eq!(open(0.2).closed_opacity, 0.0);
        assert_eq!(open(0.2).open_opacity, 0.0);
        assert_eq!(open(0.2).color, colors().middle);
        assert!(close(open(0.6).open_opacity, 0.5));
        let shut =
            |a| container_transform(a, false, ContainerTransitionType::FadeThrough, colors());
        assert_eq!(
            shut(0.8).open_opacity,
            0.0,
            "20 % back: the open content is gone"
        );
        assert_eq!(shut(0.8).color, colors().middle);
        assert!(close(shut(0.4).closed_opacity, 0.5));
        assert_eq!(shut(0.0).closed_opacity, 1.0);
    }
}
