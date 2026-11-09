// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **How a page arrives and leaves**, platform by platform (milestone 620).
//!
//! The reference's shape (`page_transitions_theme.dart`): a [`PageTransitionsTheme`] on the
//! theme maps each platform to a [`PageTransitionsBuilder`], and the builder decides how the
//! page being pushed or popped, and the page under it, look at each moment of the
//! transition. By default the desktops zoom, Apple's platforms slide, and Android fades
//! forwards — the reference's own defaults, read from the **theme's** platform so a theme
//! set to iOS slides on any device.
//!
//! Everything here is a pure function of the transition's progress: the navigator asks
//! [`PageTransitionsBuilder::frame`] what to paint, and the router runs the progress over
//! [`PageTransitionsBuilder::transition_duration`].

use frus_core::{Curve, TargetPlatform};

use crate::motion::MotionTheme;
use crate::motionpatterns::{MotionPattern, MotionPose, SharedAxis};

/// One of the reference's page transitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageTransitionsBuilder {
    /// The page zooms in from 85 % over a scrim, the one under it grows a little; popping,
    /// the page shrinks away and fades (`ZoomPageTransitionsBuilder`). What the desktops
    /// use by default, and what a platform with no builder falls back to.
    Zoom,
    /// The page slides in from the end edge while the one under it drifts a third of the
    /// way towards the start, as iOS pages do (`CupertinoPageTransitionsBuilder`).
    Cupertino,
    /// The page slides a quarter of the way in as it fades in, and the one under it a
    /// quarter of the way out as it fades out, as Android's own pages do
    /// (`FadeForwardsPageTransitionsBuilder`). What Android uses when no back gesture is
    /// under way.
    FadeForwards,
    /// **Fade through** (milestone 785): the page leaving fades out, then the page
    /// arriving fades in as it grows from 92 %, over the surface. For pages that are not
    /// related to each other. See [`MotionPattern::FadeThrough`].
    FadeThrough,
    /// **Shared axis** (milestone 785): the pages move 30 px along the axis — or scale, in
    /// depth — as one fades out and the other in. For pages that are steps of one flow.
    /// See [`MotionPattern::SharedAxis`].
    SharedAxis(SharedAxis),
    /// **Fade scale** (milestone 785): the page arriving fades in as it grows from 80 %
    /// over the page under it, which does not move; leaving, it fades out. For a page that
    /// stands over the one under it, as a full-screen dialog does. See
    /// [`MotionPattern::FadeScale`].
    FadeScale,
}

impl From<MotionPattern> for PageTransitionsBuilder {
    fn from(pattern: MotionPattern) -> Self {
        match pattern {
            MotionPattern::FadeThrough => PageTransitionsBuilder::FadeThrough,
            MotionPattern::SharedAxis(axis) => PageTransitionsBuilder::SharedAxis(axis),
            MotionPattern::FadeScale => PageTransitionsBuilder::FadeScale,
        }
    }
}

/// How the theme's platform chooses a [`PageTransitionsBuilder`]: the reference's
/// `PageTransitionsTheme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PageTransitionsTheme {
    /// One per platform, in [`TargetPlatform::ALL`]'s order; `None` falls back to
    /// [`PageTransitionsBuilder::Zoom`].
    builders: [Option<PageTransitionsBuilder>; 6],
}

impl Default for PageTransitionsTheme {
    /// The reference's defaults (`page_transitions_theme.dart:764`): Android fades
    /// forwards, iOS and macOS slide, Linux and Windows zoom, and Fuchsia, which has none,
    /// zooms.
    fn default() -> Self {
        use PageTransitionsBuilder as B;
        let mut builders = [None; 6];
        for (platform, builder) in [
            (TargetPlatform::Android, B::FadeForwards),
            (TargetPlatform::Ios, B::Cupertino),
            (TargetPlatform::MacOs, B::Cupertino),
            (TargetPlatform::Windows, B::Zoom),
            (TargetPlatform::Linux, B::Zoom),
        ] {
            builders[index(platform)] = Some(builder);
        }
        Self { builders }
    }
}

fn index(platform: TargetPlatform) -> usize {
    TargetPlatform::ALL
        .iter()
        .position(|p| *p == platform)
        .unwrap_or(0)
}

