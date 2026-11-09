// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`FullScreenDialog`]: a task that takes the whole window — a close button, a title and
//! the action that confirms it (milestone 786).

use frus_core::{Color, Insets, TextStyle};
use frus_layout::{Align, Dimension, FlexDirection, Style};

use crate::button::{Button, Variant};
use crate::dialog::Dialog;
use crate::icons::Icons;
use crate::lazy::Lazy;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{Flex, IconButton, Text};

/// The height of a full-screen dialog's header, under the status bar.
pub const FULL_SCREEN_DIALOG_HEADER_HEIGHT: f32 = 56.0;

/// Defaults for [`FullScreenDialog`]. Each field unset is the scheme's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FullScreenDialogTheme {
    /// The surface. Unset, `surface`.
    pub background: Option<Color>,
    /// The header's surface. Unset, `surface`.
    pub header_background: Option<Color>,
    /// The title's type. Unset, `title_large`.
    pub title_style: Option<TextStyle>,
    /// The title's and the close button's colour. Unset, `on_surface`.
    pub foreground: Option<Color>,
    /// The confirming action's colour. Unset, `primary`.
    pub action_color: Option<Color>,
    /// The header's height. Unset, 56.
    pub header_height: Option<f32>,
}

struct Parts<Msg> {
    open: bool,
    title: Option<String>,
    on_close: Option<Msg>,
    action: Option<(String, Option<Msg>)>,
    content: Option<Box<dyn Widget<Msg>>>,
    body: Option<Box<dyn Widget<Msg>>>,
    background: Option<Color>,
    title_style: Option<TextStyle>,
}

/// **A task that takes the whole window**: a close button, a title, the action that
/// confirms the task, and the form or the content under them.
///
/// ```
/// # use frus_widgets::{FullScreenDialog, Text};
/// # #[derive(Clone)] enum Msg { Close, Save }
/// let open = true;
/// FullScreenDialog::new(open)
///     .title("New event")
///     .on_close(Msg::Close)
///     .action("Save", Msg::Save)
///     .child(Text::new("Where and when"))
///     .body(Text::new("The calendar"));
/// ```
///
/// The surface is `surface` and square, as large as the window; the header is 56 high,
/// under the status bar, with a cross at the start, the title in `title_large` and the
/// confirming action — a text button in `primary` — at the end. It is controlled like
/// every dialog here: `open` says whether it shows, and `on_close` is what the cross,
/// Escape and the system's Back send. The keyboard stays inside it while it is open.
/// A reader hears the title as a heading and the cross as "Close".
pub struct FullScreenDialog<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> FullScreenDialog<Msg> {
    /// A dialog; `open` says whether it shows.
    pub fn new(open: bool) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    open,
                    title: None,
                    on_close: None,
                    action: None,
                    content: None,
                    body: None,
                    background: None,
                    title_style: None,
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// The title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.p().title = Some(title.into());
        self
    }

    /// What the cross, Escape and the system's Back send.
    #[must_use]
    pub fn on_close(mut self, message: impl Into<Msg>) -> Self {
        self.p().on_close = Some(message.into());
        self
    }

    /// The confirming action at the end of the header: its words and what it sends.
    #[must_use]
    pub fn action(mut self, label: impl Into<String>, message: impl Into<Msg>) -> Self {
        self.p().action = Some((label.into(), Some(message.into())));
        self
    }

    /// The confirming action, shown but unavailable — a form not yet complete.
    #[must_use]
    pub fn disabled_action(mut self, label: impl Into<String>) -> Self {
        self.p().action = Some((label.into(), None));
        self
    }

    /// What the dialog holds, under the header.
    #[must_use]
    pub fn child(mut self, content: impl Widget<Msg> + 'static) -> Self {
        self.p().content = Some(Box::new(content));
        self
    }

    /// The page the dialog opens over.
    #[must_use]
    pub fn body(mut self, body: impl Widget<Msg> + 'static) -> Self {
        self.p().body = Some(Box::new(body));
        self
    }

    /// The surface, over the theme's and `surface`.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.p().background = Some(color);
        self
    }

    /// The title's type, over the theme's and `title_large`.
    #[must_use]
    pub fn title_style(mut self, style: TextStyle) -> Self {
        self.p().title_style = Some(style);
        self
    }
}

crate::composite::composite_widget!(FullScreenDialog);

