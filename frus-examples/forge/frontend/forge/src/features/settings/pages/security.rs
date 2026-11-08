use frus::{
    Align, Column, Container, Expanded, FontWeight, Row, State, StateContext, StatefulWidget,
    Widget,
};

use crate::{
    common::{
        chrome::{back, AppHeader, HeaderStyle, NavStyle, Page, Tab},
        colors::{black, gray, green_tint, grey, light_gray, primary_color, red, red_wash},
        icons::{icon, Glyph},
        routes::PROFILE,
        shared::on,
        ui::{
            back_link, caption, danger_button, filled, group, tap, text, title_block, toggle, FULL,
        },
    },
    features::settings::{
        states::settings_state::SecurityState,
        widgets::rows::{titled_row, RowStyle},
    },
};

/// The account's password, two-factor authentication and sessions.
pub(crate) struct SecurityPage;

impl StatefulWidget for SecurityPage {
    type State = SecurityState;

    fn create_state(&self) -> SecurityState {
        SecurityState::default()
    }
}

/// A device the account signed in from.
fn session(
    glyph: Glyph,
    current: bool,
    name: &str,
    place: &str,
    trailing: Option<Box<dyn Widget>>,
) -> Box<dyn Widget> {
    let (fill, ink) = if current {
        (green_tint(), primary_color())
    } else {
        (light_gray(), grey())
    };
    let mut details = Column::new()
        .spacing(4.0)
        .cross_axis_alignment(Align::Start)
        .child(text(name, 14.0, 20.0, black(), FontWeight::Medium))
        .child(caption(place));
    if current {
        details = details.child(text(
            "Current session",
            12.0,
            16.0,
            primary_color(),
            FontWeight::Medium,
        ));
    }
    let mut row = Row::new()
        .spacing(12.0)
        .cross_axis_alignment(Align::Start)
        .child(
            Container::new().padding_each(4.0, 0.0, 0.0, 0.0).child(
                Container::new()
                    .width(28.0)
                    .height(28.0)
                    .decoration(filled(fill, FULL))
                    .alignment(frus::Alignment::CENTER)
                    .child(icon(glyph, 16.0, ink)),
            ),
        )
        .child(Expanded::new(details));
    if let Some(trailing) = trailing {
        row = row.child(trailing);
    }
    Box::new(row)
}

impl State for SecurityState {
    type Widget = SecurityPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let router = cx.router();
        let flip = cx.callback(|s: &mut SecurityState| s.two_factor = !s.two_factor);

        let rows = vec![
            titled_row(
                "Change password",
                "Last changed 3 months ago",
                RowStyle::ACCOUNT,
                None,
                on(|| {}),
            ),
            titled_row(
                "Two-factor authentication",
                if self.two_factor {
                    "Enabled"
                } else {
                    "Not enabled"
                },
                RowStyle::ACCOUNT,
                Some(toggle(self.two_factor, flip.clone())),
                flip,
            ),
            titled_row(
                "Active sessions",
                if self.laptop_signed_out || self.everywhere {
                    "1 device"
                } else {
                    "2 devices"
                },
                RowStyle::ACCOUNT,
                None,
                on(|| {}),
            ),
        ];

        let mut sessions = Column::new()
            .spacing(16.0)
            .cross_axis_alignment(Align::Stretch)
            .child(session(
                Glyph::Smartphone,
                true,
                "iPhone 15 Pro",
                "Paris, France · Today at 09:38",
                None,
            ));
        if !self.laptop_signed_out && !self.everywhere {
            sessions = sessions.child(session(
                Glyph::Laptop,
                false,
                "Safari on MacBook Pro",
                "Paris, France · Yesterday at 18:42",
                Some(tap(
                    caption("Sign out"),
                    cx.callback(|s: &mut SecurityState| s.laptop_signed_out = true),
                )),
            ));
        }

        let danger = Container::new()
            .decoration(
                filled(red_wash(), 12.0)
                    .border(frus::Border::new(1.0, gray()))
                    .shadow(frus::BoxShadow::new(
                        0.0,
                        1.0,
                        3.0,
                        frus::Colors::BLACK.with_alpha(0.08),
                    )),
            )
            .padding(17.0)
            .child(
                Column::new()
                    .spacing(16.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(
                        Column::new()
                            .spacing(4.0)
                            .cross_axis_alignment(Align::Start)
                            .child(text(
                                "Sign out of all devices",
                                14.0,
                                20.0,
                                red(),
                                FontWeight::SemiBold,
                            ))
                            .child(caption(if self.everywhere {
                                "Every other session has been ended."
                            } else {
                                "This will end every active Forge session."
                            })),
                    )
                    .child(danger_button(
                        "Sign out everywhere",
                        36.0,
                        6.0,
                        cx.callback(|s: &mut SecurityState| s.everywhere = true),
                    )),
            );

        let content = Column::new()
            .spacing(24.0)
            .cross_axis_alignment(Align::Stretch)
            .child(back_link(
                "Account",
                16.0,
                8.0,
                back(&router, PROFILE.to_string()),
            ))
            .child(title_block(
                "Password & security",
                "Protect your Forge account.",
                4.0,
            ))
            .child(group(rows))
            .child(
                Column::new()
                    .spacing(16.0)
                    .cross_axis_alignment(Align::Stretch)
                    .child(text(
                        "Recent sign-in activity",
                        16.0,
                        24.0,
                        black(),
                        FontWeight::SemiBold,
                    ))
                    .child(sessions),
            )
            .child(danger);

        Page::new(
            AppHeader::new("Security").style(HeaderStyle::Account),
            content,
        )
        .nav(Tab::Settings, NavStyle::Compact)
        .padding(20.0, 20.0, 32.0)
        .build()
    }
}
