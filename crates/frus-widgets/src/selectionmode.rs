// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Selection mode**: a long press puts a list or a grid into multi-select, the app bar
//! becomes a contextual bar, checkmarks appear and Back leaves the mode (milestone 786).
//!
//! - [`SelectionController`] keeps which items are selected and whether the mode is on.
//! - [`SelectableItem`] is one item: a press opens it outside the mode and toggles it
//!   inside, a long press starts the mode, and a check shows inside the mode.
//! - [`SelectionAppBar`] is the contextual bar: a close button, "3 selected", the
//!   actions on the selection — and, while it is on screen, what Back does.

use std::hash::Hash;

use frus_core::{BorderRadius, Color, Insets, Rect, Scene, TextStyle};
use frus_layout::{Align, FlexDirection, Style};

use crate::icons::Icons;
use crate::interaction::Status;
use crate::lazy::Lazy;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{IconButton, Text};

/// The room a selectable item keeps at its start for the check while the mode is on.
pub const SELECTION_CHECK_ROOM: f32 = 48.0;
/// The check's side.
pub const SELECTION_CHECK_SIZE: f32 = 24.0;

/// Defaults for [`SelectableItem`] and [`SelectionAppBar`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SelectionTheme {
    /// A selected item's surface. Unset, `secondary_container`.
    pub selected_background: Option<Color>,
    /// The check on a selected item. Unset, `primary`.
    pub check_color: Option<Color>,
    /// The empty circle on an item not selected. Unset, `on_surface_variant`.
    pub unchecked_color: Option<Color>,
    /// The contextual bar's surface. Unset, `secondary_container`.
    pub bar_background: Option<Color>,
    /// The contextual bar's words and glyphs. Unset, `on_secondary_container`.
    pub bar_foreground: Option<Color>,
    /// The count's type. Unset, `title_large`.
    pub title_style: Option<TextStyle>,
}

/// **Which items are selected, and whether the list is in selection mode.**
///
/// ```
/// # use frus_widgets::SelectionController;
/// let mut selection = SelectionController::new();
/// assert!(!selection.tap(&7));        // outside the mode: open item 7
/// selection.long_press(7);            // the mode starts, with 7 selected
/// assert!(selection.tap(&9));         // inside it: 9 is toggled on
/// assert_eq!(selection.count(), 2);
/// selection.tap(&7);
/// selection.tap(&9);                  // the last one off ends the mode
/// assert!(!selection.is_active());
/// ```
///
/// Keep one in the screen's state, keyed by whatever identifies an item; send a message
/// from each item's press and long press, and from the contextual bar's close button
/// and Back, and answer them with [`tap`](Self::tap), [`long_press`](Self::long_press)
/// and [`clear`](Self::clear).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionController<K> {
    selected: Vec<K>,
    active: bool,
}

impl<K> Default for SelectionController<K> {
    fn default() -> Self {
        Self {
            selected: Vec::new(),
            active: false,
        }
    }
}

impl<K: Clone + Eq + Hash> SelectionController<K> {
    /// Nothing selected, the mode off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the list is in selection mode.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether `key` is selected.
    pub fn is_selected(&self, key: &K) -> bool {
        self.selected.contains(key)
    }

    /// How many are selected.
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// The selected keys, in the order they were selected.
    pub fn selected(&self) -> &[K] {
        &self.selected
    }

    /// A **long press** on `key`: starts the mode with it selected; inside the mode,
    /// toggles it like a press.
    pub fn long_press(&mut self, key: K) {
        if self.active {
            self.toggle(key);
        } else {
            self.active = true;
            self.selected = vec![key];
        }
    }

    /// A **press** on `key`. Inside the mode it toggles the key and returns `true`: the
    /// press was the selection's. Outside it, it returns `false`: the press is the
    /// item's own — open it.
    pub fn tap(&mut self, key: &K) -> bool {
        if !self.active {
            return false;
        }
        self.toggle(key.clone());
        true
    }

    /// Selects `key` if it was not, deselects it if it was — starting the mode when it
    /// was off, and ending it when the last one goes.
    pub fn toggle(&mut self, key: K) {
        if let Some(at) = self.selected.iter().position(|k| *k == key) {
            self.selected.remove(at);
            if self.selected.is_empty() {
                self.active = false;
            }
        } else {
            self.selected.push(key);
            self.active = true;
        }
    }

    /// Selects every one of `keys`, starting the mode.
    pub fn select_all(&mut self, keys: impl IntoIterator<Item = K>) {
        for key in keys {
            if !self.selected.contains(&key) {
                self.selected.push(key);
            }
        }
        self.active = !self.selected.is_empty();
    }

