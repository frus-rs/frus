// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Sidebar`]: side navigation that **folds into a rail of icons** and opens again — the
//! left column of an administration screen, a mail client, a dashboard (milestone 811).
//!
//! Open, it is a column of labelled items in groups; folded, a narrow rail of their icons,
//! each telling its name in a tooltip. The width moves between the two over the theme's
//! duration. The item in use wears the indicator.
//!
//! Given the page it sits beside ([`Sidebar::body`]), it also decides how to be shown: on
//! a wide window it stands at the start of the row and the page takes the rest; below a
//! breakpoint it becomes a **drawer** — hidden while folded, and when opened, sliding in
//! over the page with a scrim that closes it, as do Back and Escape.
//!
//! It is controlled: the application keeps which item is in use and whether the sidebar
//! is open, and is told when either should change.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use frus_core::{Alignment, Color, Rect, Role, Scene, SemanticsProperties, TextStyle};
use frus_layout::{Align, Dimension, Style};

use crate::constraints::AxisConstraint;
use crate::flex::Flex;
use crate::icons::{IconData, Icons};
use crate::interaction::Status;
use crate::text::Text;
use crate::theme::Theme;
use crate::widget::Widget;

/// The sidebar's default width open.
pub const SIDEBAR_EXPANDED_WIDTH: f32 = 256.0;
/// The sidebar's default width folded into a rail.
pub const SIDEBAR_COLLAPSED_WIDTH: f32 = 72.0;
/// The window width under which a sidebar with a body becomes a drawer.
pub const SIDEBAR_BREAKPOINT: f32 = 600.0;

/// Theme defaults for [`Sidebar`] (`theme.widgets.sidebar`); each `None` falls through to
/// the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SidebarTheme {
    /// The panel's surface. Unset, `surface_container_low`.
    pub background: Option<Color>,
    /// The item in use's indicator. Unset, `secondary_container`.
    pub indicator_color: Option<Color>,
    /// The item in use's icon and label. Unset, `on_secondary_container`.
    pub selected_color: Option<Color>,
    /// Every other item's icon and label. Unset, `on_surface_variant`.
    pub unselected_color: Option<Color>,
    /// The items' labels. Unset, `label_large`.
    pub label_style: Option<TextStyle>,
    /// The groups' titles. Unset, `title_small`.
    pub group_style: Option<TextStyle>,
    /// The width open. Unset, 256.
    pub expanded_width: Option<f32>,
    /// The width folded. Unset, 72.
    pub collapsed_width: Option<f32>,
    /// An item's height. Unset, 44.
    pub item_height: Option<f32>,
    /// The window width under which it becomes a drawer. Unset, 600.
    pub breakpoint: Option<f32>,
    /// The scrim over the page while it is open as a drawer. Unset, `scrim` at 32 %.
    pub scrim_color: Option<Color>,
}

/// One destination of a [`Sidebar`].
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarItem {
    icon: IconData,
    selected_icon: Option<IconData>,
    label: String,
}

impl SidebarItem {
    /// A destination drawn as `icon`, called `label`.
    pub fn new(icon: IconData, label: impl Into<String>) -> Self {
        Self {
            icon,
            selected_icon: None,
            label: label.into(),
        }
    }

    /// The icon while it is in use (a filled form, usually).
    pub fn selected_icon(mut self, icon: IconData) -> Self {
        self.selected_icon = Some(icon);
        self
    }
}

/// An item, or the title of the group that follows.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Group(String),
    Item(SidebarItem),
}

/// What the sidebar was told, taken once when it is built.
struct Parts<Msg> {
    header: Option<Box<dyn Widget<Msg>>>,
    footer: Option<Box<dyn Widget<Msg>>>,
    body: Option<Box<dyn Widget<Msg>>>,
}

