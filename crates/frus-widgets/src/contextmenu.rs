// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`ContextMenu`] and [`ContextMenuState`]: the menu a right-click — or a long press on a
//! touch screen — opens **at the pointer** (milestone 809).
//!
//! It is the menu a [`MenuBar`](crate::MenuBar) drops, opened from somewhere else: the same
//! rows ([`MenuItem`]: actions, ticks, radio choices, keys shown on the right, rules,
//! unavailable rows), the same submenus ([`SubmenuButton`]), the same keyboard (the arrows
//! move between rows and open and leave submenus, Escape closes one level), and the same panel.
//!
//! # Where "is it open" lives
//!
//! In the application, as a [`ContextMenuState`]: where the menu was opened and which submenus
//! are open in it. One field and one message that carries the new state — the way every menu
//! here works, so `update` stays the one place the interface's state changes.

use std::rc::Rc;

use frus_core::{Point, Rect, Scene, Size};
use frus_layout::Style;

use crate::interaction::Status;
use crate::menu::{MenuItem, PanelKind};
use crate::menubar::{panel, Across, Entry, MenuPath, OnPath, SubmenuButton};
use crate::portal::Placement;
use crate::theme::Theme;
use crate::widget::Widget;

/// The room a menu at the pointer keeps from the window's edges, in pixels.
const EDGE: f32 = 4.0;

/// **Where a menu of `size` opened at `at` goes**, its top-left corner: its start corner on the
/// point, opening towards the end and down; towards the start where the window has no room at
/// the end, and up where it has none below; and held inside the window if it fits neither way.
pub(crate) fn place_at_point(at: Point, size: Size, window: Rect, rtl: bool) -> (f32, f32) {
    let (w, h) = (size.width, size.height);
    let left = window.x + EDGE;
    let top = window.y + EDGE;
    let right = window.x + window.width - EDGE;
    let bottom = window.y + window.height - EDGE;
    let (forward, back) = if rtl {
        (at.x - w, at.x)
    } else {
        (at.x, at.x - w)
    };
    let fits_x = |x: f32| x >= left && x + w <= right;
    let x = if fits_x(forward) {
        forward
    } else if fits_x(back) {
        back
    } else {
        forward
    };
    let y = if at.y + h <= bottom {
        at.y
    } else if at.y - h >= top {
        at.y - h
    } else {
        at.y
    };
    (
        x.min(right - w).max(left.min(right - w).max(window.x)),
        y.min(bottom - h).max(top.min(bottom - h).max(window.y)),
    )
}

/// **Whether a [`ContextMenu`] is open, where, and how deep**: the point it was opened at, in
/// the window's logical pixels, and which of its submenus are open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenuState {
    at: Option<Point>,
    path: MenuPath,
}

impl ContextMenuState {
    /// The menu closed.
    pub fn closed() -> Self {
        Self::default()
    }

    /// The menu open at `at`, in the window's logical pixels, with no submenu open.
    pub fn open_at(at: Point) -> Self {
        Self {
            at: Some(at),
            path: MenuPath::from_indices([0]),
        }
    }

    /// Whether the menu is open.
    pub fn is_open(&self) -> bool {
        self.at.is_some() && self.path.is_open()
    }

    /// Where the menu was opened, if it is open.
    pub fn at(&self) -> Option<Point> {
        self.at.filter(|_| self.path.is_open())
    }

    /// Which submenus are open: the menu itself is level `0`, entry `0`; its submenus follow.
    pub fn path(&self) -> &MenuPath {
        &self.path
    }

    /// The state with `path` open, at the same point; closed when `path` is.
    fn with_path(&self, path: MenuPath) -> Self {
        if path.is_open() {
            Self { at: self.at, path }
        } else {
            Self::closed()
        }
    }
}

