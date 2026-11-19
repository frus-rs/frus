// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Milestone 822: a component whose state changes is built again, and the components around it
//! are not. Measured through the shell's own frame, on a realistic screen.

use std::cell::{Cell, RefCell};

use frus_widgets::{
    component, AppBar, BuildContext, Callback, Column, ListTile, Scaffold, SingleChildScrollView,
    Switch, Text, UseState, Widget,
};

use crate::app::testing::Driver;
use crate::FrusApp;

thread_local! {
    /// How many times a row of the screen was built.
    static ROWS_BUILT: Cell<u32> = const { Cell::new(0) };
    /// How many times the screen around the rows was built.
    static SCREEN_BUILT: Cell<u32> = const { Cell::new(0) };
    /// The ticking leaf's count, to change it from the test as a handler would.
    static TICK: RefCell<Option<UseState<u32>>> = const { RefCell::new(None) };
}

const ROWS: usize = 40;

fn row(index: usize) -> Box<dyn Widget> {
    Box::new(component(move |_cx: &BuildContext| {
        ROWS_BUILT.with(|n| n.set(n.get() + 1));
        Box::new(
            ListTile::new()
                .title(format!("Conversation {index}"))
                .subtitle(format!(
                    "The last message of conversation {index}, cut short"
                ))
                .trailing(Switch::new(index.is_multiple_of(3))),
        ) as Box<dyn Widget>
    }))
}

fn ticker() -> Box<dyn Widget> {
    Box::new(component(|cx: &BuildContext| {
        let count = cx.use_state(|| 0u32);
        TICK.with(|t| *t.borrow_mut() = Some(count.clone()));
        Box::new(Text::new(format!("Ticks: {}", count.get()))) as Box<dyn Widget>
    }))
}

/// An inbox: an app bar, a ticking leaf, and forty rows, each a component.
fn screen() -> FrusApp {
    FrusApp::from_fn(|_cx| {
        SCREEN_BUILT.with(|n| n.set(n.get() + 1));
        let mut list = Column::new().child(ticker());
        for index in 0..ROWS {
            list = list.child(row(index));
        }
        Box::new(
            Scaffold::new()
                .app_bar(AppBar::new().title(Text::new("Inbox")))
                .body(SingleChildScrollView::new().child(list)),
        )
    })
}

fn tick() -> Callback {
    Callback::new(|| {
        TICK.with(|t| {
            if let Some(count) = t.borrow().as_ref() {
                count.update(|n| *n += 1);
            }
        })
    })
}

fn reset() {
    ROWS_BUILT.with(|n| n.set(0));
    SCREEN_BUILT.with(|n| n.set(0));
}

fn rows_built() -> u32 {
    ROWS_BUILT.with(Cell::get)
}

fn screen_built() -> u32 {
    SCREEN_BUILT.with(Cell::get)
}

fn settled() -> Driver<FrusApp> {
    frus_widgets::__use_memory_prefs();
    let mut driver = Driver::new(screen(), 400.0, 800.0);
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    driver
}

fn shows(driver: &Driver<FrusApp>, text: &str) -> bool {
    driver.texts().iter().any(|(shown, _)| shown == text)
}

/// **The numbers of milestone 822**: one counter tick in a leaf, through the shell's frame.
/// `cargo test -p frus-shell --release rebuild_cost -- --ignored --nocapture`
#[test]
#[ignore = "a measurement: run it by hand, in a release build"]
fn rebuild_cost_of_one_leaf_tick() {
    let mut driver = settled();
    let (mut builds, mut layouts) = (Vec::new(), Vec::new());
    let mut rows = 0;
    let mut screens = 0;
    const TICKS: usize = 300;
    for _ in 0..TICKS {
        reset();
        let _ = frus_widgets::take_frame_rebuilds();
        driver.update(tick());
        driver.frame(1.0 / 60.0);
        let (build, layout) = driver.costs();
        builds.push(build.unwrap_or(0.0));
        layouts.push(layout);
        rows += rows_built();
        screens += screen_built();
    }
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.total_cmp(b));
        v[v.len() / 2]
    };
    println!(
        "822 leaf tick: build {:.3} ms, layout {:.3} ms (medians of {TICKS}); per tick {} rows and {} screens built",
        median(&mut builds),
        median(&mut layouts),
        rows as f32 / TICKS as f32,
        screens as f32 / TICKS as f32,
    );
    assert!(shows(&driver, &format!("Ticks: {TICKS}")));
}

