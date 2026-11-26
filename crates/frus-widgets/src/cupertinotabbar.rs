// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoTabBar`]: the Apple-style bar of tabs along the bottom of a screen — icons
//! over small labels, the selected one in the primary colour, a hairline along its top,
//! over a translucent background (milestone 812).

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use frus_core::{Color, Insets, Rect, Scene};
use frus_layout::{Dimension, FlexDirection, Justify, Style};

use crate::cupertinocolors::{CupertinoColors, CupertinoDynamicColor};
use crate::cupertinotheme::CupertinoTheme;
use crate::icons::IconData;
use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// The bar's height above the room it keeps for the system's bars (`bottom_tab_bar.dart:19`).
pub const CUPERTINO_TAB_BAR_HEIGHT: f32 = 50.0;
/// The icons' size unless told (`:70`).
const ICON_SIZE: f32 = 30.0;
/// The room under each label (`:234`).
const ITEM_BOTTOM: f32 = 4.0;
/// The blur under a translucent bar (`:201`).
const BLUR_SIGMA: f32 = 10.0;
/// The hairline along its top unless told (`:21`).
pub const CUPERTINO_TAB_BAR_BORDER: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0x4D000000),
    Color::from_argb_u32(0x29000000),
);

/// One tab of a [`CupertinoTabBar`]: an icon, another for when it is selected, a label.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoTabBarItem {
    icon: IconData,
    active_icon: Option<IconData>,
    label: String,
    shows_label: bool,
}

impl CupertinoTabBarItem {
    /// A tab with `icon` over `label`.
    pub fn new(icon: IconData, label: impl Into<String>) -> Self {
        Self {
            icon,
            active_icon: None,
            label: label.into(),
            shows_label: true,
        }
    }

    /// A tab with an icon and no label on show. `name` is not drawn; it is what a screen
    /// reader says for the tab.
    pub fn icon_only(icon: IconData, name: impl Into<String>) -> Self {
        Self {
            icon,
            active_icon: None,
            label: name.into(),
            shows_label: false,
        }
    }

    /// The icon shown while the tab is selected — a filled one, usually.
    #[must_use]
    pub fn active_icon(mut self, icon: IconData) -> Self {
        self.active_icon = Some(icon);
        self
    }
}

/// What a bar was told, until it is built against its theme.
struct Parts<Msg> {
    items: Vec<CupertinoTabBarItem>,
    current_index: usize,
    on_tap: Option<Rc<dyn Fn(usize) -> Msg>>,
    active_color: Option<Color>,
    inactive_color: Option<Color>,
    background_color: Option<Color>,
    icon_size: f32,
    height: f32,
    border: Option<Option<Color>>,
}

/// **The Apple-style tab bar**: tabs side by side across the bottom of the screen, each an
/// icon over a small label, the selected one in the primary colour and the others grey.
///
/// ```
/// use frus_widgets::{CupertinoTabBar, CupertinoTabBarItem, Icons};
///
/// #[derive(Clone)]
/// enum Msg { Tab(usize) }
/// let _bar: CupertinoTabBar<Msg> = CupertinoTabBar::new(0, Msg::Tab)
///     .item(CupertinoTabBarItem::new(Icons::HOME, "Home"))
///     .item(CupertinoTabBarItem::new(Icons::SETTINGS, "Settings"));
/// ```
///
/// The selection is the caller's: the bar shows the tab it is told and says which one was
/// tapped. It is 50 pixels tall above the room the system's bars take at the bottom of the
/// screen, which its background runs behind. A hairline runs along its top; a translucent
/// background blurs what is under it.
pub struct CupertinoTabBar<Msg = crate::callback::Callback> {
    parts: RefCell<Option<Parts<Msg>>>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> CupertinoTabBar<Msg> {
    /// A bar whose tab `current_index` is selected, telling `on_tap` the index of a tab
    /// tapped.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        current_index: usize,
        on_tap: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self {
            parts: RefCell::new(Some(Parts {
                items: Vec::new(),
                current_index,
                on_tap: Some(Rc::new(crate::callback::handler1(on_tap))),
                active_color: None,
                inactive_color: None,
                background_color: None,
                icon_size: ICON_SIZE,
                height: CUPERTINO_TAB_BAR_HEIGHT,
                border: None,
            })),
            built: OnceCell::new(),
        }
    }

    fn with(self, f: impl FnOnce(&mut Parts<Msg>)) -> Self {
        if let Some(parts) = self.parts.borrow_mut().as_mut() {
            f(parts);
        }
        self
    }

    /// Adds a tab.
    #[must_use]
    pub fn item(self, item: CupertinoTabBarItem) -> Self {
        self.with(|p| p.items.push(item))
    }

    /// Adds several tabs.
    #[must_use]
    pub fn items(self, items: impl IntoIterator<Item = CupertinoTabBarItem>) -> Self {
        self.with(|p| p.items.extend(items))
    }

