use frus::{
    Align, Column, Container, Expanded, FontWeight, Justify, KeyboardType, Row, State,
    StateContext, StatefulWidget, TextField, Widget,
};

use crate::{
    common::{
        chrome::{back, Page},
        colors::{
            black, gray, green_tint, grey, light_gray, primary_color, red_text, red_tint, surface,
            white,
        },
        icons::{icon, Glyph},
        routes::{edit_task, project_board},
        shared::on,
        ui::{
            avatar, avatar_colored, banner, divider, field_style, filled, gap, hug, icon_button,
            outlined, pill, section_label, tap, text, tracked, FULL,
        },
    },
    features::{
        projects::pages::board::missing, tasks::states::task_detail_state::TaskDetailState,
    },
    models::{store, task::Comment},
};

/// A task: its status, priority, assignee and date, its description and labels, and the
/// conversation about it.
pub(crate) struct TaskDetailPage {
    pub task: u32,
}

impl StatefulWidget for TaskDetailPage {
    type State = TaskDetailState;

    fn create_state(&self) -> TaskDetailState {
        TaskDetailState::default()
    }
}

/// A grey pill with an icon or an avatar before its text.
fn meta_pill(lead: Box<dyn Widget>, label: &str) -> Box<dyn Widget> {
    Box::new(
        Container::new()
            .height(30.0)
            .decoration(filled(light_gray(), FULL))
            .padding_each(0.0, 12.0, 0.0, 6.0)
            .alignment(frus::Alignment::CENTER)
            .child(hug(
                6.0,
                vec![
                    lead,
                    Box::new(text(label, 13.0, 18.0, black(), FontWeight::Medium)),
                ],
            )),
    )
}

fn comment_card(comment: &Comment, letter: &str, name: &str, you: bool) -> Box<dyn Widget> {
    Box::new(
        Container::new()
            .decoration(outlined(12.0))
            .padding(17.0)
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(
                        Row::new()
                            .spacing(8.0)
                            .cross_axis_alignment(Align::Center)
                            .child(avatar(letter, 28.0, 11.0, you))
                            .child(text(name, 12.0, 16.0, black(), FontWeight::SemiBold))
                            .child(Expanded::new(text(
                                format!("· {}", comment.ago),
                                12.0,
                                16.0,
                                grey(),
                                FontWeight::Regular,
                            )))
                            .child(icon(Glyph::Ellipsis, 16.0, black())),
                    )
                    .child(text(
                        comment.text.clone(),
                        14.0,
                        24.0,
                        grey(),
                        FontWeight::Regular,
                    )),
            ),
    )
}

impl State for TaskDetailState {
    type Widget = TaskDetailPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let task_id = cx.widget().task;
        let Some((task, project, assignee, comments)) = store::read(|s| {
            let task = s.task(task_id)?.clone();
            let project = s.project(task.project_id)?.clone();
            let assignee = task.assignee.and_then(|m| s.member(m)).cloned();
            let comments = s
                .comments_of(task.id)
                .into_iter()
                .filter_map(|c| s.member(c.author).map(|m| (c, m.clone())))
                .collect::<Vec<_>>();
            Some((task, project, assignee, comments))
        }) else {
            return missing("Task");
        };

