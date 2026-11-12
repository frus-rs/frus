package dev.frus.input;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Color;
import android.util.Base64;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.webkit.CookieManager;
import android.webkit.JavascriptInterface;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebStorage;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import dev.frus.platform.PlatformView;
import dev.frus.platform.PlatformViewChannel;
import dev.frus.platform.PlatformViewFactory;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import org.json.JSONArray;
import org.json.JSONObject;
import org.json.JSONTokener;

/**
 * {@code frus/webview}: Android's WebView, driven by a {@code WebViewController} (milestone
 * 799), or, given an address or HTML as its parameters, the plain view of milestone 794.
 *
 * <p>A controller's page outlives the widget that shows it: the pages are kept by controller,
 * and a widget made again for the same controller -- a screen pushed over it and popped -- is
 * given the same WebView, with its history and its scroll. The page goes when the controller
 * is dropped ({@code web.release}).
 *
 * <p>Methods and events are named in {@code frus_widgets::web_view_wire}; their arguments and
 * data are JSON. Every method runs on the UI thread.
 */
final class FrusWebViews implements PlatformViewFactory {
    /** Every controller's page, by controller. Touched on the UI thread only. */
    private static final HashMap<Long, Page> pages = new HashMap<Long, Page>();

    @Override
    public PlatformView create(Context context, int id, String params, PlatformViewChannel channel) {
        String trimmed = params.trim();
        if (!trimmed.startsWith("{")) {
            return Legacy.create(context, params, channel);
        }
        JSONObject p;
        try {
            p = new JSONObject(trimmed);
        } catch (Throwable error) {
            throw new IllegalArgumentException("frus/webview parameters: " + error);
        }
        long key = p.optLong("controller", -1);
        Page page = pages.get(key);
        if (page == null) {
            page = new Page(context, p);
            if (key >= 0) {
                pages.put(key, page);
            }
        }
        // The page leaves the widget that showed it before, if any: it is shown here now.
        ViewParent parent = page.web.getParent();
        if (parent instanceof ViewGroup) {
            ((ViewGroup) parent).removeView(page.web);
        }
        page.channel = channel;
        page.state();
        return page.shownBy(channel, key < 0);
    }

    /** A call for every web view: the cookies, a page let go. Returns nothing. */
    static void global(String method, String arguments) {
        try {
            if ("web.release".equals(method)) {
                long key = new JSONObject(arguments).optLong("controller", -1);
                Page page = pages.remove(key);
                if (page != null) {
                    page.destroy();
                }
            } else if ("web.setCookie".equals(method)) {
                JSONObject c = new JSONObject(arguments);
                String cookie = c.optString("name") + "=" + c.optString("value")
                        + "; Domain=" + c.optString("domain") + "; Path=" + c.optString("path", "/");
                CookieManager.getInstance().setCookie(c.optString("url"), cookie);
            } else if ("web.clearCookies".equals(method)) {
                final long id = new JSONObject(arguments).optLong("id");
                final CookieManager cookies = CookieManager.getInstance();
                final boolean had = cookies.hasCookies();
                cookies.removeAllCookies(new ValueCallback<Boolean>() {
                    @Override
                    public void onReceiveValue(Boolean value) {
                        FrusPlatformViews.sendGlobal("web.cookiesCleared",
                                "{\"id\":" + id + ",\"had\":" + had + "}");
                    }
                });
            }
        } catch (Throwable error) {
            // A malformed call changes nothing.
        }
    }

    private static String json(Object... pairs) {
        JSONObject out = new JSONObject();
        try {
            for (int i = 0; i + 1 < pairs.length; i += 2) {
                out.put((String) pairs[i], pairs[i + 1] == null ? JSONObject.NULL : pairs[i + 1]);
            }
        } catch (Throwable ignored) {
            // Only plain keys and values are put.
        }
        return out.toString();
    }

    /** A JSON string argument, or the text itself when it is not JSON. */
    private static String text(String arguments) {
        try {
            Object value = new JSONTokener(arguments).nextValue();
            return value instanceof String ? (String) value : arguments;
        } catch (Throwable error) {
            return arguments;
        }
    }

    private static boolean flag(String arguments) {
        return "true".equals(arguments.trim());
    }

    /** {@code #rrggbbaa}, as an Android colour. */
    private static int color(String hex) {
        String h = hex.startsWith("#") ? hex.substring(1) : hex;
        long v = Long.parseLong(h, 16);
        if (h.length() == 6) {
            return (int) (0xff000000L | v);
        }
        int r = (int) ((v >> 24) & 0xff);
        int g = (int) ((v >> 16) & 0xff);
        int b = (int) ((v >> 8) & 0xff);
        int a = (int) (v & 0xff);
        return Color.argb(a, r, g, b);
    }

    /** One controller's page. */
    private static final class Page {
        final WebView web;
        /** Where its events go: the widget showing it now. */
        volatile PlatformViewChannel channel;
        volatile boolean decidesNavigation;
        final HashSet<String> channels = new HashSet<String>();

