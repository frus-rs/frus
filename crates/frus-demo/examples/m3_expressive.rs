// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Material 3 expressive widgets of milestone 787, one page each: wavy progress,
//! the carousel, medium and large app bars, the date picker, the expandable rail, glass
//! and the shapes with the loading indicator.
//!
//! `cargo run -p frus-demo --example m3_expressive` on the desktop. `FRUS_M3_PAGE=3`
//! opens on the fourth page; `FRUS_M3_PICKER=single`, `range` or `input` opens the date
//! picker, and `FRUS_M3_RAIL=open` the rail.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus_shell::anyhow::Result<()> {
    use frus_shell::FrusApp;
    use frus_widgets::{
        component, AppBar, Button, CalendarDate, Carousel, CircularWavyProgressIndicator, Color,
        Container, CustomScrollView, DatePickerDialog, DatePickerModel, DatePickerOutcome,
        ExpandableNavigationRail, Flex, GlassSurface, Icons, LinearWavyProgressIndicator, ListTile,
        LoadingIndicator, MaterialShapes, MorphingShape, Slider, SliverAppBar, SliverList, Text,
        Variant, WavySliderTrackShape, Widget,
    };

    let first: usize = std::env::var("FRUS_M3_PAGE")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let app = FrusApp::from_fn(move |_| {
        Box::new(component(move |cx| {
            let page = cx.use_state(move || first);
            let progress = cx.use_state(|| 0.62f32);
            let index = cx.use_state(|| 0usize);
            let rail_open =
                cx.use_state(|| std::env::var("FRUS_M3_RAIL").is_ok_and(|v| v == "open"));
            let rail_at = cx.use_state(|| 0usize);
            let picker = cx.use_state(|| {
                let (first, last, today) = (
                    CalendarDate::new(2000, 1, 1),
                    CalendarDate::new(2040, 12, 31),
                    CalendarDate::new(2026, 11, 8),
                );
                match std::env::var("FRUS_M3_PICKER").as_deref() {
                    Ok("single") => {
                        Some(DatePickerModel::single(Some(today), first, last).with_today(today))
                    }
                    Ok("range") => Some(
                        DatePickerModel::range(
                            Some(CalendarDate::new(2026, 11, 3)),
                            Some(CalendarDate::new(2026, 11, 12)),
                            first,
                            last,
                        )
                        .with_today(today),
                    ),
                    Ok("input") => Some(
                        DatePickerModel::single(Some(today), first, last)
                            .with_today(today)
                            .input_mode(),
                    ),
                    _ => None,
                }
            });
            let picked = cx.use_state(|| None::<String>);
            let morph = cx.use_state(|| 0.5f32);
            let theme = cx.theme().clone();
            let names = [
                "Progress", "Carousel", "App bars", "Date", "Rail", "Glass", "Shapes",
            ];
            let mut tabs = Flex::row().gap(6.0).padding(8.0);
            for (i, name) in names.iter().enumerate() {
                let p = page.clone();
                tabs = tabs.child(
                    Button::new(*name)
                        .variant(if page.get() == i {
                            Variant::Filled
                        } else {
                            Variant::Text
                        })
                        .on_press(move || p.set(i)),
                );
            }
            let tile = |i: usize| {
                let hue = (i % 7) as f32 / 7.0;
                Container::new()
                    .color(Color::rgb(
                        0.25 + 0.55 * hue,
                        0.55 - 0.25 * hue,
                        0.85 - 0.5 * hue,
                    ))
                    .alignment(frus_widgets::Alignment::CENTER)
                    .child(Text::new(format!("Photo {i}")).color(Color::WHITE))
            };
            let body: Box<dyn Widget> = match page.get() {
                0 => {
                    let v = progress.get();
                    let set = progress.clone();
                    Box::new(
                        Flex::column()
                            .gap(24.0)
                            .padding(24.0)
                            .child(Text::new("Linear, determinate and indeterminate"))
                            .child(LinearWavyProgressIndicator::new(v).width(360.0))
                            .child(LinearWavyProgressIndicator::indeterminate().width(360.0))
                            .child(Text::new("Circular"))
                            .child(
                                Flex::row()
                                    .gap(24.0)
                                    .child(CircularWavyProgressIndicator::new(v))
                                    .child(CircularWavyProgressIndicator::indeterminate()),
                            )
                            .child(Text::new("Wavy slider track"))
                            .child(
                                Slider::new(v)
                                    .width(360.0)
                                    .track_shape(WavySliderTrackShape::default())
                                    .on_change(move |x: f32| set.set(x)),
                            ),
                    )
                }
                1 => {
                    let i = index.get();
                    let on = index.clone();
                    Box::new(
                        Flex::column()
                            .gap(16.0)
                            .padding(16.0)
                            .child(Text::new("Multi-browse (arrow keys move it once focused)"))
                            .child(
                                Carousel::multi_browse(10, tile)
                                    .width(1000.0)
                                    .height(180.0)
                                    .index(i)
                                    .on_index_changed(move |n| on.set(n)),
                            )
                            .child(Text::new("Hero"))
                            .child(Carousel::hero(8, tile).width(1000.0).height(180.0))
                            .child(Text::new("Uncontained"))
                            .child(
                                Carousel::uncontained(12, 200.0, tile)
                                    .width(1000.0)
                                    .height(140.0),
                            ),
                    )
                }
                2 => Box::new(
                    Flex::row()
                        .gap(16.0)
                        .padding(16.0)
                        .child(
                            Flex::column()
                                .width(360.0)
                                .gap(16.0)
                                .child(
                                    AppBar::medium()
                                        .width(360.0)
                                        .title(Text::new("Medium"))
                                        .build(),
                                )
                                .child(
                                    AppBar::large()
                                        .width(360.0)
                                        .title(Text::new("Large"))
                                        .build(),
                                ),
                        )
                        .child(
                            Container::new().width(360.0).height(520.0).child(
                                CustomScrollView::new()
                                    .sliver(
                                        SliverAppBar::large()
                                            .primary(false)
                                            .title(Text::new("Collapses")),
                                    )
                                    .sliver(SliverList::new(40, 56.0, |i| {
                                        ListTile::new().title(format!("Row {i}"))
                                    })),
                            ),
                        ),
                ),
                3 => {
                    let open = picker.clone();
                    let open_range = picker.clone();
                    let shown = picked.get().unwrap_or_else(|| "Nothing picked".into());
                    let screen = Flex::column()
                        .width(1100.0)
                        .height(660.0)
                        .gap(16.0)
                        .padding(24.0)
                        .child(Text::new(shown))
                        .child(Button::new("Pick a date").on_press(move || {
                            open.set(Some(
                                DatePickerModel::single(
                                    Some(CalendarDate::new(2026, 11, 8)),
                                    CalendarDate::new(2000, 1, 1),
                                    CalendarDate::new(2040, 12, 31),
                                )
                                .with_today(CalendarDate::new(2026, 11, 8)),
                            ))
                        }))
                        .child(Button::new("Pick a range").on_press(move || {
                            open_range.set(Some(
                                DatePickerModel::range(
                                    Some(CalendarDate::new(2026, 11, 3)),
                                    Some(CalendarDate::new(2026, 11, 12)),
                                    CalendarDate::new(2000, 1, 1),
                                    CalendarDate::new(2040, 12, 31),
                                )
                                .with_today(CalendarDate::new(2026, 11, 8)),
                            ))
                        }));
                    let (state, result) = (picker.clone(), picked.clone());
                    let model = picker.get();
                    Box::new(
                        DatePickerDialog::new(model.as_ref(), move |e| {
                            let Some(mut m) = state.get() else { return };
                            match m.update(e) {
                                Some(DatePickerOutcome::Picked(d)) => {
                                    result.set(Some(format!(
                                        "Picked {}-{:02}-{:02}",
                                        d.year, d.month, d.day
                                    )));
                                    state.set(None);
                                }
                                Some(DatePickerOutcome::PickedRange(a, b)) => {
                                    result.set(Some(format!(
                                        "Range {}-{:02}-{:02} to {}-{:02}-{:02}",
                                        a.year, a.month, a.day, b.year, b.month, b.day
                                    )));
                                    state.set(None);
                                }
                                Some(DatePickerOutcome::Cancelled) => state.set(None),
                                None => state.set(Some(m)),
                            }
                        })
                        .body(screen),
                    )
                }
                4 => {
                    let (toggle, go) = (rail_open.clone(), rail_at.clone());
                    Box::new(
                        Flex::row()
                            .height(560.0)
                            .child(
                                ExpandableNavigationRail::new(rail_at.get(), move |i| go.set(i))
                                    .item(Icons::INBOX, "Inbox")
                                    .item(Icons::SEND, "Sent")
                                    .item(Icons::DRAFTS, "Drafts")
                                    .item(Icons::DELETE, "Trash")
                                    .expanded(rail_open.get())
                                    .on_toggle(move || toggle.update(|o| *o = !*o))
                                    .fab(Icons::EDIT, "Compose", || ())
                                    .height(560.0),
                            )
                            .child(
                                Container::new()
                                    .padding(24.0)
                                    .child(Text::new(format!("Destination {}", rail_at.get()))),
                            ),
                    )
                }
                5 => {
                    let mut stripes = Flex::row();
                    for i in 0..12 {
                        stripes = stripes.child(Container::new().width(40.0).height(320.0).color(
                            if i % 2 == 0 {
                                Color::rgb(0.95, 0.45, 0.2)
                            } else {
                                Color::rgb(0.15, 0.35, 0.75)
                            },
                        ));
                    }
                    Box::new(
                        frus_widgets::Stack::new()
                            .width(480.0)
                            .height(320.0)
                            .layer(stripes)
                            .layer(
                                frus_widgets::Positioned::new(
                                    GlassSurface::new(
                                        Text::new("Frosted glass").color(theme.scheme.on_surface),
                                    )
                                    .width(320.0)
                                    .height(160.0)
                                    .padding(24.0),
                                )
                                .left(80.0)
                                .top(80.0),
                            ),
                    )
                }
                _ => {
                    let m = morph.get();
                    let set = morph.clone();
                    let mut grid = Flex::row().gap(12.0).wrap().width(640.0);
                    for (_, shape) in MaterialShapes::all() {
                        grid = grid.child(MorphingShape::shape(shape).size(56.0));
                    }
                    Box::new(
                        Flex::column()
                            .gap(16.0)
                            .padding(16.0)
                            .child(
                                Flex::row()
                                    .gap(24.0)
                                    .child(LoadingIndicator::new())
                                    .child(LoadingIndicator::new().contained(true))
                                    .child(
                                        MorphingShape::new(
                                            MaterialShapes::circle(),
                                            MaterialShapes::cookie9(),
                                            m,
                                        )
                                        .size(72.0),
                                    )
                                    .child(
                                        Slider::new(m)
                                            .width(240.0)
                                            .on_change(move |x: f32| set.set(x)),
                                    ),
                            )
                            .child(grid),
                    )
                }
            };
            Box::new(frus_widgets::SizedBox::expand(
                Container::new()
                    .color(theme.scheme.surface)
                    .child(Flex::column().child(tabs).child(body)),
            )) as Box<dyn Widget>
        }))
    })
    .title("frus M3 expressive")
    .window_size(1100.0, 760.0);
    frus_shell::run(app)
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
