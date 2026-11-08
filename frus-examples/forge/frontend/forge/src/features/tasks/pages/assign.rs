use frus::{
    Align, Column, Container, Expanded, FontWeight, GestureDetector, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, grey, primary_color},
        icons::{icon, Glyph},
        routes::edit_task,
        ui::{
            avatar, back_link, cancel, caption, gap, presence, primary_button, section_label, text,
            title_block, Field,
        },
    },
    features::projects::pages::board::missing,
    models::store,
};

/// Choose who a task belongs to.
pub(crate) struct AssignTaskPage {
    pub task: u32,
}

/// The member chosen so far and the search.
pub(crate) struct AssignState {
    pub chosen: Option<u32>,
    pub query: String,
}

impl StatefulWidget for AssignTaskPage {
    type State = AssignState;

    fn create_state(&self) -> AssignState {
        AssignState {
            chosen: store::read(|s| s.task(self.task).and_then(|t| t.assignee)),
            query: String::new(),
        }
    }
}

impl State for AssignState {
    type Widget = AssignTaskPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let task_id = cx.widget().task;
        let query = self.query.to_lowercase();
        let Some((project_name, members)) = store::read(|s| {
            let t = s.task(task_id)?;
            let project = s.project(t.project_id)?;
            Some((project.name.clone(), s.members_of(project)))
        }) else {
            return missing("Task");
        };

        let mut list = Column::new()
            .spacing(4.0)
            .cross_axis_alignment(Align::Stretch);
        for m in members
            .iter()
            .filter(|m| query.is_empty() || m.full_name.to_lowercase().contains(&query))
        {
            let id = m.id;
            let selected = self.chosen == Some(id);
            let mut row = Row::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Center)
                .child(avatar(&m.letter(), 40.0, 14.0, selected))
                .child(Expanded::new(
                    Column::new()
                        .spacing(2.0)
                        .cross_axis_alignment(Align::Start)
                        .child(text(
                            m.name.clone(),
                            14.0,
                            20.0,
                            black(),
                            FontWeight::SemiBold,
                        ))
                        .child(caption(m.title.clone()))
                        .child(
                            Container::new()
                                .padding_each(2.0, 0.0, 0.0, 0.0)
                                .child(presence(m.presence, grey())),
                        ),
                ));
            if selected {
                row = row.child(icon(Glyph::Check, 20.0, primary_color()));
            }
            list = list.child(
                GestureDetector::new(Container::new().padding_each(8.0, 0.0, 8.0, 0.0).child(row))
                    .on_tap(cx.callback(move |s: &mut AssignState| s.chosen = Some(id))),
            );
        }

        let assign = {
            let router = router.clone();
            cx.callback(move |s: &mut AssignState| {
                let chosen = s.chosen;
                store::write(|store| {
                    if let Some(t) = store.tasks.iter_mut().find(|t| t.id == task_id) {
                        t.assignee = chosen;
                    }
                });
                if !router.pop() {
                    router.go(edit_task(task_id));
                }
            })
        };
        let go_back = back(&router, edit_task(task_id));

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(back_link("Edit task", 16.0, 8.0, go_back.clone()))
            .child(gap(24.0))
            .child(title_block(
                "Assign task",
                "Choose who owns this work.",
                8.0,
            ))
            .child(gap(24.0))
            .child(
                Field::new(
                    &self.query,
                    "Search members",
                    cx.handler(|s: &mut AssignState, v: String| s.query = v),
                )
                .height(44.0)
                .radius(8.0)
                .lead(Glyph::Search)
                .build(),
            )
            .child(gap(24.0))
            .child(section_label("Members"))
            .child(gap(12.0))
            .child(list);

        let actions = Column::new()
            .spacing(4.0)
            .cross_axis_alignment(Align::Stretch)
            .child(primary_button("Assign", 48.0, 8.0, assign))
            .child(cancel(go_back));

        Page::new(AppHeader::new(project_name), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .footer(actions)
            .padding(24.0, 20.0, 24.0)
            .build()
    }
}
