// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`RollingDigits`]: a number whose digits **roll** to a new value, like the wheels of a
//! counter (milestone 811).
//!
//! Give it a value; when the value changes, the number travels from where it is to the new
//! one over the theme's duration and curve, and each column of digits turns like an
//! odometer's wheel: the units turn freely and every other wheel turns only while the wheel
//! under it passes from nine to zero. A number going up rolls upwards, one going down rolls
//! back, and a value changed while the last change is still rolling sets off from where it
//! got to.
//!
//! Every digit is drawn in a cell as wide as the widest digit of the font, with the font's
//! tabular figures asked for too, so the number does not shimmy as it turns. A leading
//! wheel that is still at zero is not drawn, and fades in as it turns to one. With motion
//! turned off by the reader, the number changes at once.

use std::cell::{Cell, RefCell};

use frus_core::{
    Color, Curve, Point, Rect, ResolvedTextStyle, Role, Scene, SemanticsProperties, TextStyle,
};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// Theme defaults for [`RollingDigits`] (`theme.widgets.rolling_digits`); each `None`
/// falls through to the default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RollingDigitsTheme {
    /// The digits' style. Unset, `headline_medium`.
    pub text_style: Option<TextStyle>,
    /// The digits' colour. Unset, `on_surface`.
    pub color: Option<Color>,
    /// How long a change rolls, in seconds. Unset, the theme's `motion.long2`.
    pub duration: Option<f32>,
    /// Along which curve. Unset, the theme's `motion.emphasized_decelerate`.
    pub curve: Option<Curve>,
}

/// A number whose digits **roll** to each new value.
///
/// ```
/// use frus_widgets::RollingDigits;
///
/// // 12,480 — rolling there from whatever it showed before.
/// let _visitors: RollingDigits = RollingDigits::new(12_480.0);
///
/// // $1,299.50, its cents rolling too.
/// let _total: RollingDigits = RollingDigits::new(1299.5).decimals(2).prefix("$");
///
/// // 87 %, rolling over a full second.
/// let _score: RollingDigits = RollingDigits::new(87.0).suffix(" %").duration(1.0);
/// ```
///
/// The thousands are grouped with the separator of the reader's language unless told
/// ([`RollingDigits::group_separator`]); a screen reader hears the value as it is written.
/// **Themed** through [`RollingDigitsTheme`] (`theme.widgets.rolling_digits`).
pub struct RollingDigits<Msg = crate::callback::Callback> {
    value: f64,
    decimals: u32,
    prefix: String,
    suffix: String,
    group_separator: Option<String>,
    decimal_separator: String,
    style: Option<TextStyle>,
    color: Option<Color>,
    duration: Option<f32>,
    curve: Option<Curve>,
    /// The duration and curve the theme resolved, read by the runtime through
    /// `anim_duration` and `anim_curve`, which are not told the theme.
    timing: Cell<f32>,
    easing: RefCell<Curve>,
    _msg: std::marker::PhantomData<Msg>,
}

impl<Msg> RollingDigits<Msg> {
    /// A rolling number showing `value`.
    pub fn new(value: f64) -> Self {
        Self {
            value,
            decimals: 0,
            prefix: String::new(),
            suffix: String::new(),
            group_separator: None,
            decimal_separator: ".".into(),
            style: None,
            color: None,
            duration: None,
            curve: None,
            timing: Cell::new(crate::MotionTheme::M3.long2),
            easing: RefCell::new(crate::MotionTheme::M3.emphasized_decelerate),
            _msg: std::marker::PhantomData,
        }
    }

    /// How many digits after the decimal separator (they roll too). Clamped to 6.
    pub fn decimals(mut self, decimals: u32) -> Self {
        self.decimals = decimals.min(6);
        self
    }

    /// Text before the number — a currency sign.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Text after the number — a unit.
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// What separates the thousands; `""` for nothing. Unset, the reader's language's.
    pub fn group_separator(mut self, separator: impl Into<String>) -> Self {
        self.group_separator = Some(separator.into());
        self
    }

    /// What separates the decimals. Unset, `.`.
    pub fn decimal_separator(mut self, separator: impl Into<String>) -> Self {
        self.decimal_separator = separator.into();
        self
    }

