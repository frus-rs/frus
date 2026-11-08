use frus::{
    Align, Column, Container, FontWeight, Justify, Row, State, StateContext, StatefulWidget, Widget,
};

use crate::{
    common::{
        chrome::{AppHeader, NavStyle, Page, Tab},
        colors::{grey, primary_color},
        icons::{icon, Glyph},
        ui::{filled, gap, hug, muted, tap, text, title_block, tracked, underline_tabs},
    },
    features::notifications::states::notifications_state::NotificationsState,
};

/// What happened that concerns the signed-in member. Everything has been read: the page
/// says so.
pub(crate) struct NotificationsPage;

impl StatefulWidget for NotificationsPage {
    type State = NotificationsState;

    fn create_state(&self) -> NotificationsState {
        NotificationsState::default()
    }
}

impl State for NotificationsState {
    type Widget = NotificationsPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let header = Row::new()
            .main_axis_alignment(Justify::SpaceBetween)
            .cross_axis_alignment(Align::Start)
            .child(title_block(
                "Notifications",
                "Stay in the loop with your team.",
                4.0,
            ))
            .child(tap(
                Container::new()
                    .padding_each(4.0, 0.0, 4.0, 0.0)
                    .child(text(
                        "Mark all as read",
                        12.0,
                        16.0,
                        primary_color(),
                        FontWeight::Regular,
                    )),
                cx.callback(|s: &mut NotificationsState| s.all_read = true),
            ));

        let empty = Column::new()
            .spacing(8.0)
            .cross_axis_alignment(Align::Center)
            .child(icon(Glyph::CircleCheck, 20.0, primary_color()))
            .child(muted(match self.tab {
                1 => "No unread notifications",
                2 => "No mentions yet",
                _ => "You’re all caught up",
            }));

        let synced = Container::new()
            .height(42.0)
            .decoration(
                filled(frus::Color::hex("#FAFAFA"), 12.0)
                    .border(frus::Border::new(1.0, crate::common::colors::gray())),
            )
            .alignment(frus::Alignment::CENTER)
            .child(hug(
                8.0,
                vec![
                    Box::new(icon(Glyph::CloudCheck, 16.0, primary_color())),
                    Box::new(text(
                        "Synced moments ago",
                        12.0,
                        16.0,
                        grey(),
                        FontWeight::Regular,
                    )),
                ],
            ));

        let content = Column::new()
            .cross_axis_alignment(Align::Stretch)
            .child(header)
            .child(gap(24.0))
            .child(underline_tabs(
                &["All", "Unread", "Mentions"],
                self.tab,
                |i| cx.callback(move |s: &mut NotificationsState| s.tab = i),
            ))
            .child(gap(16.0))
            .child(tracked(
                "EARLIER",
                12.0,
                16.0,
                0.6,
                grey(),
                FontWeight::SemiBold,
            ))
            .child(gap(48.0))
            .child(empty)
            .child(gap(56.0))
            .child(synced);

        Page::new(AppHeader::new("Notifications"), content)
            .nav(Tab::Notifications, NavStyle::Standard)
            .build()
    }
}
