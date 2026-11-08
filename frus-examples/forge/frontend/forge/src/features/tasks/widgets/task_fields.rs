//! The pieces the task forms share.

use frus::{Align, Callback, Container, FontWeight, GestureDetector, Row, Widget};

use crate::common::{
    colors::{black, gray, grey, light_gray, primary_color, surface},
    icons::{icon, Glyph},
    ui::{filled, text, FULL},
};

/// A label as a grey pill, with a cross to take it off when `remove` is given.
pub(crate) fn label_chip(label: &str, remove: Option<Callback>) -> Box<dyn Widget> {
    let mut row = Row::new()
        .spacing(4.0)
        .main_axis_size(frus::MainAxisSize::Min)
        .cross_axis_alignment(Align::Center)
        .child(text(label, 12.0, 16.0, black(), FontWeight::Medium));
    if let Some(remove) = remove {
        row = row.child(GestureDetector::new(icon(Glyph::X, 12.0, grey())).on_tap(remove));
    }
    Box::new(
        Container::new()
            .height(28.0)
            .decoration(filled(light_gray(), FULL))
            .padding_each(0.0, 12.0, 0.0, 12.0)
            .alignment(frus::Alignment::CENTER)
            .child(row),
    )
}

/// The round "+" after the labels.
pub(crate) fn add_chip(action: Callback) -> Box<dyn Widget> {
    Box::new(
        GestureDetector::new(
            Container::new()
                .width(28.0)
                .height(28.0)
                .decoration(filled(surface(), FULL).border(frus::Border::new(1.0, gray())))
                .alignment(frus::Alignment::CENTER)
                .child(icon(Glyph::Plus, 14.0, grey())),
        )
        .on_tap(action),
    )
}

/// Choices as text pills, the chosen one green: priorities, estimates.
pub(crate) fn choice_row(
    options: &[&str],
    selected: Option<usize>,
    on_select: impl Fn(usize) -> Callback,
) -> Box<dyn Widget> {
    let mut row = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
    for (i, option) in options.iter().enumerate() {
        let chosen = selected == Some(i);
        let chip = Container::new()
            .height(32.0)
            .decoration(filled(
                if chosen {
                    crate::common::colors::green_tint()
                } else {
                    light_gray()
                },
                FULL,
            ))
            .padding_each(0.0, 14.0, 0.0, 14.0)
            .alignment(frus::Alignment::CENTER)
            .child(text(
                *option,
                14.0,
                20.0,
                if chosen { primary_color() } else { black() },
                if chosen {
                    FontWeight::Medium
                } else {
                    FontWeight::Regular
                },
            ));
        row = row.child(GestureDetector::new(chip).on_tap(on_select(i)));
    }
    Box::new(row)
}
