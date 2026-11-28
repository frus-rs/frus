// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Milestone 856: a component whose state changes is painted again, and the components kept
//! around it give back what they painted and registered the frame before. Measured and
//! checked through the shell's own frame.

use std::cell::RefCell;

use frus_widgets::{
    component, BuildContext, Callback, Column, ListTile, Scaffold, SingleChildScrollView, Switch,
    UseState, Widget,
};

use crate::app::testing::Driver;
use crate::FrusApp;

thread_local! {
    /// The count of each row, to change one from the test as a handler would.
    static COUNTS: RefCell<Vec<UseState<u32>>> = const { RefCell::new(Vec::new()) };
}

/// A row with a state of its own: its count is part of its title.
fn row(index: usize) -> Box<dyn Widget> {
    Box::new(component(move |cx: &BuildContext| {
        let count = cx.use_state(|| 0u32);
        COUNTS.with(|all| {
            let mut all = all.borrow_mut();
            if all.len() <= index {
                all.resize(index + 1, count.clone());
            }
            all[index] = count.clone();
        });
        let tapped = count.clone();
        Box::new(
            ListTile::new()
                .on_tap(Callback::new(move || tapped.update(|n| *n += 1)))
                .title(format!("Conversation {index} ({})", count.get()))
                .subtitle(format!(
                    "The last message of conversation {index}, cut short"
                ))
                .trailing(Switch::new(index.is_multiple_of(3))),
        ) as Box<dyn Widget>
    }))
}

fn screen_of(rows: usize) -> FrusApp {
    FrusApp::from_fn(move |_cx| {
        let mut list = Column::new();
        for index in 0..rows {
            list = list.child(row(index));
        }
        Box::new(Scaffold::new().body(SingleChildScrollView::new().child(list)))
    })
}

/// Adds one to row `index`'s count.
fn bump(index: usize) -> Callback {
    Callback::new(move || {
        COUNTS.with(|all| {
            if let Some(count) = all.borrow().get(index) {
                count.update(|n| *n += 1);
            }
        })
    })
}

fn settled_with(rows: usize) -> Driver<FrusApp> {
    COUNTS.with(|all| all.borrow_mut().clear());
    frus_widgets::__use_memory_prefs();
    let mut driver = Driver::new(screen_of(rows), 400.0, 800.0);
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    driver
}

fn shows(driver: &Driver<FrusApp>, text: &str) -> bool {
    driver.texts().iter().any(|(shown, _)| shown == text)
}

/// **The numbers of milestone 856**: one row's own state changes among `rows`, through the
/// shell's frame. `cargo test -p frus-shell --release one_row_cost -- --ignored --nocapture`
fn one_row_cost(rows: usize) {
    let mut driver = settled_with(rows);
    let changed = rows / 2;
    // build, the walk of `build_ui` as a whole, its layouts, their fingerprints, the walk
    // that paints and registers (what is left)
    let mut phases: [Vec<f32>; 5] = Default::default();
    const TICKS: usize = 100;
    for _ in 0..TICKS {
        driver.update(bump(changed));
        driver.frame(1.0 / 60.0);
        let (build, frame) = driver.costs();
        let (layout, fingerprints) = driver.layout_time();
        for (phase, value) in phases.iter_mut().zip([
            build.unwrap_or(0.0),
            frame,
            layout,
            fingerprints,
            frame - layout,
        ]) {
            phase.push(value);
        }
    }
    let [build, frame, layout, fingerprints, walk] = phases.map(|mut v| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    });
    let (reused, painted) = driver.paint_reuse();
    println!(
        "one row of {rows} changes: build {build:.3} ms; build_ui {frame:.3} ms = layout \
         {layout:.3} ms (of which fingerprints {fingerprints:.3} ms) + paint and register \
         {walk:.3} ms (medians of {TICKS}); last frame: {reused} subtrees given back, \
         {painted} painted"
    );
    assert!(shows(&driver, &format!("Conversation {changed} ({TICKS})")));
}

#[test]
#[ignore = "a measurement: run it by hand, in a release build"]
fn one_row_cost_among_forty() {
    one_row_cost(40);
}

#[test]
#[ignore = "a measurement: run it by hand, in a release build"]
fn one_row_cost_among_a_thousand() {
    one_row_cost(1000);
}

/// Forgets what was kept, paints the frame again from scratch, and says whether it is the
/// frame that was shown.
fn is_the_frame(driver: &mut Driver<FrusApp>) -> bool {
    let kept = driver.frame_digest();
    driver.forget_paint();
    driver.frame(1.0 / 60.0);
    let fresh = driver.frame_digest();
    if kept != fresh {
        eprintln!("kept:\n{kept}\nfresh:\n{fresh}");
    }
    kept == fresh
}

/// **One row's own state changes among forty**: that row is painted, the other thirty-nine
/// are given back as they were, and the frame is the one painted from scratch.
#[test]
fn one_row_is_painted_and_the_others_are_given_back() {
    let mut driver = settled_with(40);
    driver.update(bump(7));
    driver.frame(1.0 / 60.0);
    let (kept, painted) = driver.paint_reuse();
    assert!(shows(&driver, "Conversation 7 (1)"), "{:?}", driver.texts());
    assert_eq!(painted, 1, "the row that changed ({kept} kept)");
    assert!(kept >= 39, "the others are given back: {kept}");
    assert!(is_the_frame(&mut driver));
}

/// **A tap on a row that was given back** lands on it: what it registered was given back
/// with what it painted.
#[test]
fn a_tap_reaches_a_row_that_was_given_back() {
    let mut driver = settled_with(40);
    driver.update(bump(20));
    driver.frame(1.0 / 60.0);
    assert!(driver.tap_text("Conversation 3 (0)"));
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    assert!(shows(&driver, "Conversation 3 (1)"), "{:?}", driver.texts());
    assert!(shows(&driver, "Conversation 20 (1)"));
    assert!(is_the_frame(&mut driver));
}

/// **A new theme** reaches every row, though none of them changed.
#[test]
fn a_new_theme_reaches_the_rows_that_were_kept() {
    let mut driver = settled_with(20);
    driver.update(bump(2));
    driver.frame(1.0 / 60.0);
    frus_widgets::host::app().set_theme_mode(frus_widgets::ThemeMode::Dark);
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    let (kept, _) = driver.paint_reuse();
    let dark = is_the_frame(&mut driver);
    frus_widgets::host::app().set_theme_mode(frus_widgets::ThemeMode::System);
    assert!(dark, "the dark frame is the dark frame");
    assert!(
        kept >= 19,
        "once settled, the dark rows are kept in turn: {kept}"
    );
}

/// **A new surface** — the window resized — reaches every row.
#[test]
fn a_new_surface_reaches_the_rows_that_were_kept() {
    let mut driver = settled_with(20);
    driver.resize(700.0, 500.0);
    driver.frame(1.0 / 60.0);
    assert!(is_the_frame(&mut driver));
}
