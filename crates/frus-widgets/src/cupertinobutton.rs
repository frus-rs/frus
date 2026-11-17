// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoButton`]: the Apple-style button — plain, tinted or filled, in three sizes,
//! answering a press by **dimming** rather than by ink (milestone 812).
//!
//! ```
//! use frus_widgets::{CupertinoButton, CupertinoButtonSize};
//!
//! let _plain: CupertinoButton<()> = CupertinoButton::new("Edit").on_press(());
//! let _filled: CupertinoButton<()> = CupertinoButton::new("Continue")
//!     .filled()
//!     .size(CupertinoButtonSize::Medium)
//!     .on_press(());
//! ```
//!
//! Its colours come from the Apple-style theme ([`CupertinoTheme`]): the primary colour is
//! the label of a plain button and the fill of a filled one; a tinted button is filled with
//! it at 12 % in the light and 26 % in the dark. Everything can be overridden on the button.

use frus_core::{BorderRadius, Color, Curve, Insets, Rect, Scene, Size};
use frus_layout::{Align, Dimension, Justify, Style};

use crate::cupertinocolors::CupertinoColors;
use crate::cupertinotheme::CupertinoTheme;
use crate::interaction::Status;
use crate::media::Brightness;
use crate::theme::Theme;
use crate::widget::Widget;

/// How long a press takes to dim the button, in the reference's eyes: 120 ms out and 180 ms
/// back. The framework's press runs 200 ms each way; [`pressed_fade`] eases it so that most
/// of the dimming happens early, as a short fade-out does.
const FADE_CURVE: Curve = Curve::Cubic {
    x1: 0.215,
    y1: 0.61,
    x2: 0.355,
    y2: 1.0,
};

/// The opacity a press has brought a button down to: `1` at rest, `pressed` fully held,
/// eased between (milestone 812).
pub(crate) fn pressed_fade(pressed: f32, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    if p <= 0.0 {
        return 1.0;
    }
    let eased = if p >= 1.0 {
        1.0
    } else {
        FADE_CURVE.transform(p)
    };
    1.0 - (1.0 - pressed.clamp(0.0, 1.0)) * eased
}

/// The opacity a pressed button dims to unless told (`button.dart:87`).
pub const CUPERTINO_PRESSED_OPACITY: f32 = 0.4;
/// A tinted button's fill: the colour at 12 % in the light (`constants.dart:49`)…
pub const CUPERTINO_TINTED_OPACITY_LIGHT: f32 = 0.12;
/// …and at 26 % in the dark.
pub const CUPERTINO_TINTED_OPACITY_DARK: f32 = 0.26;
/// The ring round a focused button, outside its edge (`button.dart:529`).
const FOCUS_RING: f32 = 3.5;

/// How big a [`CupertinoButton`] is: its padding, its corners and its least size, as Apple's
/// guidelines give them (`constants.dart:62`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CupertinoButtonSize {
    /// 12 across and 6 down around the label, a pill, at least 28 square; the label at the
    /// small action size.
    Small,
    /// 15 across and 10 down, a pill, at least 32 square.
    Medium,
    /// 20 across and 16 down, 12-pixel corners, at least 44 square — the default.
    #[default]
    Large,
}

impl CupertinoButtonSize {
    /// The room around the label.
    pub const fn padding(self) -> Insets {
        match self {
            CupertinoButtonSize::Small => Insets::new(6.0, 12.0, 6.0, 12.0),
            CupertinoButtonSize::Medium => Insets::new(10.0, 15.0, 10.0, 15.0),
            CupertinoButtonSize::Large => Insets::new(16.0, 20.0, 16.0, 20.0),
        }
    }

    /// The corners' radius.
    pub const fn radius(self) -> f32 {
        match self {
            CupertinoButtonSize::Small | CupertinoButtonSize::Medium => 40.0,
            CupertinoButtonSize::Large => 12.0,
        }
    }

    /// The least width and height.
    pub const fn min_size(self) -> f32 {
        match self {
            CupertinoButtonSize::Small => 28.0,
            CupertinoButtonSize::Medium => 32.0,
            CupertinoButtonSize::Large => 44.0,
        }
    }
}

