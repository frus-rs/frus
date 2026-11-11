package dev.frus.platform;

/**
 * The way from a native view back to the application: what is sent here reaches the widget's
 * {@code on_event} as a {@code PlatformViewEvent} on the next frame. Safe from any thread.
 *
 * Compiled into the bundled dex (scripts/build-input-dex.sh).
 */
public interface PlatformViewChannel {
    /** Sends {@code name} with its {@code data} (either may be empty, neither {@code null}). */
    void send(String name, String data);
}
