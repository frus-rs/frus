// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`FabMenu`]: a floating action button that opens a list of labelled actions over a
//! scrim (milestone 786).

use frus_core::{BorderRadius, Color, Rect, Scene, ShapeBorder, Size, TextStyle};
use frus_layout::{Align, FlexDirection, Style};

use crate::button::{Button, ButtonStyle, Variant};
use crate::fab::FloatingActionButton;
use crate::icons::{IconData, Icons};
use crate::interaction::Status;
use crate::lazy::Lazy;
use crate::portal::Placement;
use crate::semantics::Semantics;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::widgetstate::WidgetStateProperty;
use crate::Icon;

/// The height of one of the menu's actions.
pub const FAB_MENU_ITEM_HEIGHT: f32 = 56.0;
/// The gap between two actions.
pub const FAB_MENU_ITEM_GAP: f32 = 4.0;
/// The gap between the last action and the close button.
pub const FAB_MENU_CLOSE_GAP: f32 = 8.0;
/// The side of the round close button the open menu shows where the button was.
pub const FAB_MENU_CLOSE_SIZE: f32 = 56.0;

/// Which of the scheme's colour families a [`FabMenu`] is drawn in.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FabMenuColor {
    /// `primary` for the close button, `primary_container` for the actions.
    #[default]
    Primary,
    /// The `secondary` family.
    Secondary,
    /// The `tertiary` family.
    Tertiary,
}

impl FabMenuColor {
    /// `(close background, close content, action background, action content)`.
    fn roles(self, theme: &Theme) -> (Color, Color, Color, Color) {
        let s = &theme.scheme;
        match self {
            FabMenuColor::Primary => (
                s.primary,
                s.on_primary,
                s.primary_container,
                s.on_primary_container,
            ),
            FabMenuColor::Secondary => (
                s.secondary,
                s.on_secondary,
                s.secondary_container,
                s.on_secondary_container,
            ),
            FabMenuColor::Tertiary => (
                s.tertiary,
                s.on_tertiary,
                s.tertiary_container,
                s.on_tertiary_container,
            ),
        }
    }
}

/// Defaults for [`FabMenu`]. Each field unset is the colour family's and the size's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FabMenuTheme {
    /// The colour family, unless the menu names one.
    pub color: Option<FabMenuColor>,
    /// The actions' surface.
    pub item_background: Option<Color>,
    /// The actions' words and glyphs.
    pub item_foreground: Option<Color>,
    /// The close button's surface.
    pub close_background: Option<Color>,
    /// The close button's glyph.
    pub close_foreground: Option<Color>,
    /// The actions' type. Unset, `title_medium`.
    pub label_style: Option<TextStyle>,
    /// The gap between two actions.
    pub item_gap: Option<f32>,
    /// The scrim behind the open menu, alpha included. Unset, `scrim` at 32 %.
    pub scrim: Option<Color>,
}

struct Item<Msg> {
    icon: IconData,
    label: String,
    on_press: Msg,
}

struct Parts<Msg> {
    open: bool,
    on_toggle: Msg,
    icon: IconData,
    label: Option<String>,
    items: Vec<Item<Msg>>,
    color: Option<FabMenuColor>,
    item_background: Option<Color>,
    item_foreground: Option<Color>,
    close_background: Option<Color>,
    close_foreground: Option<Color>,
    label_style: Option<TextStyle>,
    item_gap: Option<f32>,
    scrim: Option<Color>,
}

