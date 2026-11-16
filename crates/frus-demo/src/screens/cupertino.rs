// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Apple-style widgets (milestone 812): a navigation bar with a back chevron, the three
//! buttons in their sizes, switches, spinners, an alert, an action sheet and a tab bar —
//! and the adaptive switch and spinner, which follow the theme's platform.

use frus_widgets::{
    CircularProgressIndicator, CupertinoActionSheet, CupertinoActionSheetAction,
    CupertinoActivityIndicator, CupertinoAlertDialog, CupertinoButton, CupertinoButtonSize,
    CupertinoDialogAction, CupertinoNavigationBar, CupertinoSwitch, CupertinoTabBar,
    CupertinoTabBarItem, Text,
};

use crate::prelude::*;

/// The screen.
pub(crate) struct CupertinoPage;

/// What the screen keeps.
pub(crate) struct CupertinoState {
    wifi: bool,
    bluetooth: bool,
    adaptive: bool,
    alert: bool,
    sheet: bool,
    tab: usize,
    said: String,
}

impl StatefulWidget for CupertinoPage {
    type State = CupertinoState;

    fn create_state(&self) -> CupertinoState {
        CupertinoState {
            wifi: true,
            bluetooth: false,
            adaptive: true,
            alert: false,
            sheet: false,
            tab: 0,
            said: "Press a button.".into(),
        }
    }
}

/// A section's heading.
fn heading(label: &str, theme: &Theme) -> Text {
    text(label).size(13.0).color(theme.muted)
}

impl State for CupertinoState {
    type Widget = CupertinoPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let said = |what: &'static str| cx.callback(move |s| s.said = what.into());

        let buttons = Flex::column()
            .gap(8.0)
            .child(heading("BUTTONS", &theme))
            .child(
                Flex::row()
                    .gap(8.0)
                    .child(CupertinoButton::new("Plain").on_press(said("Plain")))
                    .child(
                        CupertinoButton::new("Tinted")
                            .tinted()
                            .on_press(said("Tinted")),
                    )
                    .child(
                        CupertinoButton::new("Filled")
                            .filled()
                            .on_press(said("Filled")),
                    ),
            )
            .child(
                Flex::row()
                    .gap(8.0)
                    .child(
                        CupertinoButton::new("Small")
                            .filled()
                            .size(CupertinoButtonSize::Small)
                            .on_press(said("Small")),
                    )
                    .child(
                        CupertinoButton::new("Medium")
                            .filled()
                            .size(CupertinoButtonSize::Medium)
                            .on_press(said("Medium")),
                    )
                    .child(CupertinoButton::new("Disabled").filled()),
            )
            .child(text(self.said.clone()).size(13.0).color(theme.on_surface));

        let switches = Flex::column()
            .gap(8.0)
            .child(heading("SWITCHES", &theme))
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(text("Wi-Fi").size(17.0).color(theme.on_surface))
                    .child(
                        CupertinoSwitch::new(self.wifi)
                            .on_toggle(cx.handler(|s, on: bool| s.wifi = on)),
                    ),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(text("Bluetooth").size(17.0).color(theme.on_surface))
                    .child(
                        CupertinoSwitch::new(self.bluetooth)
                            .on_toggle(cx.handler(|s, on: bool| s.bluetooth = on)),
                    ),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(text("Disabled").size(17.0).color(theme.muted))
                    .child(CupertinoSwitch::new(true).enabled(false)),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(
                        text(format!("Adaptive ({})", theme.platform.name()))
                            .size(17.0)
                            .color(theme.on_surface),
                    )
                    .child(
                        Switch::adaptive(self.adaptive)
                            .on_toggle(cx.handler(|s, on: bool| s.adaptive = on)),
                    ),
            );

        let spinners = Flex::column()
            .gap(8.0)
            .child(heading("ACTIVITY INDICATORS", &theme))
            .child(
                Flex::row()
                    .gap(24.0)
                    .child(CupertinoActivityIndicator::new())
                    .child(CupertinoActivityIndicator::new().radius(20.0))
                    .child(CupertinoActivityIndicator::new().animating(false))
                    .child(CupertinoActivityIndicator::partially_revealed(0.5))
                    .child(CircularProgressIndicator::adaptive(None)),
            );

        let dialogs = Flex::column()
            .gap(8.0)
            .child(heading("ALERT AND ACTION SHEET", &theme))
            .child(
                Flex::row()
                    .gap(8.0)
                    .child(
                        CupertinoButton::new("Show alert")
                            .tinted()
                            .on_press(cx.callback(|s| s.alert = true)),
                    )
                    .child(
                        CupertinoButton::new("Show action sheet")
                            .tinted()
                            .on_press(cx.callback(|s| s.sheet = true)),
                    ),
            );

        let content = SingleChildScrollView::new().flex(1.0).child(
            Flex::column()
                .gap(24.0)
                .padding(16.0)
                .child(buttons)
                .child(switches)
                .child(spinners)
                .child(dialogs),
        );

        let router = cx.router();
        let page = Flex::column()
            .flex(1.0)
            .child(
                CupertinoNavigationBar::new()
                    .title("Apple-style widgets")
                    .back(Callback::new(move || {
                        router.pop();
                    }))
                    .previous_page_title("Tasks")
                    .trailing(
                        CupertinoButton::new("Edit")
                            .padding(Default::default())
                            .on_press(said("Edit")),
                    ),
            )
            .child(content)
            .child(
                CupertinoTabBar::new(self.tab, cx.handler(|s, i: usize| s.tab = i))
                    .item(CupertinoTabBarItem::new(Icons::HOME, "Home"))
                    .item(CupertinoTabBarItem::new(Icons::SEARCH, "Search"))
                    .item(CupertinoTabBarItem::new(Icons::SETTINGS, "Settings")),
            );

        let sheet = CupertinoActionSheet::new(self.sheet)
            .title("Share this photo")
            .message("Choose where to send it.")
            .action(CupertinoActionSheetAction::new(
                "Copy link",
                cx.callback(|s| {
                    s.sheet = false;
                    s.said = "Copied the link".into();
                }),
            ))
            .action(
                CupertinoActionSheetAction::new(
                    "Delete",
                    cx.callback(|s| {
                        s.sheet = false;
                        s.said = "Deleted".into();
                    }),
                )
                .destructive(),
            )
            .cancel(
                CupertinoActionSheetAction::new("Cancel", cx.callback(|s| s.sheet = false))
                    .default_action(),
            )
            .on_dismiss(cx.callback(|s| s.sheet = false))
            .body(page);

        let alert = CupertinoAlertDialog::new(self.alert)
            .title("Delete this photo?")
            .content("It will be removed from all your devices.")
            .action(
                CupertinoDialogAction::new("Cancel", cx.callback(|s| s.alert = false))
                    .default_action(),
            )
            .action(
                CupertinoDialogAction::new(
                    "Delete",
                    cx.callback(|s| {
                        s.alert = false;
                        s.said = "Deleted".into();
                    }),
                )
                .destructive(),
            )
            .body(sheet);

        Box::new(
            Container::new()
                .color(theme.background)
                .flex(1.0)
                .child(alert),
        )
    }
}