    /// Deselects everything and leaves the mode: the close button, and Back.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.active = false;
    }

    /// The item for `key`, with its state from this controller: `on_tap` and
    /// `on_long_press` are what its press and its long press send.
    pub fn item<Msg: Clone + 'static>(
        &self,
        key: &K,
        child: impl Widget<Msg> + 'static,
        on_tap: impl Into<Msg>,
        on_long_press: impl Into<Msg>,
    ) -> SelectableItem<Msg> {
        SelectableItem::new(child, self.is_selected(key), self.active)
            .count(self.count())
            .on_tap(on_tap)
            .on_long_press(on_long_press)
    }

    /// The contextual bar for this selection: `on_close` is what its close button and
    /// Back send — answer it with [`clear`](Self::clear).
    pub fn app_bar<Msg: Clone + 'static>(&self, on_close: impl Into<Msg>) -> SelectionAppBar<Msg> {
        SelectionAppBar::new(self.count(), on_close)
    }
}

/// **One item of a list in selection mode** — what a long press selects and a press
/// toggles, with a check at its start while the mode is on.
///
/// Usually made by [`SelectionController::item`]. The item takes the presses itself, so
/// what it holds should not: a row of text and pictures, not a row with its own buttons.
/// A reader hears it as an item of a list, ticked or not while the mode is on, and on a
/// press hears how many are now selected.
pub struct SelectableItem<Msg = crate::callback::Callback> {
    children: Vec<Box<dyn Widget<Msg>>>,
    selected: bool,
    active: bool,
    count: Option<usize>,
    on_tap: Option<Msg>,
    on_long_press: Option<Msg>,
    selected_background: Option<Color>,
    check_color: Option<Color>,
    radius: f32,
}

impl<Msg: Clone + 'static> SelectableItem<Msg> {
    /// An item holding `child`: selected or not, in a list in selection mode (`active`)
    /// or not.
    pub fn new(child: impl Widget<Msg> + 'static, selected: bool, active: bool) -> Self {
        Self {
            children: vec![Box::new(child)],
            selected,
            active,
            count: None,
            on_tap: None,
            on_long_press: None,
            selected_background: None,
            check_color: None,
            radius: 0.0,
        }
    }

    /// What a press sends.
    #[must_use]
    pub fn on_tap(mut self, message: impl Into<Msg>) -> Self {
        self.on_tap = Some(message.into());
        self
    }

    /// What a long press sends.
    #[must_use]
    pub fn on_long_press(mut self, message: impl Into<Msg>) -> Self {
        self.on_long_press = Some(message.into());
        self
    }

    /// How many items are selected now, so a press can say how many there will be.
    #[must_use]
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// A selected item's surface, over the theme's and `secondary_container`.
    #[must_use]
    pub fn selected_background(mut self, color: Color) -> Self {
        self.selected_background = Some(color);
        self
    }

    /// The check's colour, over the theme's and `primary`.
    #[must_use]
    pub fn check_color(mut self, color: Color) -> Self {
        self.check_color = Some(color);
        self
    }

    /// The corners of a selected item's surface — for the tiles of a grid. Square by
    /// default.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for SelectableItem<Msg> {
    fn style(&self) -> Style {
        Widget::<Msg>::style_themed(self, &Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        let room = if self.active {
            SELECTION_CHECK_ROOM
        } else {
            0.0
        };
        let padding = if theme.direction == frus_core::TextDirection::Rtl {
            Insets::new(0.0, room, 0.0, 0.0)
        } else {
            Insets::new(0.0, 0.0, 0.0, room)
        };
        Style {
            flex_direction: FlexDirection::Row,
            align: Align::Center,
            padding,
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let t = theme.widgets.selection;
        if self.selected {
            let fill = self
                .selected_background
                .or(t.selected_background)
                .unwrap_or(theme.scheme.secondary_container);
            scene.draw_rect(
                bounds,
                fill.fade(o),
                BorderRadius::uniform(self.radius),
                0.0,
                Color::TRANSPARENT,
            );
        }
        let layer = theme.state_layer(Color::TRANSPARENT, theme.scheme.on_surface, &status);
        if layer.a > 0.0 {
            scene.draw_rect(
                bounds,
                layer.fade(o),
                BorderRadius::uniform(self.radius),
                0.0,
                Color::TRANSPARENT,
            );
        }
        if self.active {
            let (glyph, color) = if self.selected {
                (
                    Icons::CHECK_CIRCLE,
                    self.check_color
                        .or(t.check_color)
                        .unwrap_or(theme.scheme.primary),
                )
            } else {
                (
                    Icons::RADIO_BUTTON_UNCHECKED,
                    t.unchecked_color.unwrap_or(theme.scheme.on_surface_variant),
                )
            };
            let inset = (SELECTION_CHECK_ROOM - SELECTION_CHECK_SIZE) * 0.5;
            let x = if theme.direction == frus_core::TextDirection::Rtl {
                bounds.x + bounds.width - inset - SELECTION_CHECK_SIZE
            } else {
                bounds.x + inset
            };
            let y = bounds.y + (bounds.height - SELECTION_CHECK_SIZE) * 0.5;
            let path = glyph.placed(SELECTION_CHECK_SIZE, x, y, theme.direction);
            scene.fill_path(&path, color.fade(o));
        }
    }

    fn on_click(&self) -> Option<Msg> {
        self.on_tap.clone()
    }

    fn on_long_press(&self) -> Option<Msg> {
        self.on_long_press.clone()
    }

    fn focusable(&self) -> bool {
        self.on_tap.is_some()
    }

    fn ink(&self, theme: &Theme) -> Option<crate::InkStyle> {
        self.on_tap.as_ref()?;
        Some(crate::InkStyle::of(theme).radius(BorderRadius::uniform(self.radius)))
    }

    /// An item of a list; while the mode is on, ticked or not.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let semantics = frus_core::SemanticsProperties::new(frus_core::Role::ListItem);
        let semantics = if self.active {
            semantics.toggled(self.selected)
        } else {
            semantics
        };
        Some(if self.on_tap.is_some() {
            semantics.clickable()
        } else {
            semantics
        })
    }

    /// Inside the mode a press changes the count, and the count is what is said.
    fn announce(&self) -> Option<String> {
        if !self.active {
            return None;
        }
        let now = self.count?;
        let next = if self.selected {
            now.saturating_sub(1)
        } else {
            now + 1
        };
        Some(crate::localizations::of().selected_row_count_label(next))
    }
}