/// **Side navigation that folds into a rail of icons.**
///
/// ```
/// use frus_widgets::{Icons, Sidebar, Text};
///
/// #[derive(Clone)]
/// enum Msg { Go(usize), Toggle }
///
/// let open = true;
/// let _shell = Sidebar::new(0, Msg::Go)
///     .group("Workspace")
///     .item(Icons::DASHBOARD, "Overview")
///     .item(Icons::INBOX, "Inbox")
///     .group("Settings")
///     .item(Icons::SETTINGS, "Preferences")
///     .expanded(open)
///     .on_toggle(Msg::Toggle)
///     .body(Text::new("The page"));
/// ```
///
/// Items are numbered in the order they are added, groups aside: `on_select(2)` above is
/// "Preferences". **Themed** through [`SidebarTheme`] (`theme.widgets.sidebar`).
pub struct Sidebar<Msg = crate::callback::Callback> {
    selected: usize,
    on_select: Rc<dyn Fn(usize) -> Msg>,
    entries: Vec<Entry>,
    expanded: bool,
    on_toggle: Option<Msg>,
    parts: RefCell<Option<Parts<Msg>>>,
    has_body: bool,
    breakpoint: Option<f32>,
    /// Whether it was built as a drawer, for `back_dismiss`.
    drawer: std::cell::Cell<bool>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> Sidebar<Msg> {
    /// A sidebar with item `selected` in use, `on_select(i)` when another is chosen.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        selected: usize,
        on_select: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self {
            selected,
            on_select: Rc::new(crate::callback::handler1(on_select)),
            entries: Vec::new(),
            expanded: true,
            on_toggle: None,
            parts: RefCell::new(Some(Parts {
                header: None,
                footer: None,
                body: None,
            })),
            has_body: false,
            breakpoint: None,
            drawer: std::cell::Cell::new(false),
            built: OnceCell::new(),
        }
    }

    /// Starts a group titled `title`: the items after it belong to it.
    pub fn group(mut self, title: impl Into<String>) -> Self {
        self.entries.push(Entry::Group(title.into()));
        self
    }

    /// Adds an item.
    pub fn item(self, icon: IconData, label: impl Into<String>) -> Self {
        self.destination(SidebarItem::new(icon, label))
    }

    /// Adds an item built with [`SidebarItem`].
    pub fn destination(mut self, item: SidebarItem) -> Self {
        self.entries.push(Entry::Item(item));
        self
    }

    /// Open (`true`, the default) or folded into a rail — or, as a drawer, shown or hidden.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// The message the menu button sends to open or fold it; without one there is no menu
    /// button, and a drawer's scrim, Back and Escape do nothing.
    pub fn on_toggle(mut self, message: impl Into<Msg>) -> Self {
        self.on_toggle = Some(message.into());
        self
    }

    /// Something at the top, under the menu button: a logo, an account.
    pub fn header(mut self, header: impl Widget<Msg> + 'static) -> Self {
        if let Some(parts) = self.parts.get_mut() {
            parts.header = Some(Box::new(header));
        }
        self
    }

    /// Something at the bottom: an account, a sign-out.
    pub fn footer(mut self, footer: impl Widget<Msg> + 'static) -> Self {
        if let Some(parts) = self.parts.get_mut() {
            parts.footer = Some(Box::new(footer));
        }
        self
    }

    /// The page beside it. With one, the sidebar fills its parent, gives the page the rest
    /// of the row, and becomes a drawer on a narrow window.
    pub fn body(mut self, body: impl Widget<Msg> + 'static) -> Self {
        if let Some(parts) = self.parts.get_mut() {
            parts.body = Some(Box::new(body));
        }
        self.has_body = true;
        self
    }

    /// The window width under which it becomes a drawer, over the theme's.
    pub fn breakpoint(mut self, width: f32) -> Self {
        self.breakpoint = Some(width);
        self
    }

    /// Whether, under `theme` and on the window in force, it is a drawer.
    fn is_drawer(&self, theme: &Theme) -> bool {
        let breakpoint = self
            .breakpoint
            .or(theme.widgets.sidebar.breakpoint)
            .unwrap_or(SIDEBAR_BREAKPOINT);
        self.has_body && crate::MediaQuery::of().size.width < breakpoint
    }

