use frus::{
    Align, BuildContext, Column, Container, Expanded, FontWeight, Row, StatelessWidget,
    TextOverflow, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, gray, green_tint, grey, light_gray, primary_color, red},
        icons::{icon, Glyph},
        routes::{INVITATIONS, WORKSPACE},
        shared::on,
        ui::{back_link, caption, filled, group, pill, tap, text, title_block, FULL},
    },
    features::settings::widgets::rows::{titled_row, RowStyle},
    models::{activity::InvitationStatus, store},
};

/// The invitations sent, where each stands, and how invitations work.
pub(crate) struct InvitationsPage;

fn stat(value: usize, label: &str) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(4.0)
            .cross_axis_alignment(Align::Start)
            .child(text(
                value.to_string(),
                20.0,
                28.0,
                black(),
                FontWeight::SemiBold,
            ))
            .child(caption(label)),
    )
}

impl StatelessWidget for InvitationsPage {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let invitations = store::read(|s| s.invitations.clone());
        let pending = invitations
            .iter()
            .filter(|i| i.status == InvitationStatus::Pending)
            .count();

        let stats = Container::new()
            .decoration(
                filled(frus::Color::hex("#F9F9FA"), 12.0).border(frus::Border::new(1.0, gray())),
            )
            .padding(17.0)
            .child(
                Row::new()
                    .cross_axis_alignment(Align::Stretch)
                    .child(Expanded::new(
                        Container::new()
                            .box_border(frus::Border {
                                right: frus::BorderSide::new(gray(), 1.0),
                                ..frus::Border::NONE
                            })
                            .child(stat(pending, "pending")),
                    ))
                    .child(Expanded::new(
                        Container::new()
                            .padding_each(0.0, 0.0, 0.0, 16.0)
                            .child(stat(6, "accepted this month")),
                    )),
            );

        let rows = invitations
            .iter()
            .map(|inv| -> Box<dyn Widget> {
                let is_pending = inv.status == InvitationStatus::Pending;
                let (fill, ink) = if is_pending {
                    (green_tint(), primary_color())
                } else {
                    (light_gray(), grey())
                };
                let mut row = Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(
                        Container::new()
                            .width(36.0)
                            .height(36.0)
                            .decoration(filled(fill, FULL))
                            .alignment(frus::Alignment::CENTER)
                            .child(icon(Glyph::Mail, 16.0, ink)),
                    )
                    .child(Expanded::new(
                        Column::new()
                            .cross_axis_alignment(Align::Start)
                            .child(text(
                                inv.email.clone(),
                                14.0,
                                20.0,
                                black(),
                                FontWeight::Medium,
                            ))
                            .child(
                                caption(inv.detail.clone())
                                    .max_lines(1)
                                    .overflow(TextOverflow::Ellipsis),
                            ),
                    ));
                if is_pending {
                    row = row
                        .child(pill(
                            "Pending",
                            green_tint(),
                            primary_color(),
                            10.0,
                            4.0,
                            12.0,
                        ))
                        .child(icon(Glyph::Ellipsis, 20.0, grey()));
                } else {
                    row = row.child(pill("Accepted", light_gray(), grey(), 10.0, 4.0, 12.0));
                }
                Box::new(Container::new().padding(16.0).child(row))
            })
            .collect::<Vec<_>>();

        let revoke = {
            let router = router.clone();
            on(move || {
                store::write(|s| {
                    s.invitations
                        .retain(|i| i.status != InvitationStatus::Pending)
                });
                router.replace(INVITATIONS);
            })
        };

        let mut content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Workspace",
                16.0,
                8.0,
                back(&router, WORKSPACE.to_string()),
            ))
            .child(title_block(
                "Invitations",
                "Track and manage invitations to your workspace.",
                4.0,
            ))
            .child(stats);
        if !rows.is_empty() {
            content = content.child(group(rows));
        }
        content = content
            .child(
                Column::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(text(
                        "Invitation settings",
                        16.0,
                        24.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(group(vec![
                        titled_row(
                            "Default role",
                            "Member",
                            RowStyle::ACCOUNT.with_chevron(20.0),
                            None,
                            on(|| {}),
                        ),
                        titled_row(
                            "Invite link",
                            "Create a shareable link",
                            RowStyle::ACCOUNT.with_chevron(20.0),
                            None,
                            on(|| {}),
                        ),
                    ])),
            )
            .child(
                Row::new().child(tap(
                    Container::new()
                        .padding_each(8.0, 0.0, 0.0, 0.0)
                        .child(text(
                            "Revoke all pending invitations",
                            14.0,
                            20.0,
                            red(),
                            FontWeight::Medium,
                        )),
                    revoke,
                )),
            );

        Page::new(
            AppHeader::new("Invitations").style(HeaderStyle::Workspace),
            content,
        )
        .nav(Tab::Settings, NavStyle::Tall)
        .padding(24.0, 20.0, 24.0)
        .build()
    }
}
