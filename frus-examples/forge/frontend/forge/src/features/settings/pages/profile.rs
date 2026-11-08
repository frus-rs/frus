use frus::{Align, BuildContext, Column, Container, FontWeight, Row, StatelessWidget, Widget};

use crate::{
    common::{
        auth::Auth,
        chrome::{AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, green_tint, primary_color, red},
        icons::{icon, Glyph},
        routes::{LOGIN, PERSONAL_INFO, SECURITY, SETTINGS},
        shared::on,
        ui::{avatar_colored, group, hug, muted, pill, tap, text, title_block},
    },
    features::settings::widgets::rows::{titled_row, RowStyle},
    models::store,
};

/// The signed-in member: who they are, and the way into their account settings.
pub(crate) struct ProfilePage {
    pub auth: Auth,
}

impl StatelessWidget for ProfilePage {
    fn build(&self, cx: &BuildContext) -> Box<dyn Widget> {
        let router = cx.router();
        let you = store::read(|s| s.you().clone());
        let go = |to: &'static str| {
            let router = router.clone();
            on(move || router.push(to))
        };
        let sign_out = {
            let (router, auth) = (router.clone(), self.auth.clone());
            on(move || {
                auth.logout();
                router.go(LOGIN);
            })
        };

        let identity = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Center)
            .child(avatar_colored(
                &you.letter(),
                72.0,
                24.0,
                green_tint(),
                primary_color(),
                FontWeight::Medium,
            ))
            .child(tap(
                text(
                    "Change photo",
                    14.0,
                    20.0,
                    primary_color(),
                    FontWeight::Medium,
                ),
                go(PERSONAL_INFO),
            ))
            .child(
                Container::new()
                    .padding_each(4.0, 0.0, 0.0, 0.0)
                    .child(text(
                        you.full_name.clone(),
                        18.0,
                        28.0,
                        black(),
                        FontWeight::SemiBold,
                    )),
            )
            .child(muted("kokou@forge.dev"))
            .child(pill(
                "Product designer",
                green_tint(),
                primary_color(),
                12.0,
                4.0,
                12.0,
            ));

        let rows = vec![
            titled_row(
                "Personal information",
                "Name, email and profile photo",
                RowStyle::ACCOUNT,
                None,
                go(PERSONAL_INFO),
            ),
            titled_row(
                "Password & security",
                "Update your password and sign-in methods",
                RowStyle::ACCOUNT,
                None,
                go(SECURITY),
            ),
            titled_row(
                "Notifications",
                "Push, email and project alerts",
                RowStyle::ACCOUNT,
                None,
                go(SETTINGS),
            ),
        ];

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(title_block(
                "Profile",
                "Manage your personal information and account preferences.",
                4.0,
            ))
            .child(identity)
            .child(group(rows))
            .child(
                Row::new()
                    .main_axis_alignment(frus::Justify::Center)
                    .child(tap(
                        Container::new()
                            .padding_each(12.0, 16.0, 12.0, 16.0)
                            .child(hug(
                                8.0,
                                vec![
                                    Box::new(icon(Glyph::LogOut, 16.0, red())),
                                    Box::new(text(
                                        "Sign out",
                                        14.0,
                                        20.0,
                                        red(),
                                        FontWeight::Medium,
                                    )),
                                ],
                            )),
                        sign_out,
                    )),
            );

        Page::new(
            AppHeader::new("Account").style(HeaderStyle::Account),
            content,
        )
        .nav(Tab::Settings, NavStyle::Compact)
        .padding(24.0, 20.0, 32.0)
        .build()
    }
}