    /// The panel's column at width `width`, open or folded.
    fn panel(&self, theme: &Theme, open: bool, width: f32, parts: &mut Parts<Msg>) -> Flex<Msg> {
        let t = theme.widgets.sidebar;
        let s = &theme.scheme;
        let l10n = crate::localizations::of();
        let mut col = Flex::column()
            .width(width)
            .gap(2.0)
            .padding_each(8.0, 0.0, 8.0, 0.0)
            .align(Align::Stretch);
        if let Some(toggle) = self.on_toggle.clone() {
            let button = crate::IconButton::new(if open { Icons::MENU_OPEN } else { Icons::MENU })
                .label(if open {
                    l10n.close_button_label()
                } else {
                    l10n.open_drawer_label()
                })
                .on_press(toggle);
            col = col.child(Flex::row().padding_each(0.0, 0.0, 8.0, 12.0).child(button));
        }
        if let Some(header) = parts.header.take() {
            col = col.child(
                crate::Container::new()
                    .padding_each(0.0, 12.0, 8.0, 12.0)
                    .child(header),
            );
        }
        let item_height = t.item_height.unwrap_or(44.0);
        let mut index = 0;
        for (n, entry) in self.entries.iter().enumerate() {
            match entry {
                Entry::Group(title) => {
                    if open {
                        col = col.child(
                            crate::Container::new()
                                .padding_each(if n == 0 { 4.0 } else { 16.0 }, 16.0, 4.0, 16.0)
                                .child(
                                    Text::styled(
                                        title.clone(),
                                        t.group_style.unwrap_or(theme.text.title_small),
                                    )
                                    .color(s.on_surface_variant)
                                    .no_wrap()
                                    .ellipsis(),
                                ),
                        );
                    } else if n > 0 {
                        col = col.child(crate::Divider::new().indent(16.0).end_indent(16.0));
                    }
                }
                Entry::Item(item) => {
                    let selected = self.selected == index;
                    let fg = if selected {
                        t.selected_color.unwrap_or(s.on_secondary_container)
                    } else {
                        t.unselected_color.unwrap_or(s.on_surface_variant)
                    };
                    let icon = crate::Icon::new(if selected {
                        item.selected_icon.unwrap_or(item.icon)
                    } else {
                        item.icon
                    })
                    .size(24.0)
                    .color(fg);
                    let mut row = Flex::row()
                        .align(Align::Center)
                        .gap(12.0)
                        .height(item_height)
                        .padding_each(0.0, 12.0, 0.0, 12.0)
                        .child(icon);
                    if open {
                        row = row.child(crate::Expanded::new(
                            Text::styled(
                                item.label.clone(),
                                t.label_style.unwrap_or(theme.text.label_large),
                            )
                            .color(fg)
                            .no_wrap()
                            .ellipsis(),
                        ));
                    }
                    let entry = SidebarEntry {
                        label: item.label.clone(),
                        selected,
                        message: (self.on_select)(index),
                        indicator: t.indicator_color.unwrap_or(s.secondary_container),
                        children: vec![Box::new(crate::ExcludeSemantics::new(row))],
                    };
                    let entry: Box<dyn Widget<Msg>> = if open {
                        Box::new(entry)
                    } else {
                        // Folded, the name is in a tooltip.
                        Box::new(crate::Tooltip::new(item.label.clone()).child(entry))
                    };
                    // A column, so the item stretches across the panel.
                    col = col.child(
                        Flex::column()
                            .align(Align::Stretch)
                            .padding_each(0.0, 12.0, 0.0, 12.0)
                            .child(entry),
                    );
                    index += 1;
                }
            }
        }
        if let Some(footer) = parts.footer.take() {
            col = col.child(crate::Spacer::new()).child(
                crate::Container::new()
                    .padding_each(8.0, 12.0, 0.0, 12.0)
                    .child(footer),
            );
        }
        col
    }

