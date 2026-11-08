package dev.frus.input;

import android.app.Activity;
import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Environment;
import android.provider.MediaStore;
import android.util.Log;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileInputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

/**
 * Opening an address in the system's handler, and sharing through the system's chooser
 * (milestone 775). Called from Rust through JNI, as the other bridges are.
 *
 * <p>Every method answers a code rather than throwing: 0 done; 1 no application handles it;
 * 2 not possible on this Android version; 3 failed (the log says why).
 */
public final class FrusShare {
    private FrusShare() {}

    private static final String TAG = "frus";

    /** Opens {@code url} with {@code ACTION_VIEW}, in a task of its own. */
    public static int openUrl(Activity activity, String url) {
        try {
            Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            activity.startActivity(intent);
            return 0;
        } catch (ActivityNotFoundException e) {
            return 1;
        } catch (Exception e) {
            Log.w(TAG, "the address could not be opened", e);
            return 3;
        }
    }

    /**
     * Shows the chooser over an {@code ACTION_SEND} (or {@code ACTION_SEND_MULTIPLE}) carrying
     * {@code text}, {@code subject} and the files at {@code paths}, each null or empty when
     * absent. Another application cannot read this one's files, so each is copied to the shared
     * Downloads collection first (Android 10 and later, where that needs no permission).
     */
    public static int share(Activity activity, String text, String subject, String[] paths) {
        try {
            ArrayList<Uri> uris = new ArrayList<>();
            String type = null;
            if (paths != null && paths.length > 0) {
                if (Build.VERSION.SDK_INT < 29) {
                    Log.w(TAG, "sharing files needs Android 10 or later");
                    return 2;
                }
                for (String path : paths) {
                    File file = new File(path);
                    String mime = mimeOf(file.getName());
                    Uri uri = publish(activity, file, mime);
                    if (uri == null) {
                        return 3;
                    }
                    uris.add(uri);
                    type = type == null ? mime : common(type, mime);
                }
            }
            Intent send;
            if (uris.size() > 1) {
                send = new Intent(Intent.ACTION_SEND_MULTIPLE);
                send.putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris);
            } else {
                send = new Intent(Intent.ACTION_SEND);
                if (uris.size() == 1) {
                    send.putExtra(Intent.EXTRA_STREAM, uris.get(0));
                }
            }
            send.setType(type != null ? type : "text/plain");
            if (text != null && !text.isEmpty()) {
                send.putExtra(Intent.EXTRA_TEXT, text);
            }
            if (subject != null && !subject.isEmpty()) {
                send.putExtra(Intent.EXTRA_SUBJECT, subject);
                send.putExtra(Intent.EXTRA_TITLE, subject);
            }
            if (!uris.isEmpty()) {
                // The grant travels with the clip, through the chooser to the target.
                ClipData clip = ClipData.newRawUri("", uris.get(0));
                for (int i = 1; i < uris.size(); i++) {
                    clip.addItem(new ClipData.Item(uris.get(i)));
                }
                send.setClipData(clip);
                send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            }
            Intent chooser = Intent.createChooser(send, null);
            if (!uris.isEmpty()) {
                chooser.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            }
            activity.startActivity(chooser);
            return 0;
        } catch (ActivityNotFoundException e) {
            return 1;
        } catch (Exception e) {
            Log.w(TAG, "the share sheet could not be shown", e);
            return 3;
        }
    }

    /** The MIME type a file's extension names, or a generic one. */
    private static String mimeOf(String name) {
        int dot = name.lastIndexOf('.');
        if (dot >= 0) {
            String found = MimeTypeMap.getSingleton()
                    .getMimeTypeFromExtension(name.substring(dot + 1).toLowerCase());
            if (found != null) {
                return found;
            }
        }
        return "application/octet-stream";
    }

    /** The type that covers both: the same, the same family ({@code image/*}), or anything. */
    private static String common(String a, String b) {
        if (a.equals(b)) {
            return a;
        }
        int slashA = a.indexOf('/');
        int slashB = b.indexOf('/');
        if (slashA > 0 && slashB > 0 && a.substring(0, slashA).equals(b.substring(0, slashB))) {
            return a.substring(0, slashA) + "/*";
        }
        return "*/*";
    }

    /**
     * Copies {@code file} to Downloads/&lt;application label&gt;/, replacing an earlier copy of
     * the same name this application made, and answers its content address.
     */
    private static Uri publish(Activity activity, File file, String mime) {
        ContentResolver resolver = activity.getContentResolver();
        Uri collection = MediaStore.Downloads.EXTERNAL_CONTENT_URI;
        String folder = Environment.DIRECTORY_DOWNLOADS + "/" + folderName(activity) + "/";
        // An earlier copy of the same name: this application's own, so it may remove it.
        try (Cursor earlier = resolver.query(
                collection,
                new String[] {MediaStore.MediaColumns._ID},
                MediaStore.MediaColumns.DISPLAY_NAME + "=? AND "
                        + MediaStore.MediaColumns.RELATIVE_PATH + "=?",
                new String[] {file.getName(), folder},
                null)) {
            while (earlier != null && earlier.moveToNext()) {
                resolver.delete(Uri.withAppendedPath(collection, earlier.getString(0)), null, null);
            }
        } catch (Exception e) {
            Log.w(TAG, "an earlier shared copy could not be removed", e);
        }
        ContentValues values = new ContentValues();
        values.put(MediaStore.MediaColumns.DISPLAY_NAME, file.getName());
        values.put(MediaStore.MediaColumns.MIME_TYPE, mime);
        values.put(MediaStore.MediaColumns.RELATIVE_PATH, folder);
        values.put(MediaStore.MediaColumns.IS_PENDING, 1);
        Uri uri = resolver.insert(collection, values);
        if (uri == null) {
            Log.w(TAG, "no shared copy could be made of " + file);
            return null;
        }
        try (InputStream in = new FileInputStream(file);
                OutputStream out = resolver.openOutputStream(uri)) {
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = in.read(buffer)) > 0) {
                out.write(buffer, 0, read);
            }
        } catch (Exception e) {
            Log.w(TAG, "the shared copy of " + file + " failed", e);
            resolver.delete(uri, null, null);
            return null;
        }
        values.clear();
        values.put(MediaStore.MediaColumns.IS_PENDING, 0);
        resolver.update(uri, values, null, null);
        return uri;
    }

    /** The application's label, as a folder name. */
    private static String folderName(Activity activity) {
        CharSequence label = activity.getApplicationInfo().loadLabel(activity.getPackageManager());
        String name = label == null ? "" : label.toString().replaceAll("[\\\\/:*?\"<>|]", "").trim();
        return name.isEmpty() ? activity.getPackageName() : name;
    }
}