/// **The menu a right-click opens at the pointer**, or a long press on a touch screen.
///
/// It wraps the part of the screen it belongs to — a row, a picture, a canvas — and holds the
/// menu's rows. The application keeps the [`ContextMenuState`] and is sent the new one whenever
/// it should change: opened at the pointer, a submenu opened or closed, the menu closed by a
/// press outside it or by Escape.
///
/// ```
/// use frus_widgets::{ContextMenu, ContextMenuState, MenuItem, SubmenuButton, Text};
///
/// #[derive(Clone)]
/// enum Msg { Menu(ContextMenuState), Copy, Delete, Sort(usize), Wrap }
///
/// let state = ContextMenuState::closed();
/// let wrap = true;
/// let menu = ContextMenu::new(Text::new("Right-click me"), &state, Msg::Menu)
///     .item(MenuItem::new("Copy", Msg::Copy).shortcut("Ctrl+C"))
///     .item(MenuItem::new("Delete", Msg::Delete).enabled(false))
///     .divider()
///     .item(MenuItem::checked("Word wrap", wrap, Msg::Wrap))
///     .submenu(
///         SubmenuButton::new("Sort by")
///             .item(MenuItem::radio("Name", true, Msg::Sort(0)))
///             .item(MenuItem::radio("Date", false, Msg::Sort(1))),
///     );
/// # let _: ContextMenu<Msg> = menu;
/// ```
///
/// A row's own message is sent when it is chosen. **Close the menu when you handle it**
/// (`self.menu = ContextMenuState::closed()`), as a menu bar's are closed: a press is one
/// message, and it is the row's.
///
/// The menu opens with its start corner on the pointer, towards the end and down, and towards
/// the start or up where the window has no room. Its look is the caller's
/// ([`menu_style`](Self::menu_style)), then the theme's
/// [`context_menu`](crate::WidgetThemes::context_menu), then a desktop's menus on a desktop
/// theme, then the application's [`MenuTheme`](crate::MenuTheme).
pub struct ContextMenu<Msg = crate::callback::Callback> {
    state: ContextMenuState,
    on_change: Rc<dyn Fn(ContextMenuState) -> Msg>,
    entries: Vec<Entry<Msg>>,
    menus: crate::MenuTheme,
    enabled: bool,
    /// `[child]`, or `[child, menu]` while it is open.
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> ContextMenu<Msg> {
    /// A context menu over `child`, open as `state` says, sending `on_change` the state it
    /// should have next.
    pub fn new<R1: crate::callback::IntoMsg<Msg>>(
        child: impl Widget<Msg> + 'static,
        state: &ContextMenuState,
        on_change: impl Fn(ContextMenuState) -> R1 + 'static,
    ) -> Self {
        let on_change = crate::callback::handler1(on_change);
        Self {
            state: state.clone(),
            on_change: Rc::new(on_change),
            entries: Vec::new(),
            menus: crate::MenuTheme::default(),
            enabled: true,
            children: vec![Box::new(child)],
        }
    }

    /// Adds a row.
    #[must_use]
    pub fn item(mut self, item: MenuItem<Msg>) -> Self {
        self.entries.push(Entry::Item(item));
        self.rebuilt()
    }

    /// Adds a row that opens `submenu` beside the menu.
    #[must_use]
    pub fn submenu(mut self, submenu: SubmenuButton<Msg>) -> Self {
        self.entries.push(Entry::Submenu(submenu));
        self.rebuilt()
    }

    /// Adds the rule between two groups of rows.
    #[must_use]
    pub fn divider(self) -> Self {
        self.item(MenuItem::divider())
    }

    /// **The menu's look**: its rows, their type, its panel. What it says outranks the theme's
    /// context menu, a desktop's menus and the application's [`MenuTheme`](crate::MenuTheme).
    #[must_use]
    pub fn menu_style(mut self, menus: crate::MenuTheme) -> Self {
        self.menus = menus;
        self.rebuilt()
    }

    /// Whether a right-click or a long press opens the menu. A disabled one is its child.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.rebuilt()
    }

    /// The message that opens the menu at `at`.
    fn opening(&self, at: Point) -> Option<Msg> {
        (self.enabled && !self.entries.is_empty())
            .then(|| (self.on_change)(ContextMenuState::open_at(at)))
    }

