package dev.frus.platform;

import android.content.Context;

/**
 * Makes the native views an application embeds with {@code AndroidView::new(viewType, params)}.
 *
 * <p>A plugin registers one factory per view type, from Rust with
 * {@code frus::register_android_view_factory(viewType, className, dex)}: the class is loaded
 * from the plugin's dex and made with its public no-argument constructor. Every method is
 * called on the UI thread.
 *
 * Compiled into the bundled dex (scripts/build-input-dex.sh).
 */
public interface PlatformViewFactory {
    /**
     * A new view for the widget that just appeared.
     *
     * @param context the activity
     * @param id the view's identity, the same for as long as the widget stays
     * @param params the creation parameters the widget was given (never {@code null})
     * @param channel the way back to the application: events its widget hears
     */
    PlatformView create(Context context, int id, String params, PlatformViewChannel channel);
}
