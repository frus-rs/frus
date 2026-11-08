//! The pieces the sign-in, sign-up and reset pages share.

use frus::{
    Align, Column, Container, FontWeight, Justify, Row, Scaffold, SingleChildScrollView, TextAlign,
    Widget,
};

use crate::common::{
    colors::{black, gray, grey, red, surface},
    shared::app_logo,
    ui::{caption, gap, text, tracked, W},
};

/// A field's label, `14/20` medium, over the field, and its error under it once shown.
pub(crate) fn auth_field(label: &str, field: W, error: Option<String>) -> W {
    let mut column = Column::new()
        .spacing(8.0)
        .cross_axis_alignment(Align::Stretch)
        .child(text(label, 14.0, 20.0, black(), FontWeight::Medium))
        .child(field);
    if let Some(error) = error {
        column = column.child(text(error, 12.0, 16.0, red(), FontWeight::Regular));
    }
    Box::new(column)
}

/// The large centred title and the line under it.
pub(crate) fn auth_title(title: &str, subtitle: &str) -> W {
    Box::new(
        Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Center)
            .child(
                tracked(title, 30.0, 36.0, -0.75, black(), FontWeight::Bold)
                    .align(TextAlign::Center),
            )
            .child(
                text(subtitle, 14.0, 20.0, grey(), FontWeight::Regular).align(TextAlign::Center),
            ),
    )
}

/// "──── OR ────".
pub(crate) fn or_divider() -> W {
    let rule = || frus::Expanded::new(Container::new().height(1.0).color(gray()));
    Box::new(
        Row::new()
            .spacing(16.0)
            .cross_axis_alignment(Align::Center)
            .child(rule())
            .child(text("OR", 12.0, 16.0, grey(), FontWeight::Medium))
            .child(rule()),
    )
}

/// A signed-out page: the logo at the top, `content`, and the footer at the bottom.
pub(crate) fn auth_page(content: W, top_gap: f32) -> Box<dyn Widget> {
    let page = Column::new()
        .cross_axis_alignment(Align::Stretch)
        .child(
            Row::new()
                .main_axis_alignment(Justify::Start)
                .child(app_logo()),
        )
        .child(gap(top_gap))
        .child(content)
        .child(gap(110.0))
        .child(caption("Forge · Learn by building together").align(TextAlign::Center));
    Scaffold::new()
        .background(surface())
        .body(frus::SafeArea::new(
            SingleChildScrollView::new()
                .padding_each(0.0, 20.0, 24.0, 20.0)
                .child(page),
        ))
        .build()
}
