use frus::{Align, BuildContext, Color, Column, Container, FontWeight, StatelessWidget, Widget};

use crate::common::{
    colors::{black, grey},
    icons::{icon, Glyph},
    ui::{filled, text},
};

/// One of the four figures at the top of the dashboard.
pub(crate) struct StatsCard {
    pub glyph: Glyph,
    pub glyph_color: Color,
    pub count: usize,
    pub title: String,
    pub color: Color,
}

impl StatelessWidget for StatsCard {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        Box::new(
            Container::new()
                .height(120.0)
                .decoration(filled(self.color, 12.0))
                .padding(16.0)
                .child(
                    Column::new()
                        .spacing(16.0)
                        .cross_axis_alignment(Align::Start)
                        .child(icon(self.glyph, 20.0, self.glyph_color))
                        .child(
                            Column::new()
                                .spacing(4.0)
                                .cross_axis_alignment(Align::Start)
                                .child(text(
                                    self.count.to_string(),
                                    24.0,
                                    32.0,
                                    black(),
                                    FontWeight::Bold,
                                ))
                                .child(text(
                                    self.title.clone(),
                                    12.0,
                                    16.0,
                                    grey(),
                                    FontWeight::Regular,
                                )),
                        ),
                ),
        )
    }
}
