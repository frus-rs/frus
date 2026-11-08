use frus::{asset, Align, Callback, FontWeight, Row, Text, Widget};

use crate::common::colors::black;

/// A callback running `work`.
pub(crate) fn on(work: impl Fn() + 'static) -> Callback {
    Callback::new(work)
}

/// The Forge logo and name, at the top of the signed-out pages.
pub(crate) fn app_logo() -> Box<dyn Widget> {
    let logo = asset!("../../assets/logo.png").size(32.0, 32.0);
    Box::new(
        Row::new()
            .child(logo)
            .child(
                Text::new("Forge")
                    .weight(FontWeight::SemiBold)
                    .size(18.0)
                    .color(black()),
            )
            .cross_axis_alignment(Align::Center)
            .spacing(8.0),
    )
}