    /// The whole sidebar, built under `theme`.
    fn build(&self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let mut parts = self.parts.borrow_mut().take().unwrap_or(Parts {
            header: None,
            footer: None,
            body: None,
        });
        let t = theme.widgets.sidebar;
        let s = &theme.scheme;
        let background = t.background.unwrap_or(s.surface_container_low);
        let expanded_width = t.expanded_width.unwrap_or(SIDEBAR_EXPANDED_WIDTH);
        let collapsed_width = t.collapsed_width.unwrap_or(SIDEBAR_COLLAPSED_WIDTH);
        let duration = if crate::MediaQuery::reduce_motion() {
            0.0
        } else {
            theme.motion.medium2
        };
        let curve = theme.motion.emphasized_decelerate.clone();
        let drawer = self.is_drawer(theme);
        self.drawer.set(drawer);

        if drawer {
            // Below the breakpoint: the page fills the room, and the panel, open, slides
            // in over it at its full width with a scrim behind it.
            let open = self.expanded;
            let panel = self.panel(theme, true, expanded_width, &mut parts);
            let panel = crate::Container::new()
                .color(background)
                .shadow(0.0, 4.0, 16.0, Color::rgba(0.0, 0.0, 0.0, 0.25))
                .child(panel);
            let slid = crate::AnimatedSlide::new(
                if open { 0.0 } else { -1.05 },
                0.0,
                duration,
                curve.clone(),
                panel,
            );
            // Closed, it is out of reach: no touch, no focus, nothing read.
            let slid =
                crate::ExcludeSemantics::new(crate::IgnorePointer::new(slid).ignoring(!open))
                    .excluding(!open);
            let slid: Box<dyn Widget<Msg>> = if open {
                Box::new(slid)
            } else {
                Box::new(crate::ExcludeFocus::new(slid))
            };
            let scrim_color = t.scrim_color.unwrap_or(s.scrim.fade(0.32));
            let mut scrim = crate::GestureDetector::new(crate::Container::new().color(scrim_color));
            if let Some(toggle) = self.on_toggle.clone() {
                scrim = scrim.on_tap(toggle);
            }
            let scrim = crate::IgnorePointer::new(crate::AnimatedOpacity::new(
                if open { 1.0 } else { 0.0 },
                duration,
                curve,
                crate::ExcludeSemantics::new(scrim),
            ))
            .ignoring(!open);
            let mut stack = crate::Stack::new().flex(1.0);
            if let Some(body) = parts.body.take() {
                stack = stack.layer(crate::Positioned::fill(body));
            }
            stack = stack.layer(crate::Positioned::fill(scrim)).layer(
                crate::Positioned::new(slid)
                    .top(0.0)
                    .bottom(0.0)
                    .start(0.0)
                    .width(expanded_width),
            );
            return Box::new(stack);
        }

        let open = self.expanded;
        let width = if open {
            expanded_width
        } else {
            collapsed_width
        };
        let panel = self.panel(theme, open, width, &mut parts);
        let rail = crate::AnimatedSize::new(
            duration,
            crate::Container::new()
                .color(background)
                .alignment(Alignment::TOP_LEFT)
                .child(panel),
        )
        .curve(theme.motion.emphasized_decelerate.clone())
        .alignment(Alignment::TOP_LEFT)
        .height(AxisConstraint::AsGiven);
        let nav = rail;
        match parts.body.take() {
            Some(body) => Box::new(
                Flex::row()
                    .flex(1.0)
                    .align(Align::Stretch)
                    .child(nav)
                    .child(crate::Expanded::new(body)),
            ),
            None => Box::new(nav),
        }
    }
}

