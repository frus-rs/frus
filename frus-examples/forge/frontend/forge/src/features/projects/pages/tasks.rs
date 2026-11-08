use frus::{
    Align, Column, FontWeight, Justify, Row, State, StateContext, StatefulWidget, StatelessWidget,
    Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::grey,
        icons::{icon, Glyph},
        routes::task,
        shared::on,
        ui::{caption, chips, gap, hug, muted, page_title, tracked, Field},
    },
    features::projects::{
        pages::board::missing,
        states::tasks_state::ProjectTasksState,
        widgets::{project_nav::back_and_new_task, task_card::TaskCard},
    },
    models::{store, task::TaskStatus},
};

/// A project's tasks as a list, grouped by status, searchable and filtered.
pub(crate) struct ProjectTasksPage {
    pub project: u32,
}

impl StatefulWidget for ProjectTasksPage {
    type State = ProjectTasksState;

    fn create_state(&self) -> ProjectTasksState {
        ProjectTasksState::default()
    }
}

/// The statuses the filter offers, after "All".
const FILTERS: [TaskStatus; 3] = [TaskStatus::Todo, TaskStatus::InProgress, TaskStatus::Review];

impl State for ProjectTasksState {
    type Widget = ProjectTasksPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let project_id = cx.widget().project;
        let query = self.query.to_lowercase();
        let filter = self.filter;

        let Some((project, groups, total)) = store::read(|s| {
            let project = s.project(project_id)?.clone();
            let tasks = s.tasks_of(project.id);
            let total = project.task_count.max(tasks.len());
            // In progress first, as the design lists them.
            let order = [
                TaskStatus::InProgress,
                TaskStatus::Todo,
                TaskStatus::Review,
                TaskStatus::Done,
            ];
            let groups = order
                .iter()
                .filter(|status| filter == 0 || FILTERS[filter - 1] == **status)
                .map(|status| {
                    let cards = tasks
                        .iter()
                        .filter(|t| t.status == *status)
                        .filter(|t| query.is_empty() || t.title.to_lowercase().contains(&query))
                        .map(|t| {
                            let router = router.clone();
                            let id = t.id;
                            TaskCard {
                                task: t.clone(),
                                assignee: t.assignee.and_then(|m| s.member(m)).cloned(),
                                on_tap: on(move || router.push(task(id))),
                            }
                        })
                        .collect::<Vec<_>>();
                    (*status, cards)
                })
                .filter(|(_, cards)| !cards.is_empty())
                .collect::<Vec<_>>();
            Some((project, groups, total))
        }) else {
            return missing("Project");
        };

        let mut list = Column::new()
            .spacing(16.0)
            .cross_axis_alignment(Align::Stretch);
        if groups.is_empty() {
            list = list.child(muted("No tasks match."));
        }
        for (status, cards) in groups {
            let mut section = Column::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Stretch)
                .child(tracked(
                    status.label().to_uppercase(),
                    12.0,
                    16.0,
                    0.6,
                    grey(),
                    FontWeight::SemiBold,
                ));
            for card in cards {
                section = section.child(card.into_widget());
            }
            list = list.child(section);
        }

        let mut options = vec!["All".to_string()];
        options.extend(FILTERS.iter().map(|s| s.label().to_string()));

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(back_and_new_task(&router, project.id, &project.name))
            .child(gap(24.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Center)
                    .child(page_title("Tasks"))
                    .child(muted(format!("{total} tasks"))),
            )
            .child(gap(24.0))
            .child(
                Field::new(
                    &self.query,
                    "Search tasks",
                    cx.handler(ProjectTasksState::set_query),
                )
                .height(44.0)
                .radius(8.0)
                .lead(Glyph::Search)
                .build(),
            )
            .child(gap(24.0))
            .child(chips(&options, self.filter, |i| {
                cx.callback(move |s: &mut ProjectTasksState| s.filter = i)
            }))
            .child(gap(24.0))
            .child(list)
            .child(gap(28.0))
            .child(Row::new().main_axis_alignment(Justify::Center).child(hug(
                8.0,
                vec![
                    Box::new(icon(Glyph::Cloud, 16.0, grey())),
                    Box::new(caption("Changes sync automatically")),
                ],
            )));

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