fn build<Msg: Clone + 'static>(mut p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(
        ThemeBuilder::boxed(move |theme: &Theme| {
            let t = theme.widgets.full_screen_dialog;
            let background = p
                .background
                .or(t.background)
                .unwrap_or(theme.scheme.surface);
            let ink = t.foreground.unwrap_or(theme.scheme.on_surface);
            let title_style = p
                .title_style
                .or(t.title_style)
                .unwrap_or(theme.text.title_large);
            let media = crate::MediaQuery::of();
            let safe = media.safe();
            let header_height = t.header_height.unwrap_or(FULL_SCREEN_DIALOG_HEADER_HEIGHT);

            let mut header = crate::composite::Surface::new(
                Style {
                    flex_direction: FlexDirection::Row,
                    align: Align::Center,
                    height: Dimension::Length(header_height + safe.top),
                    padding: Insets::new(safe.top, 4.0 + safe.right, 0.0, 4.0 + safe.left),
                    gap: 4.0,
                    ..Default::default()
                },
                t.header_background.unwrap_or(background),
            );
            if let Some(close) = p.on_close.clone() {
                header = header.child(
                    IconButton::new(Icons::CLOSE)
                        .icon_color(ink)
                        .label(crate::localizations::of().close_button_label())
                        .on_press(close),
                );
            }
            header = header.child(crate::Expanded::new(crate::semantics::Semantics::heading(
                Text::styled(p.title.clone().unwrap_or_default(), title_style).color(ink),
            )));
            if let Some((label, message)) = p.action.take() {
                let mut action = Button::new(label)
                    .variant(Variant::Text)
                    .label_color(t.action_color.unwrap_or(theme.scheme.primary))
                    .label_style(theme.text.label_large);
                match message {
                    Some(message) => action = action.on_press(message),
                    None => action = action.enabled(false),
                }
                header = header.child(action);
            }

            let (width, height) = if media.is_described() {
                (
                    Dimension::Length(media.size.width),
                    Dimension::Length(media.size.height),
                )
            } else {
                (Dimension::Percent(1.0), Dimension::Percent(1.0))
            };
            let mut surface = crate::composite::Surface::new(
                Style {
                    flex_direction: FlexDirection::Column,
                    width,
                    height,
                    ..Default::default()
                },
                background,
            )
            .child(header);
            if let Some(content) = p.content.take() {
                surface = surface.child(
                    Flex::column()
                        .flex(1.0)
                        .padding_each(0.0, 24.0 + safe.right, safe.bottom, 24.0 + safe.left)
                        .child_boxed(content),
                );
            }

            let mut dialog = Dialog::new(p.open)
                .inset_padding(Insets::ZERO)
                .radius(0.0)
                .min_width(0.0)
                .elevation(0.0)
                .background(background)
                .child(surface);
            if let Some(close) = p.on_close.clone() {
                dialog = dialog.on_dismiss(close);
            }
            let body = p
                .body
                .take()
                .unwrap_or_else(|| Box::new(crate::SizedBox::empty()));
            Box::new(dialog.body(body)) as Box<dyn Widget<Msg>>
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
        Save,
    }

    /// **It takes the whole window**: the header along the top with the cross at the
    /// start and the action at the end, the title said as a heading, and Back closing it.
    #[test]
    fn it_takes_the_window_and_back_closes_it() {
        let dialog = FullScreenDialog::new(true)
            .title("New event")
            .on_close(Msg::Close)
            .action("Save", Msg::Save)
            .child(Text::new("Where"))
            .body(crate::Container::new().flex(1.0));
        let media = crate::MediaQuery {
            size: Size::new(400.0, 700.0),
            ..crate::MediaQuery::UNSET
        };
        let ui = media.scope(|| {
            build_ui(
                &dialog,
                Size::new(400.0, 700.0),
                &Runtime::default(),
                &Theme::default(),
            )
        });
        let find = |label: &str| {
            ui.semantics()
                .iter()
                .find(|(_, _, s)| s.label.as_deref() == Some(label))
                .cloned()
                .unwrap_or_else(|| panic!("no {label}: {:#?}", ui.semantics()))
        };
        let (_, close, _) = find("Close");
        let (_, title, heading) = find("New event");
        let (_, save, _) = find("Save");
        assert_eq!(heading.role, frus_core::Role::Heading);
        assert!(close.x < 60.0 && close.y < FULL_SCREEN_DIALOG_HEADER_HEIGHT);
        assert!(title.x > close.x + close.width - 1.0);
        assert!(
            save.x + save.width > 330.0,
            "the action at the end: {save:?}"
        );
        assert_eq!(ui.top_dismiss(), Some(Msg::Close));
        let at = frus_core::Point::new(save.x + save.width * 0.5, save.y + save.height * 0.5);
        assert_eq!(ui.hit(at).and_then(|id| ui.msg_for(id)), Some(Msg::Save));
        // A press on the surface itself is not a press on the page under it.
        let empty = frus_core::Point::new(200.0, 500.0);
        assert!(ui.hit(at).is_some());
        assert_eq!(ui.hit(empty).and_then(|id| ui.msg_for(id)), None);
    }
}
