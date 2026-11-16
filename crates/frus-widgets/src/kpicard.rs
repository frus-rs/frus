// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`KpiCard`]: the figure of a dashboard — a label, a value, how it moved, and optionally
//! its trend — on a card (milestone 811).
//!
//! Every part has a default and every part can be replaced: the label, the value (text, a
//! [`RollingDigits`] that rolls when the figure changes, or any widget), the change (an
//! arrow and a figure coloured by the theme's status colours), a leading icon, a trailing
//! slot, and a footer — a [`Sparkline`] when given values. A card reads as one element to a
//! screen reader: its label, its value and its change, in that order.

use std::cell::{OnceCell, RefCell};

use frus_core::{Color, Rect, Scene, TextStyle};
use frus_layout::{Align, Style};

use crate::flex::Flex;
use crate::icons::{IconData, Icons};
use crate::interaction::Status;
use crate::rollingdigits::RollingDigits;
use crate::sparkline::Sparkline;
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::Widget;

/// Which way a figure moved, and so which colour its change wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KpiTrend {
    /// It went up.
    Up,
    /// It went down.
    Down,
    /// It held.
    Neutral,
}

impl KpiTrend {
    /// The trend of a signed change: up above zero, down below, neutral at zero.
    pub fn of(change: f64) -> Self {
        if change > 0.0 {
            KpiTrend::Up
        } else if change < 0.0 {
            KpiTrend::Down
        } else {
            KpiTrend::Neutral
        }
    }
}

/// Theme defaults for [`KpiCard`] (`theme.widgets.kpi_card`); each `None` falls through to
/// the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KpiCardTheme {
    /// The card's surface. Unset, `surface_container_low`.
    pub background: Option<Color>,
    /// The card's outline. Unset, `outline_variant`.
    pub border_color: Option<Color>,
    /// The card's corner radius. Unset, 12.
    pub radius: Option<f32>,
    /// The space inside the card. Unset, 16.
    pub padding: Option<f32>,
    /// The label's style. Unset, `title_small`.
    pub label_style: Option<TextStyle>,
    /// The label's colour. Unset, `on_surface_variant`.
    pub label_color: Option<Color>,
    /// The value's style. Unset, `headline_medium`.
    pub value_style: Option<TextStyle>,
    /// The change's style. Unset, `label_large`.
    pub delta_style: Option<TextStyle>,
    /// The colour of a change for the better. Unset, `success`.
    pub positive_color: Option<Color>,
    /// The colour of a change for the worse. Unset, `error`.
    pub negative_color: Option<Color>,
    /// The colour of no change. Unset, `on_surface_variant`.
    pub neutral_color: Option<Color>,
}

/// What the card shows as its value.
enum Value<Msg> {
    None,
    Text(String),
    Digits(Box<RollingDigits<Msg>>),
    Widget(Box<dyn Widget<Msg>>),
}

/// Everything a card was told, taken once when it is built.
struct Parts<Msg> {
    label: String,
    value: Value<Msg>,
    delta: Option<(String, KpiTrend)>,
    lower_is_better: bool,
    icon: Option<IconData>,
    label_widget: Option<Box<dyn Widget<Msg>>>,
    delta_widget: Option<Box<dyn Widget<Msg>>>,
    trailing: Option<Box<dyn Widget<Msg>>>,
    footer: Option<Box<dyn Widget<Msg>>>,
    sparkline: Option<Vec<f32>>,
    on_tap: Option<Msg>,
    background: Option<Color>,
}

