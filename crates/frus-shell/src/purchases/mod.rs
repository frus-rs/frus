// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! In-app purchases (feature `purchases`, milestone 807): [`Purchases`], the store's products
//! ([`Product`]), buying one, restoring what the account owns, and the updates the store
//! reports ([`PurchaseUpdate`]), which the application completes ([`Purchases::complete`]).
//!
//! The controller lives on the interface's thread. A store answers from threads of its own
//! (Android's main thread): a call's answer comes back through a one-shot [`Reply`], and the
//! purchase updates through a [`Sink`], a queue whose waker is a task of the interface's
//! thread ([`spawn_local`]), which hands them to the listeners.
//!
//! - **Android:** Google Play Billing (`android.rs`).
//! - **Everywhere else:** no store; every call answers [`PurchaseError::Unsupported`] and
//!   [`Purchases::is_available`] answers `false`.
//! - **The test store** ([`Purchases::fake`], [`FakeStore`]): in memory, on every platform,
//!   for tests and for driving the flow without a store.

#[cfg(target_os = "android")]
mod android;
mod fake;
#[cfg(any(test, target_os = "android"))]
mod play;

use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use frus_widgets::{spawn_local, TaskHandle};

pub use fake::{FakeOutcome, FakeStore};

#[cfg(target_os = "android")]
pub(crate) use android::remember;

// ---------------------------------------------------------------------------------------
// What the store sells and reports
// ---------------------------------------------------------------------------------------

/// What kind of product the store sells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProductKind {
    /// Bought, used up and bought again: coins, lives. [`Purchases::complete`] consumes it.
    Consumable,
    /// Bought once and kept: an upgrade, a level pack. [`Purchases::complete`] acknowledges it.
    NonConsumable,
    /// Renewed every period until it is cancelled.
    Subscription,
}

/// A product, as the store describes it ([`Purchases::query_products`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    /// The id the product has in the store's console.
    pub id: String,
    /// Its title, in the person's language.
    pub title: String,
    /// Its description.
    pub description: String,
    /// The price, formatted by the store for the person's country: `"€1.99"`.
    pub price: String,
    /// The price in millionths of the currency's unit: 1,990,000 for 1.99.
    pub raw_price_micros: i64,
    /// The currency, as its ISO 4217 code: `"EUR"`.
    pub currency_code: String,
    /// Consumable, non-consumable or subscription.
    pub kind: ProductKind,
    /// A subscription's period, ISO 8601 (`"P1M"`), when the store gives one.
    pub subscription_period: Option<String>,
    /// The store's offer to buy (a subscription's base plan on Google Play).
    pub(crate) offer: Option<String>,
}

impl Product {
    /// A product described by the application itself, for the test store
    /// ([`FakeStore::product`]).
    pub fn new(
        id: impl Into<String>,
        kind: ProductKind,
        title: impl Into<String>,
        price: impl Into<String>,
        raw_price_micros: i64,
        currency_code: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            price: price.into(),
            raw_price_micros,
            currency_code: currency_code.into(),
            kind,
            subscription_period: None,
            offer: None,
        }
    }

    /// The same product with a description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// The price as a number of the currency's units: 1.99.
    pub fn raw_price(&self) -> f64 {
        self.raw_price_micros as f64 / 1_000_000.0
    }
}

/// The answer of [`Purchases::query_products`]: the products found, and the ids the store does
/// not know.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProductsResponse {
    /// The products found, in the store's order.
    pub products: Vec<Product>,
    /// The ids asked for that the store does not sell (a typo, a product not yet active).
    pub not_found: Vec<String>,
}

/// Where a purchase is.
#[derive(Clone, Debug, PartialEq)]
pub enum PurchaseStatus {
    /// Waiting for a payment that is not instant (cash at a shop, a parent's approval). Do
    /// not deliver yet: a later update says purchased, or error.
    Pending,
    /// Paid: deliver the product, then [`Purchases::complete`] the purchase.
    Purchased,
    /// Owned from before, found by [`Purchases::restore`]: deliver it again.
    Restored,
    /// The purchase failed.
    Error(PurchaseError),
    /// The person closed the store's sheet without buying.
    Canceled,
}

