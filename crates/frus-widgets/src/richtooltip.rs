// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`RichTooltip`]: a tooltip with a title, a few lines and actions, shown while hovered
//! or kept open until dismissed (milestone 786).

use frus_core::{BorderRadius, Color, Insets, Rect, Scene, TextStyle};
use frus_layout::{Align, Dimension, FlexDirection, Style};

use crate::button::{Button, Variant};
use crate::interaction::Status;
use crate::lazy::Lazy;
use crate::portal::Placement;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{Flex, Text};

/// The widest a rich tooltip gets before its lines wrap.
pub const RICH_TOOLTIP_MAX_WIDTH: f32 = 320.0;
/// A rich tooltip's corner.
pub const RICH_TOOLTIP_RADIUS: f32 = 12.0;
/// How far off the page a rich tooltip sits.
pub const RICH_TOOLTIP_ELEVATION: f32 = 3.0;

/// Defaults for [`RichTooltip`]. Each field unset is the scheme's and the type scale's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RichTooltipTheme {
    /// The surface. Unset, `surface_container`.
    pub background: Option<Color>,
    /// The title's type. Unset, `title_small`.
    pub title_style: Option<TextStyle>,
    /// The lines' type. Unset, `body_medium`.
    pub text_style: Option<TextStyle>,
    /// The title's and the lines' colour. Unset, `on_surface_variant`.
    pub foreground: Option<Color>,
    /// The actions' colour. Unset, `primary`.
    pub action_color: Option<Color>,
    /// The corner. Unset, 12.
    pub radius: Option<f32>,
    /// The height off the page. Unset, three.
    pub elevation: Option<f32>,
    /// The widest it gets. Unset, 320.
    pub max_width: Option<f32>,
}

struct Parts<Msg> {
    child: Option<Box<dyn Widget<Msg>>>,
    title: Option<String>,
    text: String,
    actions: Vec<(String, Msg)>,
    persistent: Option<(bool, Msg)>,
    background: Option<Color>,
    max_width: Option<f32>,
}

/// **A tooltip that explains**: a title, a few lines, and up to two actions — "Learn
/// more", "Got it".
///
/// ```
/// # use frus_widgets::{IconButton, Icons, RichTooltip};
/// # #[derive(Clone)] enum Msg { ToggleTip, LearnMore }
/// let open = false;
/// // Shown while the pointer rests on the button:
/// RichTooltip::<Msg>::new(IconButton::new(Icons::INFO))
///     .title("Grant value")
///     .text("The amount awarded, before fees.");
/// // Kept open until dismissed, with an action:
/// RichTooltip::<Msg>::new(IconButton::new(Icons::INFO).on_press(Msg::ToggleTip))
///     .title("Grant value")
///     .text("The amount awarded, before fees.")
///     .action("Learn more", Msg::LearnMore)
///     .persistent(open, Msg::ToggleTip);
/// ```
///
/// It sits where the plain [`Tooltip`](crate::Tooltip) does — above its anchor, below it
/// when there is no room, held inside the window — on a `surface_container` card with a
/// 12 corner, three off the page, the title in `title_small` and the lines in
/// `body_medium`, the actions as text buttons in `primary`.
///
/// **Hover-triggered** by default: it shows while the pointer rests on the anchor, which
/// suits a tooltip with nothing to press. A tooltip with actions should be
/// [`persistent`](Self::persistent): shown while `open` says so, closed by a press outside
/// it, Escape or the system's Back, all of which send `on_dismiss`. A reader hears the
/// title and the lines with the anchor, as with the plain tooltip.
pub struct RichTooltip<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> RichTooltip<Msg> {
    /// A tooltip describing `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    child: Some(Box::new(child)),
                    title: None,
                    text: String::new(),
                    actions: Vec::new(),
                    persistent: None,
                    background: None,
                    max_width: None,
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// The title, over the lines.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.p().title = Some(title.into());
        self
    }

    /// What it says.
    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.p().text = text.into();
        self
    }

    /// An action along the bottom: its words and what it sends.
    #[must_use]
    pub fn action(mut self, label: impl Into<String>, message: impl Into<Msg>) -> Self {
        self.p().actions.push((label.into(), message.into()));
        self
    }

    /// Kept open while `open` says so, rather than while hovered; `on_dismiss` is what a
    /// press outside it, Escape and the system's Back send.
    #[must_use]
    pub fn persistent(mut self, open: bool, on_dismiss: impl Into<Msg>) -> Self {
        self.p().persistent = Some((open, on_dismiss.into()));
        self
    }

    /// The surface, over the theme's and `surface_container`.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.p().background = Some(color);
        self
    }

    /// The widest it gets before its lines wrap.
    #[must_use]
    pub fn max_width(mut self, width: f32) -> Self {
        self.p().max_width = Some(width);
        self
    }
}

