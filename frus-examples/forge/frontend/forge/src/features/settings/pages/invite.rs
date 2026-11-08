use frus::{
    Align, Column, Container, FontWeight, GestureDetector, KeyboardType, Row, State, StateContext,
    StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, grey, light_gray, primary_color},
        icons::{icon, Glyph},
        routes::{INVITATIONS, WORKSPACE_MEMBERS},
        ui::{
            back_link, banner_large, cancel, filled, outlined, primary_button, tap, text,
            title_block, Field, FULL,
        },
    },
    features::settings::states::settings_state::InviteState,
    models::{
        activity::{Invitation, InvitationStatus},
        store,
    },
};

/// Invite people into the workspace by email.
pub(crate) struct InvitePage;

impl StatefulWidget for InvitePage {
    type State = InviteState;

    fn create_state(&self) -> InviteState {
        InviteState::default()
    }
}

fn label(text_: &str) -> frus::Text {
    text(text_, 14.0, 20.0, black(), FontWeight::SemiBold)
}

fn helper(text_: &str) -> frus::Text {
    text(text_, 12.0, 20.0, grey(), FontWeight::Regular)
}

impl State for InviteState {
    type Widget = InvitePage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();

        let mut chips = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
        for (i, email) in self.emails.iter().enumerate() {
            chips = chips.child(
                Container::new()
                    .decoration(filled(light_gray(), FULL))
                    .padding_each(8.0, 12.0, 8.0, 12.0)
                    .child(
                        Row::new()
                            .spacing(8.0)
                            .main_axis_size(frus::MainAxisSize::Min)
                            .cross_axis_alignment(Align::Center)
                            .child(text(
                                email.clone(),
                                14.0,
                                20.0,
                                black(),
                                FontWeight::Regular,
                            ))
                            .child(GestureDetector::new(icon(Glyph::X, 16.0, grey())).on_tap(
                                cx.callback(move |s: &mut InviteState| {
                                    s.emails.remove(i);
                                }),
                            )),
                    ),
            );
        }

        let role = tap(
            Container::new()
                .decoration(outlined(8.0))
                .padding_each(8.0, 17.0, 8.0, 17.0)
                .child(
                    Row::new()
                        .spacing(4.0)
                        .main_axis_size(frus::MainAxisSize::Min)
                        .cross_axis_alignment(Align::Center)
                        .child(text(
                            if self.admin { "Admin" } else { "Member" },
                            14.0,
                            20.0,
                            black(),
                            FontWeight::Regular,
                        ))
                        .child(icon(Glyph::ChevronDown, 16.0, grey())),
                ),
            cx.callback(|s: &mut InviteState| s.admin = !s.admin),
        );

        let send = {
            let router = router.clone();
            cx.callback(move |s: &mut InviteState| {
                let typed = format!("{},{}", s.email, s.another);
                s.take_emails(&typed);
                if s.emails.is_empty() {
                    return;
                }
                let emails = s.emails.clone();
                store::write(|store| {
                    let you = store.you().full_name.clone();
                    for email in emails {
                        if store.invitations.iter().any(|i| i.email == email) {
                            continue;
                        }
                        store.invitations.insert(
                            0,
                            Invitation {
                                email,
                                detail: format!("Invited by {you} · today"),
                                status: InvitationStatus::Pending,
                            },
                        );
                    }
                });
                router.replace(INVITATIONS);
            })
        };

        let form = Column::new()
            .spacing(16.0)
            .cross_axis_alignment(Align::Stretch)
            .child(
                Column::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(label("Email addresses"))
                    .child(
                        Field::new(
                            &self.email,
                            "name@company.com",
                            cx.handler(|s: &mut InviteState, v: String| {
                                if v.ends_with(',') {
                                    s.take_emails(&v);
                                    s.email.clear();
                                } else {
                                    s.email = v;
                                }
                            }),
                        )
                        .radius(8.0)
                        .keyboard(KeyboardType::Email)
                        .on_submit(cx.callback(|s: &mut InviteState| {
                            let typed = s.email.clone();
                            s.take_emails(&typed);
                        }))
                        .build(),
                    )
                    .child(helper(
                        "Add one or more email addresses separated by commas.",
                    ))
                    .child(chips)
                    .child(
                        Field::new(
                            &self.another,
                            "Add another email",
                            cx.handler(|s: &mut InviteState, v: String| s.another = v),
                        )
                        .radius(8.0)
                        .keyboard(KeyboardType::Email)
                        .on_submit(cx.callback(|s: &mut InviteState| {
                            let typed = s.another.clone();
                            s.take_emails(&typed);
                            s.another.clear();
                        }))
                        .build(),
                    ),
            )
            .child(
                Column::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(label("Role"))
                    .child(Row::new().child(role))
                    .child(helper(if self.admin {
                        "Admins can manage members and settings."
                    } else {
                        "Members can create and update tasks."
                    })),
            )
            .child(
                Column::new()
                    .spacing(8.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(label("Message (optional)"))
                    .child(
                        Field::new(
                            &self.message,
                            "Say hello",
                            cx.handler(|s: &mut InviteState, v: String| s.message = v),
                        )
                        .radius(8.0)
                        .height(112.0)
                        .lines(4)
                        .build(),
                    ),
            )
            .child(banner_large(
                Box::new(icon(Glyph::Users, 20.0, primary_color())),
                "Invites expire after 7 days.",
                16.0,
                16.0,
            ));

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Members",
                16.0,
                4.0,
                back(&router, WORKSPACE_MEMBERS.to_string()),
            ))
            .child(title_block(
                "Invite members",
                "Bring your team into Forge Studio.",
                8.0,
            ))
            .child(form)
            .child(
                Column::new()
                    .spacing(4.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(primary_button("Send invitations", 48.0, 8.0, send))
                    .child(cancel(back(&router, WORKSPACE_MEMBERS.to_string()))),
            );

        Page::new(
            AppHeader::new("Invite").style(HeaderStyle::Workspace),
            content,
        )
        .nav(Tab::Settings, NavStyle::Tall)
        .padding(24.0, 20.0, 24.0)
        .build()
    }
}
