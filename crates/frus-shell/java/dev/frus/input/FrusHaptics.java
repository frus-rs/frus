package dev.frus.input;

import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.provider.Settings;
import android.util.Log;
import android.view.View;
import android.view.Window;

/**
 * Haptic feedback (milestone 777): the system's touch feedback, performed on the Java UI
 * thread, and the vibrator where the system has no such effect.
 *
 * <p>A pipe, not a policy: which effect for which kind is decided on the Rust side. The
 * person's touch-feedback setting is read before anything vibrates, so that the vibrator
 * standing in never vibrates what the system would have kept still.
 */
public final class FrusHaptics {
    private static final String TAG = "frus-haptics";

    /** Performed, or posted to be. */
    static final int DONE = 0;
    /** The person turned touch feedback off in the system's settings. */
    static final int OFF = 1;
    /** The manifest does not declare android.permission.VIBRATE. */
    static final int NO_PERMISSION = 2;
    /** The device has no vibrator. */
    static final int NO_VIBRATOR = 3;

    private FrusHaptics() {}

    /**
     * Performs {@code constant}, a {@code HapticFeedbackConstants} value, on the window's
     * view; vibrates for {@code fallbackMillis} if the system performs nothing for it.
     */
    public static int perform(final Activity activity, final int constant,
            final long fallbackMillis) {
        if (!touchFeedbackOn(activity)) {
            Log.d(TAG, "performHapticFeedback(" + constant + ") skipped: touch feedback is off");
            return OFF;
        }
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                Window window = activity.getWindow();
                View view = window == null ? null : window.getDecorView();
                boolean performed = view != null && view.performHapticFeedback(constant);
                if (performed) {
                    Log.d(TAG, "performHapticFeedback(" + constant + ") = true");
                } else {
                    int answer = vibrate(activity, fallbackMillis);
                    Log.d(TAG, "performHapticFeedback(" + constant + ") = false; vibrator "
                            + fallbackMillis + " ms answered " + answer);
                }
            }
        });
        return DONE;
    }

    /** Vibrates the vibrator for {@code millis}. */
    @SuppressWarnings("deprecation")
    public static int vibrate(Activity activity, long millis) {
        if (activity.checkCallingOrSelfPermission("android.permission.VIBRATE")
                != PackageManager.PERMISSION_GRANTED) {
            return NO_PERMISSION;
        }
        Vibrator vibrator = (Vibrator) activity.getSystemService(Context.VIBRATOR_SERVICE);
        if (vibrator == null || !vibrator.hasVibrator()) {
            return NO_VIBRATOR;
        }
        if (Build.VERSION.SDK_INT >= 26) {
            vibrator.vibrate(VibrationEffect.createOneShot(millis,
                    VibrationEffect.DEFAULT_AMPLITUDE));
        } else {
            vibrator.vibrate(millis);
        }
        Log.d(TAG, "vibrate(" + millis + " ms)");
        return DONE;
    }

    private static boolean touchFeedbackOn(Activity activity) {
        try {
            return Settings.System.getInt(activity.getContentResolver(),
                    Settings.System.HAPTIC_FEEDBACK_ENABLED, 1) != 0;
        } catch (RuntimeException e) {
            return true;
        }
    }
}
