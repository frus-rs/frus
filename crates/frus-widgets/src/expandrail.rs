// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`ExpandableNavigationRail`]: the expressive navigation rail that **opens into a wider
//! rail with labels**, a menu button at the top to open and close it and a floating action
//! button under that.
//!
//! Collapsed it is 96 wide: each destination an icon in a 56 by 32 pill with its label
//! under it. Expanded it is 220 wide: each destination a 56-tall pill holding the icon and
//! the label side by side, and the action button grows its label. The width animates
//! between the two.
//!
//! It is **controlled**: the application keeps which destination is selected and whether
//! the rail is open, and is told when either should change.

use std::rc::Rc;

use frus_core::{Color, Rect, Scene, TextStyle};
use frus_layout::{Align, Dimension, Style};

use crate::flex::Flex;
use crate::icons::IconData;
use crate::interaction::Status;
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::Widget;

/// The rail's width collapsed.
pub const RAIL_COLLAPSED_WIDTH: f32 = 96.0;
/// The rail's width expanded.
pub const RAIL_EXPANDED_WIDTH: f32 = 220.0;
/// How long the rail takes to open or close, in seconds.
const RAIL_DURATION: f32 = 0.35;

/// Theme defaults for [`ExpandableNavigationRail`]; each `None` falls through to the
/// default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExpandableRailTheme {
    /// The rail's surface. Unset, `surface`.
    pub background: Option<Color>,
    /// The selected destination's pill. Unset, `secondary_container`.
    pub indicator_color: Option<Color>,
    /// The selected destination's icon and label. Unset, `on_secondary_container`.
    pub selected_color: Option<Color>,
    /// Every other destination's icon and label. Unset, `on_surface_variant`.
    pub unselected_color: Option<Color>,
    /// The labels under the icons, collapsed. Unset, `label_medium`.
    pub label_style: Option<TextStyle>,
    /// The labels beside the icons, expanded. Unset, `label_large`.
    pub expanded_label_style: Option<TextStyle>,
    /// The collapsed width. Unset, 96.
    pub collapsed_width: Option<f32>,
    /// The expanded width. Unset, 220.
    pub expanded_width: Option<f32>,
}

/// One destination: an icon and a label.
#[derive(Clone, Debug, PartialEq)]
pub struct RailDestination {
    icon: IconData,
    selected_icon: Option<IconData>,
    label: String,
}

impl RailDestination {
    /// A destination drawn as `icon`, called `label`.
    ///
    /// ```
    /// use frus_widgets::{Icons, RailDestination};
    /// let _d = RailDestination::new(Icons::INBOX, "Inbox");
    /// ```
    pub fn new(icon: IconData, label: impl Into<String>) -> Self {
        Self {
            icon,
            selected_icon: None,
            label: label.into(),
        }
    }

    /// The icon while it is selected (a filled form, usually).
    ///
    /// ```
    /// use frus_widgets::{Icons, RailDestination};
    /// let _d = RailDestination::new(Icons::INBOX, "Inbox").selected_icon(Icons::MAIL);
    /// ```
    pub fn selected_icon(mut self, icon: IconData) -> Self {
        self.selected_icon = Some(icon);
        self
    }
}

/// The action button at the top of the rail.
struct RailFab<Msg> {
    icon: IconData,
    label: String,
    message: Msg,
}

/// An **expandable navigation rail**: a 96-wide rail of icons with their labels under
/// them, that opens to a 220-wide rail of labelled pills.
///
/// ```
/// use frus_widgets::{ExpandableNavigationRail, Icons};
///
/// #[derive(Clone)]
/// enum Msg { Go(usize), Toggle, Compose }
///
/// let _rail = ExpandableNavigationRail::new(0, Msg::Go)
///     .item(Icons::INBOX, "Inbox")
///     .item(Icons::SEND, "Sent")
///     .expanded(false)
///     .on_toggle(Msg::Toggle)
///     .fab(Icons::EDIT, "Compose", Msg::Compose);
/// ```
///
/// **Themed** through [`ExpandableRailTheme`] (`theme.widgets.expandable_rail`).
pub struct ExpandableNavigationRail<Msg = crate::callback::Callback> {
    selected: usize,
    on_select: Rc<dyn Fn(usize) -> Msg>,
    destinations: Vec<RailDestination>,
    expanded: bool,
    on_toggle: Option<Msg>,
    fab: Option<RailFab<Msg>>,
    background: Option<Color>,
    height: Dimension,
    children: std::cell::OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> ExpandableNavigationRail<Msg> {
    /// A rail with `selected` chosen, `on_select(i)` when another destination is tapped.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        selected: usize,
        on_select: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self {
            selected,
            on_select: Rc::new(crate::callback::handler1(on_select)),
            destinations: Vec::new(),
            expanded: false,
            on_toggle: None,
            fab: None,
            background: None,
            height: Dimension::Percent(1.0),
            children: std::cell::OnceCell::new(),
        }
    }

