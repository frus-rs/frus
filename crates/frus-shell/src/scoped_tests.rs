// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Milestone 825: two instances of a screen, each with its own model in scope. A change to one
//! builds that one's readers, through the shell's own frame, and a press goes the way a
//! person's does.

use std::cell::RefCell;
use std::collections::HashMap;

use frus_widgets::{
    component, BuildContext, Button, Column, Container, ListView, Scoped, Text, Widget,
};

use crate::app::testing::Driver;
use crate::FrusApp;

thread_local! {
    /// How many times each part was built, by name: `"A lines"`, `"B badge"`, …
    static BUILT: RefCell<HashMap<String, u32>> = RefCell::new(HashMap::new());
}

fn built(part: &str) {
    BUILT.with(|b| *b.borrow_mut().entry(part.to_string()).or_insert(0) += 1);
}

fn count(part: &str) -> u32 {
    BUILT.with(|b| b.borrow().get(part).copied().unwrap_or(0))
}

fn reset() {
    BUILT.with(|b| b.borrow_mut().clear());
}

#[derive(Default)]
struct Cart {
    items: Vec<String>,
    note: String,
}

/// One cart: the panel owns it and puts it in scope; its lines watch it, its badge selects the
/// number of items, its buttons only read it, and the rows of a list select it too.
fn panel(name: &'static str) -> Box<dyn Widget> {
    Box::new(component(move |cx: &BuildContext| {
        built(&format!("{name} panel"));
        let cart = cx.use_model(Cart::default);
        let lines = component(move |cx: &BuildContext| {
            built(&format!("{name} lines"));
            let cart = cx.watch::<Cart>();
            let shown = cart.with(|c| format!("{name}: [{}] ({})", c.items.join(", "), c.note));
            Box::new(Text::new(shown)) as Box<dyn Widget>
        });
        let badge = component(move |cx: &BuildContext| {
            built(&format!("{name} badge"));
            let n = cx.select(|c: &Cart| c.items.len());
            Box::new(Text::new(format!("{name} holds {n}"))) as Box<dyn Widget>
        });
        let buttons = component(move |cx: &BuildContext| {
            built(&format!("{name} buttons"));
            let (add, note) = (cx.read::<Cart>(), cx.read::<Cart>());
            Box::new(
                Column::new()
                    .child(
                        Button::new(format!("Add to {name}"))
                            .on_press(move || add.update(|c| c.items.push("tea".into()))),
                    )
                    .child(
                        Button::new(format!("Note {name}"))
                            .on_press(move || note.update(|c| c.note.push('*'))),
                    ),
            ) as Box<dyn Widget>
        });
        let rows = ListView::builder(2, move |i| {
            component(move |cx: &BuildContext| {
                let n = cx.select(|c: &Cart| c.items.len());
                Box::new(Text::new(format!("{name} row {i} sees {n}"))) as Box<dyn Widget>
            })
        });
        Box::new(Scoped::new(
            cart,
            Column::new()
                .child(lines)
                .child(badge)
                .child(buttons)
                .child(Container::new().height(120.0).child(rows)),
        )) as Box<dyn Widget>
    }))
}

fn two_carts() -> Driver<FrusApp> {
    frus_widgets::__use_memory_prefs();
    let app = FrusApp::from_fn(|_cx| {
        built("screen");
        Box::new(Column::new().child(panel("A")).child(panel("B")))
    });
    let mut driver = Driver::new(app, 400.0, 900.0);
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
    driver
}

fn shows(driver: &Driver<FrusApp>, text: &str) -> bool {
    driver.texts().iter().any(|(shown, _)| shown == text)
}

fn settle(driver: &mut Driver<FrusApp>) {
    for _ in 0..30 {
        driver.frame(1.0 / 60.0);
    }
}

/// **Issue #222, through the shell**: a press in one cart builds that cart's readers, and
/// neither the screen, nor that cart's panel and buttons, nor anything of the other cart.
#[test]
fn a_press_in_one_cart_builds_only_that_carts_readers() {
    let mut driver = two_carts();
    assert!(shows(&driver, "A holds 0") && shows(&driver, "B holds 0"));
    reset();
    assert!(driver.tap_text("Add to A"));
    settle(&mut driver);
    assert!(shows(&driver, "A: [tea] ()"), "{:?}", driver.texts());
    assert!(shows(&driver, "A holds 1"), "{:?}", driver.texts());
    assert!(shows(&driver, "A row 1 sees 1"), "{:?}", driver.texts());
    assert!(shows(&driver, "B holds 0"), "the other cart is its own");
    assert!(shows(&driver, "B row 0 sees 0"), "{:?}", driver.texts());
    assert_eq!(
        (count("A lines"), count("A badge")),
        (1, 1),
        "the readers of the cart that changed, once each"
    );
    for part in [
        "screen",
        "A panel",
        "A buttons",
        "B panel",
        "B lines",
        "B badge",
        "B buttons",
    ] {
        assert_eq!(count(part), 0, "{part} is not built");
    }

    reset();
    assert!(driver.tap_text("Add to B"));
    assert!(driver.tap_text("Add to B"));
    settle(&mut driver);
    assert!(shows(&driver, "B holds 2"), "{:?}", driver.texts());
    assert!(shows(&driver, "A holds 1"), "{:?}", driver.texts());
    assert_eq!((count("A lines"), count("A badge")), (0, 0));
    assert_eq!(count("screen"), 0);
}

/// **A selector, through the shell**: a new note builds the lines that show it, and not the
/// badge, which counts the items.
#[test]
fn a_note_builds_the_lines_and_not_the_badge() {
    let mut driver = two_carts();
    reset();
    assert!(driver.tap_text("Note A"));
    settle(&mut driver);
    assert!(shows(&driver, "A: [] (*)"), "{:?}", driver.texts());
    assert_eq!((count("A lines"), count("A badge")), (1, 0));
    assert_eq!((count("screen"), count("B lines")), (0, 0));
}
