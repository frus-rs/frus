// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

use std::cell::RefCell;
use std::rc::Rc;

use futures_lite::future::block_on;

use super::*;

fn store() -> (FakeStore, Purchases) {
    let fake = FakeStore::new()
        .product(Product::new(
            "coins",
            ProductKind::NonConsumable,
            "100 coins",
            "$0.99",
            990_000,
            "USD",
        ))
        .product(Product::new(
            "pro",
            ProductKind::NonConsumable,
            "Pro",
            "$4.99",
            4_990_000,
            "USD",
        ));
    let purchases = Purchases::fake(fake.clone());
    purchases.set_consumables(["coins"]);
    (fake, purchases)
}

/// The updates the listeners heard, once the interface's thread has run its tasks.
fn heard(purchases: &Purchases) -> (Rc<RefCell<Vec<PurchaseUpdate>>>, PurchaseSubscription) {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let h = heard.clone();
    let subscription =
        purchases.add_listener(move |updates| h.borrow_mut().extend_from_slice(updates));
    (heard, subscription)
}

fn frame() {
    while frus_widgets::run_local_tasks() {}
}

fn product(purchases: &Purchases, id: &str) -> Product {
    let response = block_on(purchases.query_products([id])).unwrap();
    response.products.into_iter().next().unwrap()
}

#[test]
fn products_are_found_and_the_unknown_ids_named() {
    let (_, purchases) = store();
    let response = block_on(purchases.query_products(["pro", "coins", "gems"])).unwrap();
    let ids: Vec<_> = response.products.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["pro", "coins"]);
    assert_eq!(response.not_found, ["gems"]);
    // Named a consumable by the application.
    assert_eq!(response.products[1].kind, ProductKind::Consumable);
    assert_eq!(response.products[0].kind, ProductKind::NonConsumable);
    assert!((response.products[0].raw_price() - 4.99).abs() < 1e-9);
}

#[test]
fn a_purchase_is_reported_then_completed() {
    let (fake, purchases) = store();
    let (heard, _s) = heard(&purchases);
    let pro = product(&purchases, "pro");
    block_on(purchases.buy(&pro)).unwrap();
    assert!(
        heard.borrow().is_empty(),
        "updates come on the interface's thread"
    );
    frame();
    let update = heard.borrow()[0].clone();
    assert_eq!(update.status, PurchaseStatus::Purchased);
    assert!(update.pending_complete);
    assert!(update.purchase_id.is_some());
    block_on(purchases.complete(&update)).unwrap();
    assert_eq!(fake.owned(), ["pro"], "a non-consumable is kept");
    assert_eq!(fake.calls().last().unwrap(), "acknowledge pro");

    // Bought again: the store says it is owned.
    block_on(purchases.buy(&pro)).unwrap();
    frame();
    assert_eq!(
        heard.borrow()[1].status,
        PurchaseStatus::Error(PurchaseError::ItemAlreadyOwned)
    );
}

#[test]
fn a_consumable_is_consumed_and_can_be_bought_again() {
    let (fake, purchases) = store();
    let (heard, _s) = heard(&purchases);
    let coins = product(&purchases, "coins");
    block_on(purchases.buy(&coins)).unwrap();
    frame();
    let update = heard.borrow()[0].clone();
    assert_eq!(update.kind, ProductKind::Consumable);
    block_on(purchases.complete(&update)).unwrap();
    assert_eq!(fake.calls().last().unwrap(), "consume coins");
    assert!(fake.owned().is_empty());
    block_on(purchases.buy(&coins)).unwrap();
    frame();
    assert_eq!(heard.borrow()[1].status, PurchaseStatus::Purchased);
    // Completing twice: the first consumed it.
    block_on(purchases.complete(&heard.borrow()[1].clone())).unwrap();
    assert_eq!(
        block_on(purchases.complete(&heard.borrow()[1].clone())),
        Err(PurchaseError::ItemNotOwned)
    );
}

