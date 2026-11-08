// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`MotionTheme`]: the theme's **motion tokens**: named durations, easing curves and
//! spring presets (milestone 778).
//!
//! A widget that animates asks the theme how long and along which curve, instead of
//! carrying its own numbers. An application that wants everything snappier, or a design
//! system with its own timing, changes the tokens once.

use frus_core::{Curve, SpringDescription};

/// The theme's **motion tokens**: Material 3's duration scale, its easing curves and its
/// spring presets.
///
/// Durations are in **seconds**, like every duration in this crate. Read them from the
/// theme (`theme.motion.medium2`) rather than writing a number, so an application can
/// retime the whole interface in one place.
///
/// ```
/// use frus_widgets::{AnimationController, MotionTheme, Theme};
///
/// let theme = Theme::light();
/// let mut controller = AnimationController::unit();
/// controller.forward(theme.motion.medium2, theme.motion.emphasized.clone());
///
/// // A design system with slower motion overall.
/// let calm = Theme::light().with_motion(MotionTheme {
///     medium2: 0.4,
///     ..MotionTheme::M3
/// });
/// assert_eq!(calm.motion.medium2, 0.4);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct MotionTheme {
    /// 50 ms: the smallest changes, such as a state layer appearing.
    pub short1: f32,
    /// 100 ms.
    pub short2: f32,
    /// 150 ms.
    pub short3: f32,
    /// 200 ms: small components such as a switch or a checkbox.
    pub short4: f32,
    /// 250 ms.
    pub medium1: f32,
    /// 300 ms: a common default for a component that moves or changes size.
    pub medium2: f32,
    /// 350 ms.
    pub medium3: f32,
    /// 400 ms.
    pub medium4: f32,
    /// 450 ms: larger movements, such as a sheet or a page.
    pub long1: f32,
    /// 500 ms.
    pub long2: f32,
    /// 550 ms.
    pub long3: f32,
    /// 600 ms.
    pub long4: f32,
    /// 700 ms: the longest, rare, full-screen movements.
    pub extra_long1: f32,
    /// 800 ms.
    pub extra_long2: f32,
    /// 900 ms.
    pub extra_long3: f32,
    /// 1000 ms.
    pub extra_long4: f32,
    /// The **standard** easing, `cubic-bezier(0.2, 0, 0, 1)`: for things that begin and
    /// end on screen.
    pub standard: Curve,
    /// Standard **decelerate**, `cubic-bezier(0, 0, 0, 1)`: for something entering.
    pub standard_decelerate: Curve,
    /// Standard **accelerate**, `cubic-bezier(0.3, 0, 1, 1)`: for something leaving.
    pub standard_accelerate: Curve,
    /// The **emphasized** easing: a slow start, a fast middle and a long settle, for the
    /// movements that matter most.
    pub emphasized: Curve,
    /// Emphasized **decelerate**, `cubic-bezier(0.05, 0.7, 0.1, 1)`: something important
    /// entering.
    pub emphasized_decelerate: Curve,
    /// Emphasized **accelerate**, `cubic-bezier(0.3, 0, 0.8, 0.15)`: something important
    /// leaving.
    pub emphasized_accelerate: Curve,
    /// The earlier standard easing, `cubic-bezier(0.4, 0, 0.2, 1)` ("fast out, slow in"),
    /// which many established components still move along.
    pub legacy: Curve,
    /// A **fast** spring, for small movements: a damping ratio of 0.9 and a stiffness of
    /// 1400.
    pub spring_fast: SpringDescription,
    /// The **default** spring: a damping ratio of 0.9 and a stiffness of 700.
    pub spring_default: SpringDescription,
    /// A **slow** spring, for large movements: a damping ratio of 0.9 and a stiffness of
    /// 300.
    pub spring_slow: SpringDescription,
}

/// A spring of unit mass from its damping ratio and stiffness, in a const.
const fn spring(ratio: f32, stiffness: f32, damping: f32) -> SpringDescription {
    // `damping = ratio · 2·√(m·k)` with m = 1. A const fn cannot take a square root, so
    // the caller passes the damping it works out to; the ratio is kept for the reader.
    let _ = ratio;
    SpringDescription {
        mass: 1.0,
        stiffness,
        damping,
    }
}

