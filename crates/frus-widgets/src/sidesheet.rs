// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`SideSheet`]: supplementary content along the end edge, standard (beside the page) or
//! modal (over it), with a heading, a close button and actions (milestone 786).

use frus_core::{Color, TextStyle};
use frus_layout::{Align, Dimension, FlexDirection, Justify, Style};

use crate::drawer::Drawer;
use crate::icons::Icons;
use crate::lazy::Lazy;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{Flex, IconButton, Text};

/// A side sheet's width.
pub const SIDE_SHEET_WIDTH: f32 = 256.0;
/// The corner of a modal side sheet's inner edge.
pub const SIDE_SHEET_RADIUS: f32 = 16.0;
/// How far off the page a modal side sheet sits.
pub const SIDE_SHEET_ELEVATION: f32 = 1.0;
/// The room at a side sheet's edges.
pub const SIDE_SHEET_PADDING: f32 = 24.0;

/// Defaults for [`SideSheet`]. Each field unset is the kind's own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SideSheetTheme {
    /// The width. Unset, 256.
    pub width: Option<f32>,
    /// A standard sheet's surface. Unset, `surface`.
    pub background: Option<Color>,
    /// A modal sheet's surface. Unset, `surface_container_low`.
    pub modal_background: Option<Color>,
    /// The inner edge's corners on a modal sheet. Unset, 16.
    pub radius: Option<f32>,
    /// A modal sheet's height off the page. Unset, one.
    pub elevation: Option<f32>,
    /// The heading's type. Unset, `title_large`.
    pub title_style: Option<TextStyle>,
    /// The heading's and the close button's colour. Unset, `on_surface_variant`.
    pub title_color: Option<Color>,
    /// The scrim behind a modal sheet, alpha included.
    pub scrim: Option<Color>,
    /// The hairline between a standard sheet and the page. Unset, `outline_variant`.
    pub divider_color: Option<Color>,
}

struct Parts<Msg> {
    open: bool,
    modal: bool,
    title: Option<String>,
    on_dismiss: Option<Msg>,
    on_back: Option<Msg>,
    content: Option<Box<dyn Widget<Msg>>>,
    actions: Vec<Box<dyn Widget<Msg>>>,
    body: Option<Box<dyn Widget<Msg>>>,
    width: Option<f32>,
    background: Option<Color>,
    radius: Option<f32>,
    elevation: Option<f32>,
    title_style: Option<TextStyle>,
    scrim: Option<Color>,
    drag: bool,
}

