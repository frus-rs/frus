// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The test store: products, purchases and an account, in memory.

use std::sync::{Arc, Mutex};

use super::{
    Product, ProductKind, ProductsResponse, PurchaseError, PurchaseStatus, PurchaseUpdate, Reply,
    Sink, Store,
};

/// How the next purchase of the test store ends.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum FakeOutcome {
    /// Paid at once (the default).
    #[default]
    Purchased,
    /// Waiting for a payment: [`FakeStore::clear_pending`] or [`FakeStore::fail_pending`] ends
    /// it.
    Pending,
    /// The person closes the sheet.
    Canceled,
    /// The store reports this error.
    Error(PurchaseError),
}

#[derive(Default)]
struct State {
    products: Vec<Product>,
    available: bool,
    next: FakeOutcome,
    /// What the account owns: the purchases made and not consumed.
    owned: Vec<PurchaseUpdate>,
    /// Purchases waiting for their payment.
    pending: Vec<PurchaseUpdate>,
    sink: Option<Arc<Sink>>,
    serial: u64,
    /// The calls made, for tests: `"query"`, `"buy coins"`, `"consume coins"`…
    log: Vec<String>,
}

/// An in-memory store for [`Purchases::fake`](super::Purchases::fake): it sells the products
/// given to it, and the application decides how each purchase ends ([`FakeStore::next_outcome`]).
/// Cheap to clone; every clone is the same store. It answers on the calling thread, and its
/// updates reach the listeners at the next frame, as a real store's do.
///
/// ```
/// use frus_shell::{FakeOutcome, FakeStore, Product, ProductKind, Purchases};
///
/// let fake = FakeStore::new()
///     .product(Product::new("coins_100", ProductKind::Consumable, "100 coins", "$0.99", 990_000, "USD"))
///     .product(Product::new("pro", ProductKind::NonConsumable, "Pro", "$4.99", 4_990_000, "USD"));
/// fake.next_outcome(FakeOutcome::Pending);
/// let store = Purchases::fake(fake.clone());
/// # let _ = store;
/// ```
#[derive(Clone)]
pub struct FakeStore {
    state: Arc<Mutex<State>>,
}

impl Default for FakeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FakeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeStore").finish_non_exhaustive()
    }
}

impl FakeStore {
    /// A store that is available and sells nothing yet.
    pub fn new() -> Self {
        let store = Self {
            state: Arc::new(Mutex::new(State::default())),
        };
        store.with(|s| s.available = true);
        store
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut state)
    }

    /// Adds a product to sell.
    pub fn product(self, product: Product) -> Self {
        self.with(|s| s.products.push(product));
        self
    }

    /// Whether the store answers ([`Purchases::is_available`](super::Purchases::is_available));
    /// when it does not, every call fails with [`PurchaseError::BillingUnavailable`].
    pub fn set_available(&self, available: bool) {
        self.with(|s| s.available = available);
    }

    /// How the next purchase ends; after it, purchases are paid at once again.
    pub fn next_outcome(&self, outcome: FakeOutcome) {
        self.with(|s| s.next = outcome);
    }

    /// The account already owns `product_id`, from before: [`restore`](super::Purchases::restore)
    /// finds it.
    pub fn own(&self, product_id: &str) {
        self.with(|s| {
            let update = purchase(s, product_id, PurchaseStatus::Purchased);
            s.owned.push(PurchaseUpdate {
                pending_complete: false,
                ..update
            });
        });
    }

    /// The payment of the purchase waiting for `product_id` went through: it is reported
    /// purchased.
    pub fn clear_pending(&self, product_id: &str) -> bool {
        self.end_pending(product_id, None)
    }

    /// The payment of the purchase waiting for `product_id` failed: it is reported as an error.
    pub fn fail_pending(&self, product_id: &str, error: PurchaseError) -> bool {
        self.end_pending(product_id, Some(error))
    }

    fn end_pending(&self, product_id: &str, error: Option<PurchaseError>) -> bool {
        let (sink, update) = self
            .with(|s| {
                let at = s.pending.iter().position(|p| p.product_id == product_id)?;
                let mut update = s.pending.remove(at);
                match error {
                    None => {
                        update.status = PurchaseStatus::Purchased;
                        update.pending_complete = true;
                        s.owned.push(update.clone());
                    }
                    Some(error) => {
                        update.status = PurchaseStatus::Error(error);
                        update.pending_complete = false;
                    }
                }
                Some((s.sink.clone(), update))
            })
            .unzip();
        if let (Some(Some(sink)), Some(update)) = (sink, update) {
            sink.send(vec![update]);
            true
        } else {
            false
        }
    }

    /// The ids the account owns, a consumable until it is completed.
    pub fn owned(&self) -> Vec<String> {
        self.with(|s| s.owned.iter().map(|p| p.product_id.clone()).collect())
    }

    /// The calls the store received, oldest first: `"query"`, `"buy pro"`, `"restore"`,
    /// `"acknowledge pro"`, `"consume coins_100"`.
    pub fn calls(&self) -> Vec<String> {
        self.with(|s| s.log.clone())
    }

    pub(crate) fn attach(&self, sink: Arc<Sink>) -> FakeStore {
        self.with(|s| s.sink = Some(sink));
        self.clone()
    }

    fn unavailable() -> PurchaseError {
        PurchaseError::BillingUnavailable("the test store is set unavailable".to_string())
    }
}

