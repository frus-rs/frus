// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The display widgets screen (milestone 811): a sidebar that folds into a rail (a drawer on
//! a narrow window), figures on cards that roll when they change, a marquee, and a file tree
//! with a folder that loads its children when it is opened.

use std::time::Duration;

use frus_widgets::host;
use frus_widgets::{
    FileNode, FileTree, KpiCard, KpiTrend, Marquee, RollingDigits, Sidebar, StateHandle, Text,
};

use crate::prelude::*;

/// The screen.
pub(crate) struct DisplayPage;

/// What the screen keeps.
pub(crate) struct DisplayState {
    /// The sidebar's item in use.
    nav: usize,
    /// Whether the sidebar is open.
    open: bool,
    /// How many times the figures were refreshed: what they are computed from.
    round: u32,
    /// The tree's open folders.
    expanded: Vec<u64>,
    /// The tree's selected node.
    selected: Option<u64>,
    /// Whether the lazy folder's children have arrived.
    loaded: bool,
}

impl StatefulWidget for DisplayPage {
    type State = DisplayState;

    fn create_state(&self) -> DisplayState {
        DisplayState {
            nav: 0,
            // Open on a wide window, folded into a drawer on a narrow one.
            open: MediaQuery::of().size.width >= frus_widgets::SIDEBAR_BREAKPOINT,
            round: 0,
            expanded: vec![1],
            selected: Some(2),
            loaded: false,
        }
    }
}

/// The figures of round `round`: revenue, visitors, conversion (in tenths of a percent) and
/// their changes, made up but steady from one build to the next.
pub(crate) fn figures(round: u32) -> (f64, f64, f64) {
    let r = round as f64;
    let revenue = 48_210.0 + ((r * 7919.0) % 9000.0) - 3000.0 * (round % 2) as f64;
    let visitors = 12_480.0 + ((r * 3571.0) % 5000.0);
    let conversion = 3.4 + ((r * 1.7) % 2.0) - 0.8 * (round % 3 == 2) as u8 as f64;
    (revenue, visitors, conversion)
}

impl DisplayState {
    /// Opens or closes folder `id`; the lazy one starts loading the first time it opens.
    fn toggle(&mut self, handle: &StateHandle<DisplayState>, id: u64, open: bool) {
        self.expanded.retain(|e| *e != id);
        if open {
            self.expanded.push(id);
            if id == 50 && !self.loaded {
                let handle = handle.clone();
                host::after(Duration::from_millis(900), move || {
                    handle.set_state(|s| s.loaded = true)
                });
            }
        }
    }

    fn tree(&self) -> Vec<FileNode> {
        let assets = if self.loaded {
            FileNode::folder(
                50,
                "assets",
                [
                    FileNode::file(51, "logo.svg"),
                    FileNode::file(52, "splash.png"),
                ],
            )
        } else {
            FileNode::lazy_folder(50, "assets")
        };
        vec![
            FileNode::folder(
                1,
                "src",
                [
                    FileNode::file(2, "main.rs"),
                    FileNode::folder(
                        3,
                        "screens",
                        [
                            FileNode::file(4, "display.rs"),
                            FileNode::file(5, "settings.rs"),
                        ],
                    ),
                    FileNode::file(6, "a_file_with_a_rather_long_name_to_show_the_ellipsis.rs"),
                ],
            ),
            assets,
            FileNode::file(60, "Cargo.toml"),
            FileNode::file(61, "README.md"),
        ]
    }
}