        Page(Context context, JSONObject p) {
            web = new WebView(context);
            WebSettings settings = web.getSettings();
            settings.setDomStorageEnabled(true);
            settings.setJavaScriptEnabled(p.optBoolean("javascript", true));
            boolean zoom = p.optBoolean("zoom", true);
            settings.setSupportZoom(zoom);
            settings.setBuiltInZoomControls(zoom);
            settings.setDisplayZoomControls(false);
            if (!p.isNull("userAgent")) {
                settings.setUserAgentString(p.optString("userAgent"));
            }
            if (!p.isNull("background")) {
                try {
                    web.setBackgroundColor(color(p.optString("background")));
                } catch (Throwable ignored) {
                    // A colour it cannot read leaves the default.
                }
            }
            decidesNavigation = p.optBoolean("decidesNavigation", false);
            JSONArray names = p.optJSONArray("channels");
            if (names != null) {
                for (int i = 0; i < names.length(); i++) {
                    addChannel(names.optString(i));
                }
            }
            web.setWebViewClient(new Client());
            web.setWebChromeClient(new Chrome());
        }

        void send(String name, String data) {
            PlatformViewChannel to = channel;
            if (to != null) {
                to.send(name, data);
            }
        }

        /** The history and the page's names, for the controller. */
        void state() {
            send("web.state", json("url", web.getUrl(), "title", web.getTitle(),
                    "canGoBack", web.canGoBack(), "canGoForward", web.canGoForward()));
        }

        void addChannel(final String name) {
            if (name.length() == 0 || !channels.add(name)) {
                return;
            }
            web.addJavascriptInterface(new Object() {
                @JavascriptInterface
                public void postMessage(String message) {
                    send("web.channelMessage", json("channel", name, "message",
                            message == null ? "" : message));
                }
            }, name);
        }

        void destroy() {
            ViewParent parent = web.getParent();
            if (parent instanceof ViewGroup) {
                ((ViewGroup) parent).removeView(web);
            }
            web.destroy();
        }

        /** The view as one widget shows it. {@code own}: no controller keeps it. */
        PlatformView shownBy(final PlatformViewChannel host, final boolean own) {
            return new PlatformView() {
                @Override
                public View getView() {
                    return web;
                }

                @Override
                public void dispose() {
                    // The widget went. The page stays its controller's, unless another widget
                    // has it already.
                    if (channel == host) {
                        channel = null;
                    }
                    if (own) {
                        destroy();
                    }
                }

                @Override
                public void onMethodCall(String method, String arguments) {
                    call(method, arguments);
                }

                @Override
                public void onPause() {
                    web.onPause();
                }

                @Override
                public void onResume() {
                    web.onResume();
                }
            };
        }

        void call(String method, String arguments) {
            try {
                callOrThrow(method, arguments);
            } catch (RuntimeException error) {
                throw error;
            } catch (Throwable error) {
                throw new IllegalArgumentException(error.toString());
            }
        }

        private void callOrThrow(String method, String arguments) throws Exception {
            WebSettings settings = web.getSettings();
            switch (method) {
                case "web.loadRequest": {
                    JSONObject r = new JSONObject(arguments);
                    String url = r.getString("url");
                    if ("POST".equals(r.optString("method"))) {
                        web.postUrl(url, Base64.decode(r.optString("body"), Base64.DEFAULT));
                    } else {
                        HashMap<String, String> headers = new HashMap<String, String>();
                        JSONObject h = r.optJSONObject("headers");
                        if (h != null) {
                            Iterator<String> keys = h.keys();
                            while (keys.hasNext()) {
                                String k = keys.next();
                                headers.put(k, h.optString(k));
                            }
                        }
                        web.loadUrl(url, headers);
                    }
                    break;
                }
                case "web.loadHtml": {
                    JSONObject r = new JSONObject(arguments);
                    String base = r.isNull("baseUrl") ? null : r.optString("baseUrl");
                    web.loadDataWithBaseURL(base, r.optString("html"), "text/html", "utf-8", null);
                    break;
                }
                case "web.loadFile":
                    settings.setAllowFileAccess(true);
                    web.loadUrl("file://" + text(arguments));
                    break;
                case "web.loadAsset":
                    web.loadUrl("file:///android_asset/" + text(arguments));
                    break;
                case "web.reload":
                    web.reload();
                    break;
                case "web.goBack":
                    if (web.canGoBack()) {
                        web.goBack();
                    }
                    break;
                case "web.goForward":
                    if (web.canGoForward()) {
                        web.goForward();
                    }
                    break;
                case "web.runJavascript":
                    web.evaluateJavascript(text(arguments), null);
                    break;
                case "web.runJavascriptForResult": {
                    JSONObject r = new JSONObject(arguments);
                    final long id = r.getLong("id");
                    web.evaluateJavascript(r.optString("script"), new ValueCallback<String>() {
                        @Override
                        public void onReceiveValue(String value) {
                            send("web.javascriptResult", json("id", id, "ok", true,
                                    "value", value == null ? "null" : value));
                        }
                    });
                    break;
                }
                case "web.addChannel":
                    addChannel(text(arguments));
                    break;
                case "web.removeChannel": {
                    String name = text(arguments);
                    if (channels.remove(name)) {
                        web.removeJavascriptInterface(name);
                    }
                    break;
                }
                case "web.setJavascript":
                    settings.setJavaScriptEnabled(flag(arguments));
                    break;
                case "web.setUserAgent": {
                    String agent = arguments.trim();
                    settings.setUserAgentString("null".equals(agent) ? null : text(agent));
                    break;
                }
                case "web.setBackground":
                    web.setBackgroundColor(color(text(arguments)));
                    break;
                case "web.setZoom": {
                    boolean zoom = flag(arguments);
                    settings.setSupportZoom(zoom);
                    settings.setBuiltInZoomControls(zoom);
                    settings.setDisplayZoomControls(false);
                    break;
                }
                case "web.setDecidesNavigation":
                    decidesNavigation = flag(arguments);
                    break;
                case "web.clearCache":
                    web.clearCache(true);
                    break;
                case "web.clearLocalStorage":
                    WebStorage.getInstance().deleteAllData();
                    break;
                default:
                    Legacy.call(web, method, arguments, channel);
            }
        }

