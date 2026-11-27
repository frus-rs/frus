// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Milestone 854: **an overlay sees what is in scope where it is declared.**
//!
//! Every overlay of frus is declared in the tree, where the page that opens it builds it (there
//! is no route stack to push a dialog onto), and the shell lays it out and paints it after the
//! page, above everything. Since milestone 854 it takes with it the values in scope at that
//! place, as it already took that place's theme: a component inside a dialog, a sheet, a menu
//! row or a card finds the model of the page that opened it, the nearest one first, and is
//! built again, alone, when that model changes.
//!
//! Each overlay kind that holds widgets of the application is opened here through the shell's
//! frame (`Driver`), under two scopes: the application's cart at the root (100 items) and the
//! page's own (0 items). The overlay must see the page's; before milestone 854 the components
//! inside it found the root's, or, with nothing at the root, panicked.

use std::cell::RefCell;

use frus_widgets::Point;
use frus_widgets::{
    component, AlertDialog, BottomSheet, BuildContext, Button, Callback, Column, Container,
    ContextMenu, ContextMenuState, CupertinoAlertDialog, Dialog, Drawer, DropdownButton,
    DropdownOption, HoverCard, ListView, MenuAnchor, MenuBar, MenuItem, MenuPath, Model,
    OpenContainer, OverlayPortal, Placement, PopupMenuButton, Scoped, SearchAnchor, SubmenuButton,
    Text, Widget,
};

use crate::app::testing::Driver;
use crate::FrusApp;

#[derive(Default)]
struct Cart {
    items: u32,
}

thread_local! {
    /// The page's cart, for the test to change it from outside the tree.
    static PAGE_CART: RefCell<Option<Model<Cart>>> = const { RefCell::new(None) };
    /// How many times each reader was built, by tag.
    static BUILT: RefCell<Vec<(String, u32)>> = const { RefCell::new(Vec::new()) };
}

fn built(tag: &str) {
    BUILT.with(|b| {
        let mut b = b.borrow_mut();
        match b.iter_mut().find(|(t, _)| t == tag) {
            Some((_, n)) => *n += 1,
            None => b.push((tag.to_string(), 1)),
        }
    });
}

fn count(tag: &str) -> u32 {
    BUILT.with(|b| {
        b.borrow()
            .iter()
            .find(|(t, _)| t == tag)
            .map_or(0, |(_, n)| *n)
    })
}

/// A component that shows how many items the cart in scope holds.
fn reader(tag: &'static str) -> Box<dyn Widget> {
    Box::new(component(move |cx: &BuildContext| {
        built(tag);
        let n = cx.select(|c: &Cart| c.items);
        Box::new(Text::new(format!("{tag} sees {n}"))) as Box<dyn Widget>
    }))
}

/// A component that adds to the cart in scope.
fn adder(label: &'static str) -> Box<dyn Widget> {
    Box::new(component(move |cx: &BuildContext| {
        let cart = cx.read::<Cart>();
        Box::new(Button::new(label).on_press(move || cart.update(|c| c.items += 1)))
            as Box<dyn Widget>
    }))
}

/// What an overlay shows: a reader, a button that adds through the cart in scope, and a list
/// whose row is built on demand, while the overlay is painted.
fn content() -> Column {
    Column::new()
        .child(reader("overlay"))
        .child(adder("Add in overlay"))
        .child(
            Container::new()
                .width(220.0)
                .height(40.0)
                .child(ListView::builder(1, |_| reader("row"))),
        )
}

fn nothing() -> Callback {
    Callback::new(|| {})
}

/// The page: its own cart in scope, a reader of it, and what `overlay` makes, under an
/// application whose root puts another cart in scope.
fn driver(overlay: impl Fn() -> Box<dyn Widget> + 'static) -> Driver<FrusApp> {
    frus_widgets::__use_memory_prefs();
    BUILT.with(|b| b.borrow_mut().clear());
    let overlay = std::rc::Rc::new(overlay);
    let root_cart = Model::new(Cart { items: 100 });
    let app = FrusApp::from_fn(move |_cx| {
        let overlay = overlay.clone();
        let page = component(move |cx: &BuildContext| {
            let cart = cx.use_model(Cart::default);
            PAGE_CART.with(|p| *p.borrow_mut() = Some(cart.clone()));
            Box::new(Scoped::new(
                cart,
                Column::new().child(reader("page")).child(overlay()),
            )) as Box<dyn Widget>
        });
        Box::new(Scoped::new(root_cart.clone(), page)) as Box<dyn Widget>
    });
    let mut driver = Driver::new(app, 800.0, 900.0);
    settle(&mut driver);
    driver
}