/// **Supplementary content along the end edge** — filters, details, the settings of what
/// the page shows.
///
/// ```
/// # use frus_widgets::{button, SideSheet, Text};
/// # #[derive(Clone)] enum Msg { Close, Apply }
/// let open = true;
/// SideSheet::<Msg>::modal(open)
///     .title("Filters")
///     .on_dismiss(Msg::Close)
///     .child(Text::new("Price, rating, distance"))
///     .action(button("Apply", Msg::Apply))
///     .body(Text::new("The page"));
/// ```
///
/// - **Standard** ([`SideSheet::standard`]): in the flow, beside the page, which gives up
///   the room; `surface`, flat, a hairline between them. For a wide window.
/// - **Modal** ([`SideSheet::modal`]): over the page behind a scrim, sliding in from the
///   end edge; `surface_container_low`, rounded on its inner edge, one off the page. A
///   press on the scrim, Escape, the system's Back, the close button or a drag towards
///   the edge dismiss it.
///
/// The end edge is the right one in a left-to-right language and the left one in a
/// right-to-left one; the corners and the drag follow it. A heading in `title_large`
/// and a close button sit at the top; actions, if any, along the bottom.
pub struct SideSheet<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> SideSheet<Msg> {
    fn of(open: bool, modal: bool) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    open,
                    modal,
                    title: None,
                    on_dismiss: None,
                    on_back: None,
                    content: None,
                    actions: Vec::new(),
                    body: None,
                    width: None,
                    background: None,
                    radius: None,
                    elevation: None,
                    title_style: None,
                    scrim: None,
                    drag: true,
                },
                build::<Msg>,
            ),
        }
    }

    /// A sheet **over** the page, behind a scrim; `open` says whether it shows.
    pub fn modal(open: bool) -> Self {
        Self::of(open, true)
    }

    /// A sheet **beside** the page, in the flow; `open` says whether it takes its room.
    pub fn standard(open: bool) -> Self {
        Self::of(open, false)
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// The heading.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.p().title = Some(title.into());
        self
    }

    /// What closes it: the close button, and on a modal sheet the scrim, Escape, the
    /// system's Back and a drag towards the edge. Without it there is no close button.
    #[must_use]
    pub fn on_dismiss(mut self, message: impl Into<Msg>) -> Self {
        self.p().on_dismiss = Some(message.into());
        self
    }

    /// A back arrow before the heading, for a sheet showing a second level of its content.
    #[must_use]
    pub fn on_back(mut self, message: impl Into<Msg>) -> Self {
        self.p().on_back = Some(message.into());
        self
    }

    /// What the sheet holds, under its heading.
    #[must_use]
    pub fn child(mut self, content: impl Widget<Msg> + 'static) -> Self {
        self.p().content = Some(Box::new(content));
        self
    }

    /// A button along the bottom.
    #[must_use]
    pub fn action(mut self, action: impl Widget<Msg> + 'static) -> Self {
        self.p().actions.push(Box::new(action));
        self
    }

    /// The page the sheet goes over or beside.
    #[must_use]
    pub fn body(mut self, body: impl Widget<Msg> + 'static) -> Self {
        self.p().body = Some(Box::new(body));
        self
    }

    /// The width. 256 by default.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.p().width = Some(width);
        self
    }

    /// The surface, over the theme's and the kind's.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.p().background = Some(color);
        self
    }

    /// A modal sheet's inner corners. 16 by default.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.p().radius = Some(radius);
        self
    }

    /// A modal sheet's height off the page. One by default.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.p().elevation = Some(elevation);
        self
    }

    /// The heading's type. `title_large` by default.
    #[must_use]
    pub fn title_style(mut self, style: TextStyle) -> Self {
        self.p().title_style = Some(style);
        self
    }

    /// The scrim behind a modal sheet, alpha included.
    #[must_use]
    pub fn scrim(mut self, color: Color) -> Self {
        self.p().scrim = Some(color);
        self
    }

    /// Whether a drag towards the edge dismisses a modal sheet. On by default.
    #[must_use]
    pub fn drag_to_dismiss(mut self, drag: bool) -> Self {
        self.p().drag = drag;
        self
    }
}

crate::composite::composite_widget!(SideSheet);

