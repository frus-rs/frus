// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`EmptyState`]: what a screen shows when there is nothing to show yet (milestone 810).

use frus_core::{Color, TextAlign, TextStyle};
use frus_layout::{Align, Justify};

use crate::icons::IconData;
use crate::lazy::Lazy;
use crate::semantics::Semantics;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{Container, Flex, Icon, Text};

/// Defaults for [`EmptyState`]. Each field unset is the default named beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EmptyStateTheme {
    /// The icon's size. Unset, 32.
    pub icon_size: Option<f32>,
    /// The icon's colour. Unset, the scheme's `on_surface_variant`.
    pub icon_color: Option<Color>,
    /// The disc behind the icon. Unset, the scheme's `surface_container_high`;
    /// transparent for none.
    pub icon_background: Option<Color>,
    /// The disc's diameter. Unset, 64.
    pub icon_background_size: Option<f32>,
    /// The title's type. Unset, the type scale's `title_medium`.
    pub title_style: Option<TextStyle>,
    /// The title's colour. Unset, the scheme's `on_surface`.
    pub title_color: Option<Color>,
    /// The description's type. Unset, the type scale's `body_medium`.
    pub description_style: Option<TextStyle>,
    /// The description's colour. Unset, the scheme's `on_surface_variant`.
    pub description_color: Option<Color>,
    /// The space between the parts. Unset, 12.
    pub spacing: Option<f32>,
    /// The padding around the whole. Unset, 24.
    pub padding: Option<f32>,
    /// The widest the text runs. Unset, 360.
    pub max_width: Option<f32>,
}

enum Media<Msg> {
    None,
    Icon(IconData),
    Widget(Box<dyn Widget<Msg>>),
}

struct Parts<Msg> {
    media: Media<Msg>,
    title: String,
    description: Option<String>,
    actions: Vec<Box<dyn Widget<Msg>>>,
    theme: EmptyStateTheme,
}

/// **Nothing here yet**: an icon or an illustration, a title, a line saying why, and what
/// to do about it.
///
/// ```
/// use frus_widgets::{button, EmptyState, Icons};
///
/// #[derive(Clone)]
/// enum Msg { New }
///
/// let _empty: EmptyState<Msg> = EmptyState::new("No projects yet")
///     .icon(Icons::FOLDER_OPEN)
///     .description("Projects you create will show up here.")
///     .action(button("New project", Msg::New));
/// ```
///
/// Everything is centred, the text wraps within [`max_width`](EmptyState::max_width), and
/// the title is a heading a screen reader can jump to. The icon sits on a disc unless the
/// theme makes the disc transparent; [`illustration`](EmptyState::illustration) puts any
/// widget — an image, an SVG, an animation — in its place.
pub struct EmptyState<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> EmptyState<Msg> {
    /// An empty state saying `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    media: Media::None,
                    title: title.into(),
                    description: None,
                    actions: Vec::new(),
                    theme: EmptyStateTheme::default(),
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// An icon above the title, on a disc.
    #[must_use]
    pub fn icon(mut self, icon: IconData) -> Self {
        self.p().media = Media::Icon(icon);
        self
    }

    /// Any widget above the title in place of the icon — an image, an SVG, an animation.
    #[must_use]
    pub fn illustration(mut self, widget: impl Widget<Msg> + 'static) -> Self {
        self.p().media = Media::Widget(Box::new(widget));
        self
    }

    /// The line under the title saying why there is nothing, or what will appear.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.p().description = Some(description.into());
        self
    }

    /// Adds an action under the text — a button, usually; several sit side by side.
    #[must_use]
    pub fn action(mut self, action: impl Widget<Msg> + 'static) -> Self {
        self.p().actions.push(Box::new(action));
        self
    }

    /// Everything the theme can say, said for this one: each field set here wins over the
    /// theme's.
    #[must_use]
    pub fn style(mut self, style: EmptyStateTheme) -> Self {
        self.p().theme = style;
        self
    }

    /// The widest the text runs, over the theme's.
    #[must_use]
    pub fn max_width(mut self, width: f32) -> Self {
        self.p().theme.max_width = Some(width);
        self
    }
}