    /// The selected tab's colour, in place of the theme's primary colour.
    #[must_use]
    pub fn active_color(self, color: Color) -> Self {
        self.with(|p| p.active_color = Some(color))
    }

    /// The other tabs' colour, in place of the inactive grey.
    #[must_use]
    pub fn inactive_color(self, color: Color) -> Self {
        self.with(|p| p.inactive_color = Some(color))
    }

    /// The background, in place of the theme's bar colour. Opaque, nothing is blurred.
    #[must_use]
    pub fn background_color(self, color: Color) -> Self {
        self.with(|p| p.background_color = Some(color))
    }

    /// The icons' size, 30 by default.
    #[must_use]
    pub fn icon_size(self, size: f32) -> Self {
        self.with(|p| p.icon_size = size.max(0.0))
    }

    /// The bar's height above the system's bars, 50 by default.
    #[must_use]
    pub fn height(self, height: f32) -> Self {
        self.with(|p| p.height = height.max(0.0))
    }

    /// The hairline along its top, in `color`.
    #[must_use]
    pub fn border_color(self, color: Color) -> Self {
        self.with(|p| p.border = Some(Some(color)))
    }

    /// No hairline along its top.
    #[must_use]
    pub fn no_border(self) -> Self {
        self.with(|p| p.border = Some(None))
    }
}

/// One tab: the whole of its share of the bar answers a tap, and says what it is.
struct Tab<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    message: Option<Msg>,
    semantics: frus_core::SemanticsProperties,
}

impl<Msg: Clone> Widget<Msg> for Tab<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            justify: Justify::End,
            padding: Insets::new(0.0, 0.0, ITEM_BOTTOM, 0.0),
            height: Dimension::Percent(1.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        self.message.clone()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        Some(self.semantics.clone())
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoTab"
    }
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    let c = CupertinoTheme::of(theme);
    let media = crate::MediaQuery::of();
    let safe = media.padding;
    let background = p.background_color.unwrap_or(c.bar_background_color);
    let border = p
        .border
        .unwrap_or(Some(CUPERTINO_TAB_BAR_BORDER.resolve(theme)));
    let hairline = 1.0 / media.device_pixel_ratio.max(1.0);
    let active = p.active_color.unwrap_or(c.primary_color);
    let inactive = p
        .inactive_color
        .unwrap_or_else(|| CupertinoColors::INACTIVE_GRAY.resolve(theme));
    let label_style = c.text_theme.tab_label_text_style;
    let count = p.items.len();

    let mut row = crate::Row::new().cross_axis_alignment(crate::Align::Stretch);
    for (index, item) in p.items.into_iter().enumerate() {
        let selected = index == p.current_index;
        let colour = if selected { active } else { inactive };
        let icon = if selected {
            item.active_icon.unwrap_or(item.icon)
        } else {
            item.icon
        };
        let mut column = crate::Column::new()
            .main_axis_alignment(Justify::End)
            .cross_axis_alignment(crate::Align::Center)
            .child(crate::Expanded::new(crate::Center::new(
                crate::Icon::new(icon).size(p.icon_size).color(colour),
            )));
        if item.shows_label {
            column = column.child(crate::ExcludeSemantics::new(
                crate::Text::styled(item.label.clone(), label_style.color(colour))
                    .no_wrap()
                    .ellipsis(),
            ));
        }
        let position = crate::localizations::of().tab_label(index + 1, count);
        let said = format!("{}, {position}", item.label);
        let semantics = frus_core::SemanticsProperties::new(frus_core::Role::Tab)
            .label(said)
            .toggled(selected)
            .clickable();
        let message = p.on_tap.as_ref().map(|f| f(index));
        row = row.child(crate::Expanded::new(Tab {
            children: vec![Box::new(column)],
            message,
            semantics,
        }));
    }

    let surface = crate::CustomPaint::new(0.0, 0.0, move |scene, bounds, _| {
        scene.fill_rect(bounds, background);
        if let Some(line) = border {
            scene.fill_rect(Rect::new(bounds.x, bounds.y, bounds.width, hairline), line);
        }
    });
    let bar = crate::SizedBox::expand(
        crate::Stack::new()
            .fit(crate::StackFit::Expand)
            .layer(crate::Positioned::fill(surface))
            .layer(crate::Padding::new(
                Insets::new(0.0, safe.right, safe.bottom, safe.left),
                row,
            )),
    )
    .height(p.height + safe.bottom);
    Box::new(
        crate::ClipRect::new().child(
            crate::BackdropFilter::blur(BLUR_SIGMA)
                .enabled(background.a < 1.0)
                .child(bar),
        ),
    )
}