/// One update from the store, given to the listeners ([`Purchases::add_listener`]).
#[derive(Clone, Debug, PartialEq)]
pub struct PurchaseUpdate {
    /// The product bought.
    pub product_id: String,
    /// Where the purchase is.
    pub status: PurchaseStatus,
    /// The store's id for the transaction (Google Play's order id), when there is one.
    pub purchase_id: Option<String>,
    /// When the purchase was made, in milliseconds since 1970.
    pub transaction_date: Option<u64>,
    /// What a server checks the purchase with: Google Play's purchase token.
    pub server_verification_data: String,
    /// What the device can check it with: the purchase's JSON, signed by
    /// [`signature`](Self::signature).
    pub local_verification_data: String,
    /// The store's signature of `local_verification_data`.
    pub signature: String,
    /// The product's kind, which says what [`Purchases::complete`] does.
    pub kind: ProductKind,
    /// The purchase must be completed ([`Purchases::complete`]); a store refunds a purchase
    /// left uncompleted (Google Play: after three days).
    pub pending_complete: bool,
    /// How many were bought at once.
    pub quantity: u32,
}

impl PurchaseUpdate {
    /// An update with nothing but a product and a status: a cancellation, an error.
    pub(crate) fn bare(product_id: impl Into<String>, status: PurchaseStatus) -> Self {
        Self {
            product_id: product_id.into(),
            status,
            purchase_id: None,
            transaction_date: None,
            server_verification_data: String::new(),
            local_verification_data: String::new(),
            signature: String::new(),
            kind: ProductKind::NonConsumable,
            pending_complete: false,
            quantity: 1,
        }
    }
}

/// Why a call to the store failed, or a purchase did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PurchaseError {
    /// This platform has no store frus can reach, or the application was built without it.
    Unsupported(String),
    /// The store cannot sell on this device: no Play Store, an account that cannot pay, a
    /// country it does not sell in (Play's `BILLING_UNAVAILABLE`).
    BillingUnavailable(String),
    /// The store's service did not answer, or the network is down; try again later.
    ServiceUnavailable(String),
    /// The product cannot be bought: unknown, not active, or not queried first.
    ItemUnavailable(String),
    /// The account already owns this product; restore it, or complete the earlier purchase.
    ItemAlreadyOwned,
    /// The purchase to complete is not owned (already consumed).
    ItemNotOwned,
    /// The application asked for something the store refuses: a package not uploaded to the
    /// console, a wrong product type. The message says what.
    DeveloperError(String),
    /// The store does not support this on this device.
    FeatureNotSupported(String),
    /// Anything else the store reported: its code and its message.
    Store {
        /// The store's own code (Google Play's `BillingResponseCode`).
        code: i32,
        /// What it said.
        message: String,
    },
}

impl std::fmt::Display for PurchaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(why) => write!(f, "in-app purchases are not available here: {why}"),
            Self::BillingUnavailable(why) => write!(f, "the store cannot sell here: {why}"),
            Self::ServiceUnavailable(why) => write!(f, "the store did not answer: {why}"),
            Self::ItemUnavailable(why) => write!(f, "the product cannot be bought: {why}"),
            Self::ItemAlreadyOwned => f.write_str("the product is already owned"),
            Self::ItemNotOwned => f.write_str("the product is not owned"),
            Self::DeveloperError(why) => write!(f, "the store refused the request: {why}"),
            Self::FeatureNotSupported(why) => write!(f, "not supported by the store: {why}"),
            Self::Store { code, message } => write!(f, "store error {code}: {message}"),
        }
    }
}

impl std::error::Error for PurchaseError {}

// ---------------------------------------------------------------------------------------
// Answers from other threads
// ---------------------------------------------------------------------------------------

struct Slot<T> {
    value: Option<T>,
    waker: Option<Waker>,
    /// The sender went without answering.
    dropped: bool,
}

/// One answer, given from any thread ([`Replier`]) and awaited on the interface's.
pub(crate) struct Reply<T>(Arc<Mutex<Slot<T>>>);

/// Gives a [`Reply`] its answer, once.
pub(crate) struct Replier<T>(Option<Arc<Mutex<Slot<T>>>>);

impl<T> Reply<T> {
    pub(crate) fn new() -> (Self, Replier<T>) {
        let slot = Arc::new(Mutex::new(Slot {
            value: None,
            waker: None,
            dropped: false,
        }));
        (Self(slot.clone()), Replier(Some(slot)))
    }

    /// A reply already answered.
    pub(crate) fn ready(value: T) -> Self {
        let (reply, replier) = Self::new();
        replier.send(value);
        reply
    }
}

