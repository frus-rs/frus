// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A collapsing app bar over a list, expanded and collapsed (milestone 780).
//!
//! With no GPU adapter the tests skip themselves, the harness returning `None`.

use frus_core::Color;
use frus_test::Stage;
use frus_widgets::{
    Container, CustomScrollView, FlexibleSpaceBar, ListTile, SliverAppBar, SliverList, Text, Theme,
};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

fn page() -> CustomScrollView<()> {
    CustomScrollView::new()
        .width(320.0)
        .height(320.0)
        .sliver(
            SliverAppBar::new()
                .pinned(true)
                .primary(false)
                .automatically_imply_leading(false)
                .expanded_height(180.0)
                .flexible_space(
                    FlexibleSpaceBar::new()
                        .title(Text::new("Lisbon"))
                        .background(Container::new().color(Color::rgb8(120, 170, 220))),
                ),
        )
        .sliver(SliverList::new(40, 56.0, |i| {
            ListTile::new().title(format!("Street {i}"))
        }))
}

fn shot(offset: f32, name: &str) {
    let theme = Theme::light().with_platform(frus_core::TargetPlatform::Linux);
    let root = page();
    let mut stage = Stage::new(320, 320).theme(theme);
    let id = stage.build(&root).scroll_regions()[0].id;
    stage.runtime.scroll.insert(id, (0.0, offset));
    stage.settle(&root);
    let Some(snapshot) = stage.render(&root) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    snapshot.assert_golden(golden(name));
}

/// Expanded: the background behind a title at one and a half times its toolbar size, the
/// first rows under the bar.
#[test]
fn sliver_app_bar_expanded_matches_golden() {
    shot(0.0, "sliver_app_bar_expanded");
}

/// Collapsed: the toolbar's height, the title at its toolbar size, the background faded
/// out, the scrolled-under surface, rows going under it.
#[test]
fn sliver_app_bar_collapsed_matches_golden() {
    shot(300.0, "sliver_app_bar_collapsed");
}