/// **A floating action button that opens a short list of labelled actions** — the
/// related things a screen's main action leads to.
///
/// ```
/// # use frus_widgets::{FabMenu, Icons};
/// # #[derive(Clone)] enum Msg { ToggleMenu, Compose, Meet }
/// let open = false;
/// FabMenu::<Msg>::new(open, Msg::ToggleMenu)
///     .icon(Icons::ADD)
///     .item(Icons::EDIT, "Compose", Msg::Compose)
///     .item(Icons::VIDEOCAM, "New meeting", Msg::Meet);
/// ```
///
/// Closed, it is a [`FloatingActionButton`]; put it where that would go — a scaffold's
/// floating action button slot. Open, the actions stack upwards from where the button
/// was, aligned to its end edge, each a pill 56 high in the container colours, with a
/// round close button in the strong colour standing exactly over the button it replaced,
/// and a scrim over the rest of the window.
///
/// It is controlled: `open` says whether the list shows, and `on_toggle` is sent by the
/// button, by the close button, by a press on the scrim, by Escape and by the system's
/// Back. An action sends its own message, which should also close the menu. While open
/// the keyboard is held inside the menu. A reader hears the button as collapsed and the
/// close button as expanded.
pub struct FabMenu<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> FabMenu<Msg> {
    /// A menu, open or not, whose button and close button send `on_toggle`.
    pub fn new(open: bool, on_toggle: impl Into<Msg>) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    open,
                    on_toggle: on_toggle.into(),
                    icon: Icons::ADD,
                    label: None,
                    items: Vec::new(),
                    color: None,
                    item_background: None,
                    item_foreground: None,
                    close_background: None,
                    close_foreground: None,
                    label_style: None,
                    item_gap: None,
                    scrim: None,
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// The closed button's glyph. A plus by default.
    #[must_use]
    pub fn icon(mut self, icon: IconData) -> Self {
        self.p().icon = icon;
        self
    }

    /// What a reader hears on the closed button. Unset, "More options".
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.p().label = Some(label.into());
        self
    }

    /// An action: a glyph, its words, and what it sends.
    #[must_use]
    pub fn item(
        mut self,
        icon: IconData,
        label: impl Into<String>,
        on_press: impl Into<Msg>,
    ) -> Self {
        self.p().items.push(Item {
            icon,
            label: label.into(),
            on_press: on_press.into(),
        });
        self
    }

    /// The colour family. [`FabMenuColor::Primary`] by default.
    #[must_use]
    pub fn color(mut self, color: FabMenuColor) -> Self {
        self.p().color = Some(color);
        self
    }

    /// The actions' surface and content, over the colour family's.
    #[must_use]
    pub fn item_colors(mut self, background: Color, foreground: Color) -> Self {
        let p = self.p();
        p.item_background = Some(background);
        p.item_foreground = Some(foreground);
        self
    }

    /// The close button's surface and glyph, over the colour family's.
    #[must_use]
    pub fn close_colors(mut self, background: Color, foreground: Color) -> Self {
        let p = self.p();
        p.close_background = Some(background);
        p.close_foreground = Some(foreground);
        self
    }

    /// The actions' type. `title_medium` by default.
    #[must_use]
    pub fn label_style(mut self, style: TextStyle) -> Self {
        self.p().label_style = Some(style);
        self
    }

    /// The gap between two actions. Four by default.
    #[must_use]
    pub fn item_gap(mut self, gap: f32) -> Self {
        self.p().item_gap = Some(gap);
        self
    }

    /// The scrim behind the open menu, alpha included; [`Color::TRANSPARENT`] for none.
    #[must_use]
    pub fn scrim(mut self, color: Color) -> Self {
        self.p().scrim = Some(color);
        self
    }
}

crate::composite::composite_widget!(FabMenu);

/// The button in the flow, and the open menu floating over it.
struct Host<Msg> {
    /// `[button]` or `[button, menu]`.
    children: Vec<Box<dyn Widget<Msg>>>,
    on_toggle: Option<Msg>,
    scrim: Option<Color>,
}