/// **One tick in a leaf builds the leaf**: not the screen, not one of its forty rows, through
/// the shell's own frame — and the screen shows the new count.
#[test]
fn a_leaf_tick_builds_the_leaf_alone() {
    let mut driver = settled();
    for n in 1..=3 {
        reset();
        driver.update(tick());
        driver.frame(1.0 / 60.0);
        assert_eq!((rows_built(), screen_built()), (0, 0), "tick {n}");
        assert!(
            shows(&driver, &format!("Ticks: {n}")),
            "{:?}",
            driver.texts()
        );
        assert!(driver.costs().0.is_some(), "the frame built something");
    }
    assert!(
        shows(&driver, "Conversation 39"),
        "the rows are still there"
    );
}

/// **A tap on a leaf's button** goes the way a person's does — the press, the release, the
/// handler — and builds the leaf alone.
#[test]
fn a_tap_that_changes_a_leaf_builds_the_leaf_alone() {
    let app = FrusApp::from_fn(|_cx| {
        SCREEN_BUILT.with(|n| n.set(n.get() + 1));
        let mut list = Column::new().child(component(|cx: &BuildContext| {
            let count = cx.use_state(|| 0u32);
            let add = count.clone();
            Box::new(
                Column::new()
                    .child(Text::new(format!("Taps: {}", count.get())))
                    .child(
                        frus_widgets::Button::new("Add").on_press(move || add.update(|n| *n += 1)),
                    ),
            ) as Box<dyn Widget>
        }));
        for index in 0..5 {
            list = list.child(row(index));
        }
        Box::new(list)
    });
    frus_widgets::__use_memory_prefs();
    let mut driver = Driver::new(app, 400.0, 800.0);
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    reset();
    assert!(driver.tap_text("Add"));
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    assert!(shows(&driver, "Taps: 1"), "{:?}", driver.texts());
    assert_eq!(
        (rows_built(), screen_built()),
        (0, 0),
        "neither the rows nor the screen"
    );
}

/// **A handler that changes no component's state** still builds the whole tree, as it always
/// did: what it changed may be anything the screen reads.
#[test]
fn a_handler_that_marks_nothing_builds_the_tree() {
    let mut driver = settled();
    reset();
    driver.update(Callback::new(|| {}));
    driver.frame(1.0 / 60.0);
    assert_eq!((rows_built(), screen_built()), (ROWS as u32, 1));
}

/// **The theme changed**: everything that depends on it — every row — is built again, and the
/// leaf's count is kept.
#[test]
fn a_theme_change_builds_its_dependents_and_keeps_the_state() {
    let mut driver = settled();
    driver.update(tick());
    driver.update(tick());
    driver.frame(1.0 / 60.0);
    reset();
    frus_widgets::host::app().set_theme_mode(frus_widgets::ThemeMode::Dark);
    driver.frame(1.0 / 60.0);
    assert_eq!(rows_built(), ROWS as u32, "every row");
    assert!(shows(&driver, "Ticks: 2"), "{:?}", driver.texts());
    frus_widgets::host::app().set_theme_mode(frus_widgets::ThemeMode::System);
}

/// **The surface changed** — a resize, a phone turned — and everything reading it is built
/// again; the leaf's count is kept, and a tick after it builds the leaf alone again.
#[test]
fn a_new_surface_builds_its_dependents_and_keeps_the_state() {
    let mut driver = settled();
    driver.update(tick());
    driver.frame(1.0 / 60.0);
    reset();
    driver.resize(800.0, 400.0);
    driver.frame(1.0 / 60.0);
    assert_eq!((rows_built(), screen_built()), (ROWS as u32, 1));
    assert!(shows(&driver, "Ticks: 1"), "{:?}", driver.texts());
    reset();
    driver.update(tick());
    driver.frame(1.0 / 60.0);
    assert_eq!((rows_built(), screen_built()), (0, 0));
    assert!(shows(&driver, "Ticks: 2"), "{:?}", driver.texts());
}

/// **What a hot patch asks for**: a patch arrives as work that came back, which builds the
/// whole tree — every component, through the function the patch may have replaced — with
/// every state kept.
#[test]
fn a_patch_builds_the_whole_tree() {
    let mut driver = settled();
    driver.update(tick());
    driver.frame(1.0 / 60.0);
    reset();
    driver.arrive();
    driver.frame(1.0 / 60.0);
    assert_eq!((rows_built(), screen_built()), (ROWS as u32, 1));
    assert!(shows(&driver, "Ticks: 1"), "{:?}", driver.texts());
}