struct BarParts<Msg> {
    count: usize,
    on_close: Msg,
    actions: Vec<Box<dyn Widget<Msg>>>,
    background: Option<Color>,
    foreground: Option<Color>,
    title_style: Option<TextStyle>,
}

/// **The contextual bar of a list in selection mode**: a close button, how many are
/// selected, and the actions on them — in the app bar's place while the mode is on.
///
/// ```
/// # use frus_widgets::{IconButton, Icons, SelectionAppBar};
/// # #[derive(Clone)] enum Msg { Clear, Delete }
/// SelectionAppBar::new(3, Msg::Clear)
///     .action(IconButton::new(Icons::DELETE).label("Delete").on_press(Msg::Delete));
/// ```
///
/// It is an [`AppBar`](crate::AppBar) in `secondary_container`, its title the count as
/// the reader's language says it ("3 selected"). **While it is on screen the system's
/// Back — and Escape — send `on_close`**, before the router is asked to go back: Back
/// leaves the mode first, as a platform's own lists do.
pub struct SelectionAppBar<Msg = crate::callback::Callback> {
    inner: Lazy<BarParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> SelectionAppBar<Msg> {
    /// A bar for `count` selected items; `on_close` is what the close button and Back
    /// send.
    pub fn new(count: usize, on_close: impl Into<Msg>) -> Self {
        Self {
            inner: Lazy::new(
                BarParts {
                    count,
                    on_close: on_close.into(),
                    actions: Vec::new(),
                    background: None,
                    foreground: None,
                    title_style: None,
                },
                build_bar::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut BarParts<Msg> {
        self.inner.parts_mut()
    }

    /// An action on the selection, at the end.
    #[must_use]
    pub fn action(mut self, action: impl Widget<Msg> + 'static) -> Self {
        self.p().actions.push(Box::new(action));
        self
    }

    /// The surface and what is drawn on it, over the theme's.
    #[must_use]
    pub fn colors(mut self, background: Color, foreground: Color) -> Self {
        let p = self.p();
        p.background = Some(background);
        p.foreground = Some(foreground);
        self
    }

    /// The count's type, over the theme's and `title_large`.
    #[must_use]
    pub fn title_style(mut self, style: TextStyle) -> Self {
        self.p().title_style = Some(style);
        self
    }
}

crate::composite::composite_widget!(SelectionAppBar);

/// The bar, answering Back while it is on screen.
struct BackScope<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    on_back: Msg,
}

impl<Msg: Clone> Widget<Msg> for BackScope<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn back_dismiss(&self) -> Option<Msg> {
        Some(self.on_back.clone())
    }
}

fn build_bar<Msg: Clone + 'static>(mut p: BarParts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.selection;
        let background = p
            .background
            .or(t.bar_background)
            .unwrap_or(theme.scheme.secondary_container);
        let ink = p
            .foreground
            .or(t.bar_foreground)
            .unwrap_or(theme.scheme.on_secondary_container);
        let style = p
            .title_style
            .or(t.title_style)
            .unwrap_or(theme.text.title_large);
        let words = crate::localizations::of().selected_row_count_label(p.count);
        let mut bar = crate::AppBar::new()
            .background(background)
            .foreground(ink)
            .leading(
                IconButton::new(Icons::CLOSE)
                    .icon_color(ink)
                    .label(crate::localizations::of().close_button_label())
                    .on_press(p.on_close.clone()),
            )
            .title(Text::styled(words, style).color(ink));
        for action in p.actions.drain(..) {
            bar = bar.action(action);
        }
        Box::new(BackScope {
            children: vec![bar.build()],
            on_back: p.on_close.clone(),
        }) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Tap(u32),
        Hold(u32),
        Clear,
        Delete,
    }