/// One item: the target of a press, a focus stop, a tab to a screen reader, and the
/// indicator behind its icon and label.
struct SidebarEntry<Msg> {
    label: String,
    selected: bool,
    message: Msg,
    indicator: Color,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for SidebarEntry<Msg> {
    /// A column: its row stretches to the item's full width.
    fn style(&self) -> Style {
        Style {
            flex_direction: frus_layout::FlexDirection::Column,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let radius = bounds.height * 0.5;
        if self.selected {
            scene.draw_rect(
                bounds,
                self.indicator.fade(status.opacity),
                radius,
                0.0,
                Color::TRANSPARENT,
            );
        }
        if status.hover_progress > 0.0 || status.press_progress > 0.0 || status.focus_progress > 0.0
        {
            let base = if self.selected {
                self.indicator
            } else {
                Color::TRANSPARENT
            };
            let layer = theme.state_layer(base, theme.scheme.on_surface, &status);
            scene.draw_rect(
                bounds,
                layer.fade(status.opacity),
                radius,
                0.0,
                Color::TRANSPARENT,
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        Some(self.message.clone())
    }

    fn focusable(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        Some(
            SemanticsProperties::new(Role::Tab)
                .label(self.label.clone())
                .toggled(self.selected)
                .clickable(),
        )
    }

    fn debug_name(&self) -> &'static str {
        "SidebarItem"
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for Sidebar<Msg> {
    fn style(&self) -> Style {
        if self.has_body {
            Style {
                width: Dimension::Percent(1.0),
                height: Dimension::Percent(1.0),
                align: Align::Stretch,
                ..Default::default()
            }
        } else {
            Style::default()
        }
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

    /// Open as a drawer, Back and Escape close it.
    fn back_dismiss(&self) -> Option<Msg> {
        if self.drawer.get() && self.expanded {
            self.on_toggle.clone()
        } else {
            None
        }
    }

    fn debug_name(&self) -> &'static str {
        "Sidebar"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, find_widget, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Go(usize),
        Toggle,
    }

    fn sidebar(open: bool) -> Sidebar<Msg> {
        Sidebar::new(1, Msg::Go)
            .group("Workspace")
            .item(Icons::DASHBOARD, "Overview")
            .item(Icons::INBOX, "Inbox")
            .group("Settings")
            .item(Icons::SETTINGS, "Preferences")
            .expanded(open)
            .on_toggle(Msg::Toggle)
            .body(Text::new("Page"))
    }

    fn frame(s: &Sidebar<Msg>, width: f32) -> crate::Ui<Msg> {
        let surface = crate::MediaQuery::new(Size::new(width, 600.0));
        let _guard = surface.install();
        build_ui(
            s,
            Size::new(width, 600.0),
            &Runtime::default(),
            &Theme::light(),
        )
    }

    fn item<'a>(
        ui: &'a crate::Ui<Msg>,
        name: &str,
    ) -> &'a (crate::WidgetId, Rect, SemanticsProperties) {
        ui.semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some(name) && s.role == Role::Tab)
            .unwrap_or_else(|| panic!("{name}: {:?}", ui.semantics()))
    }

    #[test]
    fn open_it_is_wide_folded_a_rail() {
        let open = frame(&sidebar(true), 1000.0);
        let folded = frame(&sidebar(false), 1000.0);
        let wide = item(&open, "Inbox").1.width;
        let narrow = item(&folded, "Inbox").1.width;
        assert!(wide > 200.0, "open: {wide}");
        assert!(narrow < 72.0, "folded: {narrow}");
        // The page takes the rest of the row.
        let page_x = |ui: &crate::Ui<Msg>| {
            ui.scene()
                .primitives()
                .iter()
                .find_map(|p| match p {
                    frus_core::Primitive::Text { text, position, .. } if text == "Page" => {
                        Some(position.x)
                    }
                    _ => None,
                })
                .expect("the page")
        };
        assert_eq!(page_x(&open), SIDEBAR_EXPANDED_WIDTH);
        assert_eq!(page_x(&folded), SIDEBAR_COLLAPSED_WIDTH);
    }

    #[test]
    fn items_select_and_read_as_tabs() {
        let ui = frame(&sidebar(true), 1000.0);
        let s = sidebar(true);
        let ui2 = frame(&s, 1000.0);
        let (id, _, props) = item(&ui2, "Preferences");
        assert_eq!(props.toggled, frus_core::Toggled::False);
        assert_eq!(ui2.msg_for(*id), Some(Msg::Go(2)), "groups are not counted");
        let inbox = item(&ui, "Inbox");
        assert_eq!(inbox.2.toggled, frus_core::Toggled::True, "the item in use");
        let _ = find_widget(&s, *id).expect("the widget");
    }

    #[test]
    fn folded_items_tell_their_name_in_a_tooltip() {
        let s = sidebar(false);
        let ui = frame(&s, 1000.0);
        let (id, _, _) = item(&ui, "Inbox");
        // The tooltip wraps the item: the item's parent says the name.
        assert!(ui
            .semantics()
            .iter()
            .any(|(_, _, p)| p.label.as_deref() == Some("Inbox")));
        let words: Vec<String> = ui
            .scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                frus_core::Primitive::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !words.iter().any(|w| w == "Inbox"),
            "no label drawn: {words:?}"
        );
        assert!(find_widget(&s, *id).is_some());
    }

    #[test]
    fn narrow_it_is_a_drawer() {
        // Closed: nothing of it to touch or read, and the page has the whole width.
        let closed = sidebar(false);
        let ui = frame(&closed, 400.0);
        assert!(
            !ui.semantics().iter().any(|(_, _, s)| s.role == Role::Tab),
            "a closed drawer is not read"
        );
        assert_eq!(Widget::<Msg>::back_dismiss(&closed), None);
        // Open: its items are there, and Back closes it.
        let open = sidebar(true);
        let ui = frame(&open, 400.0);
        assert!(item(&ui, "Inbox").1.width > 200.0);
        assert_eq!(Widget::<Msg>::back_dismiss(&open), Some(Msg::Toggle));
        // A wide window never makes it a drawer.
        let wide = sidebar(true);
        let _ = frame(&wide, 1000.0);
        assert_eq!(Widget::<Msg>::back_dismiss(&wide), None);
    }

    #[test]
    fn the_theme_sets_the_widths() {
        let mut theme = Theme::light();
        theme.widgets.sidebar.expanded_width = Some(300.0);
        let surface = crate::MediaQuery::new(Size::new(1000.0, 600.0));
        let _guard = surface.install();
        let ui = build_ui(
            &sidebar(true),
            Size::new(1000.0, 600.0),
            &Runtime::default(),
            &theme,
        );
        assert!(item(&ui, "Inbox").1.width > 250.0);
    }
}
