package dev.frus.platform;

import android.view.View;

/**
 * One embedded native view, made by a {@link PlatformViewFactory}.
 *
 * <p>frus sizes and places {@link #getView()} where the widget was laid out, draws it with the
 * rest of the frame, hands it the touches that land on it, and calls {@link #dispose()} when
 * the widget leaves. Every method is called on the UI thread.
 *
 * Compiled into the bundled dex (scripts/build-input-dex.sh).
 */
public interface PlatformView {
    /** The view to embed. Called once, right after the factory made this object. */
    View getView();

    /** The widget left: release what the view holds. The view is detached right after. */
    void dispose();

    /**
     * A call from the application ({@code PlatformViewController::invoke}). Nothing comes back:
     * an answer is an event sent through the {@link PlatformViewChannel}.
     */
    default void onMethodCall(String method, String arguments) {}

    /** The activity was paused (it left the screen). */
    default void onPause() {}

    /** The activity was resumed. */
    default void onResume() {}
}
