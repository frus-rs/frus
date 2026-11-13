// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! What Google Play Billing's answers mean: its response codes, and the records the Java half
//! (`FrusBilling`) sends as text. Platform-free, so the host's tests read them.

use super::{
    Product, ProductKind, ProductsResponse, PurchaseError, PurchaseStatus, PurchaseUpdate,
};

const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

/// `BillingResponseCode.OK`.
pub(crate) const OK: i32 = 0;
const USER_CANCELED: i32 = 1;

/// What a response code other than `OK` means.
pub(crate) fn error(code: i32, message: &str) -> PurchaseError {
    let message = if message.is_empty() {
        format!("Google Play answered {code}")
    } else {
        message.to_string()
    };
    match code {
        // SERVICE_UNAVAILABLE, NETWORK_ERROR, SERVICE_DISCONNECTED, SERVICE_TIMEOUT
        2 | 12 | -1 | -3 => PurchaseError::ServiceUnavailable(message),
        3 => PurchaseError::BillingUnavailable(message),
        4 => PurchaseError::ItemUnavailable(message),
        5 => PurchaseError::DeveloperError(message),
        7 => PurchaseError::ItemAlreadyOwned,
        8 => PurchaseError::ItemNotOwned,
        -2 => PurchaseError::FeatureNotSupported(message),
        _ => PurchaseError::Store { code, message },
    }
}

fn records(payload: &str) -> impl Iterator<Item = Vec<&str>> {
    payload
        .split(RECORD)
        .filter(|r| !r.is_empty())
        .map(|r| r.split(FIELD).collect())
}

fn kind(product_type: &str) -> ProductKind {
    if product_type == "subs" {
        ProductKind::Subscription
    } else {
        ProductKind::NonConsumable
    }
}

/// The products of a query; the ids asked for and not in it are the ones not found.
pub(crate) fn products(payload: &str, asked: &[String]) -> ProductsResponse {
    let mut response = ProductsResponse::default();
    for f in records(payload) {
        if f.len() < 10 {
            continue;
        }
        let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
        response.products.push(Product {
            id: f[0].to_string(),
            kind: kind(f[1]),
            // The title carries the application's name in brackets; the name does not.
            title: if f[3].is_empty() { f[2] } else { f[3] }.to_string(),
            description: f[4].to_string(),
            price: f[5].to_string(),
            raw_price_micros: f[6].parse().unwrap_or(0),
            currency_code: f[7].to_string(),
            offer: some(f[8]),
            subscription_period: some(f[9]),
        });
    }
    response.not_found = asked
        .iter()
        .filter(|id| !response.products.iter().any(|p| &p.id == *id))
        .cloned()
        .collect();
    response
}

/// The purchases of an update (`restored` for the ones [`Purchases::restore`] found), one
/// update per product.
///
/// [`Purchases::restore`]: super::Purchases::restore
pub(crate) fn purchases(payload: &str, restored: bool) -> Vec<PurchaseUpdate> {
    let mut updates = Vec::new();
    for f in records(payload) {
        if f.len() < 10 {
            continue;
        }
        let acknowledged = f[6] == "1";
        let status = match f[2] {
            // Purchase.PurchaseState.PURCHASED
            "1" if restored => PurchaseStatus::Restored,
            "1" => PurchaseStatus::Purchased,
            // PENDING
            "2" => PurchaseStatus::Pending,
            _ => continue,
        };
        for product in f[0].split(',').filter(|p| !p.is_empty()) {
            updates.push(PurchaseUpdate {
                product_id: product.to_string(),
                pending_complete: status != PurchaseStatus::Pending && !acknowledged,
                status: status.clone(),
                purchase_id: (!f[4].is_empty()).then(|| f[4].to_string()),
                transaction_date: f[5].parse().ok(),
                server_verification_data: f[3].to_string(),
                local_verification_data: f[8].to_string(),
                signature: f[9].to_string(),
                kind: kind(f[1]),
                quantity: f[7].parse().unwrap_or(1),
            });
        }
    }
    updates
}

