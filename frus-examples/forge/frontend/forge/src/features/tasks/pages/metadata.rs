use frus::{
    Align, Column, Container, FontWeight, Row, State, StateContext, StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, primary_color, white},
        icons::{icon, Glyph},
        routes::edit_task,
        ui::{
            back_link, banner_large, cancel, filled, primary_button, tap, text, title_block, Field,
        },
    },
    features::{
        projects::pages::board::missing,
        tasks::widgets::{
            calendar::calendar,
            task_fields::{choice_row, label_chip},
        },
    },
    models::{calendar::step, store},
};

/// A task's due date, labels and estimate.
pub(crate) struct TaskMetadataPage {
    pub task: u32,
}

pub(crate) struct MetadataState {
    pub month: (i32, u32),
    pub due_day: Option<u32>,
    pub labels: Vec<String>,
    pub draft: String,
    pub estimate: Option<usize>,
}

const ESTIMATES: [&str; 3] = ["1h", "4h", "1d"];

impl StatefulWidget for TaskMetadataPage {
    type State = MetadataState;

    fn create_state(&self) -> MetadataState {
        store::read(|s| {
            let t = s.task(self.task);
            MetadataState {
                month: (2025, 9),
                due_day: t.and_then(|t| t.due_day),
                labels: t.map(|t| t.labels.clone()).unwrap_or_default(),
                draft: String::new(),
                estimate: t
                    .and_then(|t| t.estimate)
                    .and_then(|e| ESTIMATES.iter().position(|x| *x == e)),
            }
        })
    }
}

fn heading(title: &str) -> frus::Text {
    text(title, 16.0, 24.0, black(), FontWeight::SemiBold)
}

impl State for MetadataState {
    type Widget = TaskMetadataPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let task_id = cx.widget().task;
        let Some(project_name) = store::read(|s| {
            let t = s.task(task_id)?;
            Some(s.project(t.project_id)?.name.clone())
        }) else {
            return missing("Task");
        };

        let add = cx.callback(|s: &mut MetadataState| {
            let label = s.draft.trim().to_lowercase();
            if !label.is_empty() && !s.labels.contains(&label) {
                s.labels.push(label);
            }
            s.draft.clear();
        });

        let mut chips = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for (i, label) in self.labels.iter().enumerate() {
            chips = chips.child(label_chip(
                label,
                Some(cx.callback(move |s: &mut MetadataState| {
                    s.labels.remove(i);
                })),
            ));
        }

        let save = {
            let router = router.clone();
            cx.callback(move |s: &mut MetadataState| {
                let (due, labels, estimate) = (
                    s.due_day,
                    s.labels.clone(),
                    s.estimate.map(|i| ESTIMATES[i]),
                );
                store::write(|store| {
                    if let Some(t) = store.tasks.iter_mut().find(|t| t.id == task_id) {
                        t.due_day = due;
                        t.labels = labels;
                        t.estimate = estimate;
                    }
                });
                if !router.pop() {
                    router.go(edit_task(task_id));
                }
            })
        };
        let go_back = back(&router, edit_task(task_id));
        let (year, month) = self.month;

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link("Edit task", 16.0, 8.0, go_back.clone()))
            .child(title_block(
                "Task details",
                "Set timing and organize this work.",
                8.0,
            ))
            .child(
                Column::new()
                    .spacing(16.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(heading("Due date"))
                    .child(calendar(
                        year,
                        month,
                        self.due_day,
                        |by| {
                            cx.callback(move |s: &mut MetadataState| {
                                s.month = step(s.month.0, s.month.1, by)
                            })
                        },
                        |day| cx.callback(move |s: &mut MetadataState| s.due_day = Some(day)),
                    )),
            )
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(heading("Labels"))
                    .child(
                        Row::new()
                            .spacing(8.0)
                            .cross_axis_alignment(Align::Center)
                            .child(frus::Expanded::new(
                                Field::new(
                                    &self.draft,
                                    "Add a label",
                                    cx.handler(|s: &mut MetadataState, v: String| s.draft = v),
                                )
                                .height(40.0)
                                .radius(6.0)
                                .font(16.0)
                                .on_submit(add.clone())
                                .build(),
                            ))
                            .child(tap(
                                Container::new()
                                    .height(40.0)
                                    .padding_each(0.0, 16.0, 0.0, 16.0)
                                    .decoration(filled(primary_color(), 6.0))
                                    .alignment(frus::Alignment::CENTER)
                                    .child(text("Add", 14.0, 20.0, white(), FontWeight::Medium)),
                                add,
                            )),
                    )
                    .child(chips),
            )
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(heading("Estimate"))
                    .child(choice_row(&ESTIMATES, self.estimate, |i| {
                        cx.callback(move |s: &mut MetadataState| s.estimate = Some(i))
                    })),
            )
            .child(banner_large(
                Box::new(icon(Glyph::Info, 16.0, primary_color())),
                "Metadata syncs automatically when you save.",
                16.0,
                12.0,
            ))
            .child(
                Column::new()
                    .spacing(4.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(primary_button("Save metadata", 48.0, 6.0, save))
                    .child(cancel(go_back)),
            );

        Page::new(AppHeader::new(project_name), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