        // The top bar: back to the project, and the task's menu (edit).
        let to_edit = {
            let router = router.clone();
            on(move || router.push(edit_task(task_id)))
        };
        let top = frus::MediaQuery::of().padding.top;
        let bar = Container::new()
            .height(52.0 + top)
            .color(surface())
            .padding_each(top, 20.0, 0.0, 28.0)
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Center)
                    .child(tap(
                        hug(
                            8.0,
                            vec![
                                Box::new(icon(Glyph::ChevronLeft, 20.0, black())),
                                Box::new(text(
                                    project.name.clone(),
                                    16.0,
                                    24.0,
                                    black(),
                                    FontWeight::SemiBold,
                                )),
                            ],
                        ),
                        back(&router, project_board(project.id)),
                    ))
                    .child(icon_button(Glyph::Ellipsis, 20.0, 36.0, black(), to_edit)),
            );

        let status = Container::new()
            .decoration(
                filled(green_tint(), FULL)
                    .border(frus::Border::new(1.0, frus::Color::hex("#B9F8CF"))),
            )
            .padding_each(7.0, 12.0, 7.0, 12.0)
            .child(tracked(
                task.status.label().to_uppercase(),
                11.0,
                16.0,
                0.3,
                primary_color(),
                FontWeight::SemiBold,
            ));

        let mut chips = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        if task.priority != crate::models::task::Priority::None {
            let (fill, ink) = if task.priority == crate::models::task::Priority::High {
                (red_tint(), red_text())
            } else {
                (light_gray(), grey())
            };
            chips = chips.child(pill(
                task.priority.label().to_uppercase(),
                fill,
                ink,
                12.0,
                9.0,
                11.0,
            ));
        }
        if let Some(member) = &assignee {
            chips = chips.child(meta_pill(
                avatar_colored(
                    &member.letter(),
                    20.0,
                    10.0,
                    primary_color(),
                    white(),
                    FontWeight::SemiBold,
                ),
                member.first_name(),
            ));
        }
        if let Some(day) = task.due_day {
            chips = chips.child(meta_pill(
                Box::new(icon(Glyph::Calendar, 14.0, grey())),
                &format!("Sep {day}"),
            ));
        }

        let mut labels = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for label in &task.labels {
            labels = labels.child(
                Container::new()
                    .decoration(filled(light_gray(), FULL).border(frus::Border::new(1.0, gray())))
                    .padding_each(4.0, 12.0, 4.0, 12.0)
                    .child(text(label.clone(), 12.0, 16.0, black(), FontWeight::Medium)),
            );
        }

        let mut thread = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch);
        for (comment, member) in &comments {
            thread = thread.child(comment_card(
                comment,
                &member.letter(),
                &member.name,
                member.is_you,
            ));
        }

        let description = if task.description.is_empty() {
            "No description yet.".to_string()
        } else {
            task.description.clone()
        };

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(hug(
                12.0,
                vec![
                    Box::new(status),
                    Box::new(text(
                        format!("FRU-{}", task.id),
                        12.0,
                        16.0,
                        grey(),
                        FontWeight::Regular,
                    )),
                ],
            ))
            .child(gap(16.0))
            .child(tracked(
                task.title.clone(),
                24.0,
                32.0,
                -0.6,
                black(),
                FontWeight::Bold,
            ))
            .child(gap(12.0))
            .child(chips)
            .child(gap(20.0))
            .child(divider())
            .child(gap(20.0))
            .child(section_label("Description"))
            .child(gap(8.0))
            .child(text(description, 14.0, 24.0, grey(), FontWeight::Regular))
            .child(gap(16.0))
            .child(section_label("Labels"))
            .child(gap(8.0))
            .child(labels)
            .child(gap(20.0))
            .child(banner(
                Box::new(icon(Glyph::Radio, 16.0, primary_color())),
                "Updated just now · synced",
                8.0,
                12.0,
                10.0,
            ))
            .child(gap(28.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Center)
                    .child(text("Comments", 16.0, 24.0, black(), FontWeight::SemiBold))
                    .child(text(
                        comments.len().to_string(),
                        12.0,
                        16.0,
                        grey(),
                        FontWeight::Regular,
                    )),
            )
            .child(gap(16.0))
            .child(thread);

        // The comment field, pinned at the bottom.
        let send = cx.callback(move |s: &mut TaskDetailState| {
            let text = s.draft.trim().to_string();
            if text.is_empty() {
                return;
            }
            store::write(|store| {
                let author = store.you().id;
                store.comments.push(Comment {
                    task_id,
                    author,
                    ago: "just now".into(),
                    text,
                });
            });
            s.draft.clear();
        });
        let composer = Container::new()
            .height(60.0)
            .decoration(outlined(16.0).shadow(frus::BoxShadow::new(
                0.0,
                1.0,
                3.0,
                frus::Colors::BLACK.with_alpha(0.06),
            )))
            .padding_each(0.0, 11.0, 0.0, 16.0)
            .child(
                Row::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Center)
                    .child(Expanded::new(
                        TextField::new(self.draft.as_str())
                            .placeholder("Write a comment...")
                            .on_input(cx.handler(TaskDetailState::set_draft))
                            .on_submit(send.clone())
                            .borderless()
                            .keyboard_type(KeyboardType::Text)
                            .size(14.0)
                            .style(field_style()),
                    ))
                    .child(tap(
                        Container::new()
                            .width(36.0)
                            .height(36.0)
                            .decoration(filled(primary_color(), FULL))
                            .alignment(frus::Alignment::CENTER)
                            .child(icon(Glyph::Send, 16.0, white())),
                        send,
                    )),
            );

        Page::with_bar(Box::new(bar), content)
            .plain_footer(composer)
            .padding(12.0, 20.0, 24.0)
            .build()
    }
}