impl<T> Replier<T> {
    pub(crate) fn send(mut self, value: T) {
        if let Some(slot) = self.0.take() {
            let waker = slot.lock().ok().and_then(|mut slot| {
                slot.value = Some(value);
                slot.waker.take()
            });
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

impl<T> Drop for Replier<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.0.take() {
            let waker = slot.lock().ok().and_then(|mut slot| {
                slot.dropped = true;
                slot.waker.take()
            });
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

/// `None` when the store went without answering.
impl<T> Future for Reply<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let Ok(mut slot) = self.0.lock() else {
            return Poll::Ready(None);
        };
        if let Some(value) = slot.value.take() {
            Poll::Ready(Some(value))
        } else if slot.dropped {
            Poll::Ready(None)
        } else {
            slot.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

/// Where a store reports purchase updates, from any thread.
#[derive(Default)]
pub(crate) struct Sink {
    state: Mutex<(Vec<PurchaseUpdate>, Option<Waker>)>,
}

impl Sink {
    pub(crate) fn send(&self, updates: Vec<PurchaseUpdate>) {
        if updates.is_empty() {
            return;
        }
        let waker = self.state.lock().ok().and_then(|mut state| {
            state.0.extend(updates);
            state.1.take()
        });
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn poll_take(&self, cx: &mut Context<'_>) -> Poll<Vec<PurchaseUpdate>> {
        let Ok(mut state) = self.state.lock() else {
            return Poll::Pending;
        };
        if state.0.is_empty() {
            state.1 = Some(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(std::mem::take(&mut state.0))
        }
    }
}

// ---------------------------------------------------------------------------------------
// The store behind the controller
// ---------------------------------------------------------------------------------------

/// What the controller asks of a store. Each call answers through a [`Reply`]; purchase updates
/// go to the [`Sink`] the store was made with.
pub(crate) trait Store {
    fn is_available(&self) -> Reply<bool>;
    /// The products found (kinds as the store knows them: a one-time product is
    /// non-consumable) and the ids not found.
    fn query(&self, ids: Vec<String>) -> Reply<Result<ProductsResponse, PurchaseError>>;
    /// Opens the store's purchase sheet; answers once it is open (or could not be).
    fn buy(&self, product: &Product, account_id: Option<&str>) -> Reply<Result<(), PurchaseError>>;
    /// The purchases the account owns, reported as updates (restored, or pending).
    fn restore(&self) -> Reply<Result<(), PurchaseError>>;
    fn complete(&self, purchase: &PurchaseUpdate) -> Reply<Result<(), PurchaseError>>;
}

/// A platform with no store.
struct NoStore(&'static str);

impl NoStore {
    fn error(&self) -> PurchaseError {
        PurchaseError::Unsupported(self.0.to_string())
    }
}

impl Store for NoStore {
    fn is_available(&self) -> Reply<bool> {
        Reply::ready(false)
    }
    fn query(&self, _: Vec<String>) -> Reply<Result<ProductsResponse, PurchaseError>> {
        Reply::ready(Err(self.error()))
    }
    fn buy(&self, _: &Product, _: Option<&str>) -> Reply<Result<(), PurchaseError>> {
        Reply::ready(Err(self.error()))
    }
    fn restore(&self) -> Reply<Result<(), PurchaseError>> {
        Reply::ready(Err(self.error()))
    }
    fn complete(&self, _: &PurchaseUpdate) -> Reply<Result<(), PurchaseError>> {
        Reply::ready(Err(self.error()))
    }
}

#[cfg(not(target_os = "android"))]
fn platform_store(_sink: Arc<Sink>) -> Box<dyn Store> {
    Box::new(NoStore(
        if cfg!(any(target_os = "ios", target_os = "macos")) {
            "StoreKit is not supported yet"
        } else if cfg!(target_arch = "wasm32") {
            "the web has no app store"
        } else {
            "this platform has no app store frus supports"
        },
    ))
}

#[cfg(target_os = "android")]
fn platform_store(sink: Arc<Sink>) -> Box<dyn Store> {
    match android::PlayStore::new(sink) {
        Ok(store) => Box::new(store),
        Err(why) => Box::new(NoStore(why)),
    }
}

// ---------------------------------------------------------------------------------------
// The controller
// ---------------------------------------------------------------------------------------

type Listener = Rc<dyn Fn(&[PurchaseUpdate])>;

struct Inner {
    store: Box<dyn Store>,
    sink: Arc<Sink>,
    /// The ids the application sells as consumables: a one-time product's kind on stores
    /// that do not say (Google Play).
    consumables: RefCell<HashSet<String>>,
    listeners: RefCell<Vec<(u64, Listener)>>,
    next_listener: std::cell::Cell<u64>,
    /// Updates that came before any listener: the first one added hears them.
    unheard: RefCell<Vec<PurchaseUpdate>>,
    pump: RefCell<Option<TaskHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(pump) = self.pump.get_mut().take() {
            pump.cancel();
        }
    }
}

/// The application's store: its products, buying them, and what it reports.
///
/// Cheap to clone; every clone is the same store. [`Purchases::instance`] is the platform's
/// (one per thread), [`Purchases::fake`] an in-memory one for tests.
///
/// ```no_run
/// use frus::{Purchases, PurchaseStatus};
///
/// let store = Purchases::instance();
/// store.set_consumables(["coins_100"]);
/// // Listen before anything else: purchases left uncompleted are reported again.
/// let s = store.clone();
/// store.add_listener(move |updates| {
///     for update in updates {
///         if matches!(update.status, PurchaseStatus::Purchased | PurchaseStatus::Restored) {
///             // Deliver the product, then:
///             let (s, update) = (s.clone(), update.clone());
///             frus::spawn_local(async move { let _ = s.complete(&update).await; });
///         }
///     }
/// })
/// .forget();
/// let s = store.clone();
/// frus::spawn_local(async move {
///     if let Ok(found) = s.query_products(["coins_100", "pro"]).await {
///         if let Some(coins) = found.products.first() {
///             let _ = s.buy(coins).await;
///         }
///     }
/// });
/// ```
#[derive(Clone)]
pub struct Purchases {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for Purchases {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Purchases").finish_non_exhaustive()
    }
}

impl PartialEq for Purchases {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

thread_local! {
    // Already `const`; clippy 1.96 still asks for it on Android.
    #[allow(clippy::missing_const_for_thread_local)]
    static INSTANCE: RefCell<Option<Purchases>> = const { RefCell::new(None) };
}

impl Purchases {
    /// The platform's store: Google Play on Android; elsewhere a store that answers
    /// [`PurchaseError::Unsupported`]. The same one every time on a thread.
    #[inline(never)]
    pub fn instance() -> Purchases {
        INSTANCE.with(|instance| {
            instance
                .borrow_mut()
                .get_or_insert_with(|| Self::with_store(platform_store))
                .clone()
        })
    }

    /// An in-memory store with the products of `store`, for tests and for driving the flow
    /// without a real store. `store` stays in the caller's hands to steer it: what the next
    /// purchase does, a pending payment that clears.
    pub fn fake(store: FakeStore) -> Purchases {
        Self::with_store(move |sink| Box::new(store.attach(sink)))
    }

    fn with_store(make: impl FnOnce(Arc<Sink>) -> Box<dyn Store>) -> Purchases {
        let sink = Arc::new(Sink::default());
        let inner = Rc::new(Inner {
            store: make(sink.clone()),
            sink,
            consumables: RefCell::new(HashSet::new()),
            listeners: RefCell::new(Vec::new()),
            next_listener: std::cell::Cell::new(1),
            unheard: RefCell::new(Vec::new()),
            pump: RefCell::new(None),
        });
        let pump = spawn_local(pump(Rc::downgrade(&inner), inner.sink.clone()));
        *inner.pump.borrow_mut() = Some(pump);
        Purchases { inner }
    }

    /// Whether the store can be reached and can sell on this device.
    pub fn is_available(&self) -> impl Future<Output = bool> + 'static {
        let reply = self.inner.store.is_available();
        async move { reply.await.unwrap_or(false) }
    }

    /// Names the products sold as consumables. Google Play does not tell a consumable from a
    /// non-consumable product: the ones named here come back as [`ProductKind::Consumable`],
    /// and their purchases are consumed by [`complete`](Self::complete).
    pub fn set_consumables<I, S>(&self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        *self.inner.consumables.borrow_mut() = ids.into_iter().map(Into::into).collect();
    }

    fn kind_of(&self, id: &str, kind: ProductKind) -> ProductKind {
        if kind == ProductKind::NonConsumable && self.inner.consumables.borrow().contains(id) {
            ProductKind::Consumable
        } else {
            kind
        }
    }

    /// The store's products for `ids`: titles, prices in the person's currency, kinds.
    pub fn query_products<I, S>(
        &self,
        ids: I,
    ) -> impl Future<Output = Result<ProductsResponse, PurchaseError>> + 'static
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let ids: Vec<String> = ids.into_iter().map(Into::into).collect();
        let reply = self.inner.store.query(ids);
        let this = self.clone();
        async move {
            let mut response = reply.await.unwrap_or_else(gone)?;
            for product in &mut response.products {
                product.kind = this.kind_of(&product.id, product.kind);
            }
            Ok(response)
        }
    }

    /// Opens the store's purchase sheet for `product`, which must come from
    /// [`query_products`](Self::query_products). Answers once the sheet is open; how the
    /// purchase ends comes as an update to the listeners.
    pub fn buy(&self, product: &Product) -> impl Future<Output = Result<(), PurchaseError>> {
        self.buy_for(product, None)
    }

    /// [`buy`](Self::buy) for one of the application's accounts: `account_id` (an opaque id,
    /// a hash of the account's, never an e-mail address) goes with the purchase, for the
    /// store's fraud checks and for a server to match purchases to accounts.
    pub fn buy_for(
        &self,
        product: &Product,
        account_id: Option<&str>,
    ) -> impl Future<Output = Result<(), PurchaseError>> {
        let reply = self.inner.store.buy(product, account_id);
        async move { reply.await.unwrap_or_else(gone) }
    }

    /// Asks the store for what the account owns: each owned product comes as an update with
    /// [`PurchaseStatus::Restored`] (or `Pending`). Answers once the store has answered.
    pub fn restore(&self) -> impl Future<Output = Result<(), PurchaseError>> {
        let reply = self.inner.store.restore();
        async move { reply.await.unwrap_or_else(gone) }
    }

    /// Completes a purchase once its product is delivered: consumes a consumable (it can be
    /// bought again), acknowledges anything else. A purchase that does not need it
    /// ([`PurchaseUpdate::pending_complete`] false) answers at once.
    pub fn complete(
        &self,
        purchase: &PurchaseUpdate,
    ) -> impl Future<Output = Result<(), PurchaseError>> {
        let reply = if purchase.pending_complete {
            let mut purchase = purchase.clone();
            purchase.kind = self.kind_of(&purchase.product_id, purchase.kind);
            self.inner.store.complete(&purchase)
        } else {
            Reply::ready(Ok(()))
        };
        async move { reply.await.unwrap_or_else(gone) }
    }

    /// Calls `listener` with each batch of updates, on the interface's thread, until the
    /// returned subscription is dropped. Updates that came before the first listener are given
    /// to it, at the next frame.
    pub fn add_listener(
        &self,
        listener: impl Fn(&[PurchaseUpdate]) + 'static,
    ) -> PurchaseSubscription {
        let id = self.inner.next_listener.get();
        self.inner.next_listener.set(id + 1);
        let listener: Listener = Rc::new(listener);
        self.inner
            .listeners
            .borrow_mut()
            .push((id, listener.clone()));
        // Handed over at the next frame, as any update is, and not from inside this call: the
        // caller may be holding what its listener changes (a state being built).
        let unheard = std::mem::take(&mut *self.inner.unheard.borrow_mut());
        self.inner.sink.send(unheard);
        PurchaseSubscription {
            inner: Rc::downgrade(&self.inner),
            id,
        }
    }

    fn deliver(&self, mut updates: Vec<PurchaseUpdate>) {
        for update in &mut updates {
            update.kind = self.kind_of(&update.product_id, update.kind);
        }
        let listeners: Vec<Listener> = self
            .inner
            .listeners
            .borrow()
            .iter()
            .map(|(_, l)| l.clone())
            .collect();
        if listeners.is_empty() {
            self.inner.unheard.borrow_mut().extend(updates);
            return;
        }
        for listener in listeners {
            listener(&updates);
        }
    }
}

fn gone<T>() -> Result<T, PurchaseError> {
    Err(PurchaseError::ServiceUnavailable(
        "the store went without answering".to_string(),
    ))
}

/// Hands the store's updates to the listeners, on the interface's thread, until the controller
/// is gone.
async fn pump(inner: Weak<Inner>, sink: Arc<Sink>) {
    loop {
        let updates = std::future::poll_fn(|cx| sink.poll_take(cx)).await;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        Purchases { inner }.deliver(updates);
    }
}

/// A listener of [`Purchases::add_listener`]: dropping it stops the listener.
#[must_use = "dropping the subscription stops the listener; call .forget() to keep it"]
pub struct PurchaseSubscription {
    inner: Weak<Inner>,
    id: u64,
}

impl PurchaseSubscription {
    /// Keeps listening for as long as the store lives.
    pub fn forget(self) {
        std::mem::forget(self);
    }
}

impl Drop for PurchaseSubscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner
                .listeners
                .borrow_mut()
                .retain(|(id, _)| *id != self.id);
        }
    }
}

#[cfg(test)]
mod tests;