/// A **key figure on a card**: a label, a value, its change and, if wanted, its trend.
///
/// ```
/// use frus_widgets::{KpiCard, KpiTrend, RollingDigits};
///
/// // Text value, change given as a percentage: "+12.5 %", in the theme's success colour.
/// let _revenue: KpiCard = KpiCard::new("Revenue").value("$48,210").change(12.5);
///
/// // A figure that rolls when it changes, with its trend under it.
/// let _visitors: KpiCard = KpiCard::new("Visitors")
///     .digits(RollingDigits::new(12_480.0))
///     .delta("-320 today", KpiTrend::Down)
///     .sparkline([9.0, 12.0, 11.0, 14.0, 13.0, 12.4]);
///
/// // Lower is better: a fall is good news, and wears the success colour.
/// let _errors: KpiCard = KpiCard::new("Error rate").value("0.4 %").change(-0.2).lower_is_better(true);
/// ```
///
/// Every slot can be replaced ([`KpiCard::label_widget`], [`KpiCard::value_widget`],
/// [`KpiCard::delta_widget`], [`KpiCard::trailing`], [`KpiCard::footer`]). **Themed** through
/// [`KpiCardTheme`] (`theme.widgets.kpi_card`).
pub struct KpiCard<Msg = crate::callback::Callback> {
    parts: RefCell<Option<Parts<Msg>>>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> KpiCard<Msg> {
    /// A card labelled `label`, with no value yet.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            parts: RefCell::new(Some(Parts {
                label: label.into(),
                value: Value::None,
                delta: None,
                lower_is_better: false,
                icon: None,
                label_widget: None,
                delta_widget: None,
                trailing: None,
                footer: None,
                sparkline: None,
                on_tap: None,
                background: None,
            })),
            built: OnceCell::new(),
        }
    }

    fn parts(&mut self) -> &mut Parts<Msg> {
        self.parts
            .get_mut()
            .as_mut()
            .expect("a card is configured before it is built")
    }

    /// The value, as text.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.parts().value = Value::Text(value.into());
        self
    }

    /// The value as a number that **rolls** to each new figure, with the card's value style
    /// unless it has one.
    pub fn number(self, value: f64) -> Self {
        self.digits(RollingDigits::new(value))
    }

    /// The value as a [`RollingDigits`] set up as wanted: decimals, a prefix, a unit.
    pub fn digits(mut self, digits: RollingDigits<Msg>) -> Self {
        self.parts().value = Value::Digits(Box::new(digits));
        self
    }

    /// The value as any widget.
    pub fn value_widget(mut self, value: impl Widget<Msg> + 'static) -> Self {
        self.parts().value = Value::Widget(Box::new(value));
        self
    }

    /// The change, written out, and which way it went.
    pub fn delta(mut self, text: impl Into<String>, trend: KpiTrend) -> Self {
        self.parts().delta = Some((text.into(), trend));
        self
    }

    /// The change as a percentage, written with its sign and one decimal (`+12.5 %`), its
    /// trend from the sign.
    pub fn change(self, percent: f64) -> Self {
        let sign = if percent > 0.0 { "+" } else { "" };
        self.delta(format!("{sign}{percent:.1} %"), KpiTrend::of(percent))
    }

    /// Whether a fall is the good news (an error rate, a cost): the colours swap.
    pub fn lower_is_better(mut self, lower: bool) -> Self {
        self.parts().lower_is_better = lower;
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: IconData) -> Self {
        self.parts().icon = Some(icon);
        self
    }

    /// The label as any widget, in place of the text.
    pub fn label_widget(mut self, label: impl Widget<Msg> + 'static) -> Self {
        self.parts().label_widget = Some(Box::new(label));
        self
    }

    /// The change as any widget, in place of the arrow and the figure.
    pub fn delta_widget(mut self, delta: impl Widget<Msg> + 'static) -> Self {
        self.parts().delta_widget = Some(Box::new(delta));
        self
    }

    /// Something at the end of the label's line: a menu, a period picker, a badge.
    pub fn trailing(mut self, trailing: impl Widget<Msg> + 'static) -> Self {
        self.parts().trailing = Some(Box::new(trailing));
        self
    }

    /// Something under the value, over the whole width. Takes the place of a sparkline.
    pub fn footer(mut self, footer: impl Widget<Msg> + 'static) -> Self {
        self.parts().footer = Some(Box::new(footer));
        self
    }

    /// A [`Sparkline`] of `values` under the value, in the change's colour.
    pub fn sparkline(mut self, values: impl IntoIterator<Item = f32>) -> Self {
        self.parts().sparkline = Some(values.into_iter().collect());
        self
    }

    /// Makes the card a button sending `message`.
    pub fn on_tap(mut self, message: impl Into<Msg>) -> Self {
        self.parts().on_tap = Some(message.into());
        self
    }

    /// The card's surface, over the theme's.
    pub fn background(mut self, color: Color) -> Self {
        self.parts().background = Some(color);
        self
    }

    /// The card, built under `theme`.
    fn build(&self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let Some(parts) = self.parts.borrow_mut().take() else {
            return Box::new(crate::SizedBox::<Msg>::empty());
        };
        let t = theme.widgets.kpi_card;
        let s = &theme.scheme;
        let trend_color = |trend: KpiTrend| {
            let good = match trend {
                KpiTrend::Up => Some(!parts.lower_is_better),
                KpiTrend::Down => Some(parts.lower_is_better),
                KpiTrend::Neutral => None,
            };
            match good {
                Some(true) => t.positive_color.unwrap_or(s.success),
                Some(false) => t.negative_color.unwrap_or(s.error),
                None => t.neutral_color.unwrap_or(s.on_surface_variant),
            }
        };

        // The label's line: icon, label, trailing slot.
        let label_color = t.label_color.unwrap_or(s.on_surface_variant);
        let mut head = Flex::row().align(Align::Center).gap(8.0);
        if let Some(icon) = parts.icon {
            head = head.child(crate::Icon::new(icon).size(20.0).color(label_color));
        }
        let label: Box<dyn Widget<Msg>> = match parts.label_widget {
            Some(w) => w,
            None => Box::new(
                Text::styled(
                    parts.label.clone(),
                    t.label_style.unwrap_or(theme.text.title_small),
                )
                .color(label_color),
            ),
        };
        head = head.child(crate::Expanded::new(label));
        if let Some(trailing) = parts.trailing {
            head = head.child(trailing);
        }

        // The value's line: the value, then the change on its baseline.
        let value_style = t.value_style.unwrap_or(theme.text.headline_medium);
        let value: Option<Box<dyn Widget<Msg>>> = match parts.value {
            Value::None => None,
            Value::Text(v) => Some(Box::new(Text::styled(v, value_style).color(s.on_surface))),
            Value::Digits(d) => Some(Box::new((*d).style_unless_set(value_style))),
            Value::Widget(w) => Some(w),
        };
        let delta: Option<Box<dyn Widget<Msg>>> = match (parts.delta_widget, &parts.delta) {
            (Some(w), _) => Some(w),
            (None, Some((text, trend))) => {
                let color = trend_color(*trend);
                let arrow = match trend {
                    KpiTrend::Up => Icons::ARROW_UPWARD,
                    KpiTrend::Down => Icons::ARROW_DOWNWARD,
                    KpiTrend::Neutral => Icons::REMOVE,
                };
                Some(Box::new(
                    Flex::row()
                        .align(Align::Center)
                        .gap(2.0)
                        .child(crate::Icon::new(arrow).size(16.0).color(color))
                        .child(
                            Text::styled(
                                text.clone(),
                                t.delta_style.unwrap_or(theme.text.label_large),
                            )
                            .color(color),
                        ),
                ))
            }
            (None, None) => None,
        };
        let mut figures = Flex::row().align(Align::Center).gap(12.0);
        let mut has_figures = false;
        if let Some(value) = value {
            figures = figures.child(value);
            has_figures = true;
        }
        if let Some(delta) = delta {
            figures = figures.child(delta);
            has_figures = true;
        }

        let mut body = Flex::column().gap(8.0).align(Align::Stretch).child(head);
        if has_figures {
            body = body.child(figures);
        }
        let line_color = parts
            .delta
            .as_ref()
            .map(|(_, trend)| trend_color(*trend))
            .filter(|_| {
                parts
                    .delta
                    .as_ref()
                    .is_some_and(|d| d.1 != KpiTrend::Neutral)
            })
            .unwrap_or(s.primary);
        match (parts.footer, parts.sparkline) {
            (Some(footer), _) => body = body.child(footer),
            (None, Some(values)) => body = body.child(Sparkline::new(values).color(line_color)),
            (None, None) => {}
        }

        let radius = t.radius.unwrap_or(12.0);
        let card = crate::Container::new()
            .padding(t.padding.unwrap_or(16.0))
            .radius(radius)
            .color(
                parts
                    .background
                    .or(t.background)
                    .unwrap_or(s.surface_container_low),
            )
            .border(1.0, t.border_color.unwrap_or(s.outline_variant))
            .child(body);
        let card: Box<dyn Widget<Msg>> = match parts.on_tap {
            Some(message) => Box::new(
                crate::InkWell::new()
                    .radius(radius)
                    .on_click(message)
                    .child(card),
            ),
            None => Box::new(card),
        };
        Box::new(crate::Semantics::merge(BoxedCard(card)))
    }
}

