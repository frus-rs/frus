# Milestone 807 — In-app purchases

## Objective

An application must be able to sell digital goods through the platform's store without a
plugin of its own: ask the store for its products and their local prices, open the purchase
sheet, hear how each purchase goes (paid, waiting for a payment, canceled, failed), restore
what the account owns, and complete a purchase once it is delivered. The shape is the one
mobile developers already know from the leading toolkit's in-app purchase plugin.

## API

```rust
use frus::{FakeStore, Product, ProductKind, PurchaseStatus, Purchases};

let store = Purchases::instance();              // the platform's store, one per thread
store.set_consumables(["coins_100"]);           // Google Play does not say which are
let _s = store.add_listener(|updates| { .. });  // PurchaseUpdate: product_id, status, ...
store.is_available().await;                     // bool
let found = store.query_products(["coins_100", "pro"]).await?;   // products + not_found
store.buy(&found.products[0]).await?;           // opens the sheet; the outcome comes as an update
store.buy_for(&product, Some(account_hash)).await?;
store.restore().await?;                         // owned products come as Restored updates
store.complete(&update).await?;                 // consume a consumable, acknowledge the rest

let fake = FakeStore::new().product(Product::new("pro", ProductKind::NonConsumable, ..));
let store = Purchases::fake(fake.clone());      // in memory, every platform
```

- `Product`: `id`, `title`, `description`, `price` (formatted), `raw_price_micros`,
  `currency_code`, `kind` (`Consumable`, `NonConsumable`, `Subscription`),
  `subscription_period`.
- `PurchaseUpdate`: `product_id`, `status` (`Pending`, `Purchased`, `Restored`,
  `Error(PurchaseError)`, `Canceled`), `purchase_id`, `transaction_date`,
  `server_verification_data`, `local_verification_data`, `signature`, `kind`,
  `pending_complete`, `quantity`.
- `PurchaseError`: `Unsupported`, `BillingUnavailable`, `ServiceUnavailable`,
  `ItemUnavailable`, `ItemAlreadyOwned`, `ItemNotOwned`, `DeveloperError`,
  `FeatureNotSupported`, `Store { code, message }`.

Behind the `purchases` feature of `frus` (and `frus-shell`), off by default. Guide:
`docs/guide/purchases.md`, compiled as doctests.

## Design

### The controller (`frus-shell/src/purchases/mod.rs`)

`Purchases` lives on the interface's thread: an `Rc` of the store behind a small trait
(`is_available`, `query`, `buy`, `restore`, `complete`), the listeners, and the ids the
application sells as consumables. A store answers from threads of its own, so each call returns
a one-shot reply (a slot and a waker, answered from any thread; a store that drops it without
answering answers `ServiceUnavailable`), and purchase updates go through a sink whose waker is a
task of the interface's thread (`spawn_local`, milestone 714), as video's reports do
(milestone 797). No update, no frame.

- Updates that arrive before any listener (a purchase left uncompleted last time is reported
  again at start-up) are kept and handed to the first listener at the next frame, never from
  inside `add_listener`: the caller may be holding the state its listener changes.
- `complete` does nothing for an update that does not need it (`pending_complete` false: a
  pending purchase, one acknowledged before); otherwise it consumes a consumable and
  acknowledges anything else.
- Google Play does not tell a consumable from a non-consumable product. `set_consumables`
  names them; products, updates and `complete` use it.
- `buy` answers once the sheet is open (or could not be); the purchase's outcome is an update.
  A cancellation or a failure of the sheet names no product on Google Play's side: the Java half
  remembers the product whose sheet it opened.

### Android: Google Play Billing Library, put into the package by `frus build`

The Billing Library is Java, published as an AAR on Google's Maven repository. Three ways to
reach Google Play were weighed:

| | The Billing Library | The billing service's AIDL |
| --- | --- | --- |
| Accepted by Google Play | yes (version 7 or later required since 2025) | no: new apps and updates must use the library |
| Purchase flow | the library's `ProxyBillingActivity` | our own activity for the `PendingIntent` |
| Packaging | its classes in the package, an activity in the manifest | an interface file |

The AIDL route would be smaller but cannot be published, so it was not taken. The library
needs its classes in the package's own `classes.dex` (the system instantiates
`ProxyBillingActivity`), so it cannot ride in an in-memory dex as frus's other Java does. Its
declared dependencies are heavy (androidx.activity, androidx.core, play-services-base,
-basement, -location, -tasks, and three datatransport libraries), but reading its bytecode
shows what the purchase flow calls:

