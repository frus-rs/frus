use frus::{
    Align, Column, Container, Expanded, FontWeight, KeyboardType, Row, State, StateContext,
    StatefulWidget, TextAlign, Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, green_tint, grey, primary_color},
        icons::{icon, Glyph},
        routes::PROFILE,
        shared::on,
        ui::{
            avatar_colored, back_link, body, cancel, caption, outline_button, outlined,
            primary_button, tap, text, title_block, Field,
        },
    },
    features::settings::states::settings_state::PersonalInfoState,
    models::store,
};

/// Edit the signed-in member's name, email, job and time zone.
pub(crate) struct PersonalInfoPage;

impl StatefulWidget for PersonalInfoPage {
    type State = PersonalInfoState;

    fn create_state(&self) -> PersonalInfoState {
        let you = store::read(|s| s.you().clone());
        PersonalInfoState {
            name: you.full_name,
            email: "kokou@forge.dev".into(),
            job: "Product designer".into(),
            saved: false,
        }
    }
}

/// A field's label, `14/20` medium, over it.
fn field(label: &str, input: Box<dyn Widget>) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(text(label, 14.0, 20.0, black(), FontWeight::Medium))
            .child(input),
    )
}

impl State for PersonalInfoState {
    type Widget = PersonalInfoPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let you = store::read(|s| s.you().clone());

        let save = {
            let router = router.clone();
            cx.callback(move |s: &mut PersonalInfoState| {
                let name = s.name.trim().to_string();
                if name.is_empty() {
                    return;
                }
                store::write(|store| {
                    if let Some(m) = store.members.iter_mut().find(|m| m.is_you) {
                        m.full_name = name.clone();
                        m.name = name.clone();
                    }
                });
                s.saved = true;
                if !router.pop() {
                    router.go(PROFILE);
                }
            })
        };

        let timezone = tap(
            Container::new()
                .height(48.0)
                .decoration(outlined(8.0))
                .padding_each(0.0, 13.0, 0.0, 13.0)
                .child(
                    Row::new()
                        .cross_axis_alignment(Align::Center)
                        .child(Expanded::new(body("Europe/Paris")))
                        .child(icon(Glyph::ChevronRight, 16.0, grey())),
                ),
            on(|| {}),
        );

        let content =
            Column::new()
                .spacing(24.0)
                .cross_axis_alignment(Align::Stretch)
                .child(back_link(
                    "Account",
                    16.0,
                    8.0,
                    back(&router, PROFILE.to_string()),
                ))
                .child(title_block(
                    "Personal information",
                    "Keep your profile details up to date.",
                    8.0,
                ))
                .child(
                    Row::new()
                        .spacing(16.0)
                        .cross_axis_alignment(Align::Center)
                        .child(avatar_colored(
                            &you.letter(),
                            64.0,
                            20.0,
                            green_tint(),
                            primary_color(),
                            FontWeight::Medium,
                        ))
                        .child(outline_button(
                            Box::new(Container::new().padding_each(0.0, 17.0, 0.0, 17.0).child(
                                text("Change photo", 14.0, 20.0, black(), FontWeight::Medium),
                            )),
                            36.0,
                            8.0,
                            on(|| {}),
                        )),
                )
                .child(
                    Column::new()
                        .spacing(16.0)
                        .cross_axis_alignment(Align::Stretch)
                        .child(field(
                            "Full name",
                            Field::new(
                                &self.name,
                                "Your name",
                                cx.handler(PersonalInfoState::set_name),
                            )
                            .radius(8.0)
                            .font(16.0)
                            .build(),
                        ))
                        .child(field(
                            "Email address",
                            Field::new(
                                &self.email,
                                "you@example.com",
                                cx.handler(PersonalInfoState::set_email),
                            )
                            .radius(8.0)
                            .font(16.0)
                            .keyboard(KeyboardType::Email)
                            .build(),
                        ))
                        .child(field(
                            "Job title",
                            Field::new(
                                &self.job,
                                "What you do",
                                cx.handler(PersonalInfoState::set_job),
                            )
                            .radius(8.0)
                            .font(16.0)
                            .build(),
                        ))
                        .child(field("Time zone", timezone)),
                )
                .child(
                    Column::new()
                        .spacing(4.0)
                        .cross_axis_alignment(Align::Stretch)
                        .child(primary_button("Save changes", 48.0, 8.0, save))
                        .child(cancel(back(&router, PROFILE.to_string()))),
                )
                .child(
                    caption("Your email is used for account notifications.")
                        .align(TextAlign::Center),
                );

        Page::new(
            AppHeader::new("Edit profile").style(HeaderStyle::Account),
            content,
        )
        .nav(Tab::Settings, NavStyle::Compact)
        .padding(24.0, 24.0, 32.0)
        .build()
    }
}
