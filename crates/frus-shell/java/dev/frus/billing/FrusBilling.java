// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

package dev.frus.billing;

import android.app.Activity;
import android.os.Handler;
import android.os.Looper;

import com.android.billingclient.api.AcknowledgePurchaseParams;
import com.android.billingclient.api.BillingClient;
import com.android.billingclient.api.BillingClientStateListener;
import com.android.billingclient.api.BillingFlowParams;
import com.android.billingclient.api.BillingResult;
import com.android.billingclient.api.ConsumeParams;
import com.android.billingclient.api.PendingPurchasesParams;
import com.android.billingclient.api.ProductDetails;
import com.android.billingclient.api.Purchase;
import com.android.billingclient.api.PurchasesUpdatedListener;
import com.android.billingclient.api.QueryProductDetailsParams;
import com.android.billingclient.api.QueryProductDetailsResult;
import com.android.billingclient.api.QueryPurchasesParams;
import com.android.billingclient.api.UnfetchedProduct;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * frus's side of Google Play Billing (milestone 807): one {@link BillingClient} for the
 * application, driven from Rust.
 *
 * Every call runs on the main thread, where the library delivers its answers too. Each call
 * carries a request number and is answered once through {@link #nativeEvent}; purchase updates
 * (the library's {@link PurchasesUpdatedListener}) are reported with request 0. Records travel
 * as text: fields separated by U+001F, records by U+001E.
 *
 * The library itself is not in this dex: frus's command line puts it in the package's own
 * code when the application's library uses it, and this class finds it through its parent
 * class loader.
 */
public final class FrusBilling implements PurchasesUpdatedListener, BillingClientStateListener {
    private static final char FIELD = '\u001f';
    private static final char RECORD = '\u001e';

    /** Answers request {@code request}; request 0 carries purchase updates. */
    static native void nativeEvent(long request, int code, String message, String payload);

    private final Activity activity;
    private final Handler main = new Handler(Looper.getMainLooper());
    private BillingClient client;
    private boolean connecting;
    /** Calls waiting for the connection: run once it is made, failed with its answer if not. */
    private final List<Pending> waiting = new ArrayList<>();
    /** The products queried, by id: a purchase needs them, and so does knowing a type. */
    private final Map<String, ProductDetails> known = new HashMap<>();
    /** The product whose purchase sheet is open: a cancellation names no product. */
    private String buying = "";

    private interface Call {
        void run();
    }

    private static final class Pending {
        final long request;
        final Call call;

        Pending(long request, Call call) {
            this.request = request;
            this.call = call;
        }
    }

    private FrusBilling(Activity activity) {
        this.activity = activity;
    }

    public static FrusBilling create(Activity activity) {
        return new FrusBilling(activity);
    }

    // --- Connecting ---------------------------------------------------------------------

    /** Runs {@code call} on the main thread once the client is connected. */
    private void connected(final long request, final Call call) {
        main.post(new Runnable() {
            @Override
            public void run() {
                try {
                    if (client == null) {
                        client = BillingClient.newBuilder(activity.getApplicationContext())
                                .setListener(FrusBilling.this)
                                .enablePendingPurchases(PendingPurchasesParams.newBuilder()
                                        .enableOneTimeProducts()
                                        .build())
                                .enableAutoServiceReconnection()
                                .build();
                    }
                    if (client.isReady()) {
                        call.run();
                        return;
                    }
                    waiting.add(new Pending(request, call));
                    if (!connecting) {
                        connecting = true;
                        client.startConnection(FrusBilling.this);
                    }
                } catch (Throwable error) {
                    nativeEvent(request, 6, String.valueOf(error), null);
                }
            }
        });
    }

    @Override
    public void onBillingSetupFinished(BillingResult result) {
        connecting = false;
        List<Pending> calls = new ArrayList<>(waiting);
        waiting.clear();
        for (Pending pending : calls) {
            if (result.getResponseCode() == BillingClient.BillingResponseCode.OK) {
                try {
                    pending.call.run();
                } catch (Throwable error) {
                    nativeEvent(pending.request, 6, String.valueOf(error), null);
                }
            } else {
                nativeEvent(pending.request, result.getResponseCode(), result.getDebugMessage(), null);
            }
        }
    }

    @Override
    public void onBillingServiceDisconnected() {
        // The library reconnects by itself on the next call (enableAutoServiceReconnection).
        connecting = false;
    }

    // --- The calls ----------------------------------------------------------------------

    /** Whether the store answers: 0 when it does, else the library's response code. */
    public void available(final long request) {
        connected(request, new Call() {
            @Override
            public void run() {
                nativeEvent(request, 0, "", null);
            }
        });
    }

    /**
     * The products named, one-time products and subscriptions alike: the library asks for
     * one type at a time, so both are asked for, and an id found in neither is reported as
     * unknown.
     */
    public void query(final long request, final String[] ids) {
        connected(request, new Call() {
            @Override
            public void run() {
                queryType(request, ids, BillingClient.ProductType.INAPP, new ArrayList<ProductDetails>());
            }
        });
    }

    private void queryType(final long request, final String[] ids, final String type,
                           final List<ProductDetails> found) {
        List<QueryProductDetailsParams.Product> products = new ArrayList<>();
        for (String id : ids) {
            products.add(QueryProductDetailsParams.Product.newBuilder()
                    .setProductId(id)
                    .setProductType(type)
                    .build());
        }
        QueryProductDetailsParams params =
                QueryProductDetailsParams.newBuilder().setProductList(products).build();
        client.queryProductDetailsAsync(params, (result, details) -> main.post(() -> {
            if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                nativeEvent(request, result.getResponseCode(), result.getDebugMessage(), null);
                return;
            }
            found.addAll(productsOf(details));
            if (type.equals(BillingClient.ProductType.INAPP)) {
                queryType(request, ids, BillingClient.ProductType.SUBS, found);
                return;
            }
            StringBuilder out = new StringBuilder();
            for (ProductDetails product : found) {
                known.put(product.getProductId(), product);
                appendProduct(out, product);
            }
            nativeEvent(request, 0, "", out.toString());
        }));
    }

    private static List<ProductDetails> productsOf(QueryProductDetailsResult details) {
        if (details == null || details.getProductDetailsList() == null) {
            return Collections.emptyList();
        }
        return details.getProductDetailsList();
    }

    /** Opens the store's purchase sheet for a product queried before. */
    public void buy(final long request, final String id, final String offerToken,
                    final String accountId) {
        connected(request, new Call() {
            @Override
            public void run() {
                ProductDetails product = known.get(id);
                if (product == null) {
                    nativeEvent(request, 4, id + " was not queried", null);
                    return;
                }
                BillingFlowParams.ProductDetailsParams.Builder item =
                        BillingFlowParams.ProductDetailsParams.newBuilder().setProductDetails(product);
                if (offerToken != null && !offerToken.isEmpty()) {
                    item.setOfferToken(offerToken);
                }
                List<BillingFlowParams.ProductDetailsParams> items = new ArrayList<>();
                items.add(item.build());
                BillingFlowParams.Builder flow =
                        BillingFlowParams.newBuilder().setProductDetailsParamsList(items);
                if (accountId != null && !accountId.isEmpty()) {
                    flow.setObfuscatedAccountId(accountId);
                }
                buying = id;
                BillingResult result = client.launchBillingFlow(activity, flow.build());
                nativeEvent(request, result.getResponseCode(), result.getDebugMessage(), null);
            }
        });
    }

    /** The purchases the account owns, one-time products then subscriptions. */
    public void owned(final long request) {
        connected(request, new Call() {
            @Override
            public void run() {
                ownedType(request, BillingClient.ProductType.INAPP, new StringBuilder());
            }
        });
    }

    private void ownedType(final long request, final String type, final StringBuilder out) {
        QueryPurchasesParams params = QueryPurchasesParams.newBuilder().setProductType(type).build();
        client.queryPurchasesAsync(params, (result, purchases) -> main.post(() -> {
            if (result.getResponseCode() != BillingClient.BillingResponseCode.OK) {
                nativeEvent(request, result.getResponseCode(), result.getDebugMessage(), null);
                return;
            }
            for (Purchase purchase : purchases) {
                appendPurchase(out, purchase, type);
            }
            if (type.equals(BillingClient.ProductType.INAPP)) {
                ownedType(request, BillingClient.ProductType.SUBS, out);
            } else {
                nativeEvent(request, 0, "", out.toString());
            }
        }));
    }

    /** Acknowledges a purchase. */
    public void acknowledge(final long request, final String token) {
        connected(request, new Call() {
            @Override
            public void run() {
                client.acknowledgePurchase(
                        AcknowledgePurchaseParams.newBuilder().setPurchaseToken(token).build(),
                        result -> main.post(() -> nativeEvent(request, result.getResponseCode(),
                                result.getDebugMessage(), null)));
            }
        });
    }

    /** Consumes a purchase: the product can be bought again. */
    public void consume(final long request, final String token) {
        connected(request, new Call() {
            @Override
            public void run() {
                client.consumeAsync(ConsumeParams.newBuilder().setPurchaseToken(token).build(),
                        (result, consumed) -> main.post(() -> nativeEvent(request,
                                result.getResponseCode(), result.getDebugMessage(), null)));
            }
        });
    }

    // --- Updates ------------------------------------------------------------------------

    @Override
    public void onPurchasesUpdated(BillingResult result, List<Purchase> purchases) {
        StringBuilder out = new StringBuilder();
        if (purchases != null) {
            for (Purchase purchase : purchases) {
                appendPurchase(out, purchase, null);
            }
        }
        // The product of a cancellation or a failure: the sheet's, which the library does not
        // name.
        nativeEvent(0, result.getResponseCode(), result.getDebugMessage() + FIELD + buying,
                out.toString());
    }

    // --- Records ------------------------------------------------------------------------

    private static String clean(String text) {
        if (text == null) {
            return "";
        }
        return text.replace(FIELD, ' ').replace(RECORD, ' ');
    }

    private static void field(StringBuilder out, Object value) {
        out.append(clean(value == null ? "" : String.valueOf(value))).append(FIELD);
    }

    /**
     * id, type, title, name, description, formatted price, micros, currency, offer token,
     * billing period.
     */
    private static void appendProduct(StringBuilder out, ProductDetails product) {
        String price = "";
        long micros = 0;
        String currency = "";
        String offer = "";
        String period = "";
        ProductDetails.OneTimePurchaseOfferDetails once = product.getOneTimePurchaseOfferDetails();
        List<ProductDetails.SubscriptionOfferDetails> offers = product.getSubscriptionOfferDetails();
        if (once != null) {
            price = once.getFormattedPrice();
            micros = once.getPriceAmountMicros();
            currency = once.getPriceCurrencyCode();
        } else if (offers != null && !offers.isEmpty()) {
            // The first offer, and its last phase: the price that recurs.
            ProductDetails.SubscriptionOfferDetails first = offers.get(0);
            offer = first.getOfferToken();
            List<ProductDetails.PricingPhase> phases = first.getPricingPhases().getPricingPhaseList();
            if (!phases.isEmpty()) {
                ProductDetails.PricingPhase phase = phases.get(phases.size() - 1);
                price = phase.getFormattedPrice();
                micros = phase.getPriceAmountMicros();
                currency = phase.getPriceCurrencyCode();
                period = phase.getBillingPeriod();
            }
        }
        field(out, product.getProductId());
        field(out, product.getProductType());
        field(out, product.getTitle());
        field(out, product.getName());
        field(out, product.getDescription());
        field(out, price);
        field(out, micros);
        field(out, currency);
        field(out, offer);
        field(out, period);
        out.append(RECORD);
    }

    /**
     * products (comma-separated), type (or empty), state, token, order id, time, acknowledged,
     * quantity, original JSON, signature.
     */
    private void appendPurchase(StringBuilder out, Purchase purchase, String type) {
        List<String> products = purchase.getProducts();
        if (type == null) {
            type = "";
            for (String id : products) {
                ProductDetails product = known.get(id);
                if (product != null) {
                    type = product.getProductType();
                }
            }
        }
        field(out, String.join(",", products));
        field(out, type);
        field(out, purchase.getPurchaseState());
        field(out, purchase.getPurchaseToken());
        field(out, purchase.getOrderId());
        field(out, purchase.getPurchaseTime());
        field(out, purchase.isAcknowledged() ? 1 : 0);
        field(out, purchase.getQuantity());
        field(out, purchase.getOriginalJson());
        field(out, purchase.getSignature());
        out.append(RECORD);
    }
}
