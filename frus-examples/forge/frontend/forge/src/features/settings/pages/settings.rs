use frus::{
    Align, Column, Container, Expanded, FontWeight, Row, State, StateContext, StatefulWidget,
    TextAlign, Widget,
};

use crate::{
    common::{
        auth::Auth,
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{black, primary_color, white},
        routes::{LOGIN, PROFILE, WORKSPACE},
        shared::on,
        ui::{
            avatar_colored, caption, card, danger_button, group, muted, section_label, tap, text,
            title_block, toggle,
        },
    },
    features::settings::{
        states::settings_state::SettingsState,
        widgets::rows::{chevron20, value, value_row},
    },
    models::store,
};

/// The settings: the profile, preferences, the workspace and help.
pub(crate) struct SettingsPage {
    pub auth: Auth,
}

impl StatefulWidget for SettingsPage {
    type State = SettingsState;

    fn create_state(&self) -> SettingsState {
        SettingsState {
            notifications: true,
        }
    }
}

/// A section's label, a little inset, over its card.
fn titled(title: &str, rows: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(
        Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Stretch)
            .child(
                Container::new()
                    .padding_each(0.0, 4.0, 0.0, 4.0)
                    .child(section_label(title)),
            )
            .child(group(rows)),
    )
}

impl State for SettingsState {
    type Widget = SettingsPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let auth = cx.widget().auth.clone();
        let (you, members) = store::read(|s| (s.you().clone(), s.members.len()));
        let go = |to: &'static str| {
            let router = router.clone();
            on(move || router.push(to))
        };
        let nothing = || on(|| {});

        let profile = tap(
            card(
                Row::new()
                    .spacing(12.0)
                    .cross_axis_alignment(Align::Center)
                    .child(avatar_colored(
                        &you.letter(),
                        44.0,
                        16.0,
                        primary_color(),
                        white(),
                        FontWeight::SemiBold,
                    ))
                    .child(Expanded::new(
                        Column::new()
                            .spacing(2.0)
                            .cross_axis_alignment(Align::Start)
                            .child(text(
                                you.name.clone(),
                                16.0,
                                24.0,
                                black(),
                                FontWeight::SemiBold,
                            ))
                            .child(muted("kokou@forge.dev")),
                    ))
                    .child(chevron20()),
            ),
            go(PROFILE),
        );

        let sign_out = {
            let router = router.clone();
            on(move || {
                auth.logout();
                router.go(LOGIN);
            })
        };

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(title_block("Settings", "Manage your Forge workspace.", 4.0))
            .child(profile)
            .child(titled(
                "Preferences",
                vec![
                    value_row(
                        "Notifications",
                        vec![toggle(
                            self.notifications,
                            cx.callback(|s: &mut SettingsState| s.notifications = !s.notifications),
                        )],
                        cx.callback(|s: &mut SettingsState| s.notifications = !s.notifications),
                    ),
                    value_row("Appearance", vec![value("System"), chevron20()], nothing()),
                    value_row("Language", vec![value("English"), chevron20()], nothing()),
                ],
            ))
            .child(titled(
                "Workspace",
                vec![
                    value_row(
                        "Workspace members",
                        vec![value(&format!("{} members", members.min(4)))],
                        go(WORKSPACE),
                    ),
                    value_row(
                        "Sync & offline",
                        vec![Box::new(text(
                            "Synced",
                            14.0,
                            20.0,
                            primary_color(),
                            FontWeight::Medium,
                        ))],
                        nothing(),
                    ),
                ],
            ))
            .child(titled(
                "Support",
                vec![
                    value_row("Help center", vec![chevron20()], nothing()),
                    value_row("Send feedback", vec![chevron20()], nothing()),
                    value_row("About Forge", vec![chevron20()], nothing()),
                ],
            ))
            .child(danger_button("Sign out", 44.0, 8.0, sign_out))
            .child(caption("Forge · Learn by building together").align(TextAlign::Center));

        Page::new(AppHeader::new("Settings"), content)
            .nav(Tab::Settings, NavStyle::Standard)
            .padding(24.0, 20.0, 32.0)
            .build()
    }
}
