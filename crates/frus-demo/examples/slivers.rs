// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A collapsing app bar over a grid and a ten-thousand-row list, in one scroll view
//! (milestone 780).
//!
//! `cargo run -p frus-demo --example slivers` on the desktop. `FRUS_SLIVERS=floating`
//! swaps the pinned bar for a floating one that snaps.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus_shell::anyhow::Result<()> {
    use frus_shell::FrusApp;
    use frus_widgets::{
        Color, Container, CustomScrollView, FlexibleSpaceBar, IconButton, Icons, ListTile,
        SliverAppBar, SliverGrid, SliverList, SliverPadding, SliverPersistentHeader,
        SliverToBoxAdapter, Text, Widget,
    };

    let floating = std::env::var("FRUS_SLIVERS").is_ok_and(|mode| mode == "floating");
    let app = FrusApp::from_fn(move |cx| {
        let theme = cx.theme();
        let scheme = theme.scheme;
        let space = FlexibleSpaceBar::new()
            .title(Text::new("Lisbon"))
            .background(
                Container::new()
                    .color(Color::rgb(0.10, 0.32, 0.55))
                    .gradient(Color::rgb(0.55, 0.20, 0.45), [0.0, 1.0]),
            );
        let bar = SliverAppBar::new()
            .expanded_height(220.0)
            .action(IconButton::new(Icons::SEARCH))
            .flexible_space(space);
        let bar = if floating {
            bar.floating(true).snap(true)
        } else {
            bar.pinned(true)
        };
        let tile = move |i: usize| {
            let hue = (i % 6) as f32 / 6.0;
            Container::new()
                .color(Color::rgb(
                    0.3 + 0.5 * hue,
                    0.6 - 0.3 * hue,
                    0.8 - 0.5 * hue,
                ))
                .radius(12.0)
                .child(Text::new(format!("{i}")))
        };
        let section = move |label: &'static str| {
            SliverPersistentHeader::new(40.0, 40.0, move |_| {
                Container::new()
                    .color(scheme.surface_container_high)
                    .padding_each(10.0, 16.0, 10.0, 16.0)
                    .child(Text::new(label))
            })
            .pinned(true)
        };
        let page = CustomScrollView::new()
            .sliver(bar)
            .sliver(SliverToBoxAdapter::new(
                Container::new()
                    .padding(16.0)
                    .child(Text::new("Neighbourhoods, then every street.")),
            ))
            .sliver(section("Neighbourhoods"))
            .sliver(
                SliverPadding::new(SliverGrid::extent(160.0, 24, tile).gap(8.0).aspect(1.4))
                    .padding(12.0),
            )
            .sliver(section("Streets"))
            .sliver(SliverList::new(10_000, 56.0, |i| {
                ListTile::new().title(format!("Street {i}"))
            }));
        Box::new(Container::new().color(theme.background).child(page)) as Box<dyn Widget>
    })
    .title("frus slivers");
    frus_shell::run(app)
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