impl State for DisplayState {
    type Widget = DisplayPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let theme = &theme;
        let (revenue, visitors, conversion) = figures(self.round);
        let (before_revenue, _, before_conversion) = figures(self.round.wrapping_sub(1));
        let revenue_change = if self.round == 0 {
            12.5
        } else {
            (revenue - before_revenue) / before_revenue * 100.0
        };
        let cards = Flex::row()
            .wrap()
            .gap(12.0)
            .run_gap(12.0)
            .child(
                ConstrainedBox::new(
                    KpiCard::new("Revenue")
                        .icon(Icons::BAR_CHART)
                        .digits(RollingDigits::new(revenue).prefix("$"))
                        .change(revenue_change)
                        .sparkline([31.0, 34.0, 33.0, 38.0, 36.0, 41.0, revenue as f32 / 1000.0]),
                )
                .min_width(220.0)
                .max_width(320.0),
            )
            .child(
                ConstrainedBox::new(
                    KpiCard::new("Visitors")
                        .icon(Icons::PEOPLE)
                        .number(visitors)
                        .delta(format!("{} today", self.round * 37 + 120), KpiTrend::Up),
                )
                .min_width(220.0)
                .max_width(320.0),
            )
            .child(
                ConstrainedBox::new(
                    KpiCard::new("Bounce rate")
                        .digits(RollingDigits::new(conversion).decimals(1).suffix(" %"))
                        .change(conversion - before_conversion)
                        .lower_is_better(true),
                )
                .min_width(220.0)
                .max_width(320.0),
            );
        let refresh = button("Refresh the figures", cx.callback(|s| s.round += 1))
            .variant(Variant::Filled)
            .size(15.0);
        let ticker = Container::new()
            .color(theme.scheme.surface_container_high)
            .radius(8.0)
            .padding(10.0)
            .child(Marquee::text(
                "Markets close higher · Rain expected tomorrow afternoon · The library opens late \
                 on Thursdays · Tickets for the harbour concert go on sale at nine",
            ));
        let title = Container::new().width(180.0).child(
            Marquee::text("Now playing: a song title that is much too long for its slot")
                .velocity(40.0),
        );
        let handle = cx.handle();
        let tree = FileTree::new(self.tree())
            .expanded(self.expanded.iter().copied())
            .selected(self.selected)
            .label("Project files")
            .on_toggle(move |id, open| {
                let handle = handle.clone();
                let state = handle.clone();
                Callback::new(move || state.set_state(|s| s.toggle(&handle, id, open)))
            })
            .on_select(cx.handler(|s, id: u64| s.selected = Some(id)));
        let content = Flex::column()
            .gap(16.0)
            .padding(20.0)
            .align(Align::Stretch)
            .child(text("Figures").size(20.0).color(theme.on_surface))
            .child(cards)
            .child(Flex::row().child(refresh))
            .child(text("Marquee").size(20.0).color(theme.on_surface))
            .child(ticker)
            .child(
                Flex::row()
                    .gap(12.0)
                    .align(Align::Center)
                    .child(Icon::new(Icons::MUSIC_NOTE).color(theme.muted))
                    .child(title),
            )
            .child(text("File tree").size(20.0).color(theme.on_surface))
            .child(
                Container::new()
                    .border(1.0, theme.scheme.outline_variant)
                    .radius(8.0)
                    .padding(6.0)
                    .child(tree),
            );
        let page = SingleChildScrollView::new().flex(1.0).child(content);
        let router = cx.router();
        let shell = Sidebar::new(self.nav, cx.handler(|s, i: usize| s.nav = i))
            .header(Text::new("frus").size(18.0))
            .group("Workspace")
            .item(Icons::DASHBOARD, "Overview")
            .item(Icons::INBOX, "Inbox")
            .item(Icons::BAR_CHART, "Reports")
            .group("Account")
            .item(Icons::SETTINGS, "Settings")
            .expanded(self.open)
            .on_toggle(cx.callback(|s| s.open = !s.open))
            .body(page);
        Scaffold::new()
            .background(theme.background)
            .app_bar(
                AppBar::new()
                    .title(Text::new("Display widgets"))
                    .leading(IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                        router.pop();
                    })))
                    .build(),
            )
            .body(shell)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_figures_are_steady_and_move_between_rounds() {
        assert_eq!(figures(3), figures(3));
        assert_ne!(figures(0).0, figures(1).0);
        assert!(figures(5).1 >= 12_480.0);
    }
}
