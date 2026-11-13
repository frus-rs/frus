# In-app purchases

An application sells digital goods through the platform's store: coins, an upgrade, a
subscription. frus's `Purchases` asks the store for the products, opens its purchase sheet,
reports how each purchase goes, and completes it once the application has delivered.

Turn on the `purchases` feature:

```toml
[dependencies]
frus = { version = "0.5", features = ["purchases"] }
```

| Platform | Store |
| --- | --- |
| Android | Google Play Billing |
| iOS, macOS | not yet: every call answers `PurchaseError::Unsupported` |
| Windows, Linux, the web | none: `PurchaseError::Unsupported` |
| Everywhere | the test store, `Purchases::fake(FakeStore)` |

## The flow

1. **Listen first.** Updates come through a listener, on the interface's thread. Add it when
   the application starts: a purchase made but not completed last time (the application was
   closed, a slow payment went through) is reported again.
2. **Query the products** by the ids they have in the store's console.
3. **Buy** one. The store shows its own sheet; the outcome comes to the listener.
4. **Deliver** what was bought when the status is `Purchased` or `Restored`, then
   **complete** the purchase. A store refunds a purchase left uncompleted (Google Play does
   after three days).

```rust,no_run
use frus::{spawn_local, PurchaseStatus, Purchases};

fn start_selling() {
    let store = Purchases::instance();
    // Google Play does not tell a consumable from a non-consumable product: say which ids are
    // used up and bought again. `complete` consumes those and acknowledges the others.
    store.set_consumables(["coins_100"]);

    let s = store.clone();
    store
        .add_listener(move |updates| {
            for update in updates {
                match &update.status {
                    PurchaseStatus::Pending => { /* say the payment is waiting */ }
                    PurchaseStatus::Purchased | PurchaseStatus::Restored => {
                        // Check `update.server_verification_data` on your server, deliver
                        // the product, then complete the purchase.
                        let (s, update) = (s.clone(), update.clone());
                        spawn_local(async move {
                            if let Err(error) = s.complete(&update).await {
                                eprintln!("could not complete {}: {error}", update.product_id);
                            }
                        });
                    }
                    PurchaseStatus::Error(error) => eprintln!("purchase failed: {error}"),
                    PurchaseStatus::Canceled => {}
                }
            }
        })
        .forget();
}

async fn buy_coins() {
    let store = Purchases::instance();
    if !store.is_available().await {
        return; // no store on this device: hide the shop
    }
    match store.query_products(["coins_100", "pro"]).await {
        Ok(found) => {
            for product in &found.products {
                println!("{}: {} ({:?})", product.title, product.price, product.kind);
            }
            if let Some(coins) = found.products.iter().find(|p| p.id == "coins_100") {
                // Answers once the sheet is open; the purchase itself comes to the listener.
                let _ = store.buy(coins).await;
            }
        }
        Err(error) => eprintln!("the store did not answer: {error}"),
    }
}
```

`restore()` asks the store for what the account owns: each product comes to the listener
as `Restored`. Offer it behind a "Restore purchases" button.

`buy_for(product, Some(account_id))` sends an opaque id of the person's account with the
purchase (a hash, never an e-mail address): the store uses it against fraud, and a server
can match purchases to accounts.

## What a product and an update hold

`Product`: `id`, `title`, `description`, `price` (formatted for the person's country),
`raw_price_micros` and `raw_price()`, `currency_code`, `kind` (`Consumable`,
`NonConsumable`, `Subscription`) and, for a subscription, its `subscription_period`
(ISO 8601, `"P1M"`).

`PurchaseUpdate`: `product_id`, `status` (`Pending`, `Purchased`, `Restored`,
`Error(PurchaseError)`, `Canceled`), `purchase_id` (the order id), `transaction_date`,
`server_verification_data` (Google Play's purchase token, for your server to check with
Google's Play Developer API), `local_verification_data` and `signature`, `kind`,
`pending_complete` and `quantity`.

`PurchaseError`: `Unsupported`, `BillingUnavailable` (no Play Store, an account that cannot
pay), `ServiceUnavailable` (try later), `ItemUnavailable`, `ItemAlreadyOwned`,
`ItemNotOwned`, `DeveloperError`, `FeatureNotSupported`, and `Store { code, message }` for
anything else.

## Selling on Google Play

The Billing Library is Java and comes from Google. `frus build` and `frus run` see that the
application uses purchases and put the library into the package, with its activity and the
`com.android.vending.BILLING` permission: there is nothing to declare. The first build
fetches the library from Google's Maven repository into `~/.frus/android` and shrinks it
with the SDK's R8 to what purchases use (about 330 KB of code). An application that does not
turn the feature on gets none of it.

Products can only be queried and bought from a package Google Play knows: upload a build to
a testing track in the Play Console, create the products there, and install the application
as a tester. A package installed from the computer with another signature still connects,
but finds no products. A package made with `cargo apk` alone has no Billing Library: the
store then answers `Unsupported`, and the log says why.

Hot reload (`frus run --hot`) builds the package another way, without the library; buy with
`frus run --no-hot`.

## Trying the flow without a store

`FakeStore` sells the products given to it, in memory, on every platform. The application
decides how each purchase ends, so a shop's screens can be built and tested before the store
is set up:

```rust
use frus::{FakeOutcome, FakeStore, Product, ProductKind, Purchases};

let fake = FakeStore::new()
    .product(Product::new("coins_100", ProductKind::Consumable, "100 coins", "$0.99", 990_000, "USD"))
    .product(
        Product::new("pro", ProductKind::NonConsumable, "Pro", "$4.99", 4_990_000, "USD")
            .description("Every level, no ads"),
    );
// The next purchase waits for its payment; `fake.clear_pending("pro")` then completes it.
fake.next_outcome(FakeOutcome::Pending);
fake.own("pro"); // or: the account bought it before, and `restore` finds it
let store = Purchases::fake(fake.clone());
# let _ = store;
assert_eq!(fake.owned(), ["pro"]);
```

`FakeOutcome` is `Purchased` (the default), `Pending`, `Canceled` or `Error(PurchaseError)`.
`set_available(false)` makes every call fail as a device without a store does, and
`calls()` lists what the store was asked, for tests.