fn build<Msg: Clone + 'static>(mut p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(
        ThemeBuilder::boxed(move |theme: &Theme| {
            let t = theme.widgets.side_sheet;
            let width = p.width.or(t.width).unwrap_or(SIDE_SHEET_WIDTH);
            let ink = t.title_color.unwrap_or(theme.scheme.on_surface_variant);
            let title_style = p
                .title_style
                .or(t.title_style)
                .unwrap_or(theme.text.title_large);

            // The heading: back, title, close.
            let mut header = Flex::row().align(Align::Center).gap(4.0).padding_each(
                12.0,
                12.0,
                12.0,
                SIDE_SHEET_PADDING,
            );
            if let Some(back) = p.on_back.clone() {
                header = header.child(
                    IconButton::new(Icons::ARROW_BACK)
                        .icon_color(ink)
                        .label(crate::localizations::of().back_button_label())
                        .on_press(back),
                );
            }
            let title = p.title.clone().unwrap_or_default();
            header = header.child(crate::Expanded::new(crate::semantics::Semantics::heading(
                Text::styled(title, title_style).color(ink),
            )));
            if let Some(dismiss) = p.on_dismiss.clone() {
                header = header.child(
                    IconButton::new(Icons::CLOSE)
                        .icon_color(ink)
                        .label(crate::localizations::of().close_button_label())
                        .on_press(dismiss),
                );
            }

            let mut column = Flex::column().flex(1.0).child(header);
            if let Some(content) = p.content.take() {
                column = column.child(
                    Flex::column()
                        .flex(1.0)
                        .padding_each(0.0, SIDE_SHEET_PADDING, 0.0, SIDE_SHEET_PADDING)
                        .child_boxed(content),
                );
            }
            if !p.actions.is_empty() {
                let mut row = Flex::row()
                    .gap(8.0)
                    .align(Align::Center)
                    .justify(Justify::Start)
                    .padding_each(16.0, SIDE_SHEET_PADDING, 24.0, SIDE_SHEET_PADDING);
                for action in p.actions.drain(..) {
                    row = row.child_boxed(action);
                }
                column = column.child(crate::Divider::new()).child(row);
            }

            let body = p
                .body
                .take()
                .unwrap_or_else(|| Box::new(crate::SizedBox::empty()));
            if p.modal {
                let panel = crate::composite::Surface::new(
                    Style {
                        flex_direction: FlexDirection::Column,
                        width: Dimension::Percent(1.0),
                        flex_grow: 1.0,
                        ..Default::default()
                    },
                    Color::TRANSPARENT,
                )
                .child(column);
                let mut drawer = Drawer::new(p.open)
                    .right()
                    .width(width)
                    .background_color(
                        p.background
                            .or(t.modal_background)
                            .unwrap_or(theme.scheme.surface_container_low),
                    )
                    .border_width(0.0)
                    .radius(p.radius.or(t.radius).unwrap_or(SIDE_SHEET_RADIUS))
                    .elevation(p.elevation.or(t.elevation).unwrap_or(SIDE_SHEET_ELEVATION))
                    .shadow_color(theme.scheme.shadow.with_alpha(0.3))
                    .drag_to_dismiss(p.drag);
                if let Some(scrim) = p.scrim.or(t.scrim) {
                    drawer = drawer.scrim_color(scrim);
                }
                if let Some(dismiss) = p.on_dismiss.clone() {
                    drawer = drawer.on_dismiss(dismiss);
                }
                Box::new(drawer.panel(panel).body(body)) as Box<dyn Widget<Msg>>
            } else {
                // Standard: beside the page, in the flow, when open; the page alone when not.
                // The page gives up the room the sheet takes, whatever its content would like.
                let page = crate::Container::new()
                    .flex(1.0)
                    .shrink(1.0)
                    .child(Flex::column().flex(1.0).child_boxed(body));
                if !p.open {
                    return Box::new(
                        crate::composite::Surface::new(
                            Style {
                                flex_direction: FlexDirection::Row,
                                flex_grow: 1.0,
                                ..Default::default()
                            },
                            Color::TRANSPARENT,
                        )
                        .fill(crate::widget::FillAxes::BOTH)
                        .child(page),
                    ) as Box<dyn Widget<Msg>>;
                }
                let divider = t.divider_color.unwrap_or(theme.scheme.outline_variant);
                let sheet = crate::composite::Surface::new(
                    Style {
                        flex_direction: FlexDirection::Column,
                        width: Dimension::Length(width),
                        height: Dimension::Percent(1.0),
                        ..Default::default()
                    },
                    p.background
                        .or(t.background)
                        .unwrap_or(theme.scheme.surface),
                )
                .child(column);
                Box::new(
                    crate::composite::Surface::new(
                        Style {
                            flex_direction: FlexDirection::Row,
                            flex_grow: 1.0,
                            ..Default::default()
                        },
                        Color::TRANSPARENT,
                    )
                    .fill(crate::widget::FillAxes::BOTH)
                    .child(page)
                    .child(crate::VerticalDivider::new().color(divider).width(1.0))
                    .child(sheet),
                )
            }
        })
        .filling(crate::widget::FillAxes::BOTH),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Close,
        Apply,
    }

    fn sheet(open: bool) -> SideSheet<Msg> {
        SideSheet::modal(open)
            .title("Filters")
            .on_dismiss(Msg::Close)
            .child(Text::new("Price"))
            .action(crate::button("Apply", Msg::Apply))
            .body(crate::Container::new().flex(1.0))
    }

    fn frame(sheet: &SideSheet<Msg>, theme: &Theme) -> crate::Ui<Msg> {
        build_ui(sheet, Size::new(800.0, 600.0), &Runtime::default(), theme)
    }

    fn rect_of(ui: &crate::Ui<Msg>, label: &str) -> frus_core::Rect {
        ui.semantics()
            .iter()
            .find(|(_, _, s)| s.label.as_deref() == Some(label))
            .map(|(_, r, _)| *r)
            .unwrap_or_else(|| panic!("no {label}: {:#?}", ui.semantics()))
    }

    /// **A modal sheet comes in at the end edge** — the right in English, the left in
    /// Arabic — 256 wide, with its heading and its close button; Back closes it.
    #[test]
    fn a_modal_sheet_sits_at_the_end_edge_and_back_closes_it() {
        let theme = Theme::default();
        let ui = frame(&sheet(true), &theme);
        let title = rect_of(&ui, "Filters");
        assert!(
            title.x > 800.0 - SIDE_SHEET_WIDTH,
            "on the right: {title:?}"
        );
        let close = rect_of(&ui, "Close");
        assert!(close.x + close.width <= 800.0 && close.x > title.x);
        assert_eq!(ui.top_dismiss(), Some(Msg::Close));

        let rtl = Theme::default().rtl();
        let ui = frame(&sheet(true), &rtl);
        let title = rect_of(&ui, "Filters");
        assert!(
            title.x + title.width < SIDE_SHEET_WIDTH,
            "on the left: {title:?}"
        );

        let shut = frame(&sheet(false), &theme);
        assert_eq!(
            shut.top_dismiss(),
            None,
            "closed, nothing for Back to close"
        );
    }

    /// **A standard sheet takes its room beside the page** and has no scrim to close.
    #[test]
    fn a_standard_sheet_shares_the_row() {
        let theme = Theme::default();
        let standard = SideSheet::standard(true)
            .title("Details")
            .on_dismiss(Msg::Close)
            .body(crate::Container::new().flex(1.0));
        let ui = build_ui(
            &standard,
            Size::new(800.0, 600.0),
            &Runtime::default(),
            &theme,
        );
        let title = rect_of(&ui, "Details");
        assert!(title.x > 800.0 - SIDE_SHEET_WIDTH - 1.0);
        assert_eq!(ui.top_dismiss(), None);
    }

    /// **A drag towards the edge takes the open sheet with it**, and let go past half way
    /// it is dismissed.
    #[test]
    fn a_drag_towards_the_edge_dismisses() {
        let drawer: Drawer<Msg> = Drawer::new(true)
            .right()
            .width(200.0)
            .drag_to_dismiss(true)
            .on_dismiss(Msg::Close);
        assert!(Widget::<Msg>::pan_axis(&drawer).is_some());
        let update = |dx: f32| crate::PanEvent::Update {
            local: frus_core::Point::new(0.0, 0.0),
            delta: frus_core::Point::new(dx, 0.0),
        };
        let moved = Widget::<Msg>::pan_value(&drawer, update(100.0), 1.0, false).unwrap();
        assert!(
            (moved - 0.5).abs() < 1e-4,
            "half its width to the right: {moved}"
        );
        let rtl = Widget::<Msg>::pan_value(&drawer, update(-100.0), 1.0, true).unwrap();
        assert!((rtl - 0.5).abs() < 1e-4, "leftwards in RTL: {rtl}");
        assert_eq!(
            Widget::<Msg>::on_value_release(&drawer, 0.4),
            Some(Msg::Close)
        );
        assert_eq!(Widget::<Msg>::on_value_release(&drawer, 0.8), None);
        let shut: Drawer<Msg> = Drawer::new(false)
            .right()
            .drag_to_dismiss(true)
            .on_dismiss(Msg::Close);
        assert!(
            Widget::<Msg>::pan_axis(&shut).is_none(),
            "closed takes no drag"
        );
    }
}