        private final class Client extends WebViewClient {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                if (!decidesNavigation || !request.isForMainFrame()) {
                    return false;
                }
                // Held back: the controller decides between frames, and loads it if it goes.
                send("web.navigationRequest", json("url", request.getUrl().toString(),
                        "mainFrame", true));
                return true;
            }

            @Override
            public void onPageStarted(WebView view, String url, Bitmap favicon) {
                send("web.pageStarted", json("url", url));
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                send("web.pageFinished", json("url", url));
                state();
            }

            @Override
            public void doUpdateVisitedHistory(WebView view, String url, boolean reload) {
                send("web.urlChange", json("url", url));
                state();
            }

            @Override
            public void onReceivedError(WebView view, WebResourceRequest request,
                    WebResourceError error) {
                send("web.resourceError", json("code", error.getErrorCode(),
                        "description", String.valueOf(error.getDescription()),
                        "mainFrame", request.isForMainFrame(),
                        "url", request.getUrl().toString()));
            }

            @Override
            public void onReceivedHttpError(WebView view, WebResourceRequest request,
                    WebResourceResponse response) {
                if (request.isForMainFrame()) {
                    send("web.httpError", json("status", response.getStatusCode(),
                            "url", request.getUrl().toString()));
                }
            }
        }

        private final class Chrome extends WebChromeClient {
            @Override
            public void onProgressChanged(WebView view, int progress) {
                send("web.progress", json("progress", progress));
            }

            @Override
            public void onReceivedTitle(WebView view, String title) {
                state();
            }
        }
    }

    /**
     * The plain view of milestone 794: its parameters are an address, or HTML when they start
     * with {@code <}. Calls: {@code loadUrl}, {@code loadHtml}, {@code evaluateJavascript}
     * (answered by a {@code javascriptResult} event). Events: {@code pageFinished} (the
     * address).
     */
    private static final class Legacy {
        static PlatformView create(Context context, String params, final PlatformViewChannel channel) {
            final WebView web = new WebView(context);
            web.getSettings().setJavaScriptEnabled(true);
            web.setWebViewClient(new WebViewClient() {
                @Override
                public void onPageFinished(WebView view, String url) {
                    channel.send("pageFinished", url == null ? "" : url);
                }
            });
            if (params.trim().startsWith("<")) {
                web.loadDataWithBaseURL(null, params, "text/html", "utf-8", null);
            } else if (params.length() > 0) {
                web.loadUrl(params);
            }
            return new PlatformView() {
                @Override
                public View getView() {
                    return web;
                }

                @Override
                public void dispose() {
                    web.destroy();
                }

                @Override
                public void onMethodCall(String method, String arguments) {
                    call(web, method, arguments, channel);
                }

                @Override
                public void onPause() {
                    web.onPause();
                }

                @Override
                public void onResume() {
                    web.onResume();
                }
            };
        }

        static void call(WebView web, String method, String arguments,
                final PlatformViewChannel channel) {
            if ("loadUrl".equals(method)) {
                web.loadUrl(arguments);
            } else if ("loadHtml".equals(method)) {
                web.loadDataWithBaseURL(null, arguments, "text/html", "utf-8", null);
            } else if ("evaluateJavascript".equals(method)) {
                web.evaluateJavascript(arguments, new ValueCallback<String>() {
                    @Override
                    public void onReceiveValue(String value) {
                        if (channel != null) {
                            channel.send("javascriptResult", value == null ? "" : value);
                        }
                    }
                });
            }
        }
    }
}