    /// The digits' style, over the theme's.
    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// The style a container gives the digits when they were given none: a card's figure.
    pub(crate) fn style_unless_set(mut self, style: TextStyle) -> Self {
        self.style.get_or_insert(style);
        self
    }

    /// The digits' colour, over the theme's.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// How long a change rolls, in seconds, over the theme's.
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = Some(seconds);
        self
    }

    /// Along which curve a change rolls, over the theme's.
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = Some(curve);
        self
    }

    /// The value as whole units of the last decimal: what rolls.
    fn count(&self) -> f64 {
        (self.value * 10f64.powi(self.decimals as i32)).round()
    }

    /// The thousands separator in force.
    fn grouping(&self) -> String {
        match &self.group_separator {
            Some(separator) => separator.clone(),
            None => crate::localizations::of()
                .group_digits(1000)
                .trim_matches(|c: char| c.is_ascii_digit())
                .to_string(),
        }
    }

    /// The value written out, as a reader hears it and as it rests on the screen.
    pub fn formatted(&self) -> String {
        let count = self.count();
        let digits = columns_for(count.abs(), self.decimals);
        let mut out = String::new();
        if count < 0.0 {
            out.push('-');
        }
        out.push_str(&self.prefix);
        let group = self.grouping();
        for p in (0..digits).rev() {
            out.push_str(&digit_at(count.abs(), p).to_string());
            out.push_str(self.separator_after(p, &group));
        }
        out.push_str(&self.suffix);
        out
    }

    /// What is written after the column for place `p` (counted from the last decimal).
    fn separator_after<'a>(&'a self, p: u32, group: &'a str) -> &'a str {
        let d = self.decimals;
        if d > 0 && p == d {
            &self.decimal_separator
        } else if p > d && (p - d).is_multiple_of(3) {
            group
        } else {
            ""
        }
    }

    /// The style the digits are measured and drawn in, figures of one width asked for.
    fn resolved_style(&self, theme: Option<&Theme>) -> ResolvedTextStyle {
        self.style
            .or(theme.and_then(|t| t.widgets.rolling_digits.text_style))
            .unwrap_or_else(|| crate::theme::type_scale(theme).headline_medium)
            .font_feature("tnum", 1)
            .resolved()
    }

    fn sizing(&self, theme: Option<&Theme>) -> Style {
        let style = self.resolved_style(theme);
        let m = Metrics::of(&style);
        let width = self.layout_width(self.count(), &m);
        Style {
            width: Dimension::Length(width),
            height: Dimension::Length(style.line_height()),
            ..Default::default()
        }
    }

    /// The width of the number as it stands at `count`.
    fn layout_width(&self, count: f64, m: &Metrics) -> f32 {
        self.width_for(columns_for(count.abs(), self.decimals), count < 0.0, m)
    }

    /// The width of `columns` digits with their separators and affixes.
    fn width_for(&self, columns: u32, negative: bool, m: &Metrics) -> f32 {
        let group = self.grouping();
        let mut w = m.width(&self.prefix) + m.width(&self.suffix);
        if negative {
            w += m.width("-");
        }
        for p in 0..columns {
            w += m.digit + m.width(self.separator_after(p, &group));
        }
        w
    }
}

/// How many columns `count` needs: at least one more than the decimals.
fn columns_for(count: f64, decimals: u32) -> u32 {
    let mut n = 1;
    let mut rest = (count / 10.0).floor();
    while rest >= 1.0 && n < 30 {
        n += 1;
        rest = (rest / 10.0).floor();
    }
    n.max(decimals + 1)
}

/// The digit at place `p` of the whole number `count`.
fn digit_at(count: f64, p: u32) -> u32 {
    ((count / 10f64.powi(p as i32)).floor() % 10.0) as u32
}

/// Where each wheel stands, in digits, for a counter at `a` (a magnitude, possibly between
/// two whole numbers): the units wheel at `a`, and each wheel above at the count of whole
/// turns below it, plus the part of a turn the wheel under it makes from nine to ten.
///
/// Every value here counts all the turns, not just the last: the digit shown is the value
/// modulo ten, and a wheel above every digit of the number stands below one — which is
/// what tells a leading zero to stay hidden.
fn wheels(a: f64, columns: u32) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::with_capacity(columns as usize);
    for p in 0..columns {
        let pos = if p == 0 {
            a
        } else {
            let below = out[p as usize - 1].rem_euclid(10.0);
            (a / 10f64.powi(p as i32)).floor() + (below - 9.0).clamp(0.0, 1.0)
        };
        out.push(pos);
    }
    out
}