impl MotionTheme {
    /// Material 3's motion scale.
    pub const M3: Self = Self {
        short1: 0.05,
        short2: 0.1,
        short3: 0.15,
        short4: 0.2,
        medium1: 0.25,
        medium2: 0.3,
        medium3: 0.35,
        medium4: 0.4,
        long1: 0.45,
        long2: 0.5,
        long3: 0.55,
        long4: 0.6,
        extra_long1: 0.7,
        extra_long2: 0.8,
        extra_long3: 0.9,
        extra_long4: 1.0,
        standard: Curve::Cubic {
            x1: 0.2,
            y1: 0.0,
            x2: 0.0,
            y2: 1.0,
        },
        standard_decelerate: Curve::Cubic {
            x1: 0.0,
            y1: 0.0,
            x2: 0.0,
            y2: 1.0,
        },
        standard_accelerate: Curve::Cubic {
            x1: 0.3,
            y1: 0.0,
            x2: 1.0,
            y2: 1.0,
        },
        emphasized: Curve::ThreePointCubic {
            a1: [0.05, 0.0],
            b1: [0.133333, 0.06],
            midpoint: [0.166666, 0.4],
            a2: [0.208333, 0.82],
            b2: [0.25, 1.0],
        },
        emphasized_decelerate: Curve::Cubic {
            x1: 0.05,
            y1: 0.7,
            x2: 0.1,
            y2: 1.0,
        },
        emphasized_accelerate: Curve::Cubic {
            x1: 0.3,
            y1: 0.0,
            x2: 0.8,
            y2: 0.15,
        },
        legacy: Curve::Cubic {
            x1: 0.4,
            y1: 0.0,
            x2: 0.2,
            y2: 1.0,
        },
        // 0.9 · 2·√1400 = 67.35, 0.9 · 2·√700 = 47.62, 0.9 · 2·√300 = 31.18.
        spring_fast: spring(0.9, 1400.0, 67.350_58),
        spring_default: spring(0.9, 700.0, 47.623_52),
        spring_slow: spring(0.9, 300.0, 31.176_91),
    };
}

impl Default for MotionTheme {
    fn default() -> Self {
        Self::M3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The tokens are the scale's**: sixteen durations 50 ms apart up to 600 ms, then
    /// 100 ms apart up to a second.
    #[test]
    fn the_durations_follow_the_scale() {
        let m = MotionTheme::default();
        let all = [
            m.short1,
            m.short2,
            m.short3,
            m.short4,
            m.medium1,
            m.medium2,
            m.medium3,
            m.medium4,
            m.long1,
            m.long2,
            m.long3,
            m.long4,
            m.extra_long1,
            m.extra_long2,
            m.extra_long3,
            m.extra_long4,
        ];
        for (i, d) in all.iter().enumerate().take(12) {
            assert!((d - 0.05 * (i as f32 + 1.0)).abs() < 1e-6, "step {i}: {d}");
        }
        for (i, d) in all.iter().enumerate().skip(12) {
            assert!(
                (d - (0.7 + 0.1 * (i as f32 - 12.0))).abs() < 1e-6,
                "step {i}: {d}"
            );
        }
    }

    /// Every easing starts at 0 and ends at 1, and the decelerating ones are ahead of the
    /// accelerating ones half way.
    #[test]
    fn the_easings_run_from_zero_to_one_and_have_their_shapes() {
        let m = MotionTheme::default();
        for curve in [
            &m.standard,
            &m.standard_decelerate,
            &m.standard_accelerate,
            &m.emphasized,
            &m.emphasized_decelerate,
            &m.emphasized_accelerate,
            &m.legacy,
        ] {
            assert!(curve.transform(0.0).abs() < 1e-3, "{curve:?}");
            assert!((curve.transform(1.0) - 1.0).abs() < 1e-3, "{curve:?}");
        }
        assert!(m.standard_decelerate.transform(0.5) > m.standard_accelerate.transform(0.5));
        assert!(m.emphasized_decelerate.transform(0.5) > m.emphasized_accelerate.transform(0.5));
        assert_eq!(m.emphasized, Curve::ease_in_out_cubic_emphasized());
    }

    /// The springs are the ratios and stiffnesses they say, and slower ones are softer.
    #[test]
    fn the_springs_are_what_they_say() {
        let m = MotionTheme::default();
        for (s, k) in [
            (m.spring_fast, 1400.0),
            (m.spring_default, 700.0),
            (m.spring_slow, 300.0),
        ] {
            let expected = SpringDescription::with_damping_ratio(1.0, k, 0.9);
            assert_eq!(s.stiffness, expected.stiffness);
            assert!((s.damping - expected.damping).abs() < 1e-3, "{s:?}");
        }
    }
}
