use frus::{
    Align, Column, Container, Expanded, FontWeight, Row, State, StateContext, StatefulWidget,
    Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, green_tint, grey, light_gray, primary_color},
        icons::{icon, Glyph},
        routes::{INVITE, WORKSPACE},
        shared::on,
        ui::{
            avatar_colored, back_link, caption, chips, dot, group, hug, primary_button_with, text,
            title_block, Field,
        },
    },
    features::settings::states::settings_state::WorkspaceMembersState,
    models::{
        activity::InvitationStatus,
        member::{Member, Presence, WorkspaceRole},
        store,
    },
};

/// Everyone in the workspace, by role.
pub(crate) struct WorkspaceMembersPage;

impl StatefulWidget for WorkspaceMembersPage {
    type State = WorkspaceMembersState;

    fn create_state(&self) -> WorkspaceMembersState {
        WorkspaceMembersState::default()
    }
}

fn member_row(m: &Member) -> Box<dyn Widget> {
    let (fill, ink) = if m.is_you {
        (green_tint(), primary_color())
    } else {
        (light_gray(), black())
    };
    let presence_label = if m.presence == Presence::Online {
        primary_color()
    } else {
        grey()
    };
    let presence_dot = if m.presence == Presence::Offline {
        grey()
    } else {
        m.presence.color()
    };
    Box::new(
        Container::new().padding(16.0).child(
            Row::new()
                .spacing(12.0)
                .cross_axis_alignment(Align::Center)
                .child(avatar_colored(
                    &m.letter(),
                    40.0,
                    14.0,
                    fill,
                    ink,
                    FontWeight::SemiBold,
                ))
                .child(Expanded::new(
                    Column::new()
                        .cross_axis_alignment(Align::Start)
                        .child(text(
                            m.full_name.clone(),
                            14.0,
                            20.0,
                            black(),
                            FontWeight::SemiBold,
                        ))
                        .child(caption(m.title.clone()))
                        .child(Container::new().padding_each(3.0, 0.0, 0.0, 0.0).child(hug(
                            4.0,
                            vec![
                                dot(presence_dot, 8.0),
                                Box::new(text(
                                    m.presence.label(),
                                    12.0,
                                    16.0,
                                    presence_label,
                                    FontWeight::Regular,
                                )),
                            ],
                        ))),
                ))
                .child(
                    Container::new()
                        .padding(8.0)
                        .child(icon(Glyph::Ellipsis, 20.0, grey())),
                ),
        ),
    )
}

impl State for WorkspaceMembersState {
    type Widget = WorkspaceMembersPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let query = self.query.to_lowercase();
        let role = self.role;
        let (members, pending) = store::read(|s| {
            (
                s.members.clone(),
                s.invitations
                    .iter()
                    .filter(|i| i.status == InvitationStatus::Pending)
                    .count(),
            )
        });
        let admins = members
            .iter()
            .filter(|m| m.role != WorkspaceRole::Member)
            .count();
        let plain = members.len() - admins;

        let rows = members
            .iter()
            .filter(|m| match role {
                1 => m.role != WorkspaceRole::Member,
                2 => m.role == WorkspaceRole::Member,
                _ => true,
            })
            .filter(|m| query.is_empty() || m.full_name.to_lowercase().contains(&query))
            .map(member_row)
            .collect::<Vec<_>>();

        let invite = {
            let router = router.clone();
            on(move || router.push(INVITE))
        };

        let mut list = Column::new()
            .spacing(12.0)
            .cross_axis_alignment(Align::Stretch)
            .child(caption(format!(
                "{} members · {pending} pending invitation{}",
                members.len(),
                if pending == 1 { "" } else { "s" }
            )));
        if !rows.is_empty() {
            list = list.child(group(rows));
        }

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Workspace",
                16.0,
                4.0,
                back(&router, WORKSPACE.to_string()),
            ))
            .child(title_block(
                "Members",
                "Manage people and access in Forge Studio.",
                8.0,
            ))
            .child(primary_button_with(
                Some(Glyph::UserPlus),
                "Invite members",
                48.0,
                8.0,
                invite,
            ))
            .child(
                Field::new(
                    &self.query,
                    "Search members",
                    cx.handler(|s: &mut WorkspaceMembersState, v: String| s.query = v),
                )
                .height(44.0)
                .radius(8.0)
                .lead(Glyph::Search)
                .build(),
            )
            .child(chips(
                &[
                    format!("All {}", members.len()),
                    format!("Admins {admins}"),
                    format!("Members {plain}"),
                ],
                self.role,
                |i| cx.callback(move |s: &mut WorkspaceMembersState| s.role = i),
            ))
            .child(list);

        Page::new(
            AppHeader::new("Members").style(HeaderStyle::Workspace),
            content,
        )
        .nav(Tab::Settings, NavStyle::Tall)
        .padding(24.0, 20.0, 24.0)
        .build()
    }
}
