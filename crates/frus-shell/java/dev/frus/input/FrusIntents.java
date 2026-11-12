package dev.frus.input;

import android.app.Activity;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Parcelable;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

/**
 * What an intent that opens the application carries (milestone 801): a link, or a share from
 * another application's share sheet, read here and handed to the native side as strings.
 *
 * <p>A link: {@code {"link", data}}. A share: {@code {"share", text, subject, type, file...}},
 * each file a {@code content://} stream copied first into the cache folder's {@code shared/},
 * on a thread of its own, as only the receiving activity may read it and a video takes a while.
 */
public final class FrusIntents {
    private FrusIntents() {}

    /** Hands what an intent carried to the native side; registered by it. */
    private static native void nativeReceived(String[] fields);

    /**
     * Reads {@code intent}: a share is handed over (once its files are copied), a link too
     * unless {@code launch}, the activity's first intent, whose link the native side reads
     * itself.
     */
    public static void receive(final Activity activity, Intent intent, boolean launch) {
        if (intent == null) {
            return;
        }
        String action = intent.getAction();
        if (Intent.ACTION_SEND.equals(action) || Intent.ACTION_SEND_MULTIPLE.equals(action)) {
            final ArrayList<String> fields = new ArrayList<>();
            fields.add("share");
            fields.add(text(intent.getCharSequenceExtra(Intent.EXTRA_TEXT)));
            fields.add(text(intent.getCharSequenceExtra(Intent.EXTRA_SUBJECT)));
            fields.add(intent.getType());
            final ArrayList<Uri> streams = new ArrayList<>();
            if (Intent.ACTION_SEND.equals(action)) {
                Parcelable one = intent.getParcelableExtra(Intent.EXTRA_STREAM);
                if (one instanceof Uri) {
                    streams.add((Uri) one);
                }
            } else {
                ArrayList<Parcelable> many = intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
                if (many != null) {
                    for (Parcelable item : many) {
                        if (item instanceof Uri) {
                            streams.add((Uri) item);
                        }
                    }
                }
            }
            if (streams.isEmpty()) {
                deliver(fields);
                return;
            }
            new Thread(new Runnable() {
                @Override
                public void run() {
                    copy(activity, streams, fields);
                    deliver(fields);
                }
            }, "frus-shared-files").start();
        } else if (!launch && intent.getDataString() != null) {
            ArrayList<String> fields = new ArrayList<>();
            fields.add("link");
            fields.add(intent.getDataString());
            deliver(fields);
        }
    }

    private static void deliver(ArrayList<String> fields) {
        try {
            nativeReceived(fields.toArray(new String[0]));
        } catch (UnsatisfiedLinkError notYet) {
            // The native side has not registered: nothing can take it.
        }
    }

    private static String text(CharSequence value) {
        return value == null ? null : value.toString();
    }

    /** Copies each stream into {@code cache/shared/}; what cannot be read is left out. */
    private static void copy(Activity activity, ArrayList<Uri> streams, ArrayList<String> out) {
        File dir = new File(activity.getCacheDir(), "shared");
        dir.mkdirs();
        ContentResolver resolver = activity.getContentResolver();
        long stamp = System.currentTimeMillis();
        byte[] buffer = new byte[64 * 1024];
        for (int at = 0; at < streams.size(); at++) {
            Uri uri = streams.get(at);
            File file = new File(dir, stamp + "-" + at + "-" + safeName(name(resolver, uri)));
            try (InputStream in = resolver.openInputStream(uri);
                 OutputStream os = new FileOutputStream(file)) {
                if (in == null) {
                    continue;
                }
                int read;
                while ((read = in.read(buffer)) >= 0) {
                    os.write(buffer, 0, read);
                }
                out.add(file.getAbsolutePath());
            } catch (Exception e) {
                file.delete();
                android.util.Log.w("frus", "a shared file could not be copied: " + e);
            }
        }
    }

    /** What the sender calls the stream: its display name, or its last segment. */
    private static String name(ContentResolver resolver, Uri uri) {
        try (Cursor cursor = resolver.query(uri, new String[] {"_display_name"}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                String name = cursor.getString(0);
                if (name != null) {
                    return name;
                }
            }
        } catch (Exception ignored) {
            // Some providers answer no query: the address's last segment is the name then.
        }
        String last = uri.getLastPathSegment();
        return last == null ? "" : last;
    }

    /** A name that cannot leave its folder: the last segment, unsafe characters replaced. */
    static String safeName(String name) {
        int slash = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'));
        String last = name.substring(slash + 1);
        StringBuilder out = new StringBuilder();
        for (char c : last.toCharArray()) {
            out.append(":*?\"<>|".indexOf(c) >= 0 || Character.isISOControl(c) ? '_' : c);
        }
        String cleaned = out.toString().trim();
        while (cleaned.startsWith(".")) {
            cleaned = cleaned.substring(1);
        }
        if (cleaned.isEmpty()) {
            return "shared";
        }
        return cleaned.length() > 120 ? cleaned.substring(0, 120) : cleaned;
    }
}