    /// Adds a destination.
    ///
    /// ```
    /// # use frus_widgets::{ExpandableNavigationRail, Icons};
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).item(Icons::HOME, "Home");
    /// ```
    pub fn item(mut self, icon: IconData, label: impl Into<String>) -> Self {
        self.destinations.push(RailDestination::new(icon, label));
        self
    }

    /// Adds a destination built with [`RailDestination`].
    ///
    /// ```
    /// # use frus_widgets::{ExpandableNavigationRail, Icons, RailDestination};
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i)
    ///     .destination(RailDestination::new(Icons::HOME, "Home"));
    /// ```
    pub fn destination(mut self, destination: RailDestination) -> Self {
        self.destinations.push(destination);
        self
    }

    /// Open (`true`) or closed.
    ///
    /// ```
    /// # use frus_widgets::ExpandableNavigationRail;
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).expanded(true);
    /// ```
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// The message the menu button sends; without one there is no menu button.
    ///
    /// ```
    /// # use frus_widgets::ExpandableNavigationRail;
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).on_toggle(99);
    /// ```
    pub fn on_toggle(mut self, message: impl Into<Msg>) -> Self {
        self.on_toggle = Some(message.into());
        self
    }

    /// The floating action button under the menu button: its icon, the label it shows when
    /// the rail is open, and what it sends.
    ///
    /// ```
    /// # use frus_widgets::{ExpandableNavigationRail, Icons};
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).fab(Icons::ADD, "New", 7);
    /// ```
    pub fn fab(
        mut self,
        icon: IconData,
        label: impl Into<String>,
        message: impl Into<Msg>,
    ) -> Self {
        self.fab = Some(RailFab {
            icon,
            label: label.into(),
            message: message.into(),
        });
        self
    }

    /// The rail's surface.
    ///
    /// ```
    /// # use frus_widgets::{Color, ExpandableNavigationRail};
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).background(Color::rgb(0.95, 0.95, 0.97));
    /// ```
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// The rail's height; its parent's unless told.
    ///
    /// ```
    /// # use frus_widgets::ExpandableNavigationRail;
    /// let _r = ExpandableNavigationRail::<usize>::new(0, |i| i).height(480.0);
    /// ```
    pub fn height(mut self, height: f32) -> Self {
        self.height = Dimension::Length(height);
        self
    }

    fn make(&self) -> Box<dyn Widget<Msg>> {
        let selected = self.selected;
        let on_select = self.on_select.clone();
        let destinations = self.destinations.clone();
        let expanded = self.expanded;
        let toggle = self.on_toggle.clone();
        let fab = self
            .fab
            .as_ref()
            .map(|f| (f.icon, f.label.clone(), f.message.clone()));
        let background = self.background;
        let height = self.height;
        Box::new(crate::themebuilder::ThemeBuilder::boxed(move |theme| {
            let t = theme.widgets.expandable_rail;
            let width = rail_width(theme, expanded);
            let surface_color = background.or(t.background).unwrap_or(theme.scheme.surface);
            let theme = theme.clone();
            // The rail at height `h`: its width animated between the two forms.
            let rail = move |h: f32| -> Box<dyn Widget<Msg>> {
                let l10n = crate::localizations::of();
                let mut col = Flex::column()
                    .gap(4.0)
                    .padding_each(44.0, 0.0, 0.0, 0.0)
                    .align(if expanded {
                        Align::Start
                    } else {
                        Align::Center
                    });
                if let Some(toggle) = toggle.clone() {
                    let menu = crate::IconButton::new(if expanded {
                        crate::icons::Icons::MENU_OPEN
                    } else {
                        crate::icons::Icons::MENU
                    })
                    .label(if expanded {
                        l10n.close_button_label()
                    } else {
                        l10n.open_drawer_label()
                    })
                    .on_press(toggle);
                    col = col.child(
                        Flex::row()
                            .padding_each(0.0, 0.0, 0.0, if expanded { 20.0 } else { 0.0 })
                            .child(menu),
                    );
                }
                if let Some((icon, label, message)) = fab.clone() {
                    let button = if expanded {
                        crate::FloatingActionButton::extended(label)
                            .icon(icon)
                            .on_press(message)
                    } else {
                        crate::FloatingActionButton::new(icon).on_press(message)
                    };
                    col = col.child(
                        Flex::row()
                            .padding_each(4.0, 0.0, 36.0, if expanded { 20.0 } else { 0.0 })
                            .child(button),
                    );
                }
                for (i, d) in destinations.iter().enumerate() {
                    let message = on_select(i);
                    col = col.child(RailItem::<Msg>::build(
                        d,
                        selected == i,
                        expanded,
                        message,
                        &theme,
                    ));
                }
                Box::new(
                    crate::Container::new()
                        .animated_size(
                            width,
                            h,
                            RAIL_DURATION,
                            frus_core::Curve::Cubic {
                                x1: 0.2,
                                y1: 0.0,
                                x2: 0.0,
                                y2: 1.0,
                            },
                        )
                        .color(surface_color)
                        .clip()
                        .child(col),
                )
            };
            match height {
                Dimension::Length(h) => rail(h),
                _ => Box::new(crate::LayoutBuilder::new(move |size: crate::Size| {
                    BoxedItem(rail(size.height))
                })) as Box<dyn Widget<Msg>>,
            }
        }))
    }
}