crate::composite::composite_widget!(EmptyState);

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    let p = std::rc::Rc::new(std::cell::RefCell::new(Some(p)));
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let Some(p) = p.borrow_mut().take() else {
            return Box::new(Flex::column()) as Box<dyn Widget<Msg>>;
        };
        let t = theme.widgets.empty_state;
        let o = p.theme;
        let scheme = &theme.scheme;
        let scale = crate::theme::type_scale(Some(theme));
        let pick = |own: Option<Color>, themed: Option<Color>, default: Color| {
            own.or(themed).unwrap_or(default)
        };
        let spacing = o.spacing.or(t.spacing).unwrap_or(12.0);
        let max_width = o.max_width.or(t.max_width).unwrap_or(360.0);
        let mut column = Flex::column()
            .gap(spacing)
            .padding(o.padding.or(t.padding).unwrap_or(24.0))
            .align(Align::Center)
            .justify(Justify::Center);
        match p.media {
            Media::None => {}
            Media::Icon(icon) => {
                let size = o.icon_size.or(t.icon_size).unwrap_or(32.0);
                let disc = o
                    .icon_background_size
                    .or(t.icon_background_size)
                    .unwrap_or(64.0);
                let glyph = Icon::new(icon).size(size).color(pick(
                    o.icon_color,
                    t.icon_color,
                    scheme.on_surface_variant,
                ));
                column = column.child(
                    Container::new()
                        .width(disc)
                        .height(disc)
                        .radius(disc / 2.0)
                        .color(pick(
                            o.icon_background,
                            t.icon_background,
                            scheme.surface_container_high,
                        ))
                        .alignment(frus_core::Alignment::CENTER)
                        .child(glyph),
                );
            }
            Media::Widget(widget) => column = column.child_boxed(widget),
        }
        let title_style = o
            .title_style
            .or(t.title_style)
            .unwrap_or(scale.title_medium)
            .color(pick(o.title_color, t.title_color, scheme.on_surface));
        column = column.child(
            Container::new()
                .max_width(max_width)
                .child(Semantics::heading(
                    Text::styled(p.title.clone(), title_style)
                        .wrap()
                        .align(TextAlign::Center),
                )),
        );
        if let Some(description) = &p.description {
            let style = o
                .description_style
                .or(t.description_style)
                .unwrap_or(scale.body_medium)
                .color(pick(
                    o.description_color,
                    t.description_color,
                    scheme.on_surface_variant,
                ));
            column = column.child(
                Container::new().max_width(max_width).child(
                    Text::styled(description.clone(), style)
                        .wrap()
                        .align(TextAlign::Center),
                ),
            );
        }
        if !p.actions.is_empty() {
            let mut row = Flex::row()
                .gap(8.0)
                .align(Align::Center)
                .justify(Justify::Center)
                .wrap();
            for action in p.actions {
                row = row.child_boxed(action);
            }
            column = column.child(Container::new().padding_each(4.0, 0.0, 0.0, 0.0).child(row));
        }
        Box::new(column) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, button, Icons, Runtime, Size};
    use frus_core::{Primitive, Role};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        New,
    }

    fn empty() -> EmptyState<Msg> {
        EmptyState::new("No projects yet")
            .icon(Icons::FOLDER_OPEN)
            .description("Projects you create will show up here.")
            .action(button("New project", Msg::New))
    }

    /// The title is a heading, the action a button, and the parts stack top to bottom in
    /// the middle of the room.
    #[test]
    fn an_empty_state_reads_and_lays_out() {
        let e = empty();
        let ui = build_ui(
            &e,
            Size::new(400.0, 600.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let nodes: Vec<_> = ui
            .semantics()
            .iter()
            .map(|(_, r, s)| (*r, s.clone()))
            .collect();
        let (title_rect, _) = nodes
            .iter()
            .find(|(_, s)| s.role == Role::Heading)
            .expect("a heading");
        assert!(nodes
            .iter()
            .any(|(_, s)| s.role == Role::Button && s.label.as_deref() == Some("New project")));
        let text_y = |words: &str| {
            ui.scene()
                .primitives()
                .iter()
                .find_map(|p| match p {
                    Primitive::Text { text, position, .. } if text.starts_with(words) => {
                        Some(position.y)
                    }
                    _ => None,
                })
                .expect(words)
        };
        assert!(text_y("No projects") < text_y("Projects you"));
        let centre = title_rect.x + title_rect.width / 2.0;
        assert!((centre - 200.0).abs() < 2.0, "centred: {centre}");
    }

    /// The theme reaches the disc and the title; the widget's own style wins over it.
    #[test]
    fn the_theme_and_the_style_colour_it() {
        let red = Color::rgb(1.0, 0.0, 0.0);
        let green = Color::rgb(0.0, 1.0, 0.0);
        let mut theme = Theme::default();
        theme.widgets.empty_state.icon_background = Some(red);
        theme.widgets.empty_state.title_color = Some(red);
        let e = empty();
        let ui = build_ui(&e, Size::new(400.0, 600.0), &Runtime::default(), &theme);
        let p = ui.scene().primitives();
        assert!(p
            .iter()
            .any(|p| matches!(p, Primitive::Rect { color, .. } if *color == red)));
        assert!(p.iter().any(
            |p| matches!(p, Primitive::Text { text, color, .. } if text == "No projects yet" && *color == red)
        ));
        let e = empty().style(EmptyStateTheme {
            title_color: Some(green),
            ..Default::default()
        });
        let ui = build_ui(&e, Size::new(400.0, 600.0), &Runtime::default(), &theme);
        assert!(ui.scene().primitives().iter().any(
            |p| matches!(p, Primitive::Text { text, color, .. } if text == "No projects yet" && *color == green)
        ));
    }
}
