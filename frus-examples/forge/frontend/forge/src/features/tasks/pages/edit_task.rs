use frus::{
    Align, Column, Container, Expanded, FontWeight, Justify, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, green_tint, grey, primary_color, red},
        icons::{icon, Glyph},
        routes::{assign_task, project_board, task, task_metadata},
        shared::on,
        ui::{
            avatar, back_link, filled, gap, icon_button, labelled, outlined, primary_button,
            section_label, segmented, tap, text, title_block, Field,
        },
    },
    features::{
        projects::pages::board::missing,
        tasks::{
            states::task_form_state::{TaskFormState, FORM_STATUSES},
            widgets::task_fields::{choice_row, label_chip},
        },
    },
    models::{store, task::Priority},
};

/// Change a task. Its assignee and its due date and labels have pages of their own.
pub(crate) struct EditTaskPage {
    pub task: u32,
}

/// The edit form.
pub(crate) struct EditTaskState {
    pub form: TaskFormState,
}

impl StatefulWidget for EditTaskPage {
    type State = EditTaskState;

    fn create_state(&self) -> EditTaskState {
        let form = store::read(|s| s.task(self.task).map(TaskFormState::of))
            .unwrap_or_else(|| TaskFormState::blank(None));
        EditTaskState { form }
    }
}

/// A full-width row that opens another page: the assignee, the due date.
fn link_row(content: Vec<Box<dyn Widget>>, action: frus::Callback) -> Box<dyn Widget> {
    let mut row = Row::new().spacing(10.0).cross_axis_alignment(Align::Center);
    for item in content {
        row = row.child(item);
    }
    row = row
        .child(Expanded::new(Container::new()))
        .child(icon(Glyph::ChevronRight, 16.0, grey()));
    tap(
        Container::new()
            .height(48.0)
            .decoration(outlined(8.0))
            .padding_each(0.0, 12.0, 0.0, 12.0)
            .child(row),
        action,
    )
}

/// The priorities the edit form offers.
const PRIORITIES: [Priority; 3] = [Priority::Low, Priority::Medium, Priority::High];

impl State for EditTaskState {
    type Widget = EditTaskPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let task_id = cx.widget().task;
        let Some((stored, project_name, assignee)) = store::read(|s| {
            let t = s.task(task_id)?.clone();
            let name = s.project(t.project_id)?.name.clone();
            let assignee = t.assignee.and_then(|m| s.member(m)).cloned();
            Some((t, name, assignee))
        }) else {
            return missing("Task");
        };
        let form = &self.form;

        let save = {
            let router = router.clone();
            cx.callback(move |s: &mut EditTaskState| {
                if s.form.title.trim().is_empty() {
                    s.form.submitted = true;
                    return;
                }
                store::write(|store| {
                    if let Some(t) = store.tasks.iter_mut().find(|t| t.id == task_id) {
                        t.title = s.form.title.trim().to_string();
                        t.description = s.form.description.trim().to_string();
                        t.status = FORM_STATUSES[s.form.status];
                        t.priority = s.form.priority;
                    }
                });
                if !router.pop() {
                    router.go(task(task_id));
                }
            })
        };
        let delete = {
            let router = router.clone();
            let project = stored.project_id;
            on(move || {
                store::write(|store| {
                    store.tasks.retain(|t| t.id != task_id);
                    if let Some(p) = store.projects.iter_mut().find(|p| p.id == project) {
                        p.task_count = p.task_count.saturating_sub(1);
                    }
                });
                router.go(project_board(project));
            })
        };

        let top = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Center)
            .child(back_link(
                "Task details",
                16.0,
                4.0,
                back(&router, task(task_id)),
            ))
            .child(icon_button(Glyph::Ellipsis, 20.0, 36.0, grey(), on(|| {})));

        let mut title_field = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(section_label("Task title"))
            .child(
                Field::new(
                    &form.title,
                    "Task title",
                    cx.handler(|s: &mut EditTaskState, v: String| s.form.title = v),
                )
                .radius(8.0)
                .build(),
            );
        if form.submitted && form.title.trim().is_empty() {
            title_field = title_field.child(text(
                "A task needs a title.",
                12.0,
                16.0,
                red(),
                FontWeight::Regular,
            ));
        }

        let mut assignee_row: Vec<Box<dyn Widget>> = vec![];
        if let Some(m) = &assignee {
            assignee_row.push(avatar(&m.letter(), 28.0, 12.0, m.is_you));
            assignee_row.push(Box::new(text(
                m.name.clone(),
                14.0,
                20.0,
                black(),
                FontWeight::Regular,
            )));
        } else {
            assignee_row.push(Box::new(text(
                "Nobody",
                14.0,
                20.0,
                grey(),
                FontWeight::Regular,
            )));
        }
        let mut due_row: Vec<Box<dyn Widget>> =
            vec![Box::new(icon(Glyph::CalendarDays, 16.0, grey()))];
        if let Some(day) = stored.due_day {
            due_row.push(Box::new(text(
                format!("Sep {day}"),
                14.0,
                20.0,
                black(),
                FontWeight::Regular,
            )));
        }

        let mut labels = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for label in &stored.labels {
            labels = labels.child(label_chip(label, None));
        }

        let open = |to: String| {
            let router = router.clone();
            on(move || router.push(&to))
        };
        let priority_index = PRIORITIES.iter().position(|p| *p == form.priority);

        let fields = Column::new()
            .spacing(20.0)
            .cross_axis_alignment(Align::Stretch)
            .child(title_field)
            .child(labelled(
                "Description",
                Field::new(
                    &form.description,
                    "What needs to be done?",
                    cx.handler(|s: &mut EditTaskState, v: String| s.form.description = v),
                )
                .radius(8.0)
                .height(112.0)
                .lines(4)
                .build(),
            ))
            .child(labelled(
                "Status",
                segmented(&["Todo", "In progress", "Review"], form.status, 14.0, |i| {
                    cx.callback(move |s: &mut EditTaskState| s.form.status = i)
                }),
            ))
            .child(labelled(
                "Priority",
                choice_row(&["Low", "Medium", "High"], priority_index, |i| {
                    cx.callback(move |s: &mut EditTaskState| s.form.priority = PRIORITIES[i])
                }),
            ))
            .child(labelled(
                "Assignee",
                link_row(assignee_row, open(assign_task(task_id))),
            ))
            .child(labelled(
                "Due date",
                link_row(due_row, open(task_metadata(task_id))),
            ))
            .child(labelled("Labels", Box::new(labels)));

        let synced = Container::new()
            .decoration(filled(green_tint(), 8.0))
            .padding_each(12.0, 12.0, 12.0, 12.0)
            .child(
                Row::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Center)
                    .child(icon(Glyph::CloudCheck, 16.0, primary_color()))
                    .child(text(
                        "Changes synced just now",
                        14.0,
                        20.0,
                        primary_color(),
                        FontWeight::Regular,
                    )),
            );

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(top)
            .child(gap(20.0))
            .child(title_block(
                "Edit task",
                "Keep the task details up to date.",
                8.0,
            ))
            .child(gap(24.0))
            .child(fields)
            .child(gap(24.0))
            .child(synced)
            .child(gap(20.0))
            .child(
                Row::new().child(tap(
                    Container::new()
                        .padding_each(12.0, 4.0, 12.0, 4.0)
                        .child(text("Delete task", 14.0, 20.0, red(), FontWeight::Medium)),
                    delete,
                )),
            )
            .child(gap(12.0))
            .child(primary_button("Save changes", 48.0, 8.0, save));

        Page::new(AppHeader::new(project_name), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
