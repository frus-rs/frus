use frus::{
    Align, Column, Container, Expanded, FontWeight, Justify, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{black, grey},
        icons::{icon, Glyph},
        routes::INVITE,
        shared::on,
        ui::{
            avatar, caption, card, gap, outlined, presence, section_heading, small_primary, text,
            title_block, two_lines, Field,
        },
    },
    features::projects::{
        pages::board::missing, states::members_state::ProjectMembersState,
        widgets::project_nav::back_to_project,
    },
    models::{member::Member, store},
};

/// Who works on a project, and what they may do in it.
pub(crate) struct ProjectMembersPage {
    pub project: u32,
}

impl StatefulWidget for ProjectMembersPage {
    type State = ProjectMembersState;

    fn create_state(&self) -> ProjectMembersState {
        ProjectMembersState::default()
    }
}

/// A member as a card: avatar, name and job, presence, a menu.
pub(crate) fn member_card(member: &Member) -> Box<dyn Widget> {
    Box::new(
        Container::new()
            .decoration(outlined(12.0))
            .padding_each(20.0, 17.0, 20.0, 17.0)
            .child(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(avatar(&member.letter(), 40.0, 14.0, member.is_you))
                    .child(Expanded::new(two_lines(
                        text(
                            member.name.clone(),
                            14.0,
                            20.0,
                            black(),
                            FontWeight::SemiBold,
                        ),
                        caption(member.title.clone()),
                        0.0,
                    )))
                    .child(presence(member.presence, grey()))
                    .child(
                        Container::new()
                            .width(36.0)
                            .height(36.0)
                            .alignment(frus::Alignment::CENTER)
                            .child(icon(Glyph::Ellipsis, 20.0, grey())),
                    ),
            ),
    )
}

impl State for ProjectMembersState {
    type Widget = ProjectMembersPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let project_id = cx.widget().project;
        let query = self.query.to_lowercase();
        let Some((project, members)) = store::read(|s| {
            let project = s.project(project_id)?.clone();
            let members = s.members_of(&project);
            Some((project, members))
        }) else {
            return missing("Project");
        };
        let count = members.len();
        let shown = members
            .iter()
            .filter(|m| query.is_empty() || m.full_name.to_lowercase().contains(&query))
            .collect::<Vec<_>>();

        let invite = {
            let router = router.clone();
            on(move || router.push(INVITE))
        };

        let mut list = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch);
        for member in shown {
            list = list.child(member_card(member));
        }

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(back_to_project(&router, project.id, &project.name))
            .child(gap(24.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Start)
                    .child(title_block(
                        "Members",
                        &format!("{count} people working on this project"),
                        4.0,
                    ))
                    .child(small_primary(Glyph::Plus, "Invite", 40.0, 16.0, invite)),
            )
            .child(gap(24.0))
            .child(
                Field::new(
                    &self.query,
                    "Search members",
                    cx.handler(ProjectMembersState::set_query),
                )
                .height(44.0)
                .radius(8.0)
                .lead(Glyph::Search)
                .build(),
            )
            .child(gap(24.0))
            .child(
                Row::new()
                    .main_axis_alignment(Justify::SpaceBetween)
                    .cross_axis_alignment(Align::Center)
                    .child(text("Members", 16.0, 24.0, black(), FontWeight::SemiBold))
                    .child(caption(format!("{count} people"))),
            )
            .child(gap(12.0))
            .child(list)
            .child(gap(24.0))
            .child(section_heading("Project access").size(16.0).height(1.5))
            .child(gap(12.0))
            .child(card(text(
                "Members can view, edit, and comment on project tasks.",
                14.0,
                24.0,
                grey(),
                FontWeight::Regular,
            )));

        Page::new(AppHeader::new(project.name.clone()), content)
            .nav(Tab::Projects, NavStyle::Standard)
            .padding(20.0, 20.0, 32.0)
            .build()
    }
}