impl<Msg: Clone + 'static> Widget<Msg> for CupertinoTabBar<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| {
            let Some(parts) = self.parts.borrow_mut().take() else {
                return Vec::new();
            };
            vec![Box::new(crate::themebuilder::ThemeBuilder::boxed(
                move |theme| build(parts, theme),
            ))]
        })
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    /// The row of tabs, holding them (milestone 848): a reader hears which of how many each is.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        Some(frus_core::SemanticsProperties::new(
            frus_core::Role::TabList,
        ))
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoTabBar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cupertinobutton::AllPrimitives;
    use crate::{build_ui, Icons, MediaQuery, Runtime, Ui};
    use frus_core::{Point, Primitive, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Tab(usize),
    }

    /// A two-tab bar at the bottom of a 320 × 480 window over a 34-pixel gesture bar.
    fn lay(bar: CupertinoTabBar<Msg>, theme: &Theme) -> Ui<Msg> {
        let mut media = MediaQuery::new(Size::new(320.0, 480.0));
        media.padding = Insets::new(0.0, 0.0, 34.0, 0.0);
        let _surface = media.install();
        let root = crate::Column::new()
            .main_axis_alignment(Justify::End)
            .child(bar);
        build_ui(&root, Size::new(320.0, 480.0), &Runtime::default(), theme)
    }

    fn two() -> CupertinoTabBar<Msg> {
        CupertinoTabBar::new(0, Msg::Tab)
            .item(CupertinoTabBarItem::new(Icons::HOME, "Home"))
            .item(CupertinoTabBarItem::new(Icons::SETTINGS, "Settings"))
    }

    /// **50 pixels above the gesture bar, the bar's colour behind both, a hairline on top.**
    #[test]
    fn the_bar_is_50_above_the_gesture_bar() {
        let theme = Theme::light();
        let ui = lay(two(), &theme);
        let background = CupertinoTheme::of(&theme).bar_background_color;
        let rect = |colour: Color| {
            ui.scene()
                .all()
                .into_iter()
                .find_map(|p| match p {
                    Primitive::Rect { rect, color, .. } if *color == colour => Some(*rect),
                    _ => None,
                })
                .expect("painted")
        };
        assert_eq!(rect(background), Rect::new(0.0, 396.0, 320.0, 84.0));
        assert_eq!(
            rect(CUPERTINO_TAB_BAR_BORDER.resolve(&theme)),
            Rect::new(0.0, 396.0, 320.0, 1.0)
        );
    }

    /// **The selected tab is in the primary colour and the others grey**, icons and labels
    /// alike.
    #[test]
    fn the_selected_tab_is_in_the_primary_colour() {
        let theme = Theme::light();
        let ui = lay(two(), &theme);
        let colours: Vec<(String, Color)> = ui
            .scene()
            .all()
            .into_iter()
            .filter_map(|p| match p {
                Primitive::Text { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect();
        let of = |label: &str| {
            colours
                .iter()
                .find(|(t, _)| t == label)
                .map(|(_, c)| *c)
                .expect("drawn")
        };
        assert_eq!(of("Home"), CupertinoTheme::of(&theme).primary_color);
        assert_eq!(
            of("Settings"),
            CupertinoColors::INACTIVE_GRAY.resolve(&theme)
        );
    }

    /// **Each tab is announced with its place and whether it is selected, and a tap on it
    /// sends its index.** Right to left, the first tab is on the right.
    #[test]
    fn each_tab_is_announced_and_tappable() {
        let theme = Theme::light();
        let ui = lay(two(), &theme);
        let tabs: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Tab)
            .cloned()
            .collect();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].2.label.as_deref(), Some("Home, Tab 1 of 2"));
        assert_eq!(tabs[0].2.toggled, frus_core::Toggled::True);
        assert_eq!(tabs[1].2.toggled, frus_core::Toggled::False);
        let id = ui.hit(Point::new(240.0, 420.0)).expect("a tab");
        assert_eq!(ui.msg_for(id), Some(Msg::Tab(1)));
        // Each takes half the width, all of the bar's height above the gesture bar.
        assert_eq!(tabs[0].1, Rect::new(0.0, 396.0, 160.0, 50.0));

        let ui = lay(two(), &Theme::light().rtl());
        let id = ui.hit(Point::new(240.0, 420.0)).expect("a tab");
        assert_eq!(ui.msg_for(id), Some(Msg::Tab(0)));
    }

    /// **An icon-only tab draws no label and is still named to a screen reader.**
    #[test]
    fn an_icon_only_tab_is_named_but_not_labelled() {
        let theme = Theme::light();
        let bar = CupertinoTabBar::new(0, Msg::Tab)
            .item(CupertinoTabBarItem::icon_only(Icons::HOME, "Home"))
            .item(CupertinoTabBarItem::new(Icons::SETTINGS, "Settings"));
        let ui = lay(bar, &theme);
        let texts: Vec<String> = ui
            .scene()
            .all()
            .into_iter()
            .filter_map(|p| match p {
                Primitive::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(!texts.iter().any(|t| t == "Home"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Settings"), "{texts:?}");
        let names: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Tab)
            .map(|(_, _, s)| s.label.clone())
            .collect();
        assert_eq!(names[0].as_deref(), Some("Home, Tab 1 of 2"));
    }
}