- `androidx.core`: `ContextCompat`, `BundleCompat`, `Consumer`;
- `androidx.activity`: only `ProxyBillingActivityV2`, the activity of the alternative billing
  programs, which frus does not declare;
- datatransport: only the library's telemetry logger, whose construction is wrapped in a
  `catch (Throwable)` that turns logging off ("Skipping logging since initialization
  failed");
- the play-services classes it uses are its own shaded copies (`gms.internal.play_billing`);
- no `R` class: its three resources are optional.

So the package needs the library's `classes.jar` and `androidx.core`'s. `frus build` / `frus run`
(`frus-cli/src/android_billing.rs`):

1. **Detect.** frus-shell's Android store logs a marker, `frus-play-billing-library=8.3.0;`,
   which therefore lives in the library of any application with the feature. After cargo-apk,
   `finish_package` looks for it in the packaged libraries. An application without the feature
   has no marker and gets nothing; no cargo metadata or manifest setting is involved, and the
   version comes from the code that will call the library.
2. **Fetch, once.** `billing-8.3.0.aar` and `core-1.13.1.aar` from
   `dl.google.com/dl/android/maven2` into `~/.frus/android/play-billing-8.3.0/`.
3. **Shrink.** R8 from the SDK's build tools (`build-tools/*/lib/d8.jar`), on the JDK frus
   installs when there is none, keeping the library's public API (the bridge calls it by name),
   `ProxyBillingActivity`, and the library's own consumer rules (`proguard.txt`): 332 KB of dex
   rather than 1.38 MB unshrunk. D8 merges frus's activity (milestone 801) into the same
   `classes.dex`, cached beside it.
4. **Declare.** The manifest gains the `com.android.vending.BILLING` permission, the
   `<queries>` for the store's billing service, the
   `com.google.android.play.billingclient.version` meta-data and `ProxyBillingActivity`,
   as the library's own manifest declares them.

The split packages and the App Bundle are made from the `classes.dex` left beside the package,
so they carry the library too.

### Android: the bridge (`FrusBilling.java`, `purchases/android.rs`)

`dev.frus.billing.FrusBilling`, compiled against the library's API
(`scripts/build-billing-dex.sh`), is an 18 KB dex embedded with the feature and loaded through
an in-memory class loader whose parent is the package's: it finds the library there. It holds
one `BillingClient` (pending purchases enabled for one-time products, automatic service
reconnection), runs every call on the main thread, connects on the first call (calls made
while it connects wait, and fail with the connection's answer if it fails), and answers each
call once through a native method with a request number, the response code, the message and
the records as text (fields and records separated by U+001F and U+001E). Request 0 carries
`onPurchasesUpdated`. Rust parses the records in `purchases/play.rs`, which has no platform
code, so the host's tests cover it.

- `query_products` asks for one-time products and subscriptions in turn (the library takes
  one type per query) and keeps the details: `buy` needs them. The title is the product's
  name (the library's title carries the application's name in brackets). A subscription's
  price is its first offer's recurring phase, with its period; `buy` uses that offer.
- `restore` is `queryPurchasesAsync` for both types, reported as `Restored` (or `Pending`).
- `complete` is `consumeAsync` for a consumable, `acknowledgePurchase` otherwise.
- Response codes become `PurchaseError`s (`BILLING_UNAVAILABLE`, `SERVICE_UNAVAILABLE`,
  `NETWORK_ERROR`, `ITEM_ALREADY_OWNED`...).

A package made without frus's step (plain cargo-apk, the hot-reload engine's Gradle project)
has no library. The bridge checks for `BillingClient` before loading itself, and the store
answers `Unsupported` with that reason in the log instead of failing on the first call.

### Everywhere else

iOS, macOS, Windows, Linux and the web answer `PurchaseError::Unsupported` (with the reason:
StoreKit not supported yet, no store on the web...), and `is_available` answers `false`.
StoreKit and `Windows.Services.Store` are the paths (see Not done).

### The test store (`purchases/fake.rs`)

`FakeStore` sells the products it is given, in memory, on every platform: the next purchase
can be paid, wait for its payment (`clear_pending` / `fail_pending` end it), be canceled or
fail with any error; a product bought and not consumed is owned (`ItemAlreadyOwned` on a second
purchase, as on Google Play), `own` marks one as bought before, `set_available(false)` fails
every call, and `calls()` lists what it was asked. Its updates take the same path as a real
store's: they reach the listeners at the next frame.

### The demo

`frus-demo` has a shop screen (`/shop`, "In-app purchases" in the menu): coins, an upgrade and a
subscription. The test store sells them, with a choice of how the next purchase ends and a
button that pays the pending ones; delivered purchases are completed and counted. The other
tab asks the platform's store for the same ids.

### Cost

Without the feature: nothing. With it: the 18 KB bridge dex and the controller in the library;
nothing runs until the application asks for `Purchases::instance()`. On Android the package
gains about 330 KB of dex.

## How it was tested

- **Host.** `cargo test -p frus-shell --features purchases --lib purchases::` (the controller,
  the test store, the parsing of Google Play's records) and the guide's doctests
  (`cargo test -p frus --features purchases --doc`, 81 passed). frus-cli: 224 tests, among them
  the Billing step's (the marker names the version, the manifest gains the library's
  declarations once, a library without the marker gets nothing). Clippy on the host, and
  `cargo check` for Android, iOS and wasm32 with the feature: no new warnings.
- **The package.** `frus build apk --profile` in `frus-demo` found the marker, fetched Billing
  Library 8.3.0 once, and wrote a 25.3 MB package whose `classes.dex` (333 KB) holds the
  library's `com/android/billingclient` classes and frus's activity. Its manifest declares the
  `BILLING` permission, the two `<queries>`, the version meta-data and `ProxyBillingActivity`,
  each once.
- **frus-hello does not grow.** It does not enable the feature (`cargo tree -p frus-hello -e
  features` names no `purchases`); everything the feature adds is behind
  `cfg(feature = "purchases")`, the feature pulls in no dependency, and `frus build` adds the
  library only to a package whose library carries the marker.
- **The phone** (Huawei STK-L21, Android 10, Play Store 54.1.11, no Play Console application
  for `com.frus.demo`), the profile package opened on `frusdemo://shop`:
  - Google Play: the first call loaded the bridge
    (`in-app purchases: Google Play Billing (frus-play-billing-library=8.3.0;)`) and the client
    connected. The library's telemetry turned itself off as expected
    (`BillingLogger: Skipping logging since initialization failed.`). `is_available()` answered
    `true`, and `query_products(["coins_100", "pro", "plus_monthly"])` answered in 1.5 s:
    `Ok(ProductsResponse { products: [], not_found: ["coins_100", "pro", "plus_monthly"] })`,
    since the package is not on the Play Console. No panic, no crash.
  - The test store listed its three products with prices. A coins purchase set to "Pending"
    reported `coins_100: Pending`; "Pay the pending purchases" then reported `Purchased`, the
    screen completed it (`completed`), and the balance went from 0 to 100 coins.
  - Leaving: the page's listener goes with its state (`PurchaseSubscription` drops it); the
    client is the thread's one `Purchases::instance()`, created once. Six visits to the page,
    each asking Google Play again, logged the bridge's loading once, in one process, and
    `dumpsys meminfo` showed no growth: native heap 65.3 MB after the first visit and 60.8 MB
    after five more, Java heap 3.9 then 3.7 MB.

## Not done

- **StoreKit (iOS, macOS).** Every call answers `Unsupported`. StoreKit 2 is Swift-only;
  StoreKit 1 (`SKPaymentQueue`, deprecated but working) through objc2 is the likely path.
- **Windows.Services.Store** (Microsoft Store): `Unsupported`.
- **Hot reload.** `frus run --hot` builds through the engine's Gradle project, which does not
  get the library: purchases answer `Unsupported` there; `frus run --no-hot` has them.
- **A real purchase.** No Play Console application was available: Google Play was asked for
  products and answered that it sells none of them, but no product was found, bought, consumed
  or acknowledged there, and `restore` was not checked against it. The purchase sheet
  (`ProxyBillingActivity`) was never opened.
- **Purchases made while the application was away** (a pending payment that cleared, a
  purchase on another device) are found by `restore`; they are not queried by themselves on
  return to the foreground, as Google recommends. An application calls `restore` when it
  starts.
- The demo's "Pending" segment is cut to "Pen..." once selected on a phone-wide screen.
- Subscription upgrades and downgrades (replacement modes), several offers per subscription,
  one-time product offers, the alternative billing programs, in-app messages.
- Server-side verification is the application's: `server_verification_data` is the purchase
  token to check with Google's Play Developer API.