    /// Builds the menu again (child 1), so that a row or a style said after another reaches it.
    fn rebuilt(mut self) -> Self {
        self.children.truncate(1);
        if !(self.enabled && self.state.is_open() && !self.entries.is_empty()) {
            return self;
        }
        let on_change = self.on_change.clone();
        let state = self.state.clone();
        let on_path: OnPath<Msg> = Rc::new(move |path| on_change(state.with_path(path)));
        let across = Across {
            previous: None,
            next: None,
        };
        let menu = panel(
            &self.entries,
            1,
            &self.state.path,
            &on_path,
            &across,
            &self.menus,
            PanelKind::Context,
        );
        self.children.push(menu);
        self
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for ContextMenu<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    /// The menu is not a box round its child: it is its child, with something that may float
    /// over it. The child's structure is forwarded, its padding left to the child.
    fn style_themed(&self, theme: &Theme) -> Style {
        match self.children.first() {
            Some(child) => Style {
                padding: frus_core::Insets::ZERO,
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

    /// A right-click with no point to go by — the shell always gives one; this is what a
    /// widget asked without it answers — opens the menu at the window's corner.
    fn on_secondary_tap(&self) -> Option<Msg> {
        self.opening(Point::new(0.0, 0.0))
    }

    fn on_secondary_tap_at(&self, at: Point) -> Option<Msg> {
        self.opening(at)
    }

    fn on_long_press(&self) -> Option<Msg> {
        self.opening(Point::new(0.0, 0.0))
    }

    fn on_long_press_at(&self, at: Point) -> Option<Msg> {
        self.opening(at)
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.children
            .get(1)
            .map(|menu| (menu.as_ref(), Placement::Pointer))
    }

    fn overlay_at(&self) -> Option<Point> {
        self.state.at()
    }

    /// A press outside the menu, or Escape with nothing in the menu answering it, closes it.
    fn overlay_dismiss(&self) -> Option<Msg> {
        (self.children.len() > 1).then(|| (self.on_change)(ContextMenuState::closed()))
    }

    /// The arrows and Tab stay in the menu while it is open, as a menu's keyboard does.
    fn overlay_traps_focus(&self) -> bool {
        self.children.len() > 1
    }

    /// What it wraps says what it is; the menu adds whether it is open.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        (self.enabled && !self.entries.is_empty()).then(|| {
            frus_core::SemanticsProperties::new(frus_core::Role::None)
                .expanded(self.children.len() > 1)
        })
    }

    fn debug_name(&self) -> &'static str {
        "ContextMenu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Container, Runtime};
    use frus_core::Primitive;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Menu(ContextMenuState),
        Copy,
        Sort(usize),
    }