impl PageTransitionsTheme {
    /// The same theme, with `builder` for `platform`.
    #[must_use]
    pub fn with(mut self, platform: TargetPlatform, builder: PageTransitionsBuilder) -> Self {
        self.builders[index(platform)] = Some(builder);
        self
    }

    /// The same theme, with `builder` for **every** platform: the one transition an
    /// application uses wherever it runs.
    ///
    /// ```
    /// use frus_widgets::{PageTransitionsBuilder, PageTransitionsTheme, TargetPlatform};
    ///
    /// let theme = PageTransitionsTheme::default().all(PageTransitionsBuilder::FadeThrough);
    /// assert_eq!(
    ///     theme.builder_for(TargetPlatform::Ios),
    ///     PageTransitionsBuilder::FadeThrough
    /// );
    /// ```
    #[must_use]
    pub fn all(mut self, builder: PageTransitionsBuilder) -> Self {
        self.builders = [Some(builder); 6];
        self
    }

    /// The builder for `platform`: its own, else the zoom, as the reference falls back
    /// (`page_transitions_theme.dart:881`) — except iOS, which slides whatever.
    pub fn builder_for(&self, platform: TargetPlatform) -> PageTransitionsBuilder {
        self.builders[index(platform)].unwrap_or(match platform {
            TargetPlatform::Ios => PageTransitionsBuilder::Cupertino,
            _ => PageTransitionsBuilder::Zoom,
        })
    }
}

/// How one page looks at a moment of a transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagePose {
    /// How far it is moved along the reading direction, in fractions of the page's
    /// width: positive towards the end edge.
    pub dx: f32,
    /// Its scale about its centre.
    pub scale: f32,
    /// Its opacity.
    pub opacity: f32,
    /// How far it is moved along the reading direction **in logical pixels**, on top of
    /// `dx`: positive towards the end edge. The shared axis's 30 px (milestone 785).
    pub shift_x: f32,
    /// How far it is moved down, in logical pixels.
    pub shift_y: f32,
}

impl PagePose {
    /// Where a page rests.
    pub const REST: PagePose = PagePose {
        dx: 0.0,
        scale: 1.0,
        opacity: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    };
}

impl From<MotionPose> for PagePose {
    fn from(pose: MotionPose) -> Self {
        PagePose {
            dx: 0.0,
            scale: pose.scale,
            opacity: pose.opacity,
            shift_x: pose.dx,
            shift_y: pose.dy,
        }
    }
}

/// What a transition paints at one moment: the two pages, and what lies between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionFrame {
    /// The page pushed, or being popped.
    pub top: PagePose,
    /// The page under it.
    pub below: PagePose,
    /// A fill of the scheme's surface **behind both pages**, at this opacity: the fade's
    /// background, what shows where the pages have faded (`page_transitions_theme.dart:541`).
    /// `0` for none.
    pub backdrop: f32,
    /// A fill of the scheme's surface **between the two pages**, at this opacity: the zoom's
    /// scrim, which hides the page under the one arriving (`:1014`). `0` for none.
    pub scrim: f32,
    /// A dimming over the page under the top one, black at this opacity: the iOS route's
    /// barrier. `0` for none.
    pub barrier: f32,
    /// The iOS page's shadow along its start edge, `0..=1` of its strength. `0` for none.
    pub edge_shadow: f32,
    /// Whether `dx` turns round under a right-to-left layout: the iOS slide does, the fade
    /// does not (the reference's `SlideTransition` there is given no direction).
    pub mirrored: bool,
}

/// The iOS route's barrier colour, black at `0x18` (`route.dart:48`).
pub const CUPERTINO_BARRIER_OPACITY: f32 = 0x18 as f32 / 255.0;

impl PageTransitionsBuilder {
    /// How long a push or a pop takes, in seconds: 300 ms for the zoom, 500 for the iOS
    /// slide, 450 for the fade (`page_transitions_builder.dart:66`, `route.dart:155`,
    /// `page_transitions_theme.dart:470`).
    ///
    /// The motion patterns take theirs from the default motion tokens here; see
    /// [`transition_duration_in`](Self::transition_duration_in) for a theme's.
    pub fn transition_duration(self) -> f32 {
        self.transition_duration_in(&MotionTheme::M3, true)
    }

