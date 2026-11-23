// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

package dev.frus.input;

import android.app.Activity;
import android.content.ComponentCallbacks2;
import android.content.res.Configuration;

/**
 * The system's memory signals (milestone 840): {@code onTrimMemory}, which the native activity
 * does not forward to native code, handed to {@link #nativeTrim} with its level.
 *
 * <p>Registered once, on the application's context, for the life of the process. The
 * low-memory callback is not forwarded from here: the native activity already passes it on.
 */
public final class FrusMemory implements ComponentCallbacks2 {
    private static FrusMemory registered;

    private FrusMemory() {}

    static native void nativeTrim(int level);

    /** Starts listening, once. */
    public static synchronized void start(Activity activity) {
        if (registered != null) {
            return;
        }
        registered = new FrusMemory();
        activity.getApplicationContext().registerComponentCallbacks(registered);
    }

    @Override
    public void onTrimMemory(int level) {
        nativeTrim(level);
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {}

    @Override
    public void onLowMemory() {}
}