fn settle(driver: &mut Driver<FrusApp>) {
    for _ in 0..40 {
        driver.frame(1.0 / 60.0);
    }
}

fn shows(driver: &Driver<FrusApp>, text: &str) -> bool {
    driver.texts().iter().any(|(shown, _)| shown == text)
}

/// The overlay's readers see the page's cart, a change to it builds them, and, where the
/// overlay has the button, a press inside the overlay reaches the page.
fn sees_the_page(kind: &str, driver: &mut Driver<FrusApp>, readers: &[&str], press: bool) {
    for tag in readers {
        assert!(
            shows(driver, &format!("{tag} sees 0")),
            "{kind}: {tag} sees the page's cart: {:?}",
            driver.texts()
        );
    }
    // Changed from outside: the readers in the overlay are built again, once each.
    let before: Vec<u32> = readers.iter().map(|t| count(t)).collect();
    PAGE_CART.with(|p| p.borrow().as_ref().unwrap().update(|c| c.items += 1));
    settle(driver);
    for (tag, before) in readers.iter().zip(before) {
        assert!(
            shows(driver, &format!("{tag} sees 1")),
            "{kind}: {tag} follows the page's cart: {:?}",
            driver.texts()
        );
        // A list builds the rows in view on every frame it paints; the other readers are
        // built by the change alone.
        if *tag != "row" {
            assert_eq!(count(tag) - before, 1, "{kind}: {tag} built once");
        }
    }
    assert!(shows(driver, "page sees 1"), "{kind}");
    if press {
        assert!(
            driver.tap_text("Add in overlay"),
            "{kind}: the button is there"
        );
        settle(driver);
        assert!(
            shows(driver, "overlay sees 2") && shows(driver, "page sees 2"),
            "{kind}: a press in the overlay changes the page's cart: {:?}",
            driver.texts()
        );
    }
}

fn anchor() -> Container {
    Container::new().width(120.0).height(40.0)
}

/// **Every placement of the generic portal**: the content sees the page's cart.
#[test]
fn a_portal_sees_the_page_at_every_placement() {
    for placement in [
        Placement::Below,
        Placement::Center,
        Placement::Beside,
        Placement::Left,
        Placement::Right,
        Placement::Bottom,
        Placement::Covering,
        Placement::Pointer,
        Placement::Notification,
    ] {
        let mut d =
            driver(move || Box::new(OverlayPortal::new(anchor()).overlay(content(), placement)));
        sees_the_page(&format!("{placement:?}"), &mut d, &["overlay", "row"], true);
    }
}

#[test]
fn a_dialog_sees_the_page() {
    let mut d = driver(|| Box::new(Dialog::new(true).child(content()).body(anchor())));
    sees_the_page("Dialog", &mut d, &["overlay", "row"], true);
}

#[test]
fn an_alert_dialog_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            AlertDialog::new(true)
                .title("Cart")
                .content_widget(content())
                .action(Button::new("Close"))
                .body(anchor()),
        )
    });
    sees_the_page("AlertDialog", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_cupertino_alert_dialog_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            CupertinoAlertDialog::new(true)
                .title("Cart")
                .content_widget(content())
                .body(anchor()),
        )
    });
    sees_the_page("CupertinoAlertDialog", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_bottom_sheet_sees_the_page() {
    let mut d = driver(|| Box::new(BottomSheet::new(true).sheet(content()).body(anchor())));
    sees_the_page("BottomSheet", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_drawer_sees_the_page() {
    let mut d = driver(|| Box::new(Drawer::new(true).panel(content()).body(anchor())));
    sees_the_page("Drawer", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_menu_anchor_sees_the_page() {
    let mut d = driver(|| Box::new(MenuAnchor::new(anchor(), true, nothing()).content(content())));
    sees_the_page("MenuAnchor", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_hover_card_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            HoverCard::new(Text::new("Hover me"))
                .open(true)
                .content(content()),
        )
    });
    sees_the_page("HoverCard", &mut d, &["overlay", "row"], true);
}

#[test]
fn a_popup_menu_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            PopupMenuButton::new(anchor(), true, nothing()).item_widget(reader("item"), nothing()),
        )
    });
    sees_the_page("PopupMenuButton", &mut d, &["item"], false);
}