#[test]
fn cancellations_errors_and_pending_payments() {
    let (fake, purchases) = store();
    let (heard, _s) = heard(&purchases);
    let pro = product(&purchases, "pro");
    fake.next_outcome(FakeOutcome::Canceled);
    block_on(purchases.buy(&pro)).unwrap();
    fake.next_outcome(FakeOutcome::Error(PurchaseError::ServiceUnavailable(
        "offline".into(),
    )));
    block_on(purchases.buy(&pro)).unwrap();
    fake.next_outcome(FakeOutcome::Pending);
    block_on(purchases.buy(&pro)).unwrap();
    frame();
    let statuses: Vec<_> = heard.borrow().iter().map(|u| u.status.clone()).collect();
    assert_eq!(
        statuses,
        [
            PurchaseStatus::Canceled,
            PurchaseStatus::Error(PurchaseError::ServiceUnavailable("offline".into())),
            PurchaseStatus::Pending,
        ]
    );
    assert!(
        !heard.borrow()[2].pending_complete,
        "nothing to complete yet"
    );
    assert!(fake.owned().is_empty());
    assert!(fake.clear_pending("pro"));
    frame();
    assert_eq!(heard.borrow()[3].status, PurchaseStatus::Purchased);
    assert!(heard.borrow()[3].pending_complete);
    assert_eq!(fake.owned(), ["pro"]);
    assert!(!fake.clear_pending("pro"), "nothing is waiting any more");
}

#[test]
fn restore_reports_what_the_account_owns() {
    let (fake, purchases) = store();
    fake.own("pro");
    let (heard, _s) = heard(&purchases);
    block_on(purchases.restore()).unwrap();
    frame();
    let heard = heard.borrow();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].product_id, "pro");
    assert_eq!(heard[0].status, PurchaseStatus::Restored);
    // Completed before: nothing to do, and the store is not asked.
    block_on(purchases.complete(&heard[0])).unwrap();
    assert_eq!(fake.calls(), ["restore"]);
}

#[test]
fn an_unavailable_store_says_so() {
    let (fake, purchases) = store();
    fake.set_available(false);
    assert!(!block_on(purchases.is_available()));
    assert!(matches!(
        block_on(purchases.query_products(["pro"])),
        Err(PurchaseError::BillingUnavailable(_))
    ));
    fake.set_available(true);
    assert!(block_on(purchases.is_available()));
}

#[test]
fn updates_before_any_listener_go_to_the_first() {
    let (_, purchases) = store();
    let pro = product(&purchases, "pro");
    block_on(purchases.buy(&pro)).unwrap();
    frame();
    let (heard, subscription) = heard(&purchases);
    assert!(heard.borrow().is_empty(), "not from inside add_listener");
    frame();
    assert_eq!(heard.borrow().len(), 1);
    drop(subscription);
    let coins = product(&purchases, "coins");
    block_on(purchases.buy(&coins)).unwrap();
    frame();
    assert_eq!(
        heard.borrow().len(),
        1,
        "a dropped subscription hears nothing"
    );
}

#[test]
fn a_product_not_sold_cannot_be_bought() {
    let (_, purchases) = store();
    let gems = Product::new(
        "gems",
        ProductKind::Consumable,
        "Gems",
        "$1",
        1_000_000,
        "USD",
    );
    assert!(matches!(
        block_on(purchases.buy(&gems)),
        Err(PurchaseError::ItemUnavailable(_))
    ));
}

#[test]
fn the_platform_store_here_is_unsupported() {
    let purchases = Purchases::instance();
    assert_eq!(purchases, Purchases::instance(), "one per thread");
    assert!(!block_on(purchases.is_available()));
    assert!(matches!(
        block_on(purchases.query_products(["pro"])),
        Err(PurchaseError::Unsupported(_))
    ));
    assert!(matches!(
        block_on(purchases.restore()),
        Err(PurchaseError::Unsupported(_))
    ));
}

#[test]
fn an_answer_from_another_thread_wakes_the_caller() {
    let (reply, replier) = Reply::<u32>::new();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        replier.send(7);
    });
    assert_eq!(block_on(reply), Some(7));
    thread.join().unwrap();
    // A sender that goes without answering does not leave the caller waiting.
    let (reply, replier) = Reply::<u32>::new();
    drop(replier);
    assert_eq!(block_on(reply), None);
}

#[test]
fn updates_from_another_thread_reach_the_listener() {
    let (_, purchases) = store();
    let (heard, _s) = heard(&purchases);
    let sink = purchases.inner.sink.clone();
    std::thread::spawn(move || {
        sink.send(vec![PurchaseUpdate::bare("pro", PurchaseStatus::Canceled)]);
    })
    .join()
    .unwrap();
    frame();
    assert_eq!(heard.borrow()[0].status, PurchaseStatus::Canceled);
}