/// Which of the three Apple-style buttons it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CupertinoButtonStyle {
    /// No fill: a label in the primary colour.
    #[default]
    Plain,
    /// A light wash of the primary colour under a label in it.
    Tinted,
    /// The primary colour, under a label in the colour that goes on it.
    Filled,
}

/// **An Apple-style button**: any widget, or a label, that dims while it is pressed.
///
/// Plain by default; [`tinted`](Self::tinted) and [`filled`](Self::filled) give it a fill.
/// Large by default — 44 pixels at least, the size of a finger — and
/// [`size`](Self::size) makes it smaller. A button without [`on_press`](Self::on_press) is
/// disabled: its label turns to the tertiary label colour and a fill to the disabled colour,
/// and it no longer takes the focus or dims.
pub struct CupertinoButton<Msg = crate::callback::Callback> {
    children: Vec<Box<dyn Widget<Msg>>>,
    label: Option<String>,
    style: CupertinoButtonStyle,
    size: CupertinoButtonSize,
    on_press: Option<Msg>,
    on_long_press: Option<Msg>,
    color: Option<Color>,
    foreground_color: Option<Color>,
    disabled_color: Option<Color>,
    focus_color: Option<Color>,
    pressed_opacity: Option<f32>,
    radius: Option<BorderRadius>,
    padding: Option<Insets>,
    minimum_size: Option<Size>,
}

impl<Msg: Clone + 'static> CupertinoButton<Msg> {
    /// A button labelled `label`. The button's node says the label; the words drawn inside it
    /// say nothing, so that a reader hears it once.
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        let mut button = Self::with_child(crate::ExcludeSemantics::new(crate::Text::new(
            label.clone(),
        )));
        button.label = Some(label);
        button
    }

    /// A button holding `child`, which takes the button's text style and colour, and its
    /// icons the button's colour at 1.2 times the text size.
    pub fn with_child(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            children: vec![Box::new(child)],
            label: None,
            style: CupertinoButtonStyle::Plain,
            size: CupertinoButtonSize::Large,
            on_press: None,
            on_long_press: None,
            color: None,
            foreground_color: None,
            disabled_color: None,
            focus_color: None,
            pressed_opacity: None,
            radius: None,
            padding: None,
            minimum_size: None,
        }
    }
}

impl<Msg> CupertinoButton<Msg> {
    /// A tinted button: the primary colour — or [`color`](Self::color) — at 12 % (26 % in the
    /// dark) under a label in it.
    #[must_use]
    pub fn tinted(mut self) -> Self {
        self.style = CupertinoButtonStyle::Tinted;
        self
    }

    /// A filled button: the primary colour — or [`color`](Self::color) — under a label in
    /// the colour that goes on it.
    #[must_use]
    pub fn filled(mut self) -> Self {
        self.style = CupertinoButtonStyle::Filled;
        self
    }

