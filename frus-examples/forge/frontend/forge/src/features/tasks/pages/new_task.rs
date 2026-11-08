use frus::{
    Align, Column, Container, FontWeight, GestureDetector, Justify, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, grey, primary_color, red},
        icons::{icon, Glyph},
        routes::{project_board, task},
        ui::{
            avatar, back_link, cancel, gap, hug, labelled, outlined, primary_button, section_label,
            segmented, tap, text, title_block, Field,
        },
    },
    features::{
        projects::pages::board::missing,
        tasks::{
            states::task_form_state::{Picker, TaskFormState, FORM_STATUSES},
            widgets::{
                calendar::calendar,
                task_fields::{add_chip, choice_row, label_chip},
            },
        },
    },
    models::{
        calendar::step,
        store,
        task::{Priority, Task},
    },
};

/// Create a task in a project.
pub(crate) struct NewTaskPage {
    pub project: u32,
}

impl StatefulWidget for NewTaskPage {
    type State = TaskFormState;

    fn create_state(&self) -> TaskFormState {
        TaskFormState::blank(Some(store::read(|s| s.you().id)))
    }
}

/// A small bordered box that opens a picker.
fn picker_box(content: Vec<Box<dyn Widget>>, action: frus::Callback) -> Box<dyn Widget> {
    Box::new(
        Row::new().child(tap(
            Container::new()
                .decoration(outlined(8.0))
                .padding(12.0)
                .child(hug(8.0, content)),
            action,
        )),
    )
}

impl State for TaskFormState {
    type Widget = NewTaskPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let project_id = cx.widget().project;
        let Some((project, members)) = store::read(|s| {
            let project = s.project(project_id)?.clone();
            let members = s.members_of(&project);
            Some((project, members))
        }) else {
            return missing("Project");
        };
        let assignee = self
            .assignee
            .and_then(|id| members.iter().find(|m| m.id == id))
            .cloned();

        let create = {
            let router = router.clone();
            cx.callback(move |s: &mut TaskFormState| {
                s.submitted = true;
                if s.title.trim().is_empty() {
                    return;
                }
                let id = store::write(|store| {
                    let id = store.next_task_id();
                    store.tasks.push(Task {
                        id,
                        project_id,
                        title: s.title.trim().to_string(),
                        description: s.description.trim().to_string(),
                        status: FORM_STATUSES[s.status],
                        priority: s.priority,
                        assignee: s.assignee,
                        due_day: s.due_day,
                        labels: s.labels.clone(),
                        estimate: None,
                    });
                    if let Some(p) = store.projects.iter_mut().find(|p| p.id == project_id) {
                        p.task_count += 1;
                    }
                    id
                });
                router.replace(task(id));
            })
        };