crate::composite::composite_widget!(RichTooltip);

/// The anchor in the flow, and the card floating at it.
struct Host<Msg> {
    /// `[anchor]` or `[anchor, card]`.
    children: Vec<Box<dyn Widget<Msg>>>,
    pinned: bool,
    on_dismiss: Option<Msg>,
    said: Option<String>,
}

impl<Msg: Clone> Widget<Msg> for Host<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    /// It is whatever its anchor is, with something floating over it — as the plain
    /// tooltip is.
    fn style_themed(&self, theme: &Theme) -> Style {
        match self.children.first() {
            Some(child) => Style {
                padding: Insets::ZERO,
                ..child.style_themed(theme)
            },
            None => Style::default(),
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.children
            .get(1)
            .map(|card| (card.as_ref(), Placement::Tooltip))
    }

    fn overlay_pinned(&self) -> bool {
        self.pinned
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.on_dismiss.clone()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        self.said
            .clone()
            .map(|said| frus_core::SemanticsProperties::new(frus_core::Role::None).label(said))
    }
}

fn build<Msg: Clone + 'static>(mut p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.rich_tooltip;
        let ink = t.foreground.unwrap_or(theme.scheme.on_surface_variant);
        let max_width = p
            .max_width
            .or(t.max_width)
            .unwrap_or(RICH_TOOLTIP_MAX_WIDTH);
        let radius = t.radius.unwrap_or(RICH_TOOLTIP_RADIUS);
        let has_actions = !p.actions.is_empty();
        let title_style = t.title_style.unwrap_or(theme.text.title_small);
        let text_style = t.text_style.unwrap_or(theme.text.body_medium);
        // As wide as its longest line wants, up to the most it may be: past that, the
        // lines wrap. A width has to be said, since the overlay offers unbounded room.
        let natural = [
            p.title
                .as_deref()
                .map(|t| frus_text::measure_resolved(t, &title_style.resolved()).width),
            Some(frus_text::measure_resolved(&p.text, &text_style.resolved()).width),
            Some(if has_actions { 200.0 } else { 0.0 }),
        ]
        .into_iter()
        .flatten()
        .fold(0.0_f32, f32::max);
        let width = (natural + 32.0).ceil().min(max_width);
        let mut card = crate::composite::Surface::new(
            Style {
                flex_direction: FlexDirection::Column,
                width: Dimension::Length(width),
                padding: Insets::new(12.0, 16.0, if has_actions { 8.0 } else { 12.0 }, 16.0),
                gap: 4.0,
                align: Align::Start,
                ..Default::default()
            },
            p.background
                .or(t.background)
                .unwrap_or(theme.scheme.surface_container),
        )
        .radius(BorderRadius::uniform(radius))
        .elevation(
            t.elevation.unwrap_or(RICH_TOOLTIP_ELEVATION),
            theme.scheme.shadow.with_alpha(0.3),
        );
        if let Some(title) = &p.title {
            card = card.child(Text::styled(title.clone(), title_style).color(ink));
        }
        if !p.text.is_empty() {
            card = card.child(
                Text::styled(
                    p.text.clone(),
                    t.text_style.unwrap_or(theme.text.body_medium),
                )
                .color(ink),
            );
        }
        if has_actions {
            let mut row = Flex::row().gap(8.0).padding_each(4.0, 0.0, 0.0, 0.0);
            for (label, message) in p.actions.drain(..) {
                row = row.child(
                    Button::new(label)
                        .variant(Variant::Text)
                        .label_color(t.action_color.unwrap_or(theme.scheme.primary))
                        .padding(8.0)
                        .on_press(message),
                );
            }
            card = card.child(row);
        }
        let anchor = p
            .child
            .take()
            .unwrap_or_else(|| Box::new(crate::SizedBox::empty()));
        let (shows, pinned, on_dismiss) = match p.persistent.take() {
            Some((open, dismiss)) => (open, open, open.then_some(dismiss)),
            None => (true, false, None),
        };
        let said = [p.title.clone(), Some(p.text.clone())]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let mut children = vec![anchor];
        if shows {
            children.push(Box::new(card) as Box<dyn Widget<Msg>>);
        }
        Box::new(Host {
            children,
            pinned,
            on_dismiss,
            said: (!said.is_empty()).then_some(said),
        }) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Toggle,
        Learn,
    }

    fn anchor() -> crate::Container<Msg> {
        crate::Container::new().width(40.0).height(40.0)
    }

    fn placed(tip: RichTooltip<Msg>) -> crate::Ui<Msg> {
        let page = crate::Container::new()
            .padding_each(300.0, 0.0, 0.0, 100.0)
            .child(tip);
        build_ui(
            &page,
            Size::new(600.0, 500.0),
            &Runtime::default(),
            &Theme::default(),
        )
    }

    /// **A persistent tooltip shows while it is told to**, above its anchor, with its
    /// actions pressable, and a press outside it, Escape or Back send the dismissal.
    #[test]
    fn a_persistent_tooltip_shows_while_open_and_back_closes_it() {
        let open = RichTooltip::new(anchor())
            .title("Grant value")
            .text("The amount awarded.")
            .action("Learn more", Msg::Learn)
            .persistent(true, Msg::Toggle);
        let ui = placed(open);
        let learn = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some("Learn more"))
            .map(|(_, r, _)| *r)
            .expect("the action shows without a hover");
        assert!(
            learn.y + learn.height <= 300.0,
            "above the anchor: {learn:?}"
        );
        let at = frus_core::Point::new(learn.x + learn.width * 0.5, learn.y + learn.height * 0.5);
        assert_eq!(ui.hit(at).and_then(|id| ui.msg_for(id)), Some(Msg::Learn));
        assert_eq!(ui.top_dismiss(), Some(Msg::Toggle));
        let anchor_said = ui
            .semantics()
            .iter()
            .any(|(_, _, s)| s.label.as_deref() == Some("Grant value\nThe amount awarded."));
        assert!(anchor_said, "the anchor says what the tooltip says");

        let shut = RichTooltip::new(anchor())
            .title("Grant value")
            .action("Learn more", Msg::Learn)
            .persistent(false, Msg::Toggle);
        let ui = placed(shut);
        assert_eq!(ui.top_dismiss(), None);
        assert!(!ui
            .semantics()
            .iter()
            .any(|(_, _, s)| s.label.as_deref() == Some("Learn more")));
    }

    /// **A hover tooltip shows nothing until hovered.**
    #[test]
    fn a_hover_tooltip_waits_for_the_pointer() {
        let tip = RichTooltip::new(anchor())
            .title("Grant value")
            .text("The amount awarded.");
        let ui = build_ui(
            &tip,
            Size::new(600.0, 500.0),
            &Runtime::default(),
            &Theme::default(),
        );
        assert!(!ui
            .semantics()
            .iter()
            .any(|(_, _, s)| s.label.as_deref() == Some("Grant value")));
        assert_eq!(ui.top_dismiss(), None);
    }
}