    /// How long a push (`forward`) or a pop takes under `motion`, in seconds. The
    /// platforms' own transitions keep their platform's numbers; the motion patterns
    /// read the tokens — 300 ms (`medium2`) for fade through and shared axis, 150 ms
    /// (`short3`) for a fade scale arriving and 75 ms for one leaving.
    pub fn transition_duration_in(self, motion: &MotionTheme, forward: bool) -> f32 {
        match self {
            PageTransitionsBuilder::Zoom => 0.3,
            PageTransitionsBuilder::Cupertino => 0.5,
            PageTransitionsBuilder::FadeForwards => 0.45,
            _ => {
                let pattern = self.pattern().unwrap_or(MotionPattern::FadeThrough);
                if forward {
                    pattern.duration(motion)
                } else {
                    pattern.reverse_duration(motion)
                }
            }
        }
    }

    /// The motion pattern this transition is, if it is one.
    pub fn pattern(self) -> Option<MotionPattern> {
        match self {
            PageTransitionsBuilder::FadeThrough => Some(MotionPattern::FadeThrough),
            PageTransitionsBuilder::SharedAxis(axis) => Some(MotionPattern::SharedAxis(axis)),
            PageTransitionsBuilder::FadeScale => Some(MotionPattern::FadeScale),
            _ => None,
        }
    }

    /// What is painted `progress` of the way through a transition, `0` being the old page
    /// at rest and `1` the new one. `forward` is a push; otherwise a pop, where the top
    /// page is the one leaving. `gesture` is a finger dragging a pop: the motion follows it
    /// linearly, as the reference's does while a pop gesture is under way.
    pub fn frame(self, progress: f32, forward: bool, gesture: bool) -> TransitionFrame {
        let p = progress.clamp(0.0, 1.0);
        match self {
            PageTransitionsBuilder::Zoom => zoom(p, forward),
            PageTransitionsBuilder::Cupertino => cupertino(p, forward, gesture),
            PageTransitionsBuilder::FadeForwards => fade_forwards(p, forward),
            PageTransitionsBuilder::FadeThrough => pattern(MotionPattern::FadeThrough, p, forward),
            PageTransitionsBuilder::SharedAxis(axis) => {
                pattern(MotionPattern::SharedAxis(axis), p, forward)
            }
            PageTransitionsBuilder::FadeScale => pattern(MotionPattern::FadeScale, p, forward),
        }
    }
}

/// A motion pattern between two pages: the page arriving and the page leaving, posed by
/// the pattern. On a push the page on top arrives; on a pop the page on top leaves and
/// the one under it arrives. A fade scale moves the page on top only, as a dialog does
/// over what it covers. The patterns that fill stand the pages on the surface.
fn pattern(pattern: MotionPattern, p: f32, forward: bool) -> TransitionFrame {
    let (top, below) = match (pattern, forward) {
        (MotionPattern::FadeScale, _) => (pattern.pose(p, forward, forward), MotionPose::REST),
        (_, true) => (pattern.pose(p, true, true), pattern.pose(p, false, true)),
        (_, false) => (pattern.pose(p, false, false), pattern.pose(p, true, false)),
    };
    TransitionFrame {
        top: top.into(),
        below: below.into(),
        backdrop: if pattern.fills() && p < 1.0 { 1.0 } else { 0.0 },
        scrim: 0.0,
        barrier: 0.0,
        edge_shadow: 0.0,
        mirrored: pattern.mirrored(),
    }
}