        let go_back = back(&router, project_board(project_id));
        let top = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Center)
            .child(back_link(&project.name, 16.0, 8.0, go_back.clone()))
            .child(tap(
                text("Create", 14.0, 20.0, primary_color(), FontWeight::SemiBold),
                create.clone(),
            ));

        // Assignee: the chosen member, and the project's members to choose from when open.
        let mut assignee_section = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label("Assignee"));
        let mut chosen: Vec<Box<dyn Widget>> = vec![];
        match &assignee {
            Some(m) => chosen.push(avatar(&m.letter(), 32.0, 12.0, m.is_you)),
            None => chosen.push(Box::new(text(
                "Nobody",
                14.0,
                20.0,
                grey(),
                FontWeight::Regular,
            ))),
        }
        chosen.push(Box::new(icon(Glyph::ChevronRight, 16.0, grey())));
        assignee_section = assignee_section.child(picker_box(
            chosen,
            cx.callback(|s: &mut TaskFormState| s.toggle(Picker::Assignee)),
        ));
        if self.picker == Picker::Assignee {
            let mut choices = Row::new().spacing(12.0).cross_axis_alignment(Align::Center);
            for m in &members {
                let id = m.id;
                choices = choices.child(
                    GestureDetector::new(
                        Column::new()
                            .spacing(4.0)
                            .cross_axis_alignment(Align::Center)
                            .child(avatar(&m.letter(), 36.0, 13.0, self.assignee == Some(id)))
                            .child(text(
                                m.first_name(),
                                12.0,
                                16.0,
                                black(),
                                FontWeight::Regular,
                            )),
                    )
                    .on_tap(cx.callback(move |s: &mut TaskFormState| {
                        s.assignee = Some(id);
                        s.picker = Picker::None;
                    })),
                );
            }
            assignee_section = assignee_section.child(choices);
        }

        // Due date: the day, and the month to pick it from when open.
        let mut due: Vec<Box<dyn Widget>> = vec![Box::new(icon(Glyph::CalendarDays, 16.0, grey()))];
        if let Some(day) = self.due_day {
            due.push(Box::new(text(
                format!("Sep {day}"),
                14.0,
                20.0,
                black(),
                FontWeight::Regular,
            )));
        }
        due.push(Box::new(icon(Glyph::ChevronRight, 16.0, grey())));
        let mut due_section = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label("Due date"))
            .child(picker_box(
                due,
                cx.callback(|s: &mut TaskFormState| s.toggle(Picker::Due)),
            ));
        if self.picker == Picker::Due {
            let (year, month) = self.month;
            due_section = due_section.child(calendar(
                year,
                month,
                self.due_day,
                |by| {
                    cx.callback(move |s: &mut TaskFormState| {
                        s.month = step(s.month.0, s.month.1, by)
                    })
                },
                |day| {
                    cx.callback(move |s: &mut TaskFormState| {
                        s.due_day = Some(day);
                        s.picker = Picker::None;
                    })
                },
            ));
        }

        // Labels, and a field to add one when "+" is pressed.
        let mut labels = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for label in &self.labels {
            labels = labels.child(label_chip(label, None));
        }
        labels = labels.child(add_chip(
            cx.callback(|s: &mut TaskFormState| s.toggle(Picker::Label)),
        ));
        let mut label_section = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label("Labels"))
            .child(labels);
        if self.picker == Picker::Label {
            label_section = label_section.child(
                Field::new(
                    &self.label_draft,
                    "Add a label",
                    cx.handler(TaskFormState::set_label_draft),
                )
                .height(40.0)
                .radius(8.0)
                .on_submit(cx.callback(TaskFormState::add_label))
                .build(),
            );
        }

        let mut title_field = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label("Task title"))
            .child(
                Field::new(
                    &self.title,
                    "e.g. Improve onboarding flow",
                    cx.handler(TaskFormState::set_title),
                )
                .radius(8.0)
                .build(),
            );
        if self.submitted && self.title.trim().is_empty() {
            title_field = title_field.child(text(
                "Give the task a title.",
                12.0,
                16.0,
                red(),
                FontWeight::Regular,
            ));
        }

        let priority_index = Priority::ALL.iter().position(|p| *p == self.priority);
        let form = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(title_field)
            .child(labelled(
                "Description",
                Field::new(
                    &self.description,
                    "What needs to be done?",
                    cx.handler(TaskFormState::set_description),
                )
                .radius(8.0)
                .height(128.0)
                .lines(5)
                .build(),
            ))
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Status"))
                    .child(segmented(
                        &["Todo", "In progress", "Review"],
                        self.status,
                        14.0,
                        |i| cx.callback(move |s: &mut TaskFormState| s.status = i),
                    )),
            )
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Priority"))
                    .child(choice_row(
                        &["None", "Low", "Medium", "High"],
                        priority_index,
                        |i| cx.callback(move |s: &mut TaskFormState| s.priority = Priority::ALL[i]),
                    )),
            )
            .child(assignee_section)
            .child(due_section)
            .child(label_section);

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(top)
            .child(gap(16.0))
            .child(title_block(
                "Create a task",
                "Add work for your team to pick up.",
                4.0,
            ))
            .child(gap(24.0))
            .child(form)
            .child(gap(32.0))
            .child(primary_button("Create task", 48.0, 8.0, create))
            .child(gap(4.0))
            .child(cancel(go_back));

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(16.0, 20.0, 24.0)
            .build()
    }
}