/// The rail's width, open or closed: the theme's, else 220 and 96.
fn rail_width(theme: &Theme, expanded: bool) -> f32 {
    let t = theme.widgets.expandable_rail;
    if expanded {
        t.expanded_width.unwrap_or(RAIL_EXPANDED_WIDTH)
    } else {
        t.collapsed_width.unwrap_or(RAIL_COLLAPSED_WIDTH)
    }
}

/// One destination, built for the rail's state.
struct RailItem<Msg>(std::marker::PhantomData<Msg>);

impl<Msg: Clone + 'static> RailItem<Msg> {
    fn build(
        d: &RailDestination,
        selected: bool,
        expanded: bool,
        message: Msg,
        theme: &Theme,
    ) -> Box<dyn Widget<Msg>> {
        let t = theme.widgets.expandable_rail;
        let fg = if selected {
            t.selected_color
                .unwrap_or(theme.scheme.on_secondary_container)
        } else {
            t.unselected_color
                .unwrap_or(theme.scheme.on_surface_variant)
        };
        let indicator = t
            .indicator_color
            .unwrap_or(theme.scheme.secondary_container);
        let icon = crate::Icon::new(if selected {
            d.selected_icon.unwrap_or(d.icon)
        } else {
            d.icon
        })
        .size(24.0)
        .color(fg);
        let item: Box<dyn Widget<Msg>> = if expanded {
            let style = t.expanded_label_style.unwrap_or(theme.text.label_large);
            let mut pill = crate::Container::new()
                .height(56.0)
                .radius(28.0)
                .padding_each(0.0, 24.0, 0.0, 16.0)
                .child(
                    Flex::row()
                        .align(Align::Center)
                        .gap(8.0)
                        .height(56.0)
                        .child(icon)
                        .child(Text::styled(d.label.clone(), style).color(fg)),
                );
            if selected {
                pill = pill.color(indicator);
            }
            let ink = crate::InkWell::new()
                .radius(28.0)
                .color(fg)
                .on_click(message)
                .child(pill);
            Box::new(Flex::row().padding_each(0.0, 20.0, 0.0, 20.0).child(ink))
        } else {
            let style = t.label_style.unwrap_or(theme.text.label_medium);
            let mut pill = crate::Container::new()
                .width(56.0)
                .height(32.0)
                .radius(16.0)
                .alignment(frus_core::Alignment::CENTER)
                .child(icon);
            if selected {
                pill = pill.color(indicator);
            }
            let ink = crate::InkWell::new()
                .radius(16.0)
                .color(fg)
                .on_click(message)
                .child(pill);
            Box::new(
                Flex::column()
                    .align(Align::Center)
                    .gap(4.0)
                    .width(RAIL_COLLAPSED_WIDTH)
                    .padding_each(6.0, 0.0, 6.0, 0.0)
                    .child(ink)
                    .child(Text::styled(d.label.clone(), style).color(fg)),
            )
        };
        let sem = frus_core::SemanticsProperties::new(frus_core::Role::Tab)
            .label(d.label.clone())
            .toggled(selected);
        Box::new(crate::Semantics::new(sem, BoxedItem(item)))
    }
}

struct BoxedItem<Msg>(Box<dyn Widget<Msg>>);

impl<Msg: 'static> Widget<Msg> for BoxedItem<Msg> {
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

impl<Msg: Clone + 'static> Widget<Msg> for ExpandableNavigationRail<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.children.get_or_init(|| vec![self.make()])
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "ExpandableNavigationRail"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[test]
    fn collapsed_it_is_96_wide_and_expanded_220_unless_themed() {
        let mut theme = Theme::light();
        assert_eq!(rail_width(&theme, false), RAIL_COLLAPSED_WIDTH);
        assert_eq!(rail_width(&theme, true), RAIL_EXPANDED_WIDTH);
        theme.widgets.expandable_rail.expanded_width = Some(300.0);
        assert_eq!(rail_width(&theme, true), 300.0);
    }

    fn pills(selected: bool, expanded: bool) -> Vec<Rect> {
        let theme = Theme::light();
        let d = RailDestination::new(crate::icons::Icons::INBOX, "Inbox");
        let item = BoxedItem(RailItem::<()>::build(&d, selected, expanded, (), &theme));
        let ui = build_ui(&item, Size::new(240.0, 120.0), &Runtime::default(), &theme);
        ui.scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                frus_core::Primitive::Rect { rect, color, .. }
                    if *color == theme.scheme.secondary_container =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_selected_destination_wears_the_indicator() {
        let collapsed = pills(true, false);
        assert_eq!(collapsed.len(), 1);
        assert_eq!((collapsed[0].width, collapsed[0].height), (56.0, 32.0));
        let expanded = pills(true, true);
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].height, 56.0);
        assert!(pills(false, false).is_empty() && pills(false, true).is_empty());
    }
}
