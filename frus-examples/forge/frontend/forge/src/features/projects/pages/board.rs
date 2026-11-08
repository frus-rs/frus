use frus::{
    Align, Axis, BuildContext, Column, Container, FontWeight, Row, SingleChildScrollView,
    StatelessWidget, Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{green_tint, primary_color},
        routes::task,
        shared::on,
        ui::{dot, filled, gap, line, muted, text},
    },
    features::projects::widgets::{
        board_column::BoardColumn,
        project_nav::{back_and_new_task, project_tabs},
    },
    models::{store, task::TaskStatus},
};

/// A project's board: its tasks in a column per status, side by side.
pub(crate) struct BoardPage {
    pub project: u32,
}

impl StatelessWidget for BoardPage {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let Some((project, columns, online)) = store::read(|s| {
            let project = s.project(self.project)?.clone();
            let tasks = s.tasks_of(project.id);
            let columns = TaskStatus::ALL
                .iter()
                .map(|status| {
                    let cards = tasks
                        .iter()
                        .filter(|t| t.status == *status)
                        .map(|t| {
                            let router = router.clone();
                            let id = t.id;
                            (
                                t.clone(),
                                t.assignee.and_then(|m| s.member(m)).cloned(),
                                on(move || router.push(task(id))),
                            )
                        })
                        .collect();
                    BoardColumn {
                        title: status.label().to_string(),
                        cards,
                    }
                })
                .collect::<Vec<_>>();
            let online = project.member_count;
            Some((project, columns, online))
        }) else {
            return missing("Project");
        };

        let live = Container::new()
            .decoration(
                filled(green_tint(), 8.0)
                    .border(frus::Border::new(1.0, frus::Color::hex("#B9F8CF"))),
            )
            .padding_each(10.0, 12.0, 10.0, 12.0)
            .child(line(
                8.0,
                vec![
                    dot(primary_color(), 8.0),
                    Box::new(text(
                        format!("Live · {online} members online"),
                        12.0,
                        16.0,
                        primary_color(),
                        FontWeight::Regular,
                    )),
                ],
            ));

        let mut board = Row::new().spacing(16.0).cross_axis_alignment(Align::Start);
        for column in columns {
            board = board.child(column.into_widget());
        }

        let side = |child: Box<dyn Widget>| {
            Container::new()
                .padding_each(0.0, 20.0, 0.0, 20.0)
                .child(child)
        };
        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(side(back_and_new_task(&router, project.id, &project.name)))
            .child(gap(16.0))
            .child(side(project_tabs(&router, project.id, 0)))
            .child(gap(16.0))
            .child(side(Box::new(live)))
            .child(gap(20.0))
            .child(
                SingleChildScrollView::new()
                    .axis(Axis::Horizontal)
                    .padding_each(0.0, 20.0, 0.0, 20.0)
                    .child(board),
            );

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 0.0, 32.0)
            .build()
    }
}

/// What a page shows when the project or task in its address does not exist.
pub(crate) fn missing(what: &str) -> Box<dyn Widget> {
    Page::new(
        AppHeader::new("Forge"),
        Column::new()
            .cross_axis_alignment(Align::Center)
            .child(gap(80.0))
            .child(muted(format!("{what} not found."))),
    )
    .nav(Tab::Projects, NavStyle::Standard)
    .build()
}