fn purchase(s: &mut State, product_id: &str, status: PurchaseStatus) -> PurchaseUpdate {
    s.serial += 1;
    let kind = s
        .products
        .iter()
        .find(|p| p.id == product_id)
        .map_or(ProductKind::NonConsumable, |p| p.kind);
    PurchaseUpdate {
        product_id: product_id.to_string(),
        purchase_id: Some(format!("FAKE.{}", s.serial)),
        transaction_date: Some(
            web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
        ),
        server_verification_data: format!("fake-token-{}", s.serial),
        local_verification_data: format!("{{\"productId\":\"{product_id}\"}}"),
        signature: String::new(),
        kind,
        pending_complete: status == PurchaseStatus::Purchased,
        quantity: 1,
        status,
    }
}

impl Store for FakeStore {
    fn is_available(&self) -> Reply<bool> {
        Reply::ready(self.with(|s| s.available))
    }

    fn query(&self, ids: Vec<String>) -> Reply<Result<ProductsResponse, PurchaseError>> {
        Reply::ready(self.with(|s| {
            s.log.push("query".to_string());
            if !s.available {
                return Err(Self::unavailable());
            }
            let mut response = ProductsResponse::default();
            for id in ids {
                match s.products.iter().find(|p| p.id == id) {
                    Some(product) => response.products.push(product.clone()),
                    None => response.not_found.push(id),
                }
            }
            Ok(response)
        }))
    }

    fn buy(&self, product: &Product, _: Option<&str>) -> Reply<Result<(), PurchaseError>> {
        let result = self.with(|s| {
            s.log.push(format!("buy {}", product.id));
            if !s.available {
                return Err(Self::unavailable());
            }
            if !s.products.iter().any(|p| p.id == product.id) {
                return Err(PurchaseError::ItemUnavailable(product.id.clone()));
            }
            let owned = s.owned.iter().any(|p| p.product_id == product.id);
            let outcome = std::mem::take(&mut s.next);
            // As on Google Play: a consumable not consumed yet is owned too.
            let update = if owned {
                PurchaseUpdate::bare(
                    &product.id,
                    PurchaseStatus::Error(PurchaseError::ItemAlreadyOwned),
                )
            } else {
                match outcome {
                    FakeOutcome::Purchased => {
                        let update = purchase(s, &product.id, PurchaseStatus::Purchased);
                        s.owned.push(update.clone());
                        update
                    }
                    FakeOutcome::Pending => {
                        let update = purchase(s, &product.id, PurchaseStatus::Pending);
                        s.pending.push(update.clone());
                        update
                    }
                    FakeOutcome::Canceled => {
                        PurchaseUpdate::bare(&product.id, PurchaseStatus::Canceled)
                    }
                    FakeOutcome::Error(error) => {
                        PurchaseUpdate::bare(&product.id, PurchaseStatus::Error(error))
                    }
                }
            };
            Ok((s.sink.clone(), update))
        });
        Reply::ready(result.map(|(sink, update)| {
            if let Some(sink) = sink {
                sink.send(vec![update]);
            }
        }))
    }

    fn restore(&self) -> Reply<Result<(), PurchaseError>> {
        let result = self.with(|s| {
            s.log.push("restore".to_string());
            if !s.available {
                return Err(Self::unavailable());
            }
            let mut updates: Vec<PurchaseUpdate> = s
                .owned
                .iter()
                .map(|p| PurchaseUpdate {
                    status: PurchaseStatus::Restored,
                    ..p.clone()
                })
                .collect();
            updates.extend(s.pending.iter().cloned());
            Ok((s.sink.clone(), updates))
        });
        Reply::ready(result.map(|(sink, updates)| {
            if let Some(sink) = sink {
                sink.send(updates);
            }
        }))
    }

    fn complete(&self, purchase: &PurchaseUpdate) -> Reply<Result<(), PurchaseError>> {
        Reply::ready(self.with(|s| {
            let consume = purchase.kind == ProductKind::Consumable;
            s.log.push(format!(
                "{} {}",
                if consume { "consume" } else { "acknowledge" },
                purchase.product_id
            ));
            if !s.available {
                return Err(Self::unavailable());
            }
            let token = &purchase.server_verification_data;
            let Some(at) = s
                .owned
                .iter()
                .position(|p| &p.server_verification_data == token)
            else {
                return Err(PurchaseError::ItemNotOwned);
            };
            if consume {
                s.owned.remove(at);
            } else {
                s.owned[at].pending_complete = false;
            }
            Ok(())
        }))
    }
}
