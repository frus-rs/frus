package {{org}}.{{crate_name}};

import android.content.Context;
import android.os.Build;

/**
 * The plugin's Android side: what Rust calls through JNI (see src/android.rs). Add methods
 * here, as public static ones, and call them the same way.
 */
public final class {{ClassName}}Plugin {
    private {{ClassName}}Plugin() {}

    /** The platform's version, as a person reads it: "Android 10 (API 29)". */
    public static String platformVersion(Context context) {
        return "Android " + Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + ")";
    }
}
