use frus::{
    Align, BuildContext, Callback, Column, Container, FontWeight, GestureDetector, Justify, Row,
    StatelessWidget, Widget,
};

use crate::{
    common::{
        colors::{amber, black, grey, light_gray, red, surface, teal},
        ui::{avatar, caption, dot, filled, outlined, text, tracked, FULL},
    },
    models::{
        member::Member,
        task::{Priority, Task},
    },
};

/// The dot a task's priority shows on the board.
pub(crate) fn priority_color(priority: Priority) -> frus::Color {
    match priority {
        Priority::None => light_gray(),
        Priority::Low => teal(),
        Priority::Medium => amber(),
        Priority::High => red(),
    }
}

/// A column of the board: its status, how many tasks it holds, and their cards.
pub(crate) struct BoardColumn {
    pub title: String,
    pub cards: Vec<(Task, Option<Member>, Callback)>,
}

impl StatelessWidget for BoardColumn {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let header = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Center)
            .child(tracked(
                self.title.to_uppercase(),
                12.0,
                16.0,
                0.6,
                black(),
                FontWeight::SemiBold,
            ))
            .child(
                Container::new()
                    .height(22.0)
                    .padding_each(0.0, 8.0, 0.0, 8.0)
                    .decoration(filled(surface(), FULL))
                    .alignment(frus::Alignment::CENTER)
                    .child(text(
                        self.cards.len().to_string(),
                        12.0,
                        16.0,
                        grey(),
                        FontWeight::Medium,
                    )),
            );

        let mut column = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch)
            .child(
                Container::new()
                    .padding_each(0.0, 0.0, 4.0, 0.0)
                    .child(header),
            );
        for (task, member, open) in &self.cards {
            let mut meta = Row::new()
                .spacing(8.0)
                .cross_axis_alignment(Align::Center)
                .child(caption(format!("#{}", task.id)))
                .child(frus::Spacer::new())
                .child(dot(priority_color(task.priority), 8.0));
            if let Some(member) = member {
                meta = meta.child(avatar(&member.initials, 24.0, 9.0, member.is_you));
            }
            let card = Container::new()
                .decoration(outlined(8.0).shadow(frus::BoxShadow::new(
                    0.0,
                    1.0,
                    2.0,
                    frus::Colors::BLACK.with_alpha(0.05),
                )))
                .padding(13.0)
                .child(
                    Column::new()
                        .spacing(16.0)
                        .cross_axis_alignment(Align::Stretch)
                        .child(text(
                            task.title.clone(),
                            14.0,
                            20.0,
                            black(),
                            FontWeight::SemiBold,
                        ))
                        .child(meta),
                );
            column = column.child(GestureDetector::new(card).on_tap(open.clone()));
        }
        if self.cards.is_empty() {
            column = column.child(
                Container::new()
                    .height(64.0)
                    .alignment(frus::Alignment::CENTER)
                    .child(caption("No tasks")),
            );
        }

        Box::new(
            Container::new()
                .width(230.0)
                .decoration(filled(frus::Color::hex("#F7F7F8"), 12.0))
                .padding(12.0)
                .child(column),
        )
    }
}
