use frus::{BuildContext, Column, Expanded, Row, StatelessWidget, Widget};

use crate::{
    common::{
        colors::{green_tint, grey, light_gray, primary_color},
        icons::Glyph,
    },
    features::home::pages::widgets::stats_card::StatsCard,
};

/// The four figures, two by two.
pub(crate) struct StatsGrid {
    pub assigned: usize,
    pub completed: usize,
    pub due_soon: usize,
    pub active_projects: usize,
}

impl StatelessWidget for StatsGrid {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let card = |glyph, glyph_color, count, title: &str, color| {
            Expanded::new(
                StatsCard {
                    glyph,
                    glyph_color,
                    count,
                    title: title.to_string(),
                    color,
                }
                .into_widget(),
            )
        };
        Box::new(
            Column::new()
                .spacing(12.0)
                .child(
                    Row::new()
                        .spacing(12.0)
                        .child(card(
                            Glyph::SquareCheck,
                            primary_color(),
                            self.assigned,
                            "Assigned tasks",
                            green_tint(),
                        ))
                        .child(card(
                            Glyph::CircleCheck,
                            primary_color(),
                            self.completed,
                            "Completed",
                            light_gray(),
                        )),
                )
                .child(
                    Row::new()
                        .spacing(12.0)
                        .child(card(
                            Glyph::Clock,
                            grey(),
                            self.due_soon,
                            "Due soon",
                            light_gray(),
                        ))
                        .child(card(
                            Glyph::Folder,
                            grey(),
                            self.active_projects,
                            "Active projects",
                            light_gray(),
                        )),
                ),
        )
    }
}