/// A boxed subtree as a widget.
struct BoxedCard<Msg>(Box<dyn Widget<Msg>>);

impl<Msg: 'static> Widget<Msg> for BoxedCard<Msg> {
    fn style(&self) -> Style {
        self.0.style()
    }
    fn style_themed(&self, theme: &Theme) -> Style {
        self.0.style_themed(theme)
    }
    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        std::slice::from_ref(&self.0)
    }
    fn paint(&self, _: Rect, _: Status, _: &Theme, _: &mut Scene) {}
    fn on_click(&self) -> Option<Msg> {
        None
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for KpiCard<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn build_themed(&self, theme: &Theme) {
        let _ = self.built.get_or_init(|| vec![self.build(theme)]);
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built
            .get_or_init(|| vec![self.build(&Theme::default())])
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "KpiCard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};
    use frus_core::Primitive;

    fn scene_of(card: KpiCard<()>, theme: &Theme) -> Vec<Primitive> {
        let screen = Flex::<()>::column().align(Align::Start).child(card);
        build_ui(&screen, Size::new(360.0, 400.0), &Runtime::default(), theme)
            .scene()
            .primitives()
            .to_vec()
    }

    fn text_color(ps: &[Primitive], words: &str) -> Option<Color> {
        ps.iter().find_map(|p| match p {
            Primitive::Text { text, color, .. } if text == words => Some(*color),
            Primitive::Layer { primitives, .. } => text_color(primitives, words),
            _ => None,
        })
    }

    #[test]
    fn a_rise_wears_success_and_a_fall_wears_error() {
        let theme = Theme::light();
        let up = scene_of(KpiCard::new("Revenue").value("$10").change(12.5), &theme);
        assert_eq!(text_color(&up, "+12.5 %"), Some(theme.scheme.success));
        let down = scene_of(KpiCard::new("Revenue").value("$10").change(-3.0), &theme);
        assert_eq!(text_color(&down, "-3.0 %"), Some(theme.scheme.error));
        let flat = scene_of(KpiCard::new("Revenue").value("$10").change(0.0), &theme);
        assert_eq!(
            text_color(&flat, "0.0 %"),
            Some(theme.scheme.on_surface_variant)
        );
    }

    #[test]
    fn lower_is_better_swaps_the_colours() {
        let theme = Theme::light();
        let ps = scene_of(
            KpiCard::new("Errors")
                .value("4")
                .change(-10.0)
                .lower_is_better(true),
            &theme,
        );
        assert_eq!(text_color(&ps, "-10.0 %"), Some(theme.scheme.success));
    }

    #[test]
    fn the_theme_recolours_the_card() {
        let mut theme = Theme::light();
        let gold = Color::rgb(0.9, 0.7, 0.1);
        theme.widgets.kpi_card.positive_color = Some(gold);
        theme.widgets.kpi_card.label_color = Some(gold);
        let ps = scene_of(KpiCard::new("Sales").value("1").change(1.0), &theme);
        assert_eq!(text_color(&ps, "+1.0 %"), Some(gold));
        assert_eq!(text_color(&ps, "Sales"), Some(gold));
    }

    #[test]
    fn slots_replace_the_defaults() {
        let theme = Theme::light();
        let ps = scene_of(
            KpiCard::new("Hidden")
                .label_widget(Text::new("Custom label"))
                .value_widget(Text::new("42 things"))
                .delta_widget(Text::new("steady"))
                .trailing(Text::new("7d")),
            &theme,
        );
        for words in ["Custom label", "42 things", "steady", "7d"] {
            assert!(text_color(&ps, words).is_some(), "{words} is drawn");
        }
        assert!(
            text_color(&ps, "Hidden").is_none(),
            "the label was replaced"
        );
    }

    #[test]
    fn the_card_reads_as_one_element() {
        let theme = Theme::light();
        let screen =
            Flex::<()>::column().child(KpiCard::new("Revenue").value("$48,210").change(12.5));
        let ui = build_ui(
            &screen,
            Size::new(360.0, 400.0),
            &Runtime::default(),
            &theme,
        );
        let labels: Vec<String> = ui
            .semantics()
            .iter()
            .filter_map(|(_, _, props)| props.label.clone())
            .collect();
        let merged = labels
            .iter()
            .find(|l| l.contains("Revenue"))
            .expect("the card is read");
        assert!(
            merged.contains("$48,210") && merged.contains("+12.5 %"),
            "{labels:?}"
        );
    }

    #[test]
    fn a_sparkline_is_drawn_in_the_trend_colour() {
        let theme = Theme::light();
        let ps = scene_of(
            KpiCard::new("Visits")
                .number(1200.0)
                .change(4.0)
                .sparkline([1.0, 3.0, 2.0, 5.0]),
            &theme,
        );
        let stroke = ps.iter().find_map(|p| match p {
            Primitive::Path {
                stroke: Some(stroke),
                ..
            } => Some(stroke.color),
            _ => None,
        });
        assert_eq!(stroke, Some(theme.scheme.success));
    }
}