    /// **The controller's rules**: a press opens outside the mode and toggles inside it,
    /// a long press starts it, the last one off ends it, and clearing leaves it.
    #[test]
    fn the_controller_follows_the_pattern() {
        let mut s = SelectionController::new();
        assert!(!s.tap(&1), "outside the mode a press is the item's");
        assert!(!s.is_active());
        s.long_press(1);
        assert!(s.is_active() && s.is_selected(&1));
        assert!(s.tap(&2));
        assert_eq!(s.selected(), &[1, 2]);
        s.long_press(2);
        assert_eq!(s.selected(), &[1], "a long press inside the mode toggles");
        s.tap(&1);
        assert!(!s.is_active(), "the last one off ends the mode");
        s.select_all([3, 4, 5]);
        assert_eq!(s.count(), 3);
        s.clear();
        assert!(!s.is_active() && s.count() == 0);
    }

    fn page(selection: &SelectionController<u32>) -> crate::Flex<Msg> {
        let mut list = crate::Flex::column();
        if selection.is_active() {
            list = list.child(
                selection.app_bar(Msg::Clear).action(
                    IconButton::new(Icons::DELETE)
                        .label("Delete")
                        .on_press(Msg::Delete),
                ),
            );
        }
        for key in 0..3u32 {
            list = list.child(
                selection.item(
                    &key,
                    crate::Container::new()
                        .height(56.0)
                        .child(Text::new(format!("Row {key}"))),
                    Msg::Tap(key),
                    Msg::Hold(key),
                ),
            );
        }
        list
    }

    /// **The mode as a screen**: a long press reaches the item; once on, the bar says how
    /// many, Back sends its close, checks appear, items say they are ticked, and a press
    /// says the count it leads to.
    #[test]
    fn the_mode_shows_the_bar_the_checks_and_answers_back() {
        let theme = Theme::default();
        let mut selection = SelectionController::new();
        let ui = build_ui(
            &page(&selection),
            Size::new(360.0, 400.0),
            &Runtime::default(),
            &theme,
        );
        assert_eq!(
            ui.top_dismiss(),
            None,
            "outside the mode Back is the router's"
        );
        assert_eq!(
            ui.long_press_at(frus_core::Point::new(100.0, 28.0)),
            Some(Msg::Hold(0))
        );

        selection.long_press(0);
        selection.tap(&2);
        let tree = page(&selection);
        let ui = build_ui(&tree, Size::new(360.0, 400.0), &Runtime::default(), &theme);
        assert_eq!(ui.top_dismiss(), Some(Msg::Clear), "Back leaves the mode");
        let said: Vec<_> = ui.semantics().iter().map(|(_, _, s)| s.clone()).collect();
        assert!(
            said.iter()
                .any(|s| s.label.as_deref() == Some("2 selected")),
            "the count is the bar's title: {said:#?}"
        );
        let items: Vec<_> = said
            .iter()
            .filter(|s| s.role == frus_core::Role::ListItem)
            .map(|s| s.toggled)
            .collect();
        assert_eq!(
            items,
            vec![
                frus_core::Toggled::True,
                frus_core::Toggled::False,
                frus_core::Toggled::True
            ]
        );
        let checks = ui
            .scene()
            .primitives()
            .iter()
            .filter(|p| matches!(p, frus_core::Primitive::Path { .. }))
            .count();
        assert!(checks >= 3, "a check or a circle on every item");
        let item = selection.item(&1, Text::new("Row 1"), Msg::Tap(1), Msg::Hold(1));
        assert_eq!(
            Widget::<Msg>::announce(&item).as_deref(),
            Some("3 selected"),
            "a press on an unselected item says the count it leads to"
        );
        let item = selection.item(&0, Text::new("Row 0"), Msg::Tap(0), Msg::Hold(0));
        assert_eq!(
            Widget::<Msg>::announce(&item).as_deref(),
            Some("1 selected")
        );
    }
}
