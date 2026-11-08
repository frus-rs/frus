use frus::{
    Align, BuildContext, Column, Container, Expanded, FontWeight, Justify, Row, StatelessWidget,
    Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, NavStyle, Page, Tab},
        colors::{black, grey, primary_color},
        icons::{icon, Glyph},
        routes::PROJECTS,
        shared::on,
        ui::{
            avatar, back_link, caption, card, gap, green_tag, group, outlined, page_title,
            section_heading, tag, tap, text, two_lines,
        },
    },
    features::{
        home::pages::widgets::project_tile::progress_bar,
        projects::{pages::board::missing, widgets::activity_text::sentence},
    },
    models::{project::ProjectStatus, store},
};

/// A project at a glance: its completion, its figures, its description and its activity.
pub(crate) struct OverviewPage {
    pub project: u32,
}

/// A figure in an outlined box: "4 / Members".
fn figure(value: usize, label: &str) -> Box<dyn Widget> {
    Box::new(Expanded::new(
        Container::new()
            .decoration(outlined(12.0))
            .padding(17.0)
            .child(
                Column::new()
                    .spacing(4.0)
                    .cross_axis_alignment(Align::Start)
                    .child(text(
                        value.to_string(),
                        24.0,
                        32.0,
                        black(),
                        FontWeight::Bold,
                    ))
                    .child(text(label, 14.0, 20.0, grey(), FontWeight::Regular)),
            ),
    ))
}

impl StatelessWidget for OverviewPage {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let Some((project, activity)) = store::read(|s| {
            let project = s.project(self.project)?.clone();
            let activity = s
                .activity_of(project.id)
                .into_iter()
                .filter(|a| a.day == "Today")
                .filter_map(|a| s.member(a.member).map(|m| (a, m.clone())))
                .collect::<Vec<_>>();
            Some((project, activity))
        }) else {
            return missing("Project");
        };

        let status = match project.status {
            ProjectStatus::Active => {
                green_tag(project.status.label()).padding_each(4.0, 10.0, 4.0, 10.0)
            }
            _ => tag(project.status.label()).padding_each(4.0, 10.0, 4.0, 10.0),
        };

        let completion = card(
            Column::new()
                .spacing(16.0)
                .cross_axis_alignment(Align::Stretch)
                .child(
                    Row::new()
                        .main_axis_alignment(Justify::SpaceBetween)
                        .cross_axis_alignment(Align::Center)
                        .child(text("Completion", 14.0, 20.0, grey(), FontWeight::Regular))
                        .child(text(
                            format!("{}%", project.progress),
                            18.0,
                            28.0,
                            primary_color(),
                            FontWeight::SemiBold,
                        )),
                )
                .child(progress_bar(
                    project.progress as f32 / 100.0,
                    8.0,
                    primary_color(),
                ))
                .child(text(
                    format!(
                        "{} tasks · {} members",
                        project.task_count, project.member_count
                    ),
                    14.0,
                    20.0,
                    grey(),
                    FontWeight::Regular,
                )),
        );

        let figures = Column::new()
            .spacing(12.0)
            .child(
                Row::new()
                    .spacing(12.0)
                    .child(figure(project.member_count, "Members"))
                    .child(figure(project.task_count, "Total tasks")),
            )
            .child(
                Row::new()
                    .spacing(12.0)
                    .child(figure(project.in_progress, "In progress"))
                    .child(figure(project.due_soon, "Due soon")),
            );

        let settings = tap(
            Container::new()
                .decoration(outlined(12.0))
                .padding_each(16.0, 16.0, 16.0, 16.0)
                .child(
                    Row::new()
                        .spacing(12.0)
                        .cross_axis_alignment(Align::Center)
                        .main_axis_size(frus::MainAxisSize::Min)
                        .child(icon(Glyph::Settings, 20.0, grey()))
                        .child(text(
                            "Project settings",
                            14.0,
                            20.0,
                            black(),
                            FontWeight::SemiBold,
                        ))
                        .child(icon(Glyph::ChevronRight, 16.0, black())),
                ),
            on(|| {}),
        );

        let rows = activity
            .iter()
            .map(|(entry, member)| -> Box<dyn Widget> {
                Box::new(
                    Container::new().padding_each(16.0, 16.0, 16.0, 16.0).child(
                        Row::new()
                            .spacing(12.0)
                            .cross_axis_alignment(Align::Center)
                            .child(avatar(&member.letter(), 32.0, 12.0, member.is_you))
                            .child(Expanded::new(two_lines(
                                text(
                                    sentence(entry, member, false),
                                    14.0,
                                    20.0,
                                    black(),
                                    FontWeight::Regular,
                                ),
                                caption(entry.ago.clone()),
                                2.0,
                            ))),
                    ),
                )
            })
            .collect();

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Projects",
                16.0,
                8.0,
                back(&router, PROJECTS.to_string()),
            ))
            .child(gap(24.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Center)
                    .child(page_title(project.name.clone()))
                    .child(status),
            )
            .child(gap(24.0))
            .child(completion)
            .child(gap(24.0))
            .child(figures)
            .child(gap(32.0))
            .child(section_heading("Project description"))
            .child(gap(12.0))
            .child(text(
                project.description.clone(),
                14.0,
                24.0,
                grey(),
                FontWeight::Regular,
            ))
            .child(gap(24.0))
            .child(Row::new().child(settings))
            .child(gap(32.0))
            .child(section_heading("Recent activity"))
            .child(gap(12.0))
            .child(group(rows));

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
