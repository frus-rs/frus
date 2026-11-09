// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Material 3 expressive widgets of milestone 787, rendered: wavy progress, the
//! carousel, medium and large app bars, the date picker, the expandable rail, glass and the
//! shapes with the loading indicator.
//!
//! With no GPU adapter the tests skip themselves, the harness returning `None`.

use frus_core::Color;
use frus_test::Stage;
use frus_widgets::{
    AppBar, CalendarDate, Carousel, CircularWavyProgressIndicator, Container, CustomScrollView,
    DatePickerDialog, DatePickerModel, ExpandableNavigationRail, Flex, GlassSurface, Icons,
    LinearWavyProgressIndicator, ListTile, LoadingIndicator, MaterialShapes, MorphingShape,
    Positioned, Slider, SliverAppBar, SliverList, Stack, Text, Theme, WavySliderTrackShape, Widget,
};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

fn theme() -> Theme {
    Theme::light().with_platform(frus_core::TargetPlatform::Linux)
}

fn shot(root: &dyn Widget<()>, w: u32, h: u32, name: &str) -> Option<frus_test::Snapshot> {
    let mut stage = Stage::new(w, h).theme(theme());
    stage.settle(root);
    let snapshot = stage.render(root);
    match &snapshot {
        Some(s) => s.assert_golden(golden(name)),
        None => eprintln!("no GPU adapter available: test skipped"),
    }
    snapshot
}

fn page(child: impl Widget<()> + 'static) -> Container<()> {
    Container::new().color(theme().scheme.surface).child(child)
}

#[test]
fn wavy_progress_matches_golden() {
    let root = page(
        Flex::column()
            .gap(16.0)
            .padding(16.0)
            .child(
                LinearWavyProgressIndicator::new(0.6)
                    .width(288.0)
                    .wave_speed(0.0),
            )
            .child(LinearWavyProgressIndicator::new(0.05).width(288.0))
            .child(
                Flex::row()
                    .gap(16.0)
                    .child(CircularWavyProgressIndicator::new(0.7).wave_speed(0.0))
                    .child(CircularWavyProgressIndicator::new(0.3).wave_speed(0.0)),
            )
            .child(
                Slider::<()>::new(0.5)
                    .width(288.0)
                    .track_shape(WavySliderTrackShape::default()),
            ),
    );
    shot(&root, 320, 240, "m3_wavy_progress");
}

fn photo(i: usize) -> Container<()> {
    let hue = (i % 5) as f32 / 5.0;
    Container::new().color(Color::rgb(
        0.25 + 0.6 * hue,
        0.55 - 0.3 * hue,
        0.85 - 0.5 * hue,
    ))
}

#[test]
fn carousel_layouts_match_golden() {
    let root = page(
        Flex::column()
            .gap(8.0)
            .child(Carousel::multi_browse(8, photo).width(360.0).height(120.0))
            .child(Carousel::hero(8, photo).width(360.0).height(120.0))
            .child(
                Carousel::uncontained(8, 150.0, photo)
                    .width(360.0)
                    .height(100.0),
            ),
    );
    shot(&root, 360, 360, "m3_carousel");
}

#[test]
fn medium_and_large_app_bars_match_golden() {
    let root = page(
        Flex::column()
            .gap(8.0)
            .child(
                AppBar::<()>::medium()
                    .width(360.0)
                    .primary(false)
                    .automatically_imply_leading(false)
                    .title(Text::new("Medium"))
                    .build(),
            )
            .child(
                AppBar::<()>::large()
                    .width(360.0)
                    .primary(false)
                    .automatically_imply_leading(false)
                    .title(Text::new("Large"))
                    .build(),
            ),
    );
    shot(&root, 360, 290, "m3_app_bars");
}

fn sliver_page() -> CustomScrollView<()> {
    CustomScrollView::new()
        .width(360.0)
        .height(320.0)
        .sliver(
            SliverAppBar::large()
                .primary(false)
                .automatically_imply_leading(false)
                .title(Text::new("Library")),
        )
        .sliver(SliverList::new(40, 56.0, |i| {
            ListTile::new().title(format!("Book {i}"))
        }))
}

