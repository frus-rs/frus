//! The rows of the settings, account and workspace lists.

use frus::{Align, Callback, Column, Container, Expanded, FontWeight, Row, Widget};

use crate::common::{
    colors::{black, grey},
    icons::{icon, Glyph},
    ui::{tap, text},
};

/// The sizes a list's rows use: its title and subtitle type, its chevron, its padding.
#[derive(Clone, Copy)]
pub(crate) struct RowStyle {
    pub title: (f32, f32),
    pub subtitle: (f32, f32),
    pub chevron: f32,
    pub pad: (f32, f32),
    pub gap: f32,
}

impl RowStyle {
    /// Account rows: `14/20` over `12/16`, a 16 chevron.
    pub(crate) const ACCOUNT: RowStyle = RowStyle {
        title: (14.0, 20.0),
        subtitle: (12.0, 16.0),
        chevron: 16.0,
        pad: (16.0, 16.0),
        gap: 4.0,
    };

    /// Workspace rows: `16/24` over `14/20`, a 20 chevron.
    pub(crate) const WORKSPACE: RowStyle = RowStyle {
        title: (16.0, 24.0),
        subtitle: (14.0, 20.0),
        chevron: 20.0,
        pad: (16.0, 16.0),
        gap: 4.0,
    };

    /// The same rows with another chevron.
    pub(crate) const fn with_chevron(mut self, size: f32) -> RowStyle {
        self.chevron = size;
        self
    }
}

/// A title over a subtitle, then whatever ends the row (a chevron when `trailing` is none).
pub(crate) fn titled_row(
    title: &str,
    subtitle: &str,
    style: RowStyle,
    trailing: Option<Box<dyn Widget>>,
    action: Callback,
) -> Box<dyn Widget> {
    let trailing =
        trailing.unwrap_or_else(|| Box::new(icon(Glyph::ChevronRight, style.chevron, grey())));
    tap(
        Container::new()
            .padding_each(style.pad.0, style.pad.1, style.pad.0, style.pad.1)
            .child(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(Expanded::new(
                        // Stretched: a text sized to itself here can wrap its last word.
                        Column::new()
                            .spacing(style.gap)
                            .cross_axis_alignment(Align::Stretch)
                            .child(text(
                                title,
                                style.title.0,
                                style.title.1,
                                black(),
                                FontWeight::Medium,
                            ))
                            .child(text(
                                subtitle,
                                style.subtitle.0,
                                style.subtitle.1,
                                grey(),
                                FontWeight::Regular,
                            )),
                    ))
                    .child(trailing),
            ),
        action,
    )
}

/// A 52 tall settings row: a label, then a value and a chevron or any widget.
pub(crate) fn value_row(
    label: &str,
    trailing: Vec<Box<dyn Widget>>,
    action: Callback,
) -> Box<dyn Widget> {
    let mut row = Row::new()
        .spacing(8.0)
        .cross_axis_alignment(Align::Center)
        .child(Expanded::new(text(
            label,
            14.0,
            20.0,
            black(),
            FontWeight::Medium,
        )));
    for item in trailing {
        row = row.child(item);
    }
    tap(
        Container::new()
            .height(52.0)
            .padding_each(0.0, 16.0, 0.0, 16.0)
            .child(row),
        action,
    )
}

/// A grey value at the end of a row: "System".
pub(crate) fn value(label: &str) -> Box<dyn Widget> {
    Box::new(text(label, 14.0, 20.0, grey(), FontWeight::Regular))
}

/// The chevron at the end of a settings row.
pub(crate) fn chevron20() -> Box<dyn Widget> {
    Box::new(icon(Glyph::ChevronRight, 20.0, grey()))
}
