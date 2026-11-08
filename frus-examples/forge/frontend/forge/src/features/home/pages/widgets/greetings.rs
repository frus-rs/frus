use frus::{Align, BuildContext, Column, FontWeight, StatelessWidget, Widget};

use crate::common::{
    colors::{black, grey},
    ui::{text, tracked},
};

/// The date, the greeting and the line under it.
pub(crate) struct Greetings {
    pub name: String,
}

impl StatelessWidget for Greetings {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        Box::new(
            Column::new()
                .spacing(4.0)
                .cross_axis_alignment(Align::Start)
                .child(text(
                    "Tuesday, September 24",
                    12.0,
                    16.0,
                    grey(),
                    FontWeight::Regular,
                ))
                .child(tracked(
                    format!("Good morning, {}", self.name),
                    30.0,
                    36.0,
                    -0.75,
                    black(),
                    FontWeight::Bold,
                ))
                .child(text(
                    "Here’s what needs your attention.",
                    14.0,
                    20.0,
                    grey(),
                    FontWeight::Regular,
                )),
        )
    }
}
