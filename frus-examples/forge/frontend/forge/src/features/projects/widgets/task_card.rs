use frus::{
    Align, BuildContext, Callback, Column, FontWeight, GestureDetector, Justify, Row,
    StatelessWidget, Widget,
};

use crate::{
    common::{
        colors::{black, green_tint_strong, grey, light_gray, primary_color, red_text, red_tint},
        icons::{icon, Glyph},
        ui::{avatar_colored, caption, card, dot, hug, pill, tag, text},
    },
    models::{
        member::Member,
        task::{Priority, Task, TaskStatus},
    },
};

/// A task in a project's list: its title, number, priority or status, and who has it.
pub(crate) struct TaskCard {
    pub task: Task,
    pub assignee: Option<Member>,
    pub on_tap: Callback,
}

impl StatelessWidget for TaskCard {
    fn build(&self, _cx: &BuildContext) -> Box<dyn Widget> {
        let task = &self.task;
        let mut meta = Row::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Center)
            .child(caption(format!("#{}", task.id)));
        if task.priority == Priority::High {
            // A high priority takes the badge; the status follows as a dot and a word.
            meta = meta
                .child(pill("High", red_tint(), red_text(), 8.0, 4.0, 12.0))
                .child(hug(
                    4.0,
                    vec![
                        dot(
                            if task.status == TaskStatus::InProgress {
                                primary_color()
                            } else {
                                grey()
                            },
                            8.0,
                        ),
                        Box::new(caption(task.status.label())),
                    ],
                ));
        } else {
            meta = meta.child(tag(task.status.label()));
        }

        let mut column = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Start)
                    .child(text(
                        task.title.clone(),
                        14.0,
                        20.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(icon(Glyph::ChevronRight, 16.0, grey())),
            )
            .child(meta);
        if let Some(member) = &self.assignee {
            let (fill, ink) = if member.is_you {
                (green_tint_strong(), primary_color())
            } else {
                (light_gray(), grey())
            };
            column = column.child(hug(
                8.0,
                vec![
                    avatar_colored(
                        &member.letter(),
                        24.0,
                        10.0,
                        fill,
                        ink,
                        FontWeight::SemiBold,
                    ),
                    Box::new(caption(member.first_name())),
                ],
            ));
        }
        Box::new(GestureDetector::new(card(column)).on_tap(self.on_tap.clone()))
    }
}
