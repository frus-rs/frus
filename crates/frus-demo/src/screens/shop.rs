// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The in-app purchases screen (milestone 807): a shop selling coins (consumable), an upgrade
//! (non-consumable) and a subscription.
//!
//! Two stores: the test store, in memory, which sells the three and lets the next purchase be
//! paid, wait for its payment, be canceled or fail; and the platform's (Google Play on
//! Android), which answers what it can for a package the Play Console does not know: whether
//! it connects, and that it sells none of these products.

use crate::prelude::*;
use frus_shell::{
    FakeOutcome, FakeStore, Product, ProductKind, PurchaseError, PurchaseStatus,
    PurchaseSubscription, PurchaseUpdate, Purchases,
};
use frus_widgets::{column, row, Button, StateHandle};

const IDS: [&str; 3] = ["coins_100", "pro", "plus_monthly"];

/// The screen.
pub(crate) struct ShopPage;

/// What the screen keeps.
pub(crate) struct ShopState {
    /// 0: the test store; 1: the platform's.
    source: usize,
    store: Purchases,
    fake: FakeStore,
    next: usize,
    products: Vec<Product>,
    status: String,
    log: Vec<String>,
    coins: u32,
    pro: bool,
    _listening: Option<PurchaseSubscription>,
}

const OUTCOMES: [&str; 4] = ["Paid", "Pending", "Canceled", "Fails"];

fn fake_store() -> FakeStore {
    FakeStore::new()
        .product(
            Product::new(
                "coins_100",
                ProductKind::Consumable,
                "100 coins",
                "$0.99",
                990_000,
                "USD",
            )
            .description("A bag of coins to spend"),
        )
        .product(
            Product::new(
                "pro",
                ProductKind::NonConsumable,
                "Pro",
                "$4.99",
                4_990_000,
                "USD",
            )
            .description("Every level, no ads"),
        )
        .product(
            Product::new(
                "plus_monthly",
                ProductKind::Subscription,
                "Plus",
                "$1.99",
                1_990_000,
                "USD",
            )
            .description("Monthly perks"),
        )
}

impl StatefulWidget for ShopPage {
    type State = ShopState;

    fn create_state(&self) -> ShopState {
        let fake = fake_store();
        ShopState {
            source: 0,
            store: Purchases::fake(fake.clone()),
            fake,
            next: 0,
            products: Vec::new(),
            status: String::new(),
            log: Vec::new(),
            coins: 0,
            pro: false,
            _listening: None,
        }
    }
}

/// Listens to `state`'s store and asks it for the products.
fn open(handle: StateHandle<ShopState>, state: &mut ShopState) {
    state.store.set_consumables(["coins_100"]);
    let listener = handle.clone();
    let store = state.store.clone();
    state._listening = Some(state.store.add_listener(move |updates| {
        for update in updates {
            heard(&listener, &store, update);
        }
    }));
    state.products.clear();
    state.status = "Asking the store…".into();
    let store = state.store.clone();
    handle.clone().spawn(async move {
        let available = store.is_available().await;
        let found = store.query_products(IDS).await;
        log::info!("purchases: available {available}, products {found:?}");
        handle.set_state(|s| match found {
            Ok(found) => {
                s.status = format!(
                    "Store {}; {} product(s){}",
                    if available {
                        "available"
                    } else {
                        "not available"
                    },
                    found.products.len(),
                    if found.not_found.is_empty() {
                        String::new()
                    } else {
                        format!(", not sold: {}", found.not_found.join(", "))
                    }
                );
                s.products = found.products;
            }
            Err(err) => {
                s.status = format!(
                    "Store {}: {err}",
                    if available {
                        "available"
                    } else {
                        "not available"
                    }
                )
            }
        });
    });
}

/// One update: shown, and a purchase paid for is delivered, then completed.
fn heard(handle: &StateHandle<ShopState>, store: &Purchases, update: &PurchaseUpdate) {
    let line = format!("{}: {:?}", update.product_id, update.status);
    let paid = matches!(
        update.status,
        PurchaseStatus::Purchased | PurchaseStatus::Restored
    );
    handle.set_state(|s| {
        s.log.insert(0, line);
        s.log.truncate(8);
        if paid {
            match update.product_id.as_str() {
                "coins_100" if update.status == PurchaseStatus::Purchased => s.coins += 100,
                "pro" => s.pro = true,
                _ => {}
            }
        }
    });
    if paid {
        let (store, update, handle) = (store.clone(), update.clone(), handle.clone());
        handle.clone().spawn(async move {
            let done = store.complete(&update).await;
            handle.set_state(|s| {
                s.log.insert(
                    0,
                    match done {
                        Ok(()) => format!("{}: completed", update.product_id),
                        Err(err) => format!("{}: not completed, {err}", update.product_id),
                    },
                );
                s.log.truncate(8);
            });
        });
    }
}

