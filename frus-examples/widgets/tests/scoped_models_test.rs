//! Tests for `widgets::features::state::widgets::scoped_models`: two carts, each with its own
//! model, and a change to one builds only the parts that read it.

use frus::{FrusApp, Tester, Widget};
use widgets::features::state::widgets::scoped_models::{example, Cart};

fn page() -> Tester<FrusApp> {
    Tester::sized(
        FrusApp::from_fn(|_cx| -> Box<dyn Widget> { example() }),
        900.0,
        900.0,
    )
}

/// The parts each cart shows, by the start of their text.
const PARTS: [&str; 8] = [
    "Cart A ",
    "A lines:",
    "A badge:",
    "A buttons ",
    "Cart B ",
    "B lines:",
    "B badge:",
    "B buttons ",
];

/// How many times each part has been built, read from its "(built N)", in `PARTS` order.
fn builds(page: &Tester<FrusApp>) -> Vec<u32> {
    let texts = page.texts();
    PARTS
        .iter()
        .map(|part| {
            let text = texts
                .iter()
                .find(|text| text.starts_with(part))
                .unwrap_or_else(|| panic!("no {part:?} on screen: {texts:?}"));
            let count = text
                .rsplit("(built ")
                .next()
                .and_then(|rest| rest.strip_suffix(')'))
                .unwrap_or_else(|| panic!("no build count in {text:?}"));
            count.parse().expect("a build count")
        })
        .collect()
}

/// The parts built again between `before` and `after`.
fn built_again(before: &[u32], after: &[u32]) -> Vec<&'static str> {
    PARTS
        .iter()
        .zip(before.iter().zip(after))
        .filter(|(_, (before, after))| after > before)
        .map(|(part, _)| part.trim_end())
        .collect()
}

#[test]
fn a_cart_changes_through_its_methods() {
    let mut cart = Cart::default();
    cart.add("tea");
    cart.add("bread");
    cart.sign();
    assert_eq!(cart.total(), 2);
    assert_eq!(cart.note, "*");
}

/// Each part shows how many times it was built: the counter the issue asks for.
#[test]
fn adding_to_one_cart_builds_only_its_readers() {
    let mut page = page();
    assert!(page.shows("A lines: empty, note ''"), "{:?}", page.texts());
    assert!(page.shows("B badge: 0 items"));
    let before = builds(&page);

    page.tap("Add tea to A").settle();
    assert!(page.shows("A lines: tea, note ''"), "{:?}", page.texts());
    assert!(page.shows("A badge: 1 items"));
    assert!(page.shows("B lines: empty, note ''"));
    // Neither the cart that owns the model, nor the buttons that change it, nor the other cart.
    assert_eq!(
        built_again(&before, &builds(&page)),
        ["A lines:", "A badge:"],
        "{:?}",
        page.texts()
    );
}

/// The badge selects the number of items: signing the note does not build it.
#[test]
fn signing_the_note_builds_the_lines_and_not_the_badge() {
    let mut page = page();
    let before = builds(&page);

    page.tap("Sign the note of B").settle();
    assert!(page.shows("B lines: empty, note '*'"), "{:?}", page.texts());
    assert_eq!(
        built_again(&before, &builds(&page)),
        ["B lines:"],
        "{:?}",
        page.texts()
    );
}