impl<Msg: Clone> Widget<Msg> for Host<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            ..Default::default()
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
            .map(|menu| (menu.as_ref(), Placement::Covering))
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.on_toggle.clone()
    }

    fn overlay_scrim(&self, _theme: &Theme) -> Option<Color> {
        self.scrim
    }
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.fab_menu;
        let family = p.color.or(t.color).unwrap_or_default();
        let (close_bg, close_fg, item_bg, item_fg) = family.roles(theme);
        let close_bg = p
            .close_background
            .or(t.close_background)
            .unwrap_or(close_bg);
        let close_fg = p
            .close_foreground
            .or(t.close_foreground)
            .unwrap_or(close_fg);
        let item_bg = p.item_background.or(t.item_background).unwrap_or(item_bg);
        let item_fg = p.item_foreground.or(t.item_foreground).unwrap_or(item_fg);
        let label_style = p
            .label_style
            .or(t.label_style)
            .unwrap_or(theme.text.title_medium);
        let scrim = p
            .scrim
            .or(t.scrim)
            .unwrap_or_else(|| theme.scheme.scrim.with_alpha(0.32));
        let name = p
            .label
            .clone()
            .unwrap_or_else(|| crate::localizations::of().more_options_label().to_string());

        let fab = Semantics::new(
            frus_core::SemanticsProperties::new(frus_core::Role::Button)
                .label(name)
                .expanded(p.open),
            FloatingActionButton::new(p.icon).on_press(p.on_toggle.clone()),
        )
        .merging(true);
        let mut children: Vec<Box<dyn Widget<Msg>>> = vec![Box::new(fab)];

        if p.open {
            let gap = p.item_gap.or(t.item_gap).unwrap_or(FAB_MENU_ITEM_GAP);
            let mut column = crate::Flex::column().gap(gap).align(Align::End);
            for item in p.items {
                let style = ButtonStyle {
                    shape: Some(WidgetStateProperty::all(ShapeBorder::rounded(
                        BorderRadius::uniform(FAB_MENU_ITEM_HEIGHT * 0.5),
                    ))),
                    background_color: Some(WidgetStateProperty::all(item_bg)),
                    foreground_color: Some(WidgetStateProperty::all(item_fg)),
                    icon_color: Some(WidgetStateProperty::all(item_fg)),
                    icon_size: Some(WidgetStateProperty::all(24.0)),
                    text_style: Some(WidgetStateProperty::all(label_style)),
                    padding: Some(WidgetStateProperty::all(frus_core::Insets::new(
                        0.0, 24.0, 0.0, 24.0,
                    ))),
                    ..ButtonStyle::default()
                };
                column = column.child(
                    Button::new(item.label)
                        .icon(Icon::new(item.icon))
                        .variant(Variant::Tonal)
                        .style(style)
                        .height(FAB_MENU_ITEM_HEIGHT)
                        .elevation(0.0)
                        .on_press(item.on_press),
                );
            }
            let close_style = ButtonStyle {
                shape: Some(WidgetStateProperty::all(ShapeBorder::rounded(
                    BorderRadius::uniform(FAB_MENU_CLOSE_SIZE * 0.5),
                ))),
                background_color: Some(WidgetStateProperty::all(close_bg)),
                foreground_color: Some(WidgetStateProperty::all(close_fg)),
                icon_color: Some(WidgetStateProperty::all(close_fg)),
                icon_size: Some(WidgetStateProperty::all(20.0)),
                padding: Some(WidgetStateProperty::all(frus_core::Insets::ZERO)),
                fixed_size: Some(WidgetStateProperty::all(Size::new(
                    FAB_MENU_CLOSE_SIZE,
                    FAB_MENU_CLOSE_SIZE,
                ))),
                minimum_size: Some(WidgetStateProperty::all(Size::new(
                    FAB_MENU_CLOSE_SIZE,
                    FAB_MENU_CLOSE_SIZE,
                ))),
                ..ButtonStyle::default()
            };
            let close = Semantics::new(
                frus_core::SemanticsProperties::new(frus_core::Role::Button)
                    .label(crate::localizations::of().close_button_label())
                    .expanded(true),
                Button::with_child(Icon::new(Icons::CLOSE))
                    .variant(Variant::Filled)
                    .style(close_style)
                    .height(FAB_MENU_CLOSE_SIZE)
                    .elevation(0.0)
                    .on_press(p.on_toggle.clone()),
            )
            .merging(true);
            // The close button stands where the button stood: the menu's bottom edge is
            // the button's, and the gap above it is the close gap rather than an item's.
            let panel = crate::composite::Surface::new(
                Style {
                    flex_direction: FlexDirection::Column,
                    align: Align::End,
                    gap: FAB_MENU_CLOSE_GAP,
                    ..Default::default()
                },
                Color::TRANSPARENT,
            )
            .child(column)
            .child(close);
            children.push(Box::new(panel));
        }
        Box::new(Host {
            children,
            on_toggle: p.open.then(|| p.on_toggle.clone()),
            scrim: Some(scrim),
        }) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Toggle,
        Compose,
        Meet,
    }

    fn menu(open: bool) -> crate::Stack<Msg> {
        crate::Stack::new().layer(
            crate::Positioned::new(
                FabMenu::new(open, Msg::Toggle)
                    .item(Icons::EDIT, "Compose", Msg::Compose)
                    .item(Icons::VIDEOCAM, "New meeting", Msg::Meet),
            )
            .right(16.0)
            .bottom(16.0),
        )
    }

    fn click(ui: &crate::Ui<Msg>, at: frus_core::Point) -> Option<Msg> {
        ui.hit(at).and_then(|id| ui.msg_for(id))
    }

    fn frame(open: bool) -> crate::Ui<Msg> {
        build_ui(
            &menu(open),
            frus_core::Size::new(400.0, 600.0),
            &Runtime::default(),
            &Theme::default(),
        )
    }

    fn labelled<'a>(
        ui: &'a crate::Ui<Msg>,
        label: &str,
    ) -> Option<&'a (crate::WidgetId, Rect, frus_core::SemanticsProperties)> {
        ui.semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some(label))
    }

    /// **Closed, it is a button that says it is collapsed**; there is no menu, nothing
    /// for Back to close, and a press on the button asks for the menu.
    #[test]
    fn closed_it_is_a_collapsed_button() {
        let ui = frame(false);
        let (_, rect, said) = labelled(&ui, "More options").expect("the button");
        assert_eq!(said.expanded, Some(false));
        assert_eq!(said.role, frus_core::Role::Button);
        assert!(
            labelled(&ui, "Compose").is_none(),
            "no actions while closed"
        );
        assert_eq!(ui.top_dismiss(), None, "nothing for Back to close");
        let center = frus_core::Point::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        assert_eq!(click(&ui, center), Some(Msg::Toggle));
    }

    /// **Open, the actions stand above the button and the close button stands on it.**
    /// Back, Escape and a press on the scrim all close it; each action sends its own.
    #[test]
    fn open_the_actions_stack_upwards_and_back_closes() {
        let closed = frame(false);
        let (_, button, _) = *labelled(&closed, "More options").unwrap();
        let ui = frame(true);
        let (_, close, said) = labelled(&ui, "Close").expect("a close button");
        assert_eq!(said.expanded, Some(true));
        assert_eq!(
            (close.x + close.width, close.y + close.height),
            (button.x + button.width, button.y + button.height),
            "the close button's bottom-end corner is the button's"
        );
        let (_, compose, _) = *labelled(&ui, "Compose").expect("an action");
        let (_, meet, _) = *labelled(&ui, "New meeting").expect("an action");
        assert!(
            compose.y < meet.y && meet.y < close.y,
            "stacked upwards in order"
        );
        assert!(
            (compose.x + compose.width - (close.x + close.width)).abs() < 0.5,
            "aligned to the end edge"
        );
        assert_eq!(compose.height, FAB_MENU_ITEM_HEIGHT);
        assert_eq!(
            ui.top_dismiss(),
            Some(Msg::Toggle),
            "Back and Escape close it"
        );
        let at = |r: Rect| frus_core::Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5);
        assert_eq!(click(&ui, at(compose)), Some(Msg::Compose));
        assert_eq!(click(&ui, at(*close)), Some(Msg::Toggle));
        assert_eq!(
            click(&ui, frus_core::Point::new(20.0, 20.0)),
            Some(Msg::Toggle),
            "a press on the scrim closes it"
        );
    }
}
