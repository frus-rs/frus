package dev.frus.input;

import android.app.Activity;
import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.os.Build;
import android.util.Log;

/**
 * Which network the device is on (milestone 777): the default network, watched with
 * {@code ConnectivityManager}'s callback, each change handed to {@link #nativeChanged}.
 *
 * <p>The kinds: 0 none, 1 Wi-Fi, 2 cellular, 3 ethernet, 4 another; -1 when the system
 * refuses (the manifest does not declare ACCESS_NETWORK_STATE).
 */
public final class FrusConnectivity {
    private static final String TAG = "frus-connectivity";

    private static ConnectivityManager.NetworkCallback callback;
    private static Network current;

    private FrusConnectivity() {}

    static native void nativeChanged(int kind);

    /** Starts watching, once, and answers the kind of network the device is on now. */
    public static synchronized int start(Activity activity) {
        ConnectivityManager manager =
                (ConnectivityManager) activity.getSystemService(Context.CONNECTIVITY_SERVICE);
        if (manager == null) {
            return -1;
        }
        try {
            Network active = manager.getActiveNetwork();
            int now = active == null ? 0 : kind(manager.getNetworkCapabilities(active));
            if (callback == null && Build.VERSION.SDK_INT >= 24) {
                callback = new ConnectivityManager.NetworkCallback() {
                    @Override
                    public void onAvailable(Network network) {
                        synchronized (FrusConnectivity.class) {
                            current = network;
                        }
                    }

                    @Override
                    public void onCapabilitiesChanged(Network network,
                            NetworkCapabilities capabilities) {
                        synchronized (FrusConnectivity.class) {
                            current = network;
                        }
                        nativeChanged(kind(capabilities));
                    }

                    @Override
                    public void onLost(Network network) {
                        boolean wasDefault;
                        synchronized (FrusConnectivity.class) {
                            wasDefault = current == null || network.equals(current);
                            if (wasDefault) {
                                current = null;
                            }
                        }
                        if (wasDefault) {
                            nativeChanged(0);
                        }
                    }
                };
                manager.registerDefaultNetworkCallback(callback);
            }
            return now;
        } catch (SecurityException e) {
            Log.w(TAG, "ACCESS_NETWORK_STATE is not declared: " + e.getMessage());
            callback = null;
            return -1;
        }
    }

    static int kind(NetworkCapabilities capabilities) {
        if (capabilities == null) {
            return 0;
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) {
            return 1;
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) {
            return 2;
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) {
            return 3;
        }
        return 4;
    }
}