/// The widths a number is laid out with.
struct Metrics<'a> {
    style: &'a ResolvedTextStyle,
    /// The widest digit: every column's width.
    digit: f32,
}

impl<'a> Metrics<'a> {
    fn of(style: &'a ResolvedTextStyle) -> Self {
        let digit = (0..10)
            .map(|d| frus_text::measure_resolved(&d.to_string(), style).width)
            .fold(0.0, f32::max);
        Self { style, digit }
    }

    fn width(&self, text: &str) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            frus_text::measure_resolved(text, self.style).width
        }
    }
}

impl<Msg> Widget<Msg> for RollingDigits<Msg> {
    fn style(&self) -> Style {
        self.sizing(None)
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        self.sizing(Some(theme))
    }

    fn build_themed(&self, theme: &Theme) {
        let t = &theme.widgets.rolling_digits;
        self.timing
            .set(self.duration.or(t.duration).unwrap_or(theme.motion.long2));
        *self.easing.borrow_mut() = self
            .curve
            .clone()
            .or_else(|| t.curve.clone())
            .unwrap_or_else(|| theme.motion.emphasized_decelerate.clone());
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    /// The count the runtime rolls towards.
    fn anim_target(&self) -> Option<f32> {
        Some(self.count() as f32)
    }

    fn anim_duration(&self) -> f32 {
        if crate::MediaQuery::reduce_motion() {
            0.0
        } else {
            self.duration.unwrap_or(self.timing.get())
        }
    }

    fn anim_curve(&self) -> Curve {
        self.curve
            .clone()
            .unwrap_or_else(|| self.easing.borrow().clone())
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        Some(SemanticsProperties::new(Role::Label).value(self.formatted()))
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let style = self.resolved_style(Some(theme));
        let m = Metrics::of(&style);
        let color = self
            .color
            .or(theme.widgets.rolling_digits.color)
            .unwrap_or(theme.scheme.on_surface)
            .fade(status.opacity);
        let target = self.count();
        // At rest, the exact number: the runtime carries it as a single-precision float,
        // which is not exact past sixteen million.
        let now = if (status.value as f64 - target).abs() < 1e-3 {
            target
        } else {
            status.value as f64
        };
        let a = now.abs();
        let columns = columns_for(a.max(target.abs()), self.decimals);
        let positions = wheels(a, columns);
        let group = self.grouping();
        let line = style.line_height();
        let y = bounds.y + (bounds.height - line) * 0.5;
        // Laid out at the target's width; a wheel more, while a number shrinks, hangs off
        // the start and fades.
        let negative = now < -0.5 || (now < 0.0 && target < 0.0);
        let mut x = bounds.x + bounds.width - self.width_for(columns, negative, &m);
        let ambient = scene.current_clip();
        if negative {
            scene.text(Point::new(x, y), "-", &style, color);
            x += m.width("-");
        }
        if !self.prefix.is_empty() {
            scene.text(Point::new(x, y), self.prefix.clone(), &style, color);
            x += m.width(&self.prefix);
        }
        for p in (0..columns).rev() {
            let pos = positions[p as usize];
            // A wheel above the number's first digit is a leading zero: hidden, and coming
            // in as it turns towards one.
            let leading = p > self.decimals;
            let alpha = if leading {
                pos.clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            if alpha > 0.0 {
                let turn = pos.rem_euclid(10.0);
                let shown = turn.floor();
                let part = (turn - shown) as f32;
                let cell = Rect::new(x, y, m.digit, line);
                scene.set_clip(ambient.intersect(cell));
                let ink = color.fade(alpha);
                let glyph = |d: f64| ((d as u32) % 10).to_string();
                let w0 = m.width(&glyph(shown));
                scene.text(
                    Point::new(x + (m.digit - w0) * 0.5, y - part * line),
                    glyph(shown),
                    &style,
                    ink,
                );
                if part > 0.0 {
                    let next = glyph(shown + 1.0);
                    let w1 = m.width(&next);
                    scene.text(
                        Point::new(x + (m.digit - w1) * 0.5, y + (1.0 - part) * line),
                        next,
                        &style,
                        ink,
                    );
                }
                scene.set_clip(ambient);
            }
            x += m.digit;
            let separator = self.separator_after(p, &group);
            if !separator.is_empty() {
                if alpha > 0.0 {
                    scene.text(
                        Point::new(x, y),
                        separator.to_string(),
                        &style,
                        color.fade(alpha),
                    );
                }
                x += m.width(separator);
            }
        }
        if !self.suffix.is_empty() {
            scene.text(Point::new(x, y), self.suffix.clone(), &style, color);
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "RollingDigits"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};
    use frus_core::Primitive;

    fn digits(n: &RollingDigits<()>) -> String {
        n.formatted()
    }

    #[test]
    fn it_formats_with_groups_decimals_and_affixes() {
        assert_eq!(digits(&RollingDigits::new(1234567.0)), "1,234,567");
        assert_eq!(digits(&RollingDigits::new(0.0)), "0");
        assert_eq!(
            digits(&RollingDigits::new(1299.5).decimals(2).prefix("$")),
            "$1,299.50"
        );
        assert_eq!(digits(&RollingDigits::new(-42.0).suffix(" pts")), "-42 pts");
        assert_eq!(
            digits(
                &RollingDigits::new(12345.678)
                    .decimals(1)
                    .group_separator(" ")
                    .decimal_separator(",")
            ),
            "12 345,7"
        );
        assert_eq!(
            digits(&RollingDigits::new(0.05).decimals(2)),
            "0.05",
            "a leading zero before the decimals stays"
        );
    }

    /// **The wheels turn like an odometer's**: the units freely, every other wheel only
    /// while the one under it passes from nine to zero.
    #[test]
    fn the_wheels_carry_like_an_odometer() {
        let at = |a: f64| wheels(a, 3);
        assert_eq!(at(19.0), vec![19.0, 1.0, 0.0], "at rest, whole digits");
        let mid = at(19.5);
        assert!((mid[0] - 19.5).abs() < 1e-9);
        assert!(
            (mid[1] - 1.5).abs() < 1e-9,
            "the tens turn with the nine: {mid:?}"
        );
        let low = at(14.5);
        assert!((low[1] - 1.0).abs() < 1e-9, "and not before: {low:?}");
        let all = at(99.5);
        assert!(
            (all[2] - 0.5).abs() < 1e-9,
            "a carry goes all the way up: {all:?}"
        );
    }

    /// The x of every glyph drawn, with the glyph.
    fn screen(n: RollingDigits<()>) -> crate::Flex<()> {
        crate::Flex::<()>::column()
            .align(frus_layout::Align::Start)
            .child(n)
    }

    fn glyphs(n: RollingDigits<()>, runtime: &Runtime) -> Vec<(f32, f32, String, f32)> {
        let screen = screen(n);
        let ui = build_ui(&screen, Size::new(400.0, 200.0), runtime, &Theme::light());
        ui.scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                Primitive::Text {
                    position,
                    text,
                    color,
                    ..
                } => Some((position.x, position.y, text.clone(), color.a)),
                _ => None,
            })
            .collect()
    }

    /// **Each digit sits in a cell as wide as the widest digit**, so "1" and "8" take the
    /// same room and the number does not shimmy as it turns.
    #[test]
    fn digits_sit_in_cells_of_one_width() {
        let style = TextStyle::new(20.0);
        let ones = glyphs(RollingDigits::new(111.0).style(style), &Runtime::default());
        let eights = glyphs(RollingDigits::new(888.0).style(style), &Runtime::default());
        assert_eq!(ones.len(), 3);
        let step = |g: &[(f32, f32, String, f32)]| g[1].0 - g[0].0;
        assert!(
            (step(&ones) - step(&eights)).abs() < 0.01,
            "{ones:?} {eights:?}"
        );
        let n = RollingDigits::<()>::new(111.0).style(style);
        let a = Widget::<()>::style(&n).width;
        let b = Widget::<()>::style(&RollingDigits::<()>::new(888.0).style(style)).width;
        assert_eq!(a, b, "one width for any digits");
    }

    /// **A new value rolls there and arrives**: half way, two glyphs share a column; at
    /// the end, the new digits alone, exactly.
    #[test]
    fn a_change_rolls_and_ends_on_the_value() {
        let mut runtime = Runtime::default();
        let make = |v: f64| {
            RollingDigits::<()>::new(v)
                .style(TextStyle::new(20.0))
                .duration(1.0)
                .curve(Curve::Linear)
        };
        // First seen at 10: no roll on appearing.
        runtime.advance_values::<()>(&screen(make(10.0)), 0.0);
        assert_eq!(glyphs(make(10.0), &runtime).len(), 2);
        // Changed to 11: half a second in, the units are between 0 and 1.
        runtime.advance_values::<()>(&screen(make(11.0)), 0.0);
        let moving = runtime.advance_values::<()>(&screen(make(11.0)), 0.5);
        assert!(moving, "still rolling");
        let mid = glyphs(make(11.0), &runtime);
        let units: Vec<&String> = mid.iter().skip(1).map(|g| &g.2).collect();
        assert_eq!(units, ["0", "1"], "the units between two digits: {mid:?}");
        assert!(mid[1].1 < mid[2].1, "the new digit comes up from below");
        let done = runtime.advance_values::<()>(&screen(make(11.0)), 0.6);
        assert!(!done, "the roll ends");
        let end = glyphs(make(11.0), &runtime);
        let text: String = end.iter().map(|g| g.2.as_str()).collect();
        assert_eq!(text, "11");
    }

    /// **A wheel that has not reached one yet is not drawn**: going from 9 to 10, the tens
    /// wheel fades in as it turns.
    #[test]
    fn a_leading_wheel_fades_in() {
        let mut runtime = Runtime::default();
        let make = |v: f64| {
            RollingDigits::<()>::new(v)
                .style(TextStyle::new(20.0))
                .duration(1.0)
                .curve(Curve::Linear)
        };
        runtime.advance_values::<()>(&screen(make(9.0)), 0.0);
        runtime.advance_values::<()>(&screen(make(10.0)), 0.0);
        runtime.advance_values::<()>(&screen(make(10.0)), 0.25);
        let early = glyphs(make(10.0), &runtime);
        // 9.25: the tens wheel stands at 0.25 of a turn, drawn faint.
        let tens = &early[0];
        assert!(tens.3 > 0.0 && tens.3 < 0.5, "faint: {early:?}");
    }

    /// **Reduced motion: the value changes at once.**
    #[test]
    fn reduced_motion_changes_at_once() {
        let still = crate::MediaQuery::new(Size::new(400.0, 300.0)).with_accessibility(
            crate::Accessibility {
                disable_animations: true,
                ..crate::Accessibility::NONE
            },
        );
        let _guard = still.install();
        let n = RollingDigits::<()>::new(5.0).duration(2.0);
        assert_eq!(Widget::<()>::anim_duration(&n), 0.0);
    }

    /// **The theme times the roll** and sizes the digits a widget did not.
    #[test]
    fn the_theme_sets_timing_and_style() {
        let mut theme = Theme::light();
        theme.widgets.rolling_digits.duration = Some(0.9);
        let n = RollingDigits::<()>::new(1.0);
        n.build_themed(&theme);
        assert_eq!(Widget::<()>::anim_duration(&n), 0.9);
        let own = RollingDigits::<()>::new(1.0).duration(0.2);
        own.build_themed(&theme);
        assert_eq!(Widget::<()>::anim_duration(&own), 0.2);
        theme.widgets.rolling_digits.text_style = Some(TextStyle::new(40.0));
        let big = Widget::<()>::style_themed(&n, &theme).height;
        let small = Widget::<()>::style_themed(&n, &Theme::light()).height;
        assert!(matches!((big, small), (Dimension::Length(b), Dimension::Length(s)) if b > s));
    }

    /// **A reader hears the value as it is written.**
    #[test]
    fn it_reads_as_its_value() {
        let n = RollingDigits::<()>::new(2048.0).prefix("$");
        let sem = Widget::<()>::semantics(&n).expect("semantics");
        assert_eq!(sem.role, Role::Label);
        assert_eq!(sem.value.as_deref(), Some("$2,048"));
    }
}
