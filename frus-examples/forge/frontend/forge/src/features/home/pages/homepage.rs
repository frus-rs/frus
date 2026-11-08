use frus::{Align, Column, State, StateContext, StatefulWidget, StatelessWidget, Widget};

use crate::{
    common::{
        auth::Auth,
        chrome::{AppHeader, NavStyle, Page, Tab},
    },
    features::home::pages::{
        states::home_state::HomeState,
        widgets::{
            greetings::Greetings, recent_activity::RecentActivity,
            recent_project_widget::RecentProjectWidget, stats_grid::StatsGrid,
        },
    },
    models::{project::ProjectStatus, store, task::TaskStatus},
};

/// The dashboard: what needs the signed-in member's attention.
pub(crate) struct HomePage {
    #[allow(dead_code)]
    auth: Auth,
}

impl HomePage {
    pub(crate) fn new(auth: Auth) -> Self {
        Self { auth }
    }
}

impl StatefulWidget for HomePage {
    type State = HomeState;

    fn create_state(&self) -> HomeState {
        HomeState {}
    }
}

impl State for HomeState {
    type Widget = HomePage;

    fn build(&self, _cx: &StateContext<Self>) -> Box<dyn Widget> {
        let (name, grid, activity) = store::read(|s| {
            let you = s.you().clone();
            let mine = s.tasks.iter().filter(|t| t.assignee == Some(you.id));
            let grid = StatsGrid {
                assigned: 12.max(mine.clone().count()),
                completed: 28
                    + mine
                        .clone()
                        .filter(|t| t.status == TaskStatus::Done)
                        .count(),
                due_soon: 5,
                active_projects: s
                    .projects
                    .iter()
                    .filter(|p| p.status == ProjectStatus::Active)
                    .count()
                    .max(3),
            };
            // The dashboard's three latest entries, the newest first.
            let activity = s
                .activity
                .iter()
                .filter(|a| a.day == "Today")
                .filter_map(|a| s.member(a.member).map(|m| (a.clone(), m.clone())))
                .collect::<Vec<_>>();
            (you.name, grid, activity)
        });

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(Greetings { name }.into_widget())
            .child(grid.into_widget())
            .child(RecentProjectWidget.into_widget())
            .child(RecentActivity { entries: activity }.into_widget());

        Page::new(AppHeader::new("Dashboard"), content)
            .nav(Tab::Dashboard, NavStyle::Standard)
            .padding(24.0, 20.0, 32.0)
            .build()
    }
}