/// `inner` over `[begin, end]` of `t`, linear inside.
fn interval(begin: f32, end: f32, t: f32) -> f32 {
    Curve::Interval {
        begin,
        end,
        inner: Box::new(Curve::Linear),
    }
    .transform(t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The zoom (`page_transitions_theme.dart:116-133`, `:175-298`). Its scales move on the
/// emphasized curve; its fades and scrim on linear intervals.
fn zoom(p: f32, forward: bool) -> TransitionFrame {
    let scale =
        |from: f32, to: f32| lerp(from, to, Curve::ease_in_out_cubic_emphasized().transform(p));
    if forward {
        TransitionFrame {
            top: PagePose {
                scale: scale(0.85, 1.0),
                opacity: interval(0.125, 0.25, p),
                ..PagePose::REST
            },
            below: PagePose {
                scale: scale(1.0, 1.05),
                ..PagePose::REST
            },
            // Drawn behind the page arriving, until it has arrived (`:1014`).
            backdrop: 0.0,
            scrim: if p < 1.0 {
                0.6 * interval(0.2075, 0.4175, p)
            } else {
                0.0
            },
            barrier: 0.0,
            edge_shadow: 0.0,
            mirrored: false,
        }
    } else {
        TransitionFrame {
            top: PagePose {
                scale: scale(1.0, 0.9),
                opacity: 1.0 - interval(0.0825, 0.2075, p),
                ..PagePose::REST
            },
            below: PagePose {
                scale: scale(1.1, 1.0),
                ..PagePose::REST
            },
            backdrop: 0.0,
            scrim: 0.0,
            barrier: 0.0,
            edge_shadow: 0.0,
            mirrored: false,
        }
    }
}

/// The iOS slide (`route.dart:531-572`): the top page from the end edge on
/// `fastEaseInToSlowEaseOut` (its mirror on the way back), the one under it a third of the
/// way towards the start on `linearToEaseOut` (`easeInToLinear` on the way back); linear
/// under a finger.
fn cupertino(p: f32, forward: bool, gesture: bool) -> TransitionFrame {
    // The route's own animation, `a`: 0 when the top page is off screen, 1 at rest.
    let a = if forward { p } else { 1.0 - p };
    let (top, below) = if gesture {
        (a, a)
    } else if forward {
        (
            Curve::fast_ease_in_to_slow_ease_out().transform(a),
            Curve::linear_to_ease_out().transform(a),
        )
    } else {
        (
            Curve::Flipped(Box::new(Curve::fast_ease_in_to_slow_ease_out())).transform(a),
            Curve::ease_in_to_linear().transform(a),
        )
    };
    TransitionFrame {
        top: PagePose {
            dx: 1.0 - top,
            ..PagePose::REST
        },
        below: PagePose {
            dx: -below / 3.0,
            ..PagePose::REST
        },
        backdrop: 0.0,
        scrim: 0.0,
        // The barrier eases in with the route (`Curves.ease`, the barrier's default).
        barrier: CUPERTINO_BARRIER_OPACITY * Curve::ease().transform(a),
        edge_shadow: Curve::linear_to_ease_out().transform(a),
        mirrored: true,
    }
}

/// The fade forwards (`page_transitions_theme.dart:396-552`): a quarter of a page's width
/// on the emphasized curve, with the fades on linear intervals, and the surface behind the
/// page leaving.
fn fade_forwards(p: f32, forward: bool) -> TransitionFrame {
    let slide = Curve::ease_in_out_cubic_emphasized().transform(p);
    let (top, below) = if forward {
        (
            PagePose {
                dx: 0.25 * (1.0 - slide),
                opacity: interval(0.0, 0.75, p),
                ..PagePose::REST
            },
            PagePose {
                dx: -0.25 * slide,
                opacity: 1.0 - interval(0.0, 0.25, p),
                ..PagePose::REST
            },
        )
    } else {
        (
            PagePose {
                dx: 0.25 * slide,
                opacity: 1.0 - interval(0.0, 0.25, p),
                ..PagePose::REST
            },
            PagePose {
                dx: -0.25 * (1.0 - slide),
                opacity: interval(0.0, 0.75, p),
                ..PagePose::REST
            },
        )
    };
    TransitionFrame {
        top,
        below,
        backdrop: if p < 1.0 { 1.0 } else { 0.0 },
        scrim: 0.0,
        barrier: 0.0,
        edge_shadow: 0.0,
        mirrored: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PageTransitionsBuilder as B;

    /// **The reference's defaults**: Android fades forwards, Apple's platforms slide, the
    /// desktops and Fuchsia zoom; a theme can say otherwise for any one of them.
    #[test]
    fn each_platform_has_the_reference_s_transition() {
        let theme = PageTransitionsTheme::default();
        for platform in TargetPlatform::ALL {
            let want = match platform {
                TargetPlatform::Android => B::FadeForwards,
                TargetPlatform::Ios | TargetPlatform::MacOs => B::Cupertino,
                _ => B::Zoom,
            };
            assert_eq!(theme.builder_for(platform), want, "{platform}");
        }
        let custom = theme.with(TargetPlatform::Linux, B::Cupertino);
        assert_eq!(custom.builder_for(TargetPlatform::Linux), B::Cupertino);
        assert_eq!(custom.builder_for(TargetPlatform::Windows), B::Zoom);
        assert_eq!(B::Zoom.transition_duration(), 0.3);
        assert_eq!(B::Cupertino.transition_duration(), 0.5);
        assert_eq!(B::FadeForwards.transition_duration(), 0.45);
    }

    /// **Every transition starts with the old page at rest and ends with the new one at
    /// rest**, push or pop, with nothing left between them.
    #[test]
    fn every_transition_begins_and_ends_at_rest() {
        for builder in [
            B::Zoom,
            B::Cupertino,
            B::FadeForwards,
            B::FadeThrough,
            B::SharedAxis(SharedAxis::X),
            B::SharedAxis(SharedAxis::Y),
            B::SharedAxis(SharedAxis::Z),
            B::FadeScale,
        ] {
            for forward in [true, false] {
                let start = builder.frame(0.0, forward, false);
                let end = builder.frame(1.0, forward, false);
                // At 0 the page that was showing shows; at 1 the page that will.
                let (was, will) = if forward {
                    (start.below, end.top)
                } else {
                    (start.top, end.below)
                };
                for (pose, what) in [(was, "start"), (will, "end")] {
                    assert!(
                        pose.dx.abs() < 1e-4
                            && pose.shift_x.abs() < 1e-4
                            && pose.shift_y.abs() < 1e-4
                            && (pose.scale - 1.0).abs() < 1e-4
                            && (pose.opacity - 1.0).abs() < 1e-4,
                        "{builder:?} forward={forward} {what}: {pose:?}"
                    );
                }
                assert_eq!(
                    (end.backdrop, end.scrim),
                    (0.0, 0.0),
                    "{builder:?} leaves nothing behind"
                );
            }
        }
    }

    /// **The zoom, push**: the page grows from 85 % and fades in over 12.5–25 % of the
    /// way, over a scrim that rises to 60 % between 20.75 and 41.75 %; the page under it
    /// grows to 105 %.
    #[test]
    fn the_zoom_pushes_from_85_percent_over_a_scrim() {
        let start = B::Zoom.frame(0.0, true, false);
        assert_eq!((start.top.scale, start.top.opacity), (0.85, 0.0));
        let early = B::Zoom.frame(0.1, true, false);
        assert_eq!(early.top.opacity, 0.0, "not yet faded in");
        let quarter = B::Zoom.frame(0.25, true, false);
        assert_eq!(quarter.top.opacity, 1.0, "faded in by a quarter");
        let mid = B::Zoom.frame(0.5, true, false);
        assert!((mid.scrim - 0.6).abs() < 1e-4, "the scrim at 60 %");
        let near = B::Zoom.frame(0.999, true, false);
        assert!((near.below.scale - 1.05).abs() < 1e-2);
    }

    /// **The zoom, pop**: the page shrinks to 90 % and is gone by 20.75 %; the one under it
    /// settles from 110 %.
    #[test]
    fn the_zoom_pops_by_shrinking_away() {
        let gone = B::Zoom.frame(0.21, false, false);
        assert_eq!(gone.top.opacity, 0.0);
        let start = B::Zoom.frame(0.0, false, false);
        assert!((start.below.scale - 1.1).abs() < 1e-4);
        assert_eq!(start.scrim, 0.0, "no scrim on the way back");
    }

    /// **The iOS slide**: in from the end edge, the page under it a third of the way out,
    /// its barrier at 0x18 and its shadow at full strength at rest; under a finger, linear.
    #[test]
    fn the_cupertino_slide_parallaxes_a_third() {
        let start = B::Cupertino.frame(0.0, true, false);
        assert_eq!(start.top.dx, 1.0);
        let end = B::Cupertino.frame(1.0, true, false);
        assert!((end.below.dx + 1.0 / 3.0).abs() < 1e-4);
        assert!((end.barrier - CUPERTINO_BARRIER_OPACITY).abs() < 1e-4);
        assert!(end.mirrored);
        let half = B::Cupertino.frame(0.5, false, true);
        assert!((half.top.dx - 0.5).abs() < 1e-4, "linear under a finger");
        assert!((half.below.dx + 1.0 / 6.0).abs() < 1e-4);
        let eased = B::Cupertino.frame(0.5, false, false);
        assert!(eased.top.dx != 0.5, "eased when no finger drives it");
    }

    /// **The fade forwards**: a quarter of the width each way, the new page faded in over
    /// three quarters of the way, the old one out over the first quarter; not mirrored.
    #[test]
    fn the_fade_forwards_moves_a_quarter() {
        let start = B::FadeForwards.frame(0.0, true, false);
        assert_eq!((start.top.dx, start.top.opacity), (0.25, 0.0));
        let quarter = B::FadeForwards.frame(0.25, true, false);
        assert_eq!(quarter.below.opacity, 0.0);
        // Halfway, the new page is two thirds in: its fade runs over the first 75 %.
        let half = B::FadeForwards.frame(0.5, true, false);
        assert!(
            (half.top.opacity - 2.0 / 3.0).abs() < 1e-4,
            "{}",
            half.top.opacity
        );
        let back = B::FadeForwards.frame(0.5, false, false);
        assert!(
            (back.below.opacity - 2.0 / 3.0).abs() < 1e-4,
            "and on the way back"
        );
        let end = B::FadeForwards.frame(1.0, true, false);
        assert!((end.below.dx + 0.25).abs() < 1e-4);
        assert!(!end.mirrored);
        let pop = B::FadeForwards.frame(0.0, false, false);
        assert_eq!(pop.below.dx, -0.25);
    }

    /// **The motion patterns as page transitions**: on a push the page on top is the
    /// arriving one, on a pop the leaving one — posed by the same function as a switcher's
    /// children — over the surface; the shared X axis turns round in right-to-left.
    #[test]
    fn the_motion_patterns_pose_the_two_pages() {
        let axis = MotionPattern::SharedAxis(SharedAxis::X);
        let push = B::SharedAxis(SharedAxis::X).frame(0.4, true, false);
        assert_eq!(push.top, PagePose::from(axis.pose(0.4, true, true)));
        assert_eq!(push.below, PagePose::from(axis.pose(0.4, false, true)));
        assert!(push.top.shift_x > 0.0 && push.below.shift_x < 0.0);
        assert!(push.mirrored);
        assert_eq!(push.backdrop, 1.0, "over the surface");
        let pop = B::SharedAxis(SharedAxis::X).frame(0.4, false, false);
        assert_eq!(pop.top, PagePose::from(axis.pose(0.4, false, false)));
        assert_eq!(pop.below, PagePose::from(axis.pose(0.4, true, false)));
        assert!(
            pop.top.shift_x > 0.0,
            "the page popped goes towards the end edge"
        );
        let through = B::FadeThrough.frame(0.65, true, false);
        let posed = MotionPattern::FadeThrough.pose(0.65, true, true);
        assert!((through.top.scale - posed.scale).abs() < 1e-6);
        assert!(!through.mirrored);
    }

    /// **A fade scale moves the page on top only**; it arrives in 150 ms and leaves in 75.
    #[test]
    fn a_fade_scale_page_leaves_the_page_under_it_alone() {
        let push = B::FadeScale.frame(0.5, true, false);
        assert_eq!(push.below, PagePose::REST);
        assert!(push.top.scale < 1.0 && push.top.opacity == 1.0);
        assert_eq!(push.backdrop, 0.0);
        let pop = B::FadeScale.frame(0.5, false, false);
        assert_eq!(pop.below, PagePose::REST);
        assert!((pop.top.opacity - 0.5).abs() < 1e-6);
        let m = MotionTheme::M3;
        assert!((B::FadeScale.transition_duration_in(&m, true) - 0.15).abs() < 1e-6);
        assert!((B::FadeScale.transition_duration_in(&m, false) - 0.075).abs() < 1e-6);
        assert!((B::FadeThrough.transition_duration() - 0.3).abs() < 1e-6);
        let slow = MotionTheme {
            medium2: 0.5,
            ..MotionTheme::M3
        };
        assert_eq!(
            B::SharedAxis(SharedAxis::Y).transition_duration_in(&slow, true),
            0.5
        );
        assert_eq!(
            B::Zoom.transition_duration_in(&slow, true),
            0.3,
            "the platform's own"
        );
        let theme = PageTransitionsTheme::default().all(B::FadeScale);
        for platform in TargetPlatform::ALL {
            assert_eq!(theme.builder_for(platform), B::FadeScale);
        }
    }
}
