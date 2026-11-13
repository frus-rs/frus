// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

package dev.frus;

import android.app.NativeActivity;
import android.content.Intent;

/**
 * The activity a frus application runs in on Android (milestone 801): the system's native
 * activity, plus the one thing it does not forward to native code, a new intent.
 *
 * <p>Declared with {@code launchMode="singleTop"}, the running activity is handed the intents
 * that open the application again — a link, a share, a notification's tap, a sign-in's
 * redirect that a browser sends back with {@code FLAG_ACTIVITY_CLEAR_TOP} — as
 * {@link #onNewIntent}, instead of being finished and created again. The intent becomes the
 * activity's own ({@link #setIntent}) and goes to the native side, which registers
 * {@link #nativeNewIntent} as soon as it starts. An intent that comes before that is read as
 * the activity's intent with the launch's.
 *
 * <p>{@code frus build} and {@code frus run} compile this class into the package and name it
 * in the manifest; nothing in it depends on the rest of frus's Java, which is loaded later.
 */
public class FrusActivity extends NativeActivity {
    /** Hands the intent to the native side; registered by it at startup. */
    private native void nativeNewIntent(Intent intent);

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        try {
            nativeNewIntent(intent);
        } catch (UnsatisfiedLinkError notYet) {
            // The native side has not started: it reads getIntent() when it does.
        }
    }
}
