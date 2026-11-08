use frus::{
    Align, BuildContext, Column, Container, Expanded, FontWeight, Row, StatelessWidget, TextAlign,
    Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, primary_color, white},
        routes::{INVITATIONS, SETTINGS, WORKSPACE_MEMBERS},
        shared::on,
        ui::{back_link, caption, filled, group, muted, section_label, tap, text, title_block},
    },
    features::settings::widgets::rows::{chevron20, titled_row, RowStyle},
    models::{activity::InvitationStatus, store},
};

/// The workspace: its plan, its people and its defaults.
pub(crate) struct WorkspacePage;

impl StatelessWidget for WorkspacePage {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let (members, pending) = store::read(|s| {
            (
                s.members.len(),
                s.invitations
                    .iter()
                    .filter(|i| i.status == InvitationStatus::Pending)
                    .count(),
            )
        });
        let go = |to: &'static str| {
            let router = router.clone();
            on(move || router.push(to))
        };
        let nothing = || on(|| {});

        let studio = tap(
            Container::new()
                .decoration(crate::common::ui::outlined(12.0))
                .padding(17.0)
                .child(
                    Row::new()
                        .spacing(12.0)
                        .cross_axis_alignment(Align::Center)
                        .child(
                            Container::new()
                                .width(44.0)
                                .height(44.0)
                                .decoration(filled(primary_color(), 8.0))
                                .alignment(frus::Alignment::CENTER)
                                .child(text("F", 18.0, 28.0, white(), FontWeight::SemiBold)),
                        )
                        .child(Expanded::new(
                            Column::new()
                                .spacing(4.0)
                                .cross_axis_alignment(Align::Start)
                                .child(text(
                                    "Forge Studio",
                                    16.0,
                                    24.0,
                                    black(),
                                    FontWeight::SemiBold,
                                ))
                                .child(muted("Pro plan")),
                        ))
                        .child(chevron20()),
                ),
            nothing(),
        );

        let people = vec![
            titled_row(
                "Members",
                &format!("{members} people"),
                RowStyle::WORKSPACE,
                None,
                go(WORKSPACE_MEMBERS),
            ),
            titled_row(
                "Invitations",
                &format!("{pending} pending"),
                RowStyle::WORKSPACE,
                None,
                go(INVITATIONS),
            ),
            titled_row(
                "Roles & permissions",
                "Control access levels",
                RowStyle::WORKSPACE,
                None,
                nothing(),
            ),
            titled_row(
                "Billing",
                "Next invoice on Apr 30",
                RowStyle::WORKSPACE,
                None,
                nothing(),
            ),
        ];
        let preferences = vec![
            titled_row(
                "Default project view",
                "Board",
                RowStyle::WORKSPACE,
                None,
                nothing(),
            ),
            titled_row(
                "Week starts on",
                "Monday",
                RowStyle::WORKSPACE,
                None,
                nothing(),
            ),
            titled_row("Appearance", "System", RowStyle::WORKSPACE, None, nothing()),
        ];

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Settings",
                16.0,
                4.0,
                back(&router, SETTINGS.to_string()),
            ))
            .child(title_block(
                "Workspace",
                "Manage your team and workspace defaults.",
                4.0,
            ))
            .child(studio)
            .child(group(people))
            .child(
                Column::new()
                    .spacing(16.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(section_label("Workspace preferences"))
                    .child(group(preferences)),
            )
            .child(caption("Workspace ID: forge-studio-8F2A").align(TextAlign::Center));

        Page::new(
            AppHeader::new("Workspace").style(HeaderStyle::Workspace),
            content,
        )
        .nav(Tab::Settings, NavStyle::Tall)
        .padding(24.0, 20.0, 24.0)
        .build()
    }
}