    /// Which of the three it is, as a value.
    #[must_use]
    pub fn button_style(mut self, style: CupertinoButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// How big it is. [`Large`](CupertinoButtonSize::Large) by default.
    #[must_use]
    pub fn size(mut self, size: CupertinoButtonSize) -> Self {
        self.size = size;
        self
    }

    /// What a press sends. Without it the button is disabled.
    #[must_use]
    pub fn on_press(mut self, message: impl Into<Msg>) -> Self {
        self.on_press = Some(message.into());
        self
    }

    /// What a long press sends, in place of the press.
    #[must_use]
    pub fn on_long_press(mut self, message: impl Into<Msg>) -> Self {
        self.on_long_press = Some(message.into());
        self
    }

    /// Enabled or not, whatever was given to [`on_press`](Self::on_press).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        if !enabled {
            self.on_press = None;
            self.on_long_press = None;
        }
        self
    }

    /// What a screen reader says the button is, in place of its label.
    #[must_use]
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The fill. A plain button given one is filled with it as given; a tinted one takes it
    /// at the tint's opacity; a filled one in place of the primary colour.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The label's colour, in place of the style's.
    #[must_use]
    pub fn foreground_color(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    /// The fill of a disabled button that has one. The quaternary system fill by default.
    #[must_use]
    pub fn disabled_color(mut self, color: Color) -> Self {
        self.disabled_color = Some(color);
        self
    }

    /// The ring round the button while it holds the keyboard focus. By default, the fill —
    /// or the system blue — lightened and softened the way Apple's focus rings are.
    #[must_use]
    pub fn focus_color(mut self, color: Color) -> Self {
        self.focus_color = Some(color);
        self
    }

    /// The opacity it dims to while pressed: `0.4` by default, `1.0` for no dimming at all.
    #[must_use]
    pub fn pressed_opacity(mut self, opacity: f32) -> Self {
        self.pressed_opacity = Some(opacity.clamp(0.0, 1.0));
        self
    }

    /// The corners, in place of the size's.
    #[must_use]
    pub fn radius(mut self, radius: impl Into<BorderRadius>) -> Self {
        self.radius = Some(radius.into());
        self
    }

    /// The room around the label, in place of the size's.
    #[must_use]
    pub fn padding(mut self, padding: Insets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// The least size, in place of the size's.
    #[must_use]
    pub fn minimum_size(mut self, size: Size) -> Self {
        self.minimum_size = Some(size);
        self
    }

    fn enabled_now(&self) -> bool {
        self.on_press.is_some() || self.on_long_press.is_some()
    }

    /// The label's text style for this size, in the theme's type.
    fn text_style(&self, cupertino: &CupertinoTheme) -> frus_core::TextStyle {
        match self.size {
            CupertinoButtonSize::Small => cupertino.text_theme.action_small_text_style,
            _ => cupertino.text_theme.action_text_style,
        }
    }

    /// The fill, resolved (`button.dart:469`): `None` for a plain button left unfilled.
    fn fill(&self, theme: &Theme, cupertino: &CupertinoTheme) -> Option<Color> {
        let base = match (self.color, self.style) {
            (Some(c), _) => Some(c),
            (None, CupertinoButtonStyle::Plain) => None,
            (None, _) => Some(cupertino.primary_color),
        }?;
        let fill = if self.style == CupertinoButtonStyle::Tinted {
            base.with_alpha(match CupertinoTheme::brightness_of(theme) {
                Brightness::Light => CUPERTINO_TINTED_OPACITY_LIGHT,
                Brightness::Dark => CUPERTINO_TINTED_OPACITY_DARK,
            })
        } else {
            base
        };
        Some(if self.enabled_now() {
            fill
        } else {
            self.disabled_color
                .unwrap_or_else(|| CupertinoColors::QUATERNARY_SYSTEM_FILL.resolve(theme))
        })
    }

    /// The label's colour (`button.dart:482`).
    fn foreground(&self, theme: &Theme, cupertino: &CupertinoTheme) -> Color {
        if let Some(c) = self.foreground_color {
            return c;
        }
        match (self.style, self.enabled_now()) {
            (CupertinoButtonStyle::Filled, _) => cupertino.primary_contrasting_color,
            (_, true) => cupertino.primary_color,
            (_, false) => CupertinoColors::TERTIARY_LABEL.resolve(theme),
        }
    }

    fn sizing(&self) -> Style {
        let min = self
            .minimum_size
            .unwrap_or(Size::new(self.size.min_size(), self.size.min_size()));
        Style {
            min_width: Dimension::Length(min.width),
            min_height: Dimension::Length(min.height),
            padding: self.padding.unwrap_or(self.size.padding()),
            justify: Justify::Center,
            align: Align::Center,
            ..Default::default()
        }
    }
}

/// The ring colour Apple derives from a control's colour (`constants.dart:40`): at 80 %,
/// its lightness set to 0.69 and its saturation to 0.835.
pub(crate) fn focus_ring_color(base: Color) -> Color {
    let (r, g, b) = (base.r, base.g, base.b);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let hue = if (max - min).abs() < 1e-6 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / (max - min)).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / (max - min) + 2.0)
    } else {
        60.0 * ((r - g) / (max - min) + 4.0)
    };
    let (l, s) = (0.69_f32, 0.835_f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match hue {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::rgba(r1 + m, g1 + m, b1 + m, 0.8 * base.a)
}

impl<Msg: Clone + 'static> Widget<Msg> for CupertinoButton<Msg> {
    fn style(&self) -> Style {
        self.sizing()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    /// The content's text takes the button's action style in its colour, and its icons that
    /// colour at 1.2 times the text's size (`button.dart:501`).
    fn theme_override(&self, inherited: &Theme) -> Option<Box<Theme>> {
        let cupertino = CupertinoTheme::of(inherited);
        let fg = self.foreground(inherited, &cupertino);
        let style = self.text_style(&cupertino).color(fg);
        let mut theme = Box::new(inherited.clone());
        theme.widgets.text = crate::DefaultTextStyle::from_text_style(style);
        theme.widgets.icon.color = Some(fg);
        theme.widgets.icon.size = Some(style.size.unwrap_or(17.0) * 1.2);
        Some(theme)
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let cupertino = CupertinoTheme::of(theme);
        let radius = self
            .radius
            .unwrap_or(BorderRadius::uniform(self.size.radius()));
        let fill = self.fill(theme, &cupertino);
        if let Some(fill) = fill {
            scene.draw_rect(bounds, fill.fade(o), radius, 0.0, Color::TRANSPARENT);
        }
        // The focus ring, outside the edge, while the keyboard is on it.
        if self.enabled_now() && status.focus_progress > 0.0 {
            let ring = self.focus_color.unwrap_or_else(|| {
                focus_ring_color(fill.unwrap_or(CupertinoColors::ACTIVE_BLUE.resolve(theme)))
            });
            let grown = Rect::new(
                bounds.x - FOCUS_RING,
                bounds.y - FOCUS_RING,
                bounds.width + 2.0 * FOCUS_RING,
                bounds.height + 2.0 * FOCUS_RING,
            );
            scene.draw_rect(
                grown,
                Color::TRANSPARENT,
                BorderRadius {
                    top_left: radius.top_left + FOCUS_RING,
                    top_right: radius.top_right + FOCUS_RING,
                    bottom_right: radius.bottom_right + FOCUS_RING,
                    bottom_left: radius.bottom_left + FOCUS_RING,
                },
                FOCUS_RING,
                ring.fade(o * status.focus_progress),
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        self.on_press.clone()
    }

    fn on_long_press(&self) -> Option<Msg> {
        self.on_long_press.clone()
    }

    fn focusable(&self) -> bool {
        self.enabled_now()
    }

    fn pressed_opacity(&self) -> Option<f32> {
        let opacity = self.pressed_opacity.unwrap_or(CUPERTINO_PRESSED_OPACITY);
        (self.enabled_now() && opacity < 1.0).then_some(opacity)
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut semantics = frus_core::SemanticsProperties::new(frus_core::Role::Button);
        if let Some(label) = &self.label {
            semantics = semantics.label(label.clone());
        }
        Some(if self.enabled_now() {
            semantics.clickable()
        } else {
            semantics.disabled(true)
        })
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoButton"
    }
}

/// Every primitive of a scene, those inside layers included, in painting order: what the
/// tests of the Apple-style widgets look through, since a blurred bar or a faded button puts
/// its content in a layer.
#[cfg(test)]
pub(crate) trait AllPrimitives {
    fn all(&self) -> Vec<&frus_core::Primitive>;
}

#[cfg(test)]
impl AllPrimitives for Scene {
    fn all(&self) -> Vec<&frus_core::Primitive> {
        fn walk<'a>(list: &'a [frus_core::Primitive], out: &mut Vec<&'a frus_core::Primitive>) {
            for p in list {
                out.push(p);
                if let frus_core::Primitive::Layer { primitives, .. } = p {
                    walk(primitives, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self.primitives(), &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Ui};
    use frus_core::{Point, Primitive};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Go,
    }

    fn rects(ui: &Ui<Msg>) -> Vec<(Rect, Color)> {
        ui.scene()
            .all()
            .into_iter()
            .filter_map(|p| match p {
                Primitive::Rect { rect, color, .. } => Some((*rect, *color)),
                _ => None,
            })
            .collect()
    }

    /// A button laid out alone, centred in a large window.
    fn lay(button: CupertinoButton<Msg>, theme: &Theme, rt: &Runtime) -> Ui<Msg> {
        let root = crate::Center::new(button);
        build_ui(&root, Size::new(400.0, 300.0), rt, theme)
    }

    /// A button holding an empty 10 × 10 box.
    fn blank() -> CupertinoButton<Msg> {
        CupertinoButton::with_child(crate::SizedBox::<Msg>::empty().width(10.0).height(10.0))
    }

    fn has_layer(ui: &Ui<Msg>) -> bool {
        ui.scene()
            .all()
            .into_iter()
            .any(|p| matches!(p, Primitive::Layer { .. }))
    }

    /// **Each size has the reference's least size, padding and corners** (`constants.dart:62`).
    #[test]
    fn each_size_is_the_reference_s() {
        assert_eq!(CupertinoButtonSize::Small.min_size(), 28.0);
        assert_eq!(CupertinoButtonSize::Medium.min_size(), 32.0);
        assert_eq!(CupertinoButtonSize::Large.min_size(), 44.0);
        assert_eq!(
            CupertinoButtonSize::Medium.padding(),
            Insets::new(10.0, 15.0, 10.0, 15.0)
        );
        assert_eq!(CupertinoButtonSize::Large.radius(), 12.0);
        assert_eq!(CupertinoButtonSize::Small.radius(), 40.0);
    }

    /// **The box is the padding round the content, at least the size's least**: a 10-pixel
    /// child in a large button is 50 wide by the padding, and the least size lifts its
    /// height to 44.
    #[test]
    fn the_box_is_the_padding_round_the_content() {
        let theme = Theme::light();
        let rt = Runtime::default();
        let ui = lay(blank().filled().on_press(Msg::Go), &theme, &rt);
        let (fill, _) = rects(&ui)[0];
        assert_eq!((fill.width, fill.height), (50.0, 44.0));
        let small = blank()
            .filled()
            .size(CupertinoButtonSize::Small)
            .on_press(Msg::Go);
        let (fill, _) = rects(&lay(small, &theme, &rt))[0];
        assert_eq!((fill.width, fill.height), (34.0, 28.0));
    }

    /// **Plain has no fill; filled is the primary colour; tinted is it at 12 % in the light
    /// and 26 % in the dark** (`button.dart:469`).
    #[test]
    fn the_three_styles_fill_as_the_reference_s() {
        let rt = Runtime::default();
        let light = Theme::light();
        assert!(rects(&lay(blank().on_press(Msg::Go), &light, &rt)).is_empty());
        let primary = CupertinoTheme::of(&light).primary_color;
        let (_, filled) = rects(&lay(blank().filled().on_press(Msg::Go), &light, &rt))[0];
        assert_eq!(filled, primary);
        let (_, tinted) = rects(&lay(blank().tinted().on_press(Msg::Go), &light, &rt))[0];
        assert_eq!(tinted, primary.with_alpha(0.12));
        let dark = Theme::dark();
        let primary = CupertinoTheme::of(&dark).primary_color;
        let (_, tinted) = rects(&lay(blank().tinted().on_press(Msg::Go), &dark, &rt))[0];
        assert_eq!(tinted, primary.with_alpha(0.26));
        // A colour of the caller's takes the primary's place, at the tint's opacity.
        let red = Color::rgb(1.0, 0.0, 0.0);
        let own = blank().tinted().color(red).on_press(Msg::Go);
        let (_, own) = rects(&lay(own, &light, &rt))[0];
        assert_eq!(own, red.with_alpha(0.12));
    }

    /// **The label takes the style's colour**: the primary on a plain button, the colour on
    /// the primary on a filled one, the tertiary label when disabled (`button.dart:482`).
    #[test]
    fn the_label_takes_the_style_s_colour() {
        let theme = Theme::light();
        let c = CupertinoTheme::of(&theme);
        let label = |b: CupertinoButton<Msg>| {
            let t = Widget::<Msg>::theme_override(&b, &theme).expect("it themes its content");
            t.widgets.text.style.color
        };
        assert_eq!(
            label(CupertinoButton::new("A").on_press(Msg::Go)),
            Some(c.primary_color)
        );
        assert_eq!(
            label(CupertinoButton::new("A").filled().on_press(Msg::Go)),
            Some(c.primary_contrasting_color)
        );
        assert_eq!(
            label(CupertinoButton::new("A")),
            Some(CupertinoColors::TERTIARY_LABEL.resolve(&theme))
        );
        // Icons inside take the colour at 1.2 times the text: 17 × 1.2.
        let t = Widget::<Msg>::theme_override(&CupertinoButton::<Msg>::new("A"), &theme).unwrap();
        assert_eq!(t.widgets.icon.size, Some(17.0 * 1.2));
    }

    /// **A press dims the whole button to 0.4**, its fill and its content as one layer; at
    /// rest there is no layer at all.
    #[test]
    fn a_press_dims_the_whole_button() {
        let theme = Theme::light();
        let rest = Runtime::default();
        let ui = lay(blank().filled().on_press(Msg::Go), &theme, &rest);
        assert!(!has_layer(&ui));
        let id = ui
            .hit(Point::new(200.0, 150.0))
            .expect("the button is pressable");
        assert_eq!(ui.msg_for(id), Some(Msg::Go));

        let mut held = Runtime::default();
        held.input.pressed = Some(id);
        held.input.hovered = Some(id);
        let ui = lay(blank().filled().on_press(Msg::Go), &theme, &held);
        let opacity = ui
            .scene()
            .all()
            .into_iter()
            .find_map(|p| match p {
                Primitive::Layer {
                    opacity,
                    primitives,
                    ..
                } if !primitives.is_empty() => Some(*opacity),
                _ => None,
            })
            .expect("a held button is one faded layer");
        assert!((opacity - CUPERTINO_PRESSED_OPACITY).abs() < 1e-5);
        // Told not to dim, it does not.
        let still = blank().filled().pressed_opacity(1.0).on_press(Msg::Go);
        assert!(!has_layer(&lay(still, &theme, &held)));
    }

    /// **The fade eases**: nothing at rest, all of it held, most of it early.
    #[test]
    fn the_fade_eases() {
        assert_eq!(pressed_fade(0.4, 0.0), 1.0);
        assert!((pressed_fade(0.4, 1.0) - 0.4).abs() < 1e-6);
        let half = pressed_fade(0.4, 0.5);
        assert!(half < 0.7, "past half the dimming at half the time: {half}");
    }

    /// **Disabled, it is inert and says so**: no message, no focus, no dimming, the disabled
    /// fill, and a node announced as unavailable.
    #[test]
    fn a_disabled_button_is_inert() {
        let theme = Theme::light();
        let b = CupertinoButton::<Msg>::new("Send").filled();
        assert_eq!(Widget::<Msg>::on_click(&b), None);
        assert!(!Widget::<Msg>::focusable(&b));
        assert_eq!(Widget::<Msg>::pressed_opacity(&b), None);
        let s = Widget::<Msg>::semantics(&b).unwrap();
        assert!(s.disabled && !s.clickable);
        assert_eq!(s.label.as_deref(), Some("Send"));
        let (_, fill) = rects(&lay(blank().filled(), &theme, &Runtime::default()))[0];
        assert_eq!(
            fill,
            CupertinoColors::QUATERNARY_SYSTEM_FILL.resolve(&theme)
        );
        let live = CupertinoButton::<Msg>::new("Send").on_press(Msg::Go);
        let s = Widget::<Msg>::semantics(&live).unwrap();
        assert!(s.clickable && !s.disabled);
        assert_eq!(s.role, frus_core::Role::Button);
    }

    /// **The focus ring is Apple's**: the colour at 80 %, lightness 0.69, saturation 0.835.
    #[test]
    fn the_focus_ring_is_apple_s() {
        let ring = focus_ring_color(Color::rgb(0.0, 0.478, 1.0));
        assert!((ring.a - 0.8).abs() < 1e-6);
        let max = ring.r.max(ring.g).max(ring.b);
        let min = ring.r.min(ring.g).min(ring.b);
        assert!(((max + min) / 2.0 - 0.69).abs() < 1e-3);
        assert!(ring.b > ring.r, "still blue");
    }
}