#[test]
fn a_context_menu_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            ContextMenu::new(
                anchor(),
                &ContextMenuState::open_at(Point::new(40.0, 60.0)),
                |_| {},
            )
            .item(MenuItem::widget(reader("item"), nothing())),
        )
    });
    sees_the_page("ContextMenu", &mut d, &["item"], false);
}

#[test]
fn a_menu_bar_menu_sees_the_page() {
    let mut d = driver(|| {
        Box::new(
            MenuBar::new(&MenuPath::from_indices([0]), |_| {})
                .menu(SubmenuButton::new("File").item(MenuItem::widget(reader("item"), nothing()))),
        )
    });
    sees_the_page("MenuBar", &mut d, &["item"], false);
}

#[test]
fn a_dropdown_sees_the_page() {
    let mut d = driver(|| {
        Box::new(DropdownButton::new("Pick", nothing()).options_widgets(
            true,
            vec![DropdownOption::widget(reader("item"))],
            |_| {},
        ))
    });
    sees_the_page("DropdownButton", &mut d, &["item"], false);
}

#[test]
fn a_search_view_sees_the_page() {
    let mut d = driver(|| Box::new(SearchAnchor::new(true, "").suggestion(content())));
    sees_the_page("SearchAnchor", &mut d, &["overlay", "row"], true);
}

#[test]
fn an_open_container_sees_the_page() {
    let mut d = driver(|| Box::new(OpenContainer::new(true, Text::new("Closed"), content())));
    sees_the_page("OpenContainer", &mut d, &["overlay", "row"], true);
}

/// **A page pushed on the router sees what is above the router**, not what the page that
/// pushed it put in scope: the two pages are siblings under the router, as a route pushed on a
/// navigator is. What every page needs goes around the router (`FrusApp::builder`); a dialog
/// declared on the pushed page sees that page's scope, as everywhere else.
#[test]
fn a_pushed_page_sees_above_the_router_and_not_the_page_that_pushed_it() {
    use frus_widgets::{GoRoute, GoRouter};
    frus_widgets::__use_memory_prefs();
    let router = GoRouter::new(vec![
        GoRoute::new("/", |_, _| {
            component(|cx: &BuildContext| {
                let mine = cx.use_model(|| Cart { items: 7 });
                let router = cx.router();
                Box::new(Scoped::new(
                    mine,
                    Column::new()
                        .child(reader("home"))
                        .child(Button::new("Open").on_press(move || router.push("/detail"))),
                )) as Box<dyn Widget>
            })
        }),
        GoRoute::new("/detail", |_, _| {
            component(|cx: &BuildContext| {
                let mine = cx.use_model(|| Cart { items: 42 });
                Box::new(Column::new().child(reader("detail")).child(Scoped::new(
                    mine,
                    Dialog::new(true).child(reader("dialog")).body(anchor()),
                ))) as Box<dyn Widget>
            })
        }),
    ])
    .animate(false);
    let app_cart = Model::new(Cart { items: 100 });
    let app = FrusApp::router(router)
        .builder(move |_, pages| Box::new(Scoped::new(app_cart.clone(), pages)));
    let mut d = Driver::new(app, 800.0, 900.0);
    settle(&mut d);
    assert!(shows(&d, "home sees 7"), "{:?}", d.texts());
    assert!(d.tap_text("Open"));
    settle(&mut d);
    assert!(shows(&d, "detail sees 100"), "{:?}", d.texts());
    assert!(shows(&d, "dialog sees 42"), "{:?}", d.texts());
}

/// **A component in a dialog's panel is built before it is measured**, whether it reads a model
/// or not. The panel is not among the dialog's children (those are the page behind), so no
/// walk built it before the layout cache took the overlay's fingerprint, which read the
/// children of a component never built: a panic in a debug build.
#[test]
fn a_component_in_a_dialogs_panel_is_built_before_it_is_measured() {
    frus_widgets::__use_memory_prefs();
    let app = FrusApp::from_fn(|_cx| {
        let words =
            component(|_cx: &BuildContext| Box::new(Text::new("Inside")) as Box<dyn Widget>);
        Box::new(Dialog::new(true).child(words).body(anchor())) as Box<dyn Widget>
    });
    let mut d = Driver::new(app, 800.0, 900.0);
    settle(&mut d);
    assert!(shows(&d, "Inside"), "{:?}", d.texts());
}