    const WINDOW: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);

    #[test]
    fn a_menu_opens_at_the_point_and_away_from_the_edges() {
        let size = Size::new(100.0, 80.0);
        let place = |x, y, rtl| place_at_point(Point::new(x, y), size, WINDOW, rtl);
        assert_eq!(place(50.0, 40.0, false), (50.0, 40.0));
        assert_eq!(
            place(50.0, 40.0, true),
            (50.0, 40.0),
            "no room on the left: right"
        );
        assert_eq!(
            place(350.0, 40.0, false),
            (250.0, 40.0),
            "no room on the right"
        );
        assert_eq!(
            place(350.0, 40.0, true),
            (250.0, 40.0),
            "right to left: towards the left"
        );
        assert_eq!(
            place(50.0, 280.0, false),
            (50.0, 200.0),
            "no room below: up"
        );
        assert_eq!(
            place(399.0, 299.0, false),
            (296.0, 216.0),
            "held off the edges"
        );
    }

    fn menu(state: &ContextMenuState) -> ContextMenu<Msg> {
        ContextMenu::new(
            Container::<Msg>::new().width(200.0).height(100.0),
            state,
            Msg::Menu,
        )
        .item(MenuItem::new("Copy", Msg::Copy).shortcut("Ctrl+C"))
        .divider()
        .submenu(
            SubmenuButton::new("Sort by")
                .item(MenuItem::radio("Name", true, Msg::Sort(0)))
                .item(MenuItem::radio("Date", false, Msg::Sort(1))),
        )
    }

    fn frame(widget: &ContextMenu<Msg>, theme: &Theme) -> crate::Ui<Msg> {
        build_ui(widget, Size::new(400.0, 300.0), &Runtime::default(), theme)
    }

    fn text_at(ui: &crate::Ui<Msg>, word: &str) -> Option<Point> {
        ui.scene().primitives().iter().find_map(|p| match p {
            Primitive::Text { text, position, .. } if text == word => Some(*position),
            _ => None,
        })
    }

    /// A right-click, or a long press, opens the menu where it landed.
    #[test]
    fn a_right_click_or_a_long_press_opens_it_at_the_pointer() {
        let closed = menu(&ContextMenuState::closed());
        let at = Point::new(120.0, 60.0);
        assert_eq!(
            Widget::<Msg>::on_secondary_tap_at(&closed, at),
            Some(Msg::Menu(ContextMenuState::open_at(at)))
        );
        assert_eq!(
            Widget::<Msg>::on_long_press_at(&closed, at),
            Some(Msg::Menu(ContextMenuState::open_at(at)))
        );
        assert!(Widget::<Msg>::overlay(&closed).is_none());
        let disabled = menu(&ContextMenuState::closed()).enabled(false);
        assert_eq!(Widget::<Msg>::on_secondary_tap_at(&disabled, at), None);
        // The shell finds it under the pointer.
        let ui = frame(&closed, &Theme::default());
        assert!(ui.hit(at).is_some(), "a target for the secondary tap");
        assert_eq!(ui.long_press_target_at(at), ui.hit(at));
    }

    /// Open, its rows hang from the point; a press outside, or Escape, closes it.
    #[test]
    fn open_it_hangs_from_the_point_and_closes_outside() {
        let at = Point::new(120.0, 60.0);
        let open = menu(&ContextMenuState::open_at(at));
        let ui = frame(&open, &Theme::default());
        let copy = text_at(&ui, "Copy").expect("the rows are drawn");
        assert!(copy.x > at.x && copy.x < at.x + 40.0, "{copy:?}");
        assert!(copy.y > at.y && copy.y < at.y + 60.0, "{copy:?}");
        assert!(text_at(&ui, "Ctrl+C").is_some(), "the keys on the right");
        assert_eq!(
            ui.top_dismiss(),
            Some(Msg::Menu(ContextMenuState::closed()))
        );
        assert_eq!(
            Widget::<Msg>::semantics(&open).and_then(|s| s.expanded),
            Some(true)
        );
        assert!(text_at(&ui, "Name").is_none(), "the submenu is shut");
    }

    /// The submenu opens beside its row, with its radio rows announced as radio buttons.
    #[test]
    fn a_submenu_opens_beside_with_radio_rows() {
        let at = Point::new(20.0, 20.0);
        let state = ContextMenuState::open_at(at);
        let deeper = ContextMenuState {
            path: state.path().opened(1, 2),
            ..state.clone()
        };
        let ui = frame(&menu(&deeper), &Theme::default());
        let sort = text_at(&ui, "Sort by").unwrap();
        let name = text_at(&ui, "Name").expect("the submenu is open");
        assert!(name.x > sort.x + 150.0, "beside: {sort:?} {name:?}");
        let radios: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::RadioButton)
            .map(|(_, _, s)| s.toggled)
            .collect();
        assert_eq!(radios.len(), 2);
        assert_ne!(radios[0], radios[1], "one chosen");
    }

    /// Right to left, the menu opens towards the left of the point, and its words are on the
    /// right of its rows, the keys on the left.
    #[test]
    fn right_to_left_it_is_mirrored() {
        let theme = Theme {
            direction: frus_core::TextDirection::Rtl,
            ..Theme::default()
        };
        let at = Point::new(300.0, 60.0);
        let ui = frame(&menu(&ContextMenuState::open_at(at)), &theme);
        let copy = text_at(&ui, "Copy").unwrap();
        let keys = text_at(&ui, "Ctrl+C").unwrap();
        assert!(copy.x < at.x, "towards the left: {copy:?}");
        assert!(keys.x < copy.x, "the keys on the left: {keys:?} {copy:?}");
    }

    /// The menu answers to its own style, then the theme's context menu.
    #[test]
    fn its_look_is_the_callers_then_the_themes() {
        let at = Point::new(20.0, 20.0);
        let surface = |ui: &crate::Ui<Msg>, colour: frus_core::Color| {
            ui.scene().primitives().iter().any(|p| match p {
                Primitive::Rect { color, .. } => *color == colour,
                _ => false,
            })
        };
        let mut theme = Theme::default();
        let themed = frus_core::Color::rgb(0.3, 0.5, 0.2);
        theme.widgets.context_menu.background = Some(themed);
        assert!(surface(
            &frame(&menu(&ContextMenuState::open_at(at)), &theme),
            themed
        ));
        let own = frus_core::Color::rgb(0.7, 0.2, 0.2);
        let styled = menu(&ContextMenuState::open_at(at)).menu_style(crate::MenuTheme {
            background: Some(own),
            ..Default::default()
        });
        let ui = frame(&styled, &theme);
        assert!(surface(&ui, own) && !surface(&ui, themed));
    }
}