#[test]
fn a_large_sliver_app_bar_collapses_on_scroll() {
    for (offset, name) in [
        (0.0, "m3_large_bar_open"),
        (200.0, "m3_large_bar_collapsed"),
    ] {
        let root = sliver_page();
        let mut stage = Stage::new(360, 320).theme(theme());
        let id = stage.build(&root).scroll_regions()[0].id;
        stage.runtime.scroll.insert(id, (0.0, offset));
        stage.settle(&root);
        match stage.render(&root) {
            Some(s) => s.assert_golden(golden(name)),
            None => eprintln!("no GPU adapter available: test skipped"),
        }
    }
}

#[test]
fn date_picker_matches_golden() {
    let model = DatePickerModel::single(
        Some(CalendarDate::new(2026, 8, 17)),
        CalendarDate::new(2000, 1, 1),
        CalendarDate::new(2040, 12, 31),
    )
    .with_today(CalendarDate::new(2026, 8, 5));
    let root = page(DatePickerDialog::<()>::new(Some(&model), |_| ()));
    shot(&root, 360, 540, "m3_date_picker");

    let range = DatePickerModel::range(
        Some(CalendarDate::new(2026, 8, 10)),
        Some(CalendarDate::new(2026, 8, 14)),
        CalendarDate::new(2000, 1, 1),
        CalendarDate::new(2040, 12, 31),
    )
    .with_today(CalendarDate::new(2026, 8, 5));
    let root = page(DatePickerDialog::<()>::new(Some(&range), |_| ()));
    shot(&root, 360, 540, "m3_date_range_picker");
}

#[test]
fn expandable_rail_matches_golden() {
    for (open, name) in [(false, "m3_rail_collapsed"), (true, "m3_rail_expanded")] {
        let rail = ExpandableNavigationRail::<()>::new(1, |_| ())
            .item(Icons::INBOX, "Inbox")
            .item(Icons::SEND, "Sent")
            .item(Icons::DRAFTS, "Drafts")
            .expanded(open)
            .on_toggle(())
            .fab(Icons::EDIT, "Compose", ())
            .height(420.0);
        let root = page(Flex::row().child(rail));
        shot(&root, 260, 420, name);
    }
}

#[test]
fn glass_blurs_what_is_behind_it() {
    let mut stripes = Flex::row();
    for i in 0..10 {
        stripes = stripes.child(
            Container::new()
                .width(32.0)
                .height(200.0)
                .color(if i % 2 == 0 {
                    Color::rgb8(240, 110, 40)
                } else {
                    Color::rgb8(30, 80, 190)
                }),
        );
    }
    let root = Stack::<()>::new()
        .width(320.0)
        .height(200.0)
        .layer(stripes)
        .layer(
            Positioned::new(
                GlassSurface::new(Text::new("Glass"))
                    .width(200.0)
                    .height(120.0)
                    .padding(16.0),
            )
            .left(60.0)
            .top(40.0),
        );
    let Some(snapshot) = shot(&root, 320, 200, "m3_glass") else {
        return;
    };
    // Outside the panel the stripes are untouched.
    assert_eq!(&snapshot.pixel(10, 10)[..3], &[240, 110, 40]);
    // Inside, over the middle of an orange stripe, the blue beside it has bled in: the
    // pixel is neither stripe's colour, and the tint has lightened it.
    let p = snapshot.pixel(80, 140);
    assert!(p[0] < 240 && p[2] > 40, "blurred: {p:?}");
    assert!(p[1] > 110, "tinted toward the light surface: {p:?}");
    // Two pixels a stripe apart read nearly alike, which they would not unblurred.
    let q = snapshot.pixel(112, 140);
    let gap = (p[0] as i32 - q[0] as i32).abs() + (p[2] as i32 - q[2] as i32).abs();
    assert!(gap < 120, "{p:?} against {q:?}");
}

#[test]
fn shapes_and_loading_indicator_match_golden() {
    let mut grid = Flex::row().gap(8.0).wrap().width(344.0);
    for (_, shape) in MaterialShapes::all() {
        grid = grid.child(MorphingShape::shape(shape).size(40.0));
    }
    let root = page(
        Flex::column()
            .gap(12.0)
            .padding(8.0)
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(LoadingIndicator::determinate(0.0))
                    .child(LoadingIndicator::determinate(0.5).contained(true))
                    .child(
                        MorphingShape::new(
                            MaterialShapes::circle(),
                            MaterialShapes::cookie9(),
                            0.5,
                        )
                        .size(48.0),
                    ),
            )
            .child(grid),
    );
    shot(&root, 360, 300, "m3_shapes");
}
