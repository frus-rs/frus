use frus::{Align, BuildContext, Column, FontWeight, Justify, Row, StatelessWidget, Widget};

use crate::{
    common::{
        colors::{grey, primary_color},
        routes::{project_board, PROJECTS},
        shared::on,
        ui::{section_heading, tap, text},
    },
    features::home::pages::widgets::project_tile::ProjectTile,
    models::{project::ProjectStatus, store},
};

/// The projects still under way, newest first, with a link to all of them.
pub(crate) struct RecentProjectWidget;

impl StatelessWidget for RecentProjectWidget {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let view_all = {
            let router = router.clone();
            on(move || router.go(PROJECTS))
        };

        let tiles = store::read(|s| {
            s.projects
                .iter()
                .filter(|p| p.status == ProjectStatus::Active)
                .enumerate()
                .map(|(i, p)| {
                    let avatars = s
                        .members_of(p)
                        .iter()
                        .take(3)
                        .map(|m| (m.letter(), m.is_you))
                        .collect();
                    let router = router.clone();
                    let id = p.id;
                    ProjectTile {
                        title: p.name.clone(),
                        tasks: p.task_count,
                        members: p.member_count,
                        percentage: p.progress,
                        per_color: if i == 0 { primary_color() } else { grey() },
                        bar_color: if i == 0 {
                            primary_color()
                        } else {
                            frus::Color::hex("#A170EB")
                        },
                        avatars,
                        on_tap: on(move || router.push(project_board(id))),
                    }
                })
                .collect::<Vec<_>>()
        });

        let mut list = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch);
        for tile in tiles {
            list = list.child(tile.into_widget());
        }

        Box::new(
            Column::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Stretch)
                .child(
                    Row::new()
                        .main_axis_alignment(Justify::SpaceBetween)
                        .cross_axis_alignment(Align::Center)
                        .child(section_heading("Recent projects"))
                        .child(tap(
                            text("View all", 12.0, 16.0, primary_color(), FontWeight::Regular),
                            view_all,
                        )),
                )
                .child(list),
        )
    }
}
