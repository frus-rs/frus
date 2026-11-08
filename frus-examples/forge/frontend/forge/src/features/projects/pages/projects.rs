use frus::{Align, Column, State, StateContext, StatefulWidget, StatelessWidget, Widget};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::grey,
        icons::{icon, Glyph},
        routes::{project_board, NEW_PROJECT},
        shared::on,
        ui::{caption, chips, gap, hug, primary_button_with, text, title_block, Field},
    },
    features::projects::{
        states::projects_state::ProjectsState, widgets::project_card::ProjectCard,
    },
    models::{project::ProjectStatus, store},
};

/// Every project, searchable and filtered by status.
pub(crate) struct ProjectsPage;

impl StatefulWidget for ProjectsPage {
    type State = ProjectsState;

    fn create_state(&self) -> ProjectsState {
        ProjectsState::default()
    }
}

impl State for ProjectsState {
    type Widget = ProjectsPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let query = self.query.to_lowercase();
        let filter = self.filter;

        let cards = store::read(|s| {
            s.projects
                .iter()
                .filter(|p| match filter {
                    1 => p.status == ProjectStatus::Active,
                    2 => p.status != ProjectStatus::Active,
                    _ => true,
                })
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
                .map(|p| {
                    let router = router.clone();
                    let id = p.id;
                    ProjectCard {
                        project: p.clone(),
                        letters: s.members_of(p).iter().take(3).map(|m| m.letter()).collect(),
                        on_tap: on(move || router.push(project_board(id))),
                    }
                })
                .collect::<Vec<_>>()
        });

        let count = cards.len();
        let mut list = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch);
        for card in cards {
            list = list.child(card.into_widget());
        }

        let new_project = {
            let router = router.clone();
            on(move || router.push(NEW_PROJECT))
        };

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(title_block(
                "Projects",
                "Stay on top of the work that matters.",
                4.0,
            ))
            .child(gap(24.0))
            .child(
                Field::new(
                    &self.query,
                    "Search projects",
                    cx.handler(ProjectsState::set_query),
                )
                .height(44.0)
                .radius(12.0)
                .lead(Glyph::Search)
                .build(),
            )
            .child(gap(24.0))
            .child(chips(
                &[
                    "All".to_string(),
                    "Active".to_string(),
                    "Archived".to_string(),
                ],
                self.filter,
                |i| cx.callback(move |s: &mut ProjectsState| s.filter = i),
            ))
            .child(gap(24.0))
            .child(primary_button_with(
                Some(Glyph::Plus),
                "New project",
                48.0,
                12.0,
                new_project,
            ))
            .child(gap(24.0))
            .child(text(
                format!("{count} project{}", if count == 1 { "" } else { "s" }),
                14.0,
                20.0,
                grey(),
                frus::FontWeight::SemiBold,
            ))
            .child(gap(12.0))
            .child(list)
            .child(gap(24.0))
            .child(hug(
                8.0,
                vec![
                    Box::new(icon(Glyph::Cloud, 16.0, grey())),
                    Box::new(caption("Changes sync automatically")),
                ],
            ));

        Page::new(AppHeader::new("Projects"), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .build()
    }
}