impl State for ShopState {
    type Widget = ShopPage;

    fn init_state(&mut self, cx: &StateContext<Self>) {
        open(cx.handle(), self);
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let handle = cx.handle();

        let sources = SegmentedButton::new(self.source, {
            let handle = handle.clone();
            move |index: usize| {
                let handle = handle.clone();
                Callback::new(move || {
                    let reopen = handle.clone();
                    handle.set_state(|s| {
                        if s.source != index {
                            s.source = index;
                            s.store = if index == 0 {
                                Purchases::fake(s.fake.clone())
                            } else {
                                Purchases::instance()
                            };
                            open(reopen, s);
                        }
                    });
                })
            }
        })
        .segment("Test store")
        .segment("Platform store");

        let mut shelf = column![].gap(8.0);
        for product in &self.products {
            let buy = {
                let (handle, product) = (handle.clone(), product.clone());
                handle.clone().async_callback(move |_| {
                    let (handle, product) = (handle.clone(), product.clone());
                    async move {
                        let store = handle.read(|s| s.store.clone());
                        if let Err(err) = store.buy(&product).await {
                            handle.set_state(|s| s.status = err.to_string());
                        }
                    }
                })
            };
            shelf = shelf.child(
                row![
                    Expanded::new(
                        column![
                            text(format!("{} · {}", product.title, product.price)).size(16.0),
                            text(format!("{:?} — {}", product.kind, product.description))
                                .size(12.0)
                                .color(theme.muted)
                                .wrap(),
                        ]
                        .gap(2.0)
                    ),
                    Button::new("Buy").on_press(buy),
                ]
                .gap(8.0),
            );
        }

        let restore = {
            let handle = handle.clone();
            handle.clone().async_callback(move |_| {
                let handle = handle.clone();
                async move {
                    let store = handle.read(|s| s.store.clone());
                    let result = store.restore().await;
                    handle.set_state(|s| {
                        s.status = match result {
                            Ok(()) => "Restored what the account owns".into(),
                            Err(err) => err.to_string(),
                        }
                    });
                }
            })
        };
        let mut controls = column![Button::new("Restore purchases")
            .variant(Variant::Outlined)
            .on_press(restore)]
        .gap(8.0);
        if self.source == 0 {
            let mut next = SegmentedButton::new(
                self.next,
                cx.handler(|s: &mut ShopState, index: usize| {
                    s.next = index;
                    s.fake.next_outcome(match index {
                        1 => FakeOutcome::Pending,
                        2 => FakeOutcome::Canceled,
                        3 => FakeOutcome::Error(PurchaseError::ServiceUnavailable(
                            "the test store's network is down".into(),
                        )),
                        _ => FakeOutcome::Purchased,
                    });
                }),
            );
            for label in OUTCOMES {
                next = next.segment(label);
            }
            let clear = cx.handler(|s: &mut ShopState, ()| {
                for id in IDS {
                    s.fake.clear_pending(id);
                }
                s.next = 0;
            });
            controls = controls
                .child(text("The next purchase:").size(13.0).color(theme.muted))
                .child(next)
                .child(
                    Button::new("Pay the pending purchases")
                        .variant(Variant::Outlined)
                        .on_press(clear(())),
                );
        }

        let mut log = column![].gap(4.0);
        for line in &self.log {
            log = log.child(text(line.clone()).size(12.0).color(theme.muted).wrap());
        }

        let Size { width, height } = surface();
        let router = cx.router();
        let content = SingleChildScrollView::new().flex(1.0).child(
            column![
                sources,
                text(self.status.clone())
                    .size(14.0)
                    .color(theme.muted)
                    .wrap(),
                text(format!(
                    "{} coins · Pro {}",
                    self.coins,
                    if self.pro { "unlocked" } else { "locked" }
                ))
                .size(18.0)
                .weight(FontWeight::Bold),
                shelf,
                controls,
                log,
            ]
            .gap(12.0)
            .padding(16.0),
        );
        let screen = column![
            NavigationBar::new("In-app purchases").on_back(on(move || {
                router.pop();
            })),
            content
        ]
        .flex(1.0);
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(theme.background)
                .child(SafeArea::new(screen)),
        )
    }
}