/// What `onPurchasesUpdated` reports: the purchases, or the product of the sheet that was
/// canceled or failed. `message` is the library's message and that product, separated.
pub(crate) fn updated(code: i32, message: &str, payload: &str) -> Vec<PurchaseUpdate> {
    let (message, buying) = message.split_once(FIELD).unwrap_or((message, ""));
    match code {
        OK => purchases(payload, false),
        USER_CANCELED => vec![PurchaseUpdate::bare(buying, PurchaseStatus::Canceled)],
        _ => vec![PurchaseUpdate::bare(
            buying,
            PurchaseStatus::Error(error(code, message)),
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[&str]) -> String {
        let mut out = String::new();
        for f in fields {
            out.push_str(f);
            out.push(FIELD);
        }
        out.push(RECORD);
        out
    }

    #[test]
    fn products_are_read_and_the_missing_ids_named() {
        let payload = record(&[
            "coins",
            "inapp",
            "Coins (Game)",
            "Coins",
            "A bag",
            "€0.99",
            "990000",
            "EUR",
            "",
            "",
        ]) + &record(&[
            "pro",
            "subs",
            "Pro (Game)",
            "Pro",
            "Monthly",
            "€2.99",
            "2990000",
            "EUR",
            "tok",
            "P1M",
        ]);
        let asked = ["coins", "pro", "gems"].map(String::from);
        let response = products(&payload, &asked);
        assert_eq!(response.not_found, ["gems"]);
        let coins = &response.products[0];
        assert_eq!(
            (coins.title.as_str(), coins.kind, coins.raw_price_micros),
            ("Coins", ProductKind::NonConsumable, 990_000)
        );
        assert_eq!(coins.offer, None);
        let pro = &response.products[1];
        assert_eq!(pro.kind, ProductKind::Subscription);
        assert_eq!(pro.offer.as_deref(), Some("tok"));
        assert_eq!(pro.subscription_period.as_deref(), Some("P1M"));
    }

    #[test]
    fn purchases_map_their_states() {
        let payload = record(&[
            "pro",
            "inapp",
            "1",
            "token1",
            "GPA.1",
            "1700000000000",
            "0",
            "1",
            "{}",
            "sig",
        ]) + &record(&["a,b", "", "2", "token2", "", "5", "0", "1", "{}", ""])
            + &record(&["x", "", "0", "token3", "", "5", "0", "1", "{}", ""]);
        let updates = purchases(&payload, false);
        assert_eq!(updates.len(), 3, "the unspecified state is skipped");
        assert_eq!(updates[0].status, PurchaseStatus::Purchased);
        assert!(updates[0].pending_complete);
        assert_eq!(updates[0].purchase_id.as_deref(), Some("GPA.1"));
        assert_eq!(updates[0].server_verification_data, "token1");
        assert_eq!(updates[1].product_id, "a");
        assert_eq!(updates[2].product_id, "b");
        assert_eq!(updates[2].status, PurchaseStatus::Pending);
        assert!(
            !updates[2].pending_complete,
            "a pending purchase is not completed"
        );

        let acknowledged = record(&["pro", "subs", "1", "t", "", "5", "1", "1", "{}", ""]);
        let restored = purchases(&acknowledged, true);
        assert_eq!(restored[0].status, PurchaseStatus::Restored);
        assert_eq!(restored[0].kind, ProductKind::Subscription);
        assert!(!restored[0].pending_complete);
    }

    #[test]
    fn a_cancellation_and_a_failure_name_the_product_on_sale() {
        let canceled = updated(1, "\u{1f}pro", "");
        assert_eq!(canceled[0].product_id, "pro");
        assert_eq!(canceled[0].status, PurchaseStatus::Canceled);
        let owned = updated(7, "Item is already owned.\u{1f}pro", "");
        assert_eq!(
            owned[0].status,
            PurchaseStatus::Error(PurchaseError::ItemAlreadyOwned)
        );
    }

    #[test]
    fn response_codes_become_errors() {
        assert_eq!(
            error(3, "x"),
            PurchaseError::BillingUnavailable("x".to_string())
        );
        assert!(matches!(error(-1, ""), PurchaseError::ServiceUnavailable(m) if m.contains("-1")));
        assert!(matches!(error(5, "d"), PurchaseError::DeveloperError(_)));
        assert!(matches!(
            error(6, "e"),
            PurchaseError::Store { code: 6, .. }
        ));
    }
}
